use fat_checksum::{Checksum, HashFunction};
use fat_vfs::{
    FileSystem, Permissions, VfsFile, VfsMetadata, VfsNodeType, VfsOpenOptions, VfsReadDir,
};
use std::{fmt, io, sync::Arc};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

use self::{
    handle::{FileReadHandle, FileWriteHandle, OpenFileMode},
    inner::{ImfsDirEntry, ImfsInner, ImfsNodeOperation, ImfsWalkDir},
    node::*,
};

mod handle;
mod inner;
mod node;
mod snapshot;

pub use self::snapshot::{ImfsDirectoryBuilder, ImfsSnapshotNode};

/// An ephemeral, concurrent in-memory filesystem.
#[derive(Clone)]
pub struct InMemoryFs {
    inner: Arc<ImfsInner>,
}

impl InMemoryFs {
    /// Creates a new in-memory file system containing only the root directory.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ImfsInner::empty()),
        }
    }

    /// Gets the total active nodes of this in-memory file system.
    #[must_use]
    pub fn nodes(&self) -> usize {
        self.inner.nodes.active_slots()
    }

    /// Gets the total slots (including free slots) in this store.
    #[must_use]
    pub fn node_slots(&self) -> usize {
        self.inner.nodes.slots()
    }

    /// Sets the permissions from a specified path recursively.
    #[inline(always)]
    pub fn set_permissions_recursive(
        &self,
        path: Utf8TypedPath<'_>,
        permissions: Permissions,
    ) -> io::Result<()> {
        let path = self.inner.normalize(path)?;
        self.inner.set_permissions_recursive(&path, permissions)
    }

    /// Wipes the entire in-memory file system.
    pub fn wipe(&self) -> io::Result<()> {
        // TODO: Prevent all nodes from wiping if there are files opened.
        self.inner.nodes.clear(self.inner.root);

        // Ensuring all children are removed in the root directory
        let root = self.inner.nodes.get(self.inner.root)?;
        let root = root.as_dir()?;
        root.children.clear();

        Ok(())
    }
}

impl fmt::Debug for InMemoryFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InMemoryFs")
            .field("nodes", &self.inner.nodes)
            .field("root", &self.inner.root)
            .finish()
    }
}

impl Default for InMemoryFs {
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}

impl FileSystem for InMemoryFs {
    fn current_dir(&self) -> io::Result<Utf8TypedPathBuf> {
        // Make sure that directory exists before giving back to the user.
        let current_dir = match self.inner.current_dir.read() {
            Ok(value) => value,
            Err(inner) => inner.into_inner(),
        };

        let (node, ..) = self.inner.find_node(&current_dir)?;
        node.as_dir()?;

        Ok(Utf8TypedPathBuf::Unix(current_dir.clone()))
    }

    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = self.inner.normalize(path)?;
        self.inner.create_dir(&path)
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = self.inner.normalize(path)?;
        self.inner.create_dir_all(&path)
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        let path = self.inner.normalize(path)?;

        let (parent, ..) = self.inner.get_file_name(&path);
        let parent_id = self.inner.find_node_id(parent)?;
        self.inner.check_perms(parent_id, ImfsNodeOperation::Read)?;

        match self.inner.find_node(&path) {
            Ok(..) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn hash(
        &self,
        path: Utf8TypedPath<'_>,
        mut hasher: Box<dyn HashFunction>,
    ) -> io::Result<Checksum> {
        let target = self.inner.normalize(path)?;
        let (node, node_id) = self.inner.find_node(&target)?;

        let content = node.as_file()?.read();
        self.inner.check_perms(node_id, ImfsNodeOperation::Read)?;

        hasher.update(&content);
        Ok(hasher.digest())
    }

    fn join(&self, path: Utf8TypedPath<'_>, name: &str) -> io::Result<Utf8TypedPathBuf> {
        self.inner
            .normalize(path)?
            .join_checked(name)
            .map_err(io::Error::other)
            .map(Utf8TypedPathBuf::Unix)
    }

    fn metadata(&self, target: Utf8TypedPath<'_>) -> io::Result<VfsMetadata> {
        let target = self.inner.normalize(target)?;

        let (parent, ..) = self.inner.get_file_name(&target);
        let parent_id = self.inner.find_node_id(parent)?;
        self.inner.check_perms(parent_id, ImfsNodeOperation::Read)?;

        let target_node_id = self.inner.find_node_id(&target)?;
        self.inner.metadata(target_node_id)
    }

    fn open(&self, path: Utf8TypedPath<'_>, options: &mut VfsOpenOptions) -> io::Result<VfsFile> {
        let path = self.inner.normalize(path)?;
        let node_id = match self.inner.find_node_id(&path) {
            Ok(..) if options.create_new => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "file already exists",
                ));
            }
            Ok(id) => id,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !options.create && !options.create_new {
                    return Err(error);
                }

                let (parent_path, name) = self.inner.require_file_name(&path)?;
                let (parent, parent_id) = self.inner.find_node(parent_path)?;

                let parent = parent.as_dir()?;
                self.inner
                    .check_perms(parent_id, ImfsNodeOperation::Write)?;

                let node = Node::empty_file();
                self.inner.add_node(parent, parent_id, name, node)?
            }
            Err(error) => return Err(error),
        };

        if options.truncate && !options.write {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attempt to truncate file with no write access",
            ));
        }

        if (options.create || options.create_new) && !(options.write || options.append) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attempt to create file with no write access",
            ));
        }

        let mode = if options.read {
            self.inner.check_perms(node_id, ImfsNodeOperation::Read)?;
            OpenFileMode::Read
        } else if options.write || options.append {
            self.inner.check_perms(node_id, ImfsNodeOperation::Write)?;
            OpenFileMode::Write
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown open access",
            ));
        };

        let node = self.inner.nodes.get(node_id)?;
        node.as_file()?;

        let handle: VfsFile = match mode {
            OpenFileMode::Read => Box::new(FileReadHandle::new(self.inner.clone(), node_id)?),
            OpenFileMode::Write => {
                Box::new(FileWriteHandle::new(self.inner.clone(), options, node_id)?)
            }
        };

        Ok(handle)
    }

    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>> {
        let path = self.inner.normalize(path)?;
        let (node, node_id) = self.inner.find_node(&path)?;
        self.inner.check_perms(node_id, ImfsNodeOperation::Read)?;

        let file = node.as_file()?;
        let content = file.read();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        Ok(vec)
    }

    fn read_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<VfsReadDir> {
        let path = self.inner.normalize(path)?;
        let (node, node_id) = self.inner.find_node(&path)?;

        let directory = node.as_dir()?;
        self.inner.check_perms(node_id, ImfsNodeOperation::Read)?;

        let mut entries = Vec::new();
        entries.try_reserve(directory.children.len())?;

        for entry in directory.children.iter() {
            let entry_id = *entry.value();
            let entry = self
                .inner
                .check_perms(entry_id, ImfsNodeOperation::Read)
                .map(|_| ImfsDirEntry::base(self.inner.clone(), path.join(entry.key())));

            entries.push(entry);
        }

        Ok(Box::new(entries.into_iter()))
    }

    fn read_to_string(&self, path: Utf8TypedPath<'_>) -> io::Result<String> {
        let bytes = self.read(path)?;
        match String::from_utf8(bytes) {
            Ok(content) => Ok(content),
            Err(error) => Err(io::Error::new(io::ErrorKind::InvalidData, error)),
        }
    }

    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()> {
        let from = self.inner.normalize(from)?;
        let to = self.inner.normalize(to)?;

        if from == to {
            return Ok(());
        }

        // On Unix, if `from` is not a directory, `to` must also be not a directory
        let (from_parent, from_name) = self.inner.require_file_name(&from)?;
        let (from_node, from_id) = self.inner.find_node(&from)?;

        let from_node_ty = from_node.ty();
        match from_node_ty {
            // Renames a file or directory to a new name, replacing the original
            // file if to already exists.
            VfsNodeType::File => return self.inner.rename_file(&from, &to),
            VfsNodeType::Symlink => todo!(),
            VfsNodeType::Unknown => unreachable!(),
            VfsNodeType::Directory => {}
        };

        // Make sure either the target directory is empty or not exists.
        let (to_node, to_id) = match self.inner.find_node(&to) {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Create a new directory and do stuff afterwards :D
                let (to_parent_path, name) = self.inner.require_file_name(&to)?;
                let (to_parent_node, to_parent_id) = self.inner.find_node(to_parent_path)?;

                let to_parent = to_parent_node.as_dir()?;
                let node = Node::empty_dir();

                self.inner.add_node(to_parent, to_parent_id, name, node)?;
                self.inner.find_node(&to)?
            }
            Err(error) => return Err(error),
        };

        // Pretty efficient way to transfer one place to another.
        let to_dir = to_node.as_dir()?;
        if !to_dir.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                "target directory not empty",
            ));
        }

        self.inner.check_perms(to_id, ImfsNodeOperation::Write)?;
        self.inner.check_perms(from_id, ImfsNodeOperation::Read)?;

        self.inner.nodes.remove(to_id)?;

        let (from_parent_node, ..) = self.inner.find_node(from_parent)?;
        from_parent_node.as_dir()?.remove(from_name);

        let (to_parent, to_name) = self.inner.require_file_name(&to)?;
        let (to_parent_node, ..) = self.inner.find_node(to_parent)?;
        to_parent_node.as_dir()?.add(to_name, from_id);

        Ok(())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let target = self.inner.normalize(path)?;
        let (target_node, ..) = self.inner.find_node(&target)?;

        let target_dir = target_node.as_dir()?;
        if !target_dir.children.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "directory not empty",
            ));
        }

        self.inner.remove_dir(&target)
    }

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let target = self.inner.normalize(path)?;
        self.inner.remove_dir(&target)
    }

    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let target = self.inner.normalize(path)?;
        self.inner.remove_file(&target)
    }

    fn set_current_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let Ok(mut pointer) = self.inner.current_dir.write() else {
            return Err(io::Error::new(
                io::ErrorKind::Deadlock,
                "current directory value got poisoned",
            ));
        };

        let target = self.inner.normalize(path)?;
        let (node, ..) = self.inner.find_node(&target)?;
        node.as_dir()?;

        *pointer = target;
        Ok(())
    }

    fn soft_link(&self, _original: Utf8TypedPath<'_>, _link: Utf8TypedPath<'_>) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symbolic links are not supported in InMemoryFs (yet)",
        ))
    }

    fn walkdir(&self, root: Utf8TypedPath<'_>) -> io::Result<VfsReadDir> {
        let iter = ImfsWalkDir::new(&self.inner, root)?;
        Ok(Box::new(iter))
    }

    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()> {
        let path = self.inner.normalize(path)?;
        self.inner.write(&path, contents)
    }
}

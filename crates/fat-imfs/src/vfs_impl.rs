use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, NodeType, OpenOptions, Permissions, VfsFileStream};
use std::io;
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf, constants::unix::SEPARATOR_STR};

use crate::{
    InMemoryFs,
    handle::{FileReadHandle, FileWriteHandle, OpenFileMode},
    node::{Node, NodePermissions},
};

impl FileSystem for InMemoryFs {
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        self.create_dir_inner(&path)
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let mut current = self.inner.root;
        for component in path.iter() {
            if component == SEPARATOR_STR {
                continue;
            }

            let node = self.inner.nodes.get(current)?;
            let directory = node.as_dir()?;
            self.inner.check_for_write(current, NodeType::Directory)?;

            if let Some(child_id) = directory.get(component) {
                current = child_id;
                continue;
            }

            let node = Node::empty_dir();
            let child_id = self
                .inner
                .insert_node(component, directory, current, node)?;

            current = child_id;
        }

        Ok(())
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        let path = Self::normalize(path)?;
        let (parent, ..) = Self::split_parent(&path)?;
        let parent_node_id = self.inner.find_node_id(parent)?;

        self.inner
            .check_for_read(parent_node_id, NodeType::Directory)?;

        match self.inner.find_node_id(&path) {
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
        let path = Self::normalize(path)?;
        let (node, node_id) = self.inner.find_node(&path)?;

        let content = node.as_file()?.read();
        self.inner.check_for_read(node_id, NodeType::File)?;
        hasher.update(&content);

        Ok(hasher.digest())
    }

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<Metadata> {
        let path = Self::normalize(path)?;

        // Making sure its parent has the permission to read its metadata.
        if let Some(parent) = path.parent() {
            let (node, node_id) = self.inner.find_node(&parent)?;
            self.inner.check_for_read(node_id, node.ty())?;
        }

        let node_id = self.inner.find_node_id(&path)?;
        self.inner.metadata_from_node(node_id)
    }

    fn open(
        &self,
        path: Utf8TypedPath<'_>,
        options: &mut OpenOptions,
    ) -> io::Result<Box<dyn VfsFileStream>> {
        let path = Self::normalize(path)?;
        let node_id = match self.inner.find_node_id(&path) {
            Ok(id) if options.create_new => {
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

                let (parent_path, name) = Self::split_parent(&path)?;
                let (parent, parent_id) = self.inner.find_node(parent_path)?;

                let parent = parent.as_dir()?;
                self.inner.check_for_write(parent_id, NodeType::Directory)?;

                let node = Node::empty_file();
                self.inner.insert_node(name, parent, parent_id, node)?
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
            self.inner.check_for_read(node_id, NodeType::File)?;
            OpenFileMode::Read
        } else if options.write || options.append {
            self.inner.check_for_write(node_id, NodeType::File)?;
            OpenFileMode::Write
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown open access",
            ));
        };

        let node = self.inner.nodes.get(node_id)?;
        node.as_file()?;

        let handle: Box<dyn VfsFileStream> = match mode {
            OpenFileMode::Read => Box::new(FileReadHandle::new(self.inner.clone(), node_id)?),
            OpenFileMode::Write => {
                Box::new(FileWriteHandle::new(self.inner.clone(), options, node_id)?)
            }
        };

        Ok(handle)
    }

    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>> {
        let path = Self::normalize(path)?;
        let (node, node_id) = self.inner.find_node(&path)?;
        self.inner.check_for_read(node_id, NodeType::File)?;

        let file = node.as_file()?;
        let content = file.read();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        Ok(vec)
    }

    fn read_dir(
        &self,
        path: Utf8TypedPath<'_>,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<Utf8TypedPathBuf>>>> {
        let path = Self::normalize(path)?;
        let (node, node_id) = self.inner.find_node(&path)?;
        self.inner.check_for_read(node_id, NodeType::Directory)?;

        let mut entries = Vec::new();
        let directory = node.as_dir()?;

        for entry in directory.children.iter() {
            let name = &*entry.key();
            let entry_node_id = *entry.value();

            let entry = self
                .inner
                .check_for_read(entry_node_id, NodeType::File)
                .map(|_| Utf8TypedPath::derive(&path).join(name));

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
        let from = Self::normalize(from)?;
        let to = Self::normalize(to)?;

        if from == to {
            return Ok(());
        }

        // On Unix, if `from` is not a directory, `to` must also be not a directory
        let (from_parent, from_name) = Self::split_parent(&from)?;
        let (from_node, from_id) = self.inner.find_node(&from)?;

        let from_node_ty = from_node.ty();
        match from_node_ty {
            // Renames a file or directory to a new name, replacing the original
            // file if to already exists.
            NodeType::File => return self.inner.rename_file(&from, &to),
            NodeType::Symlink => todo!(),
            NodeType::Unknown => unreachable!(),
            NodeType::Directory => {}
        };

        // Make sure either the target directory is empty or not exists.
        let (to_node, to_id) = match self.inner.find_node(&to) {
            Ok(entry) => entry,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // Create a new directory and do stuff afterwards :D
                let (to_parent_path, name) = Self::split_parent(&to)?;
                let (to_parent_node, to_parent_id) = self.inner.find_node(to_parent_path)?;

                let to_parent_dir = to_parent_node.as_dir()?;
                let node = Node::empty_dir();

                self.inner
                    .insert_node(name, to_parent_dir, to_parent_id, node)?;

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

        self.inner.check_for_write(to_id, NodeType::Directory)?;
        self.inner.check_for_read(from_id, NodeType::Directory)?;

        self.inner.nodes.remove(to_id)?;

        let (from_parent_node, ..) = self.inner.find_node(from_parent)?;
        from_parent_node.as_dir()?.remove(from_name);

        let (to_parent, to_name) = Self::split_parent(&to)?;
        let (to_parent_node, ..) = self.inner.find_node(to_parent)?;
        to_parent_node.as_dir()?.add(to_name, from_id);

        Ok(())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let node_id = self.inner.find_node_id(&path)?;
        if node_id == self.inner.root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove root directory",
            ));
        }

        let node_ref = self.inner.nodes.get(node_id)?;
        let node = node_ref.as_dir()?;
        if !node.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "directory not empty",
            ));
        }

        let (parent_path, name) = Self::split_parent(&path)?;
        let (parent_node, parent_id) = self.inner.find_node(parent_path)?;
        let parent_node = parent_node.as_dir()?;

        self.inner.check_for_write(parent_id, NodeType::Directory)?;
        self.inner.check_for_write(node_id, NodeType::Directory)?;

        parent_node.remove(name);
        self.inner.nodes.remove(node_id)?;

        Ok(())
    }

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let node_id = self.inner.find_node_id(&path)?;

        if node_id == self.inner.root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove root directory",
            ));
        }

        let (parent_path, name) = Self::split_parent(&path)?;
        let (parent_node, parent_id) = self.inner.find_node(parent_path)?;
        let parent = parent_node.as_dir()?;

        self.inner.check_for_write(parent_id, NodeType::Directory)?;
        self.inner.remove_node(node_id)?;
        parent.remove(name);

        Ok(())
    }

    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        self.inner.remove_file(&path)
    }

    fn set_permissions(&self, path: Utf8TypedPath<'_>, permissions: Permissions) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let (node, ..) = self.inner.find_node(&path)?;

        // Bypass the permission checks for the root directory
        if path.as_str() == "/" {
            node.set_permissions(NodePermissions::Set(permissions));
            return Ok(());
        }

        // Maybe try to find the parent path to ensure that this node has the permission to set it.
        let (parent, ..) = Self::split_parent(&path)?;
        let parent_id = self.inner.find_node_id(parent)?;

        if !self
            .inner
            .resolve_permissions(parent_id)?
            .contains(Permissions::WRITE)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot set permissions on a read-only directory",
            ));
        }

        node.set_permissions(NodePermissions::Set(permissions));
        Ok(())
    }

    fn soft_link(&self, _original: Utf8TypedPath<'_>, _link: Utf8TypedPath<'_>) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symbolic links are not supported in InMemoryFs",
        ))
    }

    // Unlike other platfoms, we'll throw an error if a full directory path does not exist.
    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()> {
        let path = Self::normalize(path)?;
        self.write_inner(&path, contents)
    }
}

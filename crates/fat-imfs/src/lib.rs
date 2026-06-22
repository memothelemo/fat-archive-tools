use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, OpenOptions, Permissions, VfsFileStream};
use std::sync::Arc;
use std::{fmt, io};
use typed_path::constants::unix::SEPARATOR_STR;
use typed_path::{Utf8TypedPath, Utf8UnixComponent, Utf8UnixPath, Utf8UnixPathBuf};

mod node;
mod store;

use self::{node::*, store::*};

/// An ephemeral, concurrent in-memory filesystem.
#[derive(Clone)]
pub struct InMemoryFS(Arc<ImfsInner>);

impl InMemoryFS {
    /// Creates a new in-memory filesystem containing only the root directory.
    #[must_use]
    pub fn new() -> Self {
        let nodes = ImfsNodeStore::new();
        let root = nodes
            .insert(Node::empty_dir())
            .expect("node ids should not be exhausted after ImfsNodeStore::new");

        Self(Arc::new(ImfsInner { nodes, root }))
    }
}

impl FileSystem for InMemoryFS {
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let (parent_path, name) = Self::split_parent(&path)?;

        let parent_id = self.find_node_id(parent_path)?;
        let parent_node = self.0.nodes.get(parent_id)?;
        let parent = parent_node.as_dir()?;

        if parent.contains(name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "specified path already exists",
            ));
        }

        let child_id = self.0.nodes.insert(Node::empty_dir())?;
        parent.add(name, child_id);

        Ok(())
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let mut current = self.0.root;
        for component in path.iter() {
            if component == SEPARATOR_STR {
                continue;
            }

            let node = self.0.nodes.get(current)?;
            let directory = node.as_dir()?;

            if let Some(child_id) = directory.get(component) {
                current = child_id;
                continue;
            }

            let child_id = self.0.nodes.insert(Node::empty_dir())?;
            directory.add(component, child_id);
            current = child_id;
        }

        Ok(())
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        let path = Self::normalize(path)?;
        self.find_node_id(&path).optional().map(|v| v.is_some())
    }

    fn hash(
        &self,
        _path: Utf8TypedPath<'_>,
        _hasher: Box<dyn HashFunction>,
    ) -> io::Result<Checksum> {
        todo!()
    }

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<Metadata> {
        let path = Self::normalize(path)?;
        let node_id = self.find_node_id(&path)?;
        Ok(self.0.nodes.get(node_id)?.metadata())
    }

    fn open(
        &self,
        _path: Utf8TypedPath<'_>,
        _options: &mut OpenOptions,
    ) -> io::Result<Box<dyn VfsFileStream>> {
        todo!()
    }

    fn rename(&self, _from: Utf8TypedPath<'_>, _to: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let node_id = self.find_node_id(&path)?;
        let node_ref = self.0.nodes.get(node_id)?;
        let node = node_ref.as_dir()?;
        if !node.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "directory not empty",
            ));
        }

        let (parent_path, name) = Self::split_parent(&path)?;
        let parent_id = self.find_node_id(parent_path)?;
        let parent_node_ref = self.0.nodes.get(parent_id)?;
        let parent_node = parent_node_ref.as_dir()?;
        parent_node.remove(name);

        self.0.nodes.remove(node_id)?;

        Ok(())
    }

    fn remove_dir_all(&self, _path: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
    }

    fn set_permissions(
        &self,
        _path: Utf8TypedPath<'_>,
        _permissions: Permissions,
    ) -> io::Result<()> {
        todo!()
    }

    fn soft_link(&self, _original: Utf8TypedPath<'_>, _link: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
    }
}

impl InMemoryFS {
    fn find_node_id(&self, path: &Utf8UnixPath) -> io::Result<NodeId> {
        let mut current = self.0.root;
        for component in path.components() {
            let name = match component {
                Utf8UnixComponent::RootDir => {
                    current = self.0.root;
                    continue;
                }
                Utf8UnixComponent::Normal(name) => name,

                // other variants should be eliminated by normalize.
                _ => continue,
            };

            let node = self.0.nodes.get(current)?;
            current = node.as_dir()?.get(name).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "specified path not found")
            })?;
        }
        Ok(current)
    }

    /// Normalizes a [`Utf8TypedPath`] (either on Unix or Windows) to an
    /// absolute Unix path, rejecting relative and invalid paths.
    fn normalize(path: Utf8TypedPath<'_>) -> io::Result<Utf8UnixPathBuf> {
        let path = match path {
            Utf8TypedPath::Unix(p) => p.normalize(),
            Utf8TypedPath::Windows(p) => p.with_unix_encoding().normalize(),
        };

        if path.is_relative() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "relative paths are prohibited (use absolute paths instead)",
            ));
        }

        if !path.is_valid() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Unix path",
            ));
        }

        Ok(path)
    }

    fn split_parent(path: &Utf8UnixPath) -> io::Result<(&Utf8UnixPath, &str)> {
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;

        let parent = path.parent().unwrap_or_else(|| Utf8UnixPath::new("/"));
        Ok((parent, name))
    }
}

struct ImfsInner {
    /// Storage medium for all in-memory file system nodes.
    nodes: ImfsNodeStore,

    /// Handle to the root directory.
    root: NodeId,
}

impl Default for InMemoryFS {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InMemoryFS {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InMemoryFS")
            .field("nodes", &self.0.nodes)
            .field("root", &self.0.root)
            .finish()
    }
}

trait IoResultExt<T> {
    fn optional(self) -> io::Result<Option<T>>;
}

impl<T> IoResultExt<T> for io::Result<T> {
    fn optional(self) -> io::Result<Option<T>> {
        match self {
            Ok(okay) => Ok(Some(okay)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

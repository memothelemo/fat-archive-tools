use std::sync::Arc;
use std::{fmt, io};
use typed_path::{Utf8TypedPath, Utf8UnixComponent, Utf8UnixPath, Utf8UnixPathBuf};

mod handle;
mod node;
mod vfs_impl;

use self::node::*;

/// An ephemeral, concurrent in-memory filesystem.
#[derive(Clone)]
pub struct InMemoryFs(Arc<ImfsInner>);

impl InMemoryFs {
    /// Creates a new in-memory filesystem containing only the root directory.
    #[must_use]
    pub fn new() -> Self {
        let nodes = NodeStore::new();
        let root = nodes
            .insert(Node::empty_dir())
            .expect("node ids should not be exhausted after ImfsNodeStore::new");

        Self(Arc::new(ImfsInner { nodes, root }))
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

impl ImfsInner {
    /// Finds a node from a presumably normalized path.
    #[inline(always)]
    fn find_node(&self, path: &Utf8UnixPath) -> io::Result<Arc<Node>> {
        let node_id = self.find_node_id(path)?;
        self.nodes.get(node_id)
    }

    /// Finds an assigned node id from a presumably normalized path.
    fn find_node_id(&self, path: &Utf8UnixPath) -> io::Result<NodeId> {
        let mut current = self.root;

        for component in path.components() {
            let name = match component {
                Utf8UnixComponent::RootDir => {
                    current = self.root;
                    continue;
                }
                Utf8UnixComponent::Normal(name) => name,

                // other variants should be eliminated by normalize.
                _ => continue,
            };

            let node = self.nodes.get(current)?;
            current = node.as_dir()?.get(name).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "specified path not found")
            })?;
        }

        Ok(current)
    }
}

impl Default for InMemoryFs {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InMemoryFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InMemoryFS")
            .field("nodes", &self.nodes)
            .field("root", &self.root)
            .finish()
    }
}

impl std::ops::Deref for InMemoryFs {
    type Target = ImfsInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

mod private {
    use crate::node::{NodeId, NodeStore};

    pub struct ImfsInner {
        /// Storage medium for all in-memory file system nodes.
        pub(crate) nodes: NodeStore,

        /// Handle to the root directory.
        pub(crate) root: NodeId,
    }
}
use self::private::ImfsInner;

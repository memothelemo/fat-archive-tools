use fat_vfs::{NodeType, Permissions};
use std::collections::VecDeque;
use std::sync::Arc;
use std::{fmt, io};
use typed_path::{Utf8TypedPath, Utf8UnixComponent, Utf8UnixPath, Utf8UnixPathBuf};

mod handle;
mod node;
mod vfs_impl;

use self::node::*;

/// An ephemeral, concurrent in-memory filesystem.
#[derive(Clone)]
pub struct InMemoryFs {
    inner: Arc<ImfsInner>,
}

impl InMemoryFs {
    /// Creates a new in-memory filesystem containing only the root directory.
    #[must_use]
    pub fn new() -> Self {
        let nodes = NodeStore::new();
        let root = nodes
            .insert(Node::empty_dir())
            .expect("node ids should not be exhausted after ImfsNodeStore::new");

        Self {
            inner: Arc::new(ImfsInner { nodes, root }),
        }
    }

    /// Sets the permissions from a specified path recursively.
    pub fn set_permissions_recursive(
        &self,
        path: Utf8TypedPath<'_>,
        permissions: Permissions,
    ) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let node_id = self.inner.find_node_id(&path)?;

        // Set all of the descendants' permission to inherited.
        let mut stack = VecDeque::new();
        let mut first = true;
        stack.push_back(node_id);

        while let Some(node_id) = stack.pop_front() {
            let Ok(node) = self.inner.nodes.get(node_id) else {
                continue;
            };

            match &*node {
                Node::Directory(dir) => {
                    for entry in dir.children.iter() {
                        stack.push_back(*entry.value());
                    }
                }
                Node::File(..) => {}
            };

            if !first {
                node.set_permissions(NodePermissions::Inherited)
            }
            first = false;
        }

        self.inner
            .nodes
            .get(node_id)?
            .set_permissions(NodePermissions::Set(permissions));

        Ok(())
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
    /// Checks whether this node can be written.
    fn check_for_read(&self, node_id: NodeId, node_ty: NodeType) -> io::Result<Permissions> {
        let permissions = self.resolve_permissions(node_id)?;
        if permissions.contains(Permissions::READ) {
            Ok(permissions)
        } else {
            let message = match node_ty {
                NodeType::Directory => "could not access directory",
                NodeType::File => "could not access file",
                NodeType::Symlink => "could not access symlink",
                NodeType::Unknown => "could not access unknown node",
            };
            Err(io::Error::new(io::ErrorKind::PermissionDenied, message))
        }
    }

    /// Checks whether this node can be written.
    fn check_for_write(&self, node_id: NodeId, node_ty: NodeType) -> io::Result<Permissions> {
        let permissions = self.resolve_permissions(node_id)?;
        if permissions.contains(Permissions::WRITE) {
            Ok(permissions)
        } else {
            let message = match node_ty {
                NodeType::Directory => "attempt to access a read-only directory",
                NodeType::File => "attempt to access a read-only file",
                NodeType::Symlink => "attempt to access a read-only symlink",
                NodeType::Unknown => "attempt to access a read-only unknown node",
            };
            Err(io::Error::new(io::ErrorKind::PermissionDenied, message))
        }
    }

    /// Resolves permissions from a specified node based on its ancestors.
    fn resolve_permissions(&self, node_id: NodeId) -> io::Result<Permissions> {
        let mut current = Some(node_id);

        while let Some(node_id) = current {
            let node = self.nodes.get(node_id)?;
            let permissions = node.permissions();

            // Move to the next parent to resolve more
            if let NodePermissions::Set(value) = permissions {
                return Ok(value);
            }

            current = node.parent();
        }

        // We can safely assume that this node has READ_WRITE permission.
        Ok(Permissions::READ_WRITE)
    }

    /// Finds a node from a presumably normalized path.
    #[inline(always)]
    fn find_node(&self, path: &Utf8UnixPath) -> io::Result<(Arc<Node>, NodeId)> {
        let node_id = self.find_node_id(path)?;
        match self.nodes.get(node_id) {
            Ok(node) => Ok((node, node_id)),
            Err(error) => Err(error),
        }
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

    /// Inserts a new node into the node store with required fields.
    ///
    /// ## Caution
    ///
    /// This function assumes that the parent is not referencing back
    /// to the tree that it may be cyclic.
    fn insert_node(
        &self,
        name: &str,
        parent: &DirectoryNode,
        parent_id: NodeId,
        node: Node,
    ) -> io::Result<NodeId> {
        node.set_parent(parent_id);

        let node_id = self.nodes.insert(node)?;
        parent.add(name, node_id);

        Ok(node_id)
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
            .field("nodes", &self.inner.nodes)
            .field("root", &self.inner.root)
            .finish()
    }
}

struct ImfsInner {
    /// Storage medium for all in-memory file system nodes.
    nodes: NodeStore,

    /// Handle to the root directory.
    root: NodeId,
}

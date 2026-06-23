use fat_vfs::{Metadata, NodeType, Permissions, VfsSnapshotNode};
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

    pub fn apply_snapshot(&self, snapshot: &VfsSnapshotNode) -> io::Result<()> {
        let VfsSnapshotNode::Directory {
            children,
            permissions,
        } = snapshot
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "snapshot node should be a directory",
            ));
        };

        let root_id = self.inner.root;
        let root = self.inner.nodes.get(root_id)?;

        self.inner.check_for_write(root_id, NodeType::Directory)?;
        root.set_permissions(NodePermissions::Set(*permissions));

        for (name, node) in children.iter() {
            let path = Utf8UnixPath::new("/")
                .join_checked(name)
                .map_err(io::Error::other)?;

            self.apply_snapshot_inner(&path, node)?;
        }

        Ok(())
    }

    /// Sets the permissions from a specified path recursively.
    #[inline(always)]
    pub fn set_permissions_recursive(
        &self,
        path: Utf8TypedPath<'_>,
        permissions: Permissions,
    ) -> io::Result<()> {
        self.inner.set_permissions_recursive(path, permissions)
    }
}

impl InMemoryFs {
    /// Applies [`VfsSnapshotNode`] to a target path.
    fn apply_snapshot_inner(&self, path: &Utf8UnixPath, node: &VfsSnapshotNode) -> io::Result<()> {
        let permissions = match node {
            VfsSnapshotNode::File {
                data: contents,
                permissions,
            } => {
                self.write_inner(path, contents)?;
                permissions
            }
            VfsSnapshotNode::Directory {
                children,
                permissions,
            } => {
                self.create_dir_inner(path)?;
                for (name, node) in children.iter() {
                    let path = path.join_checked(name).map_err(io::Error::other)?;
                    self.apply_snapshot_inner(&path, node)?;
                }
                permissions
            }
        };

        let (node, ..) = self.inner.find_node(path)?;
        node.set_permissions(NodePermissions::Set(*permissions));

        Ok(())
    }

    /// Inner implementation of [`InMemoryFs::create_dir`]
    #[inline(always)]
    fn create_dir_inner(&self, path: &Utf8UnixPath) -> io::Result<()> {
        let (parent_path, name) = Self::split_parent(&path)?;

        let (parent, parent_id) = self.inner.find_node(parent_path)?;
        let parent = parent.as_dir()?;
        self.inner.check_for_write(parent_id, NodeType::Directory)?;

        if parent.contains(name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "specified path already exists",
            ));
        }

        let node = Node::empty_dir();
        self.inner.insert_node(name, parent, parent_id, node)?;

        Ok(())
    }

    /// Inner implementation of [`InMemoryFs::write`].
    #[inline(always)]
    fn write_inner(&self, path: &Utf8UnixPath, contents: &[u8]) -> io::Result<()> {
        let (parent, name) = Self::split_parent(&path)?;

        // Make sure the full directory path does exist.
        let (parent, parent_id) = self.inner.find_node(parent)?;
        let parent = parent.as_dir()?;
        self.inner.check_for_write(parent_id, NodeType::Directory)?;

        // Overwrite the entire contents with the help of file node.
        let child_node_id = if let Some(id) = parent.get(name) {
            id
        } else {
            let node = Node::empty_file();
            self.inner.insert_node(name, parent, parent_id, node)?
        };

        // Do we have the permission to overwrite the file?
        let child_node = self.inner.nodes.get(child_node_id)?;
        self.inner.check_for_write(child_node_id, NodeType::File)?;

        let file = child_node.as_file()?;
        file.replace(contents)?;

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
        if !permissions.contains(Permissions::READ) {
            let message = match node_ty {
                NodeType::Directory => "could not access directory",
                NodeType::File => "could not access file",
                NodeType::Symlink => "could not access symlink",
                NodeType::Unknown => "could not access unknown node",
            };
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, message));
        }
        Ok(permissions)
    }

    /// Checks whether this node can be written.
    fn check_for_write(&self, node_id: NodeId, node_ty: NodeType) -> io::Result<Permissions> {
        let permissions = self.resolve_permissions(node_id)?;
        if !permissions.contains(Permissions::WRITE) {
            let message = match node_ty {
                NodeType::Directory => "attempt to access a read-only directory",
                NodeType::File => "attempt to access a read-only file",
                NodeType::Symlink => "attempt to access a read-only symlink",
                NodeType::Unknown => "attempt to access a read-only unknown node",
            };
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, message));
        }
        Ok(permissions)
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

    /// Attempts to get file system node metadata from a node ID.
    fn metadata_from_node(&self, node_id: NodeId) -> io::Result<Metadata> {
        let node = self.nodes.get(node_id)?;
        let permissions = self.resolve_permissions(node_id)?;

        Ok(Metadata {
            mode: permissions,
            size: match &*node {
                Node::Directory(..) => 0,
                Node::File(file) => file.read().len() as u64,
            },
            ty: node.ty(),
        })
    }

    /// Renames a file to a new name, replacing the original file if `to` already exists.
    fn rename_file(&self, from: &Utf8UnixPath, to: &Utf8UnixPath) -> io::Result<()> {
        if from == to {
            return Ok(());
        }

        let (from_node, from_node_id) = self.find_node(from)?;
        let from_file = from_node.as_file()?;

        match self.find_node(to) {
            Ok((to, to_node_id)) => {
                self.check_for_read(from_node_id, NodeType::File)?;
                self.check_for_write(to_node_id, NodeType::File)?;

                to.as_file()?.replace(&from_file.read())?;
                self.remove_file(from)?;

                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        };

        let (from_parent, from_file_name) = InMemoryFs::split_parent(from)?;
        let (from_parent_node, from_parent_id) = self.find_node(from_parent)?;

        let (to_parent, to_file_name) = InMemoryFs::split_parent(to)?;
        let (to_parent_node, to_parent_id) = self.find_node(to_parent)?;

        let from_parent_dir = from_parent_node.as_dir()?;
        let to_parent_dir = to_parent_node.as_dir()?;

        self.check_for_read(from_node_id, NodeType::File)?;
        self.check_for_write(from_parent_id, NodeType::Directory)?;
        self.check_for_write(to_parent_id, NodeType::Directory)?;

        let node = Node::empty_file();
        node.as_file()?.replace(&from_file.read())?;

        self.insert_node(to_file_name, to_parent_dir, to_parent_id, node)?;
        from_parent_dir.remove(from_file_name);
        self.nodes.remove(from_node_id)?;

        Ok(())
    }

    /// Removes a file from a specified path.
    fn remove_file(&self, path: &Utf8UnixPath) -> io::Result<()> {
        let (parent, name) = InMemoryFs::split_parent(&path)?;
        let (parent, parent_id) = self.find_node(&parent)?;

        let directory = parent.as_dir()?;
        let (file, file_id) = self.find_node(&path)?;
        file.as_file()?;

        self.check_for_write(file_id, NodeType::Directory)?;
        self.check_for_write(parent_id, NodeType::Directory)?;

        directory.remove(name);
        self.nodes.remove(file_id)?;

        Ok(())
    }

    /// Removes node recursively along with its children.
    fn remove_node(&self, id: NodeId) -> io::Result<()> {
        let mut to_visit = vec![id];
        let mut to_delete = Vec::new();

        while let Some(current_id) = to_visit.pop() {
            to_delete.push(current_id);
            let node = self.nodes.get(current_id)?;
            self.check_for_write(current_id, node.ty())?;

            if let Node::Directory(dir) = &*node {
                for entry in dir.children.iter() {
                    to_visit.push(*entry.value());
                }
            }
        }

        for current_id in to_delete.into_iter().rev() {
            self.nodes.remove(current_id)?;
        }

        Ok(())
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

    /// Sets the permissions from a specified path recursively.
    fn set_permissions_recursive(
        &self,
        path: Utf8TypedPath<'_>,
        permissions: Permissions,
    ) -> io::Result<()> {
        let path = InMemoryFs::normalize(path)?;
        let node_id = self.find_node_id(&path)?;

        // Bypass check if it is root
        if path.as_str() != "/" {
            let (parent_path, ..) = InMemoryFs::split_parent(&path)?;
            let parent_id = self.find_node_id(parent_path)?;
            if !self
                .resolve_permissions(parent_id)?
                .contains(Permissions::WRITE)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "cannot set permissions on a read-only directory",
                ));
            }
        }

        // Set all of the descendants' permission to inherited.
        let mut stack = VecDeque::new();
        let mut first = true;
        stack.push_back(node_id);

        while let Some(node_id) = stack.pop_front() {
            let Ok(node) = self.nodes.get(node_id) else {
                continue;
            };

            if let Node::Directory(dir) = &*node {
                for entry in dir.children.iter() {
                    stack.push_back(*entry.value());
                }
            }

            if !first {
                node.set_permissions(NodePermissions::Inherited)
            }
            first = false;
        }

        self.nodes
            .get(node_id)?
            .set_permissions(NodePermissions::Set(permissions));

        Ok(())
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

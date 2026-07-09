use crossbeam::atomic::AtomicCell;
use dashmap::DashMap;
use fat_vfs::{Permissions, VfsNodeType};
use std::{
    fmt, io,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

use super::NodeId;

/// A file system node, either a directory or a file.
pub enum Node {
    Directory(DirectoryNode),
    File(FileNode),
}

impl Node {
    /// Creates an empty directory node with default read-write permissions.
    #[must_use]
    pub fn empty_dir() -> Self {
        Self::Directory(DirectoryNode {
            children: DashMap::new(),
            parent: AtomicCell::new(None),
            permissions: AtomicCell::new(NodePermissions::Inherited),
        })
    }

    /// Creates an empty file node with default read-write permissions.
    #[must_use]
    pub fn empty_file() -> Self {
        Self::File(FileNode {
            content: RwLock::new(Arc::default()),
            parent: AtomicCell::new(None),
            permissions: AtomicCell::new(NodePermissions::Inherited),
        })
    }

    /// Returns a reference to the inner [`DirectoryNode`].
    pub fn as_dir(&self) -> io::Result<&DirectoryNode> {
        match self {
            Self::Directory(dir) => Ok(dir),
            Self::File(_) => Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "expected a directory, found a file",
            )),
        }
    }

    /// Returns a reference to the inner [`DirectoryNode`].
    pub fn as_file(&self) -> io::Result<&FileNode> {
        match self {
            Self::File(file) => Ok(file),
            Self::Directory(_) => Err(io::Error::new(
                io::ErrorKind::IsADirectory,
                "expected a file, found a directory",
            )),
        }
    }

    /// Returns the parent of this node.
    #[allow(dead_code)]
    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        match self {
            Self::Directory(dir) => dir.parent.load(),
            Self::File(file) => file.parent.load(),
        }
    }

    /// Returns the permission set for this node.
    #[must_use]
    pub fn permissions(&self) -> NodePermissions {
        match self {
            Self::Directory(dir) => dir.permissions.load(),
            Self::File(file) => file.permissions.load(),
        }
    }

    /// Gets the node type.
    pub fn ty(&self) -> VfsNodeType {
        match self {
            Self::Directory(..) => VfsNodeType::Directory,
            Self::File(..) => VfsNodeType::File,
        }
    }

    /// Sets the node parent for this node.
    pub fn set_parent(&self, parent: NodeId) {
        match self {
            Self::Directory(dir) => dir.parent.store(Some(parent)),
            Self::File(file) => file.parent.store(Some(parent)),
        }
    }

    /// Sets the permissions for this node.
    pub fn set_permissions(&self, permissions: NodePermissions) {
        match self {
            Self::Directory(dir) => dir.permissions.store(permissions),
            Self::File(file) => file.permissions.store(permissions),
        }
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory(inner) => fmt::Debug::fmt(&inner, f),
            Self::File(inner) => fmt::Debug::fmt(&inner, f),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum NodePermissions {
    /// This node has explictly set permissions.
    Set(Permissions),

    /// This node has no permissions set, therefore its permissions
    /// are inherited from its ancestors.
    Inherited,
}

#[derive(Debug)]
pub struct DirectoryNode {
    pub(crate) children: DashMap<String, NodeId>,
    pub(crate) parent: AtomicCell<Option<NodeId>>,
    pub(crate) permissions: AtomicCell<NodePermissions>,
}

impl DirectoryNode {
    pub fn add(&self, name: &str, node: NodeId) {
        self.children.insert(name.to_string(), node);
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.children.contains_key(name)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<NodeId> {
        self.children.get(name).map(|v| *v.value())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    pub fn remove(&self, name: &str) {
        self.children.remove(name);
    }
}

#[derive(Debug)]
pub struct FileNode {
    pub(crate) content: RwLock<Arc<Vec<u8>>>,
    pub(crate) parent: AtomicCell<Option<NodeId>>,
    pub(crate) permissions: AtomicCell<NodePermissions>,
}

impl FileNode {
    /// Attempts to copy the entire contents of a source node to a target node
    /// along with its permissions.
    #[allow(dead_code)]
    pub fn copy(source: &Arc<FileNode>, target: &Arc<FileNode>) -> io::Result<()> {
        target.replace(&source.read())?;
        target.permissions.store(source.permissions.load());

        Ok(())
    }

    /// Safely reads the content of a file regardless the pointer has been poisoned.
    pub fn read(&self) -> RwLockReadGuard<'_, Arc<Vec<u8>>> {
        match self.content.read() {
            Ok(content) => content,

            // We have to recover it no matter what.
            Err(error) => error.into_inner(),
        }
    }

    pub fn replace_owned(&self, content: Vec<u8>) {
        *self.write() = Arc::new(content);
    }

    pub fn replace(&self, content: &[u8]) -> io::Result<()> {
        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(content);

        self.replace_owned(vec);
        Ok(())
    }

    /// Safely reads the content of a file regardless the pointer has been poisoned.
    fn write(&self) -> RwLockWriteGuard<'_, Arc<Vec<u8>>> {
        match self.content.write() {
            Ok(content) => content,

            // We have to recover it no matter what.
            Err(error) => error.into_inner(),
        }
    }
}

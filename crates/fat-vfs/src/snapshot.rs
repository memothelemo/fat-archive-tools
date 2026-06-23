use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Permissions;

/// A builder to construct a VFS snapshot directory hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct VfsSnapshotDir {
    children: BTreeMap<String, VfsSnapshotNode>,
    permissions: Permissions,
}

impl VfsSnapshotDir {
    /// Creates a new, empty directory builder with default read-write permissions.
    #[must_use]
    pub fn new() -> Self {
        Self {
            children: BTreeMap::new(),
            permissions: Permissions::READ_WRITE,
        }
    }

    /// Adds a child node to the directory.
    ///
    /// The child can be any type that implements `Into<VfsSnapshotNode>`, including
    /// [`VfsSnapshotNode`] itself and [`VfsSnapshotDir`].
    pub fn add(mut self, name: impl Into<String>, value: impl Into<VfsSnapshotNode>) -> Self {
        self.children.insert(name.into(), value.into());
        self
    }

    /// Adds an empty file to the directory.
    pub fn empty_file(mut self, name: impl Into<String>) -> Self {
        let node = VfsSnapshotNode::empty_file();
        self.children.insert(name.into(), node);
        self
    }

    /// Adds a file with the given content to the directory.
    pub fn file(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        let node = VfsSnapshotNode::File {
            data: value.into(),
            permissions: Permissions::READ_WRITE,
        };
        self.children.insert(name.into(), node);
        self
    }

    /// Sets the permissions of this directory.
    pub fn permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = permissions;
        self
    }

    /// Finalizes building and returns the directory as a [`VfsSnapshotNode`].
    pub fn build(self) -> VfsSnapshotNode {
        VfsSnapshotNode::Directory {
            children: self.children,
            permissions: self.permissions,
        }
    }
}

impl Default for VfsSnapshotDir {
    fn default() -> Self {
        Self::new()
    }
}

/// A node in a VFS snapshot tree, representing either a file or a directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
#[must_use]
pub enum VfsSnapshotNode {
    /// A file containing binary data and permissions.
    File {
        /// The file data.
        #[serde(rename = "$data")]
        data: Vec<u8>,

        /// The file permissions.
        #[serde(
            default = "default_permissions",
            rename = "$permissions",
            skip_serializing_if = "is_default_permissions"
        )]
        permissions: Permissions,
    },

    /// A directory containing child nodes and permissions.
    Directory {
        /// The child nodes mapped by their names.
        #[serde(flatten)]
        children: BTreeMap<String, VfsSnapshotNode>,

        /// The directory permissions.
        #[serde(
            default = "default_permissions",
            rename = "$permissions",
            skip_serializing_if = "is_default_permissions"
        )]
        permissions: Permissions,
    },
}

const fn default_permissions() -> Permissions {
    Permissions::READ_WRITE
}

const fn is_default_permissions(permissions: &Permissions) -> bool {
    permissions.contains(Permissions::READ_WRITE)
}

impl VfsSnapshotNode {
    /// Returns a new [`VfsSnapshotDir`] builder.
    pub fn directory() -> VfsSnapshotDir {
        VfsSnapshotDir::new()
    }

    /// Returns an empty directory snapshot node.
    pub fn empty_dir() -> Self {
        Self::Directory {
            children: BTreeMap::new(),
            permissions: Permissions::READ_WRITE,
        }
    }

    /// Returns a file snapshot node with the specified data.
    pub fn file(data: impl Into<Vec<u8>>) -> Self {
        Self::File {
            data: data.into(),
            permissions: Permissions::READ_WRITE,
        }
    }

    /// Returns an empty file snapshot node.
    pub fn empty_file() -> Self {
        Self::File {
            data: Vec::new(),
            permissions: Permissions::READ_WRITE,
        }
    }

    /// Returns a copy of the node with modified permissions.
    pub fn permissions(self, permissions: Permissions) -> Self {
        match self {
            Self::Directory { children, .. } => Self::Directory {
                children,
                permissions,
            },
            Self::File { data, .. } => Self::File { data, permissions },
        }
    }
}

impl From<VfsSnapshotDir> for VfsSnapshotNode {
    fn from(node: VfsSnapshotDir) -> Self {
        VfsSnapshotNode::Directory {
            children: node.children,
            permissions: node.permissions,
        }
    }
}

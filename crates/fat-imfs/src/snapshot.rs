use crossbeam::atomic::AtomicCell;
use std::{collections::BTreeMap, io, sync::Arc};
use typed_path::Utf8UnixPath;

use crate::{
    Permissions,
    inner::ImfsInner,
    node::{Node, NodeId, NodePermissions},
};

/// A builder to construct a VFS snapshot directory hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct ImfsDirectoryBuilder {
    children: BTreeMap<String, ImfsSnapshotNode>,
    permissions: Permissions,
}

impl ImfsDirectoryBuilder {
    /// Creates a new, empty directory builder with default read-write permissions.
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
    pub fn add(mut self, name: impl Into<String>, value: impl Into<ImfsSnapshotNode>) -> Self {
        self.children.insert(name.into(), value.into());
        self
    }

    /// Adds an empty file to the directory.
    pub fn empty_file(mut self, name: impl Into<String>) -> Self {
        let node = ImfsSnapshotNode::empty_file();
        self.children.insert(name.into(), node);
        self
    }

    /// Adds a file with the given content to the directory.
    pub fn file(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        let node = ImfsSnapshotNode::File {
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
    pub fn build(self) -> ImfsSnapshotNode {
        ImfsSnapshotNode::Directory {
            children: self.children,
            permissions: self.permissions,
        }
    }
}

impl Default for ImfsDirectoryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A node in a VFS snapshot tree, representing either a file or a directory.
#[derive(Clone, Debug, Eq, PartialEq)]
// #[serde(untagged)]
#[must_use]
pub enum ImfsSnapshotNode {
    /// A file containing binary data and permissions.
    File {
        /// The file data.
        // #[serde(rename = "$data")]
        data: Vec<u8>,

        /// The file permissions.
        // #[serde(
        //     default = "default_permissions",
        //     rename = "$permissions",
        //     skip_serializing_if = "is_default_permissions"
        // )]
        permissions: Permissions,
    },

    /// A directory containing child nodes and permissions.
    Directory {
        /// The child nodes mapped by their names.
        // #[serde(flatten)]
        children: BTreeMap<String, ImfsSnapshotNode>,

        /// The directory permissions.
        // #[serde(
        //     default = "default_permissions",
        //     rename = "$permissions",
        //     skip_serializing_if = "is_default_permissions"
        // )]
        permissions: Permissions,
    },
}

// const fn default_permissions() -> Permissions {
//     Permissions::READ_WRITE
// }

// const fn is_default_permissions(permissions: &Permissions) -> bool {
//     permissions.contains(Permissions::READ_WRITE)
// }

impl ImfsSnapshotNode {
    /// Returns a new [`VfsSnapshotDir`] builder.
    pub fn directory() -> ImfsDirectoryBuilder {
        ImfsDirectoryBuilder::new()
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

impl From<ImfsDirectoryBuilder> for ImfsSnapshotNode {
    fn from(node: ImfsDirectoryBuilder) -> Self {
        ImfsSnapshotNode::Directory {
            children: node.children,
            permissions: node.permissions,
        }
    }
}

thread_local! {
    static VISIT_DEPTH: AtomicCell<usize> = const { AtomicCell::new(0) };
}

const MAXIMUM_VISIT_DEPTH: usize = 50;

impl crate::InMemoryFs {
    /// Applies the snapshot to the in-memory file system. It ignores
    /// the permissions configured by the snapshot beforehand.
    ///
    /// Any existing files will be wiped before it is inserted or updated.
    pub fn apply_from_snapshot(&self, snapshot: &ImfsSnapshotNode) -> io::Result<()> {
        let ImfsSnapshotNode::Directory {
            children,
            permissions,
        } = snapshot
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "root snapshot node should be a directory",
            ));
        };

        self.wipe()?;
        for (name, node) in children.iter() {
            VISIT_DEPTH.with(|depth| {
                depth.store(0);
            });
            apply_from_snapshot(&self.inner, self.inner.root, name, node)?;
        }

        let root = self.inner.nodes.get(self.inner.root)?;
        root.set_permissions(NodePermissions::Set(*permissions));

        Ok(())
    }

    /// Creates a snapshot based on the contents of the in-memory file system.
    pub fn generate_snapshot(&self) -> io::Result<ImfsSnapshotNode> {
        VISIT_DEPTH.with(|depth| {
            depth.store(0);
        });
        generate_snapshot(&self.inner, Utf8UnixPath::new("/"))
    }
}

pub(crate) fn apply_from_snapshot(
    fs: &Arc<ImfsInner>,
    parent_id: NodeId,
    name: &str,
    node: &ImfsSnapshotNode,
) -> io::Result<()> {
    VISIT_DEPTH.with(|depth| {
        if depth.fetch_add(1) > MAXIMUM_VISIT_DEPTH {
            return Err(io::Error::other(
                "specified path has entries that are too nested to apply from a snapshot",
            ));
        }
        Ok(())
    })?;

    let parent_node = fs.nodes.get(parent_id)?;
    let parent = parent_node.as_dir()?;

    let permissions = match &node {
        ImfsSnapshotNode::File { permissions, .. } => permissions,
        ImfsSnapshotNode::Directory { permissions, .. } => permissions,
    };

    let node_id = match node {
        ImfsSnapshotNode::File { data, .. } => {
            let node = Node::empty_file();
            node.as_file()?.replace(data)?;
            fs.add_node(parent, parent_id, name, node)?
        }
        ImfsSnapshotNode::Directory { children, .. } => {
            // Register first, before visiting to one of its children
            let node_id = fs.add_node(parent, parent_id, name, Node::empty_dir())?;
            for (name, node) in children.iter() {
                apply_from_snapshot(fs, node_id, name, node)?;
            }
            node_id
        }
    };

    let node = fs.nodes.get(node_id)?;
    node.set_permissions(NodePermissions::Set(*permissions));

    VISIT_DEPTH.with(|depth| {
        depth.fetch_sub(1);
    });
    Ok(())
}

pub(crate) fn generate_snapshot(
    fs: &Arc<ImfsInner>,
    path: &Utf8UnixPath,
) -> io::Result<ImfsSnapshotNode> {
    VISIT_DEPTH.with(|depth| {
        if depth.fetch_add(1) > MAXIMUM_VISIT_DEPTH {
            return Err(io::Error::other(
                "specified path has entries that are too nested to generate a snapshot",
            ));
        }
        Ok(())
    })?;

    let (node, node_id) = fs.find_node(path)?;
    let permissions = fs.permissions(node_id)?;

    let node = match &*node {
        Node::Directory(directory) => {
            let mut children = BTreeMap::new();
            for entry in directory.children.iter() {
                let path = path.join(entry.key());
                let node = generate_snapshot(fs, &path)?;
                children.insert(entry.key().to_string(), node);
            }

            ImfsSnapshotNode::Directory {
                children,
                permissions,
            }
        }
        Node::File(file) => {
            let mut data = Vec::new();
            let content = file.read();
            data.try_reserve(content.len())?;
            data.extend_from_slice(&content);

            drop(content);
            ImfsSnapshotNode::File { data, permissions }
        }
    };

    VISIT_DEPTH.with(|depth| {
        depth.fetch_sub(1);
    });
    Ok(node)
}

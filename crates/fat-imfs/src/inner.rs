use fat_vfs::{Permissions, VfsDirEntry, VfsMetadata, VfsNodeType};
use std::{
    collections::VecDeque,
    io,
    sync::{Arc, RwLock},
};
use typed_path::{
    Utf8TypedPath, Utf8UnixComponent, Utf8UnixPath, Utf8UnixPathBuf, constants::unix::SEPARATOR_STR,
};

use crate::node::{DirectoryNode, Node, NodeId, NodePermissions, NodeStore};

pub struct ImfsInner {
    /// The current directory of the file system to reference
    /// for the relative paths.
    pub(crate) current_dir: RwLock<Utf8UnixPathBuf>,

    /// Storage medium for all in-memory file system nodes.
    pub(crate) nodes: NodeStore,

    /// Handle to the root directory.
    pub(crate) root: NodeId,
}

impl ImfsInner {
    #[must_use]
    pub fn empty() -> Self {
        let nodes = NodeStore::new();
        let root = nodes
            .insert(Node::empty_dir())
            .expect("node ids should not be exhausted after ImfsNodeStore::new");

        Self {
            current_dir: RwLock::new(Utf8UnixPathBuf::from("/")),
            nodes,
            root,
        }
    }

    /// Implementation of [`std::fs::create_dir`] but for in-memory file system.
    pub fn create_dir(&self, target: &Utf8UnixPath) -> io::Result<()> {
        let (parent, target_name) = self.require_file_name(target)?;

        let (parent, parent_id) = self.find_node(parent)?;
        let parent = parent.as_dir()?;
        self.check_perms(parent_id, ImfsNodeOperation::Write)?;

        if parent.contains(target_name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "specified path already exists",
            ));
        }

        self.add_node(parent, parent_id, target_name, Node::empty_dir())?;
        Ok(())
    }

    /// Implementation of [`std::fs::create_dir`] but for in-memory file system.
    pub fn create_dir_all(&self, target: &Utf8UnixPath) -> io::Result<()> {
        let mut current = self.root;
        for component in target.iter() {
            if component == SEPARATOR_STR {
                continue;
            }

            let node = self.nodes.get(current)?;
            let directory = node.as_dir()?;
            self.check_perms(current, ImfsNodeOperation::Write)?;

            if let Some(child_id) = directory.get(component) {
                current = child_id;
                continue;
            }

            current = self.add_node(directory, current, component, Node::empty_dir())?;
        }

        Ok(())
    }

    /// Normalizes a platform-specific path (either on Unix or Windows) to an
    /// absolute Unix path, rejecting relative and invalid paths.
    pub fn normalize(&self, path: Utf8TypedPath<'_>) -> io::Result<Utf8UnixPathBuf> {
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

    /// Removes all node's descendants.
    pub fn remove_all_descendants(&self, node_id: NodeId) -> io::Result<()> {
        let node = self.nodes.get(node_id)?;
        let Node::Directory(directory) = &*node else {
            return Ok(());
        };
        self.check_perms(node_id, ImfsNodeOperation::Write)?;

        let mut to_visit = Vec::new();
        to_visit.try_reserve(directory.children.len())?;

        for child in directory.children.iter() {
            to_visit.push(*child.value());
        }

        let mut to_delete = Vec::new();
        to_delete.try_reserve(directory.children.len())?;

        while let Some(current_id) = to_visit.pop() {
            to_delete.push(current_id);

            let Ok(node) = self.nodes.get(current_id) else {
                continue;
            };
            self.check_perms(current_id, ImfsNodeOperation::Write)?;

            if let Node::Directory(directory) = &*node {
                for entry in directory.children.iter() {
                    to_visit.push(*entry.value());
                }
            }
        }

        for current_id in to_delete.into_iter().rev() {
            self.nodes.remove(current_id)?;
        }

        Ok(())
    }

    /// Removes the directory from the specified path regardless if
    /// the directory has children or not.
    pub fn remove_dir(&self, target: &Utf8UnixPath) -> io::Result<()> {
        if target == SEPARATOR_STR {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove root directory",
            ));
        }

        let (parent, target_name) = self.require_file_name(target)?;
        let (parent, parent_node_id) = self.find_node(parent)?;

        let directory = parent.as_dir()?;
        self.check_perms(parent_node_id, ImfsNodeOperation::Write)?;

        let (target_node, target_node_id) = self.find_node(target)?;
        target_node.as_dir()?;

        self.check_perms(target_node_id, ImfsNodeOperation::Write)?;
        directory.remove(target_name);

        self.remove_all_descendants(target_node_id)?;
        Ok(())
    }

    /// Implementation of [`std::fs::remove_file`] but for in-memory file system.
    pub fn remove_file(&self, target: &Utf8UnixPath) -> io::Result<()> {
        let (parent, target_name) = self.require_file_name(target)?;
        let (parent, parent_node_id) = self.find_node(parent)?;

        let directory = parent.as_dir()?;
        self.check_perms(parent_node_id, ImfsNodeOperation::Write)?;

        let (target_node, target_node_id) = self.find_node(target)?;
        target_node.as_file()?;

        self.check_perms(target_node_id, ImfsNodeOperation::Write)?;
        directory.remove(target_name);

        self.nodes.remove(target_node_id)?;
        Ok(())
    }

    /// Gets the optional file name from the provided target path, it returns the
    /// split into two components:
    /// - The parent directory.
    /// - The file or node name (optional)
    pub fn get_file_name<'a>(
        &self,
        target: &'a Utf8UnixPath,
    ) -> (&'a Utf8UnixPath, Option<&'a str>) {
        let name = target.file_name();
        let parent = target.parent().unwrap_or_else(|| Utf8UnixPath::new("/"));
        (parent, name)
    }

    /// Renames a file to a new name, replacing the original file if `to` already exists.
    pub fn rename_file(&self, from: &Utf8UnixPath, to: &Utf8UnixPath) -> io::Result<()> {
        if from == to {
            return Ok(());
        }

        let (from_node, from_node_id) = self.find_node(from)?;
        let from_file = from_node.as_file()?;

        match self.find_node(to) {
            Ok((to, to_node_id)) => {
                self.check_perms(from_node_id, ImfsNodeOperation::Read)?;
                self.check_perms(to_node_id, ImfsNodeOperation::Write)?;

                to.as_file()?.replace(&from_file.read())?;
                self.remove_file(from)?;

                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        };

        let (from_parent, from_file_name) = self.require_file_name(from)?;
        let (from_parent_node, from_parent_id) = self.find_node(from_parent)?;

        let (to_parent, to_file_name) = self.require_file_name(to)?;
        let (to_parent_node, to_parent_id) = self.find_node(to_parent)?;

        let from_parent_dir = from_parent_node.as_dir()?;
        let to_parent_dir = to_parent_node.as_dir()?;

        self.check_perms(from_node_id, ImfsNodeOperation::Read)?;
        self.check_perms(from_parent_id, ImfsNodeOperation::Write)?;
        self.check_perms(to_parent_id, ImfsNodeOperation::Write)?;

        let node = Node::empty_file();
        node.as_file()?.replace(&from_file.read())?;

        self.add_node(to_parent_dir, to_parent_id, to_file_name, node)?;
        from_parent_dir.remove(from_file_name);
        self.nodes.remove(from_node_id)?;

        Ok(())
    }

    /// Ensures that the provided target path contains a file name, then returns
    /// the split into two components:
    /// - The parent directory.
    /// - The file or node name.
    pub fn require_file_name<'a>(
        &self,
        target: &'a Utf8UnixPath,
    ) -> io::Result<(&'a Utf8UnixPath, &'a str)> {
        let name = target
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;

        let parent = target.parent().unwrap_or_else(|| Utf8UnixPath::new("/"));
        Ok((parent, name))
    }

    /// Sets the permissions recursively from a specific path.
    pub fn set_permissions_recursive(
        &self,
        target: &Utf8UnixPath,
        permissions: Permissions,
    ) -> io::Result<()> {
        // Bypass permission checks if the target is a root directory
        let target_node_id = if target.as_str() == SEPARATOR_STR {
            let permissions = self.permissions(self.root)?;
            if !permissions.contains(Permissions::WRITE) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "cannot set permissions on a read-only directory",
                ));
            }
            self.root
        } else {
            self.find_node_id(target)?
        };

        // Set all of the descendants' permission to inherited.
        let mut stack = VecDeque::new();
        stack.push_back(target_node_id);

        while let Some(node_id) = stack.pop_front() {
            let Ok(node) = self.nodes.get(node_id) else {
                continue;
            };

            if let Node::Directory(directory) = &*node {
                let iter = directory.children.iter();
                iter.for_each(|entry| stack.push_back(*entry.value()));
            }

            // Permission checking is already done in the first part of the function.
            if node_id == target_node_id {
                continue;
            }

            self.check_perms(node_id, ImfsNodeOperation::Write)?;
            node.set_permissions(NodePermissions::Inherited);
        }

        self.nodes
            .get(target_node_id)?
            .set_permissions(NodePermissions::Set(permissions));

        Ok(())
    }

    /// Implementation of [`std::fs::write`] but for in-memory file system.
    pub fn write(&self, target: &Utf8UnixPath, contents: &[u8]) -> io::Result<()> {
        let (parent, target_name) = self.require_file_name(target)?;

        // Make sure its parent directory exists
        let (parent, parent_id) = self.find_node(parent)?;
        let parent = parent.as_dir()?;

        let target_node_id = if let Some(id) = parent.get(target_name) {
            id
        } else {
            self.add_node(parent, parent_id, target_name, Node::empty_file())?
        };

        self.check_perms(parent_id, ImfsNodeOperation::Write)?;

        // Do we have the permission to overwrite/write the file?
        let target_node = self.nodes.get(target_node_id)?;
        self.check_perms(target_node_id, ImfsNodeOperation::Write)?;

        // It is much faster to replace the entire file than to use streaming.
        let file = target_node.as_file()?;
        file.replace(contents)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImfsNodeOperation {
    /// Checks whether a node can be read.
    Read,

    /// Checks whether a node can be written.
    Write,
}

// Node-related functions
impl ImfsInner {
    /// Adds a new node into the node store with required fields, then
    /// set its parent to the specified parent node.
    ///
    /// ## Caution
    ///
    /// This function assumes that the parent is not referencing back
    /// to the tree that it may be cyclic.
    pub fn add_node(
        &self,
        parent: &DirectoryNode,
        parent_id: NodeId,
        name: &str,
        node: Node,
    ) -> io::Result<NodeId> {
        node.set_parent(parent_id);

        let node_id = self.nodes.insert(node)?;
        parent.add(name, node_id);

        Ok(node_id)
    }

    /// Checks whether a node can perform an operation based the permissions
    /// set by the node and its ancestors.
    pub fn check_perms(&self, node_id: NodeId, operation: ImfsNodeOperation) -> io::Result<()> {
        let permissions = self.permissions(node_id)?;
        let can_operate_this = match operation {
            ImfsNodeOperation::Read => permissions.contains(Permissions::READ),
            ImfsNodeOperation::Write => permissions.contains(Permissions::WRITE),
        };

        if !can_operate_this {
            let node = self.nodes.get(node_id)?;
            let message = match node.ty() {
                VfsNodeType::Directory => "could not access directory",
                VfsNodeType::File => "could not access file",
                VfsNodeType::Symlink => "could not access symlink",
                VfsNodeType::Unknown => "could not access unknown in-memory file system node",
            };
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, message));
        }

        Ok(())
    }

    /// Resolves node metadata from a node ID.
    pub fn metadata(&self, node_id: NodeId) -> io::Result<VfsMetadata> {
        let node = self.nodes.get(node_id)?;
        let permissions = self.permissions(node_id)?;

        Ok(VfsMetadata {
            mode: permissions,
            size: match &*node {
                Node::Directory(..) => 0,
                Node::File(file) => file.read().len() as u64,
            },
            ty: node.ty(),
        })
    }

    /// Resolves node permissions from a specified node based on its ancestors.
    pub fn permissions(&self, node_id: NodeId) -> io::Result<Permissions> {
        let mut current = Some(node_id);
        let mut resolved = Permissions::READ_WRITE;

        while let Some(node_id) = current {
            let node = self.nodes.get(node_id)?;
            let permissions = node.permissions();

            if let NodePermissions::Set(value) = permissions {
                resolved &= value;
            }

            // Move to the next parent to resolve more
            current = node.parent();
        }

        Ok(resolved)
    }

    /// Finds an assigned [node id] from a presumably normalized path.
    ///
    /// [node id]: NodeId
    pub fn find_node_id(&self, path: &Utf8UnixPath) -> io::Result<NodeId> {
        let mut current = self.root;
        for component in path.components() {
            let name = match component {
                Utf8UnixComponent::RootDir => {
                    current = self.root;
                    continue;
                }
                Utf8UnixComponent::Normal(name) => name,

                // other variants should be eliminated by normalize.
                #[cfg(debug_assertions)]
                variant => panic!(
                    "unhandled variant: {variant:?} (maybe forgot to normalize the path it first?)"
                ),

                #[cfg(not(debug_assertions))]
                _ => continue,
            };

            let node = self.nodes.get(current)?;
            current = node.as_dir()?.get(name).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "specified path not found")
            })?;
        }

        Ok(current)
    }

    /// Finds a node from a presumably normalized path, then returns
    /// the node along with its associated assigned [node id] for
    /// retrieval if needed.
    ///
    /// [node id]: NodeId
    pub fn find_node(&self, path: &Utf8UnixPath) -> io::Result<(Arc<Node>, NodeId)> {
        let node_id = self.find_node_id(path)?;
        match self.nodes.get(node_id) {
            Ok(node) => Ok((node, node_id)),
            Err(error) => Err(error),
        }
    }
}

pub struct ImfsDirEntry {
    pub(crate) depth: usize,
    pub(crate) fs: Arc<ImfsInner>,
    pub(crate) path: Utf8UnixPathBuf,
}

impl ImfsDirEntry {
    #[must_use]
    pub fn base(fs: Arc<ImfsInner>, path: Utf8UnixPathBuf) -> Box<dyn VfsDirEntry> {
        Box::new(Self { depth: 0, fs, path })
    }

    #[must_use]
    pub fn nested(depth: usize, fs: Arc<ImfsInner>, path: Utf8UnixPathBuf) -> Box<dyn VfsDirEntry> {
        Box::new(Self { depth, fs, path })
    }
}

impl VfsDirEntry for ImfsDirEntry {
    fn depth(&self) -> usize {
        self.depth
    }

    fn file_name(&self) -> io::Result<String> {
        let str = self.path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidFilename, "path has no file name")
        })?;
        Ok(str.to_string())
    }

    fn metadata(&self) -> io::Result<VfsMetadata> {
        let node_id = self.fs.find_node_id(&self.path)?;
        self.fs.metadata(node_id)
    }

    fn node_type(&self) -> io::Result<VfsNodeType> {
        self.metadata().map(|v| v.ty)
    }

    fn path(&self) -> Utf8TypedPath<'_> {
        self.path.to_typed_path()
    }
}

/// An iterator that walks a directory tree or a single file recursively.
pub struct ImfsWalkDir {
    fs: Arc<ImfsInner>,
    stack: Vec<WalkDirStackNode>,
    deferred_error: Option<io::Error>,
}

struct WalkDirStackNode {
    id: NodeId,
    depth: usize,
    name: Option<String>,
    parent_path: Arc<Utf8UnixPathBuf>,
}

impl ImfsWalkDir {
    pub(crate) fn new(fs: &Arc<ImfsInner>, root: Utf8TypedPath<'_>) -> io::Result<Self> {
        let root = fs.normalize(root)?;
        let root_node_id = fs.find_node_id(&root)?;
        fs.check_perms(root_node_id, ImfsNodeOperation::Read)?;

        let stack = vec![WalkDirStackNode {
            id: root_node_id,
            depth: 0,
            name: None,
            parent_path: Arc::new(root),
        }];

        Ok(Self {
            fs: fs.clone(),
            stack,
            deferred_error: None,
        })
    }

    fn populate(&mut self, id: NodeId, depth: usize, path: Utf8UnixPathBuf) -> io::Result<()> {
        let node = self.fs.nodes.get(id)?;
        let Node::Directory(directory) = &*node else {
            return Ok(());
        };

        self.fs.check_perms(id, ImfsNodeOperation::Read)?;

        // Optimize lock contention: clone keys and copy values immediately
        // to release all DashMap locks/references before sorting.
        let mut entries: Vec<(String, NodeId)> = directory
            .children
            .iter()
            .map(|entry| (entry.key().clone(), *entry.value()))
            .collect();

        entries.sort_by(|a, b| a.0.cmp(&b.0));

        // Push children in reverse order so they are popped in alphabetical order.
        let parent_path = Arc::new(path);
        let depth = depth
            .checked_add(1)
            .ok_or_else(|| io::Error::other("exceeded maximum depth"))?;

        for (name, child_id) in entries.into_iter().rev() {
            self.stack.push(WalkDirStackNode {
                id: child_id,
                depth,
                name: Some(name),
                parent_path: parent_path.clone(),
            });
        }

        Ok(())
    }
}

impl Iterator for ImfsWalkDir {
    type Item = io::Result<Box<dyn VfsDirEntry>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(error) = self.deferred_error.take() {
            return Some(Err(error));
        }

        let node = self.stack.pop()?;
        let path = match &node.name {
            Some(name) => node.parent_path.join(name),
            None => (*node.parent_path).clone(),
        };

        if let Err(error) = self.populate(node.id, node.depth, path.clone()) {
            self.deferred_error = Some(error);
        }

        Some(Ok(ImfsDirEntry::nested(node.depth, self.fs.clone(), path)))
    }
}

use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, NodeType, OpenOptions, Permissions, VfsFileStream};
use std::io;
use typed_path::{Utf8TypedPath, constants::unix::SEPARATOR_STR};

use crate::{
    InMemoryFs,
    node::{Node, NodePermissions},
};

impl FileSystem for InMemoryFs {
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
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

        let (node, node_id) = self.inner.find_node(&path)?;
        let permissions = self.inner.resolve_permissions(node_id)?;

        Ok(Metadata {
            mode: permissions,
            size: match &*node {
                Node::Directory(..) => 0,
                Node::File(file) => file.read().len() as u64,
            },
            ty: node.ty(),
        })
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

    fn remove_dir(&self, _path: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
    }

    fn remove_dir_all(&self, _path: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
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
}

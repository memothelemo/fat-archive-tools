use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, OpenOptions, Permissions, VfsFileStream};
use std::io;
use typed_path::Utf8TypedPath;
use typed_path::constants::unix::SEPARATOR_STR;

use crate::{InMemoryFs, node::Node};

impl FileSystem for InMemoryFs {
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let (parent_path, name) = Self::split_parent(&path)?;

        let parent = self.find_node(parent_path)?;
        let parent = parent.as_dir()?;

        if parent.contains(name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "specified path already exists",
            ));
        }

        let child_id = self.nodes.insert(Node::empty_dir())?;
        parent.add(name, child_id);

        Ok(())
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let mut current = self.root;
        for component in path.iter() {
            if component == SEPARATOR_STR {
                continue;
            }

            let node = self.nodes.get(current)?;
            let directory = node.as_dir()?;

            if let Some(child_id) = directory.get(component) {
                current = child_id;
                continue;
            }

            let child_id = self.nodes.insert(Node::empty_dir())?;
            directory.add(component, child_id);
            current = child_id;
        }

        Ok(())
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        let path = Self::normalize(path)?;
        match self.find_node_id(&path) {
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
        let node = self.find_node(&path)?;

        let content = node.as_file()?.read();
        hasher.update(&content);

        Ok(hasher.digest())
    }

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<Metadata> {
        let path = Self::normalize(path)?;
        Ok(self.find_node(&path)?.metadata())
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
        let node_ref = self.nodes.get(node_id)?;
        let node = node_ref.as_dir()?;
        if !node.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "directory not empty",
            ));
        }

        let (parent_path, name) = Self::split_parent(&path)?;
        let parent_id = self.find_node_id(parent_path)?;

        let parent_node = self.nodes.get(parent_id)?;
        let parent_node = parent_node.as_dir()?;
        parent_node.remove(name);

        self.nodes.remove(node_id)?;
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

    // Unlike other platfoms, we'll throw an error if a full directory path does not exist.
    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let (parent, name) = Self::split_parent(&path)?;

        // Make sure the full directory path does exist.
        let parent = self.find_node(parent)?;
        let parent = parent.as_dir()?;

        // Overwrite the entire contents with the help of file node.
        let child_node_id = if let Some(id) = parent.get(name) {
            id
        } else {
            let node_id = self.nodes.insert(Node::empty_file())?;
            parent.add(name, node_id);
            node_id
        };

        let child_node = self.nodes.get(child_node_id)?;
        let file = child_node.as_file()?;
        file.replace(contents)?;

        Ok(())
    }
}

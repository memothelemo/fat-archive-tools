use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, OpenOptions, Permissions, VfsFileStream};
use std::io;
use typed_path::Utf8TypedPath;
use typed_path::constants::unix::SEPARATOR_STR;

use crate::{
    InMemoryFs,
    handle::{FileWriteHandle, WriteHandleMode},
    node::Node,
};

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
        path: Utf8TypedPath<'_>,
        options: &mut OpenOptions,
    ) -> io::Result<Box<dyn VfsFileStream>> {
        let path = Self::normalize(path)?;

        let node_id = match self.find_node_id(&path) {
            Ok(id) => {
                if options.create_new {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "file already exists",
                    ));
                }
                id
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if options.create || options.create_new {
                    let (parent, name) = Self::split_parent(&path)?;
                    let parent = self.find_node(parent)?;
                    let parent = parent.as_dir()?;

                    let new_id = self.nodes.insert(Node::empty_file())?;
                    parent.add(name, new_id);
                    new_id
                } else {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        };

        // Check that it's a file, not a directory
        let node = self.nodes.get(node_id)?;
        if node.as_file().is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot open a directory as a file",
            ));
        }

        // Handle truncate immediately if requested
        if options.truncate {
            node.as_file()?.replace_owned(Vec::new());
        }

        let mode = if options.truncate {
            WriteHandleMode::Truncate
        } else if options.append {
            WriteHandleMode::Append
        } else {
            WriteHandleMode::Standard
        };

        let handle = FileWriteHandle::new(self.0.clone(), node_id, mode)?;
        Ok(Box::new(handle))
    }

    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()> {
        let from = Self::normalize(from)?;
        let to = Self::normalize(to)?;

        if from == to {
            return Ok(());
        }

        let from_str = from.as_str();
        let to_str = to.as_str();
        if to_str.starts_with(from_str) && (to_str.len() == from_str.len() || to_str.chars().nth(from_str.len()) == Some('/')) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot rename a directory to a subclass of itself",
            ));
        }

        let from_id = self.find_node_id(&from)?;

        let (from_parent_path, from_name) = Self::split_parent(&from)?;
        let from_parent = self.find_node(from_parent_path)?;
        let from_parent = from_parent.as_dir()?;

        let (to_parent_path, to_name) = Self::split_parent(&to)?;
        let to_parent = self.find_node(to_parent_path)?;
        let to_parent = to_parent.as_dir()?;

        if let Ok(to_id) = self.find_node_id(&to) {
            let to_node = self.nodes.get(to_id)?;
            if to_node.as_dir().is_ok() {
                let to_dir = to_node.as_dir()?;
                if !to_dir.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "destination directory is not empty",
                    ));
                }
                if self.nodes.get(from_id)?.as_dir().is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "cannot rename a file to a directory",
                    ));
                }
            } else {
                if self.nodes.get(from_id)?.as_dir().is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "cannot rename a directory to a file",
                    ));
                }
            }
            to_parent.remove(to_name);
            self.nodes.remove(to_id)?;
        }

        to_parent.add(to_name, from_id);
        from_parent.remove(from_name);

        Ok(())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;

        let node_id = self.find_node_id(&path)?;
        if node_id == self.root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove root directory",
            ));
        }

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

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let node_id = self.find_node_id(&path)?;

        if node_id == self.root {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cannot remove root directory",
            ));
        }

        self.remove_node_recursive(node_id)?;

        let (parent_path, name) = Self::split_parent(&path)?;
        let parent_id = self.find_node_id(parent_path)?;
        let parent_node = self.nodes.get(parent_id)?;
        let parent_node = parent_node.as_dir()?;
        parent_node.remove(name);

        Ok(())
    }

    fn set_permissions(
        &self,
        path: Utf8TypedPath<'_>,
        permissions: Permissions,
    ) -> io::Result<()> {
        let path = Self::normalize(path)?;
        let node = self.find_node(&path)?;
        match &*node {
            Node::Directory(dir) => dir.permissions.store(permissions),
            Node::File(file) => file.permissions.store(permissions),
        }
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

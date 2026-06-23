use crate::{FileSystem, Metadata, NodeType, OpenOptions, Permissions, VfsFileStream};

use fat_hasher::{Checksum, HashFunction};
use std::{fs, io};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

/// A [`FileSystem`] backend that implements transparently with [`std::fs`].
#[derive(Debug)]
pub struct OsFileSystem {}

type BoxedReadDir = Box<dyn Iterator<Item = io::Result<Utf8TypedPathBuf>>>;

impl OsFileSystem {
    /// Creates a new [`OsFileSystem`] instance.
    #[expect(
        clippy::new_without_default,
        reason = "no configuration required for OsFileSystem; makes no sense to implement one"
    )]
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }
}

impl FileSystem for OsFileSystem {
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::create_dir(path.as_str())
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::create_dir_all(path.as_str())
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        fs::exists(path.as_str())
    }

    fn hash(
        &self,
        path: Utf8TypedPath<'_>,
        mut hasher: Box<dyn HashFunction>,
    ) -> io::Result<Checksum> {
        let mut file = fs::File::open(path.as_str())?;
        std::io::copy(&mut file, &mut hasher)?;

        Ok(hasher.digest())
    }

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<Metadata> {
        fs::metadata(path.as_str()).map(into_vfs_metadata)
    }

    fn open(
        &self,
        path: Utf8TypedPath<'_>,
        options: &mut OpenOptions,
    ) -> io::Result<Box<dyn VfsFileStream>> {
        let file = fs::OpenOptions::new()
            .append(options.append)
            .create(options.create)
            .create_new(options.create_new)
            .read(options.read)
            .truncate(options.truncate)
            .write(options.write)
            .open(path.as_str())?;

        Ok(Box::new(file))
    }

    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>> {
        fs::read(path.as_str())
    }

    fn read_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<BoxedReadDir> {
        let iter = fs::read_dir(path.as_str())?
            .map(|v| v.map(|v| Utf8TypedPath::derive(&v.path().to_string_lossy()).to_path_buf()));

        Ok(Box::new(iter))
    }

    fn read_to_string(&self, path: Utf8TypedPath<'_>) -> io::Result<String> {
        fs::read_to_string(path.as_str())
    }

    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::rename(from.as_str(), to.as_str())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::remove_dir(path.as_str())
    }

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::remove_dir_all(path.as_str())
    }

    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::remove_file(path.as_str())
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

    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()> {
        fs::write(path.as_str(), contents)
    }
}

impl VfsFileStream for fs::File {
    fn metadata(&self) -> io::Result<Metadata> {
        fs::File::metadata(self).map(into_vfs_metadata)
    }

    fn sync_data(&mut self) -> io::Result<()> {
        fs::File::sync_all(self)
    }
}

/// Converts [`std::fs::Metadata`] into [`fat_vfs::Metadata`].
#[must_use]
fn into_vfs_metadata(metadata: fs::Metadata) -> Metadata {
    let ft = metadata.file_type();
    Metadata {
        mode: if metadata.permissions().readonly() {
            Permissions::READ
        } else {
            Permissions::READ_WRITE
        },
        size: metadata.len(),
        ty: if ft.is_file() {
            NodeType::File
        } else if ft.is_dir() {
            NodeType::Directory
        } else if ft.is_symlink() {
            NodeType::Symlink
        } else {
            NodeType::Unknown
        },
    }
}

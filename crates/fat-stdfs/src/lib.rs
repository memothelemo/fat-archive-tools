use fat_hasher::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Metadata, OpenOptions, Permissions, VfsFileStream};
use std::{fs, io};
use typed_path::Utf8TypedPath;

mod util;

/// A [`FileSystem`] backend that implements transparently with [`std::fs`].
#[derive(Debug)]
pub struct OsFileSystem {}

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
        fs::metadata(path.as_str()).map(self::util::into_vfs_metadata)
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

    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::rename(from.as_str(), to.as_str())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::remove_dir(path.as_str())
    }

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        fs::remove_dir_all(path.as_str())
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
}

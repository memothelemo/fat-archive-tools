use fat_checksum::{Checksum, HashFunction};
use fat_vfs::{FileSystem, Permissions, VfsMetadata, VfsReadDir};
use std::{env, fs, io};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};
use walkdir::WalkDir;

mod internal;
use crate::internal::OsReadDir;

use self::internal::{OsFileHandle, OsWalkDirEntry, file_type_to_vfs};

/// A [`FileSystem`] driver that implements transparently with [`std::fs`].
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

macro_rules! ensure_path_is_absolute {
    ($path:expr) => {{
        if $path.is_relative() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "relative paths are prohibited (use absolute paths instead)",
            ));
        }
    }};
}

impl FileSystem for OsFileSystem {
    fn current_dir(&self) -> io::Result<Utf8TypedPathBuf> {
        let current_dir = env::current_dir()?;
        Ok(Utf8TypedPath::derive(&current_dir.to_string_lossy()).to_path_buf())
    }

    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::create_dir(path.as_str())
    }

    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::create_dir_all(path.as_str())
    }

    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool> {
        ensure_path_is_absolute!(path);
        fs::exists(path.as_str())
    }

    fn hash(
        &self,
        path: Utf8TypedPath<'_>,
        mut hasher: Box<dyn HashFunction>,
    ) -> io::Result<Checksum> {
        ensure_path_is_absolute!(path);

        let mut file = fs::File::open(path.as_str())?;
        std::io::copy(&mut file, &mut hasher)?;

        Ok(hasher.digest())
    }

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<fat_vfs::VfsMetadata> {
        ensure_path_is_absolute!(path);
        Ok(fs::metadata(path.as_str())?.to_vfs())
    }

    fn open(
        &self,
        path: Utf8TypedPath<'_>,
        options: &mut fat_vfs::VfsOpenOptions,
    ) -> io::Result<fat_vfs::VfsFile> {
        ensure_path_is_absolute!(path);

        let file = fs::OpenOptions::new()
            .append(options.append)
            .create(options.create)
            .create_new(options.create_new)
            .read(options.read)
            .truncate(options.truncate)
            .write(options.write)
            .open(path.as_str())?;

        Ok(Box::new(OsFileHandle(file)))
    }

    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>> {
        ensure_path_is_absolute!(path);
        fs::read(path.as_str())
    }

    fn read_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<VfsReadDir> {
        ensure_path_is_absolute!(path);
        let iter = fs::read_dir(path.as_str())?;
        Ok(Box::new(OsReadDir(iter)))
    }

    fn read_to_string(&self, path: Utf8TypedPath<'_>) -> io::Result<String> {
        ensure_path_is_absolute!(path);
        fs::read_to_string(path.as_str())
    }

    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(from);
        ensure_path_is_absolute!(to);
        fs::rename(from.as_str(), to.as_str())
    }

    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::remove_dir(path.as_str())
    }

    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::remove_dir_all(path.as_str())
    }

    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::remove_file(path.as_str())
    }

    fn set_current_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        env::set_current_dir(path.as_str())
    }

    #[cfg(windows)]
    fn soft_link(&self, original: Utf8TypedPath<'_>, link: Utf8TypedPath<'_>) -> io::Result<()> {
        todo!()
    }

    #[cfg(unix)]
    fn soft_link(&self, original: Utf8TypedPath<'_>, link: Utf8TypedPath<'_>) -> io::Result<()> {
        ensure_path_is_absolute!(original);
        ensure_path_is_absolute!(link);
        std::os::unix::fs::symlink(original.as_str(), link.as_str())
    }

    fn walkdir(&self, root: Utf8TypedPath<'_>) -> io::Result<VfsReadDir> {
        ensure_path_is_absolute!(root);
        let iter = WalkDir::new(root.as_str())
            .into_iter()
            .map(|v| v.map(OsWalkDirEntry::boxed).map_err(Into::into));

        Ok(Box::new(iter))
    }

    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()> {
        ensure_path_is_absolute!(path);
        fs::write(path.as_str(), contents)
    }
}

trait OsMetadataExt {
    fn to_vfs(self) -> VfsMetadata;
}

impl OsMetadataExt for fs::Metadata {
    fn to_vfs(self) -> VfsMetadata {
        let ft = self.file_type();
        VfsMetadata {
            mode: if self.permissions().readonly() {
                Permissions::READ
            } else {
                Permissions::READ_WRITE
            },
            size: self.len(),
            ty: file_type_to_vfs(ft),
        }
    }
}

// Copied from: https://github.com/BurntSushi/walkdir/blob/master/src/dent.rs
use cfg_if::cfg_if;
use std::{
    ffi::OsStr,
    fmt, fs, io,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{DirEntryExt as _, MetadataExt as _};

/// A directory entry.
///
/// This is the expected type of value received from the walker
/// visitor in this crate.
///
/// On Unix systems, this type implements the [`DirEntryExt`]
/// trait, which provides access to the inode number of the directory
/// entry.
///
/// # Differences with [`std::fs::DirEntry`]
///
/// This type mostly mirrors the type by the same name in [`std::fs`],
/// but there are some differences:
///
/// - [`path`] and [`file_name`] return as borrowed types
///
/// [`std::fs`]: https://doc.rust-lang.org/stable/std/fs/index.html
/// [`path`]: DirEntry::path
/// [`file_name`]: DirEntry::file_name
pub struct DirEntry {
    path: PathBuf,

    /// The file type of the entry.
    ty: fs::FileType,

    /// The depth at which this entry was generate relative to its root.
    depth: usize,

    /// The underlying inode number (Unix only).
    #[cfg(unix)]
    ino: u64,

    /// The underlying metadata (Windows only). We store this on Windows
    /// because this comes for free while reading a directory.
    ///
    /// We use this to determine whether an entry is a directory or not, which
    /// works around a bug in Rust's standard library:
    /// <https://github.com/rust-lang/rust/issues/46484>
    #[cfg(windows)]
    metadata: fs::Metadata,
}

impl DirEntry {
    /// The full path that this entry represents.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consumes the full path that this entry represents.
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        self.path
    }

    /// Return the file type for the file that this entry points to.
    #[must_use]
    pub fn file_type(&self) -> fs::FileType {
        self.ty
    }

    /// Return the file name of this entry.
    #[must_use]
    pub fn file_name(&self) -> Option<&OsStr> {
        self.path.file_name()
    }

    /// Returns the depth at which this entry was created relative to its root.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Return the metadata for the file that this entry points to.
    ///
    /// # Platform behavior
    ///
    /// This always calls [`std::fs::symlink_metadata`].
    pub fn metadata(&self) -> io::Result<fs::Metadata> {
        cfg_if! {
            if #[cfg(windows)] {
                Ok(self.metadata.clone())
            } else {
                fs::metadata(&self.path)
            }
        }
    }
}

impl DirEntry {
    pub(super) fn from_entry(depth: usize, entry: &fs::DirEntry) -> io::Result<Self> {
        let ty = entry.file_type()?;

        #[cfg(windows)]
        let metadata = entry.metadata()?;

        Ok(DirEntry {
            path: entry.path(),
            ty,
            depth,

            #[cfg(unix)]
            ino: entry.ino(),
            #[cfg(windows)]
            metadata,
        })
    }

    pub(super) fn from_path(depth: usize, path: PathBuf) -> io::Result<Self> {
        let metadata = fs::metadata(&path)?;
        Ok(DirEntry {
            path,
            ty: metadata.file_type(),
            depth,

            #[cfg(unix)]
            ino: metadata.ino(),
            #[cfg(windows)]
            metadata,
        })
    }
}

impl Clone for DirEntry {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            ty: self.ty,
            depth: self.depth,

            #[cfg(unix)]
            ino: self.ino,
            #[cfg(windows)]
            metadata: self.metadata.clone(),
        }
    }
}

impl fmt::Debug for DirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DirEntry({:?})", self.path)
    }
}

/// Unix-specific extension methods for [`DirEntry`].
#[cfg(unix)]
pub trait DirEntryExt {
    /// Returns the underlying `d_ino` field in the contained `dirent`
    /// structure.
    fn ino(&self) -> u64;
}

#[cfg(unix)]
impl DirEntryExt for DirEntry {
    /// Returns the underlying `d_ino` field in the contained `dirent`
    /// structure.
    fn ino(&self) -> u64 {
        self.ino
    }
}

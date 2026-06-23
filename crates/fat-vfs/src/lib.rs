use bitflags::bitflags;
use fat_hasher::{Checksum, HashFunction};
use serde::{Deserialize, Serialize};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

use ::std::{fmt, io};

pub mod snapshot;
pub mod std;

pub use self::{snapshot::VfsSnapshotNode, std::OsFileSystem};

/// The core virtual filesystem abstraction.
pub trait FileSystem: fmt::Debug {
    /// Creates a new, empty directory at the provided path.
    ///
    /// It should replicate the behavior of [`std::fs::create_dir`].
    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Recurisvely create a directory and all of its parent components if they are missing.
    ///
    /// It should replicate the behavior of [`std::fs::create_dir_all`].
    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Returns `Ok(true)` if the path exists.
    ///
    /// It should replicate the behavior of [`std::fs::exists`].
    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool>;

    /// Calculates a checksum from the contents of a file.
    fn hash(&self, path: Utf8TypedPath<'_>, hasher: Box<dyn HashFunction>) -> io::Result<Checksum>;

    /// Queries the file system to get information about a file, directory, etc.
    ///
    /// It should replicate the behavior of [`std::fs::metadata`].
    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<Metadata>;

    /// Opens a file at `path` with the options specified by [`OpenOptions`]
    fn open(
        &self,
        path: Utf8TypedPath<'_>,
        options: &mut OpenOptions,
    ) -> io::Result<Box<dyn VfsFileStream>>;

    /// Reads the entire contents of a file into a byte vector.
    ///
    /// It should replicate the behavior of [`std::fs::read`].
    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>>;

    /// Returns an iterator over the entries within a directory.
    ///
    /// It should replicate the behavior of [`std::fs::read_dir`].
    fn read_dir(
        &self,
        path: Utf8TypedPath<'_>,
    ) -> io::Result<Box<dyn Iterator<Item = io::Result<Utf8TypedPathBuf>>>>;

    /// Reads the entire contents of a file into a string.
    ///
    /// It should replicate the behavior of [`std::fs::read_to_string`].
    fn read_to_string(&self, path: Utf8TypedPath<'_>) -> io::Result<String>;

    /// Renames a file or directory to a new name, replacing the original file
    /// if `to` already exists.
    ///
    /// It should replicate the behavior of [`std::fs::rename`].
    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Removes an empty file.
    ///
    /// It should replicate the behavior of [`std::fs::remove_file`].
    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Removes an empty directory.
    ///
    /// It should replicate the behavior of [`std::fs::remove_dir`].
    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Removes a directory at this path, after removing all of its contents.
    ///
    /// It should replicate the behavior of [`std::fs::remove_dir_all`].
    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Changes the permissions found on a file or a directory.
    ///
    /// It should replicate the behavior of [`std::fs::set_permissions`].
    fn set_permissions(&self, path: Utf8TypedPath<'_>, permissions: Permissions) -> io::Result<()>;

    /// Creates a new symbolic link on the file system.
    ///
    /// It should replicate the behavior of [`std::fs::soft_link`].
    fn soft_link(&self, original: Utf8TypedPath<'_>, link: Utf8TypedPath<'_>) -> io::Result<()>;

    /// Writes a slice as the entire contents of a file.
    ///
    /// This function will create a file if it does not exist, and will
    /// entirely replace its contents if it does.
    ///
    /// It should replicate the behavior of [`std::fs::write`].
    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()>;
}

pub trait VfsFileStream: io::Read + io::Write + io::Seek {
    /// Queries metadata about the underlying file.
    fn metadata(&self) -> io::Result<Metadata>;

    /// This function sychronizes file contents from the file system.
    fn sync_data(&mut self) -> io::Result<()>;
}

/// An OS-agnostic replica of [`std::fs::OpenOptions`] but all of its fields are public.
#[derive(Clone, Debug)]
#[must_use = "OpenOptions does nothing, use `.open(..)` to open a file"]
pub struct OpenOptions {
    pub append: bool,
    pub create: bool,
    pub create_new: bool,
    pub read: bool,
    pub truncate: bool,
    pub write: bool,
}

impl OpenOptions {
    #[expect(
        clippy::new_without_default,
        reason = "std::fs::OpenOptions does not implement Default"
    )]
    /// Creates a new set of options with all flags set to `false`.
    pub const fn new() -> Self {
        Self {
            append: false,
            create: false,
            create_new: false,
            read: false,
            truncate: false,
            write: false,
        }
    }

    /// Sets the option for append mode.
    pub const fn append(&mut self, append: bool) -> &mut Self {
        self.append = append;
        self
    }

    /// Sets the option to create a new file, or open it if it already exists.
    pub const fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    /// Sets the option to create a new file, failing if it already exists.
    pub const fn create_new(&mut self, create_new: bool) -> &mut Self {
        self.create_new = create_new;
        self
    }

    /// Sets the option for read access.
    pub const fn read(&mut self, read: bool) -> &mut Self {
        self.read = read;
        self
    }

    /// Sets the option to truncate the file to zero length on open.
    pub const fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    /// Sets the option for write access.
    pub const fn write(&mut self, write: bool) -> &mut Self {
        self.write = write;
        self
    }

    /// Opens a file at `path` with the options specified by `self`.
    pub fn open(
        &mut self,
        path: Utf8TypedPath<'_>,
        fs: &dyn FileSystem,
    ) -> io::Result<Box<dyn VfsFileStream>> {
        fs.open(path, self)
    }
}

/// A common file system node (file/directory) metadata.
#[derive(Debug)]
pub struct Metadata {
    /// A node permission mode.
    pub mode: Permissions,

    /// Node's total size in bytes (including subfiles).
    pub size: u64,

    /// A node type.
    pub ty: NodeType,
}

/// Represents the type of a filesystem node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NodeType {
    /// A regular directory
    Directory,

    /// A regular file
    File,

    /// A symbolic link
    Symlink,

    /// An unsupported node type.
    Unknown,
}

bitflags! {
    /// A file system node permission.
    #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
    pub struct Permissions: u16 {
        /// This node has read permission.
        const READ = 0b10;

        /// This node has write permission.
        const WRITE = 0b01;

        /// A default permission for every new node in some file system backends
        /// that shows it can be read and written.
        const READ_WRITE = Self::READ.bits() | Self::WRITE.bits();
    }
}

impl Permissions {
    /// Whether this node is readable.
    #[must_use]
    pub const fn can_read(&self) -> bool {
        self.contains(Self::READ)
    }

    /// Whether this node is writable.
    #[must_use]
    pub const fn can_write(&self) -> bool {
        self.contains(Self::WRITE)
    }
}

impl Default for Permissions {
    fn default() -> Self {
        Self::READ | Self::WRITE
    }
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        bitflags::parser::to_writer(&Permissions(self.0), f)
    }
}

use bitflags::bitflags;
use fat_checksum::{Checksum, HashFunction};
use std::{any::Any, fmt, io};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

pub type VfsFile = Box<dyn VfsFileStream>;
pub type VfsReadDir = Box<dyn Iterator<Item = io::Result<Box<dyn VfsDirEntry>>>>;

pub trait FileSystem: Any + Send + Sync + fmt::Debug {
    fn current_dir(&self) -> io::Result<Utf8TypedPathBuf>;

    fn create_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn create_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn exists(&self, path: Utf8TypedPath<'_>) -> io::Result<bool>;
    fn join(&self, path: Utf8TypedPath<'_>, name: &str) -> io::Result<Utf8TypedPathBuf>;

    /// Calculates a checksum from the contents of a file.
    fn hash(&self, path: Utf8TypedPath<'_>, hasher: Box<dyn HashFunction>) -> io::Result<Checksum>;

    fn metadata(&self, path: Utf8TypedPath<'_>) -> io::Result<VfsMetadata>;
    fn open(&self, path: Utf8TypedPath<'_>, options: &mut VfsOpenOptions) -> io::Result<VfsFile>;
    fn read(&self, path: Utf8TypedPath<'_>) -> io::Result<Vec<u8>>;
    fn read_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<VfsReadDir>;
    fn read_to_string(&self, path: Utf8TypedPath<'_>) -> io::Result<String>;
    fn rename(&self, from: Utf8TypedPath<'_>, to: Utf8TypedPath<'_>) -> io::Result<()>;
    fn remove_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn remove_dir_all(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn remove_file(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn set_current_dir(&self, path: Utf8TypedPath<'_>) -> io::Result<()>;
    fn soft_link(&self, original: Utf8TypedPath<'_>, link: Utf8TypedPath<'_>) -> io::Result<()>;
    fn walkdir(&self, root: Utf8TypedPath<'_>) -> io::Result<VfsReadDir>;
    fn write(&self, path: Utf8TypedPath<'_>, contents: &[u8]) -> io::Result<()>;
}

pub trait VfsDirEntry {
    fn depth(&self) -> usize;
    fn file_name(&self) -> io::Result<String>;

    /// Queries metadata about the underlying entry.
    fn metadata(&self) -> io::Result<VfsMetadata>;
    fn node_type(&self) -> io::Result<VfsNodeType>;
    fn path(&self) -> Utf8TypedPath<'_>;
}

pub trait VfsFileStream: io::Read + io::Write + io::Seek {
    /// Queries metadata about the underlying file.
    fn metadata(&self) -> io::Result<VfsMetadata>;

    /// This function sychronizes file contents from the file system.
    fn sync_data(&mut self) -> io::Result<()>;
}

/// An OS-agnostic replica of [`std::fs::OpenOptions`] but all of its fields are public.
#[derive(Clone, Debug)]
#[must_use = "OpenOptions does nothing, use `.open(..)` to open a file"]
pub struct VfsOpenOptions {
    pub append: bool,
    pub create: bool,
    pub create_new: bool,
    pub read: bool,
    pub truncate: bool,
    pub write: bool,
}

impl VfsOpenOptions {
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
pub struct VfsMetadata {
    /// A node permission mode.
    pub mode: Permissions,

    /// Node's total size in bytes (including subfiles).
    pub size: u64,

    /// A node type.
    pub ty: VfsNodeType,
}

/// Represents the type of a file system node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum VfsNodeType {
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
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

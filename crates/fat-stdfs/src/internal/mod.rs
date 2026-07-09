use fat_vfs::{VfsDirEntry, VfsFileStream, VfsMetadata, VfsNodeType};
use std::{fs, io};
use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

use crate::OsMetadataExt;

pub struct OsReadDir(pub(crate) fs::ReadDir);

impl Iterator for OsReadDir {
    type Item = io::Result<Box<dyn VfsDirEntry>>;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = match self.0.next()? {
            Ok(entry) => entry,
            Err(error) => return Some(Err(error)),
        };
        Some(Ok(OsDirEntry::boxed(entry)))
    }
}

pub struct OsWalkDirEntry {
    pub(crate) entry: walkdir::DirEntry,
    pub(crate) path: Utf8TypedPathBuf,
}

impl OsWalkDirEntry {
    #[must_use]
    pub(crate) fn boxed(entry: walkdir::DirEntry) -> Box<dyn VfsDirEntry> {
        let path = Utf8TypedPath::derive(&entry.path().to_string_lossy()).to_path_buf();
        Box::new(Self { entry, path })
    }
}

impl VfsDirEntry for OsWalkDirEntry {
    fn depth(&self) -> usize {
        self.entry.depth()
    }

    fn file_name(&self) -> io::Result<String> {
        let str = self.path.file_name().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidFilename, "path has no file name")
        })?;
        Ok(str.to_string())
    }

    fn metadata(&self) -> io::Result<VfsMetadata> {
        Ok(self.entry.metadata()?.to_vfs())
    }

    fn node_type(&self) -> io::Result<VfsNodeType> {
        Ok(file_type_to_vfs(self.entry.file_type()))
    }

    fn path(&self) -> Utf8TypedPath<'_> {
        self.path.to_path()
    }
}

pub struct OsDirEntry {
    pub(crate) entry: fs::DirEntry,
    pub(crate) path: Utf8TypedPathBuf,
}

impl OsDirEntry {
    #[must_use]
    pub(crate) fn boxed(entry: fs::DirEntry) -> Box<dyn VfsDirEntry> {
        let path = Utf8TypedPath::derive(&entry.path().to_string_lossy()).to_path_buf();
        Box::new(Self { entry, path })
    }
}

impl VfsDirEntry for OsDirEntry {
    fn depth(&self) -> usize {
        1
    }

    fn file_name(&self) -> io::Result<String> {
        self.entry.file_name().into_string().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidFilename,
                "file name not in valid UTF-8 string",
            )
        })
    }

    fn metadata(&self) -> io::Result<VfsMetadata> {
        Ok(self.entry.metadata()?.to_vfs())
    }

    fn node_type(&self) -> io::Result<VfsNodeType> {
        Ok(file_type_to_vfs(self.entry.file_type()?))
    }

    fn path(&self) -> Utf8TypedPath<'_> {
        self.path.to_path()
    }
}

pub struct OsFileHandle(pub(crate) fs::File);

impl VfsFileStream for OsFileHandle {
    fn metadata(&self) -> std::io::Result<VfsMetadata> {
        Ok(self.0.metadata()?.to_vfs())
    }

    fn sync_data(&mut self) -> std::io::Result<()> {
        self.0.sync_all()
    }
}

impl io::Seek for OsFileHandle {
    fn rewind(&mut self) -> io::Result<()> {
        self.0.rewind()
    }

    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.0.seek(pos)
    }

    fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
        self.0.seek_relative(offset)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        self.0.stream_position()
    }
}

impl io::Read for OsFileHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }

    fn read_vectored(&mut self, bufs: &mut [io::IoSliceMut<'_>]) -> io::Result<usize> {
        self.0.read_vectored(bufs)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.0.read_exact(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.0.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.0.read_to_string(buf)
    }
}

impl io::Write for OsFileHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.write_all(buf)
    }

    fn write_fmt(&mut self, args: std::fmt::Arguments<'_>) -> io::Result<()> {
        self.0.write_fmt(args)
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.0.write_vectored(bufs)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[must_use]
pub fn file_type_to_vfs(ft: fs::FileType) -> VfsNodeType {
    if ft.is_file() {
        VfsNodeType::File
    } else if ft.is_dir() {
        VfsNodeType::Directory
    } else if ft.is_symlink() {
        VfsNodeType::Symlink
    } else {
        VfsNodeType::Unknown
    }
}

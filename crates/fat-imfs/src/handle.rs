use fat_vfs::{VfsFileStream, VfsMetadata, VfsOpenOptions};
use std::{
    io::{self, Cursor},
    sync::Arc,
};

use crate::{ImfsInner, node::NodeId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenFileMode {
    Read,
    Write,
}

#[must_use = "handles do not do anything unless you read/write them"]
pub struct FileWriteHandle {
    cursor: Cursor<Vec<u8>>,
    inner: Arc<ImfsInner>,
    node_id: NodeId,
    readable: bool,
}

impl FileWriteHandle {
    pub fn new(
        inner: Arc<ImfsInner>,
        options: &mut VfsOpenOptions,
        node_id: NodeId,
    ) -> io::Result<Self> {
        debug_assert!(options.write, "write must be enabled for FileWriteHandle");

        let node = inner.nodes.get(node_id)?;
        let file = node.as_file()?;

        let content = file.read();
        let vec = if options.truncate {
            Vec::new()
        } else {
            let mut vec = Vec::new();
            vec.try_reserve(content.len())?;
            vec.extend_from_slice(&content);
            vec
        };

        let mut cursor = Cursor::new(vec);
        if options.append {
            let pos = cursor.get_ref().len() as u64;
            cursor.set_position(pos);
        }

        Ok(Self {
            cursor,
            inner,
            node_id,
            readable: options.read,
        })
    }

    fn check_for_reading(&self) -> io::Result<()> {
        if !self.readable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "handle not open for reading",
            ));
        }
        Ok(())
    }

    fn commit(&mut self) -> io::Result<()> {
        let node = self.inner.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.replace(self.cursor.get_ref())
    }
}

impl VfsFileStream for FileWriteHandle {
    fn metadata(&self) -> io::Result<VfsMetadata> {
        self.inner.metadata(self.node_id)
    }

    fn sync_data(&mut self) -> io::Result<()> {
        let node = self.inner.nodes.get(self.node_id)?;
        let content = node.as_file()?.read();
        let current_pos = self.cursor.position();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        let len = content.len() as u64;
        self.cursor = Cursor::new(vec);
        self.cursor.set_position(std::cmp::min(current_pos, len));

        Ok(())
    }
}

impl io::Read for FileWriteHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.check_for_reading()?;
        self.cursor.read(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.check_for_reading()?;
        self.cursor.read_exact(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.check_for_reading()?;
        self.cursor.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.check_for_reading()?;
        self.cursor.read_to_string(buf)
    }
}

impl io::Write for FileWriteHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.cursor.write(buf)
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.cursor.write_all(buf)
    }

    fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        self.cursor.write_vectored(bufs)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.commit()
    }
}

impl io::Seek for FileWriteHandle {
    fn rewind(&mut self) -> io::Result<()> {
        self.cursor.rewind()
    }

    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.cursor.seek(pos)
    }

    fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
        self.cursor.seek_relative(offset)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        self.cursor.stream_position()
    }
}

impl Drop for FileWriteHandle {
    fn drop(&mut self) {
        let _ = self.commit();
    }
}

// TODO: replicate the Cursor<Vec<u8>>
#[must_use = "handles do not do anything unless you read them"]
pub struct FileReadHandle {
    cursor: Cursor<Vec<u8>>,
    inner: Arc<ImfsInner>,
    node_id: NodeId,
}

impl FileReadHandle {
    pub fn new(inner: Arc<ImfsInner>, node_id: NodeId) -> io::Result<Self> {
        let node = inner.nodes.get(node_id)?;
        let file = node.as_file()?;
        let content = file.read();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        Ok(Self {
            cursor: Cursor::new(vec),
            inner,
            node_id,
        })
    }
}

impl VfsFileStream for FileReadHandle {
    fn metadata(&self) -> io::Result<VfsMetadata> {
        self.inner.metadata(self.node_id)
    }

    fn sync_data(&mut self) -> io::Result<()> {
        let node = self.inner.nodes.get(self.node_id)?;
        let content = node.as_file()?.read();
        let current_pos = self.cursor.position();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        let len = content.len() as u64;
        self.cursor = Cursor::new(vec);
        self.cursor.set_position(std::cmp::min(current_pos, len));

        Ok(())
    }
}

impl io::Read for FileReadHandle {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.cursor.read(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.cursor.read_exact(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.cursor.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.cursor.read_to_string(buf)
    }
}

impl io::Write for FileReadHandle {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handle not open for writing",
        ))
    }

    fn write_all(&mut self, _buf: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handle not open for writing",
        ))
    }

    fn write_vectored(&mut self, _bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handle not open for writing",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Seek for FileReadHandle {
    fn rewind(&mut self) -> io::Result<()> {
        self.cursor.rewind()
    }

    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.cursor.seek(pos)
    }

    fn seek_relative(&mut self, offset: i64) -> io::Result<()> {
        self.cursor.seek_relative(offset)
    }

    fn stream_position(&mut self) -> io::Result<u64> {
        self.cursor.stream_position()
    }
}

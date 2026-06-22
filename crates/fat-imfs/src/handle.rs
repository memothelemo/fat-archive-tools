use fat_vfs::VfsFileStream;
use std::{
    cell::Cell,
    io::{self, Cursor},
    sync::Arc,
};

use crate::{node::NodeId, private::ImfsInner};

/// Represents the locking state held by a file stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeldLock {
    /// No lock is held.
    None,
    /// A shared (read) lock is held.
    Shared,
    /// An exclusive (write) lock is held.
    Exclusive,
}

#[must_use = "writers do not do anything unless you write them"]
pub struct FileWriteHandle {
    cursor: Cursor<Vec<u8>>,
    fs: Arc<ImfsInner>,
    node_id: NodeId,
    held_lock: Cell<HeldLock>,
}

#[derive(Debug)]
pub enum WriteHandleMode {
    Standard,
    Append,
    Truncate,
}

impl FileWriteHandle {
    pub fn new(fs: Arc<ImfsInner>, node_id: NodeId, mode: WriteHandleMode) -> io::Result<Self> {
        let node = fs.nodes.get(node_id)?;
        let file = node.as_file()?;

        match mode {
            WriteHandleMode::Standard => {
                let content = file.read();

                let mut vec = Vec::new();
                vec.try_reserve(content.len())?;
                vec.extend_from_slice(&content);

                let cursor = Cursor::new(vec);

                Ok(Self {
                    cursor,
                    fs,
                    node_id,
                    held_lock: Cell::new(HeldLock::None),
                })
            }
            WriteHandleMode::Append => {
                let content = file.read();

                let mut vec = Vec::new();
                vec.try_reserve(content.len())?;
                vec.extend_from_slice(&content);

                let mut cursor = Cursor::new(vec);
                let pos = u64::try_from(content.len()).unwrap_or(0);
                cursor.set_position(pos);

                Ok(Self {
                    cursor,
                    fs,
                    node_id,
                    held_lock: Cell::new(HeldLock::None),
                })
            }
            WriteHandleMode::Truncate => Ok(Self {
                cursor: Cursor::new(Vec::new()),
                fs,
                node_id,
                held_lock: Cell::new(HeldLock::None),
            }),
        }
    }

    fn commit(&mut self) -> io::Result<()> {
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.replace(self.cursor.get_ref())
    }
}

impl Drop for FileWriteHandle {
    fn drop(&mut self) {
        let _ = self.unlock();
    }
}

impl VfsFileStream for FileWriteHandle {
    fn sync_data(&mut self) -> io::Result<()> {
        let node = self.fs.nodes.get(self.node_id)?;
        let content = node.as_file()?.read();

        let mut vec = Vec::new();
        vec.try_reserve(content.len())?;
        vec.extend_from_slice(&content);

        let current_pos = self.cursor.position();
        let mut cursor = Cursor::new(vec);
        let pos = std::cmp::min(current_pos, cursor.get_ref().len() as u64);
        cursor.set_position(pos);

        self.cursor = cursor;
        Ok(())
    }

    fn lock_shared(&self) -> io::Result<()> {
        if self.held_lock.get() != HeldLock::None {
            return Err(io::Error::new(io::ErrorKind::Other, "already holding a lock on this handle"));
        }
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.lock_shared()?;
        self.held_lock.set(HeldLock::Shared);
        Ok(())
    }

    fn lock_exclusive(&self) -> io::Result<()> {
        if self.held_lock.get() != HeldLock::None {
            return Err(io::Error::new(io::ErrorKind::Other, "already holding a lock on this handle"));
        }
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.lock_exclusive()?;
        self.held_lock.set(HeldLock::Exclusive);
        Ok(())
    }

    fn try_lock_shared(&self) -> io::Result<()> {
        if self.held_lock.get() != HeldLock::None {
            return Err(io::Error::new(io::ErrorKind::Other, "already holding a lock on this handle"));
        }
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.try_lock_shared()?;
        self.held_lock.set(HeldLock::Shared);
        Ok(())
    }

    fn try_lock_exclusive(&self) -> io::Result<()> {
        if self.held_lock.get() != HeldLock::None {
            return Err(io::Error::new(io::ErrorKind::Other, "already holding a lock on this handle"));
        }
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        file.try_lock_exclusive()?;
        self.held_lock.set(HeldLock::Exclusive);
        Ok(())
    }

    fn unlock(&self) -> io::Result<()> {
        let node = self.fs.nodes.get(self.node_id)?;
        let file = node.as_file()?;
        match self.held_lock.get() {
            HeldLock::Shared => {
                file.unlock_shared()?;
                self.held_lock.set(HeldLock::None);
            }
            HeldLock::Exclusive => {
                file.unlock_exclusive()?;
                self.held_lock.set(HeldLock::None);
            }
            HeldLock::None => {}
        }
        Ok(())
    }
}

impl io::Read for FileWriteHandle {
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

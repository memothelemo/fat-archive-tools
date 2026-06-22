use crate::VfsFileStream;
use fs2::FileExt;

impl VfsFileStream for std::fs::File {
    fn sync_data(&mut self) -> std::io::Result<()> {
        std::fs::File::sync_all(self)
    }

    fn lock_shared(&self) -> std::io::Result<()> {
        FileExt::lock_shared(self)
    }

    fn lock_exclusive(&self) -> std::io::Result<()> {
        FileExt::lock_exclusive(self)
    }

    fn try_lock_shared(&self) -> std::io::Result<()> {
        FileExt::try_lock_shared(self)
    }

    fn try_lock_exclusive(&self) -> std::io::Result<()> {
        FileExt::try_lock_exclusive(self)
    }

    fn unlock(&self) -> std::io::Result<()> {
        FileExt::unlock(self)
    }
}

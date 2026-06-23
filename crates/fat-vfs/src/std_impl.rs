use crate::VfsFileStream;

impl VfsFileStream for std::fs::File {
    fn sync_data(&mut self) -> std::io::Result<()> {
        std::fs::File::sync_all(self)
    }
}

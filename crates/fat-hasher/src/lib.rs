pub mod checksum;
pub use self::checksum::Checksum;

mod sha256;
mod xxh3;

pub use ::sha2::Sha256;
pub use ::xxhash_rust::xxh3::Xxh3;

/// A pluggable interface for streaming hash computation.
pub trait HashFunction {
    /// Creates a new default hash function.
    fn new() -> Self
    where
        Self: Sized;

    /// Finalizes the hashing and generates a checksum based on the provided data.
    fn digest(&mut self) -> Checksum;

    /// Adds chunk of data to hash.
    fn update(&mut self, bytes: &[u8]);

    /// Resets the hash function's state
    fn reset(&mut self);
}

impl std::io::Write for Box<dyn HashFunction> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

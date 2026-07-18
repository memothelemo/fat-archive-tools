use std::{fmt, ops::Deref};
use thiserror::Error;

mod blake3;
mod sha256;
mod xxh3;

pub use ::blake3::Hasher as Blake3;
pub use ::sha2::Sha256;
pub use ::xxhash_rust::xxh3::Xxh3;

/// A finalized hash digest, wrapping raw bytes with hex encoding support.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Checksum(Vec<u8>);

impl Checksum {
    /// Creates a new checksum from raw bytes.
    ///
    /// Returns `Err(EmptyChecksum)` if the byte slice is empty.
    pub fn new<B>(bytes: B) -> Result<Self, EmptyChecksum>
    where
        B: Into<Vec<u8>>,
    {
        let bytes = bytes.into();
        if bytes.is_empty() {
            Err(EmptyChecksum)
        } else {
            Ok(Self(bytes.to_vec()))
        }
    }

    /// Returns the hex-encoded string representation of the checksum.
    #[must_use]
    pub fn encode(&self) -> String {
        hex::encode(&self.0)
    }

    /// Returns the length in bytes of the raw checksum.
    #[allow(clippy::len_without_is_empty)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl Deref for Checksum {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Error returned when attempting to create a checksum from empty bytes.
#[derive(Debug, Error)]
#[error("checksums should not be empty")]
pub struct EmptyChecksum;

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

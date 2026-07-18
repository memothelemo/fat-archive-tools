pub use blake3::Hasher as Blake3;

use crate::{Checksum, HashFunction};

impl HashFunction for Blake3 {
    fn new() -> Self
    where
        Self: Sized,
    {
        Blake3::new()
    }

    fn digest(&mut self) -> Checksum {
        Checksum::new(self.clone().finalize().as_bytes()).expect("Blake3 should provide checksum")
    }

    fn update(&mut self, bytes: &[u8]) {
        Blake3::update(self, bytes);
    }

    fn reset(&mut self) {
        Blake3::reset(self);
    }
}

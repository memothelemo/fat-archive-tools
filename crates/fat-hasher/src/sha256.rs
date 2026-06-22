use sha2::{Digest, Sha256};

use crate::{Checksum, HashFunction};

impl HashFunction for Sha256 {
    fn new() -> Self
    where
        Self: Sized,
    {
        <Sha256 as Digest>::new()
    }

    fn digest(&mut self) -> Checksum {
        Checksum::new(self.clone().finalize().to_vec())
            .expect("SHA256 should provide exactly 32 bytes")
    }

    fn update(&mut self, bytes: &[u8]) {
        <Sha256 as Digest>::update(self, bytes);
    }

    fn reset(&mut self) {
        <Sha256 as Digest>::reset(self);
    }
}

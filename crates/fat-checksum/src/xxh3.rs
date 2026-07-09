use xxhash_rust::xxh3::Xxh3;

use crate::{Checksum, HashFunction};

impl HashFunction for Xxh3 {
    fn new() -> Self
    where
        Self: Sized,
    {
        Xxh3::new()
    }

    fn digest(&mut self) -> Checksum {
        Checksum::new(Xxh3::digest(self).to_be_bytes())
            .expect("u64::to_be_bytes should provide exactly 4 bytes")
    }

    fn update(&mut self, bytes: &[u8]) {
        Xxh3::update(self, bytes);
    }

    fn reset(&mut self) {
        Xxh3::reset(self);
    }
}

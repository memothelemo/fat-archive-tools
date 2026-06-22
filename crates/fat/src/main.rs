use fat_hasher::{Checksum, HashFunction, Sha256};
use fat_stdfs::OsFileSystem;
use fat_vfs::FileSystem;
use std::io;

fn hash_cargo_toml(fs: &dyn FileSystem) -> io::Result<Checksum> {
    fs.hash("Cargo.toml".into(), Box::new(Sha256::new()))
}

fn main() {
    let fs = OsFileSystem::new();
    let checksum = hash_cargo_toml(&fs).unwrap();
    println!("Cargo.toml: {checksum}");
}

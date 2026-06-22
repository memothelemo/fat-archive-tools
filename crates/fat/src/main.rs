use fat_hasher::{Checksum, HashFunction, Sha256};
use fat_imfs::InMemoryFs;
use fat_vfs::FileSystem;
use std::io;

fn hash_sample_file(fs: &dyn FileSystem) -> io::Result<Checksum> {
    fs.hash("/etc/sshd/sshd_config".into(), Box::new(Sha256::new()))
}

fn main() {
    let fs = InMemoryFs::new();
    fs.create_dir_all("/etc/sshd".into()).unwrap();
    fs.write("/etc/sshd/sshd_config".into(), b"Hello").unwrap();

    let checksum = hash_sample_file(&fs).unwrap();
    println!("hash: {checksum}");
}

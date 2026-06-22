use fat_imfs::InMemoryFs;
use fat_vfs::{FileSystem, OpenOptions};
use std::io;

fn main() -> io::Result<()> {
    let fs = InMemoryFs::new();

    let mut handle = fs.open(
        "/test.txt".into(),
        OpenOptions::new().create(true).write(true),
    )?;

    handle.write_all(b"Hello, World!")?;
    handle.sync_data()?;
    handle.flush()?;

    println!("{fs:#?}");

    Ok(())
}

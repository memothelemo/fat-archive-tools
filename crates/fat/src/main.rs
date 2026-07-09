use fat::{WalkConfig, walk};
use fat_stdfs::OsFileSystem;
use std::sync::Arc;

fn main() {
    let fs = Arc::new(OsFileSystem::new());
    let config = WalkConfig::new("/".into());
    let result = walk(fs, &config);
    println!("{} entries scanned", result.entries_scanned);
}

fn main() {}

// use fat::{WalkConfig, walk};
// use fat_imfs::InMemoryFs;
// use fat_stdfs::OsFileSystem;
// use fat_vfs::FileSystem;
// use fat_vfs_harness::{create_balanced_tree, create_deep_tree, create_wide_tree};
// use std::{sync::Arc, time::Instant};
// use typed_path::Utf8TypedPath;

// fn main() {
//     let fs = InMemoryFs::new();
//     let now = Instant::now();
//     create_deep_tree(&fs, Utf8TypedPath::derive("/"), 50, 100);
//     // create_wide_tree(&fs, Utf8TypedPath::derive("/"), 50000, 50000);

//     let elapsed = now.elapsed();
//     println!(
//         "{:?}",
//         fs.read_dir("/d".into())
//             .unwrap()
//             .map(|v| v.unwrap().path().to_string())
//             .collect::<Vec<_>>()
//     );

//     println!("done ({elapsed:.2?}, nodes={})", fs.nodes());
//     // let fs = Arc::new(OsFileSystem::new());
//     // let config = WalkConfig::new("/".into());
//     // let result = walk(fs, &config);
//     // println!("{} entries scanned", result.entries_scanned);
// }

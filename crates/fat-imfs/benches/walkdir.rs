use divan::{Bencher, black_box};
use fat_imfs::InMemoryFs;
use fat_vfs::FileSystem;
use fat_vfs_harness::{create_balanced_tree, create_wide_tree};
use typed_path::Utf8TypedPath;

#[divan::bench(
    // first: 1,001 nodes, second: 350,001 nodes
    args = [(400, 100), (50000, 50000)],
    max_time = 3,
)]
fn walkdir_wide_tree(bencher: Bencher, (total_files, total_dirs): (usize, usize)) {
    let fs = InMemoryFs::new();
    let path = Utf8TypedPath::derive("/");
    create_wide_tree(&fs, path, total_files, total_dirs);

    bencher
        .with_inputs(|| Utf8TypedPath::derive("/"))
        .bench_values(|root| {
            let iter = black_box(fs.walkdir(root)).unwrap();
            for entry in iter {
                _ = black_box(entry.unwrap());
            }
        });
}

#[divan::bench(
    // first: 466 nodes, second: 58,591 nodes
    args = [(2, 5, 10), (6, 5, 10)],
    max_time = 3,
)]
fn walkdir_balanced_tree(
    bencher: Bencher,
    (depth, dirs_per_level, files_per_dir): (usize, usize, usize),
) {
    let fs = InMemoryFs::new();
    let path = Utf8TypedPath::derive("/");
    create_balanced_tree(&fs, path, depth, dirs_per_level, files_per_dir);

    bencher
        .with_inputs(|| Utf8TypedPath::derive("/"))
        .bench_values(|root| {
            let iter = black_box(fs.walkdir(root)).unwrap();
            for entry in iter {
                _ = black_box(entry.unwrap());
            }
        });
}

#[divan::bench(args = ["default"])]
fn walkdir_init(bencher: Bencher, _: &str) {
    let fs = InMemoryFs::new();
    bencher.bench(|| {
        _ = black_box(fs.walkdir("/".into())).unwrap();
    });
}

fn main() {
    divan::main();
}

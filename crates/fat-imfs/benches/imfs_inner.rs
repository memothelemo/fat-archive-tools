use divan::Bencher;
use fat_imfs::{ImfsNodeOperation, InMemoryFs};
use fat_vfs_harness::create_deep_tree;
use std::{hint::black_box, sync::LazyLock};
use typed_path::Utf8UnixPath;

static TEST_DEEP_DIRS: &[&str] = &[
    // depth 0
    "/f0",
    // depth 1
    "/d/f0",
    // depth 5
    "/d/d/d/d/d/f0",
    // depth 10
    "/d/d/d/d/d/d/d/d/d/f0",
];

#[divan::bench(args = TEST_DEEP_DIRS)]
fn check_perms(bencher: Bencher, path: &str) {
    bencher
        .with_inputs(|| Utf8UnixPath::new(path))
        .bench_values(|path| {
            let node_id = DEEP_FS_TREE.inner().find_node_id(path).unwrap();
            let result = DEEP_FS_TREE
                .inner()
                .check_perms(node_id, ImfsNodeOperation::Read);

            black_box(result).unwrap();
        });
}

#[divan::bench(args = TEST_DEEP_DIRS)]
fn find_node(bencher: Bencher, path: &str) {
    bencher
        .with_inputs(|| Utf8UnixPath::new(path))
        .bench_values(|path| {
            black_box(DEEP_FS_TREE.inner().find_node(path)).unwrap();
        });
}

#[divan::bench(args = TEST_DEEP_DIRS)]
fn find_node_id(bencher: Bencher, path: &str) {
    bencher
        .with_inputs(|| Utf8UnixPath::new(path))
        .bench_values(|path| {
            black_box(DEEP_FS_TREE.inner().find_node_id(path)).unwrap();
        });
}

#[divan::bench(args = &[
    "C:\\",
    "C:\\Users\\user",
    "/",
    "/nested",
    "/nested/2"
])]
fn normalize(bencher: Bencher, path: &str) {
    bencher.bench(|| {
        black_box(DEEP_FS_TREE.inner().normalize(path.into())).unwrap();
    });
}

static DEEP_FS_TREE: LazyLock<InMemoryFs> = LazyLock::new(|| {
    let fs = InMemoryFs::new();
    create_deep_tree(&fs, "/".into(), 10, 1);
    fs
});

fn main() {
    divan::main();
}

use fat_vfs::{FileSystem, VfsOpenOptions};
use typed_path::Utf8TypedPath;

pub fn create_balanced_tree(
    fs: &dyn FileSystem,
    path: Utf8TypedPath<'_>,
    depth: usize,
    dirs_per_level: usize,
    files_per_dir: usize,
) {
    if depth == 0 {
        return;
    }

    for file_idx in 0..files_per_dir {
        let file_path = path.join(format!("f{file_idx}"));
        fs.write(file_path.to_path(), b"").unwrap();
    }

    for dir_idx in 0..dirs_per_level {
        let dir_path = path.join(format!("d{dir_idx}"));
        fs.create_dir(dir_path.to_path()).unwrap();

        create_balanced_tree(
            fs,
            dir_path.to_path(),
            depth - 1,
            dirs_per_level,
            files_per_dir,
        );
    }
}

pub fn create_deep_tree(
    fs: &dyn FileSystem,
    path: Utf8TypedPath<'_>,
    depth: usize,
    files_per_level: usize,
) {
    let mut current_dir = path.to_path_buf();
    for level in 0..depth {
        for file_idx in 0..files_per_level {
            let path = current_dir.to_path().join(format!("f{file_idx}"));
            VfsOpenOptions::new()
                .create(true)
                .write(true)
                .open(path.to_path(), fs)
                .unwrap();
        }

        if level < depth - 1 {
            let next_dir = current_dir.join("d");
            fs.create_dir(next_dir.to_path()).unwrap();
            current_dir = next_dir;
        }
    }
}

pub fn create_wide_tree(
    fs: &dyn FileSystem,
    path: Utf8TypedPath<'_>,
    total_files: usize,
    total_dirs: usize,
) {
    for file_idx in 0..total_files {
        let path = path.join(format!("f{file_idx}"));
        VfsOpenOptions::new()
            .create(true)
            .write(true)
            .open(path.to_path(), fs)
            .unwrap();
    }

    for dir_idx in 0..total_dirs {
        let path = path.join(format!("d{dir_idx}"));
        fs.create_dir(path.to_path()).unwrap();

        for file_idx in 0..5 {
            let path = path.join(format!("f{file_idx}"));
            VfsOpenOptions::new()
                .create(true)
                .write(true)
                .open(path.to_path(), fs)
                .unwrap();
        }
    }
}

use crossbeam::atomic::AtomicCell;
use dashmap::DashMap;
use fat_checksum::{Blake3, Checksum, HashFunction};
use fat_parallel_iter::ParallelIter;
use fat_walkdir::{DirEntry, WalkVisitor, Walker, WalkerAction};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct SimpleVisitor {
    lengths: DashMap<u64, Vec<PathBuf>>,
    progress: Arc<AtomicUsize>,
}

impl WalkVisitor for SimpleVisitor {
    fn visit(&self, entry: io::Result<DirEntry>) -> WalkerAction {
        let Ok(entry) = entry else {
            return WalkerAction::Continue;
        };

        let Ok(metadata) = entry.metadata() else {
            return WalkerAction::Continue;
        };

        if !metadata.is_file() {
            if let Some(name) = entry.file_name().map(|v| v.to_string_lossy())
                && (name.starts_with(".") || name == "node_modules")
            {
                return WalkerAction::Skip;
            }
            return WalkerAction::Continue;
        }

        self.progress.fetch_add(1, Ordering::Relaxed);
        self.lengths
            .entry(metadata.len())
            .or_default()
            .push(entry.into_path());

        WalkerAction::Continue
    }
}

// This function tries to compare between two possibly duplicated
// based on the following actions:
// - First 16 bytes of each files
// - The contents of each file
fn fast_compare_files(paths: Vec<PathBuf>) -> io::Result<HashMap<Checksum, Vec<PathBuf>>> {
    // Phase 1: Group by first-16-bytes hash
    let mut groups: HashMap<Checksum, Vec<(PathBuf, fs::File, Blake3)>> = HashMap::new();
    {
        let mut buf = [0u8; 16];
        for path in paths {
            let mut file = fs::OpenOptions::new().read(true).open(&path)?;
            let read = file.read(&mut buf)?;

            let mut hasher = Blake3::new();
            hasher.update(&buf[..read]);

            let partial_checksum = hasher.digest();
            groups
                .entry(partial_checksum)
                .or_default()
                .push((path, file, hasher));
        }
    }

    // Phase 2: Full hash only within prefix-collision groups
    //
    // TODO: Try to compare files through 1 KB sequences
    let mut duplicates = HashMap::<Checksum, Vec<PathBuf>>::new();
    for (_, group) in groups {
        // Skip groups with only one file, they are guaranteed unique
        if group.len() < 2 {
            continue;
        }

        for (path, mut file, mut hasher) in group {
            // Resume reading from byte 17 onward (first 16 already read)
            let mut content = Vec::new();
            file.read_to_end(&mut content)?;
            hasher.update(&content);

            let full_checksum = hasher.digest();
            duplicates.entry(full_checksum).or_default().push(path);
        }
    }

    // Retain only actual duplicates (2+ files sharing the same hash)
    duplicates.retain(|_, paths| paths.len() >= 2);
    Ok(duplicates)
}

#[derive(Clone, Copy)]
enum Task {
    None,
    Walking,
    Comparing,
}

fn main() {
    let current_task = Arc::new(AtomicCell::new(Task::Walking));
    let progress = Arc::new(AtomicUsize::new(0));

    let visitor = Arc::new(SimpleVisitor {
        progress: progress.clone(),
        lengths: DashMap::new(),
    });

    std::thread::spawn({
        let current_task = current_task.clone();
        let progress = progress.clone();
        move || {
            loop {
                if !matches!(current_task.load(), Task::None) {
                    let progress = progress.load(Ordering::Relaxed);
                    println!("{progress} file(s) processed");
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });

    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .try_into()
        .unwrap();

    let path = std::env::var("WALKER_ROOT").unwrap_or_else(|_| ".".to_string());
    println!("walking all directories (path={path:?})");

    let now = Instant::now();
    Walker::new(path).threads(threads).visit(&visitor);

    let elapsed = now.elapsed();
    println!(
        "[size stage] gathered {} file(s) ({} entries; {elapsed:.2?})",
        visitor.progress.load(Ordering::Relaxed),
        visitor.lengths.len()
    );

    current_task.store(Task::None);
    progress.store(0, Ordering::Relaxed);

    // Work against with other paths if possible
    let visitor = Arc::into_inner(visitor).unwrap();
    let same_sized_files = visitor
        .lengths
        .into_iter()
        .filter_map(|(_, v)| (v.len() > 1).then_some(v))
        .collect::<Vec<_>>();

    println!(
        "[partial hash stage] comparing files' checksums ({})",
        same_sized_files
            .iter()
            .fold(0usize, |acc, v| v.len().wrapping_add(acc))
    );

    let now = Instant::now();
    let duplicated_files = Arc::new(RwLock::new(Vec::new()));
    current_task.store(Task::Comparing);

    ParallelIter::new(same_sized_files.into_iter())
        .threads(threads)
        .run({
            let duplicated_files = duplicated_files.clone();
            let progress = progress.clone();
            move |paths, _, _| {
                let path_count = paths.len();
                let map = match fast_compare_files(paths) {
                    Ok(maps) => maps,
                    Err(error) => {
                        eprintln!("could not compare {path_count} conflicting file(s): {error:?}");
                        return;
                    }
                };

                let mut ptr = duplicated_files.write().unwrap();
                ptr.extend(map.into_values());
                progress.fetch_add(path_count, Ordering::Relaxed);
            }
        });

    let duplicated_files = RwLock::into_inner(Arc::into_inner(duplicated_files).unwrap()).unwrap();
    let elapsed = now.elapsed();

    println!(
        "[hash stage] found {} duplicated file entries",
        duplicated_files.len()
    );
    println!("[hash stage] done ({elapsed:.2?})");
}

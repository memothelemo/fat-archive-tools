use fat_vfs::{FileSystem, VfsNodeType};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use typed_path::Utf8TypedPathBuf;

/// Configuration for the parallel directory walker.
pub struct WalkConfig {
    /// Number of worker threads to spawn.
    /// Defaults to the number of available CPUs.
    pub num_threads: usize,

    /// Root path to start walking from.
    pub root: Utf8TypedPathBuf,
}

impl WalkConfig {
    /// Creates a new configuration with the given root path and
    /// a thread count based on available parallelism.
    #[must_use]
    pub fn new(root: Utf8TypedPathBuf) -> Self {
        let num_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        Self { num_threads, root }
    }
}

/// Result of a completed directory walk.
pub struct WalkResult {
    /// Total number of entries (files + directories) discovered.
    pub entries_scanned: usize,
}

/// Walks a directory tree in parallel using a work-stealing channel pattern.
///
/// Workers pull directory paths from a shared channel, list their contents,
/// and push discovered subdirectories back for other workers to process.
/// An atomic in-flight counter tracks outstanding work so the walker can
/// detect true completion without polling or sleep hacks.
pub fn walk(fs: Arc<dyn FileSystem>, config: &WalkConfig) -> WalkResult {
    let (tx, rx) = crossbeam::channel::unbounded::<Option<Utf8TypedPathBuf>>();

    let count = Arc::new(AtomicUsize::new(0));

    // Tracks how many directory listings are still in-flight or queued.
    // Incremented before sending work, decremented when a worker finishes
    // processing a directory. When this reaches zero, all work is done.
    let in_flight = Arc::new(AtomicUsize::new(0));

    // Seed the queue with the root directory's immediate children.
    match fs.read_dir(config.root.to_path()) {
        Ok(iter) => {
            for entry in iter {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        eprintln!("ERROR: {error}");
                        continue;
                    }
                };

                match entry.node_type() {
                    Ok(VfsNodeType::Directory) => {
                        in_flight.fetch_add(1, Ordering::AcqRel);
                        _ = tx.send(Some(entry.path().to_path_buf()));
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("ERROR: {error}"),
                }
            }
        }
        Err(error) => {
            eprintln!("ERROR at {}: {error}", config.root);
            return WalkResult { entries_scanned: 0 };
        }
    }

    // If there were no subdirectories under root, we're already done.
    if in_flight.load(Ordering::Acquire) == 0 {
        return WalkResult {
            entries_scanned: count.load(Ordering::Acquire),
        };
    }

    crossbeam::scope(|scope| {
        for _ in 0..config.num_threads {
            let count = &count;
            let fs = fs.clone();
            let tx = tx.clone();
            let rx = rx.clone();
            let in_flight = &in_flight;

            scope.spawn(move |_| {
                let process_dir = |tx: &crossbeam::channel::Sender<Option<Utf8TypedPathBuf>>,
                                   path: Utf8TypedPathBuf| {
                    let iter = match fs.read_dir(path.to_path()) {
                        Ok(iter) => iter,
                        Err(error) => {
                            eprintln!("ERROR at {path}: {error}");
                            return;
                        }
                    };

                    for entry in iter {
                        let entry = match entry {
                            Ok(entry) => entry,
                            Err(error) => {
                                eprintln!("ERROR: {error}");
                                continue;
                            }
                        };

                        count.fetch_add(1, Ordering::Relaxed);

                        match entry.node_type() {
                            Ok(VfsNodeType::Directory) => {
                                in_flight.fetch_add(1, Ordering::AcqRel);
                                _ = tx.send(Some(entry.path().to_path_buf()));
                            }
                            Ok(_) => {}
                            Err(error) => eprintln!("ERROR: {error}"),
                        }
                    }
                };

                while let Ok(msg) = rx.recv() {
                    match msg {
                        Some(path) => {
                            process_dir(&tx, path);

                            // This directory is fully processed (or failed).
                            // Decrement the in-flight counter; if we were the
                            // last, send None to trigger shutdown propagation.
                            if in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
                                _ = tx.send(None);
                                return;
                            }
                        }
                        None => {
                            // Propagate the shutdown signal to the next worker.
                            _ = tx.send(None);
                            return;
                        }
                    }
                }
            });
        }

        // Drop the main thread's sender so workers can terminate
        // once all work is done.
        drop(tx);
    })
    .expect("worker thread panicked");

    WalkResult {
        entries_scanned: count.load(Ordering::Acquire),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fat_imfs::InMemoryFs;

    #[test]
    fn test_walker() {
        let fs = Arc::new(InMemoryFs::new());
        // Create a basic directory structure:
        // /root
        // /root/dir1
        // /root/dir1/file1
        // /root/dir2
        // /root/dir2/file2
        fs.create_dir_all("/root/dir1".into()).unwrap();
        fs.create_dir_all("/root/dir2".into()).unwrap();
        fs.write("/root/dir1/file1".into(), b"hello").unwrap();
        fs.write("/root/dir2/file2".into(), b"world").unwrap();

        let config = WalkConfig::new("/root".into());
        let result = walk(fs, &config);

        // Inside the worker threads, we read /root/dir1 and /root/dir2.
        // We find /root/dir1/file1 and /root/dir2/file2, and increment the count.
        assert_eq!(result.entries_scanned, 2);
    }
}


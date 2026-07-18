use fat_parallel_iter::{ParallelIter, WorkerQueue};
use std::{
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

pub mod entry;

pub use self::entry::DirEntry;
#[cfg(unix)]
pub use self::entry::DirEntryExt;

/// Parallelized directory walker.
///
/// It scans directory trees recursively and calls a [`WalkVisitor`] callback
/// with the discovered directory entries.
pub struct Walker {
    paths: Vec<PathBuf>,
    threads: NonZeroUsize,
}

impl Walker {
    /// Creates a new `Walker` initialized with a single root path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self::from_iter([path])
    }

    /// Creates a new, empty `Walker` with no root paths.
    pub fn empty() -> Self {
        Self {
            paths: vec![],
            threads: NonZeroUsize::new(1).expect("one is not zero"),
        }
    }

    /// Adds a root path to the walker.
    #[expect(
        clippy::should_implement_trait,
        reason = "this function is for configuring a directory walker"
    )]
    pub fn add<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.paths.push(path.as_ref().to_path_buf());
        self
    }

    /// Sets the number of threads to use for walking.
    pub fn threads(&mut self, threads: NonZeroUsize) -> &mut Self {
        self.threads = threads;
        self
    }

    /// Recursively visits all paths, invoking the `visitor` callback for each entry found.
    ///
    /// The visitation is parallelized across the configured thread pool.
    pub fn visit<V: WalkerVisitor + Clone>(&mut self, visitor: &V) {
        // Initial queue for paths to descend.
        let mut messages = Vec::new();
        let mut paths = Vec::new();
        std::mem::swap(&mut self.paths, &mut paths);

        for path in paths {
            let entry = match DirEntry::from_path(0, path.to_path_buf()) {
                Ok(entry) => entry,
                Err(error) => {
                    if visitor.visit(Err(error)).is_quit() {
                        return;
                    }
                    continue;
                }
            };
            messages.push(entry);
        }

        if messages.is_empty() {
            return;
        }

        // Create the workers and then wait for them to finish
        ParallelIter::new(messages.into_iter())
            .threads(self.threads)
            .run(|entry, queue, abort| {
                let action = match visit_entry(queue, visitor, entry) {
                    Ok(action) => action,
                    Err(error) => visitor.visit(Err(error)),
                };

                if action.is_quit() {
                    abort.store(true);
                }
            });
    }
}

impl<P: AsRef<Path>> FromIterator<P> for Walker {
    fn from_iter<T: IntoIterator<Item = P>>(iter: T) -> Self {
        let mut walker = Self::empty();
        for path in iter.into_iter() {
            walker = walker.add(path);
        }
        walker
    }
}

/// A trait representing a visitor for directory walking.
pub trait WalkerVisitor: Send + Sync {
    /// Visits a directory entry, or handles an I/O error encountered during traversal.
    ///
    /// Returns a [`WalkerAction`] to control the future execution of the directory walker.
    fn visit(&self, entry: io::Result<DirEntry>) -> WalkerAction;
}

impl<T> WalkerVisitor for std::sync::Arc<T>
where
    T: WalkerVisitor,
{
    fn visit(&self, entry: io::Result<DirEntry>) -> WalkerAction {
        (**self).visit(entry)
    }
}

/// Actions returned by [`WalkVisitor::visit`] to control directory traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WalkerAction {
    /// Continue walking as normal.
    Continue,

    /// Do not descend if the given directory entry is a directory,
    /// otherwise this has no effect.
    Skip,

    /// Abort the iterator as soon as possible.
    Quit,
}

impl WalkerAction {
    /// Returns `true` if the action is [`Self::Continue`].
    #[must_use]
    pub const fn is_continue(&self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Returns `true` if the action is [`Self::Skip`].
    #[must_use]
    pub const fn is_skip(&self) -> bool {
        matches!(self, Self::Skip)
    }

    /// Returns `true` if the action is [`Self::Quit`].
    #[must_use]
    pub const fn is_quit(&self) -> bool {
        matches!(self, Self::Quit)
    }
}

fn visit_entry<V: WalkerVisitor>(
    queue: &WorkerQueue<DirEntry>,
    visitor: &V,
    entry: DirEntry,
) -> io::Result<WalkerAction> {
    let state = visitor.visit(Ok(entry.clone()));
    if state.is_quit() {
        return Ok(WalkerAction::Quit);
    }

    // We can queue more entries if the current entry is a directory
    if entry.file_type().is_file() {
        return Ok(WalkerAction::Continue);
    }

    if state.is_skip() {
        return Ok(WalkerAction::Continue);
    }

    let next_depth = entry.depth().wrapping_add(1);
    let entries = fs::read_dir(entry.path())?;
    for entry in entries {
        let entry = entry.and_then(|entry| DirEntry::from_entry(next_depth, &entry))?;
        queue.push(entry);
    }

    Ok(WalkerAction::Continue)
}

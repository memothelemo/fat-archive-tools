use fat_parallel_iter::{ParallelIter, WorkerQueue};
use std::{
    fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

mod entry;
pub use self::entry::DirEntry;
#[cfg(unix)]
pub use self::entry::DirEntryExt;

/// A configurable builder for traversing a directory tree
/// using multiple threads in parallel.
///
/// # Differences with [`walkdir::WalkDir`]
/// - It does not follow or read symbolic links.
/// - It utilizes the [visitor pattern], where each entry
///   or error is passed to the [visitor], which returns a
///   [`WalkerAction`] to guide the waker on what to do next.
/// - It traverses directories across multiple threads, allowing
///   for faster traversals on SSDs.
///
/// # Example
///
/// It counts all nested entries within a directory:
/// ```no_run,rust
/// # use fat_walkdir::{DirEntry, Walker, WalkerAction};
/// # use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
/// #
/// struct Counter(Arc<AtomicUsize>);
///
/// impl WalkerVisitor for Counter {
///     fn visit(&self, entry: io::Result<DirEntry>) -> WalkerAction {
///         if entry.is_ok() {
///             self.0.fetch_add(1, Ordering::Relaxed);
///         }
///         WalkerAction::Continue
///     }
/// }
///
/// # fn try_main() -> std::io::Result<()> {
/// # let visitor = Counter(Default::default());
/// Walker::new("/tmp")
///     .threads(NonZeroUsize::new(4).unwrap())
///     .visit(&visitor);
///
/// println!("Found {} entries", visitor.load(Ordering::Relaxed));
/// # }
/// ```
///
/// [`walkdir::WalkDir`]: https://docs.rs/walkdir/latest/walkdir/struct.WalkDir.html
/// [visitor]: WalkerVisitor::visit
/// [visitor pattern]: WalkerVisitor
#[must_use = "Walker does nothing unless you call `visit`"]
pub struct Walker {
    paths: Vec<PathBuf>,
    threads: NonZeroUsize,
}

impl Walker {
    /// Creates a new [`Walker`] initialized with a single root path.
    ///
    /// It defaults to a single thread utilized for the walker.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self::from_iter([path])
    }

    /// Creates a new, empty [`Walker`] with no root paths.
    ///
    /// It defaults to a single thread utilized for the walker.
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

    /// Sets the number of threads to use for the walker.
    pub fn threads(&mut self, threads: NonZeroUsize) -> &mut Self {
        self.threads = threads;
        self
    }

    /// A convenience method for traversing directories without
    /// implementing [`WalkerVisitor`] on a custom type.
    ///
    /// Accepts a closure with the same signature as [`WalkerVisitor::visit`],
    /// returning a [`WalkerAction`] to guide the walker on what to do next.
    ///
    /// For behavior and error handling, see [`Walker::visit`].
    pub fn run<F>(&mut self, callback: F)
    where
        F: Fn(io::Result<DirEntry>) -> WalkerAction + Send + Sync,
    {
        struct FnVisitor<F> {
            callback: F,
        }

        impl<F> WalkerVisitor for FnVisitor<F>
        where
            F: Fn(io::Result<DirEntry>) -> WalkerAction + Send + Sync,
        {
            fn visit(&self, entry: io::Result<DirEntry>) -> WalkerAction {
                (self.callback)(entry)
            }
        }

        self.visit(&std::sync::Arc::new(FnVisitor { callback }));
    }

    /// Traverses all of the configured root paths in parallel,
    /// with the number of threads set by [`Walker::threads`].
    ///
    /// After this method is called, all configured root paths
    /// are cleared. Calling this method again on the same [`Walker`]
    /// will do nothing.
    ///
    /// # Additional [`Clone`] requirement
    /// This method requires the visitor to implement [`Clone`],
    /// as each worker thread receives its own clone of the visitor.
    ///
    /// If [`Clone`] cannot be implemented for a type, consider
    /// wrapping it in [`Arc`].
    ///
    /// [`Arc`]: std::sync::Arc
    ///
    /// # Errors
    /// Errors are not returned directly. Instead, they are passed
    /// to the visitor, allowing the program to handle or log them
    /// before traversal continues:
    /// - If a root path cannot be read, the visitor is called
    ///   with the error, then traversal moves on to the next
    ///   root path.
    /// - If a directory cannot be read during traversal, the
    ///   visitor is called with the error, then traversal
    ///   continues with remaining entries
    ///   (if it returns [`WalkerAction::Continue`]).
    ///
    /// # Aborting
    /// Returning [`WalkerAction::Quit`] from the visitor signals
    /// the walker to stop as soon as possible. However, entries
    /// already dispatched to other threads may still invoke the
    /// visitor before the walk fully stops.
    ///
    /// [`visit`]: Walker::visit
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
    /// Creates a new [`Walker`] initialized from an iterator of paths.
    ///
    /// It defaults to a single thread utilized for the walker.
    fn from_iter<T: IntoIterator<Item = P>>(iter: T) -> Self {
        let mut walker = Self::empty();
        for path in iter.into_iter() {
            walker = walker.add(path);
        }
        walker
    }
}

/// A trait called by [`Walker`] for each directory entry
/// or error encountered during traversal.
///
/// When implementing this trait, ensure the implementing type
/// is [`Send`] and [`Sync`] as the same visitor will be shared
/// across multiple threads.
pub trait WalkerVisitor: Send + Sync {
    /// This function will be called for each directory entry
    /// or I/O encountered during traversal, returning [`WalkerAction`]
    /// to guide the walker on what to do next.
    ///
    /// Errors are passed directly to this method rather than being
    /// returned from [`Walker::visit`], allowing the visitor to
    /// handle or log them before traversal continues.
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

/// Returned by [`WalkerVisitor::visit`] to guide the walker after
/// each directory entry or error is processed.
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
    /// Returns `true` if the action is [`Continue`].
    ///
    /// [`Continue`]: WalkerAction::Continue
    #[must_use]
    pub const fn is_continue(&self) -> bool {
        matches!(self, Self::Continue)
    }

    /// Returns `true` if the action is [`Skip`].
    ///
    /// [`Skip`]: WalkerAction::Skip
    #[must_use]
    pub const fn is_skip(&self) -> bool {
        matches!(self, Self::Skip)
    }

    /// Returns `true` if the action is [`Quit`].
    ///
    /// [`Quit`]: WalkerAction::Quit
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

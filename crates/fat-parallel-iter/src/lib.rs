// The entire implementation of ParallelIter is inspired by:
// https://github.com/BurntSushi/ripgrep/blob/master/crates/ignore/src/walk.rs
use crossbeam::{
    atomic::AtomicCell,
    deque::{Stealer, Worker as Deque},
};
use std::{fmt, num::NonZeroUsize, sync::Arc, time::Duration};

/// Parallel vector iterator.
///
/// This struct distributes elements of a vector to a pool of work-stealing threads
/// and processes them in parallel.
pub struct ParallelIter<T> {
    iter: Box<dyn Iterator<Item = T>>,
    threads: usize,
}

impl<T: Send> ParallelIter<T> {
    #[must_use]
    pub fn new(iter: impl Iterator<Item = T> + 'static) -> Self {
        Self {
            iter: Box::new(iter),
            threads: NonZeroUsize::new(1).expect("one is not zero").get(),
        }
    }

    /// Sets the number of threads to use for parallel processing.
    pub fn threads(mut self, threads: NonZeroUsize) -> Self {
        self.threads = threads.get();
        self
    }

    /// Runs the parallel iteration, executing the closure `iter` for each element.
    pub fn run(self, iter: impl Fn(T, &WorkerQueue<T>, &AtomicCell<bool>) + Send + Sync + Clone) {
        let queues = WorkerQueue::new_batched(self.threads, self.iter.collect::<Vec<_>>());

        let abort = Arc::new(AtomicCell::new(false));
        let busy_workers = Arc::new(AtomicCell::new(self.threads));

        std::thread::scope(|s| {
            let handles = queues
                .into_iter()
                .map(|queue| {
                    let abort = abort.clone();
                    let busy_workers = busy_workers.clone();
                    let iter = iter.clone();
                    s.spawn(move || {
                        while let Some(value) = acquire_from_queue(&queue, &abort, &busy_workers) {
                            iter(value, &queue, &abort);
                        }
                    })
                })
                .collect::<Vec<_>>();

            for handle in handles {
                handle.join().unwrap();
            }
        });
    }
}

fn acquire_from_queue<T>(
    queue: &WorkerQueue<T>,
    abort: &AtomicCell<bool>,
    busy_workers: &AtomicCell<usize>,
) -> Option<T>
where
    T: Send,
{
    if abort.load() {
        return None;
    }

    if let Some(value) = queue.pop() {
        return Some(value);
    }

    busy_workers.fetch_sub(1);

    // Check if we have other busy workers at the moment
    if busy_workers.load() == 0 {
        abort.store(true);
        return None;
    }

    // If we do have one, wait until there's a new one we can steal.
    loop {
        if abort.load() {
            return None;
        }

        if let Some(value) = queue.pop() {
            busy_workers.fetch_add(1);
            return Some(value);
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A local worker queue that allows for stealing from other queues.
pub struct WorkerQueue<T> {
    /// Current thread index to refer to its own stealer.
    index: usize,

    /// Local working queue for a thread.
    deque: Deque<T>,

    /// Work stealers from its neighboring threads.
    stealers: Arc<[Stealer<T>]>,
}

impl<T> WorkerQueue<T> {
    /// Creates a batch of empty local thread stacks with a number of threads.
    #[must_use]
    pub fn empty_batched(threads: usize) -> Vec<WorkerQueue<T>> {
        let deques = std::iter::repeat_with(Deque::new_lifo)
            .take(threads)
            .collect::<Vec<_>>();

        let stealers = deques.iter().map(Deque::stealer).collect::<Vec<_>>();
        let stealers: Arc<[_]> = Arc::from(stealers);

        deques
            .into_iter()
            .enumerate()
            .map(|(index, deque)| WorkerQueue {
                index,
                deque,
                stealers: stealers.clone(),
            })
            .collect::<Vec<_>>()
    }

    /// Creates a batch of initialized local thread stacks with a number of threads.
    ///
    /// Distributes the initial values as evenly as possible across the thread stacks.
    pub fn new_batched(threads: usize, init: Vec<T>) -> Vec<WorkerQueue<T>> {
        let stacks = Self::empty_batched(threads);

        // Distribute the initial messages
        init.into_iter()
            .rev()
            .zip(stacks.iter().cycle())
            .for_each(|(m, s)| s.push(m));

        stacks
    }

    /// Queues the value into the queue to be pulled later.
    pub fn push(&self, value: T) {
        self.deque.push(value);
    }

    /// Pops the pending value from the queue or steals one of the
    /// pending values from neighboring queues.
    #[must_use]
    pub fn pop(&self) -> Option<T> {
        self.deque.pop().or_else(|| self.steal())
    }

    /// Steals a pending value entry from neighboring queues.
    fn steal(&self) -> Option<T> {
        // Steal from the right, then to the left
        let (left, right) = self.stealers.split_at(self.index);

        // Don't steal from the same queue
        let right = &right[1..];
        let queues = right.iter().chain(left.iter());

        queues
            .map(|s| s.steal_batch_and_pop(&self.deque))
            .find_map(|s| s.success())
    }
}

impl<T> fmt::Debug for WorkerQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkerQueue")
            .field("index", &self.index)
            .finish()
    }
}

use crossbeam::{atomic::AtomicCell, queue::SegQueue};
use dashmap::{DashMap, Entry};
use std::{fmt, io, sync::Arc};

use crate::Node;

/// A generational handle to a node in the store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeId {
    index: usize,
    generation: u32,
}

/// A concurrent generational arena for file system nodes.
///
/// It uses [`DashMap`] for per-shard locking with a lock-free free list
/// ([`SegQueue`]) for index recycling.
#[derive(Debug)]
pub struct NodeStore {
    nodes: DashMap<usize, NodeSlot>,

    /// Indices to recycle if an existing node is deleted.
    free_list: SegQueue<usize>,

    /// An atomic number that generates the next index.
    ///
    /// This must be used if the free list is empty.
    next_idx: AtomicCell<usize>,
}

impl Default for NodeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeStore {
    /// Creates an empty node store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: DashMap::new(),
            free_list: SegQueue::new(),
            next_idx: AtomicCell::new(0),
        }
    }

    /// Retrieves a node by its [`NodeId`].
    ///
    /// Returns an error if the handle is stale or the node doesn't exist.
    pub fn get(&self, id: NodeId) -> io::Result<Arc<Node>> {
        let entry = self.nodes.get(&id.index).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cannot find specified node slot")
        })?;

        if entry.generation != id.generation {
            return Err(io::Error::other("stale node"));
        }

        let node = entry.get();

        // The specified slot should be occupied at this point.
        debug_assert!(node.is_some(), "slot is suddenly vacant");
        Ok(node.unwrap())
    }

    /// Inserts a node into the store, recycling a freed slot if available.
    ///
    /// Returns a fresh [`NodeId`].
    pub fn insert(&self, node: Node) -> io::Result<NodeId> {
        match self.find_available_entry()? {
            Entry::Occupied(mut entry) => {
                let index = *entry.key();
                let slot = entry.get_mut();
                slot.occupy(node);

                Ok(NodeId {
                    index,
                    generation: slot.generation,
                })
            }
            Entry::Vacant(entry) => {
                let index = *entry.key();
                let slot = NodeSlot::new(node);
                let generation = slot.generation;
                entry.insert(slot);

                Ok(NodeId { index, generation })
            }
        }
    }

    /// Removes a node by its [`NodeId`], freeing the slot for reuse.
    ///
    /// Returns an error if the handle is stale.
    pub fn remove(&self, id: NodeId) -> io::Result<()> {
        let mut entry = self.nodes.get_mut(&id.index).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cannot find specified node slot")
        })?;

        if entry.generation != id.generation {
            return Err(io::Error::other("stale node"));
        }

        entry.take();

        // This is to prevent a deadlock when `insert()` pops this index
        // and tries to lock the same shard.
        drop(entry);
        self.free_list.push(id.index);

        Ok(())
    }

    fn find_available_entry(&self) -> io::Result<Entry<'_, usize, NodeSlot>> {
        // Do we have free node indices to recycle?
        if let Some(idx) = self.free_list.pop() {
            return Ok(self.nodes.entry(idx));
        }

        // Have we exhausted the next possible indices?
        let idx = self.next_idx.fetch_add(1);
        if idx == usize::MAX {
            return Err(io::Error::new(
                io::ErrorKind::QuotaExceeded,
                "node id exhausted",
            ));
        }

        Ok(self.nodes.entry(idx))
    }
}

struct NodeSlot {
    value: Option<Arc<Node>>,
    generation: u32,
}

impl NodeSlot {
    #[must_use]
    fn new(value: Node) -> Self {
        Self {
            value: Some(Arc::new(value)),
            generation: 0,
        }
    }

    fn get(&self) -> Option<Arc<Node>> {
        self.value.as_ref().cloned()
    }

    fn occupy(&mut self, node: Node) {
        self.value = Some(Arc::new(node));
    }

    fn take(&mut self) {
        self.value = None;
        self.generation = self.generation.wrapping_add(1);
    }
}

impl fmt::Debug for NodeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(node) = self.get() {
            if f.alternate() {
                write!(f, "NodeSlot({node:#?})")
            } else {
                write!(f, "NodeSlot({node:?})")
            }
        } else {
            write!(f, "NodeSlot(<vacant>)")
        }
    }
}

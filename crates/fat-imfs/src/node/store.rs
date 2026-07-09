use crossbeam::{atomic::AtomicCell, queue::SegQueue};
use dashmap::{DashMap, Entry};
use std::{fmt, io, sync::Arc};

use crate::Node;

/// A generational handle to a node in the store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NodeId {
    pub(crate) index: usize,
    pub(crate) generation: u32,
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

    /// Vacates every node slot stored in the store excluding
    /// the root node ID provided in the parameter.
    pub fn clear(&self, root: NodeId) {
        for mut entry in self.nodes.iter_mut() {
            let idx = *entry.key();
            let slot = entry.value_mut();
            let generation = slot.generation;

            if idx == root.index && generation == root.generation {
                continue;
            }

            slot.take();
            self.free_list.push(idx);
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

    /// Gets the total active slots inserted in this store.
    #[must_use]
    pub fn active_slots(&self) -> usize {
        self.nodes.iter().filter(|v| v.value.is_some()).count()
    }

    /// Gets the total slots inserted in this store.
    #[must_use]
    pub fn slots(&self) -> usize {
        self.nodes.len()
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
            fmt::Debug::fmt(&node, f)
        } else {
            f.write_str("<vacant>")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::node::{Node, NodeId, NodeStore};
    use fat_vfs::VfsNodeType;

    #[test]
    fn test_clear() {
        let store = NodeStore::new();
        let root = store.insert(Node::empty_dir()).unwrap();
        store.insert(Node::empty_file()).unwrap();
        store.clear(root);

        // It should have one index in the free list and vacated
        // one node in the nodes field.
        assert_eq!(store.free_list.pop(), Some(1));
        assert_eq!(store.next_idx.load(), 2);

        assert!(store.nodes.get(&0).unwrap().value.is_some());
        assert!(store.nodes.get(&1).unwrap().value.is_none());
    }

    #[test]
    fn test_clear_with_stale_id() {
        let store = NodeStore::new();
        store.insert(Node::empty_dir()).unwrap();
        store.insert(Node::empty_file()).unwrap();
        store.clear(NodeId {
            index: 0,
            generation: 10,
        });

        // It should have one index in the free list and vacated
        // two nodes in the nodes field.
        assert_eq!(store.free_list.len(), 2);
        assert!(store.nodes.get(&0).unwrap().value.is_none());
        assert!(store.nodes.get(&1).unwrap().value.is_none());
    }

    #[test]
    fn test_get() {
        let store = NodeStore::new();
        let node = store.insert(Node::empty_dir()).unwrap();

        let result = store.get(node);
        assert!(result.is_ok());

        let result = result.unwrap();
        assert_eq!(result.ty(), VfsNodeType::Directory);
    }

    #[test]
    fn test_get_with_stale_id() {
        let store = NodeStore::new();
        let staled = store.insert(Node::empty_dir()).unwrap();
        store.remove(staled).unwrap();

        let new = store.insert(Node::empty_dir()).unwrap();
        assert_eq!(new.index, 0);

        let result = store.get(staled);
        assert!(result.is_err());
    }

    #[test]
    fn test_insert() {
        // Case: empty node store
        let store = NodeStore::new();
        let node1 = store.insert(Node::empty_dir()).unwrap();
        assert_eq!(node1.index, 0);
        assert_eq!(node1.generation, 0);

        // Case: at least one had occupied a node slot
        let node2 = store.insert(Node::empty_file()).unwrap();
        assert_eq!(node2.index, 1);
        assert_eq!(node2.generation, 0);

        // Case: one node slot had vacated
        store.remove(node1).unwrap();

        assert_eq!(store.next_idx.load(), 2);
        assert_eq!(store.nodes.len(), 2);
        assert!(!store.free_list.is_empty());

        let file_node = Node::empty_file();
        file_node.as_file().unwrap().replace(b"Hello!").unwrap();

        let node1 = store.insert(file_node).unwrap();
        assert_eq!(node1.index, 0);
        assert_eq!(node1.generation, 1); // got incremented by 1
    }

    #[test]
    fn test_remove() {
        let store = NodeStore::new();
        let node = store.insert(Node::empty_dir()).unwrap();

        // Remove the existing node successfuly
        store.remove(node).unwrap();

        assert!(store.nodes.get(&node.index).unwrap().value.is_none());
        assert_eq!(store.free_list.len(), 1);

        // Attempting to remove a stale node
        let error = store.remove(node).unwrap_err();
        assert_eq!(error.to_string(), "stale node");

        // Attempt to remove a node slot index that had not inserted before
        let node = NodeId {
            index: 99,
            generation: 0,
        };

        let error = store.remove(node).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    }
}

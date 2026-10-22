//! pgvector 0.8.7's in-memory HNSW build graph: hnswbuild.c's InsertTuple (its
//! in-memory phase), InsertTupleInMemory and UpdateGraphInMemory, and the
//! hnswutils.c search they call with index == NULL. The serial build and every
//! participant of a parallel build insert through one Graph.
//!
//! C keeps the graph in a Generation context (serial) or one DSM area
//! (parallel), addressed by relptr and guarded by an LWLock per element plus
//! entryLock, entryWaitLock, allocatorLock, flushLock and the graph->lock
//! spinlock (hnsw.h:182-252). Here elements live in an append-only arena
//! addressed by u32 handles (relptr's role), and the locks are pgsync's, so
//! tests/loom.rs explores this code's lock protocol. The lock order is C's
//! (hnswbuild.c:437-580):
//!   flush_lock (read) -> allocator (released before the next step)
//!   flush_lock (read) -> entry_wait_lock (transient) -> entry_lock (read or
//!   write) -> { one element's links at a time | list }
//! An overflowing inserter releases both, then takes flush_lock (write)
//! holding nothing else. Every guard is released on the error path too (C
//! needs LWLockReleaseAll for that).
//!
//! DIVERGENCE: C's flush point counts C's allocations. This graph stores Rust
//! values and charges C's sizes to a model of C's allocator (memory.rs), so
//! the flush point is C's while the bytes actually held differ; and the arena
//! is freed with the graph, not when the graph is flushed (C resets graphCtx
//! in a serial build, hnswbuild.c:315).

mod algo;
mod arena;
pub mod memory;
#[cfg(test)]
mod tests;

use pgsync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub use memory::{Accounting, GenerationModel, GraphError};

/// HNSW_HEAPTIDS (hnsw.h:69).
pub const HNSW_HEAPTIDS: usize = 10;

/// A heap TID as its 6-byte ItemPointerData image.
pub type HeapTid = [u8; 6];

/// HnswCandidate (hnsw.h:202-207), with an element handle for the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub element: u32,
    pub distance: f32,
    pub closer: bool,
}

/// HnswNeighborArray (hnsw.h:209-214).
#[derive(Clone, Debug, Default)]
pub struct NeighborArray {
    pub items: Vec<Candidate>,
    pub closer_set: bool,
}

/// What C's element LWLock guards: "It must be held when reading or modifying
/// the element's neighbors or 'heaptids'" (hnswbuild.c:20-21).
/// `neighbors[level - lc]` holds layer lc.
pub struct Links {
    pub heaptids: [HeapTid; HNSW_HEAPTIDS],
    pub heaptids_len: u8,
    pub neighbors: Vec<NeighborArray>,
}

/// HnswElementData (hnsw.h:182-198). The level and the value never change
/// once the element is allocated, so they are read without its lock, as in C.
pub struct Element {
    pub level: u8,
    /// The value's varlena image in 8-byte words, so a distance function can
    /// read its header in place.
    value: Box<[u64]>,
    value_len: usize,
    links: RwLock<Links>,
}

impl Element {
    fn new(level: u8, value: &[u8], links: Links) -> Element {
        let mut words = vec![0u64; value.len().div_ceil(8)].into_boxed_slice();
        // SAFETY: `words` spans at least value.len() bytes, and u8 has no
        // alignment or validity requirement.
        unsafe { core::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), value.len()) }
            .copy_from_slice(value);
        Element { level, value: words, value_len: value.len(), links: RwLock::new(links) }
    }

    /// HnswGetValue: the value's image.
    pub fn value(&self) -> &[u8] {
        // SAFETY: `value` holds value_len initialized bytes (Element::new).
        unsafe { core::slice::from_raw_parts(self.value.as_ptr().cast::<u8>(), self.value_len) }
    }

    /// The element's links under its lock in shared mode.
    pub fn links(&self) -> RwLockReadGuard<'_, Links> {
        read(&self.links)
    }

    fn links_mut(&self) -> RwLockWriteGuard<'_, Links> {
        write(&self.links)
    }
}

/// The build's m and efConstruction.
pub struct InsertParams {
    pub m: i32,
    pub ef_construction: i32,
}

/// Where Graph::insert put the tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    InMemory,
    /// The graph is flushed: the caller inserts the tuple on disk
    /// (HnswInsertTupleOnDisk(.., building = true)).
    OnDisk,
}

#[derive(Debug)]
pub enum InsertError<E> {
    /// A callback's error: the distance function's or on_overflow's.
    Callback(E),
    Graph(GraphError),
}

struct Allocator {
    accounting: Accounting,
    /// Elements allocated so far; the next element's handle.
    len: u32,
}

struct List {
    /// graph->head, oldest first (C prepends; head_newest_first walks it C's way).
    head: Vec<u32>,
    indtuples: f64,
}

/// HnswGraph (hnsw.h:232-252).
pub struct Graph {
    elements: arena::Arena<Element>,
    /// graph->lock (a spinlock in C): head and indtuples.
    list: Mutex<List>,
    /// entryLock, guarding entryPoint.
    entry_lock: RwLock<Option<u32>>,
    /// entryWaitLock.
    entry_wait_lock: Mutex<()>,
    /// allocatorLock, guarding memoryUsed and the element count.
    allocator: Mutex<Allocator>,
    /// flushLock, guarding flushed.
    flush_lock: RwLock<bool>,
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

impl Graph {
    /// InitGraph (hnswbuild.c:618-634).
    pub fn new(accounting: Accounting) -> Graph {
        Graph {
            elements: arena::Arena::new(),
            list: Mutex::new(List { head: Vec::new(), indtuples: 0.0 }),
            entry_lock: RwLock::new(None),
            entry_wait_lock: Mutex::new(()),
            allocator: Mutex::new(Allocator { accounting, len: 0 }),
            flush_lock: RwLock::new(false),
        }
    }

    pub fn element(&self, id: u32) -> &Element {
        self.elements.get(id)
    }

    /// Elements allocated so far, including copies merged into a duplicate.
    pub fn element_count(&self) -> u32 {
        pgsync::lock(&self.allocator).len
    }

    /// graph->head as CreateGraphPages walks it (hnswbuild.c:177-186): newest first.
    pub fn head_newest_first(&self) -> Vec<u32> {
        pgsync::lock(&self.list).head.iter().rev().copied().collect()
    }

    pub fn entry_point(&self) -> Option<u32> {
        *read(&self.entry_lock)
    }

    pub fn indtuples(&self) -> f64 {
        pgsync::lock(&self.list).indtuples
    }

    /// `++graph->indtuples` under graph->lock (BuildCallback,
    /// hnswbuild.c:603-607); the new count.
    pub fn add_indtuple(&self) -> f64 {
        let mut list = pgsync::lock(&self.list);
        list.indtuples += 1.0;
        list.indtuples
    }

    pub fn memory_used(&self) -> usize {
        pgsync::lock(&self.allocator).accounting.used()
    }

    pub fn flushed(&self) -> bool {
        *read(&self.flush_lock)
    }

    /// InsertTuple's in-memory phase (hnswbuild.c:509-577), for a formed and
    /// dimension-checked value. `draw_level` is HnswInitElement's level draw,
    /// called only when the element is allocated. `distance` is the support
    /// function on two values. `on_overflow` runs at most once per graph, with
    /// flush_lock held exclusively, when the graph no longer fits (C's NOTICE
    /// and FlushPages); it must not call insert, flush_if_needed or flushed.
    pub fn insert<E>(
        &self,
        value: &[u8],
        heaptid: HeapTid,
        params: &InsertParams,
        draw_level: impl FnOnce() -> u8,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
        on_overflow: impl FnOnce(&Graph) -> Result<(), E>,
    ) -> Result<Inserted, InsertError<E>> {
        // Ensure graph not flushed when inserting
        let flushed = read(&self.flush_lock);

        // Are we in the on-disk phase?
        if *flushed {
            return Ok(Inserted::OnDisk);
        }

        // In a parallel build, the HnswElement is allocated from the shared
        // memory area, so we need to coordinate with other processes.
        let mut allocator = pgsync::lock(&self.allocator);

        // Check that we have enough memory available for the new element now
        // that we have the allocator lock, and flush pages if needed.
        if allocator.accounting.is_full() {
            drop(allocator);
            drop(flushed);
            let mut flushed = write(&self.flush_lock);
            if !*flushed {
                on_overflow(self).map_err(InsertError::Callback)?;
                *flushed = true;
            }
            return Ok(Inserted::OnDisk);
        }

        // Ok, we can proceed to allocate the element
        let level = draw_level();
        allocator.accounting.charge_element(level, params.m, value.len()).map_err(InsertError::Graph)?;
        let id = allocator.len;
        allocator.len += 1;
        let mut heaptids = [[0u8; 6]; HNSW_HEAPTIDS];
        heaptids[0] = heaptid;
        let neighbors = (0..=level).map(|_| NeighborArray::default()).collect();
        self.elements.publish(id, Element::new(level, value, Links { heaptids, heaptids_len: 1, neighbors }));

        // We have now allocated the space needed for the element, so we don't
        // need the allocator lock anymore.
        drop(allocator);

        // Insert tuple
        self.insert_in_memory(id, params, distance).map_err(InsertError::Callback)?;

        // Release flush lock
        drop(flushed);
        Ok(Inserted::InMemory)
    }

    /// BuildGraph's `if (!graph->flushed) FlushPages(buildstate)`
    /// (hnswbuild.c:1119-1120), once every participant has finished.
    pub fn flush_if_needed<E>(&self, flush: impl FnOnce(&Graph) -> Result<(), E>) -> Result<(), E> {
        let mut flushed = write(&self.flush_lock);
        if !*flushed {
            flush(self)?;
            *flushed = true;
        }
        Ok(())
    }

    /// An element allocated but not linked, for the algorithm's unit tests.
    #[cfg(test)]
    pub(crate) fn push_unlinked(&self, level: u8, value: &[u8]) -> u32 {
        let mut allocator = pgsync::lock(&self.allocator);
        let id = allocator.len;
        allocator.len += 1;
        let neighbors = (0..=level).map(|_| NeighborArray::default()).collect();
        self.elements.publish(
            id,
            Element::new(level, value, Links { heaptids: [[0u8; 6]; HNSW_HEAPTIDS], heaptids_len: 0, neighbors }),
        );
        id
    }
}

//! hnsw.h vocabulary (pgvector): scan state lives here so relscan's
//! IndexScanOpaque can hold it without a cycle through the AM crates.

pub mod tmpctx;
pub use tmpctx::*;

use types_core::{BlockNumber, Oid};
use types_fmgr::FmgrInfo;
use types_tuple::itemptr::ItemPointerData;

pub const HNSW_MAX_DIM: usize = 2000;

pub const HNSW_DISTANCE_PROC: u16 = 1;
pub const HNSW_NORM_PROC: u16 = 2;
pub const HNSW_TYPE_INFO_PROC: u16 = 3;

pub const HNSW_VERSION: u32 = 1;
pub const HNSW_MAGIC_NUMBER: u32 = 0xA953A953;
pub const HNSW_PAGE_ID: u16 = 0xFF90;

pub const HNSW_METAPAGE_BLKNO: BlockNumber = 0;
pub const HNSW_HEAD_BLKNO: BlockNumber = 1;

pub const HNSW_UPDATE_LOCK: BlockNumber = 0;
pub const HNSW_SCAN_LOCK: BlockNumber = 1;

pub const HNSW_DEFAULT_M: i32 = 16;
pub const HNSW_MAX_M: i32 = 100;
pub const HNSW_DEFAULT_EF_CONSTRUCTION: i32 = 64;

pub const HNSW_ELEMENT_TUPLE_TYPE: u8 = 1;
pub const HNSW_NEIGHBOR_TUPLE_TYPE: u8 = 2;

pub const HNSW_HEAPTIDS: usize = 10;

pub const HNSW_UPDATE_ENTRY_GREATER: i32 = 1;
pub const HNSW_UPDATE_ENTRY_ALWAYS: i32 = 2;

pub const HNSW_ITERATIVE_SCAN_OFF: i32 = 0;
pub const HNSW_ITERATIVE_SCAN_RELAXED: i32 = 1;
pub const HNSW_ITERATIVE_SCAN_STRICT: i32 = 2;

#[inline]
pub fn hnsw_get_layer_m(m: i32, layer: i32) -> i32 {
    if layer == 0 {
        m * 2
    } else {
        m
    }
}

#[inline]
pub fn hnsw_get_ml(m: i32) -> f64 {
    1.0 / (m as f64).ln()
}

// Resolved support procs (C HnswSupport): distance (proc 1), optional norm
// (proc 2), index collation.
#[derive(Clone)]
pub struct HnswSupport {
    pub procinfo: FmgrInfo,
    pub normprocinfo: Option<FmgrInfo>,
    pub collation: Oid,
}

#[derive(Clone, Copy)]
pub struct HnswScanElement {
    pub blkno: BlockNumber,
    pub offno: u16,
    pub level: u8,
    pub version: u8,
    pub heaptids: [ItemPointerData; HNSW_HEAPTIDS],
    pub heaptids_len: u8,
    pub neighbor_page: BlockNumber,
    pub neighbor_offno: u16,
    pub distance: f64,
}

// so->discarded (hnswscan.c; allocated with InitVisited, hnswutils.c:853-854):
// a pairingheap ordered by CompareNearestDiscardedCandidates. It holds
// self-contained copies of the discarded elements so it outlives each call's
// element pool, and it is filled in the exact order C adds to it, so ties pop
// in C's order. Owned (global allocator): hnswrescan drops it, as C's
// MemoryContextReset(so->tmpCtx) frees it.
pub type ScanDiscardedHeap =
    pairingheap::PairingHeap<HnswScanElement, fn(&HnswScanElement, &HnswScanElement) -> i32>;

// CompareNearestDiscardedCandidates (hnswutils.c:647-656): nearest first.
pub fn compare_nearest_discarded(a: &HnswScanElement, b: &HnswScanElement) -> i32 {
    if a.distance < b.distance {
        1
    } else if a.distance > b.distance {
        -1
    } else {
        0
    }
}

pub fn new_scan_discarded_heap() -> ScanDiscardedHeap {
    ScanDiscardedHeap::new(compare_nearest_discarded)
}

// C HnswScanOpaqueData; `w` is furthest-first, last() = nearest.
pub struct HnswScanOpaqueData<'mcx> {
    pub mcx: mcx::Mcx<'mcx>,
    pub first: bool,
    pub m: i32,
    pub tuples: i64,
    pub previous_distance: f64,
    pub max_memory: usize,
    // Approximates C MemoryContextMemAllocated(tmpCtx) for the iterative cap.
    pub mem_used: usize,
    // value/w/visited/discarded live in C's so->tmpCtx, which hnswrescan
    // resets; here they are globally-allocated owned values so reassignment
    // in hnswrescan/hnswendscan frees them (bounded memory across rescans).
    pub value: Option<Vec<u8>>,
    pub support: HnswSupport,
    pub max_dimensions: i32,
    pub norm_is_l2: bool,
    pub w: Vec<HnswScanElement>,
    pub visited: std::collections::HashSet<(BlockNumber, u16), rustc_hash::FxBuildHasher>,
    pub discarded: Option<ScanDiscardedHeap>,
}

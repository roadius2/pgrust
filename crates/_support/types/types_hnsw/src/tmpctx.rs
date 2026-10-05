//! Byte-exact model of hnswscan.c's so->tmpCtx, the AllocSet whose
//! MemoryContextMemAllocated the iterative scan compares against
//! work_mem * hnsw.scan_mem_multiplier (hnswscan.c:264). DIVERGENCE: pgrust
//! does not palloc these structures in a C AllocSet; the scan feeds this model
//! C's allocation sequence (the sizes below, at C's allocation points) so the
//! cap trips at C's tuple. Validated against an instrumented C build; see
//! tests/data/README.md.

// C struct sizes, 64-bit, from a sizeof probe against the pgvector 0.8.7 headers.
/// sizeof(HnswElementData) (hnsw.h:182-198).
pub const HNSW_ELEMENT_DATA_SIZE: usize = 128;
/// sizeof(HnswSearchCandidate) (hnsw.h:216-222).
pub const HNSW_SEARCH_CANDIDATE_SIZE: usize = 64;
/// sizeof(pairingheap) (lib/pairingheap.h).
pub const PAIRINGHEAP_SIZE: usize = 24;
/// sizeof(HnswUnvisited) (hnsw.h:404-408).
pub const HNSW_UNVISITED_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// aset.c accounting model.
// ---------------------------------------------------------------------------

// aset.c:83-105, 64-bit, no MEMORY_CONTEXT_CHECKING (the reference build).
const ALLOC_MINBITS: u32 = 3;
const ALLOCSET_NUM_FREELISTS: usize = 11;
const ALLOC_CHUNK_LIMIT: usize = 1 << (ALLOCSET_NUM_FREELISTS - 1 + ALLOC_MINBITS as usize);
const ALLOC_CHUNK_FRACTION: usize = 4;
// MAXALIGN(sizeof(AllocBlockData)): aset, prev, next, freeptr, endptr.
const ALLOC_BLOCKHDRSZ: usize = 40;
// sizeof(MemoryChunk).
const ALLOC_CHUNKHDRSZ: usize = 8;
// MAXALIGN(sizeof(AllocSetContext)): MemoryContextData (80) + blocks (8) +
// freelist[11] (88) + 4 x uint32 (16) + freeListIndex (4) + pad (4).
const ALLOC_SET_CONTEXT_SIZE: usize = 200;

#[inline]
fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

// aset.c:277 AllocSetFreeIndex.
#[inline]
fn alloc_set_free_index(size: usize) -> usize {
    if size > (1 << ALLOC_MINBITS) {
        (usize::BITS - ((size - 1) >> ALLOC_MINBITS).leading_zeros()) as usize
    } else {
        0
    }
}

#[inline]
fn chunk_size_from_free_list_idx(fidx: usize) -> usize {
    1usize << (fidx as u32 + ALLOC_MINBITS)
}

/// Byte-for-byte model of an aset.c AllocSet's `mem_allocated` (what
/// MemoryContextMemAllocated(ctx, false) returns). Tracks only what that
/// figure depends on: the active block's free tail, the per-class freelist
/// populations, the next block size, and the large-chunk blocks.
#[derive(Clone, Debug)]
pub struct AllocSetModel {
    init_block_size: usize,
    max_block_size: usize,
    alloc_chunk_limit: usize,
    keeper_size: usize,
    mem_allocated: usize,
    next_block_size: usize,
    // set->blocks->endptr - set->blocks->freeptr
    avail: usize,
    nfree: [u32; ALLOCSET_NUM_FREELISTS],
}

impl AllocSetModel {
    /// aset.c:347 AllocSetContextCreateInternal (fresh or recycled: same state).
    pub fn new(min_context_size: usize, init_block_size: usize, max_block_size: usize) -> Self {
        let mut first_block_size = ALLOC_SET_CONTEXT_SIZE + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
        if min_context_size != 0 {
            first_block_size = first_block_size.max(min_context_size);
        } else {
            first_block_size = first_block_size.max(init_block_size);
        }
        let mut alloc_chunk_limit = ALLOC_CHUNK_LIMIT;
        while alloc_chunk_limit + ALLOC_CHUNKHDRSZ
            > (max_block_size - ALLOC_BLOCKHDRSZ) / ALLOC_CHUNK_FRACTION
        {
            alloc_chunk_limit >>= 1;
        }
        let mut m = AllocSetModel {
            init_block_size,
            max_block_size,
            alloc_chunk_limit,
            keeper_size: first_block_size,
            mem_allocated: 0,
            next_block_size: 0,
            avail: 0,
            nfree: [0; ALLOCSET_NUM_FREELISTS],
        };
        m.reset();
        m
    }

    /// aset.c:537 AllocSetReset: keeper block only, freelists cleared.
    pub fn reset(&mut self) {
        self.mem_allocated = self.keeper_size;
        self.avail = self.keeper_size - ALLOC_SET_CONTEXT_SIZE - ALLOC_BLOCKHDRSZ;
        self.nfree = [0; ALLOCSET_NUM_FREELISTS];
        self.next_block_size = self.init_block_size;
    }

    /// MemoryContextMemAllocated(context, false).
    pub fn mem_allocated(&self) -> usize {
        self.mem_allocated
    }

    /// aset.c:967 AllocSetAlloc.
    pub fn palloc(&mut self, size: usize) {
        if size > self.alloc_chunk_limit {
            // aset.c:696 AllocSetAllocLarge.
            self.mem_allocated += maxalign(size) + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
            return;
        }
        let fidx = alloc_set_free_index(size);
        if self.nfree[fidx] > 0 {
            self.nfree[fidx] -= 1;
            return;
        }
        let chunk_size = chunk_size_from_free_list_idx(fidx);
        if self.avail >= chunk_size + ALLOC_CHUNKHDRSZ {
            self.avail -= chunk_size + ALLOC_CHUNKHDRSZ;
            return;
        }
        // aset.c:819 AllocSetAllocFromNewBlock: carve the old block's tail
        // into freelist chunks, then start a new block.
        while self.avail >= (1 << ALLOC_MINBITS) + ALLOC_CHUNKHDRSZ {
            let mut availchunk = self.avail - ALLOC_CHUNKHDRSZ;
            let mut a_fidx = alloc_set_free_index(availchunk);
            if availchunk != chunk_size_from_free_list_idx(a_fidx) {
                a_fidx -= 1;
                availchunk = chunk_size_from_free_list_idx(a_fidx);
            }
            self.nfree[a_fidx] += 1;
            self.avail -= availchunk + ALLOC_CHUNKHDRSZ;
        }
        let mut blksize = self.next_block_size;
        self.next_block_size = (self.next_block_size << 1).min(self.max_block_size);
        let required_size = chunk_size + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
        while blksize < required_size {
            blksize <<= 1;
        }
        self.mem_allocated += blksize;
        self.avail = blksize - ALLOC_BLOCKHDRSZ - chunk_size - ALLOC_CHUNKHDRSZ;
    }

    /// aset.c:1062 AllocSetFree of a chunk palloc'd with `size`.
    pub fn pfree(&mut self, size: usize) {
        if size > self.alloc_chunk_limit {
            self.mem_allocated -= maxalign(size) + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
            return;
        }
        self.nfree[alloc_set_free_index(size)] += 1;
    }

    /// aset.c:1188 AllocSetRealloc, small/small and small->large cases used here.
    pub fn repalloc(&mut self, old_size: usize, new_size: usize) {
        if old_size <= self.alloc_chunk_limit
            && chunk_size_from_free_list_idx(alloc_set_free_index(old_size)) >= new_size
        {
            return;
        }
        assert!(old_size <= self.alloc_chunk_limit, "large-chunk realloc not modeled");
        self.palloc(new_size);
        self.pfree(old_size);
    }
}

// ---------------------------------------------------------------------------
// list.c allocation shape of a List built by lappend.
// ---------------------------------------------------------------------------

/// offsetof(List, initial_elements).
const LIST_HEADER_SIZE: u32 = 24;
/// sizeof(ListCell).
const LIST_CELL_SIZE: u32 = 8;
/// list.c:48 LIST_HEADER_OVERHEAD.
const LIST_HEADER_OVERHEAD: u32 = (LIST_HEADER_SIZE - 1) / LIST_CELL_SIZE + 1;

/// The tmpCtx footprint of one C `List *` that is only ever grown by
/// lappend (list.c new_list / enlarge_list) and emptied by list_delete_last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListShape {
    len: u32,
    max_len: u32,
    // initial_max: cells allocated with the header (new_list); 0 for NIL.
    initial_max: u32,
    inline: bool,
}

impl ListShape {
    pub const NIL: ListShape = ListShape { len: 0, max_len: 0, initial_max: 0, inline: true };

    pub fn len(&self) -> u32 {
        self.len
    }

    /// list.c lappend.
    pub fn lappend(&mut self, mem: &mut AllocSetModel) {
        if self.len == 0 {
            // list.c:117 new_list(T_List, 1).
            let max = (1 + LIST_HEADER_OVERHEAD).max(8).next_power_of_two() - LIST_HEADER_OVERHEAD;
            mem.palloc((LIST_HEADER_SIZE + max * LIST_CELL_SIZE) as usize);
            *self = ListShape { len: 1, max_len: max, initial_max: max, inline: true };
            return;
        }
        if self.len >= self.max_len {
            // list.c:160 enlarge_list(list, length + 1).
            let new_max = (self.len + 1).max(16).next_power_of_two();
            if self.inline {
                mem.palloc((new_max * LIST_CELL_SIZE) as usize);
            } else {
                mem.repalloc((self.max_len * LIST_CELL_SIZE) as usize, (new_max * LIST_CELL_SIZE) as usize);
            }
            self.max_len = new_max;
            self.inline = false;
        }
        self.len += 1;
    }

    /// A List built by `n` lappends onto NIL (e.g. HnswSearchLayer's w).
    pub fn lappend_n(n: usize, mem: &mut AllocSetModel) -> ListShape {
        let mut l = ListShape::NIL;
        for _ in 0..n {
            l.lappend(mem);
        }
        l
    }

    /// list.c list_delete_last: frees the List once it would become empty.
    pub fn delete_last(&mut self, mem: &mut AllocSetModel) {
        assert!(self.len > 0, "list_delete_last on NIL");
        if self.len <= 1 {
            // list.c list_free_private.
            if !self.inline {
                mem.pfree((self.max_len * LIST_CELL_SIZE) as usize);
            }
            mem.pfree((LIST_HEADER_SIZE + self.initial_max * LIST_CELL_SIZE) as usize);
            *self = ListShape::NIL;
        } else {
            self.len -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// tidhash: simplehash.h instantiated as in hnswutils.c:41-66.
// ---------------------------------------------------------------------------

const SH_FILLFACTOR: f64 = 0.9;
const SH_MAX_FILLFACTOR: f64 = 0.98;
const SH_GROW_MAX_DIB: u32 = 25;
const SH_GROW_MAX_MOVE: u32 = 150;
const SH_GROW_MIN_FILLFACTOR: f64 = 0.1;
const SH_MAX_SIZE: u64 = (u32::MAX as u64) + 1;
const TID_EMPTY: u64 = u64::MAX;
/// sizeof(TidHashEntry): ItemPointerData (6) + status (1), padded to 8.
const TID_HASH_ENTRY_SIZE: usize = 8;
/// sizeof(tidhash_hash).
const TID_HASH_HEADER_SIZE: usize = 48;

/// hash_tid's key (hnswutils.c:43-56): ItemPointerData (bi_hi, bi_lo,
/// ip_posid) little-endian in the low 6 bytes of a zeroed uint64.
#[inline]
fn tid_key(blkno: u32, offno: u16) -> u64 {
    ((blkno >> 16) as u64) | (((blkno & 0xffff) as u64) << 16) | ((offno as u64) << 32)
}

#[inline]
fn hash_tid(key: u64) -> u32 {
    hashfn::murmurhash64(key) as u32
}

/// C's visited_hash.tids (simplehash: robin hood, no stored hash). Used as
/// the scan's real visited set, so its growth points are C's.
pub struct TidHash {
    data: Vec<u64>,
    sizemask: u32,
    members: u32,
    grow_threshold: u32,
}

impl TidHash {
    fn compute_size(newsize: u64) -> u64 {
        newsize.max(2).next_power_of_two()
    }

    /// SH_CREATE(ctx, nelements): charges the header and bucket array.
    pub fn create(nelements: u32, mem: Option<&mut AllocSetModel>) -> Self {
        let size = Self::compute_size((SH_MAX_SIZE as f64).min(nelements as f64 / SH_FILLFACTOR) as u64);
        if let Some(mem) = mem {
            mem.palloc(TID_HASH_HEADER_SIZE);
            mem.palloc(TID_HASH_ENTRY_SIZE * size as usize);
        }
        let mut t = TidHash { data: vec![TID_EMPTY; size as usize], sizemask: 0, members: 0, grow_threshold: 0 };
        t.update_parameters(size);
        t
    }

    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn members(&self) -> u32 {
        self.members
    }

    fn update_parameters(&mut self, size: u64) {
        self.sizemask = (size - 1) as u32;
        self.grow_threshold = if size == SH_MAX_SIZE {
            (size as f64 * SH_MAX_FILLFACTOR) as u32
        } else {
            (size as f64 * SH_FILLFACTOR) as u32
        };
    }

    #[inline]
    fn initial_bucket(&self, hash: u32) -> u32 {
        hash & self.sizemask
    }

    #[inline]
    fn distance_from_optimal(&self, optimal: u32, bucket: u32) -> u32 {
        if optimal <= bucket {
            bucket - optimal
        } else {
            (self.size() as u32).wrapping_add(bucket).wrapping_sub(optimal)
        }
    }

    /// SH_GROW: allocate the new array, rehash, free the old one.
    fn grow(&mut self, newsize: u64, mut mem: Option<&mut AllocSetModel>) {
        let oldsize = self.size();
        let newsize = Self::compute_size(newsize);
        if let Some(mem) = mem.as_deref_mut() {
            mem.palloc(TID_HASH_ENTRY_SIZE * newsize as usize);
        }
        let old = core::mem::replace(&mut self.data, vec![TID_EMPTY; newsize as usize]);
        self.update_parameters(newsize);
        let mut startelem = 0u32;
        for i in 0..oldsize as u32 {
            let k = old[i as usize];
            if k == TID_EMPTY {
                startelem = i;
                break;
            }
            if self.initial_bucket(hash_tid(k)) == i {
                startelem = i;
                break;
            }
        }
        let mut copyelem = startelem;
        for _ in 0..oldsize {
            let k = old[copyelem as usize];
            if k != TID_EMPTY {
                let mut cur = self.initial_bucket(hash_tid(k));
                while self.data[cur as usize] != TID_EMPTY {
                    cur = (cur + 1) & self.sizemask;
                }
                self.data[cur as usize] = k;
            }
            copyelem += 1;
            if copyelem as u64 >= oldsize {
                copyelem = 0;
            }
        }
        if let Some(mem) = mem {
            mem.pfree(TID_HASH_ENTRY_SIZE * oldsize as usize);
        }
    }

    /// tidhash_insert: returns `found`.
    pub fn insert(&mut self, blkno: u32, offno: u16, mut mem: Option<&mut AllocSetModel>) -> bool {
        let key = tid_key(blkno, offno);
        let hash = hash_tid(key);
        'restart: loop {
            let mut insertdist = 0u32;
            if self.members >= self.grow_threshold {
                assert!(self.size() != SH_MAX_SIZE, "hash table size exceeded");
                self.grow(self.size() * 2, mem.as_deref_mut());
            }
            let startelem = self.initial_bucket(hash);
            let mut curelem = startelem;
            loop {
                let occ = self.data[curelem as usize];
                if occ == TID_EMPTY {
                    self.members += 1;
                    self.data[curelem as usize] = key;
                    return false;
                }
                if occ == key {
                    return true;
                }
                let curoptimal = self.initial_bucket(hash_tid(occ));
                let curdist = self.distance_from_optimal(curoptimal, curelem);
                if insertdist > curdist {
                    let mut emptyelem = curelem;
                    let mut emptydist = 0u32;
                    loop {
                        emptyelem = (emptyelem + 1) & self.sizemask;
                        if self.data[emptyelem as usize] == TID_EMPTY {
                            break;
                        }
                        emptydist += 1;
                        if emptydist > SH_GROW_MAX_MOVE
                            && (self.members as f64 / self.size() as f64) >= SH_GROW_MIN_FILLFACTOR
                        {
                            self.grow_threshold = 0;
                            continue 'restart;
                        }
                    }
                    let mut moveelem = emptyelem;
                    while moveelem != curelem {
                        let prev = moveelem.wrapping_sub(1) & self.sizemask;
                        self.data[moveelem as usize] = self.data[prev as usize];
                        moveelem = prev;
                    }
                    self.members += 1;
                    self.data[curelem as usize] = key;
                    return false;
                }
                curelem = (curelem + 1) & self.sizemask;
                insertdist += 1;
                if insertdist > SH_GROW_MAX_DIB
                    && (self.members as f64 / self.size() as f64) >= SH_GROW_MIN_FILLFACTOR
                {
                    self.grow_threshold = 0;
                    continue 'restart;
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // Event shape seen in the C trace at the end of every ef=40 layer-0
    // search: A 64, A 128, A 256, F 128, A 512, F 256; and on emptying:
    // F 512, F 64.
    #[test]
    fn list_of_40_matches_c_trace_shape() {
        let mut a = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        let mut b = a.clone();
        let mut l = ListShape::lappend_n(40, &mut a);
        for s in [64usize, 128, 256] {
            b.palloc(s);
        }
        b.pfree(128);
        b.palloc(512);
        b.pfree(256);
        assert_eq!(a.mem_allocated(), b.mem_allocated());
        assert_eq!(a.nfree, b.nfree);
        for _ in 0..40 {
            l.delete_last(&mut a);
        }
        b.pfree(512);
        b.pfree(64);
        assert_eq!(a.nfree, b.nfree);
        assert_eq!(l, ListShape::NIL);
    }

    #[test]
    fn keeper_block_holds_58_elements_then_8k_block() {
        // 8192 - 200 (context) - 40 (block hdr) = 7952 = 58 * (128 + 8) + 64.
        let mut m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        for _ in 0..58 {
            m.palloc(HNSW_ELEMENT_DATA_SIZE);
        }
        assert_eq!(m.mem_allocated(), 8192);
        m.palloc(HNSW_ELEMENT_DATA_SIZE);
        assert_eq!(m.mem_allocated(), 8192 + 8192);
        // The 64-byte tail was carved into a 32- and a 16-byte free chunk.
        assert_eq!(m.nfree[2], 1);
        assert_eq!(m.nfree[1], 1);
        // tidhash for ef_search=40, m=16: the 48-byte header comes from the
        // second block's free space; the 2048 buckets (16384 B) exceed the
        // 8192-byte chunk limit, so they get a dedicated block of
        // 16384 + 40 (block header) + 8 (chunk header).
        let t = TidHash::create(40 * 16 * 2, Some(&mut m));
        assert_eq!(t.size(), 2048);
        assert_eq!(m.mem_allocated(), 8192 + 8192 + 16384 + 48);
    }

    #[test]
    fn fresh_scan_context_is_one_keeper_block() {
        let m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        assert_eq!(m.mem_allocated(), 8192);
    }

    #[test]
    fn tidhash_set_semantics() {
        assert_eq!(TidHash::create(32, None).size(), 64);
        let mut t = TidHash::create(32, None);
        assert!(!t.insert(7, 3, None));
        assert!(t.insert(7, 3, None));
        assert!(!t.insert(7, 4, None));
        for blk in 0..10_000u32 {
            t.insert(blk, (blk % 300) as u16 + 1, None);
        }
        for blk in 0..10_000u32 {
            assert!(t.insert(blk, (blk % 300) as u16 + 1, None));
        }
        assert!(t.members() >= 10_000);
        assert!(t.size().is_power_of_two() && t.size() as f64 * 0.9 >= t.members() as f64);
    }

    // Replay C's real allocation sequence (tests/data/README.md): the model
    // must equal C's MemoryContextMemAllocated at every checkpoint.
    #[test]
    fn replay_c_trace() {
        let trace = include_str!("../tests/data/hnswscan-tmpctx-043.trace");
        let mut m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        let mut checks = 0u32;
        for line in trace.lines() {
            // Skip the event letter; `num` yields the event's numbers in order.
            let mut it = line.split_whitespace().skip(1);
            let mut num = || it.next().unwrap().parse::<u64>().unwrap();
            match line.as_bytes()[0] {
                b'B' => {}
                b'R' => m.reset(),
                b'A' => m.palloc(num() as usize),
                b'F' => m.pfree(num() as usize),
                b'H' => {
                    let n = num() as u32;
                    let size = num();
                    assert_eq!(TidHash::create(n, None).size(), size, "tidhash_create({n})");
                }
                b'C' => {
                    assert_eq!(m.mem_allocated() as u64, num(), "checkpoint {checks}: {line}");
                    checks += 1;
                }
                _ => panic!("bad event {line}"),
            }
        }
        assert_eq!(checks, 523);
    }
}

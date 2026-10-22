use super::*;
use crate::memory::{layer_m, neighbor_array_size, ELEMENT_DATA_SIZE};

fn dist(a: &[u8], b: &[u8]) -> Result<f64, ()> {
    Ok((a[0] as f64 - b[0] as f64).abs())
}

fn params() -> InsertParams {
    InsertParams { m: 2, ef_construction: 4 }
}

fn tid(n: u8) -> HeapTid {
    [0, 0, 0, n, 0, 1]
}

fn insert(g: &Graph, x: u8, level: u8) -> Inserted {
    match g.insert(&[x], tid(x), &params(), || level, &mut dist, |_| Ok(())) {
        Ok(r) => r,
        Err(_) => panic!("insert of {x} failed"),
    }
}

fn layer_ids(g: &Graph, id: u32, lc: i32) -> Vec<u32> {
    let e = g.element(id);
    let links = e.links();
    links.neighbors[(e.level as i32 - lc) as usize].items.iter().map(|c| c.element).collect()
}

#[test]
fn arena_handles_cross_segments() {
    assert_eq!(arena::locate(0), (0, 0));
    assert_eq!(arena::locate(1023), (0, 1023));
    assert_eq!(arena::locate(1024), (1, 0));
    assert_eq!(arena::locate(3071), (1, 2047));
    assert_eq!(arena::locate(3072), (2, 0));
    assert_eq!(arena::locate(u32::MAX).0, 22);
}

// GenerationContextCreate(.., 1 MB, 1 MB, 1 MB) (hnswbuild.c:732-737): the
// keeper block shares the context's malloc, minus MAXALIGN(sizeof(
// GenerationContext)) = 128 (generation.c:229, 268).
#[test]
fn generation_model_starts_with_cs_keeper_block() {
    let g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    assert_eq!(g.mem_allocated(), (1 << 20) - 128);
}

// The keeper block holds 1048448 - 56 (block header) bytes; a 1000-byte
// request is a 1008-byte chunk with its header, so 1040 fit and the 1041st
// takes a new 1 MB block (generation.c:461-500).
#[test]
fn generation_model_adds_a_block_when_the_keeper_fills() {
    let mut g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    for _ in 0..1040 {
        g.alloc(1000);
    }
    assert_eq!(g.mem_allocated(), 1_048_448);
    g.alloc(1000);
    assert_eq!(g.mem_allocated(), 1_048_448 + (1 << 20));
}

// allocChunkLimit is 65536 for 1 MB blocks (generation.c:256-259); a larger
// chunk gets a block of its own (GenerationAllocLarge, generation.c:343-372).
#[test]
fn generation_model_gives_large_chunks_their_own_block() {
    let mut g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    g.alloc(65536);
    assert_eq!(g.mem_allocated(), 1_048_448);
    g.alloc(65537);
    assert_eq!(g.mem_allocated(), 1_048_448 + 65544 + 8 + 56);
}

// InitGraph sets memoryUsed = 0 (hnswbuild.c:625); HnswMemoryContextAlloc
// updates it to the context's block total after each allocation (:655).
#[test]
fn serial_accounting_is_zero_until_the_first_allocation() {
    let mut a = Accounting::serial(1 << 20);
    assert_eq!(a.used(), 0);
    assert!(!a.is_full());
    a.charge_element(0, 16, 20).unwrap();
    assert_eq!(a.used(), 1_048_448);
    assert!(!a.is_full());
}

// HnswSharedMemoryAlloc (hnswbuild.c:663-681): the MAXALIGNed sizes of
// HnswInitElement's allocations and the value (vector(3) is 20 bytes).
#[test]
fn parallel_accounting_charges_maxaligned_c_sizes() {
    let mut a = Accounting::parallel(2 << 20);
    a.charge_element(0, 16, 20).unwrap();
    assert_eq!(a.used(), ELEMENT_DATA_SIZE + 8 + neighbor_array_size(layer_m(16, 0)) + 24);
    assert_eq!(a.used(), 680);
}

// hnswbuild.c:961-969: maintenance_work_mem minus 3 MB, only when above 3 MB.
#[test]
fn parallel_area_matches_hnswbuild_c() {
    assert_eq!(Accounting::parallel_area_size(3073), 1024);
    assert_eq!(Accounting::parallel_area_size(3072), 3 << 20);
    assert_eq!(Accounting::parallel_area_size(4096), 1 << 20);
    assert_eq!(Accounting::parallel_area_size(65536), (64 << 20) - (3 << 20));
}

// The 1 MB parallel margin (hnswbuild.c:509-510, 533): an area of 1 MB or
// less is full before the first tuple (TAP 045's "after 0 tuples").
#[test]
fn parallel_margin_fills_a_small_area_before_the_first_tuple() {
    assert!(Accounting::parallel(1 << 20).is_full());
    assert!(!Accounting::parallel((1 << 20) + 1).is_full());
    assert!(Accounting::parallel(1024).is_full());
}

// hnswbuild.c:671-676.
#[test]
fn parallel_allocation_errors_match_c() {
    assert_eq!(
        Accounting::parallel(64 << 20).charge_element(0, 16, (1 << 20) + 1),
        Err(GraphError::AllocationTooLarge)
    );
    assert_eq!(Accounting::parallel(100).charge_element(0, 16, 20), Err(GraphError::OutOfMemory));
}

#[test]
fn first_insert_becomes_the_entry_point() {
    let g = Graph::new(Accounting::serial(64 << 20));
    assert_eq!(insert(&g, 10, 0), Inserted::InMemory);
    assert_eq!(g.entry_point(), Some(0));
    assert_eq!(g.head_newest_first(), vec![0]);
}

#[test]
fn a_higher_level_takes_over_the_entry_point_and_links_both_ways() {
    let g = Graph::new(Accounting::serial(64 << 20));
    insert(&g, 10, 0);
    insert(&g, 20, 1);
    assert_eq!(g.entry_point(), Some(1));
    assert_eq!(layer_ids(&g, 0, 0), vec![1]);
    assert_eq!(layer_ids(&g, 1, 0), vec![0]);
    assert_eq!(layer_ids(&g, 1, 1), Vec::<u32>::new());
    assert_eq!(g.head_newest_first(), vec![1, 0]);
}

// FindDuplicateInMemory / AddDuplicateInMemory (hnswbuild.c:321-364): the
// first element takes ten TIDs; the eleventh copy finds it full and becomes
// an element of its own. Merged copies stay allocated (C never frees them).
#[test]
fn duplicates_merge_heap_tids_until_ten() {
    let g = Graph::new(Accounting::serial(64 << 20));
    for _ in 0..11 {
        insert(&g, 5, 0);
    }
    assert_eq!(g.element(0).links().heaptids_len, 10);
    assert_eq!(g.head_newest_first(), vec![10, 0]);
    assert_eq!(g.element_count(), 11);
}

// hnswbuild.c:533-552: the first tuple that does not fit flushes (once), and
// it and every later tuple go to disk; none draws a level.
#[test]
fn overflow_flushes_once_then_goes_to_disk() {
    let g = Graph::new(Accounting::parallel(1 << 20));
    let mut flushes = 0;
    for x in 0..3u8 {
        let r = g.insert(
            &[x],
            tid(x),
            &params(),
            || -> u8 { panic!("a tuple that goes to disk draws no level here") },
            &mut dist,
            |gr| {
                flushes += 1;
                assert_eq!(gr.indtuples(), 0.0);
                Ok(())
            },
        );
        assert_eq!(r.unwrap_or_else(|_| panic!("insert")), Inserted::OnDisk);
    }
    assert_eq!(flushes, 1);
    assert!(g.flushed());
    assert_eq!(g.element_count(), 0);
}

// BuildGraph's final flush (hnswbuild.c:1119-1120) runs once, and later
// inserts go to disk.
#[test]
fn flush_if_needed_flushes_once() {
    let g = Graph::new(Accounting::serial(64 << 20));
    insert(&g, 1, 0);
    let mut n = 0;
    g.flush_if_needed(|gr| {
        n += 1;
        assert_eq!(gr.head_newest_first(), vec![0]);
        Ok::<(), ()>(())
    })
    .unwrap();
    g.flush_if_needed(|_| {
        n += 1;
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(n, 1);
    assert_eq!(insert(&g, 2, 0), Inserted::OnDisk);
}

// C HnswUpdateConnection mutates neighbors->items[i].closer through the
// shared candidate-list pointers; these pin that the in-memory port writes
// the computed closer flags (and the flagged newHc) back into the neighbor
// array instead of leaving stale flags for later pruning. Moved from
// pgvector_hnsw_build (M1), with one-dimensional float4 values in place of
// vector images and vector.c's squared L2 distance.
fn f32_value(x: f32) -> [u8; 4] {
    x.to_le_bytes()
}

fn l2_squared(a: &[u8], b: &[u8]) -> Result<f64, ()> {
    let x = f32::from_le_bytes(a[..4].try_into().unwrap());
    let y = f32::from_le_bytes(b[..4].try_into().unwrap());
    let d = x - y;
    Ok((d * d) as f64)
}

// New candidate survives selection: the pruned slot is replaced by the newHc
// carrying its computed closer=true, and the surviving original neighbor's
// recomputed closer=true is written back.
#[test]
fn update_connection_writes_back_closer_flags_on_replace() {
    let g = Graph::new(Accounting::serial(64 << 20));
    // element 0 at 1.0, element 1 at 1.1, element 2 (new) at -1.05; the owner
    // is conceptually at 0.0, distances squared below.
    for x in [1.0f32, 1.1, -1.05] {
        g.push_unlinked(0, &f32_value(x));
    }
    // Stale flags deliberately wrong (false); C recomputes in place.
    let mut neighbors = vec![
        Candidate { element: 0, distance: 1.0, closer: false },
        Candidate { element: 1, distance: 1.21, closer: false },
    ];
    let mut closer_set = false;
    g.update_connection(&mut neighbors, &mut closer_set, 2, 1.1025, 2, &mut l2_squared).unwrap();
    assert!(closer_set, "sortCandidates=true sets closerSet");
    // Selection keeps 0 (closer) and new 2 (closer vs {0}: d(2,0)^2=4.2 > 1.1025);
    // 1 is never popped (r fills first), pruned = leftover 1 -> replaced by newHc.
    let flags: Vec<(u32, bool)> = neighbors.iter().map(|n| (n.element, n.closer)).collect();
    assert_eq!(flags, vec![(0, true), (2, true)]);
}

// New candidate is itself pruned (kept-neighbor case): no replacement, but
// the surviving array must carry the freshly computed flags, including a
// false flag for the not-closer neighbor (wd-kept).
#[test]
fn update_connection_writes_back_closer_flags_without_replace() {
    let g = Graph::new(Accounting::serial(64 << 20));
    // 0 at 1.0, 1 at 1.1, new 2 at 1.2: 1 and 2 are both not-closer to 0;
    // wd-fill keeps 1, prunes the new candidate 2.
    for x in [1.0f32, 1.1, 1.2] {
        g.push_unlinked(0, &f32_value(x));
    }
    // Stale flags deliberately wrong (true).
    let mut neighbors = vec![
        Candidate { element: 0, distance: 1.0, closer: true },
        Candidate { element: 1, distance: 1.21, closer: true },
    ];
    let mut closer_set = false;
    g.update_connection(&mut neighbors, &mut closer_set, 2, 1.44, 2, &mut l2_squared).unwrap();
    assert!(closer_set);
    let flags: Vec<(u32, bool)> = neighbors.iter().map(|n| (n.element, n.closer)).collect();
    assert_eq!(flags, vec![(0, true), (1, false)]);
}

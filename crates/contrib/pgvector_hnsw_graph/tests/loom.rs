//! Loom models of the HNSW build graph's lock protocol (hnswbuild.c:437-580):
//! the real Graph::insert, its entry-point protocol, element locks,
//! duplicate merge and flush, explored under loom's interleavings.
//!
//! Run: RUSTFLAGS="--cfg loom" cargo test -p pgvector_hnsw_graph --test loom --release
//!
//! Values are one byte and the distance is their absolute difference. Handles
//! depend on allocation order, so the models find elements by value.
#![cfg(loom)]

use std::collections::HashSet;
use std::sync::Arc;

use pgvector_hnsw_graph::{Accounting, Graph, HeapTid, InsertParams, Inserted};

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

fn by_value(g: &Graph, x: u8) -> u32 {
    (0..g.element_count()).find(|&id| g.element(id).value() == [x]).expect("an element with that value")
}

fn layer0(g: &Graph, id: u32) -> Vec<u32> {
    let e = g.element(id);
    let links = e.links();
    links.neighbors[e.level as usize].items.iter().map(|c| c.element).collect()
}

/// After concurrent inserts: the entry point has the top level, every element
/// on the head list is reachable from it, and no neighbor array names its own
/// element or one element twice.
fn assert_well_formed(g: &Graph) {
    let head = g.head_newest_first();
    let ep = g.entry_point().expect("an entry point");
    let top = head.iter().map(|&e| g.element(e).level).max().expect("elements");
    assert_eq!(g.element(ep).level, top);
    let mut seen = HashSet::new();
    let mut stack = vec![ep];
    while let Some(e) = stack.pop() {
        if !seen.insert(e) {
            continue;
        }
        let links = g.element(e).links();
        for na in &links.neighbors {
            let mut ids = HashSet::new();
            for c in &na.items {
                assert_ne!(c.element, e, "element {e} names itself");
                assert!(ids.insert(c.element), "element {e} names {} twice", c.element);
                stack.push(c.element);
            }
        }
    }
    for e in &head {
        assert!(seen.contains(e), "element {e} is unreachable");
    }
}

fn model(bound: usize, f: impl Fn() + Sync + Send + 'static) {
    let mut b = loom::model::Builder::new();
    b.preemption_bound = Some(bound);
    b.check(f);
}

// Two first inserts race for the empty graph's entry point: both take
// entryLock exclusively, one after the other, and link each other.
#[test]
fn concurrent_first_inserts_serialize_on_the_entry_lock() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 10, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 20, 1));
        assert_eq!(t1.join().unwrap(), Inserted::InMemory);
        assert_eq!(t2.join().unwrap(), Inserted::InMemory);
        assert_eq!(g.head_newest_first().len(), 2);
        assert_well_formed(&g);
        let (a, b) = (by_value(&g, 10), by_value(&g, 20));
        assert_eq!(layer0(&g, a), vec![b]);
        assert_eq!(layer0(&g, b), vec![a]);
    });
}

// Two level-0 inserts share entryLock and search from the same entry point
// at once: each may miss the other, but both link to it and it to both.
#[test]
fn concurrent_inserts_under_a_shared_entry_lock_stay_reachable() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 50, 1);
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 40, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 60, 0));
        t1.join().unwrap();
        t2.join().unwrap();
        assert_well_formed(&g);
        let mut l0 = layer0(&g, by_value(&g, 50));
        l0.sort_unstable();
        let mut want = vec![by_value(&g, 40), by_value(&g, 60)];
        want.sort_unstable();
        assert_eq!(l0, want);
    });
}

// Review Focus 4: two inserts of a value already in the graph both merge
// their heap TIDs into it under its lock; neither becomes an element.
#[test]
fn concurrent_duplicates_merge_into_one_element() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 5, 0);
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 5, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 5, 0));
        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(g.head_newest_first(), vec![0]);
        assert_eq!(g.element(0).links().heaptids_len, 3);
    });
}

// One insert fits the graph and the other overflows it: the overflowing one
// flushes exactly once, only after the in-memory insert finished (flushLock),
// and goes to disk.
#[test]
fn a_flush_waits_for_the_insert_in_flight() {
    model(3, || {
        // C's serial accounting: the first element's allocation brings
        // memoryUsed to the 1 MB keeper block (1048448 bytes), so a graph of
        // that size is full after one element.
        let g = Arc::new(Graph::new(Accounting::serial((1 << 20) - 128)));
        let flushes = Arc::new(loom::sync::atomic::AtomicUsize::new(0));
        let run = |x: u8| {
            let g = Arc::clone(&g);
            let flushes = Arc::clone(&flushes);
            loom::thread::spawn(move || {
                g.insert(&[x], tid(x), &params(), || 0, &mut dist, |gr| {
                    // Under the exclusive flush lock: the in-memory insert completed.
                    assert_eq!(gr.head_newest_first().len(), 1);
                    assert!(gr.entry_point().is_some());
                    flushes.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
                .unwrap_or_else(|_| panic!("insert of {x} failed"))
            })
        };
        let (t1, t2) = (run(1), run(2));
        let mut results = vec![t1.join().unwrap(), t2.join().unwrap()];
        results.sort_by_key(|r| *r == Inserted::OnDisk);
        assert_eq!(results, vec![Inserted::InMemory, Inserted::OnDisk]);
        assert_eq!(flushes.load(loom::sync::atomic::Ordering::SeqCst), 1);
        assert!(g.flushed());
    });
}

// Three inserts over a one-element graph, one of them raising the entry
// point: shared and exclusive entry-lock paths interleave.
#[test]
fn three_inserts_with_an_entry_point_raise() {
    model(2, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 30, 0);
        let handles: Vec<_> = [(20u8, 0u8), (40, 1), (35, 0)]
            .into_iter()
            .map(|(x, l)| {
                let g = Arc::clone(&g);
                loom::thread::spawn(move || insert(&g, x, l))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), Inserted::InMemory);
        }
        assert_eq!(g.head_newest_first().len(), 4);
        assert_well_formed(&g);
        assert_eq!(g.entry_point(), Some(by_value(&g, 40)));
    });
}

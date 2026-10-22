//! The in-memory search and graph update: hnswutils.c's HnswSearchLayer,
//! SelectNeighbors, CheckElementCloser, HnswUpdateConnection and
//! HnswFindElementNeighbors with index == NULL, and hnswbuild.c's
//! InsertTupleInMemory, UpdateGraphInMemory, FindDuplicateInMemory and
//! UpdateNeighborsInMemory. Moved from pgvector_hnsw_build (M1), with C's
//! locks added where C takes them.

use std::collections::HashSet;

use pairingheap::PairingHeap;
use pgsync::{RwLockReadGuard, RwLockWriteGuard};

use crate::memory::layer_m;
use crate::{read, write, Candidate, Graph, InsertParams, NeighborArray, HNSW_HEAPTIDS};

type CandHeap = PairingHeap<(f64, u32), fn(&(f64, u32), &(f64, u32)) -> i32>;

// CompareNearestCandidates (hnswutils.c:632-641): the nearest candidate first.
fn compare_nearest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        1
    } else if a.0 > b.0 {
        -1
    } else {
        0
    }
}

// CompareFurthestCandidates (hnswutils.c:662-671): the furthest candidate first.
fn compare_furthest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        -1
    } else if a.0 > b.0 {
        1
    } else {
        0
    }
}

fn nearest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_nearest_candidates)
}

fn furthest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_furthest_candidates)
}

/// entryLock, held in shared or exclusive mode.
enum EntryGuard<'g> {
    Shared(RwLockReadGuard<'g, Option<u32>>),
    Exclusive(RwLockWriteGuard<'g, Option<u32>>),
}

impl EntryGuard<'_> {
    fn entry_point(&self) -> Option<u32> {
        match self {
            EntryGuard::Shared(g) => **g,
            EntryGuard::Exclusive(g) => **g,
        }
    }
}

impl Graph {
    // InsertTupleInMemory (hnswbuild.c:437-480) and UpdateGraphInMemory
    // (hnswbuild.c:413-432).
    pub(crate) fn insert_in_memory<E>(
        &self,
        element: u32,
        p: &InsertParams,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let level = self.element(element).level;

        // Wait if another process needs exclusive lock on entry lock
        drop(pgsync::lock(&self.entry_wait_lock));

        // Get entry point
        let mut guard = EntryGuard::Shared(read(&self.entry_lock));
        let mut entry_point = guard.entry_point();

        // Prevent concurrent inserts when likely updating entry point
        if entry_point.is_none_or(|ep| level > self.element(ep).level) {
            // Release shared lock
            drop(guard);
            // Tell other processes to wait and get exclusive lock
            let wait = pgsync::lock(&self.entry_wait_lock);
            guard = EntryGuard::Exclusive(write(&self.entry_lock));
            drop(wait);
            // Get latest entry point after lock is acquired
            entry_point = guard.entry_point();
        }

        // Find neighbors for element
        self.find_element_neighbors(element, entry_point, p, distance)?;

        // Look for duplicate
        if self.find_duplicate(element) {
            return Ok(());
        }

        // Add element (AddElementInMemory, hnswbuild.c:369-376)
        pgsync::lock(&self.list).head.push(element);

        // Update neighbors
        self.update_neighbors(element, p.m, distance)?;

        // Update entry point if needed (already have lock)
        if entry_point.is_none_or(|ep| level > self.element(ep).level) {
            match &mut guard {
                EntryGuard::Exclusive(g) => **g = Some(element),
                // The entry point only rises under the exclusive lock, so a
                // shared holder never sees an element above it.
                EntryGuard::Shared(_) => unreachable!("entry point raised under a shared entry lock"),
            }
        }

        // Release entry lock
        drop(guard);
        Ok(())
    }

    // HnswFindElementNeighbors (hnswutils.c:1283-1360), in memory.
    fn find_element_neighbors<E>(
        &self,
        element: u32,
        entry_point: Option<u32>,
        p: &InsertParams,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let e = self.element(element);
        let q = e.value();
        let Some(entry_point) = entry_point else { return Ok(()) };

        let entry_level = self.element(entry_point).level as i32;
        let ep_dist = distance(q, self.element(entry_point).value())?;
        let mut ep: Vec<(u32, f64)> = vec![(entry_point, ep_dist)];

        let mut lc = entry_level;
        // lc >= level + 1 (hnswutils.c:1309)
        while lc > e.level as i32 {
            ep = self.search_layer(q, ep, 1, lc, distance)?;
            lc -= 1;
        }

        let level = (e.level as i32).min(entry_level);
        let mut lc = level;
        loop {
            let lm = layer_m(p.m, lc);
            let w = self.search_layer(q, ep.clone(), p.ef_construction, lc, distance)?;

            let mut lw: Vec<Candidate> =
                w.iter().map(|&(id, d)| Candidate { element: id, distance: d as f32, closer: false }).collect();

            let layer_idx = (e.level as i32 - lc) as usize;
            // The new element is not reachable yet, so no other participant
            // reads its links (C writes them unlocked, hnswutils.c:1173-1181);
            // its own lock is uncontended.
            let mut closer_set = e.links().neighbors[layer_idx].closer_set;
            let mut selected: Vec<Candidate> = Vec::new();
            self.select_neighbors(&mut lw, lm, &mut closer_set, None, None, false, &mut selected, distance)?;
            e.links_mut().neighbors[layer_idx] = NeighborArray { items: selected, closer_set };

            ep = w;
            if lc == 0 {
                break;
            }
            lc -= 1;
        }
        Ok(())
    }

    // HnswSearchLayer (hnswutils.c:828-990) with index == NULL. The
    // candidate's neighborhood is copied under its lock in shared mode
    // (HnswLoadUnvisitedFromMemory, hnswutils.c:737-759).
    fn search_layer<E>(
        &self,
        q: &[u8],
        ep: Vec<(u32, f64)>,
        ef: i32,
        lc: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<Vec<(u32, f64)>, E> {
        let mut visited: HashSet<u32> = HashSet::new();
        let mut c_heap = nearest_candidate_heap();
        let mut w_heap = furthest_candidate_heap();
        let mut wlen: i32 = 0;

        for &(e, d) in &ep {
            visited.insert(e);
            c_heap.add((d, e));
            w_heap.add((d, e));
            wlen += 1;
        }

        while let Some((c_dist, c_elem)) = c_heap.remove_first() {
            let (f_dist, _) = *w_heap.first().expect("W nonempty");
            if c_dist > f_dist {
                break;
            }
            let c = self.element(c_elem);
            let neighbor_ids: Vec<u32> = {
                let links = c.links();
                links.neighbors[(c.level as i32 - lc) as usize].items.iter().map(|hc| hc.element).collect()
            };

            for e in neighbor_ids {
                if !visited.insert(e) {
                    continue;
                }
                let always_add = wlen < ef;
                let (f_dist, _) = *w_heap.first().expect("W nonempty");
                let e_distance = distance(q, self.element(e).value())?;
                if !(e_distance < f_dist || always_add) {
                    continue;
                }
                // Make robust to issues
                if (self.element(e).level as i32) < lc {
                    continue;
                }
                c_heap.add((e_distance, e));
                w_heap.add((e_distance, e));
                wlen += 1;
                if wlen > ef {
                    w_heap.remove_first();
                }
            }
        }

        let mut w: Vec<(u32, f64)> = Vec::new();
        while let Some((d, e)) = w_heap.remove_first() {
            w.push((e, d));
        }
        Ok(w)
    }

    // CheckElementCloser (hnswutils.c:1043-1062): reads only values.
    fn check_element_closer<E>(
        &self,
        e: &Candidate,
        r: &[Candidate],
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<bool, E> {
        let e_value = self.element(e.element).value();
        for ri in r {
            let d = distance(e_value, self.element(ri.element).value())? as f32;
            if d <= e.distance {
                return Ok(false);
            }
        }
        Ok(true)
    }

    // SelectNeighbors (hnswutils.c:1065-1168). C's candidate list holds
    // pointers into the caller's neighbor array, so e->closer updates land in
    // that array; `c` is taken mutably and the computed closer flags are
    // written back by origin index. Tie order: equal distances break by element
    // handle (hnswutils.c:996-1038): C's pointer order within the serial
    // build's Generation context, C's relptr order in a parallel build's area;
    // handles rise with allocation order in both.
    #[allow(clippy::too_many_arguments)]
    fn select_neighbors<E>(
        &self,
        c: &mut [Candidate],
        lm: i32,
        closer_set: &mut bool,
        new_candidate: Option<usize>,
        pruned: Option<&mut Option<Candidate>>,
        sort_candidates: bool,
        out: &mut Vec<Candidate>,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        out.clear();
        if c.len() as i32 <= lm {
            out.extend_from_slice(c);
            return Ok(());
        }

        let mut w: Vec<Candidate> = c.to_vec();
        let mut w_is_new: Vec<bool> = vec![false; w.len()];
        // Origin index into `c` for closer-flag write-back.
        let mut w_src: Vec<usize> = (0..w.len()).collect();
        if let Some(nc) = new_candidate {
            w_is_new[nc] = true;
        }
        if sort_candidates {
            let mut order: Vec<usize> = (0..w.len()).collect();
            order.sort_by(|&a, &b| {
                w[b].distance
                    .partial_cmp(&w[a].distance)
                    .unwrap_or(core::cmp::Ordering::Equal)
                    .then_with(|| w[b].element.cmp(&w[a].element))
            });
            let neww: Vec<Candidate> = order.iter().map(|&i| w[i]).collect();
            let newn: Vec<bool> = order.iter().map(|&i| w_is_new[i]).collect();
            let news: Vec<usize> = order.iter().map(|&i| w_src[i]).collect();
            w = neww;
            w_is_new = newn;
            w_src = news;
        }

        let must_calculate = !*closer_set;
        let mut wd: Vec<Candidate> = Vec::with_capacity(w.len());
        let mut added: Vec<Candidate> = Vec::new();
        let mut removed_any = false;

        while !w.is_empty() && (out.len() as i32) < lm {
            let mut e = w.pop().expect("nonempty");
            let e_is_new = w_is_new.pop().expect("nonempty");
            let e_src = w_src.pop().expect("nonempty");

            if must_calculate {
                e.closer = self.check_element_closer(&e, out, distance)?;
            } else if !added.is_empty() {
                if e.closer {
                    e.closer = self.check_element_closer(&e, &added, distance)?;
                    if !e.closer {
                        removed_any = true;
                    }
                } else if removed_any {
                    e.closer = self.check_element_closer(&e, out, distance)?;
                    if e.closer {
                        added.push(e);
                    }
                }
            } else if e_is_new {
                e.closer = self.check_element_closer(&e, out, distance)?;
                if e.closer {
                    added.push(e);
                }
            }

            // C writes e->closer through the shared pointer; mirror into `c`.
            c[e_src].closer = e.closer;

            if e.closer {
                out.push(e);
            } else {
                wd.push(e);
            }
        }

        *closer_set = sort_candidates;

        let mut wdoff = 0usize;
        while wdoff < wd.len() && (out.len() as i32) < lm {
            out.push(wd[wdoff]);
            wdoff += 1;
        }
        if let Some(p) = pruned {
            *p = if wdoff < wd.len() { Some(wd[wdoff]) } else { w.first().copied() };
        }
        Ok(())
    }

    // HnswUpdateConnection (hnswutils.c:1186-1234), in memory: link
    // `new_element` into `neighbors`, which the caller holds under the
    // neighbor's lock in exclusive mode. C's SelectNeighbors candidate list
    // aliases neighbors->items, so the closer flags it computes persist in the
    // array (and the replacement newHc carries its computed flag); both
    // write-backs are mirrored here.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_connection<E>(
        &self,
        neighbors: &mut Vec<Candidate>,
        closer_set: &mut bool,
        new_element: u32,
        new_distance: f32,
        lm: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let new_hc = Candidate { element: new_element, distance: new_distance, closer: false };
        if (neighbors.len() as i32) < lm {
            neighbors.push(new_hc);
            return Ok(());
        }

        // Shrink connections.
        let mut c: Vec<Candidate> = neighbors.clone();
        c.push(new_hc);
        let new_idx = c.len() - 1;
        let mut pruned: Option<Candidate> = None;
        let mut selected: Vec<Candidate> = Vec::new();
        self.select_neighbors(&mut c, lm, closer_set, Some(new_idx), Some(&mut pruned), true, &mut selected, distance)?;
        // Closer flags computed in place land in the neighbor array even on
        // the pruned == NULL early return (c[0..len] is index-aligned with
        // neighbors).
        for (slot, cand) in neighbors.iter_mut().zip(c.iter()) {
            slot.closer = cand.closer;
        }
        // Should not happen (C returns without linking).
        let Some(pruned) = pruned else { return Ok(()) };
        for slot in neighbors.iter_mut() {
            if slot.element == pruned.element {
                *slot = c[new_idx];
                break;
            }
        }
        Ok(())
    }

    // UpdateNeighborsInMemory (hnswbuild.c:381-408): one element lock at a
    // time; the distance function runs under the neighbor's exclusive lock, as
    // in C.
    fn update_neighbors<E>(
        &self,
        e: u32,
        m: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let element = self.element(e);
        let level = element.level as i32;
        for lc in (0..=level).rev() {
            let lm = layer_m(m, lc);
            // Copy neighbors to local memory
            let items: Vec<Candidate> = element.links().neighbors[(level - lc) as usize].items.clone();
            for hc in items {
                let neighbor = self.element(hc.element);
                let mut links = neighbor.links_mut();
                let na = &mut links.neighbors[(neighbor.level as i32 - lc) as usize];
                self.update_connection(&mut na.items, &mut na.closer_set, e, hc.distance, lm, distance)?;
            }
        }
        Ok(())
    }

    // FindDuplicateInMemory and AddDuplicateInMemory (hnswbuild.c:321-364).
    fn find_duplicate(&self, element: u32) -> bool {
        let e = self.element(element);
        // The new element is private: its layer-0 neighbors and TID are read
        // before taking any other element's lock.
        let (neighbor_ids, tid) = {
            let links = e.links();
            let ids: Vec<u32> = links.neighbors[e.level as usize].items.iter().map(|hc| hc.element).collect();
            (ids, links.heaptids[0])
        };
        for dup in neighbor_ids {
            let d = self.element(dup);
            // Exit early since ordered by distance
            if e.value() != d.value() {
                return false;
            }
            // Check for space
            let mut links = d.links_mut();
            if (links.heaptids_len as usize) < HNSW_HEAPTIDS {
                let n = links.heaptids_len as usize;
                links.heaptids[n] = tid;
                links.heaptids_len += 1;
                return true;
            }
        }
        false
    }
}

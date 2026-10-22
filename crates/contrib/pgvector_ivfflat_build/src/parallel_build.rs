//! ivfbuild.c's parallel assign phase: IvfflatBeginParallel,
//! IvfflatParallelBuildMain, IvfflatParallelScanAndSort, ParallelHeapScan and
//! IvfflatEndParallel (ivfbuild.c:626-972), on pgvector_parallel's launched
//! workers. Sampling, k-means, the list pages and the load stay serial on the
//! leader, as in C.
//!
//! DIVERGENCE (spec §7.3): C's participants each sort their share into a tape,
//! and the leader merges the tapes through a SharedFileSet (ivfbuild.c:661-720,
//! 994-1000). pgrust's Tuplesort is thread-affine (nbtsort/src/pool.rs:20-44),
//! so each worker assigns its rows to their nearest center and sends
//! (list, tid, value) batches to the leader through its own mailbox, and the
//! leader feeds them to its one sort. List membership is C's. The order within
//! a list differs, which C's own sort, keyed on the list only
//! (ivfbuild.c:612-621), does not fix either. The parallel phase ends after
//! the scan: there are no shared tapes to keep alive through the load.

use std::sync::Arc;

use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgsync::{mailbox, MailboxReceiver, MailboxSender, Mutex, TryRecv};
use pgvector_ivfflat::sort::HeapSort;
use tableam::ParallelTableScanDescShared;
use types_core::Oid;
use types_error::{PgError, PgResult};
use types_rel::Relation;
use types_tuple::itemptr::ItemPointerData;

use crate::{add_tuple_to_sort, assign_tuple, init_build_state, BuildState};

const ENTRY_NAME: &str = "pgrust_ivfflat_parallel_build_main";
/// A worker sends its batch once the values reach this many bytes.
const BATCH_BYTES: usize = 64 * 1024;
/// Batches in flight in a worker's mailbox before the worker waits for the
/// leader.
const MAILBOX_BATCHES: usize = 64;

/// IvfflatShared (ivfflat.h:151-173), the centers (PARALLEL_KEY_IVFFLAT_CENTERS)
/// and each worker's channel to the leader.
struct IvfflatShared {
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
    pscan: ParallelTableScanDescShared,
    /// The leader's centers->items (ivfbuild.c:925-926), `itemsize` bytes per list.
    centers: Vec<u8>,
    /// One mailbox per worker, indexed by ParallelWorkerNumber. A pgsync
    /// mailbox wakes one parked sender (its single sender_wake slot): workers
    /// sharing one mailbox could each wait on it while full, and a pop would
    /// wake only the last of them.
    rows: Vec<MailboxSender<AssignBatch>>,
    /// The leader's ProcNumber, to wake it when rows arrive or a worker finishes.
    leader: usize,
    query_text: Option<String>,
    stats: Mutex<IvfflatStats>,
}

#[derive(Default)]
struct IvfflatStats {
    nparticipantsdone: i32,
    reltuples: f64,
    indtuples: f64,
}

/// Rows a participant assigned: the list, the heap TID and the value the sort
/// stores, each value 8-byte aligned.
struct AssignBatch {
    lists: Vec<i32>,
    tids: Vec<ItemPointerData>,
    offsets: Vec<usize>,
    words: Vec<u64>,
}

impl AssignBatch {
    fn new() -> AssignBatch {
        AssignBatch { lists: Vec::new(), tids: Vec::new(), offsets: Vec::new(), words: Vec::new() }
    }

    fn push(&mut self, list: i32, tid: ItemPointerData, value: &[u8]) {
        let offset = self.words.len();
        self.words.resize(offset + value.len().div_ceil(8), 0);
        // SAFETY: the words just added span at least value.len() bytes, and
        // u8 has no alignment or validity requirement.
        unsafe {
            core::slice::from_raw_parts_mut(self.words[offset..].as_mut_ptr().cast::<u8>(), value.len())
        }
        .copy_from_slice(value);
        self.lists.push(list);
        self.tids.push(tid);
        self.offsets.push(offset);
    }

    fn len(&self) -> usize {
        self.lists.len()
    }

    fn bytes(&self) -> usize {
        self.words.len() * 8
    }

    fn value_ptr(&self, i: usize) -> *const u8 {
        self.words[self.offsets[i]..].as_ptr().cast::<u8>()
    }

    /// Add every row to the leader's sort, as AddTupleToSort does
    /// (ivfbuild.c:206-222); the sort copies the TID and the value.
    fn put_all(&self, sort: &mut HeapSort) -> PgResult<()> {
        for i in 0..self.len() {
            sort.put(&[
                Datum::from_i32(self.lists[i]),
                Datum::from_usize(&self.tids[i] as *const ItemPointerData as usize),
                Datum::from_usize(self.value_ptr(i) as usize),
            ])?;
        }
        Ok(())
    }
}

/// IvfflatBeginParallel, IvfflatLeaderParticipateAsWorker, ParallelHeapScan
/// and IvfflatEndParallel (ivfbuild.c:626-656, 813-972). Ok(false) when no
/// worker launched: the caller then scans serially (ivfbuild.c:953-958).
pub(crate) fn assign_tuples_parallel<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
    request: i32,
) -> PgResult<bool> {
    let isconcurrent = index_info.ii_Concurrent;
    let (heaprelid, indexrelid) = (heap.rd_id, bs.index.rd_id);
    // Store centers in shared memory (ivfbuild.c:925-926)
    let mut centers = Vec::with_capacity(bs.centers.maxlen as usize * bs.centers.itemsize);
    for i in 0..bs.centers.maxlen {
        centers.extend_from_slice(bs.centers.get(i)?);
    }
    // One mailbox per worker (IvfflatShared::rows); a worker that does not
    // launch never sends to its own.
    let (rows, receivers): (Vec<_>, Vec<_>) =
        (0..request).map(|_| mailbox(Some(MAILBOX_BATCHES))).unzip();
    let Some((leader, shared)) = pgvector_parallel::begin_parallel(
        ENTRY_NAME,
        ivfflat_parallel_build_main,
        request,
        heap,
        isconcurrent,
        move |pscan| {
            Ok(Arc::new(IvfflatShared {
                heaprelid,
                indexrelid,
                isconcurrent,
                pscan,
                centers,
                rows,
                leader: pgvector_parallel::my_proc_number(),
                query_text: pgvector_parallel::leader_query_text(),
                stats: Mutex::new(IvfflatStats::default()),
            }))
        },
        ("ivfbuild.c", 961, "IvfflatBeginParallel"),
    )?
    else {
        return Ok(false);
    };
    let nparticipants = leader.nworkers_launched + 1;

    // IvfflatLeaderParticipateAsWorker (ivfbuild.c:813-830): the leader's own
    // rows go straight into its sort, and the workers' batches are fed in
    // between them.
    leader_scan(bs, mcx, heap, index_info, &shared, &receivers)?;

    // ivfbuild.c:971
    parallel::WaitForParallelWorkersToAttach(leader.pcxt)?;

    // ParallelHeapScan (ivfbuild.c:626-656): wait for every participant,
    // feeding their rows to the sort. A worker's error is raised by
    // check_for_interrupts here.
    let sort = bs.sortstate.as_mut().expect("sort state");
    loop {
        drain(&receivers, sort)?;
        let (done, reltuples, indtuples) = {
            let s = pgsync::lock(&shared.stats);
            (s.nparticipantsdone, s.reltuples, s.indtuples)
        };
        if done == nparticipants {
            // Every participant sent its last batch before counting itself done.
            drain(&receivers, sort)?;
            bs.reltuples = reltuples;
            bs.indtuples = indtuples;
            break;
        }
        parallel::wait_parallel_finish_quantum()?;
        postgres_seams::check_for_interrupts::call()?;
    }

    pgvector_parallel::end_parallel(leader)?;
    Ok(true)
}

/// The leader's share of the scan (IvfflatParallelScanAndSort with
/// progress = true, ivfbuild.c:661-720).
fn leader_scan<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
    shared: &IvfflatShared,
    receivers: &[MailboxReceiver<AssignBatch>],
) -> PgResult<()> {
    let index = bs.index;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        index_info,
        /* progress */ true,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            // Skip nulls
            if isnull[0] {
                return Ok(());
            }
            add_tuple_to_sort(bs, tid, values)
                .and_then(|()| drain(receivers, bs.sortstate.as_mut().expect("sort state")))
                .map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat build insert failed").into()
                })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };

    // Record statistics (ivfbuild.c:698-705)
    {
        let mut s = pgsync::lock(&shared.stats);
        s.nparticipantsdone += 1;
        s.reltuples += reltuples;
        s.indtuples += bs.indtuples;
    }
    pgvector_parallel::report_processed(true, reltuples, "ivfbuild.c", 709, 711, "IvfflatParallelScanAndSort")
}

/// Feed every batch waiting in the workers' mailboxes to the leader's sort.
fn drain(receivers: &[MailboxReceiver<AssignBatch>], sort: &mut HeapSort) -> PgResult<()> {
    for receiver in receivers {
        // Empty or Disconnected: on to the next worker's mailbox.
        while let TryRecv::Msg(batch) = receiver.try_recv() {
            batch.put_all(sort)?;
        }
    }
    Ok(())
}

/// IvfflatParallelBuildMain (ivfbuild.c:725-783).
fn ivfflat_parallel_build_main(pshared: &parallel::ParallelShared) -> PgResult<()> {
    let shared = pshared
        .private()
        .expect("ivfflat parallel build worker without its shared state")
        .downcast::<IvfflatShared>()
        .unwrap_or_else(|_| panic!("ivfflat parallel build private state is not IvfflatShared"));
    pgvector_parallel::with_worker_activity(shared.query_text.as_deref(), || {
        let cx = mcx::MemoryContext::new("Ivfflat parallel build worker");
        let mcx = cx.mcx();
        let rels = pgvector_parallel::worker_open(mcx, shared.heaprelid, shared.indexrelid, shared.isconcurrent)?;
        worker_scan(mcx, &rels.heap, &rels.index, &shared)?;
        pgvector_parallel::worker_close(rels)
    })
}

/// IvfflatParallelScanAndSort (ivfbuild.c:661-720), a worker's side.
fn worker_scan<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    shared: &IvfflatShared,
) -> PgResult<()> {
    // Join parallel scan
    let mut index_info = execindexing::BuildIndexInfo(mcx, index)?;
    index_info.ii_Concurrent = shared.isconcurrent;
    let build_cx = mcx::MemoryContext::new("Ivfflat build context");
    let mut bs = init_build_state(Some(heap), index, build_cx.mcx())?;
    // The leader's centers (ivfbuild.c:680-681)
    let itemsize = bs.centers.itemsize;
    for i in 0..bs.centers.maxlen {
        let start = i as usize * itemsize;
        bs.centers.get_mut(i)?.copy_from_slice(&shared.centers[start..start + itemsize]);
    }
    bs.centers.length = bs.centers.maxlen;
    // This worker's own mailbox (IvfflatShared::rows)
    let rows = usize::try_from(parallel::ParallelWorkerNumber())
        .ok()
        .and_then(|n| shared.rows.get(n))
        .expect("ivfflat parallel build worker without its mailbox");

    let mut batch = AssignBatch::new();
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        &mut index_info,
        /* progress */ false,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            // Skip nulls
            if isnull[0] {
                return Ok(());
            }
            let r = (|| -> PgResult<()> {
                let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");
                if let Some((list, value)) = assign_tuple(&mut bs, tmp.mcx(), values)? {
                    batch.push(list, *tid, &value);
                    bs.indtuples += 1.0;
                    if batch.bytes() >= BATCH_BYTES {
                        send(rows, shared.leader, std::mem::replace(&mut batch, AssignBatch::new()))?;
                    }
                }
                Ok(())
            })();
            r.map_err(|e| {
                inner_err = Some(e);
                PgError::error("ivfflat build insert failed").into()
            })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };
    if batch.len() > 0 {
        send(rows, shared.leader, batch)?;
    }

    // Record statistics (ivfbuild.c:698-705)
    {
        let mut s = pgsync::lock(&shared.stats);
        s.nparticipantsdone += 1;
        s.reltuples += reltuples;
        s.indtuples += bs.indtuples;
    }
    pgvector_parallel::report_processed(false, reltuples, "ivfbuild.c", 709, 711, "IvfflatParallelScanAndSort")?;
    // Notify leader (ivfbuild.c:714)
    pgvector_parallel::wake_proc(shared.leader);
    Ok(())
}

/// Send a batch to the leader through this worker's mailbox, waiting
/// (interruptibly, on this worker's latch) while it is full.
fn send(rows: &MailboxSender<AssignBatch>, leader: usize, batch: AssignBatch) -> PgResult<()> {
    let wake = (pgvector_parallel::wake_proc as fn(usize), pgvector_parallel::my_proc_number());
    match rows.send_interruptible(batch, wake, || {
        parallel::wait_parallel_finish_quantum()?;
        postgres_seams::check_for_interrupts::call()
    })? {
        Ok(()) => {
            pgvector_parallel::wake_proc(leader);
            Ok(())
        }
        // The leader dropped its receiver: it is unwinding, and this worker
        // is about to be terminated.
        Err(_) => Err(PgError::error("ivfflat parallel build leader stopped receiving rows").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A batch keeps each value 8-byte aligned (the sort reads a varlena header
    // in place) and byte-identical, with its list and TID.
    #[test]
    fn assign_batch_keeps_values_aligned() {
        let mut b = AssignBatch::new();
        let v1: Vec<u8> = (0..20u8).collect();
        let v2: Vec<u8> = (100..109u8).collect();
        b.push(3, ItemPointerData::new(7, 2), &v1);
        b.push(5, ItemPointerData::new(8, 1), &v2);
        assert_eq!(b.len(), 2);
        assert_eq!(b.bytes(), 40);
        assert_eq!(b.lists, vec![3, 5]);
        assert_eq!(b.tids, vec![ItemPointerData::new(7, 2), ItemPointerData::new(8, 1)]);
        for (i, v) in [&v1, &v2].into_iter().enumerate() {
            let p = b.value_ptr(i);
            assert_eq!(p as usize % 8, 0);
            // SAFETY: the batch holds v.len() bytes at p.
            assert_eq!(unsafe { core::slice::from_raw_parts(p, v.len()) }, &v[..]);
        }
    }
}

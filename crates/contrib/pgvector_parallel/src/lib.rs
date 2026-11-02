//! pgvector's parallel index builds' shared plumbing: what hnswbuild.c and
//! ivfbuild.c's *BeginParallel, *EndParallel and *ParallelBuildMain have in
//! common, over pgrust's thread-native parallel crate.
//!
//! DIVERGENCE (ruling M6-1): C launches the workers with
//! CreateParallelContext("vector", "HnswParallelBuildMain" or
//! "IvfflatParallelBuildMain", n) and hands them a DSM segment through
//! shm_toc keys (hnswbuild.c:947-1037, ivfbuild.c:856-943). pgrust has no
//! dynamic loader: LookupParallelWorkerFunction accepts only "postgres"
//! (parallel/src/lib.rs:333-374), so the entry points register there under
//! pgrust names, and the shared state is an Arc set as the context's private
//! payload. Neither name is visible to SQL.
//! DIVERGENCE (spec §7.1): PGRUST_VECTOR_INDEXBUILD_POOL=0 turns both AMs'
//! parallel builds off; C has no such switch.

use std::any::Any;
use std::sync::Arc;

use mcx::Mcx;
use parallel::{ParallelContextId, ParallelWorkerEntry};
use tableam::ParallelTableScanDescShared;
use types_core::Oid;
use types_error::{ErrorLocation, PgResult, DEBUG1};
use types_rel::lock::{AccessExclusiveLock, RowExclusiveLock, ShareLock, ShareUpdateExclusiveLock, LOCKMODE};
use types_rel::Relation;

/// The kill switch (spec §7.1, named after B-tree's
/// PGRUST_RUNTIME_INDEXBUILD_POOL): PGRUST_VECTOR_INDEXBUILD_POOL=0 makes
/// every HNSW and IVFFlat build serial. Read at each build.
pub fn vector_parallel_builds_enabled() -> bool {
    kill_switch_allows(std::env::var("PGRUST_VECTOR_INDEXBUILD_POOL").ok().as_deref())
}

fn kill_switch_allows(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim() != "0")
}

/// plan_create_index_workers (planner.c:7114), through the planner's seam.
pub fn plan_create_index_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32> {
    planner_seams::plan_create_index_workers::call(heap.rd_id, index.rd_id)
}

/// The max_parallel_maintenance_workers GUC (HNSW's request, hnswbuild.c:1083-1085).
pub fn max_parallel_maintenance_workers() -> i32 {
    guc_tables::vars::max_parallel_maintenance_workers.read()
}

/// HnswLeader / IvfflatLeader (hnsw.h:276-283, ivfflat.h:178-186): what the
/// leader keeps to end the parallel phase.
#[must_use = "end_parallel must run"]
pub struct Leader {
    pub pcxt: ParallelContextId,
    pub nworkers_launched: i32,
    snapshot: Option<snapmgr::Snapshot>,
}

/// The steps HnswBeginParallel and IvfflatBeginParallel share
/// (hnswbuild.c:929-1054, ivfbuild.c:835-961): enter parallel mode, create the
/// context, take the scan's snapshot (an MVCC snapshot for CREATE INDEX
/// CONCURRENTLY, else SnapshotAny), initialize the parallel heap scan, build
/// the AM's shared state around it (`make_shared`), launch the workers, and
/// print C's "using %d parallel workers" at `using_at`. None when no worker
/// launched: the parallel phase is already ended, and the caller builds
/// serially, as C does (hnswbuild.c:1046-1051, ivfbuild.c:953-958).
pub fn begin_parallel<T: Any + Send + Sync>(
    entry_name: &'static str,
    entry: ParallelWorkerEntry,
    request: i32,
    heap: &Relation<'_>,
    isconcurrent: bool,
    make_shared: impl FnOnce(ParallelTableScanDescShared) -> PgResult<Arc<T>>,
    using_at: (&'static str, i32, &'static str),
) -> PgResult<Option<(Leader, Arc<T>)>> {
    debug_assert!(request > 0);
    // A worker looks its entry point up by name; registering twice is a no-op.
    parallel::register_parallel_worker_entrypoint(entry_name, entry);

    xact::EnterParallelMode();
    let pcxt = parallel::CreateParallelContext("postgres", entry_name, request)?;

    // Prepare for scan of the base relation. In a normal index build, we use
    // SnapshotAny because we must retrieve all tuples and do our own time qual
    // checks. In a concurrent build, we take a regular MVCC snapshot and index
    // whatever's live according to that.
    let snapshot = if isconcurrent {
        snapmgr::RegisterSnapshot(Some(&snapmgr::GetTransactionSnapshot()?))?
    } else {
        None
    };

    parallel::InitializeParallelDSM(pcxt)?;
    let mut pscan = ParallelTableScanDescShared::default();
    tableam::table_parallelscan_initialize(heap, &mut pscan, &snapshot)?;
    let shared = make_shared(pscan)?;
    parallel::set_private(pcxt, Arc::clone(&shared) as Arc<dyn Any + Send + Sync>);

    // Launch workers, saving status for leader/caller
    parallel::LaunchParallelWorkers(pcxt)?;
    let leader = Leader {
        pcxt,
        nworkers_launched: parallel::nworkers_launched(pcxt),
        snapshot,
    };

    // If no workers were successfully launched, back out (do serial build)
    if leader.nworkers_launched == 0 {
        end_parallel(leader)?;
        return Ok(None);
    }

    // Log participants
    let (file, line, func) = using_at;
    elog::ereport(DEBUG1)
        .errmsg(format!("using {} parallel workers", leader.nworkers_launched))
        .finish(ErrorLocation::new(file, line, func))?;
    Ok(Some((leader, shared)))
}

/// HnswEndParallel / IvfflatEndParallel (hnswbuild.c:891-902, ivfbuild.c:788-799).
pub fn end_parallel(leader: Leader) -> PgResult<()> {
    // Shutdown worker processes
    parallel::WaitForParallelWorkersToFinish(leader.pcxt)?;
    // Free last reference to MVCC snapshot, if one was used
    if leader.snapshot.is_some() {
        snapmgr::UnregisterSnapshot(leader.snapshot.as_ref());
    }
    parallel::DestroyParallelContext(leader.pcxt)?;
    xact::ExitParallelMode();
    Ok(())
}

/// The leader's statement, for the workers' debug_query_string
/// (PARALLEL_KEY_QUERY_TEXT, hnswbuild.c:975-982, ivfbuild.c:877-885).
pub fn leader_query_text() -> Option<String> {
    elog::with_debug_query_string(|q| q.map(str::to_owned))
}

/// Set debug_query_string for the worker and report the leader's statement
/// for monitoring, then run `f` (hnswbuild.c:853-857, ivfbuild.c:740-744).
pub fn with_worker_activity<R>(
    query_text: Option<&str>,
    f: impl FnOnce() -> PgResult<R>,
) -> PgResult<R> {
    let _scope = query_text.map(elog::debug_query_string_scope);
    backend_status::pgstat_report_activity(backend_status::BackendState::STATE_RUNNING, query_text);
    f()
}

/// A worker's heap and index, opened with the build's lock modes.
pub struct WorkerRelations<'mcx> {
    pub heap: Relation<'mcx>,
    pub index: Relation<'mcx>,
    heap_lock: LOCKMODE,
    index_lock: LOCKMODE,
}

/// Open relations using lock modes known to be obtained by index.c
/// (hnswbuild.c:862-876, ivfbuild.c:749-763). The worker is a member of its
/// leader's lock group, so these never wait for the leader's own locks.
pub fn worker_open<'mcx>(
    mcx: Mcx<'mcx>,
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
) -> PgResult<WorkerRelations<'mcx>> {
    let (heap_lock, index_lock) = if !isconcurrent {
        (ShareLock, AccessExclusiveLock)
    } else {
        (ShareUpdateExclusiveLock, RowExclusiveLock)
    };
    let heap = table::table_open(mcx, heaprelid, heap_lock)?;
    let index = indexam::index_open(mcx, indexrelid, index_lock)?;
    Ok(WorkerRelations { heap, index, heap_lock, index_lock })
}

/// hnswbuild.c:884-885, ivfbuild.c:781-782.
pub fn worker_close(rels: WorkerRelations<'_>) -> PgResult<()> {
    indexam::index_close(rels.index, rels.index_lock)?;
    table::table_close(rels.heap, rels.heap_lock)
}

/// Log statistics: "leader processed %lld tuples" / "worker processed %lld
/// tuples", the heap tuples this participant scanned (hnswbuild.c:826-830,
/// ivfbuild.c:707-711). A worker's line reaches the client through its
/// leader, with "CONTEXT:  parallel worker".
pub fn report_processed(
    leader: bool,
    reltuples: f64,
    file: &'static str,
    leader_line: i32,
    worker_line: i32,
    func: &'static str,
) -> PgResult<()> {
    let (who, line) = if leader { ("leader", leader_line) } else { ("worker", worker_line) };
    elog::ereport(DEBUG1)
        .errmsg(format!("{who} processed {} tuples", reltuples as i64))
        .finish(ErrorLocation::new(file, line, func))
}

/// Wake a backend sleeping on its latch, the role ConditionVariableSignal
/// plays for C's workersdonecv: the IVFFlat leader waiting for rows, or a
/// worker waiting for room in the leader's mailbox. `fn(usize)` so a mailbox
/// can call it.
pub fn wake_proc(procno: usize) {
    latch::set_latch(&lmgr_proc::GetPGProcByNumber(procno as types_core::ProcNumber).procLatch);
}

/// This backend's ProcNumber, as `wake_proc`'s argument.
pub fn my_proc_number() -> usize {
    init_small::globals::MyProcNumber() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    // PGRUST_VECTOR_INDEXBUILD_POOL=0 (whitespace allowed) is the only value
    // that turns parallel vector builds off.
    #[test]
    fn kill_switch_reads_only_zero_as_off() {
        assert!(kill_switch_allows(None));
        assert!(kill_switch_allows(Some("1")));
        assert!(kill_switch_allows(Some("")));
        assert!(kill_switch_allows(Some("off")));
        assert!(!kill_switch_allows(Some("0")));
        assert!(!kill_switch_allows(Some(" 0 ")));
    }
}

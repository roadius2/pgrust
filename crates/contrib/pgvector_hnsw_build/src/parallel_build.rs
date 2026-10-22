//! hnswbuild.c's parallel build: HnswBeginParallel, HnswParallelBuildMain,
//! HnswParallelScanAndInsert, ParallelHeapScan and HnswEndParallel
//! (hnswbuild.c:762-1065), on pgvector_parallel's launched workers. Every
//! participant, the leader included, scans its share of the heap and inserts
//! into one shared pgvector_hnsw_graph::Graph under C's parallel accounting
//! (maintenance_work_mem - 3 MB, with a 1 MB margin). Whichever participant
//! overflows the graph prints the NOTICE and flushes it; from then on every
//! participant inserts on disk (pgvector_hnsw's on-disk path, which concurrent
//! backends already share; HNSW_UPDATE_LOCK does not exclude members of one
//! lock group, in C either). The leader flushes the graph if nobody did, and
//! WAL-logs the index once every participant has finished
//! (hnswbuild.c:1119-1143).

use std::sync::Arc;

use mcx::Mcx;
use pgsync::Mutex;
use pgvector_hnsw_graph::{Accounting, Graph};
use tableam::ParallelTableScanDescShared;
use types_core::{ForkNumber, Oid};
use types_error::{PgError, PgResult};
use types_rel::Relation;

use crate::{build_callback, flush_pages, init_build_state, BuildState};

const ENTRY_NAME: &str = "pgrust_hnsw_parallel_build_main";

/// HnswShared (hnsw.h:254-274): graphData, the parallel heap scan and the
/// participants' statistics.
struct HnswShared {
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
    pscan: ParallelTableScanDescShared,
    graph: Graph,
    query_text: Option<String>,
    stats: Mutex<HnswStats>,
}

#[derive(Default)]
struct HnswStats {
    nparticipantsdone: i32,
    reltuples: f64,
}

/// ComputeParallelWorkers (hnswbuild.c:1070-1086), behind the kill switch.
pub(crate) fn compute_parallel_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32> {
    if !pgvector_parallel::vector_parallel_builds_enabled() {
        return Ok(0);
    }
    // Make sure it's safe to use parallel workers
    if pgvector_parallel::plan_create_index_workers(heap, index)? == 0 {
        return Ok(0);
    }
    let max_parallel_maintenance_workers = pgvector_parallel::max_parallel_maintenance_workers();
    // Use parallel_workers storage parameter on table if set
    let parallel_workers = heap.get_parallel_workers(-1);
    if parallel_workers != -1 {
        return Ok(parallel_workers.min(max_parallel_maintenance_workers));
    }
    Ok(max_parallel_maintenance_workers)
}

/// HnswBeginParallel, HnswLeaderParticipateAsWorker, ParallelHeapScan, the
/// leader's FlushPages and HnswEndParallel (hnswbuild.c:762-790, 891-1065,
/// 1107-1124). Returns (reltuples, indtuples), or None when no worker
/// launched: the caller then builds serially (hnswbuild.c:1046-1051).
pub(crate) fn build_graph_parallel<'mcx>(
    bs: &BuildState<'_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    isconcurrent: bool,
    request: i32,
) -> PgResult<Option<(f64, f64)>> {
    // Leave space for other objects in shared memory (hnswbuild.c:961-969)
    let area = Accounting::parallel_area_size(init_small::globals::maintenance_work_mem());
    let (heaprelid, indexrelid) = (heap.rd_id, bs.index.rd_id);
    let Some((leader, shared)) = pgvector_parallel::begin_parallel(
        ENTRY_NAME,
        hnsw_parallel_build_main,
        request,
        heap,
        isconcurrent,
        move |pscan| {
            Ok(Arc::new(HnswShared {
                heaprelid,
                indexrelid,
                isconcurrent,
                pscan,
                graph: Graph::new(Accounting::parallel(area)),
                query_text: pgvector_parallel::leader_query_text(),
                stats: Mutex::new(HnswStats::default()),
            }))
        },
        ("hnswbuild.c", 1054, "HnswBeginParallel"),
    )?
    else {
        return Ok(None);
    };

    // HnswLeaderParticipateAsWorker (hnswbuild.c:917-923)
    scan_and_insert(mcx, heap, bs.index, &shared, true)?;

    // hnswbuild.c:1064
    parallel::WaitForParallelWorkersToAttach(leader.pcxt)?;

    // ParallelHeapScan (hnswbuild.c:762-790): C sleeps on workersdonecv until
    // every participant is done; the workers' exit is the same point here.
    parallel::WaitForParallelWorkersToFinish(leader.pcxt)?;
    let (participants, reltuples) = {
        let stats = pgsync::lock(&shared.stats);
        (stats.nparticipantsdone, stats.reltuples)
    };
    debug_assert_eq!(participants, leader.nworkers_launched + 1);
    let indtuples = shared.graph.indtuples();

    // Flush pages (hnswbuild.c:1119-1120), the leader's state on the shared graph
    shared.graph.flush_if_needed(|graph| flush_pages(bs, graph))?;

    pgvector_parallel::end_parallel(leader)?;
    Ok(Some((reltuples, indtuples)))
}

/// HnswParallelScanAndInsert (hnswbuild.c:795-836).
fn scan_and_insert<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    shared: &HnswShared,
    progress: bool,
) -> PgResult<()> {
    // Join parallel scan
    let mut index_info = execindexing::BuildIndexInfo(mcx, index)?;
    index_info.ii_Concurrent = shared.isconcurrent;
    let mut bs = init_build_state(Some(heap), index, ForkNumber::MAIN_FORKNUM, &shared.graph)?;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        &mut index_info,
        progress,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            build_callback(&mut bs, values, isnull, tid).map_err(|e| {
                inner_err = Some(e);
                PgError::error("hnsw build insert failed").into()
            })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };

    // Record statistics
    {
        let mut stats = pgsync::lock(&shared.stats);
        stats.nparticipantsdone += 1;
        stats.reltuples += reltuples;
    }

    // Log statistics
    pgvector_parallel::report_processed(progress, reltuples, "hnswbuild.c", 828, 830, "HnswParallelScanAndInsert")
}

/// HnswParallelBuildMain (hnswbuild.c:841-886).
fn hnsw_parallel_build_main(pshared: &parallel::ParallelShared) -> PgResult<()> {
    let shared = pshared
        .private()
        .expect("hnsw parallel build worker without its shared state")
        .downcast::<HnswShared>()
        .unwrap_or_else(|_| panic!("hnsw parallel build private state is not HnswShared"));
    pgvector_parallel::with_worker_activity(shared.query_text.as_deref(), || {
        let cx = mcx::MemoryContext::new("Hnsw parallel build worker");
        let mcx = cx.mcx();
        let rels = pgvector_parallel::worker_open(mcx, shared.heaprelid, shared.indexrelid, shared.isconcurrent)?;
        // Perform inserts
        scan_and_insert(mcx, &rels.heap, &rels.index, &shared, false)?;
        // Close relations within worker
        pgvector_parallel::worker_close(rels)
    })
}

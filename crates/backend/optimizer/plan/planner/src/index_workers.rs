//! plan_create_index_workers (planner.c:7097-7239): how many workers a CREATE
//! INDEX may use. index_build asks it for an AM that can build in parallel
//! (index.c:3056-3060); pgvector's builds ask it themselves
//! (hnswbuild.c:1076, ivfbuild.c:987).
//!
//! C builds a dummy Query and PlannerInfo and calls build_simple_rel only to
//! get a base RelOptInfo carrying the table's parallel_workers reloption
//! (planner.c:7135-7172, plancat.c:208). Here that RelOptInfo is built
//! directly; nothing else of the planner runs.

use mcx::Mcx;
use types_core::{Oid, RELPERSISTENCE_TEMP};
use types_error::PgResult;
use types_nodes::NodeList;
use types_pathnodes::RelOptInfo;
use types_rel::lock::NoLock;
use types_rel::Relation;

/// The number of parallel worker processes (the leader not counted) to
/// request for a CREATE INDEX of `index_oid` on `table_oid`. The caller holds
/// locks on both (planner.c:7110-7111).
pub fn plan_create_index_workers(table_oid: Oid, index_oid: Oid) -> PgResult<i32> {
    // We don't allow performing parallel operation in standalone backend or
    // when parallelism is disabled (planner.c:7128-7133).
    if !init_small::globals::IsUnderPostmaster() {
        return Ok(0);
    }
    let max_parallel_maintenance_workers =
        guc_tables::vars::max_parallel_maintenance_workers.read();
    if max_parallel_maintenance_workers == 0 {
        return Ok(0);
    }

    let cx = mcx::MemoryContext::new("plan_create_index_workers");
    let mcx = cx.mcx();
    // Rels are assumed already locked by the caller (planner.c:7174-7176).
    let heap = table::table_open(mcx, table_oid, NoLock)?;
    let index = indexam::index_open(mcx, index_oid, NoLock)?;
    let parallel_workers = index_workers(mcx, &heap, &index, max_parallel_maintenance_workers);
    indexam::index_close(index, NoLock)?;
    table::table_close(heap, NoLock)?;
    parallel_workers
}

fn index_workers(
    mcx: Mcx<'_>,
    heap: &Relation<'_>,
    index: &Relation<'_>,
    max_parallel_maintenance_workers: i32,
) -> PgResult<i32> {
    // Determine if it's safe to proceed: parallel workers can't access the
    // leader's temporary tables, and the index expressions and predicate must
    // be parallel safe (planner.c:7178-7191).
    if heap.rd_rel.relpersistence == RELPERSISTENCE_TEMP
        || !is_parallel_safe_list(execindexing::RelationGetIndexExpressions(mcx, index)?)?
        || !is_parallel_safe_list(execindexing::RelationGetIndexPredicate(mcx, index)?)?
    {
        return Ok(0);
    }

    // build_simple_rel's base rel, with the reloption get_relation_info loads
    // (planner.c:7168-7172, plancat.c:208).
    let mut rel = RelOptInfo::new(mcx);
    rel.rel_parallel_workers = heap.get_parallel_workers(-1);

    // If parallel_workers storage parameter is set for the table, accept that
    // as the number of parallel worker processes to launch (though still cap
    // at max_parallel_maintenance_workers). We deliberately do not consider
    // any other factor when parallel_workers is set (planner.c:7193-7205).
    if rel.rel_parallel_workers != -1 {
        return Ok(rel.rel_parallel_workers.min(max_parallel_maintenance_workers));
    }

    // Estimate heap relation size ourselves, since rel->pages cannot be
    // trusted (planner.c:7207-7211).
    let (heap_blocks, _reltuples, _allvisfrac) = crate::plancat::estimate_rel_size(heap, None, 0)?;

    // Determine number of workers to scan the heap relation using generic
    // model (planner.c:7213-7218).
    let parallel_workers = ::allpaths::compute_parallel_worker(
        &rel,
        heap_blocks as f64,
        -1.0,
        max_parallel_maintenance_workers,
    );
    Ok(clamp_for_maintenance_work_mem(
        parallel_workers,
        init_small::globals::maintenance_work_mem(),
    ))
}

/// Cap workers based on available maintenance_work_mem as needed: each
/// tuplesort participant, the leader included, gets an even share, and 32 MB
/// apiece is the floor (planner.c:7220-7232). `maintenance_work_mem` is in kB.
fn clamp_for_maintenance_work_mem(mut parallel_workers: i32, maintenance_work_mem: i32) -> i32 {
    while parallel_workers > 0 && maintenance_work_mem / (parallel_workers + 1) < 32 * 1024 {
        parallel_workers -= 1;
    }
    parallel_workers
}

/// is_parallel_safe(root, (Node *) list) (clauses.c:767) under C's zeroed
/// PlannerGlobal: maxParallelHazard is 0, not PROPARALLEL_SAFE, so the walker
/// always runs, and there are no initplan params.
fn is_parallel_safe_list(list: NodeList<'_>) -> PgResult<bool> {
    for node in list.iter() {
        if !clauses::is_parallel_safe(0, true, Vec::new(), node)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    // planner.c:7128-7133: a standalone backend (unit tests run outside a
    // postmaster) gets no workers, before any catalog or GUC access.
    #[test]
    fn index_workers_are_zero_outside_a_postmaster() {
        assert!(!init_small::globals::IsUnderPostmaster());
        assert_eq!(plan_create_index_workers(1, 2).unwrap(), 0);
    }

    // planner.c:7220-7232: each participant (the leader counts) needs 32 MB of
    // maintenance_work_mem (kB here).
    #[test]
    fn memory_clamp_matches_planner_c() {
        assert_eq!(clamp_for_maintenance_work_mem(2, 65536), 1);
        assert_eq!(clamp_for_maintenance_work_mem(2, 98304), 2);
        assert_eq!(clamp_for_maintenance_work_mem(1, 65535), 0);
        assert_eq!(clamp_for_maintenance_work_mem(1, 65536), 1);
        assert_eq!(clamp_for_maintenance_work_mem(4, 1048576), 4);
        assert_eq!(clamp_for_maintenance_work_mem(0, 64), 0);
    }
}

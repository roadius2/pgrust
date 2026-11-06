//! pgvector 0.8.7 hnswbuild.c. The in-memory graph phase runs on
//! pgvector_hnsw_graph (InsertTuple and InsertTupleInMemory, with C's lock
//! protocol and C's memory accounting); the graph is flushed to disk when it
//! no longer fits maintenance_work_mem, and later tuples are inserted on
//! disk. parallel_build.rs runs the same insert path in launched workers.

use bufmgr::{LockBuffer, MarkBufferDirty, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_UNLOCK};
use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgvector_hnsw::insert::{form_index_value, insert_tuple_on_disk, random_level, seed_random};
use pgvector_hnsw::layout::*;
use pgvector_hnsw::utils::*;
use pgvector_hnsw_graph::{Accounting, Element, Graph, GraphError, InsertError, InsertParams, Inserted};
use types_core::{BlockNumber, Buffer, ForkNumber};
use types_error::{PgError, PgResult, ERRCODE_FEATURE_NOT_SUPPORTED, ERRCODE_INVALID_PARAMETER_VALUE, ERRCODE_PROGRAM_LIMIT_EXCEEDED, NOTICE};
use types_hnsw::*;
use types_rel::Relation;
use types_storage::bufpage::PageMut;
use types_tuple::itemptr::ItemPointerData;

mod parallel_build;

pub struct IndexBuildResult {
    pub heap_tuples: f64,
    pub index_tuples: f64,
}

// HnswBuildState (hnsw.h:311-347): one participant's state. `graph` is the
// build's own (serial) or the shared one (parallel_build.rs's participants).
// The parallel leader's outer state keeps `graph` pointing at its empty local
// graph while it flushes the shared one (C reassigns buildstate->graph,
// hnswbuild.c:775), so flush_pages and its helpers use their `graph`
// argument, never `bs.graph`.
struct BuildState<'a, 'mcx> {
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    fork_num: ForkNumber,
    m: i32,
    ef_construction: i32,
    dimensions: i32,
    type_info: &'static HnswTypeInfo,
    ml: f64,
    max_level: i32,
    support: HnswSupport,
    graph: &'a Graph,
    reltuples: f64,
}

// An element's on-disk location, assigned by CreateGraphPages.
#[derive(Clone, Copy)]
struct Placement {
    blkno: BlockNumber,
    offno: u16,
    neighbor_page: BlockNumber,
    neighbor_offno: u16,
}

const UNPLACED: Placement = Placement {
    blkno: INVALID_BLOCK,
    offno: INVALID_OFFSET,
    neighbor_page: INVALID_BLOCK,
    neighbor_offno: INVALID_OFFSET,
};

// ---- flush to disk ----

fn create_meta_page(bs: &BuildState<'_, '_>) -> PgResult<()> {
    let buf = new_buffer(bs.index, bs.fork_num)?;
    init_page(buf);
    let meta = MetaPage {
        magic_number: HNSW_MAGIC_NUMBER,
        version: HNSW_VERSION,
        dimensions: bs.dimensions as u32,
        m: bs.m as u16,
        ef_construction: bs.ef_construction as u16,
        entry_blkno: INVALID_BLOCK,
        entry_offno: INVALID_OFFSET,
        entry_level: -1,
        insert_page: INVALID_BLOCK,
    };
    {
        let page = buf_page_bytes_mut(buf);
        meta.write(&mut page[PAGE_CONTENTS_OFF..PAGE_CONTENTS_OFF + METAPAGE_SIZE]);
        // pd_lower covers the metapage contents.
        let mut pm = buf_page_mut(buf);
        pm.set_pd_lower((PAGE_CONTENTS_OFF + METAPAGE_SIZE) as u16);
    }
    MarkBufferDirty(buf)?;
    UnlockReleaseBuffer(buf)?;
    Ok(())
}

fn build_append_page(
    index: &Relation<'_>,
    buf: &mut Buffer,
    fork_num: ForkNumber,
) -> PgResult<()> {
    let newbuf = new_buffer(index, fork_num)?;
    page_opaque_set_nextblkno(buf_page_bytes_mut(*buf), bufmgr::BufferGetBlockNumber(newbuf));
    MarkBufferDirty(*buf)?;
    UnlockReleaseBuffer(*buf)?;
    LockBuffer(newbuf, BUFFER_LOCK_UNLOCK)?;
    postgres_seams::check_for_interrupts::call()?;
    LockBuffer(newbuf, BUFFER_LOCK_EXCLUSIVE)?;
    *buf = newbuf;
    init_page(*buf);
    Ok(())
}

fn page_add(
    index: &Relation<'_>,
    buf: Buffer,
    item: &[u8],
    expected: u16,
) -> PgResult<()> {
    // SAFETY: exclusive lock held on buf.
    let mut pm = unsafe {
        PageMut::from_raw(bufmgr_seams::buffer_get_page::call(buf))
    };
    if pm.add_item(item, 0, 0) != Some(expected) {
        return Err(PgError::error(format!(
            "failed to add index item to \"{}\"",
            index.name()
        ))
        .into());
    }
    Ok(())
}

fn serialize_element_tuple(buf: &mut Vec<u8>, e: &Element, p: Placement) -> PgResult<()> {
    let value = e.value();
    let links = e.links();
    let size = element_tuple_size(value.len());
    buf.clear();
    ::mcx::std_vec_resize("pgvector_hnsw_build", buf, size, 0)?;
    buf[0] = HNSW_ELEMENT_TUPLE_TYPE;
    buf[1] = e.level;
    buf[2] = 0;
    // Start at one to make it easier to find issues (HnswInitElement, hnswutils.c:262)
    buf[3] = 1;
    for i in 0..HNSW_HEAPTIDS {
        let b = if i < links.heaptids_len as usize {
            links.heaptids[i]
        } else {
            itemptr_encode(INVALID_BLOCK, INVALID_OFFSET)
        };
        buf[4 + i * 6..4 + i * 6 + 6].copy_from_slice(&b);
    }
    buf[4 + HNSW_HEAPTIDS * 6..4 + HNSW_HEAPTIDS * 6 + 6]
        .copy_from_slice(&itemptr_encode(p.neighbor_page, p.neighbor_offno));
    buf[ELEMENT_DATA_OFFSET..ELEMENT_DATA_OFFSET + value.len()].copy_from_slice(value);
    Ok(())
}

fn serialize_neighbor_tuple(buf: &mut Vec<u8>, e: &Element, placements: &[Placement], m: i32) -> PgResult<()> {
    let size = neighbor_tuple_size(e.level, m);
    buf.clear();
    ::mcx::std_vec_resize("pgvector_hnsw_build", buf, size, 0)?;
    buf[0] = HNSW_NEIGHBOR_TUPLE_TYPE;
    // The element's version
    buf[1] = 1;
    let links = e.links();
    let mut idx = 0usize;
    for lc in (0..=e.level as i32).rev() {
        let na = &links.neighbors[(e.level as i32 - lc) as usize];
        let lm = hnsw_get_layer_m(m, lc);
        for i in 0..lm as usize {
            let b = if i < na.items.len() {
                let p = placements[na.items[i].element as usize];
                itemptr_encode(p.blkno, p.offno)
            } else {
                itemptr_encode(INVALID_BLOCK, INVALID_OFFSET)
            };
            buf[NEIGHBOR_TIDS_OFFSET + idx * 6..NEIGHBOR_TIDS_OFFSET + idx * 6 + 6].copy_from_slice(&b);
            idx += 1;
        }
    }
    buf[2..4].copy_from_slice(&(idx as u16).to_ne_bytes());
    Ok(())
}

// CreateGraphPages (hnswbuild.c:150-248): graph->head, newest first
// (duplicates merged away are not on it).
fn create_graph_pages(bs: &BuildState<'_, '_>, graph: &Graph) -> PgResult<Vec<Placement>> {
    let max_size = HNSW_MAX_SIZE;
    let mut placements = ::mcx::std_vec_from_elem("pgvector_hnsw_build", UNPLACED, graph.element_count() as usize)?;
    let mut etup: Vec<u8> = Vec::new();
    let mut ntup_placeholder: Vec<u8> = Vec::new();

    let mut buf = new_buffer(bs.index, bs.fork_num)?;
    init_page(buf);

    for id in graph.head_newest_first() {
        let e = graph.element(id);
        let etup_size = element_tuple_size(e.value().len());
        let ntup_size = neighbor_tuple_size(e.level, bs.m);
        let combined_size = etup_size + ntup_size + SIZE_OF_ITEM_ID;
        if etup_size > HNSW_TUPLE_ALLOC_SIZE {
            return Err(PgError::error("index tuple too large")
                .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
                .into());
        }

        // SAFETY: exclusive lock held on buf.
        let free = unsafe {
            types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
        }
        .free_space();
        if free < etup_size || (combined_size <= max_size && free < combined_size) {
            build_append_page(bs.index, &mut buf, bs.fork_num)?;
        }

        let placement = {
            // SAFETY: lock held.
            let pr = unsafe {
                types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
            };
            let blkno = bufmgr::BufferGetBlockNumber(buf);
            let offno = pr.max_offset_number() + 1;
            if combined_size <= max_size {
                Placement { blkno, offno, neighbor_page: blkno, neighbor_offno: offno + 1 }
            } else {
                Placement { blkno, offno, neighbor_page: blkno + 1, neighbor_offno: 1 }
            }
        };
        placements[id as usize] = placement;

        serialize_element_tuple(&mut etup, e, placement)?;
        page_add(bs.index, buf, &etup, placement.offno)?;

        // SAFETY: lock held.
        let free = unsafe {
            types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
        }
        .free_space();
        if free < ntup_size {
            build_append_page(bs.index, &mut buf, bs.fork_num)?;
        }
        ntup_placeholder.clear();
        ::mcx::std_vec_resize("pgvector_hnsw_build", &mut ntup_placeholder, ntup_size, 0)?;
        ntup_placeholder[0] = HNSW_NEIGHBOR_TUPLE_TYPE;
        page_add(bs.index, buf, &ntup_placeholder, placement.neighbor_offno)?;
    }

    let insert_page = bufmgr::BufferGetBlockNumber(buf);
    MarkBufferDirty(buf)?;
    UnlockReleaseBuffer(buf)?;

    let entry = graph.entry_point().map(|ep| {
        let p = placements[ep as usize];
        (p.blkno, p.offno, graph.element(ep).level as i16)
    });
    update_meta_page(bs.index, HNSW_UPDATE_ENTRY_ALWAYS, entry, insert_page, bs.fork_num, true)?;
    Ok(placements)
}

// WriteNeighborTuples (hnswbuild.c:253-298).
fn write_neighbor_tuples(bs: &BuildState<'_, '_>, graph: &Graph, placements: &[Placement]) -> PgResult<()> {
    let mut ntup: Vec<u8> = Vec::new();
    // Same head-list walk as create_graph_pages: duplicates have no tuples.
    for id in graph.head_newest_first() {
        postgres_seams::check_for_interrupts::call()?;
        let e = graph.element(id);
        let p = placements[id as usize];
        let ntup_size = neighbor_tuple_size(e.level, bs.m);
        serialize_neighbor_tuple(&mut ntup, e, placements, bs.m)?;
        let buf = bufmgr::ReadBufferExtended(
            bs.index,
            bs.fork_num,
            p.neighbor_page,
            types_storage::storage::ReadBufferMode::Normal,
            None,
        )?;
        LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
        // SAFETY: exclusive lock held.
        let mut pm = unsafe { PageMut::from_raw(bufmgr_seams::buffer_get_page::call(buf)) };
        if !pm.index_tuple_overwrite(p.neighbor_offno, &ntup[..ntup_size]) {
            return Err(PgError::error(format!("failed to add index item to \"{}\"", bs.index.name())).into());
        }
        MarkBufferDirty(buf)?;
        UnlockReleaseBuffer(buf)?;
    }
    Ok(())
}

// FlushPages (hnswbuild.c:303-316), called with the graph's flush lock held
// exclusively; the graph sets `flushed`.
// DIVERGENCE: C resets graphCtx here (hnswbuild.c:315); the arena lives until
// the build ends (pgvector_hnsw_graph's DIVERGENCE note). And pgsync's locks
// do not hold off interrupts, so a cancel can raise mid-flush under
// flush_lock where C's LWLockAcquire would hold it off: `flushed` stays
// false, the next participant retries the NOTICE and the flush before the
// abort reaches it, and the transaction aborts either way.
fn flush_pages(bs: &BuildState<'_, '_>, graph: &Graph) -> PgResult<()> {
    create_meta_page(bs)?;
    let placements = create_graph_pages(bs, graph)?;
    write_neighbor_tuples(bs, graph, &placements)
}

// HnswSharedMemoryAlloc's errors (hnswbuild.c:671-676).
fn graph_error(e: GraphError) -> Box<PgError> {
    let (message, line) = match e {
        GraphError::AllocationTooLarge => ("hnsw allocation too large", 672),
        GraphError::OutOfMemory => ("hnsw allocator out of memory", 676),
        // palloc's ERROR in the context C allocates from (mcxt.c:1163-1167).
        GraphError::HostOutOfMemory { context, bytes } => return mcx::oom_named(context, bytes).into(),
    };
    PgError::error(message).with_location("hnswbuild.c", line, "HnswSharedMemoryAlloc").into()
}

// InsertTuple (hnswbuild.c:485-580): form and check the value, then the
// graph's in-memory insert, or the on-disk insert once the graph is flushed.
fn insert_tuple(
    bs: &mut BuildState<'_, '_>,
    values: &[Datum],
    isnull: &[bool],
    heaptid: &ItemPointerData,
) -> PgResult<bool> {
    if isnull.first().copied().unwrap_or(true) {
        return Ok(false);
    }
    let tmp = mcx::MemoryContext::new_wholesale("Hnsw build temporary context");
    let mut support = bs.support.clone();

    // Form index value
    let Some(img) = form_index_value(tmp.mcx(), values[0], bs.type_info, &mut support)? else {
        bs.support = support;
        return Ok(false);
    };

    // Check dimensions match index
    hnsw_check_dim(bs.dimensions, bs.type_info, support.collation, tmp.mcx(), Datum::from_usize(img.as_ptr() as usize))?;

    let state: &BuildState<'_, '_> = bs;
    let params = InsertParams { m: state.m, ef_construction: state.ef_construction };
    let inserted = state.graph.insert(
        &img,
        ipd_to_bytes(heaptid),
        &params,
        || random_level(state.ml, state.max_level),
        &mut |a: &[u8], b: &[u8]| {
            get_distance(&mut support, Datum::from_usize(a.as_ptr() as usize), Datum::from_usize(b.as_ptr() as usize))
        },
        |graph| {
            elog::ereport(NOTICE)
                .errmsg(format!(
                    "hnsw graph no longer fits into maintenance_work_mem after {} tuples",
                    graph.indtuples() as i64
                ))
                .errdetail("Building will take significantly more time.".to_string())
                .errhint("Increase maintenance_work_mem to speed up builds.".to_string())
                .finish(types_error::ErrorLocation::new("hnswbuild.c", 545, "InsertTuple"))?;
            flush_pages(state, graph)
        },
    );
    let r = match inserted {
        Ok(Inserted::InMemory) => Ok(true),
        Ok(Inserted::OnDisk) => {
            insert_tuple_on_disk(state.index, state.type_info, &mut support, &img, heaptid, true).map(|_| true)
        }
        Err(InsertError::Callback(e)) => Err(e),
        Err(InsertError::Graph(e)) => Err(graph_error(e)),
    };
    bs.support = support;
    r
}

// BuildCallback (hnswbuild.c:585-612).
fn build_callback(
    bs: &mut BuildState<'_, '_>,
    values: &[Datum],
    isnull: &[bool],
    tid: &ItemPointerData,
) -> PgResult<()> {
    // Insert tuple (nulls are skipped there)
    if insert_tuple(bs, values, isnull, tid)? {
        // Update progress
        let indtuples = bs.graph.add_indtuple();
        backend_progress::pgstat_progress_update_param(
            backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_DONE,
            indtuples as i64,
        );
    }
    Ok(())
}

fn init_build_state<'a, 'mcx>(
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    fork_num: ForkNumber,
    graph: &'a Graph,
) -> PgResult<BuildState<'a, 'mcx>> {
    // InitBuildState (hnswbuild.c:686-745).
    let type_info = hnsw_get_type_info(index)?;
    let m = hnsw_get_m(index);
    let ef_construction = hnsw_get_ef_construction(index);
    let dimensions = index.rd_att.attr(0).atttypmod;

    // Disallow varbit since require fixed dimensions
    if index.rd_att.attr(0).atttypid == types_core::VARBITOID {
        return Err(PgError::error("type not supported for hnsw index")
            .with_sqlstate(ERRCODE_FEATURE_NOT_SUPPORTED)
            .with_location("hnswbuild.c", 703, "InitBuildState")
            .into());
    }
    // Require column to have dimensions to be indexed
    if dimensions < 0 {
        return Err(PgError::error("column does not have dimensions")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .with_location("hnswbuild.c", 709, "InitBuildState")
            .into());
    }
    if dimensions > type_info.max_dimensions {
        return Err(PgError::error(format!(
            "column cannot have more than {} dimensions for hnsw index",
            type_info.max_dimensions
        ))
        .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
        .with_location("hnswbuild.c", 714, "InitBuildState")
        .into());
    }
    if ef_construction < 2 * m {
        return Err(
            PgError::error("ef_construction must be greater than or equal to 2 * m")
                .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
                .with_location("hnswbuild.c", 719, "InitBuildState")
                .into(),
        );
    }

    let support = init_support(index)?;
    Ok(BuildState {
        heap,
        index,
        fork_num,
        m,
        ef_construction,
        dimensions,
        type_info,
        ml: hnsw_get_ml(m),
        max_level: hnsw_get_max_level(m),
        support,
        graph,
        reltuples: 0.0,
    })
}

// pgrust.hnsw_build_seed: a placeholder customized option, deliberately NOT a
// registered GUC (a pg_settings row would break byte-identical regression
// outputs); read the way guc_tables/src/gather_fair.rs reads its option. Set to
// N, a build calls SeedRandom(N) where BuildIndex calls SeedRandom(42) under
// #ifdef HNSW_MEMORY (hnswbuild.c:1134-1136). Unset (the default) is stock C,
// which never seeds.
// DIVERGENCE (opt-in only): stock C has no such switch; the byte-identical and
// iterative-scan tiers set 42 to match the -DHNSW_MEMORY reference build.
fn hnsw_build_seed() -> Option<u64> {
    // Uninstalled seam (unit-test binaries without a guc boot): stock C.
    if !guc_seams::get_config_option_missing_ok::is_installed() {
        return None;
    }
    parse_build_seed(
        guc_seams::get_config_option_missing_ok::call("pgrust.hnsw_build_seed")
            .ok()
            .flatten(),
    )
}

fn parse_build_seed(value: Option<String>) -> Option<u64> {
    value.and_then(|v| v.trim().parse::<u64>().ok())
}

// The serial scan: table_index_build_scan with BuildCallback
// (hnswbuild.c:1111-1113).
fn serial_scan<'mcx>(
    bs: &mut BuildState<'_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<f64> {
    let index = bs.index;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan(
        mcx,
        heap,
        index,
        index_info,
        true,
        /* progress */ true,
        |_index_rel, tid, values, isnull, _alive| {
            build_callback(bs, values, isnull, tid).map_err(|e| {
                inner_err = Some(e);
                PgError::error("hnsw build insert failed").into()
            })
        },
    );
    match reltuples {
        Ok(n) => Ok(n),
        Err(e) => Err(inner_err.unwrap_or(e)),
    }
}

// BuildIndex (hnswbuild.c:1131-1146) and BuildGraph (hnswbuild.c:1092-1125).
fn build_index<'mcx>(
    mcx: Mcx<'mcx>,
    heap: Option<&Relation<'mcx>>,
    index: &Relation<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<IndexBuildResult> {
    // SeedRandom before InitBuildState (hnswbuild.c:1134-1136).
    if let Some(seed) = hnsw_build_seed() {
        seed_random(seed);
    }
    // InitBuildState's graph: memoryTotal = maintenance_work_mem * 1024
    // (hnswbuild.c:727).
    let graph = Graph::new(Accounting::serial(
        (init_small::globals::maintenance_work_mem().max(0) as usize).saturating_mul(1024),
    ));
    let mut bs = init_build_state(heap, index, fork_num, &graph)?;

    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_HNSW_PHASE_LOAD,
    );

    // Add tuples to graph: calculate parallel workers, attempt to launch a
    // parallel build when required, else scan serially (hnswbuild.c:1099-1115).
    let mut indtuples = 0.0;
    let mut built_in_parallel = false;
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        let parallel_workers = parallel_build::compute_parallel_workers(heap, index)?;
        let built = if parallel_workers > 0 {
            parallel_build::build_graph_parallel(&bs, mcx, heap, index_info.ii_Concurrent, parallel_workers)?
        } else {
            None
        };
        match built {
            Some((reltuples, n)) => {
                bs.reltuples = reltuples;
                indtuples = n;
                built_in_parallel = true;
            }
            None => {
                bs.reltuples = serial_scan(&mut bs, mcx, heap, index_info)?;
                indtuples = graph.indtuples();
            }
        }
    }

    // Flush pages (a parallel build flushed its shared graph already)
    if !built_in_parallel {
        graph.flush_if_needed(|g| flush_pages(&bs, g))?;
    }

    // Write WAL for initialization fork since GenericXLog functions do not
    if relation_needs_wal(index) || fork_num == ForkNumber::INIT_FORKNUM {
        let nblocks = bufmgr::RelationGetNumberOfBlocksInFork(index, fork_num)?;
        xloginsert::log_newpage_range(index, fork_num, 0, nblocks, true)?;
    }

    Ok(IndexBuildResult { heap_tuples: bs.reltuples, index_tuples: indtuples })
}

pub fn hnswbuild<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<IndexBuildResult> {
    build_index(mcx, Some(heap), index, Some(index_info), ForkNumber::MAIN_FORKNUM)
}

pub fn hnswbuildempty(index: &Relation<'_>) -> PgResult<()> {
    let mcx_owner = mcx::MemoryContext::new_wholesale("hnsw buildempty");
    build_index(mcx_owner.mcx(), None, index, None, ForkNumber::INIT_FORKNUM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_build_seed_accepts_only_unsigned_integers() {
        assert_eq!(parse_build_seed(None), None);
        assert_eq!(parse_build_seed(Some("42".to_string())), Some(42));
        assert_eq!(parse_build_seed(Some(" 7 ".to_string())), Some(7));
        assert_eq!(parse_build_seed(Some(String::new())), None);
        assert_eq!(parse_build_seed(Some("-1".to_string())), None);
        assert_eq!(parse_build_seed(Some("abc".to_string())), None);
    }

    // C's seeded build (-DHNSW_MEMORY's SeedRandom(42), hnswbuild.c:1134-1136)
    // of bytecmp's `spill` rows (vector(3), 20-byte values, m = 16) at
    // maintenance_work_mem = 1MB prints "hnsw graph no longer fits into
    // maintenance_work_mem after 1436 tuples". The serial accounting must
    // flush at the same tuple, testing before each tuple's level draw as C
    // does (hnswbuild.c:533, 552-556).
    #[test]
    fn serial_flush_point_matches_c_at_one_megabyte() {
        seed_random(42);
        let m = 16;
        let (ml, max_level) = (hnsw_get_ml(m), hnsw_get_max_level(m));
        let mut accounting = pgvector_hnsw_graph::Accounting::serial(1024 * 1024);
        let mut tuples = 0;
        while !accounting.is_full() {
            accounting.charge_element(random_level(ml, max_level), m, 20).unwrap();
            tuples += 1;
        }
        assert_eq!(tuples, 1436);
    }
}

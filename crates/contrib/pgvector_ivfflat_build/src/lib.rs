//! pgvector 0.8.7 ivfbuild.c, serial rendering. It samples rows (ANALYZE's
//! block sampler and reservoir), runs k-means (kmeans.rs, ivfkmeans.c) and
//! writes the metapage and list pages. Then it assigns every row to its
//! nearest center, sorts by list and loads the entry pages. Every page goes
//! through GenericXLog, as in C.
//! DIVERGENCE (recorded): no parallel build (plan_create_index_workers is
//! unported, M6); C builds serially when no workers launch.
//! pgrust.ivfflat_build_seed reseeds a build the way C's -DIVFFLAT_BENCH
//! build does (opt-in).

pub mod kmeans;

use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgvector_ivfflat::sort::{HeapSort, INT4_LESS_OPERATOR};
use pgvector_ivfflat::utils::*;
use types_core::{ForkNumber, InvalidBlockNumber, Oid, INT4OID, TIDOID, VARBITOID};
use types_error::{
    PgError, PgResult, ERRCODE_FEATURE_NOT_SUPPORTED, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED, NOTICE,
};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_tuple::itemptr::ItemPointerData;

pub struct IndexBuildResult {
    pub heap_tuples: f64,
    pub index_tuples: f64,
}

// IvfflatBuildState (ivfflat.h:198-250), the serial fields.
struct BuildState<'a, 'm, 'mcx> {
    // Info
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    type_info: &'static IvfflatTypeInfo,

    // Settings
    dimensions: i32,
    lists: i32,

    // Statistics
    indtuples: f64,
    reltuples: f64,

    // Support functions
    procinfo: FmgrInfo,
    normprocinfo: Option<FmgrInfo>,
    kmeansnormprocinfo: Option<FmgrInfo>,
    collation: Oid,

    // Variables
    cx: Mcx<'m>,
    samples: Option<VectorArray<'m>>,
    centers: VectorArray<'m>,
    list_info: Vec<ListInfo>,
    itemsize: usize,

    // Sampling
    rstate: Option<sampling::ReservoirStateData>,
    samplerows: f64,
    rowstoskip: f64,

    // Sorting
    sortstate: Option<HeapSort>,

    // Memory
    memory_used: usize,
}

// AddSample (ivfbuild.c:56-105).
fn add_sample(bs: &mut BuildState<'_, '_, '_>, values: &[Datum]) -> PgResult<()> {
    // C allocates in buildstate->tmpCtx and resets it after each row.
    let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");

    // Detoast once for all calls
    let value = detoast_value(tmp.mcx(), values[0])?;
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Check dimensions match index
    ivfflat_check_dim(bs.dimensions, bs.type_info, bs.collation, d)?;

    // Check with KMEANS_NORM_PROC that the value can be normalized since
    // spherical distance function expects unit vectors
    if let Some(np) = bs.kmeansnormprocinfo.as_mut() {
        if !ivfflat_check_norm(np, bs.collation, d)? {
            return Ok(());
        }
    }

    let samples = bs.samples.as_mut().expect("samples allocated");
    let targsamples = samples.maxlen;
    if samples.length < targsamples {
        samples.set(samples.length, &value)?;
        samples.length += 1;
    } else {
        let rstate = bs.rstate.as_mut().expect("reservoir initialized");
        if bs.rowstoskip < 0.0 {
            bs.rowstoskip = sampling::reservoir_get_next_s(rstate, bs.samplerows, targsamples as u32);
        }

        if bs.rowstoskip <= 0.0 {
            let k = (targsamples as f64 * sampling::sampler_random_fract(&mut rstate.randstate)) as i32;
            debug_assert!(k >= 0 && k < targsamples);
            samples.set(k, &value)?;
        }

        bs.rowstoskip -= 1.0;
    }

    // Increment after reservoir_get_next_S
    bs.samplerows += 1.0;
    Ok(())
}

// SampleRows (ivfbuild.c:135-159): sample rows with the same logic as
// ANALYZE. SampleCallback (ivfbuild.c:110-130) is the closure.
fn sample_rows<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<()> {
    let targsamples = bs.samples.as_ref().expect("samples allocated").maxlen;
    let totalblocks = bufmgr::RelationGetNumberOfBlocksInFork(heap, ForkNumber::MAIN_FORKNUM)?;

    bs.samplerows = 0.0;
    bs.rowstoskip = -1.0;

    let (mut sampler, _) =
        sampling::block_sampler_init(totalblocks, targsamples as u32, pg_prng::global_prng(|p| p.next_u32()));

    // reservoir_init_selection_state seeds from pg_prng_uint32
    // (sampling.c:139).
    bs.rstate = Some(sampling::reservoir_init_selection_state(
        pg_prng::global_prng(|p| p.next_u32()) as u64,
        targsamples as u32,
    ));

    let index = bs.index;
    while sampler.has_more() {
        let targblock = sampler.next();
        let mut inner_err: Option<Box<PgError>> = None;
        // BuildState is threaded via raw pointer: the callback is FnMut and
        // borrows would alias bs.
        let bs_ptr: *mut BuildState<'_, '_, 'mcx> = &mut *bs;

        // Set anyvisible to false like table_index_build_scan
        let r = execindexing::table_index_build_range_scan(
            mcx,
            heap,
            index,
            index_info,
            false,
            false,
            false,
            targblock,
            1,
            |_index_rel, _tid, values, isnull, _alive| {
                // Skip nulls
                if isnull[0] {
                    return Ok(());
                }
                // SAFETY: single-threaded serial build; bs outlives the scan.
                let bs = unsafe { &mut *bs_ptr };
                add_sample(bs, values).map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat sample failed").into()
                })
            },
        );
        if let Err(e) = r {
            return Err(inner_err.unwrap_or(e));
        }
    }

    // Normalize if needed
    if bs.kmeansnormprocinfo.is_some() {
        ivfflat_norm_vectors(bs.type_info, bs.collation, bs.samples.as_mut().expect("samples allocated"))?;
    }
    Ok(())
}

// AddTupleToSort (ivfbuild.c:164-225).
fn add_tuple_to_sort(bs: &mut BuildState<'_, '_, '_>, tid: &ItemPointerData, values: &[Datum]) -> PgResult<()> {
    let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");
    let mut min_distance = f64::MAX;
    let mut closest_center = 0i32;

    // Detoast once for all calls
    let mut value = detoast_value(tmp.mcx(), values[0])?;

    // Normalize if needed
    if let Some(np) = bs.normprocinfo.as_mut() {
        if !ivfflat_check_norm(np, bs.collation, Datum::from_usize(value.as_ptr() as usize))? {
            return Ok(());
        }
        value = ivfflat_norm_value(bs.type_info, bs.collation, tmp.mcx(), &value)?;
    }
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Check dimensions match index
    ivfflat_check_dim(bs.dimensions, bs.type_info, bs.collation, d)?;

    // Find the list that minimizes the distance
    for i in 0..bs.centers.length {
        let distance = types_fmgr::function_call2_coll(&mut bs.procinfo, bs.collation, d, bs.centers.datum(i)?)?
            .as_f64();
        if distance < min_distance {
            min_distance = distance;
            closest_center = i;
        }
    }

    // Add tuple to sort; tuplesort copies the input, the TID by reference
    // (TIDOID)
    let tid_datum = Datum::from_usize(tid as *const ItemPointerData as usize);
    bs.sortstate
        .as_mut()
        .expect("sort state")
        .put(&[Datum::from_i32(closest_center), tid_datum, d])?;

    bs.indtuples += 1.0;
    Ok(())
}

// GetNextTuple (ivfbuild.c:255-272): the next (list, index tuple) from the
// sort; list -1 once it is drained.
fn get_next_tuple<'m>(
    sort: &mut HeapSort,
    index: &Relation<'_>,
    cx: Mcx<'m>,
) -> PgResult<(i32, Option<nbtree::itup::ItupBuf<'m>>)> {
    let mut values = [Datum::from_usize(0); 3];
    if !sort.next(&mut values)? {
        return Ok((-1, None));
    }
    let list = values[0].as_i32();

    // Form the index tuple
    let mut itup = nbtree::itup::index_form_tuple(cx, &index.rd_att, &values[2..3], &[false])?;
    // SAFETY: the sort's TID column, 6 bytes at a by-reference datum.
    let tid = unsafe { core::ptr::read_unaligned(values[1].as_usize() as *const ItemPointerData) };
    // SAFETY: t_tid is the first 6 bytes of the image (itup.h).
    unsafe { itup.as_mut_ptr().cast::<ItemPointerData>().write_unaligned(tid) };
    Ok((list, Some(itup)))
}

#[cold]
fn add_item_failed(index: &Relation<'_>, line: i32, func: &'static str) -> Box<PgError> {
    elog_error(format!("failed to add index item to \"{}\"", index.name()), "ivfbuild.c", line, func)
}

// InsertTuples (ivfbuild.c:277-337): create the initial entry pages, one
// chain per list.
fn insert_tuples(index: &Relation<'_>, bs: &mut BuildState<'_, '_, '_>, fork_num: ForkNumber) -> PgResult<()> {
    let mut inserted: i64 = 0;
    let load_cx = mcx::MemoryContext::new("Ivfflat load context");
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let sort = bs.sortstate.as_mut().expect("sort state");

    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_LOAD,
    );
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_TOTAL,
        bs.indtuples as i64,
    );

    let (mut list, mut itup) = get_next_tuple(sort, index, load_cx.mcx())?;

    for i in 0..bs.centers.length {
        // Can take a while, so ensure we can interrupt
        // Needs to be called when no buffer locks are held
        postgres_seams::check_for_interrupts::call()?;

        let buf = ivfflat_new_buffer(index, fork_num)?;
        let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;
        let start_page = bufmgr::BufferGetBlockNumber(xp.buf);

        // Get all tuples for list
        while list == i {
            let t = itup.take().expect("a tuple for this list");
            // Check for free space
            let itemsz = t.size();
            if xp.free_space() < itemsz {
                xp = ivfflat_append_page(index, xp, fork_num, xcx.mcx())?;
            }

            // Add the item
            // SAFETY: the formed tuple's MAXALIGNed image.
            let item = unsafe { core::slice::from_raw_parts(t.as_ptr(), itemsz) };
            if xp.add_item(item).is_none() {
                return Err(add_item_failed(index, 321, "InsertTuples"));
            }
            drop(t);

            inserted += 1;
            backend_progress::pgstat_progress_update_param(
                backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_DONE,
                inserted,
            );

            (list, itup) = get_next_tuple(sort, index, load_cx.mcx())?;
        }

        let insert_page = bufmgr::BufferGetBlockNumber(xp.buf);
        xp.commit()?;

        // Set the start and insert pages
        ivfflat_update_list(index, bs.list_info[i as usize], insert_page, InvalidBlockNumber, start_page, fork_num)?;
    }
    Ok(())
}

// InitBuildState (ivfbuild.c:342-418).
fn init_build_state<'a, 'm, 'mcx>(
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    cx: Mcx<'m>,
) -> PgResult<BuildState<'a, 'm, 'mcx>> {
    let type_info = ivfflat_get_type_info(index)?;
    let lists = ivfflat_get_lists(index);
    let dimensions = index.rd_att.attr(0).atttypmod;

    // Disallow varbit since require fixed dimensions
    if index.rd_att.attr(0).atttypid == VARBITOID {
        return Err(PgError::error("type not supported for ivfflat index")
            .with_sqlstate(ERRCODE_FEATURE_NOT_SUPPORTED)
            .with_location("ivfbuild.c", 358, "InitBuildState")
            .into());
    }

    // Require column to have dimensions to be indexed
    if dimensions < 0 {
        return Err(PgError::error("column does not have dimensions")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .with_location("ivfbuild.c", 364, "InitBuildState")
            .into());
    }

    if dimensions > type_info.max_dimensions {
        return Err(PgError::error(format!(
            "column cannot have more than {} dimensions for ivfflat index",
            type_info.max_dimensions
        ))
        .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
        .with_location("ivfbuild.c", 369, "InitBuildState")
        .into());
    }

    // Get support functions
    let procinfo = index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?;
    let normprocinfo = ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?;
    let kmeansnormprocinfo = ivfflat_optional_proc_info(index, IVFFLAT_KMEANS_NORM_PROC)?;
    let collation = index_collation(index);

    // Require more than one dimension for spherical k-means
    if kmeansnormprocinfo.is_some() && dimensions == 1 {
        return Err(PgError::error("dimensions must be greater than one for this opclass")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .with_location("ivfbuild.c", 384, "InitBuildState")
            .into());
    }

    let itemsize = (type_info.item_size)(dimensions);
    let memory_used = mcx::add_size(0, vector_array_size(lists, itemsize)?)?;
    ivfflat_check_memory_usage(memory_used)?;
    let centers = VectorArray::init(cx, lists, dimensions, itemsize)?;

    Ok(BuildState {
        heap,
        index,
        type_info,
        dimensions,
        lists,
        indtuples: 0.0,
        reltuples: 0.0,
        procinfo,
        normprocinfo,
        kmeansnormprocinfo,
        collation,
        cx,
        samples: None,
        centers,
        list_info: vec![ListInfo { blkno: InvalidBlockNumber, offno: 0 }; lists as usize],
        itemsize,
        rstate: None,
        samplerows: 0.0,
        rowstoskip: -1.0,
        sortstate: None,
        memory_used,
    })
}

// ComputeCenters (ivfbuild.c:440-486).
fn compute_centers<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
) -> PgResult<()> {
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_KMEANS,
    );

    // Skip samples for unlogged table
    let num_samples: i32 = match bs.heap {
        None => 1,
        Some(heap) => {
            let max_tuples = bufmgr::RelationGetNumberOfBlocksInFork(heap, ForkNumber::MAIN_FORKNUM)? as i64
                * types_storage::bufpage::MaxHeapTuplesPerPage as i64;

            // Target 50 samples per list, with at least 10000 samples
            // The number of samples has a large effect on index build time
            let mut n = bs.lists * 50;
            if n < 10000 {
                n = 10000;
            }

            // Save memory since will not have more than max tuples
            (n as i64).min(max_tuples).max(1) as i32
        }
    };

    // Sample rows
    bs.memory_used = mcx::add_size(bs.memory_used, vector_array_size(num_samples, bs.itemsize)?)?;
    ivfflat_check_memory_usage(bs.memory_used)?;
    bs.samples = Some(VectorArray::init(bs.cx, num_samples, bs.dimensions, bs.itemsize)?);
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        sample_rows(bs, mcx, heap, index_info)?;

        if bs.samples.as_ref().expect("samples allocated").length < bs.lists {
            elog::ereport(NOTICE)
                .errmsg("ivfflat index created with little data")
                .errdetail("This will cause low recall.")
                .errhint("Drop the index until the table has more data.")
                .finish(types_error::ErrorLocation::new("ivfbuild.c", 477, "ComputeCenters"))?;
        }
    }

    // Calculate centers
    let samples = bs.samples.take().expect("samples allocated");
    kmeans::ivfflat_kmeans(bs.index, &samples, &mut bs.centers, bs.type_info, bs.memory_used)?;

    // Free samples before we allocate more memory
    drop(samples);
    Ok(())
}

// CreateMetaPage (ivfbuild.c:491-512).
fn create_meta_page(index: &Relation<'_>, dimensions: i32, lists: i32, fork_num: ForkNumber) -> PgResult<()> {
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let buf = ivfflat_new_buffer(index, fork_num)?;
    let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;

    // Set metapage data
    IvfflatMetaPage {
        magic_number: IVFFLAT_MAGIC_NUMBER,
        version: IVFFLAT_VERSION,
        dimensions: dimensions as u16,
        lists: lists as u16,
    }
    .write(xp.page_mut());
    xp.set_pd_lower((PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE) as u16);

    xp.commit()
}

// CreateListPages (ivfbuild.c:517-562).
fn create_list_pages(
    index: &Relation<'_>,
    centers: &VectorArray<'_>,
    lists: i32,
    fork_num: ForkNumber,
    list_info: &mut [ListInfo],
) -> PgResult<()> {
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let list_size = maxalign(ivfflat_list_size(centers.itemsize));
    let mut list = vec![0u8; list_size];

    let buf = ivfflat_new_buffer(index, fork_num)?;
    let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;

    for i in 0..lists {
        // Zero memory for each list
        list.fill(0);

        // Load list
        set_list_start_page(&mut list, InvalidBlockNumber);
        set_list_insert_page(&mut list, InvalidBlockNumber);
        let center = centers.get(i)?;
        let size = varsize_any(center);
        list[IVFFLAT_LIST_CENTER_OFFSET..IVFFLAT_LIST_CENTER_OFFSET + size].copy_from_slice(&center[..size]);

        // Ensure free space
        if xp.free_space() < list_size {
            xp = ivfflat_append_page(index, xp, fork_num, xcx.mcx())?;
        }

        // Add the item
        let Some(offno) = xp.add_item(&list) else {
            return Err(add_item_failed(index, 552, "CreateListPages"));
        };

        // Save location info
        list_info[i as usize] = ListInfo { blkno: bufmgr::BufferGetBlockNumber(xp.buf), offno };
    }

    xp.commit()
}

// AssignTuples (ivfbuild.c:977-1018), serial. BuildCallback
// (ivfbuild.c:230-250) is the closure.
fn assign_tuples<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
) -> PgResult<()> {
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_ASSIGN,
    );

    // Begin serial tuplesort (InitBuildSortState, ivfbuild.c:612-621, over
    // InitBuildState's sortdesc, ivfbuild.c:386-390)
    let value_type = bs.index.rd_att.attr(0).atttypid;
    bs.sortstate = Some(HeapSort::begin(
        "Ivfflat build sort",
        &[("list", INT4OID), ("tid", TIDOID), ("vector", value_type)],
        INT4_LESS_OPERATOR,
        init_small::globals::maintenance_work_mem(),
    )?);

    // Add tuples to sort
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        let index = bs.index;
        let mut inner_err: Option<Box<PgError>> = None;
        // BuildState is threaded via raw pointer: the callback is FnMut and
        // borrows would alias bs.
        let bs_ptr: *mut BuildState<'_, '_, 'mcx> = &mut *bs;
        let reltuples = execindexing::table_index_build_scan(
            mcx,
            heap,
            index,
            index_info,
            true,
            /* progress */ true,
            |_index_rel, tid, values, isnull, _alive| {
                // Skip nulls
                if isnull[0] {
                    return Ok(());
                }
                // SAFETY: single-threaded serial build; bs outlives the scan.
                let bs = unsafe { &mut *bs_ptr };
                add_tuple_to_sort(bs, tid, values).map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat build insert failed").into()
                })
            },
        );
        match reltuples {
            Ok(n) => bs.reltuples = n,
            Err(e) => return Err(inner_err.unwrap_or(e)),
        }
    }
    Ok(())
}

// CreateEntryPages (ivfbuild.c:1023-1041).
fn create_entry_pages<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<()> {
    // Assign
    assign_tuples(bs, mcx, index_info)?;

    // Sort
    bs.sortstate.as_mut().expect("sort state").perform()?;

    // Load
    let index = bs.index;
    insert_tuples(index, bs, fork_num)?;

    // End sort
    bs.sortstate = None;
    Ok(())
}

// BuildIndex (ivfbuild.c:1046-1064).
fn build_index<'mcx>(
    mcx: Mcx<'mcx>,
    heap: Option<&Relation<'mcx>>,
    index: &Relation<'mcx>,
    mut index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<IndexBuildResult> {
    let build_cx = mcx::MemoryContext::new("Ivfflat build context");
    let mut bs = init_build_state(heap, index, build_cx.mcx())?;

    compute_centers(&mut bs, mcx, index_info.as_deref_mut())?;

    // Create pages
    create_meta_page(index, bs.dimensions, bs.lists, fork_num)?;
    create_list_pages(index, &bs.centers, bs.lists, fork_num, &mut bs.list_info)?;
    create_entry_pages(&mut bs, mcx, index_info, fork_num)?;

    // Write WAL for initialization fork since GenericXLog functions do not
    if fork_num == ForkNumber::INIT_FORKNUM {
        let nblocks = bufmgr::RelationGetNumberOfBlocksInFork(index, fork_num)?;
        xloginsert::log_newpage_range(index, fork_num, 0, nblocks, true)?;
    }

    Ok(IndexBuildResult { heap_tuples: bs.reltuples, index_tuples: bs.indtuples })
}

// pgrust.ivfflat_build_seed: a placeholder customized option, deliberately
// NOT a registered GUC (pgrust.hnsw_build_seed's pattern; a pg_settings row
// would break byte-identical regression outputs). Set to N, ivfflatbuild calls
// SeedRandom(N) where C calls SeedRandom(42) under #ifdef IVFFLAT_BENCH
// (ivfbuild.c:1075-1077). Unset (the default) is stock C, which never seeds;
// ivfflatbuildempty never seeds, as in C.
// DIVERGENCE (opt-in only): stock C has no such switch; the byte-identical
// tier sets 42 to match the -DIVFFLAT_BENCH reference build.
fn ivfflat_build_seed() -> Option<u64> {
    // Uninstalled seam (unit-test binaries without a guc boot): stock C.
    if !guc_seams::get_config_option_missing_ok::is_installed() {
        return None;
    }
    parse_build_seed(
        guc_seams::get_config_option_missing_ok::call("pgrust.ivfflat_build_seed")
            .ok()
            .flatten(),
    )
}

fn parse_build_seed(value: Option<String>) -> Option<u64> {
    value.and_then(|v| v.trim().parse::<u64>().ok())
}

// ivfflatbuild (ivfbuild.c:1069-1086).
pub fn ivfflatbuild<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<IndexBuildResult> {
    // SeedRandom(42) under #ifdef IVFFLAT_BENCH, before BuildIndex.
    if let Some(seed) = ivfflat_build_seed() {
        pg_prng::global_prng(|p| p.seed(seed));
    }
    build_index(mcx, Some(heap), index, Some(index_info), ForkNumber::MAIN_FORKNUM)
}

// ivfflatbuildempty (ivfbuild.c:1091-1098): the unlogged init fork, built
// with no heap (one sample slot, so random centers).
pub fn ivfflatbuildempty(index: &Relation<'_>) -> PgResult<()> {
    let owner = mcx::MemoryContext::new_bump("ivfflat buildempty");
    build_index(owner.mcx(), None, index, None, ForkNumber::INIT_FORKNUM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // pgrust.ivfflat_build_seed parses as an unsigned integer, like
    // pgrust.hnsw_build_seed.
    #[test]
    fn ivf_parse_build_seed_accepts_only_unsigned_integers() {
        assert_eq!(parse_build_seed(None), None);
        assert_eq!(parse_build_seed(Some("42".to_string())), Some(42));
        assert_eq!(parse_build_seed(Some(" 7 ".to_string())), Some(7));
        assert_eq!(parse_build_seed(Some(String::new())), None);
        assert_eq!(parse_build_seed(Some("-1".to_string())), None);
        assert_eq!(parse_build_seed(Some("abc".to_string())), None);
    }
}

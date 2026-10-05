use crate::utils::*;
use datum::Datum;
use mcx::{Mcx, PgBox, PgVec};
use types_core::InvalidOid;
use types_error::{PgError, PgResult};
use types_fmgr::FmgrInfo;
use types_hnsw::*;
use types_rel::Relation;
use types_relscan::{relation_get_index_scan, IndexScanDescData, IndexScanOpaque};
use types_scan::scankey::{ScanKeyData, SK_ISNULL};
use types_snapshot::IsMVCCSnapshot;
use types_tuple::itemptr::ItemPointerData;

fn opaque<'a, 'mcx>(
    scan: &'a mut IndexScanDescData<'mcx>,
) -> &'a mut HnswScanOpaqueData<'mcx> {
    match &mut scan.opaque {
        IndexScanOpaque::Hnsw(so) => so,
        _ => unreachable!("non-hnsw scan opaque"),
    }
}

pub fn hnswbeginscan<'mcx>(
    mcx: Mcx<'mcx>,
    index: &Relation<'mcx>,
    nkeys: i32,
    norderbys: i32,
) -> PgResult<IndexScanDescData<'mcx>> {
    check_type_supported(index)?;
    let support = init_support(index)?;
    let max_memory = (init_small::globals::work_mem() as f64
        * guc_tables::vars::hnsw_scan_mem_multiplier.read()
        * 1024.0
        + 256.0)
        .min((usize::MAX / 2) as f64) as usize;

    let so = HnswScanOpaqueData {
        mcx,
        first: true,
        m: 0,
        tuples: 0,
        previous_distance: f64::NEG_INFINITY,
        max_memory,
        // hnswscan.c:154-156
        tmp_ctx: AllocSetModel::new(0, 8 * 1024, 256 * 1024),
        value: None,
        support,
        max_dimensions: HNSW_MAX_DIM as i32,
        norm_is_l2: false,
        w: Vec::new(),
        w_list: ListShape::NIL,
        visited: None,
        discarded: None,
    };
    relation_get_index_scan(
        mcx,
        index,
        nkeys,
        norderbys,
        IndexScanOpaque::Hnsw(PgBox::new_in(so, mcx)),
        xact::TransactionStartedDuringRecovery(),
    )
}

pub fn hnswrescan(
    scan: &mut IndexScanDescData<'_>,
    keys: Option<&[ScanKeyData]>,
    orderbys: Option<&[ScanKeyData]>,
) -> PgResult<()> {
    let nkeys = scan.numberOfKeys;
    let norderbys = scan.numberOfOrderBys;
    if let Some(keys) = keys {
        if nkeys > 0 {
            for (dst, src) in scan.keyData.iter_mut().zip(keys.iter()) {
                *dst = src.clone();
            }
        }
    }
    if let Some(orderbys) = orderbys {
        if norderbys > 0 {
            for (dst, src) in scan.orderByData.iter_mut().zip(orderbys.iter()) {
                *dst = src.clone();
            }
        }
    }
    let so = opaque(scan);
    so.first = true;
    so.tuples = 0;
    so.previous_distance = f64::NEG_INFINITY;
    // hnswrescan (hnswscan.c:176-182): v and discarded live in tmpCtx, which
    // is reset. The owned values are dropped so memory stays bounded across
    // arbitrarily many rescans.
    so.tmp_ctx.reset();
    so.value = None;
    so.w = Vec::new();
    so.w_list = ListShape::NIL;
    so.visited = None;
    so.discarded = None;
    Ok(())
}

pub fn hnswendscan(scan: &mut IndexScanDescData<'_>) -> PgResult<()> {
    let so = opaque(scan);
    so.value = None;
    so.w = Vec::new();
    so.w_list = ListShape::NIL;
    so.visited = None;
    so.discarded = None;
    Ok(())
}

// GetScanValue: detoast, then normalize for cosine-family opclasses.
// Scratch goes in a local bump (C allocates in tmpCtx); the returned image
// is owned so it frees at the next rescan.
fn get_scan_value(
    support: &mut HnswSupport,
    orderby: &ScanKeyData,
) -> PgResult<Option<Vec<u8>>> {
    if orderby.sk_flags & SK_ISNULL != 0 {
        return Ok(None);
    }
    let tmp = mcx::MemoryContext::new_bump("hnsw scan value");
    let tmcx = tmp.mcx();
    let d = orderby.sk_argument;
    let p = d.as_usize() as *const u8;
    // SAFETY: non-null vector varlena datum.
    let raw = unsafe {
        core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p))
    };
    let flat = detoast::detoast_attr(tmcx, raw)?;

    if support.normprocinfo.is_some() {
        let normed = norm_value(tmcx, &flat)?;
        return Ok(Some(normed.to_vec()));
    }
    Ok(Some(flat.to_vec()))
}

// HnswNormValue for the vector opclasses: l2_normalize applied in place.
pub fn norm_value<'m>(mcx: Mcx<'m>, img: &[u8]) -> PgResult<PgVec<'m, u8>> {
    let v = pgvector::vec::VecView::from_payload(&img[4..])?;
    let mut b = pgvector::vec::VecBuilder::new(mcx, v.dim())?;
    let mut norm = 0.0f64;
    for x in v.iter() {
        norm += x as f64 * x as f64;
    }
    norm = norm.sqrt();
    if norm > 0.0 {
        for i in 0..v.dim() {
            b.set(i, (v.x(i) as f64 / norm) as f32);
        }
        for i in 0..v.dim() {
            if b.get(i).is_infinite() {
                return Err(PgError::error("value out of range: overflow")
                    .with_sqlstate(types_error::ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE)
                    .into());
            }
        }
    }
    Ok(b.image())
}

// Reloption invariants the rest of the HNSW code assumes, matching pgvector's
// limits (2 <= m <= HNSW_MAX_M, 4 <= ef_construction <= 1000). Meta-page fields
// are read from block 0 on disk unvalidated, so a corrupt or crafted meta page
// (e.g. m = 65535) could otherwise size multi-GB allocations and index fixed
// buffers. entry_level is a level index (stored per-element as a u8, -1 means
// "no entry"), so it is bounded to a small sane range as well.
const HNSW_MIN_M: i32 = 2;
const HNSW_MIN_EF_CONSTRUCTION: i32 = 4;
const HNSW_MAX_EF_CONSTRUCTION: i32 = 1000;
const HNSW_MIN_ENTRY_LEVEL: i32 = -1;
const HNSW_MAX_ENTRY_LEVEL: i32 = 255;

fn corrupt_meta(field: &str, value: i64) -> PgError {
    PgError::error(format!(
        "hnsw index metapage is corrupt: {field} = {value} is out of range"
    ))
    .with_sqlstate(types_error::ERRCODE_INDEX_CORRUPTED)
}

// Reject out-of-range meta-page fields as index corruption before any of them
// size an allocation or index a fixed buffer.
fn validate_meta_fields(m: i32, ef_construction: i32, entry_level: i32) -> PgResult<()> {
    if !(HNSW_MIN_M..=HNSW_MAX_M).contains(&m) {
        return Err(corrupt_meta("m", m as i64).into());
    }
    if !(HNSW_MIN_EF_CONSTRUCTION..=HNSW_MAX_EF_CONSTRUCTION).contains(&ef_construction) {
        return Err(corrupt_meta("ef_construction", ef_construction as i64).into());
    }
    if !(HNSW_MIN_ENTRY_LEVEL..=HNSW_MAX_ENTRY_LEVEL).contains(&entry_level) {
        return Err(corrupt_meta("entry_level", entry_level as i64).into());
    }
    Ok(())
}

// GetScanItems (algorithm 5).
fn get_scan_items(scan: &mut IndexScanDescData<'_>) -> PgResult<()> {
    let index = scan
        .indexRelation
        .as_ref()
        .expect("index open")
        .alias();
    let so = match &mut scan.opaque {
        IndexScanOpaque::Hnsw(so) => &mut **so,
        _ => unreachable!(),
    };
    let meta = read_meta(&index)?;
    // Meta fields come from disk unvalidated; reject out-of-range values before
    // they drive the hash preallocation and layer buffers below.
    validate_meta_fields(meta.m as i32, meta.ef_construction as i32, meta.entry_level as i32)?;
    // Check dimensions match index (hnswscan.c:42-44): only for a non-NULL
    // value, and before the empty-index return below.
    if let Some(v) = so.value.as_ref() {
        hnsw_check_dim(
            meta.dimensions as i32,
            so.support.collation,
            Datum::from_usize(v.as_ptr() as usize),
        )?;
    }
    so.m = meta.m as i32;
    let q = so
        .value
        .as_ref()
        .map(|v| Datum::from_usize(v.as_ptr() as usize));

    let Some(entry) = meta_entry_point(&meta) else {
        return Ok(());
    };

    let tmp = mcx::MemoryContext::new_bump("hnsw scan search");
    let tmcx = tmp.mcx();
    let mut pool = ElementPool::new(tmcx);
    let mut support = so.support.clone();

    let ep_id = pool.from_block(entry.blkno, entry.offno);
    // HnswGetMetaPageInfo's HnswInitElementFromBlock (hnswutils.c:325).
    so.tmp_ctx.palloc(HNSW_ELEMENT_DATA_SIZE);
    pool.get_mut(ep_id).level = entry.level;
    let mut ep_dist = 0.0f64;
    load_element(&mut pool, ep_id, Some(&mut ep_dist), q, &index, &mut support, false, None)?;
    // HnswEntryCandidate's HnswInitSearchCandidate, then list_make1 (hnswscan.c:52).
    so.tmp_ctx.palloc(HNSW_SEARCH_CANDIDATE_SIZE);
    let _ = ListShape::lappend_n(1, &mut so.tmp_ctx);

    let mut ep: PgVec<'_, SearchCandidate> = mcx::vec_with_capacity_in_infallible(tmcx, 1);
    ep.push(SearchCandidate { element: ep_id, distance: ep_dist });

    let mut lc = entry.level as i32;
    while lc >= 1 {
        // hnswscan.c:54-58: v == NULL; the previous ep List is not freed.
        ep = search_layer_disk(
            &mut pool, q, ep, 1, lc, &index, &mut support, so.m, false, None, None, None,
            true, None, Some(&mut so.tmp_ctx),
        )?;
        let _ = ListShape::lappend_n(ep.len(), &mut so.tmp_ctx);
        lc -= 1;
    }

    let ef_search = guc_tables::vars::hnsw_ef_search.read();
    // C reads the hnsw_iterative_scan GUC at each use (no cached copy).
    let iterative = guc_tables::vars::hnsw_iterative_scan.read() != HNSW_ITERATIVE_SCAN_OFF;
    let mut tuples = so.tuples;
    // hnswscan.c:60: &so->v, and &so->discarded only with iterative scans.
    let w = search_layer_disk(
        &mut pool,
        q,
        ep,
        ef_search,
        0,
        &index,
        &mut support,
        so.m,
        false,
        None,
        Some(&mut so.visited),
        if iterative { Some(&mut so.discarded) } else { None },
        true,
        Some(&mut tuples),
        Some(&mut so.tmp_ctx),
    )?;
    so.tuples = tuples;
    so.w_list = ListShape::lappend_n(w.len(), &mut so.tmp_ctx);
    so.support = support;
    so.w = w.iter().map(|sc| pool.scan_element(sc.element, sc.distance)).collect();
    Ok(())
}

// ResumeScanItems.
fn resume_scan_items(scan: &mut IndexScanDescData<'_>) -> PgResult<()> {
    let index = scan
        .indexRelation
        .as_ref()
        .expect("index open")
        .alias();
    let so = match &mut scan.opaque {
        IndexScanOpaque::Hnsw(so) => &mut **so,
        _ => unreachable!(),
    };
    so.w = Vec::new();
    let batch_size = guc_tables::vars::hnsw_ef_search.read();
    if so.discarded.as_ref().is_none_or(|d| d.is_empty()) {
        return Ok(());
    }

    let tmp = mcx::MemoryContext::new_bump("hnsw scan resume");
    let tmcx = tmp.mcx();
    let mut pool = ElementPool::new(tmcx);
    let mut support = so.support.clone();
    let q = so
        .value
        .as_ref()
        .map(|v| Datum::from_usize(v.as_ptr() as usize));

    let mut ep: PgVec<'_, SearchCandidate> =
        mcx::vec_with_capacity_in_infallible(tmcx, batch_size as usize);
    let mut ep_list = ListShape::NIL;
    {
        let dh = so.discarded.as_mut().expect("checked");
        for _ in 0..batch_size {
            let Some(e) = dh.remove_first() else { break };
            // ep = lappend(ep, sc) (hnswscan.c:88): sc is an existing
            // candidate, so only the List grows.
            ep_list.lappend(&mut so.tmp_ctx);
            let id = pool.from_block(e.blkno, e.offno);
            let pe = pool.get_mut(id);
            pe.level = e.level;
            pe.version = e.version;
            pe.heaptids = e.heaptids;
            pe.heaptids_len = e.heaptids_len;
            pe.neighbor_page = e.neighbor_page;
            pe.neighbor_offno = e.neighbor_offno;
            ep.push(SearchCandidate { element: id, distance: e.distance });
        }
    }

    let mut tuples = so.tuples;
    // hnswscan.c:91: &so->v and &so->discarded, initVisited = false.
    let w = search_layer_disk(
        &mut pool,
        q,
        ep,
        batch_size,
        0,
        &index,
        &mut support,
        so.m,
        false,
        None,
        Some(&mut so.visited),
        Some(&mut so.discarded),
        false,
        Some(&mut tuples),
        Some(&mut so.tmp_ctx),
    )?;
    so.tuples = tuples;
    so.w_list = ListShape::lappend_n(w.len(), &mut so.tmp_ctx);
    so.support = support;
    so.w = w.iter().map(|sc| pool.scan_element(sc.element, sc.distance)).collect();
    Ok(())
}

pub fn hnswgettuple(
    scan: &mut IndexScanDescData<'_>,
    _direction: types_scan::ScanDirection,
) -> PgResult<bool> {
    if opaque(scan).first {
        scan.xs_pgstat_index_scans += 1;
        scan.xs_nsearches += 1;

        if scan.numberOfOrderBys == 0 || scan.orderByData.is_empty() {
            return Err(PgError::error("cannot scan hnsw index without order").into());
        }
        if let Some(snap) = scan.xs_snapshot.as_ref() {
            if !IsMVCCSnapshot(snap) {
                return Err(
                    PgError::error("non-MVCC snapshots are not supported with hnsw").into(),
                );
            }
        }

        let index = scan.indexRelation.as_ref().expect("index open").alias();
        {
            let orderby = scan.orderByData[0].clone();
            let so = match &mut scan.opaque {
                IndexScanOpaque::Hnsw(so) => &mut **so,
                _ => unreachable!(),
            };
            let mut support = so.support.clone();
            so.value = get_scan_value(&mut support, &orderby)?;
            so.support = support;
            // HnswNormValue's palloc0(VECTOR_SIZE(dim)) in tmpCtx (hnswscan.c:115).
            if so.support.normprocinfo.is_some() {
                if let Some(v) = so.value.as_ref() {
                    so.tmp_ctx.palloc(v.len());
                }
            }
        }

        lmgr::LockPage(&index, HNSW_SCAN_LOCK, types_storage::lock::ShareLock)?;
        let r = get_scan_items(scan);
        lmgr::UnlockPage(&index, HNSW_SCAN_LOCK, types_storage::lock::ShareLock)?;
        r?;
        opaque(scan).first = false;
    }

    loop {
        let index = scan.indexRelation.as_ref().expect("index open").alias();
        let (need_resume, drain_one, done) = {
            let so = opaque(scan);
            if !so.w.is_empty() {
                (false, false, false)
            } else if guc_tables::vars::hnsw_iterative_scan.read() == HNSW_ITERATIVE_SCAN_OFF {
                (false, false, true)
            } else if so.discarded.is_none() {
                (false, false, true)
            } else if so.tuples >= guc_tables::vars::hnsw_max_scan_tuples.read() as i64
                || so.tmp_ctx.mem_allocated() > so.max_memory
            {
                // Reached max number of tuples or memory limit (hnswscan.c:264).
                if so.discarded.as_ref().expect("some").is_empty() {
                    (false, false, true)
                } else {
                    (false, true, false)
                }
            } else {
                (true, false, false)
            }
        };
        if done {
            break;
        }
        if drain_one {
            let so = opaque(scan);
            let e = so.discarded.as_mut().expect("some").remove_first().expect("nonempty");
            // so->w = lappend(so->w, ...) (hnswscan.c:270).
            so.w_list.lappend(&mut so.tmp_ctx);
            so.w.push(e);
        } else if need_resume {
            lmgr::LockPage(&index, HNSW_SCAN_LOCK, types_storage::lock::ShareLock)?;
            let r = resume_scan_items(scan);
            lmgr::UnlockPage(&index, HNSW_SCAN_LOCK, types_storage::lock::ShareLock)?;
            r?;
            if opaque(scan).w.is_empty() {
                break;
            }
        }

        let so = opaque(scan);
        let last = so.w.len() - 1;
        if so.w[last].heaptids_len == 0 {
            so.w.pop();
            // list_delete_last, then pfree(element) and pfree(sc) with
            // iterative scans (hnswscan.c:302-311).
            so.w_list.delete_last(&mut so.tmp_ctx);
            if guc_tables::vars::hnsw_iterative_scan.read() != HNSW_ITERATIVE_SCAN_OFF {
                so.tmp_ctx.pfree(HNSW_ELEMENT_DATA_SIZE);
                so.tmp_ctx.pfree(HNSW_SEARCH_CANDIDATE_SIZE);
            }
            continue;
        }
        let sc_distance = so.w[last].distance;
        let e = &mut so.w[last];
        e.heaptids_len -= 1;
        let heaptid: ItemPointerData = e.heaptids[e.heaptids_len as usize];

        if guc_tables::vars::hnsw_iterative_scan.read() == HNSW_ITERATIVE_SCAN_STRICT {
            if sc_distance < so.previous_distance {
                continue;
            }
            so.previous_distance = sc_distance;
        }

        scan.xs_heaptid = heaptid;
        scan.xs_recheck = false;
        scan.xs_recheckorderby = false;
        // Order-by value is not returned (amcanorderbyop distance only).
        for v in scan.xs_orderbynulls.iter_mut() {
            *v = true;
        }
        let _ = InvalidOid;
        return Ok(true);
    }
    Ok(false)
}

// Marker so FmgrInfo stays nameable from this module (support carrier).
#[allow(dead_code)]
fn _support_ty(_: &FmgrInfo) {}

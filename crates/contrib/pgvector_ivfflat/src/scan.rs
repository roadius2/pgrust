//! ivfscan.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_SHARE};
use datum::Datum;
use mcx::{Mcx, PgBox};
use types_core::{ForkNumber, InvalidBlockNumber, FLOAT8OID, TIDOID};
use types_error::PgResult;
use types_ivfflat::*;
use types_rel::Relation;
use types_relscan::{relation_get_index_scan, IndexScanDescData, IndexScanOpaque};
use types_scan::scankey::{ScanKeyData, SK_ISNULL};
use types_snapshot::IsMVCCSnapshot;
use types_storage::buf::BufferAccessStrategyType;
use types_storage::storage::ReadBufferMode;
use types_tuple::itemptr::ItemPointerData;

use crate::sort::{HeapSort, FLOAT8_LESS_OPERATOR};
use crate::utils::*;

fn opaque<'a>(scan: &'a mut IndexScanDescData<'_>) -> &'a mut IvfflatScanOpaqueData {
    match &mut scan.opaque {
        IndexScanOpaque::Ivfflat(so) => so,
        _ => unreachable!("non-ivfflat scan opaque"),
    }
}

// so->sortstate.
fn scan_sort(so: &mut IvfflatScanOpaqueData) -> &mut HeapSort {
    so.sortstate
        .as_mut()
        .and_then(|s| s.downcast_mut::<HeapSort>())
        .expect("ivfflat scan sort state")
}

// so->distfunc: ZeroDistance (ivfscan.c:192-196) for a NULL query, else
// FunctionCall2Coll with the index's distance proc.
fn scan_distance(so: &mut IvfflatScanOpaqueData, a: Datum, b: Datum) -> PgResult<f64> {
    if so.zero_distance {
        return Ok(0.0);
    }
    Ok(types_fmgr::function_call2_coll(&mut so.procinfo, so.collation, a, b)?.as_f64())
}

// The query image as a Datum; 0 for a NULL query, which ZeroDistance never
// reads.
fn value_datum(value: &Option<Vec<u8>>) -> Datum {
    value.as_ref().map_or(Datum::from_usize(0), |v| Datum::from_usize(v.as_ptr() as usize))
}

// GetScanLists (ivfscan.c:47-118): the nearest max_probes lists, nearest
// first in so->listPages.
fn get_scan_lists(index: &Relation<'_>, so: &mut IvfflatScanOpaqueData, value: Datum) -> PgResult<()> {
    let mut nextblkno = IVFFLAT_HEAD_BLKNO;
    let mut list_count: i32 = 0;
    let mut max_distance = f64::MAX;

    // Search all list pages
    while nextblkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, nextblkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let maxoffno = page_max_offset(cpage);

        for offno in FIRST_OFFSET_NUMBER..=maxoffno {
            let list = page_item(cpage, offno);

            // Use procinfo from the index instead of scan key for performance
            let center = Datum::from_usize(list_center(list).as_ptr() as usize);
            let distance = scan_distance(so, center, value)?;

            if list_count < so.max_probes {
                list_count += 1;

                // Add to heap
                so.list_queue.add(IvfflatScanList { start_page: list_start_page(list), distance });

                // Calculate max distance
                if list_count == so.max_probes {
                    max_distance = so.list_queue.first().expect("queued lists").distance;
                }
            } else if distance < max_distance {
                // Remove
                so.list_queue.remove_first();

                // Reuse
                so.list_queue.add(IvfflatScanList { start_page: list_start_page(list), distance });

                // Update max distance
                max_distance = so.list_queue.first().expect("queued lists").distance;
            }
        }

        nextblkno = page_nextblkno(cpage);
        UnlockReleaseBuffer(cbuf)?;
    }

    for i in (0..list_count as usize).rev() {
        so.list_pages[i] = so.list_queue.remove_first().expect("a queued list").start_page;
    }

    debug_assert!(so.list_queue.is_empty());
    Ok(())
}

// GetScanItems (ivfscan.c:123-187): the items of the next `probes` lists,
// sorted by distance.
fn get_scan_items(index: &Relation<'_>, so: &mut IvfflatScanOpaqueData, value: Datum) -> PgResult<()> {
    let mut batch_probes = 0;

    scan_sort(so).reset()?;

    // Search closest probes lists. C's `(++batchProbes) <= so->probes` runs
    // only when the first test passes.
    while so.list_index < so.max_probes && {
        batch_probes += 1;
        batch_probes <= so.probes
    } {
        let mut search_page = so.list_pages[so.list_index as usize];
        so.list_index += 1;

        // Search all entry pages for list
        while search_page != InvalidBlockNumber {
            let buf = bufmgr::ReadBufferExtended(
                index,
                ForkNumber::MAIN_FORKNUM,
                search_page,
                ReadBufferMode::Normal,
                so.bas.clone(),
            )?;
            LockBuffer(buf, BUFFER_LOCK_SHARE)?;
            let page = buffer_page(buf);
            let maxoffno = page_max_offset(page);

            for offno in FIRST_OFFSET_NUMBER..=maxoffno {
                let itup = page_item(page, offno).as_ptr();
                let mut isnull = false;
                // SAFETY: an index tuple on a share-locked page; one column.
                let datum = unsafe { nbtree::itup::index_getattr(itup, 1, &index.rd_att, &mut isnull) };

                // Add virtual tuple: (distance, &itup->t_tid). Use procinfo
                // from the index instead of scan key for performance
                let distance = scan_distance(so, datum, value)?;
                scan_sort(so).put(&[Datum::from_f64(distance), Datum::from_usize(itup as usize)])?;
            }

            search_page = page_nextblkno(page);
            UnlockReleaseBuffer(buf)?;
        }
    }

    scan_sort(so).perform()
}

// GetScanValue (ivfscan.c:198-236). C uses the scan key's value in place
// (it asserts that value is neither compressed nor toasted); this keeps a
// detoasted copy, as hnsw's scan does.
fn get_scan_value(so: &mut IvfflatScanOpaqueData, orderby: &ScanKeyData) -> PgResult<Option<Vec<u8>>> {
    if orderby.sk_flags & SK_ISNULL != 0 {
        so.zero_distance = true;
        return Ok(None);
    }
    so.zero_distance = false;

    let tmp = mcx::MemoryContext::new_bump("Ivfflat scan value");
    let mut value = detoast_value(tmp.mcx(), orderby.sk_argument)?;

    // Normalize if needed
    if so.normprocinfo.is_some() {
        value = ivfflat_norm_value(so.type_info, so.collation, tmp.mcx(), &value)?;
    }

    // Check dimensions match index
    ivfflat_check_dim(so.dimensions, so.type_info, so.collation, Datum::from_usize(value.as_ptr() as usize))?;

    Ok(Some(value.to_vec()))
}

// ivfflatbeginscan (ivfscan.c:252-333).
pub fn ivfflatbeginscan<'mcx>(
    mcx: Mcx<'mcx>,
    index: &Relation<'mcx>,
    nkeys: i32,
    norderbys: i32,
) -> PgResult<IndexScanDescData<'mcx>> {
    let mut probes = guc_tables::vars::ivfflat_probes.read();

    // Get lists and dimensions from metapage
    let (lists, dimensions) = ivfflat_get_meta_page_info(index)?;

    let mut max_probes = if guc_tables::vars::ivfflat_iterative_scan.read() != IVFFLAT_ITERATIVE_SCAN_OFF {
        guc_tables::vars::ivfflat_max_probes.read().max(probes)
    } else {
        probes
    };

    if probes > lists {
        probes = lists;
    }

    if max_probes > lists {
        max_probes = lists;
    }

    let so = IvfflatScanOpaqueData {
        type_info: ivfflat_get_type_info(index)?,
        probes,
        max_probes,
        dimensions,
        first: true,
        value: None,
        zero_distance: false,
        // Set support functions
        procinfo: index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?,
        normprocinfo: ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?,
        collation: index_collation(index),
        // Reuse same set of shared buffers for scan
        bas: bufmgr::GetAccessStrategy(BufferAccessStrategyType::BasBulkread),
        list_queue: new_scan_list_heap(),
        list_pages: vec![InvalidBlockNumber; max_probes as usize],
        list_index: 0,
        // Prep sort: (distance float8, heaptid tid), by distance
        sortstate: Some(Box::new(HeapSort::begin(
            "Ivfflat scan temporary context",
            &[("distance", FLOAT8OID), ("heaptid", TIDOID)],
            FLOAT8_LESS_OPERATOR,
            init_small::globals::work_mem(),
        )?)),
    };

    relation_get_index_scan(
        mcx,
        index,
        nkeys,
        norderbys,
        IndexScanOpaque::Ivfflat(PgBox::new_in(so, mcx)),
        xact::TransactionStartedDuringRecovery(),
    )
}

// ivfflatrescan (ivfscan.c:335-358).
pub fn ivfflatrescan(
    scan: &mut IndexScanDescData<'_>,
    keys: Option<&[ScanKeyData]>,
    orderbys: Option<&[ScanKeyData]>,
) -> PgResult<()> {
    {
        let so = opaque(scan);
        so.first = true;
        so.list_queue.reset();
        so.list_index = 0;
        so.value = None;
    }

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
    Ok(())
}

// ivfflatgettuple (ivfscan.c:360-417). The index can be scanned backward,
// but Postgres doesn't support backward scan on operators.
pub fn ivfflatgettuple(scan: &mut IndexScanDescData<'_>, _direction: types_scan::ScanDirection) -> PgResult<bool> {
    let index = scan.indexRelation.as_ref().expect("index open").alias();

    if opaque(scan).first {
        // Count index scan for stats
        scan.xs_pgstat_index_scans += 1;
        scan.xs_nsearches += 1;

        // Safety check
        if scan.numberOfOrderBys == 0 || scan.orderByData.is_empty() {
            return Err(elog_error("cannot scan ivfflat index without order", "ivfscan.c", 389, "ivfflatgettuple"));
        }

        // Requires MVCC-compliant snapshot as not able to pin during sorting
        if let Some(snap) = scan.xs_snapshot.as_ref() {
            if !IsMVCCSnapshot(snap) {
                return Err(elog_error(
                    "non-MVCC snapshots are not supported with ivfflat",
                    "ivfscan.c",
                    394,
                    "ivfflatgettuple",
                ));
            }
        }

        let orderby = scan.orderByData[0].clone();
        let so = opaque(scan);
        let value = get_scan_value(so, &orderby)?;
        let d = value_datum(&value);
        get_scan_lists(&index, so, d)?;
        get_scan_items(&index, so, d)?;
        so.first = false;
        // The image does not move with the Vec, so d stays valid.
        so.value = value;
    }

    let so = opaque(scan);
    let mut values = [Datum::from_usize(0); 2];
    while !scan_sort(so).next(&mut values)? {
        if so.list_index == so.max_probes {
            return Ok(false);
        }
        let d = value_datum(&so.value);
        get_scan_items(&index, so, d)?;
    }

    // SAFETY: the sort's TID column, 6 bytes at a by-reference datum.
    let heaptid = unsafe { core::ptr::read_unaligned(values[1].as_usize() as *const ItemPointerData) };

    scan.xs_heaptid = heaptid;
    scan.xs_recheck = false;
    scan.xs_recheckorderby = false;
    // The order-by value is not returned (amcanorderbyop, distance only).
    for v in scan.xs_orderbynulls.iter_mut() {
        *v = true;
    }
    Ok(true)
}

// ivfflatendscan (ivfscan.c:419-434): tuplesort_end, then the scan's memory.
pub fn ivfflatendscan(scan: &mut IndexScanDescData<'_>) -> PgResult<()> {
    let so = opaque(scan);
    so.sortstate = None;
    so.value = None;
    Ok(())
}

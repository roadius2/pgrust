//! ivfinsert.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_SHARE};
use datum::Datum;
use mcx::Mcx;
use types_core::{BlockNumber, ForkNumber, InvalidBlockNumber, Oid};
use types_error::{PgError, PgResult};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_storage::lock::ExclusiveLock;
use types_tuple::itemptr::ItemPointerData;

use crate::utils::*;

// FindInsertPage (ivfinsert.c:19-67): the list whose center is nearest
// `value`, and that list's insert page. A tie keeps the first list.
fn find_insert_page(
    index: &Relation<'_>,
    value: Datum,
    procinfo: &mut FmgrInfo,
    collation: Oid,
) -> PgResult<(BlockNumber, ListInfo)> {
    let mut min_distance = f64::MAX;
    let mut nextblkno = IVFFLAT_HEAD_BLKNO;
    let mut insert_page = InvalidBlockNumber;

    // Avoid compiler warning
    let mut list_info = ListInfo { blkno: nextblkno, offno: FIRST_OFFSET_NUMBER };

    // Search all list pages
    while nextblkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, nextblkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let maxoffno = page_max_offset(cpage);

        for offno in FIRST_OFFSET_NUMBER..=maxoffno {
            let list = page_item(cpage, offno);
            let center = Datum::from_usize(list_center(list).as_ptr() as usize);
            let distance = types_fmgr::function_call2_coll(procinfo, collation, value, center)?.as_f64();

            if distance < min_distance || insert_page == InvalidBlockNumber {
                insert_page = list_insert_page(list);
                list_info = ListInfo { blkno: nextblkno, offno };
                min_distance = distance;
            }
        }

        nextblkno = page_nextblkno(cpage);
        UnlockReleaseBuffer(cbuf)?;
    }
    Ok((insert_page, list_info))
}

// InsertTuple (ivfinsert.c:72-184).
fn insert_tuple(
    index: &Relation<'_>,
    values: &[Datum],
    isnull: &[bool],
    heap_tid: &ItemPointerData,
    cx: Mcx<'_>,
) -> PgResult<()> {
    let type_info = ivfflat_get_type_info(index)?;
    let collation = index_collation(index);

    // Detoast once for all calls
    let mut value = detoast_value(cx, values[0])?;

    // Normalize if needed
    if let Some(mut normprocinfo) = ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)? {
        if !ivfflat_check_norm(&mut normprocinfo, collation, Datum::from_usize(value.as_ptr() as usize))? {
            return Ok(());
        }
        value = ivfflat_norm_value(type_info, collation, cx, &value)?;
    }
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Ensure index is valid
    let (_, dimensions) = ivfflat_get_meta_page_info(index)?;

    // Check dimensions match index
    ivfflat_check_dim(dimensions, type_info, collation, d)?;

    // Find the insert page - sets the page and list info
    let mut procinfo = index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?;
    let (mut insert_page, list_info) = find_insert_page(index, d, &mut procinfo, collation)?;
    // DIVERGENCE: C only asserts this (ivfinsert.c:110). InvalidBlockNumber
    // is P_NEW to ReadBuffer, which would extend the index from a corrupt
    // list item.
    if insert_page == InvalidBlockNumber {
        return Err(PgError::error(format!(
            "ivfflat index \"{}\" has a list with no insert page",
            index.name()
        ))
        .with_sqlstate(types_error::ERRCODE_INDEX_CORRUPTED)
        .into());
    }
    let original_insert_page = insert_page;

    // Form tuple
    let mut itup = nbtree::itup::index_form_tuple(cx, &index.rd_att, &[d], &isnull[..1])?;
    // SAFETY: t_tid is the first 6 bytes of the image (itup.h).
    unsafe { itup.as_mut_ptr().cast::<ItemPointerData>().write_unaligned(*heap_tid) };

    // Get tuple size
    let itemsz = itup.size();
    // SAFETY: the formed tuple's MAXALIGNed image.
    let item = unsafe { core::slice::from_raw_parts(itup.as_ptr(), itemsz) };

    let xcx = mcx::MemoryContext::new("Ivfflat insert xlog");

    // Find a page to insert the item
    let mut xp = loop {
        let buf = bufmgr::ReadBuffer(index, insert_page)?;
        LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
        let xp = XlogPage::register(index, buf, 0, xcx.mcx())?;

        if xp.free_space() >= itemsz {
            break xp;
        }

        insert_page = page_nextblkno(xp.page());

        if insert_page != InvalidBlockNumber {
            // Move to next page
            xp.abort()?;
            continue;
        }

        // Add a new page
        lmgr::LockRelationForExtension(index, ExclusiveLock)?;
        let newbuf = ivfflat_new_buffer(index, ForkNumber::MAIN_FORKNUM)?;
        lmgr::UnlockRelationForExtension(index, ExclusiveLock)?;

        // Update insert page
        insert_page = bufmgr::BufferGetBlockNumber(newbuf);

        // Init the new page, link the previous buffer to it, commit, unlock
        // the previous buffer and prepare the new one
        break xp.link_new_page(index, newbuf, 0, xcx.mcx())?;
    };

    // Add to next offset
    if xp.add_item(item).is_none() {
        return Err(elog_error(
            format!("failed to add index item to \"{}\"", index.name()),
            "ivfinsert.c",
            177,
            "InsertTuple",
        ));
    }

    xp.commit()?;

    // Update the insert page
    if insert_page != original_insert_page {
        ivfflat_update_list(
            index,
            list_info,
            insert_page,
            original_insert_page,
            InvalidBlockNumber,
            ForkNumber::MAIN_FORKNUM,
        )?;
    }
    Ok(())
}

// ivfflatinsert (ivfinsert.c:186-222).
pub fn ivfflatinsert<'mcx>(
    _mcx: Mcx<'mcx>,
    index: &Relation<'mcx>,
    values: &[Datum],
    isnull: &[bool],
    heap_tid: &ItemPointerData,
    _heap: &Relation<'mcx>,
) -> PgResult<bool> {
    // Skip nulls
    if isnull.first().copied().unwrap_or(true) {
        return Ok(false);
    }

    // Use memory context since detoast, IvfflatNormValue, and
    // index_form_tuple can allocate
    let insert_ctx = mcx::MemoryContext::new("Ivfflat insert temporary context");

    // Insert tuple
    insert_tuple(index, values, isnull, heap_tid, insert_ctx.mcx())?;

    Ok(false)
}

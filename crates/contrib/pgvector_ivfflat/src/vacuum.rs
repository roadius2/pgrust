//! ivfvacuum.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, LockBufferForCleanup, UnlockReleaseBuffer, BUFFER_LOCK_SHARE};
use nbtree::IndexVacuumInfo;
use types_core::{BlockNumber, ForkNumber, InvalidBlockNumber};
use types_error::PgResult;
use types_ivfflat::*;
use types_nbtree::genam::IndexBulkDeleteResult;
use types_storage::buf::BufferAccessStrategyType;
use types_storage::storage::ReadBufferMode;
use types_tuple::itemptr::{ItemPointerData, ItemPointerGetBlockNumberNoCheck, ItemPointerGetOffsetNumberNoCheck};

use crate::utils::*;

fn tid_cmp(a: &ItemPointerData, b: &ItemPointerData) -> core::cmp::Ordering {
    (ItemPointerGetBlockNumberNoCheck(a), ItemPointerGetOffsetNumberNoCheck(a))
        .cmp(&(ItemPointerGetBlockNumberNoCheck(b), ItemPointerGetOffsetNumberNoCheck(b)))
}

// The callback lazy vacuum passes C's ambulkdelete: is this heap TID dead?
fn tid_reaped(dead_items: &[ItemPointerData], tid: &ItemPointerData) -> bool {
    dead_items.binary_search_by(|probe| tid_cmp(probe, tid)).is_ok()
}

// ivfflatbulkdelete (ivfvacuum.c:15-143). pgrust hands ambulkdelete the
// sorted dead-TID array its callback tests (indexam's split).
pub fn ivfflatbulkdelete<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    istat: Option<IndexBulkDeleteResult>,
    dead_items: &[ItemPointerData],
) -> PgResult<IndexBulkDeleteResult> {
    bulk_delete(info, istat, &mut |tid| Ok(tid_reaped(dead_items, tid)))
}

// validate_index's ambulkdelete: the same walk with a collect-only callback
// (validate_index_callback, index.c) that records every heap TID and deletes
// nothing.
pub fn ivfflatbulkdelete_collect<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    callback: &mut (dyn FnMut(&ItemPointerData) -> PgResult<()> + '_),
) -> PgResult<IndexBulkDeleteResult> {
    bulk_delete(info, None, &mut |tid| {
        callback(tid)?;
        Ok(false)
    })
}

// ivfflatbulkdelete's body over its callback.
fn bulk_delete(
    info: &IndexVacuumInfo<'_, '_>,
    istat: Option<IndexBulkDeleteResult>,
    callback: &mut dyn FnMut(&ItemPointerData) -> PgResult<bool>,
) -> PgResult<IndexBulkDeleteResult> {
    let index = info.index;
    let mut blkno = IVFFLAT_HEAD_BLKNO;
    let bas = bufmgr::GetAccessStrategy(BufferAccessStrategyType::BasBulkread);
    let mut stats = istat.unwrap_or_default();
    let xcx = mcx::MemoryContext::new("Ivfflat vacuum xlog");

    // Iterate over list pages
    while blkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, blkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let cmaxoffno = page_max_offset(cpage);

        // Iterate over lists
        let list_pages: Vec<BlockNumber> =
            (FIRST_OFFSET_NUMBER..=cmaxoffno).map(|o| list_start_page(page_item(cpage, o))).collect();

        let list_blkno = blkno;
        blkno = page_nextblkno(cpage);

        UnlockReleaseBuffer(cbuf)?;

        for (coffno, &start_page) in (FIRST_OFFSET_NUMBER..).zip(list_pages.iter()) {
            let mut search_page = start_page;
            let mut insert_page = InvalidBlockNumber;

            // Iterate over entry pages
            while search_page != InvalidBlockNumber {
                vacuum_seams::vacuum_delay_point::call(false)?;

                let buf = bufmgr::ReadBufferExtended(
                    index,
                    ForkNumber::MAIN_FORKNUM,
                    search_page,
                    ReadBufferMode::Normal,
                    bas.clone(),
                )?;

                // ambulkdelete cannot delete entries from pages that are
                // pinned by other backends
                LockBufferForCleanup(buf)?;

                let mut xp = XlogPage::register(index, buf, 0, xcx.mcx())?;
                let maxoffno = page_max_offset(xp.page());
                let mut deletable: Vec<u16> = Vec::new();

                // Find deleted tuples
                for offno in FIRST_OFFSET_NUMBER..=maxoffno {
                    // SAFETY: t_tid is the first 6 bytes of the index tuple.
                    let htup = unsafe {
                        core::ptr::read_unaligned(page_item(xp.page(), offno).as_ptr() as *const ItemPointerData)
                    };
                    if callback(&htup)? {
                        deletable.push(offno);
                        stats.tuples_removed += 1.0;
                    } else {
                        stats.num_index_tuples += 1.0;
                    }
                }

                // Set to first free page
                // Must be set before searchPage is updated
                if insert_page == InvalidBlockNumber && !deletable.is_empty() {
                    insert_page = search_page;
                }

                search_page = page_nextblkno(xp.page());

                if !deletable.is_empty() {
                    // Delete tuples
                    xp.index_multi_delete(&deletable);
                    xp.commit()?;
                } else {
                    xp.abort()?;
                }
            }

            // Update after all tuples deleted. We don't add or delete items
            // from lists pages, so offset won't change.
            if insert_page != InvalidBlockNumber {
                ivfflat_update_list(
                    index,
                    ListInfo { blkno: list_blkno, offno: coffno },
                    insert_page,
                    InvalidBlockNumber,
                    InvalidBlockNumber,
                    ForkNumber::MAIN_FORKNUM,
                )?;
            }
        }
    }

    bufmgr::FreeAccessStrategy(bas);
    Ok(stats)
}

// ivfflatvacuumcleanup (ivfvacuum.c:145-164).
pub fn ivfflatvacuumcleanup<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    istat: Option<IndexBulkDeleteResult>,
) -> PgResult<Option<IndexBulkDeleteResult>> {
    if info.analyze_only {
        return Ok(istat);
    }

    // stats is NULL if ambulkdelete not called
    // OK to return NULL if index not changed
    let Some(mut stats) = istat else {
        return Ok(None);
    };

    stats.num_pages = bufmgr::RelationGetNumberOfBlocksInFork(info.index, ForkNumber::MAIN_FORKNUM)?;
    Ok(Some(stats))
}

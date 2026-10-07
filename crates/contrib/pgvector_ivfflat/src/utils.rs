//! ivfutils.c (pgvector 0.8.7), with ivfflat.h's inline VectorArrayGet and
//! VectorArraySet: vector arrays, support procs, page and GenericXLog
//! helpers, dimension checks, and the type info an opclass's support proc 5
//! names.

use core::ptr::NonNull;

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_SHARE};
use datum::Datum;
use generic_xlog::{
    GenericXLogAbort, GenericXLogFinish, GenericXLogStart, GenericXLogState,
    GENERIC_XLOG_FULL_IMAGE,
};
use mcx::{Mcx, PgBox, PgVec};
use types_core::{BlockNumber, Buffer, ForkNumber, InvalidBlockNumber, InvalidOid, Oid, BLCKSZ};
use types_error::{PgError, PgResult};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_storage::bufpage::{PageMut, PageRef};
use types_storage::storage::ReadBufferMode;

// An elog(ERROR, ...) at its C call site (XX000).
#[cold]
pub fn elog_error(
    msg: impl Into<String>,
    file: &'static str,
    line: i32,
    func: &'static str,
) -> Box<PgError> {
    PgError::error(msg).with_location(file, line, func).into()
}

// ---- vector arrays (ivfutils.c:21-53, ivfflat.h:319-343) ----

// sizeof(VectorArrayData): int length, maxlen, dim; Size itemsize; char *items.
const VECTOR_ARRAY_DATA_SIZE: usize = 32;

// VECTOR_ARRAY_SIZE (ivfflat.h:319).
pub fn vector_array_size(length: i32, size: usize) -> PgResult<usize> {
    mcx::add_size(VECTOR_ARRAY_DATA_SIZE, mcx::mul_size(length as usize, maxalign(size))?)
}

// VectorArrayData (ivfflat.h:120-127): `maxlen` slots of `itemsize`
// (MAXALIGNed) bytes, zeroed; the first `length` hold varlena images.
pub struct VectorArray<'m> {
    pub length: i32,
    pub maxlen: i32,
    pub dim: i32,
    pub itemsize: usize,
    items: PgVec<'m, u8>,
}

impl<'m> VectorArray<'m> {
    // VectorArrayInit (ivfutils.c:24-43): palloc_extended with
    // MCXT_ALLOC_ZERO | MCXT_ALLOC_HUGE.
    pub fn init(mcx: Mcx<'m>, maxlen: i32, dimensions: i32, itemsize: usize) -> PgResult<VectorArray<'m>> {
        // Safety check
        if maxlen < 1 || dimensions < 1 || itemsize == 0 {
            return Err(elog_error("cannot create vector array", "ivfutils.c", 31, "VectorArrayInit"));
        }
        // Ensure items are aligned to prevent UB
        let itemsize = maxalign(itemsize);
        let len = mcx::mul_size(maxlen as usize, itemsize)?;
        let mut items: PgVec<'m, u8> = mcx::vec_with_capacity_huge_in(mcx, len)?;
        items.resize(len, 0);
        Ok(VectorArray { length: 0, maxlen, dim: dimensions, itemsize, items })
    }

    fn slot(&self, offset: i32) -> PgResult<usize> {
        // Safety check
        if offset < 0 || offset >= self.maxlen {
            return Err(elog_error("index out of bounds", "ivfflat.h", 328, "VectorArrayGet"));
        }
        Ok(offset as usize * self.itemsize)
    }

    // VectorArrayGet (ivfflat.h:323-331): the whole slot.
    pub fn get(&self, offset: i32) -> PgResult<&[u8]> {
        let start = self.slot(offset)?;
        Ok(&self.items[start..start + self.itemsize])
    }

    pub fn get_mut(&mut self, offset: i32) -> PgResult<&mut [u8]> {
        let start = self.slot(offset)?;
        Ok(&mut self.items[start..start + self.itemsize])
    }

    // PointerGetDatum(VectorArrayGet(arr, offset)).
    pub fn datum(&self, offset: i32) -> PgResult<Datum> {
        Ok(Datum::from_usize(self.get(offset)?.as_ptr() as usize))
    }

    // VectorArraySet (ivfflat.h:333-343): copies `val`'s varlena image.
    pub fn set(&mut self, offset: i32, val: &[u8]) -> PgResult<()> {
        let size = varsize_any(val);
        // Safety check
        if size > self.itemsize {
            return Err(elog_error("item too large", "ivfflat.h", 340, "VectorArraySet"));
        }
        self.get_mut(offset)?[..size].copy_from_slice(&val[..size]);
        Ok(())
    }
}

// VARSIZE_ANY of a varlena image.
pub fn varsize_any(val: &[u8]) -> usize {
    // SAFETY: `val` starts with a varlena header and holds the whole datum.
    unsafe { types_tuple::varatt::varsize_any(val.as_ptr()) }
}

// The varlena image a by-reference Datum points at.
pub fn datum_image<'a>(d: Datum) -> &'a [u8] {
    let p = d.as_usize() as *const u8;
    // SAFETY: a non-null varlena datum the caller keeps alive.
    unsafe { core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p)) }
}

// PG_DETOAST_DATUM: a flat copy (4-byte header) of a non-null varlena datum.
pub fn detoast_value<'m>(mcx: Mcx<'m>, value: Datum) -> PgResult<PgVec<'m, u8>> {
    detoast::detoast_attr(mcx, datum_image(value))
}

// ---- support procs (ivfutils.c:55-120) ----

// IvfflatGetLists (ivfutils.c:58-67).
pub fn ivfflat_get_lists(index: &Relation<'_>) -> i32 {
    index
        .rd_options
        .as_ref()
        .and_then(|o| o.ivfflat())
        .map(|o| o.lists)
        .unwrap_or(IVFFLAT_DEFAULT_LISTS)
}

// index_getprocid(index, 1, procnum): ivfflat indexes have one column.
fn index_getprocid(index: &Relation<'_>, procnum: u16) -> Oid {
    index.rd_support.get(procnum as usize - 1).copied().unwrap_or(InvalidOid)
}

// index_getprocinfo(index, 1, procnum) (indexam.c:917-955). Not reachable
// from SQL: CREATE OPERATOR CLASS checks the procs an ivfflat opclass has.
pub fn index_getprocinfo(index: &Relation<'_>, procnum: u16) -> PgResult<FmgrInfo> {
    let procid = index_getprocid(index, procnum);
    if procid == InvalidOid {
        return Err(elog_error(
            format!(
                "missing support function {procnum} for attribute 1 of index \"{}\"",
                index.name()
            ),
            "indexam.c",
            947,
            "index_getprocinfo",
        ));
    }
    fmgr_seams::fmgr_info::call(procid)
}

// IvfflatOptionalProcInfo (ivfutils.c:72-79).
pub fn ivfflat_optional_proc_info(index: &Relation<'_>, procnum: u16) -> PgResult<Option<FmgrInfo>> {
    if index_getprocid(index, procnum) == InvalidOid {
        return Ok(None);
    }
    index_getprocinfo(index, procnum).map(Some)
}

// index->rd_indcollation[0].
pub fn index_collation(index: &Relation<'_>) -> Oid {
    index.rd_indcollation.first().copied().unwrap_or(InvalidOid)
}

// IvfflatNormValue (ivfutils.c:84-88): the type's normalize function on a
// detoasted image; the result is a fresh image in `mcx`.
// DIVERGENCE: C calls a NULL normalize (bit given a norm proc by a
// hand-written opclass) and crashes; pgrust raises.
pub fn ivfflat_norm_value<'m>(
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    mcx: Mcx<'m>,
    img: &[u8],
) -> PgResult<PgVec<'m, u8>> {
    let Some(normalize) = type_info.normalize else {
        return Err(PgError::error("ivfflat type info has no normalize function").into());
    };
    let d = types_fmgr::fcinfo::direct_function_call1_coll_in(
        normalize,
        collation,
        mcx,
        Datum::from_usize(img.as_ptr() as usize),
    )?;
    let out = datum_image(d);
    let mut v: PgVec<'m, u8> = mcx::vec_with_capacity_in_infallible(mcx, out.len());
    let _ = mcx::vec_append_bytes(&mut v, out);
    Ok(v)
}

// IvfflatCheckNorm (ivfutils.c:93-97): a non-zero norm.
pub fn ivfflat_check_norm(procinfo: &mut FmgrInfo, collation: Oid, value: Datum) -> PgResult<bool> {
    Ok(types_fmgr::function_call1_coll(procinfo, collation, value)?.as_f64() > 0.0)
}

// IvfflatNormVectors (ivfutils.c:102-120). C normalizes in tmpCtx and resets
// it after each vector; here each result lives in a per-vector bump context.
pub fn ivfflat_norm_vectors(
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    arr: &mut VectorArray<'_>,
) -> PgResult<()> {
    for i in 0..arr.length {
        let tmp = mcx::MemoryContext::new_bump("Ivfflat norm temporary context");
        let value = arr.get(i)?;
        let new_value = ivfflat_norm_value(type_info, collation, tmp.mcx(), &value[..varsize_any(value)])?;
        // Safety check
        ivfflat_check_dim(arr.dim, type_info, collation, Datum::from_usize(new_value.as_ptr() as usize))?;
        arr.set(i, &new_value)?;
    }
    Ok(())
}

// IvfflatCheckMemoryUsage (ivfutils.c:125-134).
pub fn ivfflat_check_memory_usage(total_size: usize) -> PgResult<()> {
    check_memory_usage(total_size, init_small::globals::maintenance_work_mem())
}

fn check_memory_usage(total_size: usize, maintenance_work_mem: i32) -> PgResult<()> {
    // Add one to error message to ceil
    if total_size / 1024 > maintenance_work_mem as usize {
        return Err(PgError::error(format!(
            "memory required is {} MB, maintenance_work_mem is {} MB",
            total_size / (1024 * 1024) + 1,
            maintenance_work_mem / 1024
        ))
        .with_sqlstate(types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED)
        .with_location("ivfutils.c", 133, "IvfflatCheckMemoryUsage")
        .into());
    }
    Ok(())
}

// ---- pages (ivfutils.c:136-281) ----

// BufferGetPage, under the caller's content lock.
pub fn buffer_page<'a>(buf: Buffer) -> &'a [u8] {
    // SAFETY: the caller holds a content lock on `buf`.
    unsafe { core::slice::from_raw_parts(bufmgr_seams::buffer_get_page::call(buf).as_ptr(), BLCKSZ) }
}

fn page_ref(page: &[u8]) -> PageRef<'_> {
    // SAFETY: `page` is a whole page image.
    unsafe { PageRef::from_raw(NonNull::new_unchecked(page.as_ptr() as *mut u8)) }
}

// PageGetMaxOffsetNumber.
pub fn page_max_offset(page: &[u8]) -> u16 {
    page_ref(page).max_offset_number()
}

// PageGetItem(page, PageGetItemId(page, offno)).
pub fn page_item(page: &[u8], offno: u16) -> &[u8] {
    let pr = page_ref(page);
    let (p, len) = pr.item_raw(pr.item_id(offno));
    // SAFETY: item_raw bounds the item within the page.
    unsafe { core::slice::from_raw_parts(p, len as usize) }
}

// IvfflatNewBuffer (ivfutils.c:139-146): ReadBufferExtended(P_NEW), then an
// exclusive lock. P_NEW skips the relation extension lock, as in C; insert
// takes that lock itself.
pub fn ivfflat_new_buffer(index: &Relation<'_>, fork_num: ForkNumber) -> PgResult<Buffer> {
    let buf = bufmgr::ReadBufferExtended(index, fork_num, InvalidBlockNumber, ReadBufferMode::Normal, None)?;
    LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
    Ok(buf)
}

// IvfflatInitPage (ivfutils.c:151-157), on a page image.
pub fn ivfflat_init_page(page: &mut [u8]) {
    {
        // SAFETY: `page` is a whole BLCKSZ image the caller owns.
        let mut pm = unsafe { PageMut::from_raw(NonNull::new_unchecked(page.as_mut_ptr())) };
        pm.init(IVFFLAT_PAGE_OPAQUE_SIZE);
    }
    set_page_nextblkno(page, InvalidBlockNumber);
    set_page_id(page, IVFFLAT_PAGE_ID);
}

// A buffer registered in a GenericXLog record: C's (Buffer, Page,
// GenericXLogState *) triple. `page` points at the registered image inside
// the boxed state, which stays put until GenericXLogFinish or Abort.
pub struct XlogPage<'s> {
    pub buf: Buffer,
    state: PgBox<'s, GenericXLogState>,
    page: *mut u8,
}

impl<'s> XlogPage<'s> {
    // GenericXLogStart, then GenericXLogRegisterBuffer.
    pub fn register(index: &Relation<'_>, buf: Buffer, flags: i32, mcx: Mcx<'s>) -> PgResult<XlogPage<'s>> {
        let mut state = GenericXLogStart(mcx, index)?;
        let page = state.register_buffer(buf, flags)?.as_mut_ptr();
        Ok(XlogPage { buf, state, page })
    }

    pub fn page(&self) -> &[u8] {
        // SAFETY: the registered image, a whole page.
        unsafe { core::slice::from_raw_parts(self.page, BLCKSZ) }
    }

    pub fn page_mut(&mut self) -> &mut [u8] {
        // SAFETY: the registered image, a whole page, exclusively ours.
        unsafe { core::slice::from_raw_parts_mut(self.page, BLCKSZ) }
    }

    fn page_mut_ref(&mut self) -> PageMut<'_> {
        // SAFETY: as page_mut.
        unsafe { PageMut::from_raw(NonNull::new_unchecked(self.page)) }
    }

    // PageGetFreeSpace.
    pub fn free_space(&self) -> usize {
        page_ref(self.page()).free_space()
    }

    // PageAddItem(page, item, size, InvalidOffsetNumber, false, false);
    // None is InvalidOffsetNumber.
    pub fn add_item(&mut self, item: &[u8]) -> Option<u16> {
        self.page_mut_ref().add_item(item, 0, 0)
    }

    // PageGetItem on the registered image, writable.
    pub fn item_mut(&mut self, offno: u16) -> &mut [u8] {
        // SAFETY: `self.page` is the registered image, a whole page; building
        // the PageRef from the raw pointer (not a shared slice) keeps write
        // provenance for the item pointer below.
        let pr = unsafe { PageRef::from_raw(NonNull::new_unchecked(self.page)) };
        let (p, len) = pr.item_raw(pr.item_id(offno));
        // SAFETY: item_raw bounds the item within the registered image.
        unsafe { core::slice::from_raw_parts_mut(p as *mut u8, len as usize) }
    }

    // PageIndexMultiDelete.
    pub fn index_multi_delete(&mut self, offnos: &[u16]) {
        self.page_mut_ref().index_multi_delete(offnos);
    }

    // ((PageHeader) page)->pd_lower = pd_lower.
    pub fn set_pd_lower(&mut self, pd_lower: u16) {
        self.page_mut_ref().set_pd_lower(pd_lower);
    }

    // IvfflatCommitBuffer (ivfutils.c:173-178).
    pub fn commit(self) -> PgResult<()> {
        GenericXLogFinish(self.state)?;
        UnlockReleaseBuffer(self.buf)
    }

    // GenericXLogAbort, then UnlockReleaseBuffer.
    pub fn abort(self) -> PgResult<()> {
        GenericXLogAbort(self.state);
        UnlockReleaseBuffer(self.buf)
    }

    // The shared tail of IvfflatAppendPage (ivfutils.c:185-207) and
    // InsertTuple's new page (ivfinsert.c:143-171): register `newbuf` as a
    // full image in this record, link this page to it, initialize it,
    // commit, unlock this buffer, and start a new record on `newbuf`
    // registered with `next_flags`.
    pub fn link_new_page(
        mut self,
        index: &Relation<'_>,
        newbuf: Buffer,
        next_flags: i32,
        mcx: Mcx<'s>,
    ) -> PgResult<XlogPage<'s>> {
        let newpage = self.state.register_buffer(newbuf, GENERIC_XLOG_FULL_IMAGE)?.as_mut_ptr();
        set_page_nextblkno(self.page_mut(), bufmgr::BufferGetBlockNumber(newbuf));
        // SAFETY: newbuf's registered image, a whole page.
        ivfflat_init_page(unsafe { core::slice::from_raw_parts_mut(newpage, BLCKSZ) });
        GenericXLogFinish(self.state)?;
        UnlockReleaseBuffer(self.buf)?;
        XlogPage::register(index, newbuf, next_flags, mcx)
    }
}

// IvfflatInitRegisterPage (ivfutils.c:162-168).
pub fn ivfflat_init_register_page<'s>(index: &Relation<'_>, buf: Buffer, mcx: Mcx<'s>) -> PgResult<XlogPage<'s>> {
    let mut xp = XlogPage::register(index, buf, GENERIC_XLOG_FULL_IMAGE, mcx)?;
    ivfflat_init_page(xp.page_mut());
    Ok(xp)
}

// IvfflatAppendPage (ivfutils.c:185-207).
pub fn ivfflat_append_page<'s>(
    index: &Relation<'_>,
    xp: XlogPage<'s>,
    fork_num: ForkNumber,
    mcx: Mcx<'s>,
) -> PgResult<XlogPage<'s>> {
    // Get new buffer
    let newbuf = ivfflat_new_buffer(index, fork_num)?;
    xp.link_new_page(index, newbuf, GENERIC_XLOG_FULL_IMAGE, mcx)
}

// IvfflatGetMetaPageInfo (ivfutils.c:212-234): (lists, dimensions). C raises
// with the share lock held; this releases it first.
pub fn ivfflat_get_meta_page_info(index: &Relation<'_>) -> PgResult<(i32, i32)> {
    let buf = bufmgr::ReadBuffer(index, IVFFLAT_METAPAGE_BLKNO)?;
    LockBuffer(buf, BUFFER_LOCK_SHARE)?;
    let metap = IvfflatMetaPage::read(buffer_page(buf));
    UnlockReleaseBuffer(buf)?;
    if metap.magic_number != IVFFLAT_MAGIC_NUMBER {
        return Err(elog_error("ivfflat index is not valid", "ivfutils.c", 225, "IvfflatGetMetaPageInfo"));
    }
    Ok((metap.lists as i32, metap.dimensions as i32))
}

// IvfflatUpdateList (ivfutils.c:239-281): update the start or insert page of
// a list.
pub fn ivfflat_update_list(
    index: &Relation<'_>,
    list_info: ListInfo,
    insert_page: BlockNumber,
    original_insert_page: BlockNumber,
    start_page: BlockNumber,
    fork_num: ForkNumber,
) -> PgResult<()> {
    let buf = bufmgr::ReadBufferExtended(index, fork_num, list_info.blkno, ReadBufferMode::Normal, None)?;
    LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
    let xcx = mcx::MemoryContext::new("Ivfflat update list");
    let mut xp = XlogPage::register(index, buf, 0, xcx.mcx())?;
    let mut changed = false;
    {
        let list = xp.item_mut(list_info.offno);
        if insert_page != InvalidBlockNumber && insert_page != list_insert_page(list) {
            // Skip update if insert page is lower than original insert page
            // This is needed to prevent insert from overwriting vacuum
            if original_insert_page == InvalidBlockNumber || insert_page >= original_insert_page {
                set_list_insert_page(list, insert_page);
                changed = true;
            }
        }
        if start_page != InvalidBlockNumber && start_page != list_start_page(list) {
            set_list_start_page(list, start_page);
            changed = true;
        }
    }
    // Only commit if changed
    if changed {
        xp.commit()
    } else {
        xp.abort()
    }
}

// ---- dimension checks (ivfutils.c:283-301) ----

// IvfflatCheckExpectedDim (ivfutils.c:283-290).
pub fn ivfflat_check_expected_dim(expected: i32, dim: i32) -> PgResult<()> {
    if dim != expected {
        return Err(PgError::error(format!("expected {expected} dimensions, not {dim}"))
            .with_sqlstate(types_error::ERRCODE_DATA_EXCEPTION)
            .with_location("ivfutils.c", 289, "IvfflatCheckExpectedDim")
            .into());
    }
    Ok(())
}

// IvfflatCheckDim (ivfutils.c:295-301).
pub fn ivfflat_check_dim(
    expected: i32,
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    value: Datum,
) -> PgResult<()> {
    let dim = types_fmgr::fcinfo::direct_function_call1_coll(type_info.dimensions, collation, value)?.as_i32();
    ivfflat_check_expected_dim(expected, dim)
}

// ---- type info (ivfutils.c:303-460) ----

// VectorItemSize, HalfvecItemSize and BitItemSize (ivfutils.c:309-325):
// VECTOR_SIZE, HALFVEC_SIZE and VARBITTOTALLEN.
fn vector_item_size(dimensions: i32) -> usize {
    8 + 4 * dimensions as usize
}

fn halfvec_item_size(dimensions: i32) -> usize {
    8 + 2 * dimensions as usize
}

fn bit_item_size(dimensions: i32) -> usize {
    8 + (dimensions as usize).div_ceil(8)
}

// SET_VARSIZE (4-byte header).
fn set_varsize(v: &mut [u8], len: usize) {
    v[0..4].copy_from_slice(&types_tuple::varatt::set_varsize_4b_word(len as u32).to_ne_bytes());
}

// VectorUpdateCenter (ivfutils.c:327-337). The `unused` field keeps what the
// slot held, as in C.
fn vector_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    set_varsize(v, vector_item_size(dimensions));
    v[4..6].copy_from_slice(&(dimensions as i16).to_ne_bytes());
    for i in 0..dimensions as usize {
        v[8 + 4 * i..12 + 4 * i].copy_from_slice(&x[i].to_ne_bytes());
    }
}

// HalfvecUpdateCenter (ivfutils.c:339-349).
fn halfvec_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    set_varsize(v, halfvec_item_size(dimensions));
    v[4..6].copy_from_slice(&(dimensions as i16).to_ne_bytes());
    for i in 0..dimensions as usize {
        let h = pgvector::halfutils::float4_to_half_unchecked(x[i]);
        v[8 + 2 * i..10 + 2 * i].copy_from_slice(&h.to_ne_bytes());
    }
}

// BitUpdateCenter (ivfutils.c:351-365): a bit is set when its mean exceeds
// 0.5, most significant bit first.
fn bit_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    let total = bit_item_size(dimensions);
    set_varsize(v, total);
    v[4..8].copy_from_slice(&dimensions.to_ne_bytes());
    let nx = &mut v[8..total];
    nx.fill(0);
    for i in 0..dimensions as usize {
        nx[i / 8] |= ((x[i] > 0.5) as u8) << (7 - (i % 8));
    }
}

// VectorSumCenter (ivfutils.c:367-378).
fn vector_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i16::from_ne_bytes([v[4], v[5]]) as i32)?;
    for i in 0..dimensions as usize {
        x[i] += f32::from_ne_bytes(v[8 + 4 * i..12 + 4 * i].try_into().unwrap());
    }
    Ok(())
}

// HalfvecSumCenter (ivfutils.c:380-391).
fn halfvec_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i16::from_ne_bytes([v[4], v[5]]) as i32)?;
    for i in 0..dimensions as usize {
        let h = u16::from_ne_bytes([v[8 + 2 * i], v[9 + 2 * i]]);
        x[i] += pgvector::halfutils::half_to_float4(h);
    }
    Ok(())
}

// BitSumCenter (ivfutils.c:393-403).
fn bit_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i32::from_ne_bytes(v[4..8].try_into().unwrap()))?;
    for i in 0..dimensions as usize {
        x[i] += ((v[8 + i / 8] >> (7 - (i % 8))) & 0x01) as f32;
    }
    Ok(())
}

// IvfflatGetTypeInfo's default, no proc 5 (ivfutils.c:415-422): the vector
// opclasses.
pub static VECTOR_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM,
    dimensions: pgvector::funcs::fc_vector_dims,
    normalize: Some(pgvector::funcs::fc_l2_normalize),
    item_size: vector_item_size,
    update_center: vector_update_center,
    sum_center: vector_sum_center,
};

// ivfflat_halfvec_support (ivfutils.c:430-444).
pub static HALFVEC_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM * 2,
    dimensions: pgvector::halfvec::fc_halfvec_vector_dims,
    normalize: Some(pgvector::halfvec::fc_halfvec_l2_normalize),
    item_size: halfvec_item_size,
    update_center: halfvec_update_center,
    sum_center: halfvec_sum_center,
};

// ivfflat_bit_support (ivfutils.c:446-460): dimensions is varbit.c's
// bitlength.
pub static BIT_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM * 32,
    dimensions: adt_varbit::fc_bitlength,
    normalize: None,
    item_size: bit_item_size,
    update_center: bit_update_center,
    sum_center: bit_sum_center,
};

// The type info a support proc 5 names, by its C symbol. C calls the proc and
// gets a pointer to its static IvfflatTypeInfo; pgrust maps the symbol to the
// same table (spec §6), so the ivfflat_*_support functions are never called
// through fmgr.
pub fn ivfflat_type_info_for_symbol(symbol: &str) -> Option<&'static IvfflatTypeInfo> {
    Some(match symbol {
        "ivfflat_halfvec_support" => &HALFVEC_TYPE_INFO,
        "ivfflat_bit_support" => &BIT_TYPE_INFO,
        _ => return None,
    })
}

// IvfflatGetTypeInfo (ivfutils.c:405-428). The symbol is a LANGUAGE c proc's
// pg_proc.prosrc (pgvector::support_proc_symbol).
// DIVERGENCE: C calls any proc 5 and trusts the pointer it returns; pgrust
// knows only pgvector's two support functions and raises otherwise.
pub fn ivfflat_get_type_info(index: &Relation<'_>) -> PgResult<&'static IvfflatTypeInfo> {
    let procid = index_getprocid(index, IVFFLAT_TYPE_INFO_PROC);
    if procid == InvalidOid {
        return Ok(&VECTOR_TYPE_INFO);
    }
    let symbol = pgvector::support_proc_symbol(procid)?.unwrap_or_default();
    ivfflat_type_info_for_symbol(&symbol).ok_or_else(|| {
        PgError::error(format!(
            "ivfflat type support function \"{symbol}\" of index \"{}\" is not supported",
            index.name()
        ))
        .with_sqlstate(types_error::ERRCODE_FEATURE_NOT_SUPPORTED)
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use pgvector::halfutils::{float4_to_half_unchecked, half_to_float4};
    use pgvector::halfvec::{HalfBuilder, HalfView};
    use pgvector::vec::{VecBuilder, VecView};

    // A bit(len) varlena image: 4-byte header, i32 bit length, the bytes.
    fn bit_image(bits: &[u8], len: i32) -> Vec<u8> {
        let mut img = vec![0u8; 8 + bits.len()];
        let hdr = ((img.len() as u32) << 2).to_ne_bytes();
        img[0..4].copy_from_slice(&hdr);
        img[4..8].copy_from_slice(&len.to_ne_bytes());
        img[8..].copy_from_slice(bits);
        img
    }

    // IvfflatGetTypeInfo's tables (ivfutils.c:405-460). Only pgvector's two
    // support symbols resolve (Review Focus 5).
    #[test]
    fn ivf_type_info_resolves_only_pgvector_symbols() {
        assert_eq!(VECTOR_TYPE_INFO.max_dimensions, 2000);
        assert!(VECTOR_TYPE_INFO.normalize.is_some());
        let h = ivfflat_type_info_for_symbol("ivfflat_halfvec_support").unwrap();
        assert!(core::ptr::eq(h, &HALFVEC_TYPE_INFO));
        assert_eq!(h.max_dimensions, 4000);
        assert!(h.normalize.is_some());
        let b = ivfflat_type_info_for_symbol("ivfflat_bit_support").unwrap();
        assert!(core::ptr::eq(b, &BIT_TYPE_INFO));
        assert_eq!(b.max_dimensions, 64000);
        assert!(b.normalize.is_none());
        for other in ["ivfflathandler", "hnsw_halfvec_support", "ivfflat_sparsevec_support", "textin", ""] {
            assert!(ivfflat_type_info_for_symbol(other).is_none(), "{other}");
        }
    }

    // VECTOR_SIZE, HALFVEC_SIZE, VARBITTOTALLEN, VECTOR_ARRAY_SIZE and
    // IVFFLAT_LIST_SIZE.
    #[test]
    fn ivf_item_sizes_match_c_macros() {
        assert_eq!((VECTOR_TYPE_INFO.item_size)(3), 20);
        assert_eq!((VECTOR_TYPE_INFO.item_size)(2000), 8008);
        assert_eq!((HALFVEC_TYPE_INFO.item_size)(3), 14);
        assert_eq!((BIT_TYPE_INFO.item_size)(64), 16);
        assert_eq!((BIT_TYPE_INFO.item_size)(65), 17);
        assert_eq!(vector_array_size(100, 8008).unwrap(), 32 + 100 * 8008);
        assert_eq!(vector_array_size(10, 20).unwrap(), 32 + 10 * 24);
        assert_eq!(maxalign(ivfflat_list_size(maxalign(20))), 32);
    }

    // The center images UpdateCenter writes (ivfutils.c:327-365).
    #[test]
    fn ivf_update_center_writes_c_images() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let xs = [1.5f32, -2.0, 0.25];
        let mut v = vec![0u8; 24];
        (VECTOR_TYPE_INFO.update_center)(&mut v, 3, &xs);
        let mut want = VecBuilder::new(m, 3).unwrap();
        for (i, x) in xs.iter().enumerate() {
            want.set(i, *x);
        }
        assert_eq!(&v[..20], &want.image()[..]);

        let hs = [1.0f32, 1024.5, 0.1];
        let mut h = vec![0u8; 16];
        (HALFVEC_TYPE_INFO.update_center)(&mut h, 3, &hs);
        let mut want = HalfBuilder::new(m, 3).unwrap();
        for (i, x) in hs.iter().enumerate() {
            want.set(i, float4_to_half_unchecked(*x));
        }
        assert_eq!(&h[..14], &want.image()[..]);
        // 1024.5 rounds to the half 1024 (round to nearest even).
        assert_eq!(half_to_float4(HalfView::from_payload(&h[4..14]).unwrap().x(1)), 1024.0);

        // 0.5 is not above 0.5; bits fill from the most significant end.
        let mut b = vec![0xFFu8; 16];
        (BIT_TYPE_INFO.update_center)(&mut b, 10, &[0.6, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.51, 1.0, 0.4]);
        assert_eq!(&b[..10], &bit_image(&[0b1001_0001, 0b1000_0000], 10)[..]);
    }

    // SumCenter (ivfutils.c:367-403) adds each element after checking the
    // dimensions (IvfflatCheckExpectedDim, ivfutils.c:289).
    #[test]
    fn ivf_sum_center_accumulates_and_checks_dims() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut v = VecBuilder::new(m, 2).unwrap();
        v.set(0, 1.5);
        v.set(1, -1.0);
        let img = v.image();
        let mut x = [0.25f32, 0.0];
        (VECTOR_TYPE_INFO.sum_center)(&img, 2, &mut x).unwrap();
        assert_eq!(x, [1.75, -1.0]);
        let e = (VECTOR_TYPE_INFO.sum_center)(&img, 3, &mut [0.0; 3]).unwrap_err();
        assert_eq!(e.message(), "expected 3 dimensions, not 2");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);

        let mut h = HalfBuilder::new(m, 2).unwrap();
        h.set(0, float4_to_half_unchecked(0.5));
        h.set(1, float4_to_half_unchecked(2.0));
        let mut x = [1.0f32, 1.0];
        (HALFVEC_TYPE_INFO.sum_center)(&h.image(), 2, &mut x).unwrap();
        assert_eq!(x, [1.5, 3.0]);

        let b = bit_image(&[0b1010_0000], 3);
        let mut x = [0.0f32; 3];
        (BIT_TYPE_INFO.sum_center)(&b, 3, &mut x).unwrap();
        assert_eq!(x, [1.0, 0.0, 1.0]);
        assert!((BIT_TYPE_INFO.sum_center)(&b, 4, &mut [0.0; 4]).is_err());
    }

    // VectorArrayInit, VectorArrayGet and VectorArraySet (ivfutils.c:24-43,
    // ivfflat.h:323-343).
    #[test]
    fn ivf_vector_array_bounds_and_sizes() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        assert_eq!(VectorArray::init(m, 0, 3, 20).err().unwrap().message(), "cannot create vector array");
        let mut arr = VectorArray::init(m, 2, 3, 20).unwrap();
        assert_eq!(arr.itemsize, 24);
        assert!(arr.get(1).unwrap().iter().all(|&b| b == 0));
        let mut v = VecBuilder::new(m, 3).unwrap();
        v.set(2, 7.0);
        let img = v.image();
        arr.set(1, &img).unwrap();
        assert_eq!(&arr.get(1).unwrap()[..20], &img[..]);
        assert_eq!(arr.get(2).err().unwrap().message(), "index out of bounds");
        assert_eq!(arr.get(-1).err().unwrap().message(), "index out of bounds");
        let big = VecBuilder::new(m, 5).unwrap().image();
        assert_eq!(arr.set(0, &big).err().unwrap().message(), "item too large");
    }

    // IvfflatCheckExpectedDim (ivfutils.c:283-290) through each type's
    // dimensions function.
    #[test]
    fn ivf_check_dim_matches_c() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let vimg = VecBuilder::new(m, 2).unwrap().image();
        let himg = HalfBuilder::new(m, 2).unwrap().image();
        let bimg = bit_image(&[0b1100_0000], 2);
        let typed: [(&IvfflatTypeInfo, &[u8]); 3] =
            [(&VECTOR_TYPE_INFO, &vimg[..]), (&HALFVEC_TYPE_INFO, &himg[..]), (&BIT_TYPE_INFO, &bimg[..])];
        for (info, img) in typed {
            let d = Datum::from_usize(img.as_ptr() as usize);
            assert!(ivfflat_check_dim(2, info, InvalidOid, d).is_ok());
            let e = ivfflat_check_dim(3, info, InvalidOid, d).unwrap_err();
            assert_eq!(e.message(), "expected 3 dimensions, not 2");
            assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);
        }
    }

    // IvfflatCheckMemoryUsage (ivfutils.c:125-134): kilobytes against
    // maintenance_work_mem, the message rounding up (Review Focus 4).
    #[test]
    fn ivf_memory_usage_error_matches_c() {
        assert!(check_memory_usage(1024 * 1024, 1024).is_ok());
        assert!(check_memory_usage(1024 * 1024 + 1023, 1024).is_ok());
        let e = check_memory_usage(1024 * 1024 + 1024, 1024).unwrap_err();
        assert_eq!(e.message(), "memory required is 2 MB, maintenance_work_mem is 1 MB");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED);
        // ivfflat-errors.sql: lists = 1000 on vector(2000) at 1 MB, as C.
        let e = check_memory_usage(vector_array_size(1000, 8008).unwrap(), 1024).unwrap_err();
        assert_eq!(e.message(), "memory required is 8 MB, maintenance_work_mem is 1 MB");
    }

    // IvfflatNormValue (ivfutils.c:84-88): each type's l2_normalize; bit has
    // none.
    #[test]
    fn ivf_norm_value_dispatches_by_type() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut v = VecBuilder::new(m, 2).unwrap();
        v.set(0, 3.0);
        v.set(1, 4.0);
        let out = ivfflat_norm_value(&VECTOR_TYPE_INFO, InvalidOid, m, &v.image()).unwrap();
        let view = VecView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (0.6, 0.8));

        let mut h = HalfBuilder::new(m, 2).unwrap();
        h.set(0, float4_to_half_unchecked(3.0));
        h.set(1, float4_to_half_unchecked(4.0));
        let out = ivfflat_norm_value(&HALFVEC_TYPE_INFO, InvalidOid, m, &h.image()).unwrap();
        let view = HalfView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (float4_to_half_unchecked(0.6), float4_to_half_unchecked(0.8)));

        let e = ivfflat_norm_value(&BIT_TYPE_INFO, InvalidOid, m, &bit_image(&[0x80], 1)).unwrap_err();
        assert_eq!(e.message(), "ivfflat type info has no normalize function");
    }
}

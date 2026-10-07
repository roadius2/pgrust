//! ivfflat.h vocabulary (pgvector 0.8.7): constants, the page layout, the
//! type info an opclass's support proc 5 names, and the scan state. The scan
//! state lives here so relscan's IndexScanOpaque can hold it without a cycle
//! through the AM crates (types_hnsw's pattern).

use pairingheap::PairingHeap;
use types_core::{BlockNumber, Oid, BLCKSZ};
use types_error::PgResult;
use types_fmgr::{FmgrInfo, PGFunction};
use types_storage::buf::BufferAccessStrategy;

pub const IVFFLAT_MAX_DIM: i32 = 2000;

// Support functions (ivfflat.h:40-44).
pub const IVFFLAT_DISTANCE_PROC: u16 = 1;
pub const IVFFLAT_NORM_PROC: u16 = 2;
pub const IVFFLAT_KMEANS_DISTANCE_PROC: u16 = 3;
pub const IVFFLAT_KMEANS_NORM_PROC: u16 = 4;
pub const IVFFLAT_TYPE_INFO_PROC: u16 = 5;

pub const IVFFLAT_VERSION: u32 = 1;
pub const IVFFLAT_MAGIC_NUMBER: u32 = 0x14FF1A7;
pub const IVFFLAT_PAGE_ID: u16 = 0xFF84;

// Preserved page numbers (ivfflat.h:51-52).
pub const IVFFLAT_METAPAGE_BLKNO: BlockNumber = 0;
pub const IVFFLAT_HEAD_BLKNO: BlockNumber = 1;

// IVFFlat parameters (ivfflat.h:55-58).
pub const IVFFLAT_DEFAULT_LISTS: i32 = 100;
pub const IVFFLAT_MIN_LISTS: i32 = 1;
pub const IVFFLAT_MAX_LISTS: i32 = 32768;
pub const IVFFLAT_DEFAULT_PROBES: i32 = 1;

// Build phases (ivfflat.h:60-64); 1 is PROGRESS_CREATEIDX_SUBPHASE_INITIALIZE.
pub const PROGRESS_IVFFLAT_PHASE_KMEANS: i64 = 2;
pub const PROGRESS_IVFFLAT_PHASE_ASSIGN: i64 = 3;
pub const PROGRESS_IVFFLAT_PHASE_LOAD: i64 = 4;

// IvfflatIterativeScanMode (ivfflat.h:114-118).
pub const IVFFLAT_ITERATIVE_SCAN_OFF: i32 = 0;
pub const IVFFLAT_ITERATIVE_SCAN_RELAXED: i32 = 1;

// ---- page layout (ivfflat.h:252-276) ----

// PageGetContents: MAXALIGN(SizeOfPageHeaderData).
pub const PAGE_CONTENTS_OFFSET: usize = 24;
// sizeof(IvfflatPageOpaqueData), the page's special space.
pub const IVFFLAT_PAGE_OPAQUE_SIZE: usize = 8;
// sizeof(IvfflatMetaPageData).
pub const IVFFLAT_METAPAGE_SIZE: usize = 12;
// offsetof(IvfflatListData, center).
pub const IVFFLAT_LIST_CENTER_OFFSET: usize = 8;
// FirstOffsetNumber (off.h).
pub const FIRST_OFFSET_NUMBER: u16 = 1;

pub const fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

// IVFFLAT_LIST_SIZE (ivfflat.h:66).
pub const fn ivfflat_list_size(itemsize: usize) -> usize {
    IVFFLAT_LIST_CENTER_OFFSET + itemsize
}

// IvfflatMetaPageData (ivfflat.h:252-258), at PageGetContents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IvfflatMetaPage {
    pub magic_number: u32,
    pub version: u32,
    pub dimensions: u16,
    pub lists: u16,
}

impl IvfflatMetaPage {
    pub fn read(page: &[u8]) -> IvfflatMetaPage {
        let c = &page[PAGE_CONTENTS_OFFSET..PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE];
        IvfflatMetaPage {
            magic_number: u32::from_ne_bytes(c[0..4].try_into().unwrap()),
            version: u32::from_ne_bytes(c[4..8].try_into().unwrap()),
            dimensions: u16::from_ne_bytes(c[8..10].try_into().unwrap()),
            lists: u16::from_ne_bytes(c[10..12].try_into().unwrap()),
        }
    }

    pub fn write(&self, page: &mut [u8]) {
        let c = &mut page[PAGE_CONTENTS_OFFSET..PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE];
        c[0..4].copy_from_slice(&self.magic_number.to_ne_bytes());
        c[4..8].copy_from_slice(&self.version.to_ne_bytes());
        c[8..10].copy_from_slice(&self.dimensions.to_ne_bytes());
        c[10..12].copy_from_slice(&self.lists.to_ne_bytes());
    }
}

// IvfflatPageOpaqueData (ivfflat.h:262-267): nextblkno, unused, page_id, in
// the page's last 8 bytes.
pub fn page_nextblkno(page: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(page[BLCKSZ - 8..BLCKSZ - 4].try_into().unwrap())
}

pub fn set_page_nextblkno(page: &mut [u8], blkno: BlockNumber) {
    page[BLCKSZ - 8..BLCKSZ - 4].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn page_id(page: &[u8]) -> u16 {
    u16::from_ne_bytes(page[BLCKSZ - 2..BLCKSZ].try_into().unwrap())
}

pub fn set_page_id(page: &mut [u8], id: u16) {
    page[BLCKSZ - 2..BLCKSZ].copy_from_slice(&id.to_ne_bytes());
}

// IvfflatListData (ivfflat.h:271-276): startPage, insertPage, then the
// center, a varlena image of the indexed type.
pub fn list_start_page(item: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(item[0..4].try_into().unwrap())
}

pub fn list_insert_page(item: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(item[4..8].try_into().unwrap())
}

pub fn set_list_start_page(item: &mut [u8], blkno: BlockNumber) {
    item[0..4].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn set_list_insert_page(item: &mut [u8], blkno: BlockNumber) {
    item[4..8].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn list_center(item: &[u8]) -> &[u8] {
    &item[IVFFLAT_LIST_CENTER_OFFSET..]
}

// ListInfo (ivfflat.h:131-135): where a list item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListInfo {
    pub blkno: BlockNumber,
    pub offno: u16,
}

// IvfflatTypeInfo (ivfflat.h:188-196): what an opclass's support proc 5
// returns. `normalize` is None for bit (C's NULL). The center functions work
// on a VectorArray slot (MAXALIGNed, at least item_size(dimensions) bytes)
// and a float accumulator of `dimensions` entries; sum_center raises C's
// dimension mismatch.
pub struct IvfflatTypeInfo {
    pub max_dimensions: i32,
    pub dimensions: PGFunction,
    pub normalize: Option<PGFunction>,
    pub item_size: fn(i32) -> usize,
    pub update_center: fn(&mut [u8], i32, &[f32]),
    pub sum_center: fn(&[u8], i32, &mut [f32]) -> PgResult<()>,
}

// IvfflatScanList (ivfflat.h:280-285), without the embedded heap node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IvfflatScanList {
    pub start_page: BlockNumber,
    pub distance: f64,
}

// CompareLists (ivfscan.c:32-42): pairingheap_first is the furthest list.
pub fn compare_lists(a: &IvfflatScanList, b: &IvfflatScanList) -> i32 {
    if a.distance > b.distance {
        1
    } else if a.distance < b.distance {
        -1
    } else {
        0
    }
}

pub type ScanListHeap =
    PairingHeap<IvfflatScanList, fn(&IvfflatScanList, &IvfflatScanList) -> i32>;

pub fn new_scan_list_heap() -> ScanListHeap {
    ScanListHeap::new(compare_lists)
}

// C IvfflatScanOpaqueData (ivfflat.h:287-315).
pub struct IvfflatScanOpaqueData {
    pub type_info: &'static IvfflatTypeInfo,
    pub probes: i32,
    pub max_probes: i32,
    pub dimensions: i32,
    pub first: bool,
    // so->value: the detoasted (and, with a norm proc, normalized) query
    // image; None for a NULL query (C's PointerGetDatum(NULL)).
    pub value: Option<Vec<u8>>,
    // so->distfunc == ZeroDistance (ivfscan.c:192-196): a NULL query.
    pub zero_distance: bool,
    // Support functions
    pub procinfo: FmgrInfo,
    pub normprocinfo: Option<FmgrInfo>,
    pub collation: Oid,
    pub bas: BufferAccessStrategy,
    // Lists
    pub list_queue: ScanListHeap,
    pub list_pages: Vec<BlockNumber>,
    pub list_index: i32,
    // so->sortstate, with its tuple descriptor and memory context
    // (pgvector_ivfflat::sort::HeapSort). Type-erased: tuplesort depends on
    // relscan, which holds this struct.
    pub sortstate: Option<Box<dyn core::any::Any>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use types_core::InvalidBlockNumber;

    // IvfflatMetaPageData at PageGetContents (ivfflat.h:252-258); the bytes
    // are block 0 of an index built by the seeded C reference.
    #[test]
    fn ivf_metapage_round_trips_c_layout() {
        let mut page = vec![0u8; BLCKSZ];
        let meta = IvfflatMetaPage {
            magic_number: IVFFLAT_MAGIC_NUMBER,
            version: IVFFLAT_VERSION,
            dimensions: 3,
            lists: 10,
        };
        meta.write(&mut page);
        if cfg!(target_endian = "little") {
            assert_eq!(&page[24..36], &[0xA7, 0xF1, 0x4F, 0x01, 1, 0, 0, 0, 3, 0, 10, 0]);
        }
        assert_eq!(IvfflatMetaPage::read(&page), meta);
    }

    // IvfflatPageOpaqueData in the special space, and IvfflatListData's
    // fields (ivfflat.h:262-276).
    #[test]
    fn ivf_list_item_and_opaque_layout_match_c() {
        let mut page = vec![0u8; BLCKSZ];
        set_page_nextblkno(&mut page, 7);
        set_page_id(&mut page, IVFFLAT_PAGE_ID);
        if cfg!(target_endian = "little") {
            assert_eq!(&page[BLCKSZ - 8..], &[7, 0, 0, 0, 0, 0, 0x84, 0xFF]);
        }
        assert_eq!((page_nextblkno(&page), page_id(&page)), (7, IVFFLAT_PAGE_ID));
        let mut item = vec![0u8; ivfflat_list_size(maxalign(20))];
        set_list_start_page(&mut item, 2);
        set_list_insert_page(&mut item, InvalidBlockNumber);
        assert_eq!((list_start_page(&item), list_insert_page(&item)), (2, InvalidBlockNumber));
        assert_eq!(item.len(), 32);
        assert_eq!(list_center(&item).len(), 24);
    }

    // CompareLists (ivfscan.c:32-42): the heap pops the furthest list first.
    #[test]
    fn ivf_compare_lists_pops_furthest_first() {
        let mut h = new_scan_list_heap();
        for (p, d) in [(1, 2.0), (2, 5.0), (3, 1.0)] {
            h.add(IvfflatScanList { start_page: p, distance: d });
        }
        assert_eq!(h.first().unwrap().start_page, 2);
        let order: Vec<BlockNumber> =
            core::iter::from_fn(|| h.remove_first().map(|l| l.start_page)).collect();
        assert_eq!(order, [2, 1, 3]);
    }
}

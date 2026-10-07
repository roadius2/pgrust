//! pgvector 0.8.7 ivfflat AM (ivfflat.c, ivfutils.c, ivfinsert.c,
//! ivfscan.c, ivfvacuum.c) for vector, halfvec and bit; the build
//! (ivfbuild.c, ivfkmeans.c) lives in pgvector_ivfflat_build. Support proc 5
//! resolves by its C symbol to ivfutils.c's static type info
//! (utils.rs ivfflat_get_type_info). Elsewhere: ivfflat.c's cost estimate is
//! planner selfuncs.rs ivfflatcostestimate, its reloptions are reloptions'
//! ivfflatoptions, and its build phase names are adt_amutils'.
//! DIVERGENCES (recorded): no parallel build (M6); an unknown support proc 5
//! raises where C would call it; the scan keeps a detoasted copy of the query
//! where C uses it in place; an insert into a list with no insert page raises
//! where C asserts.

pub mod insert;
pub mod scan;
pub mod sort;
pub mod utils;
pub mod vacuum;

pub use insert::ivfflatinsert;
pub use scan::{ivfflatbeginscan, ivfflatendscan, ivfflatgettuple, ivfflatrescan};
pub use vacuum::{ivfflatbulkdelete, ivfflatbulkdelete_collect, ivfflatvacuumcleanup};

use types_core::Oid;
use types_error::PgResult;

// ivfflatvalidate (ivfflat.c:172-176).
pub fn ivfflatvalidate(_opclassoid: Oid) -> PgResult<bool> {
    Ok(true)
}

use std::cell::Cell;
thread_local! {
    static PROBES: Cell<i32> = const { Cell::new(1) };
    static ITERATIVE_SCAN: Cell<i32> = const { Cell::new(0) };
    static MAX_PROBES: Cell<i32> = const { Cell::new(32768) };
}

// ivfflat.* GUC backing (C's ivfflat_probes &co globals, ivfflat.c:24-26).
pub fn init_seams() {
    use guc_tables::GucVarAccessors;
    guc_tables::vars::ivfflat_probes.install_if_absent(GucVarAccessors {
        get: || PROBES.with(|c| c.get()),
        set: |v| PROBES.with(|c| c.set(v)),
    });
    guc_tables::vars::ivfflat_iterative_scan.install_if_absent(GucVarAccessors {
        get: || ITERATIVE_SCAN.with(|c| c.get()),
        set: |v| ITERATIVE_SCAN.with(|c| c.set(v)),
    });
    guc_tables::vars::ivfflat_max_probes.install_if_absent(GucVarAccessors {
        get: || MAX_PROBES.with(|c| c.get()),
        set: |v| MAX_PROBES.with(|c| c.set(v)),
    });
}

//! The heap tuplesorts of ivfbuild.c (InitBuildSortState, ivfbuild.c:612-621)
//! and ivfscan.c (InitScanSortState, ivfscan.c:241-250): one key on the first
//! column, ascending, nulls last, no coordinate and no random access.

use std::rc::Rc;

use datum::Datum;
use types_core::{InvalidOid, Oid};
use types_error::PgResult;

// Int4LessOperator and Float8LessOperator (pg_operator.dat).
pub const INT4_LESS_OPERATOR: Oid = 97;
pub const FLOAT8_LESS_OPERATOR: Oid = 672;

// A heap tuplesort over a template tuple descriptor (CreateTemplateTupleDesc,
// TupleDescInitEntry, tuplesort_begin_heap) that owns the memory context the
// descriptor lives in. Rows go in and out as Datums (tuplesort_putvalues and
// tuplesort_getvalues), which form the same minimal tuples C's
// tuplesort_puttupleslot copies out of its virtual slot. Dropping it is
// tuplesort_end.
pub struct HeapSort {
    // Field order is drop order: the sort, which holds the descriptor, drops
    // before the context the descriptor lives in.
    sort: tuplesort::Tuplesort,
    natts: usize,
    _cx: Box<mcx::MemoryContext>,
}

impl HeapSort {
    // `columns` are (attribute name, type oid), at most three.
    pub fn begin(
        cx_name: &'static str,
        columns: &[(&str, Oid)],
        sort_operator: Oid,
        work_mem: i32,
    ) -> PgResult<HeapSort> {
        debug_assert!(columns.len() <= 3);
        let cx = Box::new(mcx::MemoryContext::new(cx_name));
        // SAFETY: lifetime erasure only (orderedsetaggs' pattern): the
        // descriptor lives in `cx`, which this struct owns and drops last.
        let m: mcx::Mcx<'static> =
            unsafe { core::mem::transmute::<mcx::Mcx<'_>, mcx::Mcx<'static>>(cx.mcx()) };
        let mut desc = tupdesc::CreateTemplateTupleDesc(m, columns.len() as i32)?;
        for (i, (name, typid)) in columns.iter().enumerate() {
            tupdesc::TupleDescInitEntry(&mut desc, i as i16 + 1, Some(name), *typid, -1, 0)?;
        }
        let sort = tuplesort::Tuplesort::begin_heap(
            Rc::new(desc),
            &[1],
            &[sort_operator],
            &[InvalidOid],
            &[false],
            work_mem,
            tuplesort::TUPLESORT_NONE,
        )?;
        Ok(HeapSort { sort, natts: columns.len(), _cx: cx })
    }

    // tuplesort_puttupleslot of a virtual tuple with no NULLs; the values
    // are copied.
    pub fn put(&mut self, values: &[Datum]) -> PgResult<()> {
        debug_assert_eq!(values.len(), self.natts);
        self.sort.putvalues(values, &[false; 3][..values.len()])
    }

    // tuplesort_performsort.
    pub fn perform(&mut self) -> PgResult<()> {
        self.sort.performsort()
    }

    // tuplesort_gettupleslot(forward, no copy): by-reference values point
    // into the sort's memory until the next call; false once drained.
    pub fn next(&mut self, values: &mut [Datum]) -> PgResult<bool> {
        let mut isnull = [false; 3];
        self.sort.getvalues(true, values, &mut isnull[..values.len()])
    }

    // tuplesort_reset.
    pub fn reset(&mut self) -> PgResult<()> {
        self.sort.reset()
    }
}

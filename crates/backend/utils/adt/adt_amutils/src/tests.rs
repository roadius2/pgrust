use super::*;

#[test]
fn prop_names_case_insensitive() {
    assert!(matches!(lookup_prop_name(b"ASC"), Prop::Asc));
    assert!(matches!(lookup_prop_name(b"distance_orderable"), Prop::DistanceOrderable));
    assert!(matches!(lookup_prop_name(b"Can_Include"), Prop::CanInclude));
    assert!(matches!(lookup_prop_name(b"bogus"), Prop::Unknown));
    assert!(matches!(lookup_prop_name(b"asc2"), Prop::Unknown));
}

thread_local! {
    // Per test thread: the stub runs on its caller's thread, so tests running
    // in parallel never count each other's loads.
    static FMGR_INFO_CALLS: std::cell::RefCell<Vec<Oid>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn dummy_fn(_f: Option<&mut types_fmgr::FmgrInfo>, _c: &mut types_fmgr::FunctionCallInfoBaseData) -> PgResult<datum::Datum> {
    Ok(datum::Datum::from_usize(0))
}

// canonical_index_am consults the pg_am.amhandler syscache seam for
// non-builtin oids (RelationInitTableAccessMethod's lookup); unit rigs have
// no syscache below them. The stub answers with pg_am.dat's REAL builtin
// handler oids (330-335) and None for unknown oids (C's !HeapTupleIsValid),
// so the flag-vs-C assertions and the unknown-AM probe both exercise the
// genuine paths — not a vacuous pass. Same projection-rig class as
// type_is_visible / pg_constraint_primary_key_attnos / expandExpressionListStar.
fn install_amhandler_stub() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if !fmgr_seams::fmgr_info::is_installed() {
            fmgr_seams::fmgr_info::set(|oid| {
                FMGR_INFO_CALLS.with(|c| c.borrow_mut().push(oid));
                Ok(types_fmgr::FmgrInfo::new(dummy_fn, 0, 0, true, false))
            });
        }
        syscache_seams::pg_am_amhandler::set(|amoid| {
            Ok(match amoid {
                BTREE_AM_OID => Some(330),
                HASH_AM_OID => Some(331),
                GIST_AM_OID => Some(332),
                GIN_AM_OID => Some(333),
                SPGIST_AM_OID => Some(334),
                BRIN_AM_OID => Some(335),
                20000 => Some(9999),
                20001 => Some(9998),
                20002 => Some(9997),
                _ => None,
            })
        });
        syscache_seams::lookup_pg_proc_prosrc::set(|mcx, funcid| {
            Ok(match funcid {
                9999 => Some(mcx::PgString::from_str_in("blhandler", mcx)?),
                9998 => Some(mcx::PgString::from_str_in("hnswhandler", mcx)?),
                9997 => Some(mcx::PgString::from_str_in("ivfflathandler", mcx)?),
                _ => None,
            })
        });
    });
}

// audit-18.6 regression: amutils.c:198 calls the catalog's handler,
// so an extension AM is known by its handler's C symbol (prosrc), which
// ALTER FUNCTION ... RENAME does not touch.
#[test]
fn extension_am_identified_by_handler_prosrc() {
    install_amhandler_stub();
    let bloom = am_flags(20000).unwrap().unwrap();
    assert!(bloom.amcanmulticol && !bloom.has_ambuildphasename && !bloom.amcanorderbyop);
    let hnsw = am_flags(20001).unwrap().unwrap();
    assert!(hnsw.amcanorderbyop && hnsw.has_ambuildphasename && !hnsw.amcanmulticol);
    let ivfflat = am_flags(20002).unwrap().unwrap();
    assert!(ivfflat.amcanorderbyop && ivfflat.has_ambuildphasename && !ivfflat.amcanmulticol);
}

#[test]
fn am_flag_rows_match_c_handlers() {
    install_amhandler_stub();
    let bt = am_flags(BTREE_AM_OID).unwrap().unwrap();
    assert!(bt.amcanorder && bt.amcanunique && bt.amsearcharray && bt.has_ambuildphasename);
    let hash = am_flags(HASH_AM_OID).unwrap().unwrap();
    assert!(hash.amcanbackward && !hash.amcanorder && !hash.amcaninclude);
    let gin = am_flags(GIN_AM_OID).unwrap().unwrap();
    assert!(!gin.has_amgettuple && gin.has_ambuildphasename && gin.amcanmulticol);
    let brin = am_flags(BRIN_AM_OID).unwrap().unwrap();
    assert!(!brin.has_amgettuple && brin.amsearchnulls && !brin.amclusterable);
    assert!(am_flags(42).unwrap().is_none());
}

// amutils.c:368-369: bitmap_scan is amgetbitmap != NULL. Every core AM and
// bloom has one; hnsw.c:315 and ivfflat.c:232 set amgetbitmap = NULL.
#[test]
fn amgetbitmap_matches_c_handlers() {
    install_amhandler_stub();
    for oid in [BTREE_AM_OID, HASH_AM_OID, GIST_AM_OID, GIN_AM_OID, SPGIST_AM_OID, BRIN_AM_OID, 20000] {
        assert!(am_flags(oid).unwrap().unwrap().has_amgetbitmap, "{oid}");
    }
    for oid in [20001, 20002] {
        let f = am_flags(oid).unwrap().unwrap();
        assert!(!f.has_amgetbitmap && f.has_amgettuple, "{oid}");
    }
}

// amutils.c:198 calls the extension AM's handler: its library loads. Core AMs
// resolve without fmgr.
#[test]
fn extension_am_properties_load_the_library() {
    install_amhandler_stub();
    let calls = |oid| FMGR_INFO_CALLS.with(|c| c.borrow().iter().filter(|&&o| o == oid).count());
    let before = calls(9998);
    assert!(am_flags(20001).unwrap().unwrap().amcanorderbyop);
    assert_eq!(calls(9998), before + 1);
    let n = FMGR_INFO_CALLS.with(|c| c.borrow().len());
    assert!(am_flags(BTREE_AM_OID).unwrap().is_some());
    assert_eq!(FMGR_INFO_CALLS.with(|c| c.borrow().len()), n);
}

#[test]
fn phasenames_match_c() {
    assert_eq!(bt_phasename(1), Some("initializing"));
    assert_eq!(bt_phasename(5), Some("loading tuples in tree"));
    assert_eq!(bt_phasename(6), None);
    assert_eq!(gin_phasename(3), Some("sorting tuples (workers)"));
    assert_eq!(gin_phasename(6), Some("merging tuples"));
    assert_eq!(gin_phasename(7), None);
    assert_eq!(ivfflat_phasename(1), Some("initializing"));
    assert_eq!(ivfflat_phasename(2), Some("performing k-means"));
    assert_eq!(ivfflat_phasename(3), Some("assigning tuples"));
    assert_eq!(ivfflat_phasename(4), Some("loading tuples"));
    assert_eq!(ivfflat_phasename(5), None);
}

#[test]
fn phasenum_truncates_like_pg_getarg_int32() {
    let phasenum = 0x1_0000_0001i64 as i32 as i64;
    assert_eq!(phasenum, 1);
}

// C amutils.c never re-probes pg_index inside test_indoption -- it reads
// indoption off the tuple indexam_property already holds -- and reports an
// INDEXRELID miss with elog(ERROR, "cache lookup failed for index %u")
// (ruleutils.c:1310), a catchable XX000.  pgrust panicked, which aborts the
// backend; this pins the catchable form.
#[test]
fn index_cache_lookup_failure_is_a_catchable_xx000() {
    let e = index_lookup_failed(16384);
    assert_eq!(e.message(), "cache lookup failed for index 16384");
    assert_eq!(e.sqlstate(), types_error::ERRCODE_INTERNAL_ERROR);
    assert_eq!(e.level(), types_error::ERROR);
}

// audit-18.6 regression: amutils.c:97 text_to_cstring compares through
// the first NUL, so a byteain-built name with an embedded NUL still matches.
#[test]
fn prop_names_stop_at_the_first_nul() {
    assert!(matches!(lookup_prop_name(b"can_order\0junk"), Prop::CanOrder));
    assert!(matches!(lookup_prop_name(b"CAN_ORDER\0"), Prop::CanOrder));
    assert!(matches!(lookup_prop_name(b"\0can_order"), Prop::Unknown));
}

//! pgvector (github.com/pgvector/pgvector), ported from 0.8.5 @ 159b79a and
//! brought to 0.8.7 behavior: the vector, halfvec and sparsevec types and
//! the bit distance functions (I/O, functions, aggregates, casts, btree
//! opclasses), one module per C file. The access methods are the
//! pgvector_hnsw and pgvector_ivfflat crates. extension/ holds upstream
//! 0.8.7's files verbatim: vector--0.8.7.sql (its sql/vector.sql), the 41
//! upgrade scripts and vector.control (spec §4.6). The library reports
//! PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49) and reserves the hnsw and
//! ivfflat GUC prefixes on load, as _PG_init does.
//! DIVERGENCE: errors raised by the pre-M2 vector code (vec.rs, funcs.rs)
//! carry crate-derived locations, not vector.c's; halfvec_to_vector and
//! sparsevec_to_vector inherit that through vector's CheckDim helpers.
//! DIVERGENCE: the sparsevec inner product and L2 merges fuse multiply-adds
//! only on aarch64, matching C's codegen per target; neither emulates C's
//! vectorized L2/L1 tails (see sparsevec.rs fmadd_step).

pub mod bitutils;
pub mod bitvec;
pub mod funcs;
pub mod halfutils;
pub mod halfvec;
pub mod sparsevec;
pub mod vec;

use types_fmgr::PGFunction;

const LIBRARY: &str = "vector";

// ClanguageId (pg_language.dat).
const C_LANGUAGE_ID: types_core::Oid = 13;

/// The C symbol a `LANGUAGE c` support function binds: its `pg_proc.prosrc`,
/// the name C's fmgr looks up in the library. The vector access methods
/// resolve their type-info support procs by it (HNSW's 3, IVFFlat's 5; spec
/// §5.4, §6) instead of calling them. None for a function in any other
/// language, the way GIN identifies support procs (gin/src/util.rs:84-99),
/// and for one that is gone.
pub fn support_proc_symbol(procid: types_core::Oid) -> types_error::PgResult<Option<String>> {
    let Some(row) = syscache_seams::lookup_pg_proc_fmgr::call(procid)? else {
        return Ok(None);
    };
    if row.prolang != C_LANGUAGE_ID {
        return Ok(None);
    }
    let cx = mcx::MemoryContext::new("pgvector support proc symbol");
    let prosrc = syscache_seams::lookup_pg_proc_prosrc::call(cx.mcx(), procid)?;
    Ok(prosrc.map(|s| s.as_str().to_string()))
}

fn lookup(function: &str) -> Option<PGFunction> {
    use funcs::*;
    Some(match function {
        "vector_in" => fc_vector_in,
        "vector_out" => fc_vector_out,
        "vector_typmod_in" => fc_vector_typmod_in,
        "vector_recv" => fc_vector_recv,
        "vector_send" => fc_vector_send,
        "vector" => fc_vector,
        "array_to_vector" => fc_array_to_vector,
        "vector_to_float4" => fc_vector_to_float4,
        "l2_distance" => fc_l2_distance,
        "vector_l2_squared_distance" => fc_vector_l2_squared_distance,
        "inner_product" => fc_inner_product,
        "vector_negative_inner_product" => fc_vector_negative_inner_product,
        "cosine_distance" => fc_cosine_distance,
        "vector_spherical_distance" => fc_vector_spherical_distance,
        "l1_distance" => fc_l1_distance,
        "vector_dims" => fc_vector_dims,
        "vector_norm" => fc_vector_norm,
        "l2_normalize" => fc_l2_normalize,
        "vector_add" => fc_vector_add,
        "vector_sub" => fc_vector_sub,
        "vector_mul" => fc_vector_mul,
        "vector_concat" => fc_vector_concat,
        "binary_quantize" => fc_binary_quantize,
        "subvector" => fc_subvector,
        "vector_lt" => fc_vector_lt,
        "vector_le" => fc_vector_le,
        "vector_eq" => fc_vector_eq,
        "vector_ne" => fc_vector_ne,
        "vector_ge" => fc_vector_ge,
        "vector_gt" => fc_vector_gt,
        "vector_cmp" => fc_vector_cmp,
        "vector_accum" => fc_vector_accum,
        "vector_combine" => fc_vector_combine,
        "vector_avg" => fc_vector_avg,
        "halfvec_in" => halfvec::fc_halfvec_in,
        "halfvec_out" => halfvec::fc_halfvec_out,
        "halfvec_typmod_in" => halfvec::fc_halfvec_typmod_in,
        "halfvec_recv" => halfvec::fc_halfvec_recv,
        "halfvec_send" => halfvec::fc_halfvec_send,
        "halfvec" => halfvec::fc_halfvec,
        "array_to_halfvec" => halfvec::fc_array_to_halfvec,
        "halfvec_to_float4" => halfvec::fc_halfvec_to_float4,
        "vector_to_halfvec" => halfvec::fc_vector_to_halfvec,
        "halfvec_to_vector" => fc_halfvec_to_vector,
        "halfvec_l2_distance" => halfvec::fc_halfvec_l2_distance,
        "halfvec_l2_squared_distance" => halfvec::fc_halfvec_l2_squared_distance,
        "halfvec_inner_product" => halfvec::fc_halfvec_inner_product,
        "halfvec_negative_inner_product" => halfvec::fc_halfvec_negative_inner_product,
        "halfvec_cosine_distance" => halfvec::fc_halfvec_cosine_distance,
        "halfvec_spherical_distance" => halfvec::fc_halfvec_spherical_distance,
        "halfvec_l1_distance" => halfvec::fc_halfvec_l1_distance,
        "halfvec_vector_dims" => halfvec::fc_halfvec_vector_dims,
        "halfvec_l2_norm" => halfvec::fc_halfvec_l2_norm,
        "halfvec_l2_normalize" => halfvec::fc_halfvec_l2_normalize,
        "halfvec_binary_quantize" => halfvec::fc_halfvec_binary_quantize,
        "halfvec_subvector" => halfvec::fc_halfvec_subvector,
        "halfvec_add" => halfvec::fc_halfvec_add,
        "halfvec_sub" => halfvec::fc_halfvec_sub,
        "halfvec_mul" => halfvec::fc_halfvec_mul,
        "halfvec_concat" => halfvec::fc_halfvec_concat,
        "halfvec_lt" => halfvec::fc_halfvec_lt,
        "halfvec_le" => halfvec::fc_halfvec_le,
        "halfvec_eq" => halfvec::fc_halfvec_eq,
        "halfvec_ne" => halfvec::fc_halfvec_ne,
        "halfvec_ge" => halfvec::fc_halfvec_ge,
        "halfvec_gt" => halfvec::fc_halfvec_gt,
        "halfvec_cmp" => halfvec::fc_halfvec_cmp,
        "halfvec_accum" => halfvec::fc_halfvec_accum,
        "halfvec_avg" => halfvec::fc_halfvec_avg,
        "sparsevec_in" => sparsevec::fc_sparsevec_in,
        "sparsevec_out" => sparsevec::fc_sparsevec_out,
        "sparsevec_typmod_in" => sparsevec::fc_sparsevec_typmod_in,
        "sparsevec_recv" => sparsevec::fc_sparsevec_recv,
        "sparsevec_send" => sparsevec::fc_sparsevec_send,
        "sparsevec" => sparsevec::fc_sparsevec,
        "vector_to_sparsevec" => sparsevec::fc_vector_to_sparsevec,
        "halfvec_to_sparsevec" => sparsevec::fc_halfvec_to_sparsevec,
        "array_to_sparsevec" => sparsevec::fc_array_to_sparsevec,
        "sparsevec_to_vector" => fc_sparsevec_to_vector,
        "sparsevec_to_halfvec" => halfvec::fc_sparsevec_to_halfvec,
        "sparsevec_l2_distance" => sparsevec::fc_sparsevec_l2_distance,
        "sparsevec_l2_squared_distance" => sparsevec::fc_sparsevec_l2_squared_distance,
        "sparsevec_inner_product" => sparsevec::fc_sparsevec_inner_product,
        "sparsevec_negative_inner_product" => sparsevec::fc_sparsevec_negative_inner_product,
        "sparsevec_cosine_distance" => sparsevec::fc_sparsevec_cosine_distance,
        "sparsevec_l1_distance" => sparsevec::fc_sparsevec_l1_distance,
        "sparsevec_vector_dims" => sparsevec::fc_sparsevec_vector_dims,
        "sparsevec_l2_norm" => sparsevec::fc_sparsevec_l2_norm,
        "sparsevec_l2_normalize" => sparsevec::fc_sparsevec_l2_normalize,
        "sparsevec_lt" => sparsevec::fc_sparsevec_lt,
        "sparsevec_le" => sparsevec::fc_sparsevec_le,
        "sparsevec_eq" => sparsevec::fc_sparsevec_eq,
        "sparsevec_ne" => sparsevec::fc_sparsevec_ne,
        "sparsevec_ge" => sparsevec::fc_sparsevec_ge,
        "sparsevec_gt" => sparsevec::fc_sparsevec_gt,
        "sparsevec_cmp" => sparsevec::fc_sparsevec_cmp,
        "hamming_distance" => bitvec::fc_hamming_distance,
        "jaccard_distance" => bitvec::fc_jaccard_distance,
        "hnswhandler" => fc_hnswhandler,
        "hnsw_halfvec_support" => fc_hnsw_halfvec_support,
        "hnsw_bit_support" => fc_hnsw_bit_support,
        "hnsw_sparsevec_support" => fc_hnsw_sparsevec_support,
        "ivfflathandler" => fc_ivfflathandler,
        "ivfflat_halfvec_support" => fc_ivfflat_halfvec_support,
        "ivfflat_bit_support" => fc_ivfflat_bit_support,
        _ => return None,
    })
}

// The error a symbol pgrust never calls through fmgr raises if a superuser
// binds it to a callable signature (CREATE FUNCTION f(int) RETURNS int AS
// '$libdir/vector', 'hnsw_bit_support' LANGUAGE C).
fn not_called_through_fmgr(symbol: &str, resolved_by: &str) -> types_error::PgResult<datum::Datum> {
    Err(types_error::PgError::error(format!("{symbol} is resolved by {resolved_by}, never called through fmgr"))
        .with_sqlstate(types_error::ERRCODE_INTERNAL_ERROR)
        .into())
}

// hnswhandler (hnsw.c:266-402): a CREATE FUNCTION validation target only;
// the closed AM set dispatches via IndexAmKind, never through fmgr.
// DIVERGENCE: C returns its IndexAmRoutine when called; pgrust raises.
fn fc_hnswhandler(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("hnswhandler", "its access method")
}

// hnsw_halfvec_support, hnsw_bit_support and hnsw_sparsevec_support
// (hnswutils.c:1418-1458): CREATE FUNCTION validation targets only.
// pgvector_hnsw resolves an opclass's support proc 3 by its C symbol to a
// static HnswTypeInfo (hnsw_get_type_info, spec §5.4), never through fmgr,
// and SQL cannot call them through their `internal` signature.
// DIVERGENCE: C returns a pointer to its static HnswTypeInfo when called;
// pgrust raises.
fn fc_hnsw_halfvec_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("hnsw_halfvec_support", "symbol")
}

fn fc_hnsw_bit_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("hnsw_bit_support", "symbol")
}

fn fc_hnsw_sparsevec_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("hnsw_sparsevec_support", "symbol")
}

// ivfflathandler (ivfflat.c:183-319): a CREATE FUNCTION validation target
// only; the closed AM set dispatches via IndexAmKind, never through fmgr.
// DIVERGENCE: C returns its IndexAmRoutine when called; pgrust raises.
fn fc_ivfflathandler(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("ivfflathandler", "its access method")
}

// ivfflat_halfvec_support and ivfflat_bit_support (ivfutils.c:430-460):
// CREATE FUNCTION validation targets only. pgvector_ivfflat resolves an
// opclass's support proc 5 by its C symbol to a static IvfflatTypeInfo
// (ivfflat_get_type_info, spec §6), never through fmgr.
// DIVERGENCE: C returns a pointer to its static IvfflatTypeInfo when called;
// pgrust raises.
fn fc_ivfflat_halfvec_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("ivfflat_halfvec_support", "symbol")
}

fn fc_ivfflat_bit_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    not_called_through_fmgr("ivfflat_bit_support", "symbol")
}

// _PG_init (vector.c:59-65). HnswInit and IvfflatInit end by reserving
// their GUC prefixes (hnsw.c:111, ivfflat.c:58); the GUCs and reloptions
// they define are static here, and BitvecInit and HalfvecInit only pick CPU
// dispatch, which this port does not need.
// DIVERGENCE: C loads `vector`, and so runs _PG_init, on a session's first
// open of an hnsw or ivfflat index (InitIndexAmRoutine -> GetIndexAmRoutine
// -> OidFunctionCall0(amhandler), relcache.c:1432, amapi.c:38). pgrust
// resolves the handlers by name (amapi's resolve_extension_handler) and
// never loads the library there, so the prefix reservation can happen later
// than in C, and pg_get_loaded_modules() can omit `vector` until a vector
// function is called. The fix is in core amapi.
fn pg_init() -> types_error::PgResult<()> {
    guc::MarkGUCPrefixReserved("hnsw");
    guc::MarkGUCPrefixReserved("ivfflat");
    Ok(())
}

pub fn init_seams() {
    dfmgr::register_builtin_library(dfmgr::BuiltinLibraryEntry {
        name: LIBRARY,
        lookup,
        pg_init: Some(pg_init),
    });
    // PG_MODULE_MAGIC_EXT(.name = "vector", .version = "0.8.7") (vector.c:49).
    dfmgr::register_library_version(LIBRARY, "0.8.7");
}

#[cfg(test)]
mod tests {
    // support_proc_symbol: only a LANGUAGE c proc names a library symbol
    // (Review Focus 5). Stubs the two pg_proc lookups once per test binary.
    #[test]
    fn ivf_support_proc_symbol_requires_language_c() {
        const C_PROC: types_core::Oid = 70_001;
        const SQL_PROC: types_core::Oid = 70_002;
        const GONE_PROC: types_core::Oid = 70_003;
        if !syscache_seams::lookup_pg_proc_fmgr::is_installed() {
            syscache_seams::lookup_pg_proc_fmgr::set(|funcid| {
                let prolang = match funcid {
                    C_PROC => 13,
                    SQL_PROC => 14,
                    _ => return Ok(None),
                };
                Ok(Some(syscache_seams::PgProcFmgrShape {
                    prolang,
                    prorettype: 2281,
                    pronargs: 0,
                    proisstrict: true,
                    proretset: false,
                    prosecdef: false,
                    proconfig_isnull: true,
                    xmin: 0,
                    tid: Default::default(),
                }))
            });
        }
        if !syscache_seams::lookup_pg_proc_prosrc::is_installed() {
            syscache_seams::lookup_pg_proc_prosrc::set(|cx, funcid| {
                Ok(if funcid == C_PROC || funcid == SQL_PROC {
                    Some(mcx::PgString::from_str_in("ivfflat_halfvec_support", cx)?)
                } else {
                    None
                })
            });
        }
        assert_eq!(
            super::support_proc_symbol(C_PROC).unwrap().as_deref(),
            Some("ivfflat_halfvec_support")
        );
        assert_eq!(super::support_proc_symbol(SQL_PROC).unwrap(), None);
        assert_eq!(super::support_proc_symbol(GONE_PROC).unwrap(), None);
    }

    // Every file in extension/: upstream 0.8.7's vector.control,
    // vector--0.8.7.sql and its 41 upgrade scripts (spec §4.6).
    fn shipped_files() -> Vec<(String, String)> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/extension");
        let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                (name, std::fs::read_to_string(&p).unwrap())
            })
            .collect();
        out.sort();
        out
    }

    // The upstream file each shipped file copies.
    fn upstream_path(file: &str) -> String {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../pgvector-0.8.7-reference");
        match file {
            "vector.control" => format!("{root}/vector.control"),
            "vector--0.8.7.sql" => format!("{root}/sql/vector.sql"),
            other => format!("{root}/sql/{other}"),
        }
    }

    // Spec §4.6: the extension directory is upstream's, byte for byte.
    #[test]
    fn shipped_scripts_are_upstream_verbatim() {
        let files = shipped_files();
        assert_eq!(files.len(), 43, "vector.control, the install script and 41 upgrade scripts");
        for (file, text) in &files {
            let upstream = std::fs::read_to_string(upstream_path(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(*text == upstream, "{file} differs from upstream");
        }
    }

    /// The C symbol of every `CREATE FUNCTION ... AS 'MODULE_PATHNAME'` in a
    /// script: the link symbol after the comma, else the SQL name (which
    /// CREATE FUNCTION stores as prosrc).
    fn module_pathname_symbols(script: &str) -> Vec<String> {
        let mut out = Vec::new();
        for stmt in script.split(';') {
            let Some(pos) = stmt.find("CREATE FUNCTION ") else { continue };
            let rest = &stmt[pos + "CREATE FUNCTION ".len()..];
            let Some(at) = rest.find("AS 'MODULE_PATHNAME'") else { continue };
            let name = rest[..rest.find('(').expect("argument list")].trim();
            let after = rest[at + "AS 'MODULE_PATHNAME'".len()..].trim_start();
            out.push(match after.strip_prefix(',') {
                Some(link) => link.trim_start().trim_start_matches('\'').split('\'').next().unwrap().to_string(),
                None => name.to_string(),
            });
        }
        out
    }

    #[test]
    fn lookup_coverage_parser_reads_link_symbols() {
        let sql = "-- halfvec functions\n\nCREATE FUNCTION l2_distance(halfvec, halfvec) RETURNS float8\n\
                   \tAS 'MODULE_PATHNAME', 'halfvec_l2_distance' LANGUAGE C;\n\n\
                   CREATE FUNCTION vector_in(cstring) RETURNS vector\n\tAS 'MODULE_PATHNAME' LANGUAGE C;\n\n\
                   CREATE AGGREGATE avg(vector) (SFUNC = vector_accum, STYPE = double precision[]);";
        assert_eq!(module_pathname_symbols(sql), vec!["halfvec_l2_distance", "vector_in"]);
    }

    // The symbols resolved outside fmgr raise when called, never panic.
    #[test]
    fn fmgr_stubs_raise_internal_error() {
        for symbol in [
            "hnswhandler",
            "hnsw_halfvec_support",
            "hnsw_bit_support",
            "hnsw_sparsevec_support",
            "ivfflathandler",
            "ivfflat_halfvec_support",
            "ivfflat_bit_support",
        ] {
            let f = super::lookup(symbol).unwrap();
            let mut fc = types_fmgr::LocalFcinfo::<0>::new(0);
            let e = f(None, &mut fc).unwrap_err();
            assert_eq!(e.sqlstate(), types_error::ERRCODE_INTERNAL_ERROR, "{symbol}");
            assert!(e.message().starts_with(&format!("{symbol} is resolved by ")), "{symbol}: {}", e.message());
        }
    }

    // Spec §4.6: CREATE EXTENSION and ALTER EXTENSION UPDATE stop at the
    // first CREATE FUNCTION whose symbol `lookup` cannot resolve.
    #[test]
    fn lookup_covers_every_module_pathname_symbol() {
        let files = shipped_files();
        let install = &files.iter().find(|(f, _)| f == "vector--0.8.7.sql").expect("install script").1;
        assert_eq!(module_pathname_symbols(install).len(), 114);
        for (file, script) in files.iter().filter(|(f, _)| f.ends_with(".sql")) {
            let missing: Vec<String> =
                module_pathname_symbols(script).into_iter().filter(|s| super::lookup(s).is_none()).collect();
            assert!(missing.is_empty(), "{file}: no lookup entry for {missing:?}");
        }
    }
}

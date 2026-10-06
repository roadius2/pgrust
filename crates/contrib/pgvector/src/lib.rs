//! pgvector (github.com/pgvector/pgvector), ported from 0.8.5 @ 159b79a and
//! brought to 0.8.7 behavior for the vector type: distance/arithmetic
//! functions and aggregates. halfvec/sparsevec/bit and ivfflat are unported;
//! the shipped extension script is still the trimmed vector--0.8.5.sql until
//! M4 (spec §4.6). DIVERGENCE: pg_get_loaded_modules() reports 18.6 for this
//! library; C reports PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49). Revisit in M4.

pub mod bitutils;
pub mod bitvec;
pub mod funcs;
pub mod halfutils;
pub mod vec;

use types_fmgr::PGFunction;

const LIBRARY: &str = "vector";

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
        "hamming_distance" => bitvec::fc_hamming_distance,
        "jaccard_distance" => bitvec::fc_jaccard_distance,
        "hnswhandler" => fc_hnswhandler,
        _ => return None,
    })
}

// CREATE FUNCTION validation target only; the closed AM set dispatches via
// IndexAmKind, never through fmgr.
fn fc_hnswhandler(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    panic!("hnswhandler: the closed AM set dispatches via IndexAmKind, never through fmgr")
}

pub fn init_seams() {
    dfmgr::register_builtin_library(dfmgr::BuiltinLibraryEntry {
        name: LIBRARY,
        lookup,
        pg_init: None,
    });
}

#[cfg(test)]
mod tests {
    // Every extension script pgrust ships.
    const SHIPPED_SCRIPTS: &[(&str, &str)] =
        &[("vector--0.8.5.sql", include_str!("../extension/vector--0.8.5.sql"))];

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

    // Spec §4.6: CREATE EXTENSION stops at the first CREATE FUNCTION whose
    // symbol `lookup` cannot resolve.
    #[test]
    fn lookup_covers_every_module_pathname_symbol() {
        for (file, script) in SHIPPED_SCRIPTS {
            let symbols = module_pathname_symbols(script);
            assert!(symbols.len() >= 35, "{file}: parsed only {} symbols", symbols.len());
            let missing: Vec<&String> = symbols.iter().filter(|s| super::lookup(s).is_none()).collect();
            assert!(missing.is_empty(), "{file}: no lookup entry for {missing:?}");
        }
    }
}

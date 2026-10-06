//! diffrunner `--pgvector`: the exact-differential tier of the pgvector
//! Phase 1 spec (§8.2). A fixed deck (pgvector_deck.sql) and a seeded random
//! arm exercise every vector, halfvec, sparsevec and bit function, cast,
//! operator and aggregate, their error cases, and exact (non-index)
//! nearest-neighbour queries against a reference (A) and a subject (B).
//!
//! Outcomes compare as everywhere in diffrunner (errors by SQLSTATE and
//! message, rows by text, float columns by ulp), with one widening scoped to
//! this suite: a row-set divergence whose differing numerals are
//! non-integral on at least one side and within REL_TOL relative (floored at
//! magnitude 1) is RULED under `pgvector-float-rel`. pgvector builds with
//! -fassociative-math and fuses multiply-adds, so its f32 sums are not
//! bit-reproducible (spec §2); integral numerals (indices, dimensions, exact
//! sums), structure, row order and errors still compare exactly.

use crate::copybin::normalize_user_oids;
use crate::diff::{classify, Classified, DiffClass, DiffInput, StmtOutcome};
use crate::rng::Rng;
use crate::ruled::{apply_ruled, RuledEntry};
use crate::runner::{Executor, Record};

/// Relative tolerance, floored at magnitude 1, for non-integral numerals.
pub const REL_TOL: f64 = 1e-5;
/// The ledger row (docs/fuzzing/rulings.toml) that rules them.
pub const RULING_ID: &str = "pgvector-float-rel";

const DECK: &str = include_str!("pgvector_deck.sql");

/// Outcome counters of one suite section.
#[derive(Clone, Debug, Default)]
pub struct SectionStats {
    pub name: String,
    pub cases: u32,
    pub matches: u32,
    pub ruled: u32,
    pub findings: u32,
}

/// The deck as (section, statement) pairs, in file order.
pub fn deck() -> Vec<(&'static str, &'static str)> {
    let mut section = "";
    let mut out = Vec::new();
    for line in DECK.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("-- section: ") {
            section = name;
        } else if !line.is_empty() && !line.starts_with("--") {
            out.push((section, line));
        }
    }
    out
}

/// Run the suite: CREATE EXTENSION ("setup"), the deck's sections, then
/// `seeded_n` statements from `seed` ("seeded").
pub fn run_suite(
    a: &mut dyn Executor,
    b: &mut dyn Executor,
    table: &[RuledEntry],
    ulp_tol: u64,
    seed: u64,
    seeded_n: u32,
) -> (Vec<Record>, Vec<SectionStats>) {
    let mut records = Vec::new();
    let mut sections = vec![SectionStats { name: "setup".to_string(), ..Default::default() }];
    let mut idx = 0u32;
    run_case(a, b, table, ulp_tol, "CREATE EXTENSION IF NOT EXISTS vector;", idx, &mut sections[0], &mut records);
    idx += 1;
    for (section, sql) in deck() {
        if sections.last().map(|s| s.name.as_str()) != Some(section) {
            sections.push(SectionStats { name: section.to_string(), ..Default::default() });
        }
        let stats = sections.last_mut().unwrap();
        run_case(a, b, table, ulp_tol, sql, idx, stats, &mut records);
        idx += 1;
    }
    let mut seeded = SectionStats { name: "seeded".to_string(), ..Default::default() };
    let mut rng = Rng::new_pure(seed);
    for _ in 0..seeded_n {
        let sql = seeded_statement(&mut rng);
        run_case(a, b, table, ulp_tol, &sql, idx, &mut seeded, &mut records);
        idx += 1;
    }
    sections.push(seeded);
    (records, sections)
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    a: &mut dyn Executor,
    b: &mut dyn Executor,
    table: &[RuledEntry],
    ulp_tol: u64,
    sql: &str,
    stmt_index: u32,
    stats: &mut SectionStats,
    records: &mut Vec<Record>,
) {
    let oa = normalize_user_oids(&a.apply(sql));
    let ob = normalize_user_oids(&b.apply(sql));
    let c = compare(table, ulp_tol, sql, &oa, &ob);
    stats.cases += 1;
    match &c.class {
        DiffClass::Match => stats.matches += 1,
        DiffClass::Ruled(_) => stats.ruled += 1,
        _ => stats.findings += 1,
    }
    if c.class != DiffClass::Match {
        records.push(Record { stmt_index, sql: sql.to_string(), class: c.class, detail: c.detail, probe: false });
    }
}

/// classify, then rule a row-set divergence that is tolerance-equal.
pub fn compare(table: &[RuledEntry], ulp_tol: u64, sql: &str, oa: &StmtOutcome, ob: &StmtOutcome) -> Classified {
    let raw = classify(&DiffInput { sql, a: oa, b: ob, ulp_tol, soft_cols: &[], mask_explain_timing: false });
    let raw = if raw.class == DiffClass::RowsetDiff && rows_close(oa, ob, REL_TOL) {
        Classified {
            class: DiffClass::Ruled(RULING_ID.to_string()),
            detail: format!("numerals within {REL_TOL} relative: {}", raw.detail),
        }
    } else {
        raw
    };
    apply_ruled(table, sql, raw)
}

/// Same shape, and every cell equal or `cells_close`, in row order.
fn rows_close(oa: &StmtOutcome, ob: &StmtOutcome, rel: f64) -> bool {
    match (oa, ob) {
        (StmtOutcome::Rows { col_oids: ca, rows: ra }, StmtOutcome::Rows { col_oids: cb, rows: rb }) => {
            ca == cb
                && ra.len() == rb.len()
                && ra.iter().zip(rb).all(|(x, y)| {
                    x.len() == y.len()
                        && x.iter().zip(y).all(|(p, q)| match (p, q) {
                            (None, None) => true,
                            (Some(p), Some(q)) => cells_close(p, q, rel),
                            _ => false,
                        })
                })
        }
        _ => false,
    }
}

/// Two cell texts that differ only in numerals that are non-integral on at
/// least one side and within `rel` (relative, floored at magnitude 1). The
/// structural characters of vector, halfvec, sparsevec and real[] text are
/// their own tokens and must match.
pub fn cells_close(x: &str, y: &str, rel: f64) -> bool {
    if x == y {
        return true;
    }
    let (tx, ty) = (tokens(x), tokens(y));
    tx.len() == ty.len() && tx.iter().zip(&ty).all(|(a, b)| a == b || numerals_close(a, b, rel))
}

fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if matches!(c, '[' | ']' | '{' | '}' | ',' | ':' | '/' | ' ') {
            if start < i {
                out.push(&s[start..i]);
            }
            out.push(&s[i..i + 1]);
            start = i + 1;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn is_integral(t: &str) -> bool {
    let d = t.strip_prefix('-').unwrap_or(t);
    !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit())
}

fn numerals_close(a: &str, b: &str, rel: f64) -> bool {
    if is_integral(a) && is_integral(b) {
        return false; // already known unequal
    }
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) if x.is_finite() && y.is_finite() => (x - y).abs() <= rel * x.abs().max(y.abs()).max(1.0),
        _ => false,
    }
}

// --- seeded arm ---

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ty {
    Vector,
    Halfvec,
    Sparsevec,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Vector => "vector",
            Ty::Halfvec => "halfvec",
            Ty::Sparsevec => "sparsevec",
        }
    }
}

/// Element classes: Int and Dyadic keep every sum exact; Decimal exercises
/// the tolerance; Tiny hits denormal and underflow paths.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Int,
    Dyadic,
    Decimal,
    Tiny,
}

const DIMS: &[usize] = &[1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33];

fn pick<T: Copy>(rng: &mut Rng, xs: &[T]) -> T {
    xs[rng.below_usize(xs.len())]
}

fn element(rng: &mut Rng, class: Class, ty: Ty) -> String {
    match class {
        Class::Int => rng.range_i64(-9, 9).to_string(),
        Class::Dyadic => format!("{}", rng.range_i64(-64, 64) as f64 / 8.0),
        Class::Decimal => format!("{}", rng.range_i64(-2_000_000, 2_000_000) as f64 / 1e6),
        Class::Tiny => {
            let pool: &[&str] = if ty == Ty::Halfvec {
                &["6e-08", "-1e-07", "3.1e-05", "0"]
            } else {
                &["1e-40", "-1.4e-45", "1.2e-38", "0"]
            };
            pick(rng, pool).to_string()
        }
    }
}

fn literal(rng: &mut Rng, ty: Ty, dim: usize, class: Class) -> String {
    if ty == Ty::Sparsevec {
        // Distinct ascending 1-based indices; zero values are dropped on input.
        let nnz = rng.below_usize(dim.min(8) + 1);
        let mut idx: Vec<usize> = (1..=dim).collect();
        for i in 0..nnz {
            let j = i + rng.below_usize(dim - i);
            idx.swap(i, j);
        }
        let mut chosen = idx[..nnz].to_vec();
        chosen.sort_unstable();
        let els: Vec<String> = chosen.iter().map(|i| format!("{i}:{}", element(rng, class, ty))).collect();
        format!("{{{}}}/{dim}", els.join(","))
    } else {
        let els: Vec<String> = (0..dim).map(|_| element(rng, class, ty)).collect();
        format!("[{}]", els.join(","))
    }
}

/// Exact nearest neighbours over integer data (exact sums, so an exact
/// order); ties break on the row number.
fn knn_statement(rng: &mut Rng, ty: Ty, dim: usize) -> String {
    let t = ty.name();
    let dim = dim.min(8);
    let rows: Vec<String> = (1..=10).map(|i| format!("({i}, '{}'::{t})", literal(rng, ty, dim, Class::Int))).collect();
    let q = literal(rng, ty, dim, Class::Int);
    let op = pick(rng, &["<->", "<#>", "<=>", "<+>"]);
    format!("SELECT i FROM (VALUES {}) s(i, v) ORDER BY v {op} '{q}'::{t}, i LIMIT 3;", rows.join(", "))
}

/// One seeded statement: single line, `;`-terminated.
pub fn seeded_statement(rng: &mut Rng) -> String {
    let ty = pick(rng, &[Ty::Vector, Ty::Halfvec, Ty::Sparsevec]);
    let t = ty.name();
    let dim = pick(rng, DIMS);
    // Mismatched dimensions one time in ten (CheckDims errors).
    let dim_b = if rng.chance(1, 10) { pick(rng, DIMS) } else { dim };
    let class = pick(rng, &[Class::Int, Class::Dyadic, Class::Decimal, Class::Tiny]);
    let a = literal(rng, ty, dim, class);
    let b = literal(rng, ty, dim_b, class);
    match rng.below(8) {
        0 => {
            let f = pick(rng, &["l2_distance", "inner_product", "cosine_distance", "l1_distance"]);
            format!("SELECT {f}('{a}'::{t}, '{b}'::{t});")
        }
        1 => {
            let op = pick(rng, &["<->", "<#>", "<=>", "<+>"]);
            format!("SELECT '{a}'::{t} {op} '{b}'::{t};")
        }
        2 => match ty {
            Ty::Sparsevec => format!("SELECT '{a}'::{t} < '{b}'::{t}, '{a}'::{t} = '{b}'::{t}, sparsevec_cmp('{a}', '{b}');"),
            _ => {
                let op = pick(rng, &["+", "-", "*", "||"]);
                format!("SELECT '{a}'::{t} {op} '{b}'::{t};")
            }
        },
        3 => {
            let norm = if ty == Ty::Vector { "vector_norm" } else { "l2_norm" };
            format!("SELECT {norm}('{a}'::{t}), l2_normalize('{a}'::{t});")
        }
        4 => {
            let to = match ty {
                Ty::Vector => pick(rng, &["halfvec", "sparsevec", "real[]"]),
                Ty::Halfvec => pick(rng, &["vector", "sparsevec", "real[]"]),
                Ty::Sparsevec => pick(rng, &["vector", "halfvec"]),
            };
            format!("SELECT '{a}'::{t}::{to};")
        }
        5 => match ty {
            Ty::Sparsevec => format!("SELECT '{a}'::{t} <= '{b}'::{t}, '{a}'::{t} <> '{b}'::{t};"),
            _ => {
                let c = literal(rng, ty, dim, class);
                format!("SELECT avg(v), sum(v) FROM (VALUES ('{a}'::{t}), ('{b}'::{t}), ('{c}'::{t})) s(v);")
            }
        },
        6 => match ty {
            Ty::Sparsevec => format!(
                "SELECT l2_norm('{a}'::{t}), sparsevec_l2_squared_distance('{a}', '{b}'), sparsevec_negative_inner_product('{a}', '{b}');"
            ),
            _ => {
                let start = rng.range_i64(-2, dim as i64 + 2);
                let count = rng.range_i64(-1, dim as i64 + 2);
                format!("SELECT subvector('{a}'::{t}, {start}, {count}), binary_quantize('{a}'::{t});")
            }
        },
        _ => knn_statement(rng, ty, dim),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgvector_deck_is_single_line_statements_in_named_sections() {
        let d = deck();
        assert_eq!(d.len(), 184, "deck statements");
        for (section, sql) in &d {
            assert!(!section.is_empty(), "statement before any section: {sql}");
            assert!(sql.starts_with("SELECT ") && sql.ends_with(';'), "{sql}");
        }
        for want in ["io", "typmod", "cast", "arith", "distance", "norm", "agg", "cmp", "misc", "bit", "knn", "limits"] {
            assert!(d.iter().any(|(s, _)| *s == want), "missing section {want}");
        }
    }

    #[test]
    fn pgvector_seeded_arm_is_deterministic_and_single_line() {
        let gen = |seed| {
            let mut r = Rng::new_pure(seed);
            (0..300).map(|_| seeded_statement(&mut r)).collect::<Vec<_>>()
        };
        assert_eq!(gen(1), gen(1));
        assert_ne!(gen(1), gen(2));
        for s in gen(7) {
            assert!(s.starts_with("SELECT ") && s.ends_with(';') && !s.contains('\n'), "{s}");
            assert_eq!(s.matches('(').count(), s.matches(')').count(), "{s}");
        }
    }

    #[test]
    fn pgvector_cells_close_widens_only_non_integral_numerals() {
        assert!(cells_close("0.30000001", "0.3", REL_TOL));
        assert!(cells_close("[0.1,0.2000001]", "[0.1,0.2]", REL_TOL));
        assert!(cells_close("{1:0.5,3:0.25000003}/4", "{1:0.5,3:0.25}/4", REL_TOL));
        assert!(cells_close("1.2e-07", "0", REL_TOL)); // cosine distance of near-identical vectors
        assert!(!cells_close("0.3001", "0.3", REL_TOL));
        assert!(!cells_close("{1:0.5}/4", "{2:0.5}/4", REL_TOL)); // index
        assert!(!cells_close("{1:0.5}/4", "{1:0.5}/5", REL_TOL)); // dimension
        assert!(!cells_close("25", "26", REL_TOL)); // integral on both sides
        assert!(!cells_close("[1,2]", "[1,2,3]", REL_TOL)); // shape
        assert!(!cells_close("Infinity", "3.4e+38", REL_TOL));
        assert!(!cells_close("NaN", "0", REL_TOL));
    }

    fn rows(v: &str) -> StmtOutcome {
        StmtOutcome::Rows { col_oids: vec![701], rows: vec![vec![Some(v.to_string())]] }
    }

    #[test]
    fn pgvector_compare_rules_tolerance_and_reports_real_divergence() {
        let table = crate::ruled::default_table();
        let sql = "SELECT l2_distance('[0.1]', '[0.2]');";
        assert_eq!(compare(&table, 4, sql, &rows("0.1"), &rows("0.1")).class, DiffClass::Match);
        let c = compare(&table, 4, sql, &rows("0.30000001"), &rows("0.3"));
        assert_eq!(c.class, DiffClass::Ruled(RULING_ID.to_string()));
        assert!(c.detail.contains("-fassociative-math"), "ledger reference missing: {}", c.detail);
        assert_eq!(compare(&table, 4, sql, &rows("0.31"), &rows("0.3")).class, DiffClass::RowsetDiff);
        let e = |m: &str| StmtOutcome::Error { sqlstate: "22000".to_string(), message: m.to_string() };
        let c = compare(&table, 4, sql, &e("different halfvec dimensions 2 and 1"), &e("different halfvec dimensions 1 and 2"));
        assert!(!matches!(c.class, DiffClass::Match | DiffClass::Ruled(_)), "{:?}", c.class);
    }
}

//! diffrunner `--pgvector`: the exact-differential tier of the pgvector
//! Phase 1 spec (§8.2). A fixed deck (pgvector_deck.sql) and a seeded random
//! arm exercise every vector, halfvec, sparsevec and bit function, cast,
//! operator and aggregate, their send output (bytea and binary COPY), their
//! error cases, and exact (non-index) nearest-neighbour queries against a
//! reference (A) and a subject (B). The setup case (CREATE EXTENSION) is a
//! finding unless it succeeds on A, so a broken reference cannot match.
//!
//! Outcomes compare exactly: errors by SQLSTATE and message, rows by text in
//! row order, COPY payloads byte for byte. No classify ruling is accepted
//! here: every one other than `pgvector-float-rel` escalates to ROWSET_DIFF,
//! among them `tie-ordering` (every ORDER BY in the suite is a total order),
//! `cmp-magnitude` (halfvec_cmp and sparsevec_cmp return exactly -1/0/1) and
//! `b1-float-ulp` (no float ulp slack outside the one widening below).
//!
//! The one widening is scoped to distance and norm statements: deck sections
//! `distance` and `norm`, and the seeded distance-function, distance-operator,
//! norm and sparsevec-metric statements. There, a row-set divergence whose
//! differing numerals are non-integral on at least one side and within
//! REL_TOL relative (floored at magnitude 1), or are zeros of either sign,
//! cell by cell in row order, is RULED under `pgvector-float-rel`. pgvector
//! builds with -fassociative-math and fuses multiply-adds, so its f32
//! distance and norm sums are not bit-reproducible (spec §2), and the sign of
//! a sum whose products all underflow follows C's vectorized, fused
//! reduction order (user-approved 2026-10-06). Everything else compares
//! exactly: l2_normalize (deck section `normalize`, seeded normalize
//! statements), the denormal and underflow cases the floor would hide (deck
//! section `denormal`), elementwise arithmetic, I/O, casts, aggregates,
//! comparisons, KNN order, the limits and every error, as do non-zero
//! integral numerals and structure in distance and norm statements.

use crate::copybin::normalize_user_oids;
use crate::diff::{classify, Classified, DiffClass, DiffInput, StmtOutcome};
use crate::rng::Rng;
use crate::ruled::{apply_ruled, RuledEntry};
use crate::runner::{Executor, Record};

/// Relative tolerance, floored at magnitude 1, for non-integral numerals.
pub const REL_TOL: f64 = 1e-5;
/// The ledger row (docs/fuzzing/rulings.toml) that rules them.
pub const RULING_ID: &str = "pgvector-float-rel";
/// Deck sections whose statements get the REL_TOL widening.
pub const TOLERANT_SECTIONS: &[&str] = &["distance", "norm"];

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

/// Run the suite: CREATE EXTENSION ("setup", a finding unless it succeeds on
/// A), the deck's sections, then `seeded_n` statements from `seed`
/// ("seeded"). Only TOLERANT_SECTIONS and the seeded statements
/// `seeded_statement` marks tolerant get REL_TOL.
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
    let setup = "CREATE EXTENSION IF NOT EXISTS vector;";
    run_case(a, b, table, ulp_tol, setup, false, true, idx, &mut sections[0], &mut records);
    idx += 1;
    for (section, sql) in deck() {
        if sections.last().map(|s| s.name.as_str()) != Some(section) {
            sections.push(SectionStats { name: section.to_string(), ..Default::default() });
        }
        let stats = sections.last_mut().unwrap();
        let tolerant = TOLERANT_SECTIONS.contains(&section);
        run_case(a, b, table, ulp_tol, sql, tolerant, false, idx, stats, &mut records);
        idx += 1;
    }
    let mut seeded = SectionStats { name: "seeded".to_string(), ..Default::default() };
    let mut rng = Rng::new_pure(seed);
    for _ in 0..seeded_n {
        let (sql, tolerant) = seeded_statement(&mut rng);
        run_case(a, b, table, ulp_tol, &sql, tolerant, false, idx, &mut seeded, &mut records);
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
    tolerant: bool,
    require_a_ok: bool,
    stmt_index: u32,
    stats: &mut SectionStats,
    records: &mut Vec<Record>,
) {
    let oa = normalize_user_oids(&a.apply(sql));
    let ob = normalize_user_oids(&b.apply(sql));
    let c = match &oa {
        StmtOutcome::Error { sqlstate, message } if require_a_ok => Classified {
            class: DiffClass::ErrorDiff,
            detail: format!("must succeed on A, which failed: {sqlstate}: {message}"),
        },
        StmtOutcome::ConnLost { detail } if require_a_ok => Classified {
            class: DiffClass::ErrorDiff,
            detail: format!("must succeed on A, which lost its connection: {detail}"),
        },
        _ => compare(table, ulp_tol, sql, &oa, &ob, tolerant),
    };
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

/// classify; escalate every classify ruling to ROWSET_DIFF (none applies in
/// this suite); then, in a `tolerant` (distance or norm) statement only, rule
/// a row-set divergence that is tolerance-equal in row order.
pub fn compare(
    table: &[RuledEntry],
    ulp_tol: u64,
    sql: &str,
    oa: &StmtOutcome,
    ob: &StmtOutcome,
    tolerant: bool,
) -> Classified {
    let raw = classify(&DiffInput { sql, a: oa, b: ob, ulp_tol, soft_cols: &[], mask_explain_timing: false });
    let raw = match &raw.class {
        DiffClass::Ruled(id) if id != RULING_ID => Classified {
            class: DiffClass::RowsetDiff,
            detail: format!("{id} does not apply in the pgvector suite: {}", raw.detail),
        },
        _ => raw,
    };
    let raw = if tolerant && raw.class == DiffClass::RowsetDiff && rows_close(oa, ob, REL_TOL) {
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
/// least one side and within `rel` (relative, floored at magnitude 1), or
/// that are zeros of either sign. The structural characters of vector,
/// halfvec, sparsevec and real[] text are their own tokens and must match.
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

/// "0", "-0", "0.0", "-0.0", ...: a numeral that parses to a zero.
fn is_zero(t: &str) -> bool {
    matches!(t.parse::<f64>(), Ok(x) if x == 0.0)
}

fn numerals_close(a: &str, b: &str, rel: f64) -> bool {
    if is_zero(a) && is_zero(b) {
        return true; // a zero of either sign (user-approved, see module doc)
    }
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

/// One seeded statement (single line, `;`-terminated) and whether it is a
/// distance or norm statement, the only seeded kinds that get REL_TOL:
/// distance functions (0), distance operators (1), the norm arm of kind 3
/// and the sparsevec metric arm of kind 6. Everything else compares exactly,
/// l2_normalize (kind 3's other arm) included.
pub fn seeded_statement(rng: &mut Rng) -> (String, bool) {
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
            (format!("SELECT {f}('{a}'::{t}, '{b}'::{t});"), true)
        }
        1 => {
            let op = pick(rng, &["<->", "<#>", "<=>", "<+>"]);
            (format!("SELECT '{a}'::{t} {op} '{b}'::{t};"), true)
        }
        2 => match ty {
            Ty::Sparsevec => (
                format!("SELECT '{a}'::{t} < '{b}'::{t}, '{a}'::{t} = '{b}'::{t}, sparsevec_cmp('{a}', '{b}');"),
                false,
            ),
            _ => {
                let op = pick(rng, &["+", "-", "*", "||"]);
                (format!("SELECT '{a}'::{t} {op} '{b}'::{t};"), false)
            }
        },
        3 => {
            if rng.chance(1, 2) {
                let norm = if ty == Ty::Vector { "vector_norm" } else { "l2_norm" };
                (format!("SELECT {norm}('{a}'::{t});"), true)
            } else {
                (format!("SELECT l2_normalize('{a}'::{t});"), false)
            }
        }
        4 => {
            let to = match ty {
                Ty::Vector => pick(rng, &["halfvec", "sparsevec", "real[]"]),
                Ty::Halfvec => pick(rng, &["vector", "sparsevec", "real[]"]),
                Ty::Sparsevec => pick(rng, &["vector", "halfvec"]),
            };
            (format!("SELECT '{a}'::{t}::{to};"), false)
        }
        5 => match ty {
            Ty::Sparsevec => (format!("SELECT '{a}'::{t} <= '{b}'::{t}, '{a}'::{t} <> '{b}'::{t};"), false),
            _ => {
                let c = literal(rng, ty, dim, class);
                (format!("SELECT avg(v), sum(v) FROM (VALUES ('{a}'::{t}), ('{b}'::{t}), ('{c}'::{t})) s(v);"), false)
            }
        },
        6 => match ty {
            Ty::Sparsevec => (
                format!(
                    "SELECT l2_norm('{a}'::{t}), sparsevec_l2_squared_distance('{a}', '{b}'), sparsevec_negative_inner_product('{a}', '{b}');"
                ),
                true,
            ),
            _ => {
                let start = rng.range_i64(-2, dim as i64 + 2);
                let count = rng.range_i64(-1, dim as i64 + 2);
                (format!("SELECT subvector('{a}'::{t}, {start}, {count}), binary_quantize('{a}'::{t});"), false)
            }
        },
        _ => (knn_statement(rng, ty, dim), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgvector_deck_is_single_line_statements_in_named_sections() {
        let d = deck();
        assert_eq!(d.len(), 197, "deck statements");
        for (section, sql) in &d {
            assert!(!section.is_empty(), "statement before any section: {sql}");
            assert!((sql.starts_with("SELECT ") || sql.starts_with("COPY (")) && sql.ends_with(';'), "{sql}");
        }
        let mut sections: Vec<&str> = d.iter().map(|(s, _)| *s).collect();
        sections.dedup();
        let want = [
            "io", "typmod", "cast", "arith", "distance", "norm", "normalize", "denormal", "agg", "cmp", "misc", "bit",
            "knn", "limits",
        ];
        assert_eq!(sections, want, "sections, in order, each contiguous");
        // l2_normalize is exact: only in exact sections.
        for (s, sql) in &d {
            if sql.contains("l2_normalize(") {
                assert!(!TOLERANT_SECTIONS.contains(s), "{s}: {sql}");
            }
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
        // The tolerance flag is exactly "names a distance or norm and is not
        // a KNN query" (KNN order compares exactly).
        let metric = [
            "l2_distance(", "inner_product(", "cosine_distance(", "l1_distance(", " <-> ", " <#> ", " <=> ", " <+> ",
            "norm(",
        ];
        let stmts = gen(7);
        assert!(stmts.iter().any(|(_, t)| *t) && stmts.iter().any(|(_, t)| !*t), "both kinds occur");
        // Kind 3 emits a norm (tolerant) or an l2_normalize (exact), never both.
        assert!(stmts.iter().any(|(s, t)| s.contains("l2_normalize(") && !*t), "exact normalize occurs");
        assert!(stmts.iter().any(|(s, t)| s.contains("_norm(") && !s.contains("_distance") && *t), "norm occurs");
        assert!(!stmts.iter().any(|(s, _)| s.contains("l2_normalize(") && s.contains("norm('")), "split");
        for (s, tolerant) in stmts {
            assert!(s.starts_with("SELECT ") && s.ends_with(';') && !s.contains('\n'), "{s}");
            assert_eq!(s.matches('(').count(), s.matches(')').count(), "{s}");
            let names_metric = metric.iter().any(|m| s.contains(m));
            assert_eq!(tolerant, names_metric && !s.contains(" ORDER BY "), "{s}");
        }
    }

    #[test]
    fn pgvector_tolerant_sections_are_distance_and_norm() {
        let tolerant: Vec<&str> = deck().iter().map(|(s, _)| *s).filter(|s| TOLERANT_SECTIONS.contains(s)).collect();
        assert_eq!(tolerant.iter().filter(|s| **s == "distance").count(), 20);
        assert_eq!(tolerant.iter().filter(|s| **s == "norm").count(), 2);
        assert_eq!(tolerant.len(), 22);
    }

    #[test]
    fn pgvector_cells_close_widens_only_non_integral_numerals_and_zeros() {
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
        // A zero of either sign (C's sign of an all-underflow sum follows its
        // vectorized, fused reduction order); nothing else integral widens.
        assert!(cells_close("-0", "0", REL_TOL));
        assert!(cells_close("[0,-0]", "[0,0]", REL_TOL));
        assert!(cells_close("-0.0", "0", REL_TOL));
        assert!(!cells_close("0", "1", REL_TOL));
        assert!(!cells_close("-0", "-1", REL_TOL));
        assert!(!cells_close("-0", "NaN", REL_TOL));
    }

    fn rows(v: &str) -> StmtOutcome {
        StmtOutcome::Rows { col_oids: vec![701], rows: vec![vec![Some(v.to_string())]] }
    }

    #[test]
    fn pgvector_compare_rules_tolerance_and_reports_real_divergence() {
        let table = crate::ruled::default_table();
        let sql = "SELECT l2_distance('[0.1]', '[0.2]');";
        assert_eq!(compare(&table, 4, sql, &rows("0.1"), &rows("0.1"), true).class, DiffClass::Match);
        let c = compare(&table, 4, sql, &rows("0.30000001"), &rows("0.3"), true);
        assert_eq!(c.class, DiffClass::Ruled(RULING_ID.to_string()));
        assert!(c.detail.contains("-fassociative-math"), "ledger reference missing: {}", c.detail);
        assert_eq!(compare(&table, 4, sql, &rows("0.31"), &rows("0.3"), true).class, DiffClass::RowsetDiff);
        let e = |m: &str| StmtOutcome::Error { sqlstate: "22000".to_string(), message: m.to_string() };
        let c = compare(&table, 4, sql, &e("different halfvec dimensions 2 and 1"), &e("different halfvec dimensions 1 and 2"), true);
        assert!(!matches!(c.class, DiffClass::Match | DiffClass::Ruled(_)), "{:?}", c.class);
    }

    /// One vector-typed cell (user type oid, as normalize_user_oids leaves it).
    fn vrow(v: &str) -> StmtOutcome {
        StmtOutcome::Rows { col_oids: vec![16384], rows: vec![vec![Some(v.to_string())]] }
    }

    fn int4_rows(vs: &[&str]) -> StmtOutcome {
        StmtOutcome::Rows { col_oids: vec![23], rows: vs.iter().map(|v| vec![Some(v.to_string())]).collect() }
    }

    fn is_finding(c: &Classified) -> bool {
        !matches!(c.class, DiffClass::Match | DiffClass::Ruled(_))
    }

    #[test]
    fn pgvector_tolerance_applies_only_to_distance_and_norm_statements() {
        let table = crate::ruled::default_table();
        let exact = "SELECT l2_normalize('[0.1]'::vector);";
        let metric = "SELECT l2_norm('[0.1]'::halfvec);";
        for (x, y) in [("[0]", "[6e-08]"), ("[0.1]", "[0.10000001]")] {
            let c = compare(&table, 4, exact, &vrow(x), &vrow(y), false);
            assert!(is_finding(&c), "{x} vs {y} not a finding in an exact statement: {c:?}");
            let c = compare(&table, 4, metric, &vrow(x), &vrow(y), true);
            assert_eq!(c.class, DiffClass::Ruled(RULING_ID.to_string()), "{x} vs {y}: {}", c.detail);
        }
    }

    #[test]
    fn pgvector_zero_of_either_sign_is_equal_only_in_distance_and_norm_statements() {
        let table = crate::ruled::default_table();
        let metric = "SELECT '[1e-40,-1.4e-45]'::vector <#> '[1e-40,1.2e-38]';";
        let exact = "SELECT '[0,0]'::halfvec * '[1,-1]';";
        for (a, b) in [(rows("-0"), rows("0")), (vrow("[0,-0]"), vrow("[0,0]"))] {
            let c = compare(&table, 4, metric, &a, &b, true);
            assert_eq!(c.class, DiffClass::Ruled(RULING_ID.to_string()), "{a:?} vs {b:?}: {}", c.detail);
            let c = compare(&table, 4, exact, &a, &b, false);
            assert!(is_finding(&c), "{a:?} vs {b:?} not a finding in an exact statement: {c:?}");
        }
        for (x, y) in [("25", "26"), ("0", "1")] {
            let c = compare(&table, 4, metric, &rows(x), &rows(y), true);
            assert!(is_finding(&c), "{x} vs {y} not a finding in a distance statement: {c:?}");
        }
    }

    #[test]
    fn pgvector_row_order_and_cmp_magnitude_compare_exactly() {
        let table = crate::ruled::default_table();
        let raw = |sql: &str, a: &StmtOutcome, b: &StmtOutcome| {
            classify(&DiffInput { sql, a, b, ulp_tol: 4, soft_cols: &[], mask_explain_timing: false }).class
        };
        // A reorder under ORDER BY: classify alone rules it tie-ordering.
        let sql = "SELECT i FROM (VALUES (1, '[1]'::vector), (2, '[2]')) s(i, v) ORDER BY v <-> '[0]', i;";
        let (a, b) = (int4_rows(&["1", "2"]), int4_rows(&["2", "1"]));
        assert_eq!(raw(sql, &a, &b), DiffClass::Ruled("tie-ordering".to_string()));
        for tolerant in [false, true] {
            let c = compare(&table, 4, sql, &a, &b, tolerant);
            assert_eq!(c.class, DiffClass::RowsetDiff, "{}", c.detail);
            assert!(c.detail.contains("tie-ordering"), "{}", c.detail);
        }
        // *_cmp magnitude: classify alone rules it cmp-magnitude.
        let sql = "SELECT sparsevec_cmp('{1:1}/2', '{1:2}/2');";
        let (a, b) = (int4_rows(&["-1"]), int4_rows(&["-2"]));
        assert_eq!(raw(sql, &a, &b), DiffClass::Ruled("cmp-magnitude".to_string()));
        let c = compare(&table, 4, sql, &a, &b, false);
        assert_eq!(c.class, DiffClass::RowsetDiff, "{}", c.detail);
        // Float ulp: classify alone rules it b1-float-ulp; exact outside the
        // distance and norm statements, pgvector-float-rel inside them.
        let sql = "SELECT jaccard_distance('1100', '1010');";
        let (a, b) = (rows("0.6666666666666667"), rows("0.6666666666666666"));
        assert_eq!(raw(sql, &a, &b), DiffClass::Ruled("b1-float-ulp".to_string()));
        assert_eq!(compare(&table, 4, sql, &a, &b, false).class, DiffClass::RowsetDiff);
        let sql = "SELECT cosine_distance('[1,1]'::vector, '[1,2]');";
        assert_eq!(raw(sql, &a, &b), DiffClass::Ruled("b1-float-ulp".to_string()));
        assert_eq!(compare(&table, 4, sql, &a, &b, true).class, DiffClass::Ruled(RULING_ID.to_string()));
    }

    #[test]
    fn pgvector_escalates_every_classify_ruling_but_its_own() {
        let table = crate::ruled::default_table();
        let raw = |sql: &str, a: &StmtOutcome, b: &StmtOutcome| {
            classify(&DiffInput { sql, a, b, ulp_tol: 4, soft_cols: &[], mask_explain_timing: false }).class
        };
        // A ledger row outside the old denylist: a B-only parallel-worker
        // failure is ruled by classify, and must be a finding here.
        let sql = "SELECT l2_distance('[1]'::vector, '[2]');";
        let (a, b) = (
            rows("1"),
            StmtOutcome::Error {
                sqlstate: "55000".to_string(),
                message: "parallel worker failed to initialize".to_string(),
            },
        );
        assert_eq!(raw(sql, &a, &b), DiffClass::Ruled("parallel-worker-init".to_string()));
        for tolerant in [false, true] {
            let c = compare(&table, 4, sql, &a, &b, tolerant);
            assert!(is_finding(&c), "{c:?}");
            assert!(c.detail.contains("parallel-worker-init does not apply"), "{}", c.detail);
        }
    }

    #[test]
    fn pgvector_copy_payloads_compare_byte_for_byte() {
        let table = crate::ruled::default_table();
        let sql = "COPY (VALUES ('[1]'::halfvec)) TO STDOUT (FORMAT binary);";
        let co = |bytes: &[u8]| StmtOutcome::CopyOut { bytes: bytes.to_vec(), tag: "COPY 1".to_string() };
        assert_eq!(compare(&table, 4, sql, &co(b"PGCOPY\n\x3c\x00"), &co(b"PGCOPY\n\x3c\x00"), false).class, DiffClass::Match);
        let c = compare(&table, 4, sql, &co(b"PGCOPY\n\x3c\x00"), &co(b"PGCOPY\n\x3c\x01"), false);
        assert!(is_finding(&c), "{c:?}");
    }

    /// Answers every statement with one fixed outcome, CREATE EXTENSION with
    /// another.
    struct Canned {
        setup: StmtOutcome,
        other: StmtOutcome,
    }

    impl Executor for Canned {
        fn apply(&mut self, sql: &str) -> StmtOutcome {
            if sql.starts_with("CREATE EXTENSION") { self.setup.clone() } else { self.other.clone() }
        }
    }

    #[test]
    fn pgvector_setup_must_succeed_on_a() {
        let table = crate::ruled::default_table();
        let ok = StmtOutcome::Command { tag: "CREATE EXTENSION".to_string(), affected: None };
        let err = StmtOutcome::Error { sqlstate: "58P01".to_string(), message: "could not open extension control file".to_string() };
        let other = StmtOutcome::Command { tag: "SELECT".to_string(), affected: None };
        let run = |setup: &StmtOutcome| {
            let mut a = Canned { setup: setup.clone(), other: other.clone() };
            let mut b = Canned { setup: setup.clone(), other: other.clone() };
            run_suite(&mut a, &mut b, &table, 4, 1, 3).1
        };
        let s = run(&ok);
        assert_eq!((s[0].name.as_str(), s[0].matches, s[0].findings), ("setup", 1, 0));
        // The same error on both sides matches outcome-wise, but setup is a
        // finding: a reference without the extension must not pass.
        let s = run(&err);
        assert_eq!((s[0].name.as_str(), s[0].matches, s[0].findings), ("setup", 0, 1));
        assert!(s[1..].iter().all(|x| x.findings == 0), "only setup fails");
    }
}

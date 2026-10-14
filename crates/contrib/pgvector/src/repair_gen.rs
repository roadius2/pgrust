//! Generates repair/pgrust-trimmed-0.8.5.sql from upstream sql/vector.sql:
//! one guarded create-and-add block per upstream object, so a database made
//! by a pre-M4 pgrust build (vector 0.8.5 from a trimmed install script; three
//! generations shipped on main, each a subset of upstream) gains exactly the
//! objects it lacks, after which ALTER EXTENSION vector UPDATE works.
//! `PGVECTOR_REGEN_REPAIR=1 cargo test ... repair_script_is_generated` rewrites
//! the committed file.

const UPSTREAM: &str = include_str!("../../../pgvector-0.8.7-reference/sql/vector.sql");
const REPAIR_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/repair/pgrust-trimmed-0.8.5.sql");

// Upstream statements: `--` comment lines and blank lines dropped, lines joined
// up to the one ending in `;` (vector.sql has no $$ bodies).
fn statements(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in sql.lines() {
        let t = line.trim_end();
        if t.trim_start().starts_with("--") || t.trim().is_empty() || t.starts_with("\\echo") {
            continue;
        }
        cur.push(t);
        if t.ends_with(';') {
            out.push(cur.join("\n").replace("MODULE_PATHNAME", "$libdir/vector"));
            cur.clear();
        }
    }
    assert!(cur.is_empty(), "unterminated statement at end of vector.sql");
    out
}

// The text between the first '(' at or after `from` and its matching ')'.
fn parens(s: &str, from: usize) -> (usize, usize) {
    let open = from + s[from..].find('(').expect("(");
    let mut depth = 0;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (open + 1, open + i);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced parentheses in {s}")
}

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

// (guard, actions after the statement) for one upstream statement; None for a
// COMMENT (applied by vector--0.8.6--0.8.7.sql).
fn guard_and_adds(stmt: &str, am_comments: &[String]) -> Option<(String, Vec<String>)> {
    let flat = words(stmt).join(" ");
    let w = words(&flat);
    if flat.starts_with("COMMENT ON ") {
        return None;
    }
    if flat.starts_with("CREATE TYPE ") {
        let name = w[2].trim_end_matches(';');
        if flat.ends_with(';') && w.len() == 3 {
            return Some((format!("to_regtype('{name}') IS NULL"), vec![]));
        }
        // Not to_regtype: it is NULL for a shell type (parseTypeString's
        // "is only a shell", parse_type.c:811-813), which is the state the
        // shell block above leaves a missing type in.
        return Some((
            format!(
                "NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = '{name}' AND typnamespace = v_nsp_oid AND typisdefined)"
            ),
            vec![format!("ALTER EXTENSION vector ADD TYPE {name};")],
        ));
    }
    for (kw, add) in [("CREATE FUNCTION ", "FUNCTION"), ("CREATE AGGREGATE ", "AGGREGATE")] {
        if let Some(rest) = flat.strip_prefix(kw) {
            let name = rest[..rest.find('(').expect("(")].trim();
            let (a, b) = parens(rest, 0);
            let sig = format!("{name}({})", &rest[a..b]);
            return Some((
                format!("to_regprocedure('{sig}') IS NULL"),
                vec![format!("ALTER EXTENSION vector ADD {add} {sig};")],
            ));
        }
    }
    if let Some(rest) = flat.strip_prefix("CREATE CAST ") {
        let (a, b) = parens(rest, 0);
        let inner = &rest[a..b];
        let (src, dst) = inner.split_once(" AS ").expect("CAST (a AS b)");
        return Some((
            format!(
                "NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('{src}') AND casttarget = to_regtype('{dst}'))"
            ),
            vec![format!("ALTER EXTENSION vector ADD CAST ({src} AS {dst});")],
        ));
    }
    if flat.starts_with("CREATE OPERATOR CLASS ") {
        let name = w[3];
        let am = w[w.iter().position(|&x| x == "USING").expect("USING") + 1];
        return Some((
            format!(
                "NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = '{name}' AND a.amname = '{am}' AND c.opcnamespace = v_nsp_oid)"
            ),
            vec![
                format!("ALTER EXTENSION vector ADD OPERATOR CLASS {name} USING {am};"),
                format!("ALTER EXTENSION vector ADD OPERATOR FAMILY {name} USING {am};"),
            ],
        ));
    }
    if let Some(rest) = flat.strip_prefix("CREATE OPERATOR ") {
        let op = rest.split_whitespace().next().expect("operator name");
        let (a, b) = parens(rest, 0);
        let body = &rest[a..b];
        let arg = |key: &str| -> String {
            body.split(',')
                .map(str::trim)
                .find_map(|kv| kv.strip_prefix(key).map(|v| v.trim_start_matches([' ', '=']).trim().to_string()))
                .unwrap_or_else(|| panic!("{key} in {flat}"))
        };
        let (l, r) = (arg("LEFTARG"), arg("RIGHTARG"));
        // A shell (oprcode 0, made by an earlier block's COMMUTATOR or
        // NEGATOR) counts as missing: to_regoperator finds it
        // (OpernameGetOprid ignores oprcode), and CREATE OPERATOR fills it
        // in (OperatorCreate, pg_operator.c:370-380).
        return Some((
            format!(
                "NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('{op}({l},{r})') AND oprcode::oid <> 0)"
            ),
            vec![format!("ALTER EXTENSION vector ADD OPERATOR {op} ({l}, {r});")],
        ));
    }
    if flat.starts_with("CREATE ACCESS METHOD ") {
        let am = w[3];
        let mut adds = vec![format!("ALTER EXTENSION vector ADD ACCESS METHOD {am};")];
        adds.extend(am_comments.iter().filter(|c| words(c).get(4) == Some(&am)).cloned());
        return Some((format!("NOT EXISTS (SELECT 1 FROM pg_am WHERE amname = '{am}')"), adds));
    }
    panic!("repair generator: unhandled upstream statement kind: {flat}")
}

pub(crate) fn generate(upstream: &str) -> String {
    let stmts = statements(upstream);
    let am_comments: Vec<String> =
        stmts.iter().filter(|s| s.starts_with("COMMENT ON ACCESS METHOD ")).cloned().collect();
    let mut out = String::new();
    out.push_str(HEADER);
    for s in &stmts {
        let Some((guard, adds)) = guard_and_adds(s, &am_comments) else { continue };
        out.push_str(&format!("  IF {guard} THEN\n"));
        for line in s.lines() {
            out.push_str("    ");
            out.push_str(line);
            out.push('\n');
        }
        for a in adds {
            out.push_str(&format!("    {a}\n"));
        }
        out.push_str("    n := n + 1;\n  END IF;\n");
    }
    out.push_str(FOOTER);
    out
}

const HEADER: &str = "\
-- Repair for databases created by pgrust builds before M4 (pgvector
-- extversion 0.8.5 from pgrust's trimmed install script). Those builds
-- shipped a subset of upstream vector.sql under the 0.8.5 label, so
-- ALTER EXTENSION vector UPDATE fails at vector--0.8.6--0.8.7.sql (its
-- COMMENTs name objects the trimmed script never created). This script
-- creates exactly the missing upstream objects as members of the
-- extension; then run ALTER EXTENSION vector UPDATE. Run as a superuser,
-- once in each database whose vector is at 0.8.5; it is atomic, idempotent,
-- and a no-op elsewhere. Generated from upstream sql/vector.sql by
-- crates/contrib/pgvector/src/repair_gen.rs; do not edit by hand.
DO $repair$
DECLARE
  v_ver text;
  v_nsp text;
  v_nsp_oid oid;
  v_old_path text := current_setting('search_path');
  n int := 0;
BEGIN
  SELECT e.extversion, n2.nspname, n2.oid INTO v_ver, v_nsp, v_nsp_oid
    FROM pg_extension e JOIN pg_namespace n2 ON n2.oid = e.extnamespace
   WHERE e.extname = 'vector';
  IF v_ver IS NULL THEN
    RAISE EXCEPTION 'extension \"vector\" is not installed in this database';
  END IF;
  IF v_ver <> '0.8.5' THEN
    RAISE NOTICE 'vector is at %; nothing to repair', v_ver;
    RETURN;
  END IF;
  PERFORM set_config('search_path', quote_ident(v_nsp) || ', pg_catalog', true);
";

// No `\` continuation here: it would strip the first line's indentation.
const FOOTER: &str = "  PERFORM set_config('search_path', v_old_path, true);
  RAISE NOTICE 'vector: created % missing objects; now run ALTER EXTENSION vector UPDATE', n;
END
$repair$;
";

// The module is test-only already (lib.rs); the cfg(test) on each test is for
// lint-determinism, which fences brace-matched cfg(test) items but cannot see
// a module file's cfg from its parent, so the raw fs/env sites below would
// count as production code.
#[cfg(test)]
#[test]
fn repair_script_is_generated_from_upstream() {
    let want = generate(UPSTREAM);
    if std::env::var_os("PGVECTOR_REGEN_REPAIR").is_some() {
        std::fs::write(REPAIR_PATH, &want).unwrap();
    }
    let got = std::fs::read_to_string(REPAIR_PATH).expect("repair file (run with PGVECTOR_REGEN_REPAIR=1)");
    assert!(got == want, "repair/pgrust-trimmed-0.8.5.sql is stale: rerun with PGVECTOR_REGEN_REPAIR=1");
}

// Every upstream object statement gets exactly one guarded block, and each
// block carries upstream's text verbatim (MODULE_PATHNAME substituted).
#[cfg(test)]
#[test]
fn repair_covers_every_upstream_object() {
    let stmts = statements(UPSTREAM);
    let objects = stmts.iter().filter(|s| !s.starts_with("COMMENT ON ")).count();
    assert_eq!(objects, 213, "upstream vector.sql 0.8.7 has 213 CREATE statements");
    let script = generate(UPSTREAM);
    // HEADER's own two precondition IFs are not object blocks.
    assert_eq!(script.matches("  IF ").count() - HEADER.matches("  IF ").count(), objects);
    for s in stmts.iter().filter(|s| !s.starts_with("COMMENT ON ")) {
        let indented: String = s.lines().map(|l| format!("    {l}\n")).collect();
        assert!(script.contains(&indented), "missing verbatim: {s}");
    }
}

# pgvector Phase 1 · M2: `halfvec`, `sparsevec` and `bit` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port pgvector 0.8.7's `halfvec` and `sparsevec` types and the `bit` distance functions into pgrust's `pgvector` crate, byte-identical to C on disk and on the wire. Prove the port with four checks:
- the six M2 regression files pass;
- an exhaustive f16 parity test against C passes;
- a new exact-differential tier is clean;
- an error-location check is clean.

**Architecture:**
- Five new modules in `crates/contrib/pgvector`, named after the C files: `bitutils.rs`, `bitvec.rs`, `halfutils.rs`, `halfvec.rs` and `sparsevec.rs`.
  - They mirror the existing `vec.rs` / `funcs.rs` patterns: payload views, varlena builders and `fc_*` fmgr functions.
  - Every symbol registers in the single `lookup` table.
  - The trimmed extension script grows section by section, using text cut verbatim from upstream by a new helper script.
- A test-only crate, `pgvector_f16_parity`, compiles pgvector's own f16 routines from the vendored `halfutils.h`. It checks every f32, every f16 and every pair of halves against the Rust port.
- The exact differential is a new diffrunner suite, `--pgvector`: a fixed SQL deck plus a seeded random arm. It runs through a new tier script, `run-diff.sh`, which also diffs error `LOCATION` lines.

**Tech Stack:** Rust (the pgrust workspace, `fast-profile`), C via the `cc` crate (already in `Cargo.lock`, test crate only), bash 3.2 harness scripts, PostgreSQL 18.6 + pgvector 0.8.7 C reference builds, `pg_regress`, Perl TAP (`prove`), fuzzgen's `diffrunner`.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`: §4.2 (modules), §4.6 (extension SQL, lookup coverage), §5.2 (type layouts), §5.3 (numeric fidelity), §8.2 (tiers), §9 M2 row. Inputs:
- `docs/HANDOFF_2026-10-05.md` §1;
- `docs/superpowers/reports/pgvector-m1-report.md`;
- `docs/superpowers/reports/pgvector-m0-baseline.md`.

Facts verified while writing this plan (2026-10-05, macOS arm64, against the C reference):
- **f16 port.** The Rust f16 conversions below match pgvector's software routine and clang's `_Float16` casts for all 2³² f32 and all 2¹⁶ f16 inputs, NaNs included.
- **Half arithmetic.** The reference build does halfvec `+ - *` in native `_Float16` (`fadd h0, h0, h1`). Widening to f32 and rounding once gives the same bits for all 3·2³² operand pairs, except NaN payloads. NaN operands can't reach that arithmetic, because CheckElement rejects them first.
- **Float drift.** On non-integer data, C and pgrust's existing `vector` distances already differ in the last bits in 55–90% of rows. C reassociates and fuses its sums under `-fassociative-math`. The worst gap, relative and floored at 1, was 6e-7 at ≤100 dimensions and 3.9e-6 at 384. Elementwise `+` and `*` were bit-identical.
- **Error locations.** C reports the last line of the `ereport`/`elog` call. Examples: `CheckExpectedDim, vector.c:88` and `Float4ToHalf, halfutils.h:257`. pgrust's existing `vector` errors report `pgvector.c:0`, an open M1 item.
- **Deck and error file.** The differential deck in Task 7 (184 statements) and the error-location file in Task 6 (49 statements) were run on the C reference. Every error is the intended one, and every `LOCATION` matches the line numbers used below.

## Global Constraints

- **Branch:** work on `vector/m2`, cut from `main`. Merge locally (fast-forward) when done, and push only when the user asks. Work in the main checkout, not a worktree, because the disk is about 91% full.
- **Commit trailer:** every commit message ends with a `Co-Authored-By:` trailer naming the model that wrote the commit, as in M1. For example: `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. Never edit them and never build inside them. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md):
  - Keep C function names, and make comments cite C `file.c:line`. pgvector citations point into `crates/pgvector-0.8.7-reference/src/`.
  - Mark intentional differences `DIVERGENCE`.
- **Error locations:** every new error that mirrors a C `ereport`/`elog` carries `.with_location("<file>", <line>, "<CFunction>")`.
  - `<line>` is the last line of the C call (the line holding its closing `);`).
  - Inside `static inline` helpers, the C function is the helper's own name (`CheckDim`, not its caller).
  - The values in this plan's code are the verified ones; copy them exactly.
- **Pre-M2 `vector` code stays as it is** apart from:
  - `pub(crate)` visibility on helpers;
  - the two `vector.c` cast functions M2 adds (`halfvec_to_vector`, `sparsevec_to_vector`).
  
  Its errors keep their crate-derived location (an open item from the M1 report). `halfvec_to_vector` and `sparsevec_to_vector` reuse vector's existing `check_dim`/`check_expected_dim`, so they inherit that gap. Tasks 6 and 9 record it.
- **Extension SQL:**
  - `crates/contrib/pgvector/extension/vector--0.8.5.sql` stays the shipped script and `default_version = '0.8.5'` stays (spec §4.6).
  - M2 appends upstream sections only through `scripts/pgvector/upstream-sql.sh` (Task 1). That never hand-copies SQL and drops `COMMENT ON` lines, IVFFlat/HNSW opclasses and `*_support(internal)` functions, which belong to M3/M4.
- **No new external dependencies.** `cc` (locked at 1.4.0, already used by `regexp_alt`) appears only as a build-dependency of the test crate. New path dependencies on workspace crates are fine.
- **Upstream expected outputs are never edited.**
- **Unit-test commands.** They reuse the `target/fast-profile` artifacts the server build produced. Listing `main_main` keeps feature resolution identical to the server build. Never run any of them with an empty filter.
  - `UT <filter...>` means:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- <filter...>
    ```
  - `UT_PARITY` means:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p pgvector_f16_parity --lib -- f16_
    ```
  - `UT_FUZZ <filter...>` means:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p fuzzgen --lib -- <filter...>
    ```
  - `UT_M2` is every unit-test filter M2 adds:
    ```
    UT lookup_cover bitutils:: bitvec:: halfutils:: halfvec:: sparsevec:: funcs::
    ```
- **Cargo.lock:** adding a path dependency or a workspace member changes `Cargo.lock`.
  - After any `Cargo.toml` edit, run `cargo update --workspace --offline` before the next `--locked` command.
  - Check that `git diff --stat Cargo.lock` touches only the edited package's entries, then commit `Cargo.lock` with the task.
- **Server build:** `scripts/pgvector/build-pgrust.sh` builds `target/fast-profile/postgres`. Every conformance check needs it rebuilt after a Rust change.
- **Harness commands** (from the repo root):
  - Regression: `scripts/pgvector/run-regress.sh pgrust <test ...>`. Results go to `~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv`, diffs to `regression.diffs`.
  - TAP: `scripts/pgvector/run-tap.sh pgrust <NNN_name.pl ...>`.
  - Per-task SQL comparison against C: `scripts/pgvector/sqldiff.sh pgrust <file|->` (from Task 3 on).
  - Ports: pgrust 55491, C reference 55492, seeded C reference 55494.
- **`M2_REGRESS`** means `bit btree cast copy halfvec sparsevec vector_type hnsw_vector`, the six M2 files plus the two M1 ones that must stay green.
- **Harness scripts:** `#!/usr/bin/env bash`, `set -euo pipefail`, bash 3.2 compatible. That rules out associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, and bare `"${arr[@]}"` on a possibly empty array under `set -u` (use `${arr[@]+"${arr[@]}"}`).
- **Lints** (shell scripts, no build), from the repo root. Both must pass at the end of every task that touches Rust:
  - `bash crates/_support/seams_init/tests/lint-determinism.sh`
  - `bash crates/_support/seams_init/tests/lint-seam-installs.sh`
- **wasm:** the repo's wasm ledger (`wasm/wasm-crate-ledger.md`) is ratchet-only, so never add a row. The new test crate stays wasm-clean by construction: its `build.rs` returns early on `wasm32`, and its tests are cfg-gated. The full `wasm/wasm-build.sh` gate needs `nightly-2026-07-17`, which isn't installed. It is not run in M2, and the report says so.
- **Disk:** about 170 GiB free at plan time. Before Task 1, if `df -h .` shows under 120 GiB free, delete `~/.cache/pgrust/pgvector-work/tap`. It holds old TAP logs and is regenerable (handoff §2).
- **The plan's Rust code** mirrors patterns that compile today in `vec.rs`/`funcs.rs`. If a snippet doesn't compile as written:
  - make the smallest fix and name it in the task report;
  - never change behavior to make code compile.

## Review Focus

1. **Malformed or adversarial text input must give a clean SQL error, never a panic.** Cases include:
   - integers at the `i32`/`i64` limits (`'{2147483648:1}/1'`, `'{-2147483649:1}/1'`, `'{}/9223372036854775808'`);
   - truncated literals and stray separators.
   
   A panic kills the backend. TAP 037 also needs every error to name its type.
   Tests: `parse_survives_every_prefix` (Task 3), `input_survives_every_prefix` and `strtol_saturates_like_64_bit_long` (Task 5), TAP 037 (Task 6).
2. **Untrusted binary input (COPY BINARY, `*_recv`) must be rejected exactly as C rejects it, before any large allocation.** Cases include:
   - dimension 0 or negative;
   - negative, oversized or larger-than-dimension `nnz`;
   - unsorted or duplicate indices;
   - zero values, NaN or infinite halves;
   - truncated messages.
   
   Tests: `recv_validates_like_c` in Task 3 (halfvec) and Task 5 (sparsevec).
3. **`halfvec` values and arithmetic must equal the reference's bit for bit** at 65504, 65519 (rounds to 65504), 65520 (out of range), subnormals and −0. That covers both conversions and `+ - *`, which the macOS arm64 reference does in native `_Float16`.
   Tests: the three exhaustive tests in `pgvector_f16_parity` (Task 2).
4. **Sparse distance merges with disjoint, interleaved, prefix or suffix, and empty index sets** are where a port of the merge loops goes wrong. Regress covers only a few shapes.
   Test: `sparse_kernels_match_dense_expansion` (Task 6), 5,000 seeded pairs against a dense expansion with exact integer data.
5. **Huge declared sparse dimensions** (`'{1000000000:1}/1000000000'`):
   - casts to `vector`/`halfvec` must error at the 16,000 limit before allocating;
   - distance, norm, normalize and output must stay O(nnz).
   
   Tests: the `limits` section of the differential deck (Task 7), plus the SQL check in Task 6 Step 7.

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/contrib/pgvector/src/lib.rs` | module list, `lookup` (+64 symbols), lookup-coverage test, crate header | 1, 3, 4, 5, 6, 9 |
| `crates/contrib/pgvector/src/bitutils.rs` | **new:** `bitutils.c` Hamming/Jaccard kernels | 1 |
| `crates/contrib/pgvector/src/bitvec.rs` | **new:** `bitvec.c` `hamming_distance`, `jaccard_distance`, `InitBitVector` | 1 |
| `crates/contrib/pgvector/src/halfutils.rs` | **new:** `halfutils.h/.c` f16 conversions, predicates, halfvec distance kernels | 2 |
| `crates/contrib/pgvector/src/halfvec.rs` | **new:** `halfvec.c`: view/builder, checks, I/O, casts, functions, aggregates, `sparsevec_to_halfvec` | 3, 4, 6 |
| `crates/contrib/pgvector/src/sparsevec.rs` | **new:** `sparsevec.c`: view/builder, checks, I/O, casts, functions | 5, 6 |
| `crates/contrib/pgvector/src/vec.rs` | `vector_isspace` becomes `pub(crate)` | 3 |
| `crates/contrib/pgvector/src/funcs.rs` | `pub(crate)` helpers, shared array-cast prologue, `halfvec_to_vector`, `sparsevec_to_vector` | 3, 4, 6 |
| `crates/contrib/pgvector/Cargo.toml` | `adt_varbit`, `numutils` path dependencies | 1, 5 |
| `crates/contrib/pgvector/extension/vector--0.8.5.sql` | appended upstream sections | 1, 3, 4, 5, 6 |
| `crates/contrib/pgvector_f16_parity/{Cargo.toml,build.rs,src/lib.rs}` | **new:** exhaustive C parity (test-only) | 2 |
| `Cargo.toml` (root), `Cargo.lock` | workspace member, lock entries | 1, 2, 5 |
| `scripts/pgvector/upstream-sql.sh` | **new:** prints upstream extension-script sections | 1 |
| `scripts/pgvector/sqldiff.sh` | **new:** one SQL script on fresh ref and subject servers, diffed | 3 |
| `scripts/pgvector/sql/error-locations.sql` | **new:** one statement per reachable error site | 6 |
| `crates/bin/fuzzgen/src/pgvector.rs`, `pgvector_deck.sql` | **new:** diffrunner `--pgvector` suite and deck | 7 |
| `crates/bin/fuzzgen/src/{lib.rs,copybin.rs,bin/diffrunner.rs}` | module registration, a helper's visibility, CLI flag | 7 |
| `docs/fuzzing/rulings.toml` | ruling `pgvector-float-rel` | 7 |
| `scripts/pgvector/{common.sh,build-pgrust.sh,run-all.sh}` | TCP listen address, diffrunner build, the `diff` tier | 8 |
| `scripts/pgvector/run-diff.sh` | **new:** exact-differential tier | 8 |
| `scripts/pgvector/tests/harness_test.sh` | self-tests for `sqldiff.sh` and `run-diff.sh` | 3, 8 |
| `crates/contrib/pgvector/sql/` | **deleted** (spec §4.1) | 9 |
| spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m2-report.md` | docs and the M2 report | 9 |

## Execution notes

- **Task order and dependencies:**
  - Tasks 1–6 are sequential. Each extends `lookup`, which the lookup-coverage test checks against the script.
  - Task 7 depends only on Task 1's branch and could run in parallel. It still waits for Task 6, so the deck can be run against a complete pgrust.
  - Task 8 needs Tasks 6 and 7. Task 9 needs everything.
- **Suggested implementers:**
  - Sonnet for Tasks 1–5: the code is spelled out.
  - Opus for Tasks 6–9: the cross-type casts plus the full regression/TAP gate, the fuzzgen suite, the tier and its triage, and the final report.

---

### Task 1: Branch, upstream-SQL helper, lookup coverage, and the `bit` functions

**Files:**
- Create: `scripts/pgvector/upstream-sql.sh`
- Create: `crates/contrib/pgvector/src/bitutils.rs`, `crates/contrib/pgvector/src/bitvec.rs`
- Modify: `crates/contrib/pgvector/src/lib.rs` (modules, `lookup`, new `#[cfg(test)] mod tests`)
- Modify: `crates/contrib/pgvector/Cargo.toml` (+`adt_varbit`), `Cargo.lock`
- Modify: `crates/contrib/pgvector/extension/vector--0.8.5.sql` (append `bit functions`, `bit operators`)

**Interfaces:**
- Produces:
  - `scripts/pgvector/upstream-sql.sh 'SECTION' ...` prints those sections. Later tasks append its output with:
    ```bash
    { echo; scripts/pgvector/upstream-sql.sh ... | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
    ```
  - `pub fn bitutils::bit_hamming_distance(bytes: usize, ax: &[u8], bx: &[u8], distance: u64) -> u64`
  - `pub fn bitutils::bit_jaccard_distance(bytes: usize, ax: &[u8], bx: &[u8], ab: u64, aa: u64, bb: u64) -> f64`
    
    M3's HNSW `bit` support uses both of these.
  - `pub(crate) fn bitvec::init_bit_vector<'m>(mcx: Mcx<'m>, dim: usize) -> PgResult<PgVec<'m, u8>>`. It returns a full varbit image: 4-byte header, `i32` bit length, zeroed bits. Task 4 uses it.
  - `pub fn bitvec::fc_hamming_distance` and `fc_jaccard_distance` (fmgr signature).
  - `lib.rs` `#[cfg(test)] mod tests`, holding `lookup_covers_every_module_pathname_symbol`. Every later task relies on it.

- [ ] **Step 1: Branch and commit the plan**

```bash
cd /Users/jody/dev_projects/pgrust
df -h . | tail -1
git checkout -b vector/m2
git add docs/superpowers/plans/2026-10-05-pgvector-m2-types.md
git commit -q -F - <<'EOF'
docs(pgvector): M2 plan (halfvec, sparsevec, bit functions)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

- [ ] **Step 2: Write `scripts/pgvector/upstream-sql.sh`**

```bash
#!/usr/bin/env bash
# Print named sections of the vendored upstream extension script
# (crates/pgvector-0.8.7-reference/sql/vector.sql) for the trimmed
# extension/vector--0.8.5.sql. Dropped: COMMENT ON statements (0.8.6+; they
# arrive with the verbatim script in M4, spec §4.6) and every block of an
# access method not yet ported (USING ivfflat/hnsw opclasses and the
# *_support(internal) functions). Blocks are blank-line separated; a section
# runs from its "-- <name>" header to the next header.
# usage: upstream-sql.sh 'SECTION' ...   e.g. upstream-sql.sh 'bit functions' 'bit operators'
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -ge 1 ] || die "usage: upstream-sql.sh 'SECTION' ..."
want=""
for s in "$@"; do
  grep -qx -- "-- $s" "$PGV_SRC/sql/vector.sql" || die "no section '-- $s' in $PGV_SRC/sql/vector.sql"
  want="$want|-- $s|"
done
awk -v want="$want" '
  BEGIN { RS = ""; ORS = "\n\n" }
  /^-- / && !/\n/ { keep = index(want, "|" $0 "|") > 0; if (keep) print; next }
  keep && !/USING (ivfflat|hnsw)/ && !/_support\(internal\)/ {
    n = split($0, line, "\n"); out = ""
    for (i = 1; i <= n; i++) if (line[i] !~ /^COMMENT ON /) out = out (out == "" ? "" : "\n") line[i]
    if (out != "") print out
  }
' "$PGV_SRC/sql/vector.sql"
```
Then:
```bash
chmod +x scripts/pgvector/upstream-sql.sh
scripts/pgvector/upstream-sql.sh 'bit functions' 'bit operators'
scripts/pgvector/upstream-sql.sh 'halfvec opclasses' | grep -c 'OPERATOR CLASS'
scripts/pgvector/upstream-sql.sh 'no such section'; echo "exit $?"
```
Expected:
- The first command prints the `-- bit functions` header, two `CREATE FUNCTION` blocks (`hamming_distance`, `jaccard_distance`), the `-- bit operators` header, and two `CREATE OPERATOR` blocks (`<~>`, `<%>`). It prints no `COMMENT ON` lines.
- The second prints `1` (only the btree `halfvec_ops` survives).
- The third prints `error: no section '-- no such section' ...` and `exit 1`.

- [ ] **Step 3: Write the failing lookup-coverage test**

Append to the end of `crates/contrib/pgvector/src/lib.rs`:
```rust
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
```

- [ ] **Step 4: Run it, then append the `bit` SQL and see it fail**

```bash
UT lookup_cover
{ echo; scripts/pgvector/upstream-sql.sh 'bit functions' 'bit operators' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
UT lookup_cover
```
Expected:
- The first run: 2 passed. The current script is covered.
- The second run fails with `no lookup entry for ["hamming_distance", "jaccard_distance"]`.

- [ ] **Step 5: Add the dependency**

In `crates/contrib/pgvector/Cargo.toml`, add under `[dependencies]` (after `stringinfo`):
```toml
adt_varbit = { path = "../../backend/utils/adt/varbit" }
```
Then run `cargo update --workspace --offline` and `git diff --stat Cargo.lock`. Expected: one line added to the `pgvector` package's dependency list.

- [ ] **Step 6: Write `bitutils.rs`**

```rust
//! bitutils.c (pgvector 0.8.7): Hamming and Jaccard distance over bit
//! strings. Ports the portable `*Default` kernels (bitutils.c:49-131); the
//! AVX-512 dispatch (bitutils.c:75-160) computes the same integer counts.

// BitHammingDistanceDefault (bitutils.c:49-73): popcount of a XOR b, eight
// bytes at a time, then the tail bytes.
pub fn bit_hamming_distance(bytes: usize, ax: &[u8], bx: &[u8], mut distance: u64) -> u64 {
    let (mut ax, mut bx, mut bytes) = (ax, bx, bytes);
    while bytes >= 8 {
        let a = u64::from_ne_bytes(ax[..8].try_into().unwrap());
        let b = u64::from_ne_bytes(bx[..8].try_into().unwrap());
        distance += (a ^ b).count_ones() as u64;
        ax = &ax[8..];
        bx = &bx[8..];
        bytes -= 8;
    }
    for i in 0..bytes {
        distance += (ax[i] ^ bx[i]).count_ones() as u64;
    }
    distance
}

// BitJaccardDistanceDefault (bitutils.c:98-131).
pub fn bit_jaccard_distance(
    bytes: usize,
    ax: &[u8],
    bx: &[u8],
    mut ab: u64,
    mut aa: u64,
    mut bb: u64,
) -> f64 {
    let (mut ax, mut bx, mut bytes) = (ax, bx, bytes);
    while bytes >= 8 {
        let a = u64::from_ne_bytes(ax[..8].try_into().unwrap());
        let b = u64::from_ne_bytes(bx[..8].try_into().unwrap());
        ab += (a & b).count_ones() as u64;
        aa += a.count_ones() as u64;
        bb += b.count_ones() as u64;
        ax = &ax[8..];
        bx = &bx[8..];
        bytes -= 8;
    }
    for i in 0..bytes {
        ab += (ax[i] & bx[i]).count_ones() as u64;
        aa += ax[i].count_ones() as u64;
        bb += bx[i].count_ones() as u64;
    }
    if ab == 0 {
        1.0
    } else {
        1.0 - (ab as f64 / ((aa + bb - ab) as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // bit.out: '111' vs '110' differ in one bit; 17 bytes cross the
    // eight-byte loop and the tail.
    #[test]
    fn hamming_counts_differing_bits_across_words_and_tail() {
        assert_eq!(bit_hamming_distance(1, &[0b1110_0000], &[0b1100_0000], 0), 1);
        assert_eq!(bit_hamming_distance(17, &[0xFF; 17], &[0x0F; 17], 0), 17 * 4);
        assert_eq!(bit_hamming_distance(0, &[], &[], 0), 0);
    }

    // bit.out: jaccard_distance('1111', '1100') = 0.5; no common set bit
    // (including all-zero inputs) is distance 1 (bitutils.c:127-128).
    #[test]
    fn jaccard_matches_c_formula() {
        assert_eq!(bit_jaccard_distance(1, &[0b1111_0000], &[0b1100_0000], 0, 0, 0), 0.5);
        assert_eq!(bit_jaccard_distance(1, &[0], &[0], 0, 0, 0), 1.0);
        assert_eq!(bit_jaccard_distance(9, &[0xAA; 9], &[0x55; 9], 0, 0, 0), 1.0);
        assert_eq!(bit_jaccard_distance(9, &[0xAA; 9], &[0xAA; 9], 0, 0, 0), 0.0);
    }
}
```

- [ ] **Step 7: Write `bitvec.rs`**

```rust
//! bitvec.c (pgvector 0.8.7): hamming_distance and jaccard_distance over
//! Postgres' bit/varbit, and InitBitVector for binary_quantize.

use datum::Datum;
use mcx::{Mcx, PgVec};
use types_error::{PgError, PgResult, ERRCODE_DATA_EXCEPTION};
use types_fmgr::{FmgrInfo, FunctionCallInfoBaseData as Fcinfo};

use crate::bitutils::{bit_hamming_distance, bit_jaccard_distance};

// InitBitVector (bitvec.c:16-28): full varbit image (4-byte varlena header,
// i32 bit length, zeroed bits).
pub(crate) fn init_bit_vector<'m>(mcx: Mcx<'m>, dim: usize) -> PgResult<PgVec<'m, u8>> {
    let total = 4 + 4 + dim.div_ceil(8);
    let mut img: PgVec<'m, u8> = mcx::vec_with_capacity_in(mcx, total)?;
    img.resize(total, 0);
    img[..4].copy_from_slice(&((total as u32) << 2).to_ne_bytes());
    img[4..8].copy_from_slice(&(dim as i32).to_ne_bytes());
    Ok(img)
}

// CheckDims (bitvec.c:33-40). VARBITLEN prints with %u.
fn check_dims(a: &[u8], b: &[u8]) -> PgResult<()> {
    let (alen, blen) = (adt_varbit::payload_bitlen(a) as u32, adt_varbit::payload_bitlen(b) as u32);
    if alen != blen {
        return Err(PgError::error(format!("different bit lengths {alen} and {blen}"))
            .with_sqlstate(ERRCODE_DATA_EXCEPTION)
            .with_location("bitvec.c", 39, "CheckDims")
            .into());
    }
    Ok(())
}

// Payloads of the two bit arguments: [i32 bit length][bits].
fn bit_2arg(fcinfo: &Fcinfo) -> PgResult<(&[u8], &[u8])> {
    // SAFETY: strict fns — args 0 and 1 are non-null bit/varbit varlenas.
    let a = unsafe { fcinfo.arg_varlena_packed(0)? }.data();
    let b = unsafe { fcinfo.arg_varlena_packed(1)? }.data();
    Ok((a, b))
}

// hamming_distance (bitvec.c:45-55).
pub fn fc_hamming_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = bit_2arg(fcinfo)?;
    check_dims(a, b)?;
    let (ax, bx) = (adt_varbit::payload_bits(a), adt_varbit::payload_bits(b));
    Ok(Datum::from_f64(bit_hamming_distance(ax.len(), ax, bx, 0) as f64))
}

// jaccard_distance (bitvec.c:60-70).
pub fn fc_jaccard_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = bit_2arg(fcinfo)?;
    check_dims(a, b)?;
    let (ax, bx) = (adt_varbit::payload_bits(a), adt_varbit::payload_bits(b));
    Ok(Datum::from_f64(bit_jaccard_distance(ax.len(), ax, bx, 0, 0, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_bit_vector_is_a_zeroed_varbit() {
        let ctx = mcx::MemoryContext::new("bitvec-test");
        let img = init_bit_vector(ctx.mcx(), 9).unwrap();
        assert_eq!(img.len(), 4 + 4 + 2);
        assert_eq!(adt_varbit::payload_bitlen(&img[4..]), 9);
        assert_eq!(adt_varbit::payload_bits(&img[4..]), &[0, 0]);
    }

    #[test]
    fn check_dims_names_both_lengths() {
        let three = [3i32.to_ne_bytes().as_slice(), &[0b1110_0000]].concat();
        let two = [2i32.to_ne_bytes().as_slice(), &[0b0000_0000]].concat();
        let e = check_dims(&three, &two).unwrap_err();
        assert_eq!(e.message(), "different bit lengths 3 and 2");
    }
}
```

- [ ] **Step 8: Register the modules and symbols**

In `crates/contrib/pgvector/src/lib.rs`, change the module list to:
```rust
pub mod bitutils;
pub mod bitvec;
pub mod funcs;
pub mod vec;
```
and add these two arms to `lookup`, directly before `"hnswhandler" => fc_hnswhandler,`:
```rust
        "hamming_distance" => bitvec::fc_hamming_distance,
        "jaccard_distance" => bitvec::fc_jaccard_distance,
```

- [ ] **Step 9: Run the unit tests**

Run: `UT lookup_cover bitutils:: bitvec::`
Expected: 6 passed.

- [ ] **Step 10: Regression check against a rebuilt server**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust bit vector_type hnsw_vector
```
Expected: all three `ok` in `~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv`. `bit` was FAIL after M1.

- [ ] **Step 11: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add scripts/pgvector/upstream-sql.sh crates/contrib/pgvector Cargo.lock
git commit -q -F - <<'EOF'
pgvector: bit hamming/jaccard distance; lookup-coverage test

Ports bitutils.c and bitvec.c (hamming_distance, jaccard_distance, <~>,
<%>) and appends their upstream SQL via the new upstream-sql.sh helper.
The lookup-coverage unit test (spec §4.6) checks every MODULE_PATHNAME
symbol of the shipped script resolves in `lookup`. Regress `bit` passes.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 2: `halfutils` and the exhaustive f16 parity crate

**Files:**
- Create: `crates/contrib/pgvector/src/halfutils.rs`
- Modify: `crates/contrib/pgvector/src/lib.rs` (`pub mod halfutils;`)
- Create: `crates/contrib/pgvector_f16_parity/Cargo.toml`, `build.rs`, `src/lib.rs`
- Modify: root `Cargo.toml` (`members`), `Cargo.lock`

**Interfaces:**
- Produces, in `pgvector::halfutils` (Tasks 3, 4 and 6 use these; M3's HNSW halfvec support will too):
  - `pub type Half = u16;` holds the raw binary16 bits.
  - `pub fn half_is_nan(Half) -> bool`, `pub fn half_is_inf(Half) -> bool`, `pub fn half_is_zero(Half) -> bool`.
  - `pub fn half_to_float4(Half) -> f32` and `pub fn float4_to_half_unchecked(f32) -> Half`.
  - `pub fn float4_to_half(f32) -> PgResult<Half>`. It errors on a finite input that rounds to infinity.
  - Four distance kernels. Each `ax`/`bx` holds `dim` native-endian halves, and the halfvec view in Task 3 provides them:
    - `pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32`
    - `pub fn halfvec_inner_product(dim, ax, bx) -> f32`
    - `pub fn halfvec_cosine_similarity(dim, ax, bx) -> f64`
    - `pub fn halfvec_l1_distance(dim, ax, bx) -> f32`

- [ ] **Step 1: Write the failing unit tests**

Create `crates/contrib/pgvector/src/halfutils.rs` with only this test module for now:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn halves(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| float4_to_half_unchecked(*x).to_ne_bytes()).collect()
    }

    // halfvec.out values: 1.23456 stores 1.234375; 65519 rounds down to
    // HALF_MAX, 65520 up to infinity; 1e-8 underflows to signed zero.
    #[test]
    fn conversion_spot_values() {
        assert_eq!(float4_to_half_unchecked(1.0), 0x3C00);
        assert_eq!(float4_to_half_unchecked(-2.0), 0xC000);
        assert_eq!(float4_to_half_unchecked(1.23456), 0x3CF0);
        assert_eq!(float4_to_half_unchecked(65504.0), 0x7BFF);
        assert_eq!(float4_to_half_unchecked(65519.0), 0x7BFF);
        assert_eq!(float4_to_half_unchecked(65520.0), 0x7C00);
        assert_eq!(float4_to_half_unchecked(2f32.powi(-24)), 0x0001);
        assert_eq!(float4_to_half_unchecked(1e-8), 0x0000);
        assert_eq!(float4_to_half_unchecked(-1e-8), 0x8000);
        assert_eq!(half_to_float4(0x3CF0), 1.234375);
        assert_eq!(half_to_float4(0x0001), 2f32.powi(-24));
        assert_eq!(half_to_float4(0x7BFF), 65504.0);
        assert!(half_to_float4(0x7C00).is_infinite());
        assert!(half_to_float4(0x7E00).is_nan());
        assert!(half_is_nan(0x7E00) && !half_is_nan(0x7C00));
        assert!(half_is_inf(0xFC00) && !half_is_inf(0x7BFF));
        assert!(half_is_zero(0x8000) && !half_is_zero(0x0001));
    }

    // Float4ToHalf (halfutils.h:244-261): only a finite input that rounds
    // to infinity is out of range; an infinite input is CheckElement's job.
    #[test]
    fn float4_to_half_rejects_finite_overflow_only() {
        assert_eq!(float4_to_half(65519.0).unwrap(), 0x7BFF);
        let e = float4_to_half(65520.0).unwrap_err();
        assert_eq!(e.message(), "\"65520\" is out of range for type halfvec");
        assert_eq!(float4_to_half(f32::INFINITY).unwrap(), 0x7C00);
    }

    #[test]
    fn kernels_on_small_vectors() {
        let (a, b) = (halves(&[0.0, 0.0]), halves(&[3.0, 4.0]));
        assert_eq!(halfvec_l2_squared_distance(2, &a, &b), 25.0);
        assert_eq!(halfvec_l1_distance(2, &a, &b), 7.0);
        let (c, d) = (halves(&[1.0, 2.0]), halves(&[3.0, 4.0]));
        assert_eq!(halfvec_inner_product(2, &c, &d), 11.0);
        assert_eq!(halfvec_cosine_similarity(2, &c, &halves(&[2.0, 4.0])), 1.0);
    }
}
```
Add `pub mod halfutils;` to `lib.rs` (in alphabetical order, after `funcs`).

- [ ] **Step 2: Run them to see them fail**

Run: `UT halfutils::`
Expected: compile errors, `cannot find function 'float4_to_half_unchecked'` and the like.

- [ ] **Step 3: Implement `halfutils.rs`**

Insert above the test module:
```rust
//! halfutils.h / halfutils.c (pgvector 0.8.7): the `half` (IEEE binary16)
//! conversions and the halfvec distance kernels.
//!
//! The conversions port the software path halfutils.h takes when neither
//! F16C_SUPPORT nor FLT16_SUPPORT is defined (halfutils.h:63-239). IEEE
//! defines f32<->f16 round-to-nearest-even exactly, so they give the same
//! bits as the F16C and _Float16 paths; crate pgvector_f16_parity checks
//! every input. The kernels port the `*Default` loops (halfutils.c:29-207)
//! in C source order; the x86 F16C dispatch only reorders the f32 sums.

use types_error::{PgError, PgResult, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE};

/// `half` without FLT16_SUPPORT (halfvec.h:56-58): the raw binary16 bits.
pub type Half = u16;

// HalfIsNan (halfutils.h:23-31).
#[inline]
pub fn half_is_nan(num: Half) -> bool {
    (num & 0x7C00) == 0x7C00 && (num & 0x7FFF) != 0x7C00
}

// HalfIsInf (halfutils.h:36-44).
#[inline]
pub fn half_is_inf(num: Half) -> bool {
    (num & 0x7FFF) == 0x7C00
}

// HalfIsZero (halfutils.h:49-57).
#[inline]
pub fn half_is_zero(num: Half) -> bool {
    (num & 0x7FFF) == 0x0000
}

// HalfToFloat4 (halfutils.h:62-141), software path.
#[inline]
pub fn half_to_float4(num: Half) -> f32 {
    let bin = num as u32;
    let mut exponent: i32 = ((bin & 0x7C00) >> 10) as i32;
    let mut mantissa: u32 = bin & 0x03FF;
    // Sign
    let mut result: u32 = (bin & 0x8000) << 16;
    if exponent == 31 {
        if mantissa == 0 {
            // Infinite
            result |= 0x7F80_0000;
        } else {
            // NaN
            result |= 0x7FC0_0000;
        }
    } else if exponent == 0 {
        // Subnormal
        if mantissa != 0 {
            exponent = -14;
            for _ in 0..10 {
                mantissa <<= 1;
                exponent -= 1;
                if (mantissa >> 10) % 2 == 1 {
                    mantissa &= 0x03ff;
                    break;
                }
            }
            result |= ((exponent + 127) as u32) << 23;
        }
    } else {
        // Normal
        result |= ((exponent - 15 + 127) as u32) << 23;
    }
    result |= mantissa << 13;
    f32::from_bits(result)
}

// Float4ToHalfUnchecked (halfutils.h:146-239), software path.
#[inline]
pub fn float4_to_half_unchecked(num: f32) -> Half {
    let bin = num.to_bits();
    let mut exponent: i32 = ((bin & 0x7F80_0000) >> 23) as i32;
    let mut mantissa: i32 = (bin & 0x007F_FFFF) as i32;
    // Sign
    let mut result: u16 = ((bin & 0x8000_0000) >> 16) as u16;
    if num.is_infinite() {
        // Infinite
        result |= 0x7C00;
    } else if num.is_nan() {
        // NaN
        result |= 0x7E00;
        result |= (mantissa >> 13) as u16;
    } else if exponent > 98 {
        exponent -= 127;
        let mut s = mantissa & 0x0000_0FFF;
        // Subnormal
        if exponent < -14 {
            let diff = -exponent - 14;
            mantissa >>= diff;
            mantissa += 1 << (23 - diff);
            s |= mantissa & 0x0000_0FFF;
        }
        let mut m = mantissa >> 13;
        // Round
        let gr = (mantissa >> 12) % 4;
        if gr == 3 || (gr == 1 && s != 0) {
            m += 1;
        }
        if m == 1024 {
            m = 0;
            exponent += 1;
        }
        if exponent > 15 {
            // Infinite
            result |= 0x7C00;
        } else {
            if exponent >= -14 {
                result |= ((exponent + 15) << 10) as u16;
            }
            result |= m as u16;
        }
    }
    result
}

// Float4ToHalf (halfutils.h:244-261): a finite float that rounds to an
// infinite half is out of range.
pub fn float4_to_half(num: f32) -> PgResult<Half> {
    let result = float4_to_half_unchecked(num);
    if half_is_inf(result) && !num.is_infinite() {
        let mut buf = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
        let n = ryu::float_to_shortest_decimal_bufn(num, &mut buf);
        return Err(PgError::error(format!(
            "\"{}\" is out of range for type halfvec",
            String::from_utf8_lossy(&buf[..n])
        ))
        .with_sqlstate(ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE)
        .with_location("halfutils.h", 257, "Float4ToHalf")
        .into());
    }
    Ok(result)
}

// Element i of a halfvec's native-endian half array, as a float.
#[inline]
fn hx(x: &[u8], i: usize) -> f32 {
    half_to_float4(Half::from_ne_bytes([x[2 * i], x[2 * i + 1]]))
}

// HalfvecL2SquaredDistanceDefault (halfutils.c:29-43).
pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        let diff = hx(ax, i) - hx(bx, i);
        distance += diff * diff;
    }
    distance
}

// HalfvecInnerProductDefault (halfutils.c:81-91).
pub fn halfvec_inner_product(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        distance += hx(ax, i) * hx(bx, i);
    }
    distance
}

// HalfvecCosineSimilarityDefault (halfutils.c:124-144).
pub fn halfvec_cosine_similarity(dim: usize, ax: &[u8], bx: &[u8]) -> f64 {
    let mut similarity = 0.0f32;
    let mut norma = 0.0f32;
    let mut normb = 0.0f32;
    for i in 0..dim {
        let (axi, bxi) = (hx(ax, i), hx(bx, i));
        similarity += axi * bxi;
        norma += axi * axi;
        normb += bxi * bxi;
    }
    // Use sqrt(a * b) over sqrt(a) * sqrt(b)
    similarity as f64 / ((norma as f64) * (normb as f64)).sqrt()
}

// HalfvecL1DistanceDefault (halfutils.c:197-207).
pub fn halfvec_l1_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        distance += (hx(ax, i) - hx(bx, i)).abs();
    }
    distance
}
```

- [ ] **Step 4: Run the unit tests**

Run: `UT halfutils::`
Expected: 3 passed.

- [ ] **Step 5: Create the parity crate**

`crates/contrib/pgvector_f16_parity/Cargo.toml`:
```toml
[package]
name = "pgvector_f16_parity"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

[lib]
name = "pgvector_f16_parity"
path = "src/lib.rs"

[dependencies]
pgvector = { path = "../pgvector" }

[build-dependencies]
cc = "1"
```
In the root `Cargo.toml`, add `"crates/contrib/pgvector_f16_parity",` to `members`, directly after `"crates/contrib/pgvector",`.

`crates/contrib/pgvector_f16_parity/build.rs`:
```rust
//! Compiles pgvector 0.8.7's own f16 code for the parity tests in
//! src/lib.rs: the software routines cut verbatim from the vendored
//! halfutils.h (the path C takes when neither F16C_SUPPORT nor FLT16_SUPPORT
//! is defined), next to the compiler's `_Float16` conversions and
//! arithmetic (the FLT16_SUPPORT path the macOS arm64 reference build
//! takes). Skipped on wasm32 (no C toolchain there), which compiles the
//! tests out (`cfg(pgv_c_half)`).
use std::{env, fs, path::PathBuf};

const HALFUTILS: &str = "../../pgvector-0.8.7-reference/src/halfutils.h";

/// One `static inline` function of halfutils.h, keeping only its software
/// `#else` branch.
fn software_branch(src: &str, signature: &str) -> String {
    let start = src.find(signature).unwrap_or_else(|| panic!("{HALFUTILS}: no {signature:?}"));
    let end = start + src[start..].find("\n}\n").expect("function end") + "\n}\n".len();
    let f = &src[start..end];
    let cond = f.find("#if defined(F16C_SUPPORT)").expect("#if defined(F16C_SUPPORT)");
    let soft = cond + f[cond..].find("#else\n").expect("#else") + "#else\n".len();
    let endif = f.rfind("#endif\n").expect("#endif");
    format!("{}{}{}", &f[..cond], &f[soft..endif], &f[endif + "#endif\n".len()..])
}

const SHIMS: &str = r#"
uint32_t pgv_sw_half_to_float4(uint16_t h) { union { float f; uint32_t i; } u; u.f = HalfToFloat4(h); return u.i; }
uint16_t pgv_sw_float4_to_half(uint32_t bits) { union { float f; uint32_t i; } u; u.i = bits; return Float4ToHalfUnchecked(u.f); }
#ifdef __FLT16_MAX__
typedef union { _Float16 h; uint16_t i; } pgv_hu;
int pgv_has_float16(void) { return 1; }
uint32_t pgv_hw_half_to_float4(uint16_t h) { pgv_hu x; union { float f; uint32_t i; } u; x.i = h; u.f = (float) x.h; return u.i; }
uint16_t pgv_hw_float4_to_half(uint32_t bits) { pgv_hu r; union { float f; uint32_t i; } u; u.i = bits; r.h = (_Float16) u.f; return r.i; }
uint16_t pgv_hw_add(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h + y.h; return r.i; }
uint16_t pgv_hw_sub(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h - y.h; return r.i; }
uint16_t pgv_hw_mul(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h * y.h; return r.i; }
#else
int pgv_has_float16(void) { return 0; }
uint32_t pgv_hw_half_to_float4(uint16_t h) { (void) h; return 0; }
uint16_t pgv_hw_float4_to_half(uint32_t bits) { (void) bits; return 0; }
uint16_t pgv_hw_add(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
uint16_t pgv_hw_sub(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
uint16_t pgv_hw_mul(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
#endif
"#;

fn main() {
    println!("cargo:rerun-if-changed={HALFUTILS}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(pgv_c_half)");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }
    let src = fs::read_to_string(HALFUTILS).expect("read halfutils.h");
    let c = format!(
        "#include <math.h>\n#include <stdint.h>\n\
         typedef uint16_t uint16;\ntypedef uint32_t uint32;\ntypedef uint16 half;\n\
         #define unlikely(x) (x)\n\n{}\n{}\n{}",
        software_branch(&src, "static inline float\nHalfToFloat4(half num)"),
        software_branch(&src, "static inline half\nFloat4ToHalfUnchecked(float num)"),
        SHIMS
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("halfutils_parity.c");
    fs::write(&out, c).expect("write halfutils_parity.c");
    cc::Build::new().file(&out).warnings(false).compile("pgv_halfutils_parity");
    println!("cargo:rustc-cfg=pgv_c_half");
}
```

`crates/contrib/pgvector_f16_parity/src/lib.rs`:
```rust
//! Exhaustive parity of pgvector::halfutils with pgvector 0.8.7's C f16
//! code (spec §8.2, unit tier): every f16 and every f32 bit pattern through
//! the conversions, and every pair of halves through + - *, against the
//! routines build.rs compiles. Test-only: nothing depends on this crate.

#[cfg(all(test, pgv_c_half))]
mod tests {
    use pgvector::halfutils::{float4_to_half_unchecked, half_is_nan, half_to_float4, Half};

    extern "C" {
        fn pgv_sw_half_to_float4(h: u16) -> u32;
        fn pgv_sw_float4_to_half(bits: u32) -> u16;
        fn pgv_has_float16() -> i32;
        fn pgv_hw_half_to_float4(h: u16) -> u32;
        fn pgv_hw_float4_to_half(bits: u32) -> u16;
        fn pgv_hw_add(a: u16, b: u16) -> u16;
        fn pgv_hw_sub(a: u16, b: u16) -> u16;
        fn pgv_hw_mul(a: u16, b: u16) -> u16;
    }

    fn has_float16() -> bool {
        // SAFETY: pure C function, no preconditions.
        unsafe { pgv_has_float16() != 0 }
    }

    /// Sum of `check(lo, hi)` over 64 chunks of `0..n`, run in parallel.
    fn par_count(n: u64, check: impl Fn(u64, u64) -> u64 + Sync) -> u64 {
        const CHUNKS: u64 = 64;
        let step = n.div_ceil(CHUNKS);
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..CHUNKS)
                .map(|c| {
                    let check = &check;
                    s.spawn(move || check(c * step, ((c + 1) * step).min(n)))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).sum()
        })
    }

    #[test]
    fn f16_half_to_float4_matches_c_for_every_half() {
        let hw = has_float16();
        for h in 0..=u16::MAX {
            let ours = half_to_float4(h).to_bits();
            // SAFETY: pure C functions, no preconditions.
            assert_eq!(ours, unsafe { pgv_sw_half_to_float4(h) }, "software path, half {h:#06x}");
            if hw {
                // SAFETY: as above.
                assert_eq!(ours, unsafe { pgv_hw_half_to_float4(h) }, "_Float16, half {h:#06x}");
            }
        }
    }

    #[test]
    fn f16_float4_to_half_matches_c_for_every_float() {
        let hw = has_float16();
        let bad = par_count(1 << 32, |lo, hi| {
            let mut bad = 0;
            for bits in lo..hi {
                let bits = bits as u32;
                let ours = float4_to_half_unchecked(f32::from_bits(bits));
                // SAFETY: pure C functions, no preconditions.
                let sw = unsafe { pgv_sw_float4_to_half(bits) };
                let hw_bad = hw && ours != unsafe { pgv_hw_float4_to_half(bits) };
                if ours != sw || hw_bad {
                    bad += 1;
                }
            }
            bad
        });
        assert_eq!(bad, 0, "f32 inputs whose half differs from C");
    }

    // halfvec_add/sub/mul round the f32 result once (halfvec.c:786, 825,
    // 864); the FLT16_SUPPORT build computes in _Float16 (halfvec.c:784, 823,
    // 862). f32 carries >= 2*11+2 significand bits, so the two agree. NaN
    // operands, which CheckElement rejects before any arithmetic, may differ
    // only in payload.
    #[test]
    fn f16_arithmetic_matches_float16_for_every_pair() {
        if !has_float16() {
            eprintln!("skipped: the C compiler has no _Float16");
            return;
        }
        let bad = par_count(1 << 16, |lo, hi| {
            let mut bad = 0;
            for a in lo..hi {
                let a = a as Half;
                let fa = half_to_float4(a);
                for b in 0..=u16::MAX {
                    let fb = half_to_float4(b);
                    // SAFETY: pure C functions, no preconditions.
                    let pairs = unsafe {
                        [
                            (float4_to_half_unchecked(fa + fb), pgv_hw_add(a, b)),
                            (float4_to_half_unchecked(fa - fb), pgv_hw_sub(a, b)),
                            (float4_to_half_unchecked(fa * fb), pgv_hw_mul(a, b)),
                        ]
                    };
                    for (ours, c) in pairs {
                        if ours != c && !(half_is_nan(ours) && half_is_nan(c)) {
                            bad += 1;
                        }
                    }
                }
            }
            bad
        });
        assert_eq!(bad, 0, "operand pairs whose result differs from _Float16");
    }
}
```

- [ ] **Step 6: Lock and run the parity tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
UT_PARITY
```
Expected:
- `Cargo.lock` gains only the `pgvector_f16_parity` package entry, with dependencies `cc` and `pgvector`.
- `UT_PARITY` reports 3 passed in well under a minute. Each test was measured at 1.5–4 s optimized on this machine.
- If `f16_arithmetic_matches_float16_for_every_pair` prints `skipped`, the compiler lacks `_Float16`. That is expected only off macOS arm64 or on GCC before 12; report it.

- [ ] **Step 7: Prove the parity test bites**

Temporarily change the `Round` condition in `float4_to_half_unchecked` from `if gr == 3 || (gr == 1 && s != 0)` to `if gr == 3`, then run `UT_PARITY` and `UT halfutils::`.
Expected:
- `f16_float4_to_half_matches_c_for_every_float` fails with a nonzero count.
- `f16_arithmetic_matches_float16_for_every_pair` fails.

Restore the line, rerun `UT_PARITY`, and expect 3 passed.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/contrib/pgvector/src/{lib,halfutils}.rs crates/contrib/pgvector_f16_parity Cargo.toml Cargo.lock
git commit -q -F - <<'EOF'
pgvector: halfutils (f16 conversions, halfvec kernels); f16 parity crate

Ports halfutils.h's software f32<->f16 routines and halfutils.c's scalar
distance kernels. pgvector_f16_parity (test-only) compiles the vendored C
routines and clang's _Float16 path and checks every f16, every f32 and
every pair of halves through + - * against the port (spec §8.2).

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 3: `sqldiff.sh`, and the `halfvec` type with its I/O and casts

**Files:**
- Create: `scripts/pgvector/sqldiff.sh`; modify `scripts/pgvector/tests/harness_test.sh`
- Create: `crates/contrib/pgvector/src/halfvec.rs`
- Modify: `crates/contrib/pgvector/src/vec.rs:173` (`vector_isspace` → `pub(crate)`)
- Modify: `crates/contrib/pgvector/src/funcs.rs` (visibility, the shared array-cast prologue, `fc_halfvec_to_vector`)
- Modify: `crates/contrib/pgvector/src/lib.rs`, `extension/vector--0.8.5.sql`

**Interfaces:**
- Consumes:
  - from Task 2: `halfutils::{Half, half_is_inf, half_is_nan, half_to_float4, float4_to_half_unchecked, float4_to_half}`;
  - `vec::{strtof_prefix, StrtofVal, VecBuilder, check_dim, check_expected_dim}`.
- Produces:
  - `scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file.sql|->`.
  - In `halfvec.rs`:
    - `pub const HALFVEC_MAX_DIM: i32`.
    - `pub struct HalfView<'a>` with `from_payload(&'a [u8]) -> PgResult<Self>`, `dim() -> usize`, `x(i) -> Half` and `xs() -> &'a [u8]`. `xs()` returns the `dim` halves as native-endian bytes.
    - `pub struct HalfBuilder<'mcx>` with `new(Mcx, dim: usize)`, `set(i, Half)`, `get(i) -> Half` and `image() -> PgVec<u8>`.
    - `pub(crate) unsafe fn arg_halfvec<'a>(&'a Fcinfo, i: usize) -> PgResult<HalfView<'a>>`.
    - `pub(crate) fn check_dim(i32)`; `fn check_dims`, `fn check_expected_dim(i32, i32)`, `fn check_element(Half)` and `fn ereport(code, msg, line, func) -> Box<PgError>`.
  - In `funcs.rs`:
    - `pub(crate) unsafe fn arg_vector`;
    - `pub(crate) unsafe fn detoasted_image`;
    - `pub(crate) struct ArraySite { file, func, ndim_line, nulls_line }`;
    - `pub(crate) fn cast_array_elems<'m>(Mcx<'m>, &[u8], &ArraySite) -> PgResult<(Oid, PgVec<'m, Datum>)>`;
    - `pub(crate) fn cast_elem_f32(Oid, Datum) -> PgResult<Option<f32>>`.
    
    Task 6 reuses the last three.

- [ ] **Step 1: Write `scripts/pgvector/sqldiff.sh`**

```bash
#!/usr/bin/env bash
# Run one SQL script on a fresh C reference and a fresh subject server and
# compare the two echoed psql sessions: M2's per-task check of each port
# (the regress, TAP and diff tiers are the gates).
# usage: sqldiff.sh {pgrust|ref} [--verbose] <file.sql | ->
#   pgrust: the subject is pgrust.  ref: the subject is the seeded C build.
#   --verbose: \set VERBOSITY verbose first, so each error's LOCATION line
#   (C function, file and line) is compared too.
#   Each side: fresh cluster, CREATE EXTENSION vector, then the script via
#   psql -a with ON_ERROR_STOP off, run from a directory holding results/
#   (the regress files' \copy target). Exit 0 when identical; otherwise the
#   unified diff is in $PGV_WORK/sqldiff/<mode>/diff.txt.
set -euo pipefail
. "$(dirname "$0")/common.sh"

usage() { die "usage: sqldiff.sh {pgrust|ref} [--verbose] <file.sql | ->"; }
[ "$#" -ge 2 ] || usage
mode="$1"
shift
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) usage ;;
esac
verbose=0
if [ "$1" = --verbose ]; then
  verbose=1
  shift
fi
[ "$#" -eq 1 ] || usage
out="$PGV_WORK/sqldiff/$mode"
rm -rf "$out"
mkdir -p "$out/results"
{
  if [ "$verbose" = 1 ]; then echo '\set VERBOSITY verbose'; fi
  if [ "$1" = - ]; then cat; else cat "$1"; fi
} >"$out/input.sql"

trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  port="$(port_for "$side")"
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$port"
  assert_identity "$side" "$port"
  pgv_psql "$port" -c 'CREATE EXTENSION vector' >/dev/null
  (cd "$out" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f input.sql) >"$out/$side.out" 2>&1 || true
  server_stop "$side"
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$side"
done
if diff -u "$out/ref.out" "$out/$subject.out" >"$out/diff.txt"; then
  echo "sqldiff ($mode): identical; transcripts in $out"
else
  echo "sqldiff ($mode): DIFFERENT; see $out/diff.txt" >&2
  head -60 "$out/diff.txt" >&2
  exit 1
fi
```
`chmod +x scripts/pgvector/sqldiff.sh`.

Add these two self-tests to `scripts/pgvector/tests/harness_test.sh`, directly above the `# --- runner ---` line:
```bash
# sqldiff, C vs C: one script gives identical sessions on the reference and
# the seeded build, error lines included.
test_sqldiff_ref() {
  if printf '%s\n' "SELECT '[1,2,3]'::vector;" "SELECT '[1,2'::vector;" |
    "$here/../sqldiff.sh" ref - >"$PGV_WORK/sqldiff-ref.log" 2>&1; then
    pass "sqldiff ref: identical"
  else
    fail "sqldiff ref: identical (see $PGV_WORK/sqldiff-ref.log)"
  fi
}

# Negative control: version() differs between C and pgrust.
test_sqldiff_detects_difference() {
  if echo 'SELECT version();' | "$here/../sqldiff.sh" pgrust - >"$PGV_WORK/sqldiff-neg.log" 2>&1; then
    fail "sqldiff flags a difference"
  elif grep -q 'DIFFERENT' "$PGV_WORK/sqldiff-neg.log"; then
    pass "sqldiff flags a difference"
  else
    fail "sqldiff flags a difference: wrong failure (see $PGV_WORK/sqldiff-neg.log)"
  fi
}
```
Run: `scripts/pgvector/tests/harness_test.sh test_sqldiff_ref test_sqldiff_detects_difference`
Expected: `ok` twice, then `all passed`.

- [ ] **Step 2: Write the failing unit tests**

Create `crates/contrib/pgvector/src/halfvec.rs` containing only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Vec<f32>, String> {
        let mut x = Box::new([0 as Half; HALFVEC_MAX_DIM as usize]);
        match parse_halfvec(s.as_bytes(), -1, &mut x) {
            Ok(n) => Ok(x[..n].iter().map(|h| half_to_float4(*h)).collect()),
            Err(e) => Err(e.message().to_string()),
        }
    }

    // halfvec.out: every element rounds to half.
    #[test]
    fn parse_rounds_to_half() {
        assert_eq!(parse("[1,2,3]").unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(parse(" [ 1,  2 ,    3  ] ").unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(parse("[1.23456]").unwrap(), vec![1.234375]);
        assert_eq!(parse("[65519,-65519]").unwrap(), vec![65504.0, -65504.0]);
        assert_eq!(parse("[1e-8,-1e-8]").unwrap(), vec![0.0, -0.0]);
        assert_eq!(parse("[1e-46,1]").unwrap(), vec![0.0, 1.0]);
        assert_eq!(parse("[0x1p-3]").unwrap(), vec![0.125]);
    }

    #[test]
    fn parse_errors_match_halfvec_out() {
        let err = |s: &str| parse(s).unwrap_err();
        assert_eq!(err("[hello,1]"), "invalid input syntax for type halfvec: \"[hello,1]\"");
        assert_eq!(err("[NaN,1]"), "NaN not allowed in halfvec");
        assert_eq!(err("[Infinity,1]"), "infinite value not allowed in halfvec");
        assert_eq!(err("[65520,-65520]"), "\"65520\" is out of range for type halfvec");
        assert_eq!(err("[4e38,1]"), "\"4e38\" is out of range for type halfvec");
        assert_eq!(err("[]"), "halfvec must have at least 1 dimension");
        assert_eq!(err("[1,2,3"), "invalid input syntax for type halfvec: \"[1,2,3\"");
        assert_eq!(err("1,2,3"), "invalid input syntax for type halfvec: \"1,2,3\"");
        assert_eq!(err(""), "invalid input syntax for type halfvec: \"\"");
    }

    // Review Focus 1: every truncation of a valid literal is a clean error
    // or a value, never a panic.
    #[test]
    fn parse_survives_every_prefix() {
        let lit = " [ 1.5e-3 , -2 ,0x1p-3, 65504 ] ";
        for end in 0..=lit.len() {
            let _ = parse(&lit[..end]);
        }
        let want = vec![half_to_float4(float4_to_half_unchecked(1.5e-3)), -2.0, 0.125, 65504.0];
        assert_eq!(parse(lit).unwrap(), want);
    }

    fn recv(bytes: &[u8], typmod: i32) -> Result<Vec<f32>, String> {
        let ctx = mcx::MemoryContext::new("halfvec-test");
        let mcx = ctx.mcx();
        let mut buf = stringinfo::StringInfo::new_in(mcx).unwrap();
        buf.append_bytes(bytes).unwrap();
        match halfvec_recv_body(mcx, &mut buf, typmod) {
            Ok(img) => {
                let v = HalfView::from_payload(&img[4..]).unwrap();
                Ok((0..v.dim()).map(|i| half_to_float4(v.x(i))).collect())
            }
            Err(e) => Err(e.message().to_string()),
        }
    }

    fn msg(dim: i16, unused: i16, xs: &[u16]) -> Vec<u8> {
        let mut v = dim.to_be_bytes().to_vec();
        v.extend(unused.to_be_bytes());
        for x in xs {
            v.extend(x.to_be_bytes());
        }
        v
    }

    // Review Focus 2: binary input is validated like halfvec_recv
    // (halfvec.c:381-397).
    #[test]
    fn recv_validates_like_c() {
        assert_eq!(recv(&msg(2, 0, &[0x3C00, 0xC000]), -1).unwrap(), vec![1.0, -2.0]);
        assert_eq!(recv(&msg(0, 0, &[]), -1).unwrap_err(), "halfvec must have at least 1 dimension");
        assert_eq!(recv(&msg(-1, 0, &[]), -1).unwrap_err(), "halfvec must have at least 1 dimension");
        assert_eq!(recv(&msg(2, 0, &[0x3C00, 0x3C00]), 3).unwrap_err(), "expected 3 dimensions, not 2");
        assert_eq!(recv(&msg(1, 7, &[0x3C00]), -1).unwrap_err(), "expected unused to be 0, not 7");
        assert_eq!(recv(&msg(1, 0, &[0x7E00]), -1).unwrap_err(), "NaN not allowed in halfvec");
        assert_eq!(recv(&msg(1, 0, &[0xFC00]), -1).unwrap_err(), "infinite value not allowed in halfvec");
        assert_eq!(recv(&msg(3, 0, &[0x3C00]), -1).unwrap_err(), "insufficient data left in message");
    }

    #[test]
    fn from_payload_rejects_corrupt_images() {
        let corrupt = |p: &[u8]| HalfView::from_payload(p).err().unwrap().message().to_string();
        assert_eq!(corrupt(&[0xFF, 0xFF, 0, 0]), "corrupt halfvec datum");
        assert_eq!(corrupt(&[0, 0, 0, 0]), "corrupt halfvec datum");
        let mut big = vec![0u8; 4 + 2 * 16001];
        big[..2].copy_from_slice(&16001i16.to_ne_bytes());
        assert_eq!(corrupt(&big), "corrupt halfvec datum");
        let mut short = vec![0u8; 4 + 2];
        short[..2].copy_from_slice(&2i16.to_ne_bytes());
        assert_eq!(corrupt(&short), "corrupt halfvec datum");
        let mut ok = vec![0u8; 4 + 4];
        ok[..2].copy_from_slice(&2i16.to_ne_bytes());
        assert_eq!(HalfView::from_payload(&ok).unwrap().dim(), 2);
    }
}
```
Add `pub mod halfvec;` to `lib.rs` (after `halfutils`). Run `UT halfvec::`. Expected: compile errors, `cannot find function 'parse_halfvec'` and the like.

- [ ] **Step 3: Make the shared helpers visible and add the array-cast prologue**

In `crates/contrib/pgvector/src/vec.rs`, change `fn vector_isspace(ch: u8) -> bool {` to `pub(crate) fn vector_isspace(ch: u8) -> bool {`.

In `crates/contrib/pgvector/src/funcs.rs`:
- Change `unsafe fn arg_vector<'a>(` to `pub(crate) unsafe fn arg_vector<'a>(`.
- Change `unsafe fn detoasted_image<'m>(` to `pub(crate) unsafe fn detoasted_image<'m>(`.
- Insert directly after `fc_array_to_vector`. Every name it uses is already imported in `funcs.rs`.
```rust
/// Where an array cast's two up-front ereports sit in its C file.
pub(crate) struct ArraySite {
    pub file: &'static str,
    pub func: &'static str,
    pub ndim_line: i32,
    pub nulls_line: i32,
}

// The array casts' shared prologue (halfvec.c:453-464, sparsevec.c:707-718):
// reject a multi-dimensional array or one holding NULLs, then deconstruct it.
// fc_array_to_vector keeps its own copy (pre-M2 code, unchanged).
pub(crate) fn cast_array_elems<'m>(
    mcx: Mcx<'m>,
    arr: &[u8],
    site: &ArraySite,
) -> PgResult<(Oid, PgVec<'m, Datum>)> {
    if arrayfuncs::arr_ndim(arr) > 1 {
        return Err(PgError::error("array must be 1-D")
            .with_sqlstate(ERRCODE_DATA_EXCEPTION)
            .with_location(site.file, site.ndim_line, site.func)
            .into());
    }
    if arrayfuncs::arr_hasnull(arr) && arrayfuncs::array_contains_nulls(arr) {
        return Err(PgError::error("array must not contain nulls")
            .with_sqlstate(ERRCODE_NULL_VALUE_NOT_ALLOWED)
            .with_location(site.file, site.nulls_line, site.func)
            .into());
    }
    let elemtype: Oid = arrayfuncs::arr_elemtype(arr);
    // numeric is not in builtin_meta: varlena, int-aligned.
    let (elmlen, elmbyval, elmalign) = if elemtype == NUMERICOID {
        (-1, false, b'i')
    } else {
        arrayfuncs::construct::builtin_meta(elemtype)?
    };
    let (elems, _nulls) = arrayfuncs::deconstruct_array(mcx, arr, elmlen, elmbyval, elmalign, true)?;
    Ok((elemtype, elems))
}

// One array element as the float the cast stores or rounds from: C converts
// int4 and float8 to float implicitly (halfvec.c:474, 479; sparsevec.c:763)
// and numeric through numeric_float4. None: unsupported element type.
pub(crate) fn cast_elem_f32(elemtype: Oid, d: Datum) -> PgResult<Option<f32>> {
    Ok(Some(match elemtype {
        INT4OID => d.as_i32() as f32,
        FLOAT8OID => d.as_f64() as f32,
        FLOAT4OID => d.as_f32(),
        NUMERICOID => {
            let p = d.as_usize() as *const u8;
            // SAFETY: non-null numeric element datum inside the array image.
            let payload = unsafe {
                let total = varatt::varsize_any(p);
                let hdr = if varatt::varatt_is_1b(p) { 1 } else { 4 };
                core::slice::from_raw_parts(p.add(hdr), total - hdr)
            };
            adt_numeric::ops::numeric_float4(adt_numeric::Num::from_payload(payload))?
        }
        _ => return Ok(None),
    }))
}
```
- Insert directly after `fc_vector_to_float4`:
```rust
// halfvec_to_vector (vector.c:538-557).
pub fn fc_halfvec_to_vector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { crate::halfvec::arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_dim(v.dim())?;
    check_expected_dim(typmod, v.dim())?;
    let mut b = VecBuilder::new(fcinfo.result_mcx(), v.dim())?;
    for i in 0..v.dim() {
        b.set(i, crate::halfutils::half_to_float4(v.x(i)));
    }
    Ok(image_datum(b.image()))
}
```

- [ ] **Step 4: Implement `halfvec.rs` (type, I/O, casts)**

Insert above the test module:
```rust
//! halfvec.c (pgvector 0.8.7): the halfvec type, a varlena of
//! `i16 dim, i16 unused, half x[dim]` (halfvec.h:62-68) holding at most
//! 16,000 dimensions; its I/O, casts, functions and aggregates.
//! halfvec_to_vector lives in funcs.rs (vector.c); C keeps the sparsevec
//! casts other than sparsevec_to_halfvec in sparsevec.c, and so do we.
//!
//! Arithmetic widens to f32 and rounds once (halfvec.c's non-FLT16_SUPPORT
//! path); that equals C's _Float16 arithmetic bit for bit
//! (pgvector_f16_parity). Errors carry their halfvec.c location.

use datum::Datum;
use mcx::{Mcx, PgVec};
use stringinfo::StringInfo;
use types_core::FLOAT4OID;
use types_error::{
    PgError, PgResult, SqlState, ERRCODE_DATA_EXCEPTION, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_INVALID_TEXT_REPRESENTATION, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED,
};
use types_fmgr::{cstring_result, FmgrInfo, FunctionCallInfoBaseData as Fcinfo};

use crate::funcs::{arg_vector, cast_array_elems, cast_elem_f32, detoasted_image, image_datum, ArraySite};
use crate::halfutils::{float4_to_half, float4_to_half_unchecked, half_is_inf, half_is_nan, half_to_float4, Half};
use crate::vec::{strtof_prefix, vector_isspace as halfvec_isspace, StrtofVal};

pub const HALFVEC_MAX_DIM: i32 = 16000;

// Payload layout after the 4-byte varlena header: i16 dim, i16 unused, half x[dim].
pub const HALFVEC_PAYLOAD_HDR: usize = 4;

// An ereport/elog of halfvec.c, located at the call's last line.
#[cold]
pub(crate) fn ereport(code: SqlState, msg: impl Into<String>, line: i32, func: &'static str) -> Box<PgError> {
    PgError::error(msg).with_sqlstate(code).with_location("halfvec.c", line, func).into()
}

#[derive(Clone, Copy)]
pub struct HalfView<'a> {
    data: &'a [u8],
}

impl<'a> HalfView<'a> {
    pub fn from_payload(data: &'a [u8]) -> PgResult<HalfView<'a>> {
        // Like VecView: check the raw i16 dim before widening it, so a corrupt
        // image never reaches the kernels with a bogus dim().
        if data.len() < HALFVEC_PAYLOAD_HDR {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        let raw_dim = i16::from_ne_bytes([data[0], data[1]]);
        if raw_dim < 1 || raw_dim as i32 > HALFVEC_MAX_DIM {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        if data.len() < HALFVEC_PAYLOAD_HDR + 2 * raw_dim as usize {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        Ok(HalfView { data })
    }

    #[inline]
    pub fn dim(&self) -> usize {
        i16::from_ne_bytes([self.data[0], self.data[1]]) as usize
    }

    #[inline]
    pub fn x(&self, i: usize) -> Half {
        let off = HALFVEC_PAYLOAD_HDR + 2 * i;
        Half::from_ne_bytes([self.data[off], self.data[off + 1]])
    }

    /// The `dim` halves as native-endian bytes, for the halfutils kernels.
    #[inline]
    pub fn xs(&self) -> &'a [u8] {
        &self.data[HALFVEC_PAYLOAD_HDR..HALFVEC_PAYLOAD_HDR + 2 * self.dim()]
    }
}

pub struct HalfBuilder<'mcx> {
    img: PgVec<'mcx, u8>,
}

// InitHalfVector (halfvec.c:132-144): full varlena image, zeroed halves.
impl<'mcx> HalfBuilder<'mcx> {
    pub fn new(mcx: Mcx<'mcx>, dim: usize) -> PgResult<HalfBuilder<'mcx>> {
        let size = 4 + HALFVEC_PAYLOAD_HDR + dim * 2;
        let mut img: PgVec<'mcx, u8> = mcx::vec_with_capacity_in(mcx, size)?;
        img.resize(size, 0);
        img[..4].copy_from_slice(&((size as u32) << 2).to_ne_bytes());
        img[4..6].copy_from_slice(&(dim as i16).to_ne_bytes());
        Ok(HalfBuilder { img })
    }

    #[inline]
    pub fn set(&mut self, i: usize, v: Half) {
        let off = 4 + HALFVEC_PAYLOAD_HDR + 2 * i;
        self.img[off..off + 2].copy_from_slice(&v.to_ne_bytes());
    }

    #[inline]
    pub fn get(&self, i: usize) -> Half {
        let off = 4 + HALFVEC_PAYLOAD_HDR + 2 * i;
        Half::from_ne_bytes([self.img[off], self.img[off + 1]])
    }

    pub fn image(self) -> PgVec<'mcx, u8> {
        self.img
    }
}

// SAFETY contract of callers: arg i is a non-null halfvec varlena (strict fns).
pub(crate) unsafe fn arg_halfvec<'a>(fcinfo: &'a Fcinfo, i: usize) -> PgResult<HalfView<'a>> {
    let v = unsafe { fcinfo.arg_varlena_packed(i)? };
    HalfView::from_payload(v.data())
}

// CheckDims (halfvec.c:74-81).
fn check_dims(a: &HalfView<'_>, b: &HalfView<'_>) -> PgResult<()> {
    if a.dim() != b.dim() {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("different halfvec dimensions {} and {}", a.dim(), b.dim()),
            80,
            "CheckDims",
        ));
    }
    Ok(())
}

// CheckExpectedDim (halfvec.c:86-93).
fn check_expected_dim(typmod: i32, dim: i32) -> PgResult<()> {
    if typmod != -1 && typmod != dim {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected {typmod} dimensions, not {dim}"),
            92,
            "CheckExpectedDim",
        ));
    }
    Ok(())
}

// CheckDim (halfvec.c:98-110). Signed like C's int: a negative dimension
// (an i16 off the wire, a subvector span) reports "at least 1 dimension".
pub(crate) fn check_dim(dim: i32) -> PgResult<()> {
    if dim < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 104, "CheckDim"));
    }
    if dim > HALFVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("halfvec cannot have more than {HALFVEC_MAX_DIM} dimensions"),
            109,
            "CheckDim",
        ));
    }
    Ok(())
}

// CheckElement (halfvec.c:115-127).
fn check_element(value: Half) -> PgResult<()> {
    if half_is_nan(value) {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "NaN not allowed in halfvec", 121, "CheckElement"));
    }
    if half_is_inf(value) {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "infinite value not allowed in halfvec", 126, "CheckElement"));
    }
    Ok(())
}

#[cold]
fn invalid_text(lit: &[u8], detail: Option<&str>, line: i32) -> Box<PgError> {
    let mut e = PgError::error(format!(
        "invalid input syntax for type halfvec: \"{}\"",
        String::from_utf8_lossy(lit)
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("halfvec.c", line, "halfvec_in");
    if let Some(d) = detail {
        e = e.with_detail(d);
    }
    e.into()
}

// halfvec_in's parse (halfvec.c:182-286): strtof per element, each rounded
// to half; an element finite as a float but infinite as a half is out of
// range.
pub fn parse_halfvec(lit: &[u8], typmod: i32, x: &mut [Half; HALFVEC_MAX_DIM as usize]) -> PgResult<usize> {
    let n = lit.len();
    let mut pt = 0usize;
    let mut dim = 0usize;

    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'[' {
        return Err(invalid_text(lit, Some("Vector contents must start with \"[\"."), 198));
    }
    pt += 1;
    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt < n && lit[pt] == b']' {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 208, "halfvec_in"));
    }

    loop {
        if dim == HALFVEC_MAX_DIM as usize {
            return Err(ereport(
                ERRCODE_PROGRAM_LIMIT_EXCEEDED,
                format!("halfvec cannot have more than {HALFVEC_MAX_DIM} dimensions"),
                218,
                "halfvec_in",
            ));
        }
        while pt < n && halfvec_isspace(lit[pt]) {
            pt += 1;
        }
        // Check for empty string like float4in
        if pt >= n {
            return Err(invalid_text(lit, None, 227));
        }
        let Some((val, consumed)) = strtof_prefix(&lit[pt..]) else {
            return Err(invalid_text(lit, None, 237));
        };
        // Check for range error like float4in
        let out_of_range = match val {
            StrtofVal::Erange(_) => true,
            StrtofVal::Ok(v) => {
                x[dim] = float4_to_half_unchecked(v);
                half_is_inf(x[dim]) && !v.is_infinite()
            }
        };
        if out_of_range {
            return Err(ereport(
                ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
                format!(
                    "\"{}\" is out of range for type halfvec",
                    String::from_utf8_lossy(&lit[pt..pt + consumed])
                ),
                245,
                "halfvec_in",
            ));
        }
        check_element(x[dim])?;
        dim += 1;
        pt += consumed;

        while pt < n && halfvec_isspace(lit[pt]) {
            pt += 1;
        }
        if pt < n && lit[pt] == b',' {
            pt += 1;
        } else if pt < n && lit[pt] == b']' {
            pt += 1;
            break;
        } else {
            return Err(invalid_text(lit, None, 265));
        }
    }

    // Only whitespace is allowed after the closing brace
    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt != n {
        return Err(invalid_text(lit, Some("Junk after closing right brace."), 276));
    }

    check_dim(dim as i32)?;
    check_expected_dim(typmod, dim as i32)?;
    Ok(dim)
}

// halfvec_in (halfvec.c:180-286).
pub fn fc_halfvec_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict input fn — arg0 cstring, arg2 typmod.
    let lit = unsafe { fcinfo.arg_cstring(0) }.to_bytes();
    let typmod = fcinfo.arg_i32(2);
    let mut x = [0 as Half; HALFVEC_MAX_DIM as usize];
    let dim = parse_halfvec(lit, typmod, &mut x)?;
    let mut b = HalfBuilder::new(fcinfo.result_mcx(), dim)?;
    for (i, v) in x[..dim].iter().enumerate() {
        b.set(i, *v);
    }
    Ok(image_datum(b.image()))
}

// halfvec_out (halfvec.c:294-335): the shortest float text of each half.
pub fn fc_halfvec_out(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let dim = v.dim();
    let mcx = fcinfo.result_mcx();
    let mut out: PgVec<'_, u8> = mcx::vec_with_capacity_in(mcx, ryu::FLOAT_SHORTEST_DECIMAL_LEN * dim + 3)?;
    let mut scratch = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
    mcx::vec_append_bytes(&mut out, b"[")?;
    for i in 0..dim {
        if i > 0 {
            mcx::vec_append_bytes(&mut out, b",")?;
        }
        let n = ryu::float_to_shortest_decimal_bufn(half_to_float4(v.x(i)), &mut scratch);
        mcx::vec_append_bytes(&mut out, &scratch[..n])?;
    }
    mcx::vec_append_bytes(&mut out, b"]\0")?;
    Ok(cstring_result(out))
}

// halfvec_typmod_in (halfvec.c:340-366).
pub fn fc_halfvec_typmod_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 cstring[].
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let tl = arrayfuncs::array_get_integer_typmods(mcx, arr)?;
    if tl.len() != 1 {
        return Err(ereport(ERRCODE_INVALID_PARAMETER_VALUE, "invalid type modifier", 353, "halfvec_typmod_in"));
    }
    if tl[0] < 1 {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            "dimensions for type halfvec must be at least 1",
            358,
            "halfvec_typmod_in",
        ));
    }
    if tl[0] > HALFVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            format!("dimensions for type halfvec cannot exceed {HALFVEC_MAX_DIM}"),
            363,
            "halfvec_typmod_in",
        ));
    }
    Ok(Datum::from_i32(tl[0]))
}

// halfvec_recv's body (halfvec.c:373-400): i16 dim, i16 unused, then dim raw
// halves (pq_getmsghalf, halfvec.c:42-53).
pub(crate) fn halfvec_recv_body<'m>(mcx: Mcx<'m>, buf: &mut StringInfo<'_>, typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let dim = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    let unused = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    check_dim(dim as i32)?;
    check_expected_dim(typmod, dim as i32)?;
    if unused != 0 {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected unused to be 0, not {unused}"),
            390,
            "halfvec_recv",
        ));
    }
    let mut b = HalfBuilder::new(mcx, dim as usize)?;
    for i in 0..dim as usize {
        let x = pqformat::pq_getmsgint(buf, 2)? as Half;
        check_element(x)?;
        b.set(i, x);
    }
    Ok(b.image())
}

// halfvec_recv (halfvec.c:371-400).
pub fn fc_halfvec_recv(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: recv arg0 is the live StringInfo per the recv ABI.
    let buf = unsafe { fcinfo.arg_stringinfo(0) };
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(halfvec_recv_body(fcinfo.result_mcx(), buf, typmod)?))
}

// halfvec_send (halfvec.c:405-419): dim, unused, then each half's raw bits
// (pq_sendhalf, halfvec.c:58-69).
pub fn fc_halfvec_send(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut buf = pqformat::pq_begintypsend(fcinfo.result_mcx())?;
    pqformat::pq_sendint16(&mut buf, v.dim() as u16)?;
    pqformat::pq_sendint16(&mut buf, 0)?; // vec->unused, always zero
    for i in 0..v.dim() {
        pqformat::pq_sendint16(&mut buf, v.x(i))?;
    }
    Ok(types_fmgr::varlena_result(pqformat::pq_endtypsend(buf)))
}

// halfvec (halfvec.c:425-435): applies the type modifier.
pub fn fc_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_expected_dim(typmod, v.dim() as i32)?;
    Ok(fcinfo.arg(0))
}

// array_to_halfvec (halfvec.c:440-509).
pub fn fc_array_to_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 array, arg1 typmod.
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let typmod = fcinfo.arg_i32(1);
    let site = ArraySite { file: "halfvec.c", func: "array_to_halfvec", ndim_line: 456, nulls_line: 461 };
    let (elemtype, elems) = cast_array_elems(mcx, arr, &site)?;
    check_dim(elems.len() as i32)?;
    check_expected_dim(typmod, elems.len() as i32)?;
    let mut b = HalfBuilder::new(mcx, elems.len())?;
    for (i, d) in elems.iter().enumerate() {
        let Some(v) = cast_elem_f32(elemtype, *d)? else {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "unsupported array type", 495, "array_to_halfvec"));
        };
        b.set(i, float4_to_half(v)?);
    }
    // Check elements
    for i in 0..elems.len() {
        check_element(b.get(i))?;
    }
    Ok(image_datum(b.image()))
}

// halfvec_to_float4 (halfvec.c:514-533).
pub fn fc_halfvec_to_float4(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut datums: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, v.dim())?;
    for i in 0..v.dim() {
        datums.push(Datum::from_f32(half_to_float4(v.x(i))));
    }
    // Use TYPALIGN_INT for float4
    let img = arrayfuncs::construct_array(mcx, &datums, FLOAT4OID, 4, true, b'i')?;
    Ok(image_datum(img))
}

// vector_to_halfvec (halfvec.c:538-555).
pub fn fc_vector_to_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, arg1 typmod.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_dim(v.dim() as i32)?;
    check_expected_dim(typmod, v.dim() as i32)?;
    let mut b = HalfBuilder::new(fcinfo.result_mcx(), v.dim())?;
    for i in 0..v.dim() {
        b.set(i, float4_to_half(v.x(i))?);
    }
    Ok(image_datum(b.image()))
}
```

- [ ] **Step 5: Register the symbols and append the SQL**

Add to `lookup` in `lib.rs`, directly before the `hamming_distance` arm:
```rust
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
```
Then:
```bash
{ echo; scripts/pgvector/upstream-sql.sh 'halfvec type' 'halfvec cast functions' 'halfvec casts' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
```

- [ ] **Step 6: Run the unit tests**

Run: `UT halfvec:: lookup_cover funcs::`
Expected:
- The 5 halfvec tests pass.
- The 2 `lookup_cover` tests pass.
- The 2 existing `funcs::tests` pass.

- [ ] **Step 7: Compare with C**

```bash
scripts/pgvector/build-pgrust.sh
T=crates/pgvector-0.8.7-reference/test/sql
sed -n '1,38p' $T/halfvec.sql | scripts/pgvector/sqldiff.sh pgrust -
sed -n -e '25,50p' -e '79,86p' -e '93,102p' -e '128p' $T/cast.sql | scripts/pgvector/sqldiff.sh pgrust -
```
Expected: `sqldiff (pgrust): identical` twice. Together the two slices cover:
- halfvec I/O, typmods and arrays (`halfvec.sql` 1–38);
- every `cast.sql` statement that involves only `halfvec` and `vector`.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
scripts/pgvector/run-regress.sh pgrust bit vector_type hnsw_vector
git add scripts/pgvector/sqldiff.sh scripts/pgvector/tests/harness_test.sh crates/contrib/pgvector
git commit -q -F - <<'EOF'
pgvector: halfvec type, I/O and casts; sqldiff.sh

Ports halfvec.c's type half: text and binary I/O, typmods, and the casts
to and from vector, real[] and arrays (plus vector.c's halfvec_to_vector).
Every error carries its halfvec.c location. sqldiff.sh runs one script
on fresh C and pgrust servers and diffs the sessions.

Co-Authored-By: <the implementing model>
EOF
```
Expected before the commit: the three regress files are `ok`.

---

### Task 4: `halfvec` functions, operators, aggregates and btree opclass

**Files:**
- Modify: `crates/contrib/pgvector/src/halfvec.rs` (functions; extend the test module)
- Modify: `crates/contrib/pgvector/src/funcs.rs` (`StateArray` and `build_state_array` → `pub(crate)`)
- Modify: `crates/contrib/pgvector/src/lib.rs`, `extension/vector--0.8.5.sql`

**Interfaces:**
- Consumes:
  - Task 3's `HalfView`, `HalfBuilder`, `arg_halfvec`, `check_*` and `ereport`;
  - Task 2's kernels and `half_is_zero`;
  - Task 1's `bitvec::init_bit_vector`;
  - `funcs::{StateArray, build_state_array}`.
- Produces: the 25 `fc_halfvec_*` functions listed in Step 4, and `fn halfvec_cmp_internal(&HalfView, &HalfView) -> i32`.

- [ ] **Step 1: Write the failing unit test**

Append inside `halfvec.rs`'s `mod tests`:
```rust
    fn image<'m>(m: mcx::Mcx<'m>, v: &[f32]) -> mcx::PgVec<'m, u8> {
        let mut b = HalfBuilder::new(m, v.len()).unwrap();
        for (i, x) in v.iter().enumerate() {
            b.set(i, float4_to_half_unchecked(*x));
        }
        b.image()
    }

    // halfvec.out: halfvec_cmp('[1,2]', '[1,2,3]') = -1,
    // halfvec_cmp('[2,3]', '[1,2,3]') = 1; values before dimensions.
    #[test]
    fn cmp_compares_values_before_dimensions() {
        let ctx = mcx::MemoryContext::new("halfvec-test");
        let m = ctx.mcx();
        let (a, b, c) = (image(m, &[1.0, 2.0]), image(m, &[1.0, 2.0, 3.0]), image(m, &[2.0, 3.0]));
        let view = |img: &[u8]| HalfView::from_payload(&img[4..]).unwrap();
        assert_eq!(halfvec_cmp_internal(&view(&a), &view(&b)), -1);
        assert_eq!(halfvec_cmp_internal(&view(&c), &view(&b)), 1);
        assert_eq!(halfvec_cmp_internal(&view(&a), &view(&a)), 0);
        let (z, nz) = (image(m, &[0.0]), image(m, &[-0.0]));
        assert_eq!(halfvec_cmp_internal(&view(&z), &view(&nz)), 0);
    }
```
Run `UT halfvec::`. Expected: a compile error, `cannot find function 'halfvec_cmp_internal'`.

- [ ] **Step 2: Make the aggregate state helpers visible**

In `funcs.rs`:
- change `struct StateArray<'a> {` to `pub(crate) struct StateArray<'a> {`;
- prefix its methods `fn check`, `fn state_dims` and `fn value` with `pub(crate)`;
- change `fn build_state_array<'m>(` to `pub(crate) fn build_state_array<'m>(`.

- [ ] **Step 3: Implement the functions**

Extend the imports at the top of `halfvec.rs`:
```rust
use crate::funcs::{build_state_array, StateArray};
use crate::halfutils::{
    half_is_zero, halfvec_cosine_similarity, halfvec_inner_product, halfvec_l1_distance,
    halfvec_l2_squared_distance,
};
```
Then insert before the test module:
```rust
fn halfvec_2arg(fcinfo: &Fcinfo) -> PgResult<(HalfView<'_>, HalfView<'_>)> {
    // SAFETY: strict fns — args 0 and 1 are halfvecs.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    let b = unsafe { arg_halfvec(fcinfo, 1)? };
    Ok((a, b))
}

// halfvec_l2_distance (halfvec.c:560-570).
pub fn fc_halfvec_l2_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64((halfvec_l2_squared_distance(a.dim(), a.xs(), b.xs()) as f64).sqrt()))
}

// halfvec_l2_squared_distance (halfvec.c:575-585).
pub fn fc_halfvec_l2_squared_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(halfvec_l2_squared_distance(a.dim(), a.xs(), b.xs()) as f64))
}

// halfvec_inner_product (halfvec.c:590-600).
pub fn fc_halfvec_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(halfvec_inner_product(a.dim(), a.xs(), b.xs()) as f64))
}

// halfvec_negative_inner_product (halfvec.c:605-615).
pub fn fc_halfvec_negative_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(-halfvec_inner_product(a.dim(), a.xs(), b.xs()) as f64))
}

// halfvec_cosine_distance (halfvec.c:620-645).
pub fn fc_halfvec_cosine_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut similarity = halfvec_cosine_similarity(a.dim(), a.xs(), b.xs());
    // Keep in range
    if similarity > 1.0 {
        similarity = 1.0;
    } else if similarity < -1.0 {
        similarity = -1.0;
    }
    Ok(Datum::from_f64(1.0 - similarity))
}

// halfvec_spherical_distance (halfvec.c:652-671).
pub fn fc_halfvec_spherical_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut distance = halfvec_inner_product(a.dim(), a.xs(), b.xs()) as f64;
    // Prevent NaN with acos with loss of precision
    if distance > 1.0 {
        distance = 1.0;
    } else if distance < -1.0 {
        distance = -1.0;
    }
    Ok(Datum::from_f64(distance.acos() / core::f64::consts::PI))
}

// halfvec_l1_distance (halfvec.c:676-686).
pub fn fc_halfvec_l1_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(halfvec_l1_distance(a.dim(), a.xs(), b.xs()) as f64))
}

// halfvec_vector_dims (halfvec.c:691-698).
pub fn fc_halfvec_vector_dims(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    Ok(Datum::from_i32(a.dim() as i32))
}

// halfvec_l2_norm (halfvec.c:703-720).
pub fn fc_halfvec_l2_norm(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut norm = 0.0f64;
    for i in 0..a.dim() {
        let axi = half_to_float4(a.x(i)) as f64;
        norm += axi * axi;
    }
    Ok(Datum::from_f64(norm.sqrt()))
}

// halfvec_l2_normalize (halfvec.c:725-759).
pub fn fc_halfvec_l2_normalize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut r = HalfBuilder::new(fcinfo.result_mcx(), a.dim())?;
    let mut norm = 0.0f64;
    for i in 0..a.dim() {
        norm += half_to_float4(a.x(i)) as f64 * half_to_float4(a.x(i)) as f64;
    }
    norm = norm.sqrt();
    // Return zero vector for zero norm
    if norm > 0.0 {
        for i in 0..a.dim() {
            // C passes the double quotient to Float4ToHalfUnchecked(float).
            r.set(i, float4_to_half_unchecked((half_to_float4(a.x(i)) as f64 / norm) as f32));
        }
        // Check for overflow
        for i in 0..a.dim() {
            if half_is_inf(r.get(i)) {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
        }
    }
    Ok(image_datum(r.image()))
}

// halfvec_add/sub/mul (halfvec.c:764-879): each result widens to f32 and
// rounds once (equal to C's _Float16 arithmetic, pgvector_f16_parity).
fn elementwise(fcinfo: &mut Fcinfo, op: impl Fn(f32, f32) -> f32, check_underflow: bool) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut r = HalfBuilder::new(fcinfo.result_mcx(), a.dim())?;
    for i in 0..a.dim() {
        r.set(i, float4_to_half_unchecked(op(half_to_float4(a.x(i)), half_to_float4(b.x(i)))));
    }
    // Check for overflow (and, for mul, underflow)
    for i in 0..a.dim() {
        if half_is_inf(r.get(i)) {
            return Err(Box::new(adt_float::float_overflow_error()));
        }
        if check_underflow && half_is_zero(r.get(i)) && !(half_is_zero(a.x(i)) || half_is_zero(b.x(i))) {
            return Err(Box::new(adt_float::float_underflow_error()));
        }
    }
    Ok(image_datum(r.image()))
}

// halfvec_add (halfvec.c:764-798).
pub fn fc_halfvec_add(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x + y, false)
}

// halfvec_sub (halfvec.c:803-837).
pub fn fc_halfvec_sub(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x - y, false)
}

// halfvec_mul (halfvec.c:842-879).
pub fn fc_halfvec_mul(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x * y, true)
}

// halfvec_concat (halfvec.c:884-903).
pub fn fc_halfvec_concat(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    let dim = a.dim() + b.dim();
    check_dim(dim as i32)?;
    let mut r = HalfBuilder::new(fcinfo.result_mcx(), dim)?;
    for i in 0..a.dim() {
        r.set(i, a.x(i));
    }
    for i in 0..b.dim() {
        r.set(a.dim() + i, b.x(i));
    }
    Ok(image_datum(r.image()))
}

// halfvec_binary_quantize (halfvec.c:908-934): bit i is set when x[i] > 0.
pub fn fc_halfvec_binary_quantize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut img = crate::bitvec::init_bit_vector(fcinfo.result_mcx(), a.dim())?;
    for i in 0..a.dim() {
        if half_to_float4(a.x(i)) > 0.0 {
            img[8 + i / 8] |= 1 << (7 - (i % 8));
        }
    }
    Ok(image_datum(img))
}

// halfvec_subvector (halfvec.c:939-981).
pub fn fc_halfvec_subvector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, args 1-2 int4.
    let a = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut start = fcinfo.arg_i32(1);
    let count = fcinfo.arg_i32(2);
    if count < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 954, "halfvec_subvector"));
    }
    let adim = a.dim() as i32;
    // Check if (start + count > a->dim), avoiding integer overflow. a->dim
    // and count are both positive, so a->dim - count won't overflow.
    let end = if start > adim - count { adim + 1 } else { start + count };
    // Indexing starts at 1, like substring
    if start < 1 {
        start = 1;
    } else if start > adim {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 971, "halfvec_subvector"));
    }
    let dim = end - start;
    check_dim(dim)?;
    let mut r = HalfBuilder::new(fcinfo.result_mcx(), dim as usize)?;
    for i in 0..dim as usize {
        r.set(i, a.x((start - 1) as usize + i));
    }
    Ok(image_datum(r.image()))
}

// halfvec_cmp_internal (halfvec.c:986-1008).
fn halfvec_cmp_internal(a: &HalfView<'_>, b: &HalfView<'_>) -> i32 {
    let dim = a.dim().min(b.dim());
    // Check values before dimensions to be consistent with Postgres arrays
    for i in 0..dim {
        if half_to_float4(a.x(i)) < half_to_float4(b.x(i)) {
            return -1;
        }
        if half_to_float4(a.x(i)) > half_to_float4(b.x(i)) {
            return 1;
        }
    }
    if a.dim() < b.dim() {
        return -1;
    }
    if a.dim() > b.dim() {
        return 1;
    }
    0
}

// halfvec_lt (halfvec.c:1013-1021).
pub fn fc_halfvec_lt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) < 0))
}

// halfvec_le (halfvec.c:1026-1034).
pub fn fc_halfvec_le(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) <= 0))
}

// halfvec_eq (halfvec.c:1039-1047).
pub fn fc_halfvec_eq(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) == 0))
}

// halfvec_ne (halfvec.c:1052-1060).
pub fn fc_halfvec_ne(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) != 0))
}

// halfvec_ge (halfvec.c:1065-1073).
pub fn fc_halfvec_ge(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) >= 0))
}

// halfvec_gt (halfvec.c:1078-1086).
pub fn fc_halfvec_gt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_bool(halfvec_cmp_internal(&a, &b) > 0))
}

// halfvec_cmp (halfvec.c:1091-1099).
pub fn fc_halfvec_cmp(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = halfvec_2arg(fcinfo)?;
    Ok(Datum::from_i32(halfvec_cmp_internal(&a, &b)))
}

// CheckStateArray (halfvec.c:166-175), raised from halfvec.c.
fn check_state_array<'a>(mcx: Mcx<'a>, d: Datum, caller: &str) -> PgResult<StateArray<'a>> {
    StateArray::check(mcx, d, caller)
        .map_err(|e| Box::new((*e).with_location("halfvec.c", 173, "CheckStateArray")))
}

// halfvec_accum (halfvec.c:1104-1160).
pub fn fc_halfvec_accum(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let state = check_state_array(mcx, fcinfo.arg(0), "halfvec_accum")?;
    // SAFETY: strict fn — arg1 halfvec.
    let newval = unsafe { arg_halfvec(fcinfo, 1)? };

    let mut dim = state.state_dims();
    let newarr = dim == 0;
    if newarr {
        dim = newval.dim();
    } else {
        check_expected_dim(dim as i32, newval.dim() as i32)?;
    }

    let n = state.value(0) + 1.0;
    let mut datums: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
    datums.push(Datum::from_f64(n));
    if newarr {
        for i in 0..dim {
            datums.push(Datum::from_f64(half_to_float4(newval.x(i)) as f64));
        }
    } else {
        for i in 0..dim {
            let v = state.value(i + 1) + half_to_float4(newval.x(i)) as f64;
            // Check for overflow
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            datums.push(Datum::from_f64(v));
        }
    }
    Ok(image_datum(build_state_array(mcx, &datums)?))
}

// halfvec_avg (halfvec.c:1165-1194).
pub fn fc_halfvec_avg(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let state = check_state_array(mcx, fcinfo.arg(0), "halfvec_avg")?;
    let n = state.value(0);
    // SQL defines AVG of no values to be NULL
    if n == 0.0 {
        fcinfo.isnull = true;
        return Ok(Datum::null());
    }
    let dim = state.state_dims();
    check_dim(dim as i32)?;
    let mut b = HalfBuilder::new(mcx, dim)?;
    for i in 0..dim {
        // C passes the double quotient to Float4ToHalf(float).
        let x = float4_to_half((state.value(i + 1) / n) as f32)?;
        check_element(x)?;
        b.set(i, x);
    }
    Ok(image_datum(b.image()))
}
```

- [ ] **Step 4: Register the symbols and append the SQL**

Add to `lookup`, after the `halfvec_to_vector` arm:
```rust
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
```
(`halfvec_combine` binds the existing `vector_combine` symbol: no new arm.) Then:
```bash
{ echo; scripts/pgvector/upstream-sql.sh 'halfvec functions' 'halfvec private functions' 'halfvec aggregates' 'halfvec operators' 'halfvec opclasses' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
```

- [ ] **Step 5: Unit tests, then the regression gate**

```bash
UT halfvec:: lookup_cover funcs::
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust halfvec bit vector_type hnsw_vector
T=crates/pgvector-0.8.7-reference/test/sql
sed -n -e '1p' -e '14,24p' $T/btree.sql | scripts/pgvector/sqldiff.sh pgrust -
sed -n '16,30p' $T/copy.sql | scripts/pgvector/sqldiff.sh pgrust -
```
Expected:
- `UT` passes: the 6 halfvec tests, the 2 `lookup_cover` tests and the 2 `funcs::` tests.
- All four regress files are `ok`. `halfvec` was FAIL after M1.
- `sqldiff` prints `identical` twice, for the halfvec btree index and the COPY BINARY round trip.

- [ ] **Step 6: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/contrib/pgvector
git commit -q -F - <<'EOF'
pgvector: halfvec functions, operators, aggregates, btree opclass

Ports the rest of halfvec.c: distances, norms, normalize, + - * ||,
binary_quantize, subvector, comparisons (halfvec_ops), and the
accum/avg aggregates (combine reuses vector_combine). Regress `halfvec`
passes.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 5: The `sparsevec` type and its I/O

**Files:**
- Create: `crates/contrib/pgvector/src/sparsevec.rs`
- Modify: `crates/contrib/pgvector/Cargo.toml` (+`numutils`), `Cargo.lock`
- Modify: `crates/contrib/pgvector/src/lib.rs`, `extension/vector--0.8.5.sql`

**Interfaces:**
- Consumes: `vec::{strtof_prefix, StrtofVal, vector_isspace}` and `funcs::{detoasted_image, image_datum}`.
- Produces, in `sparsevec.rs` (Task 6 and M3 use these):
  - `pub const SPARSEVEC_MAX_DIM: i32` and `pub const SPARSEVEC_MAX_NNZ: i32`.
  - `pub struct SparseView<'a>` with `from_payload`, `dim() -> i32`, `nnz() -> usize`, `index(i) -> i32` and `value(i) -> f32`.
  - `pub struct SparseBuilder<'mcx>` with `new(Mcx, dim: i32, nnz: usize)`, `nnz()`, `set_index`, `set_value`, `index`, `value` and `image`.
  - `pub(crate) unsafe fn arg_sparsevec`.
  - `pub(crate) fn check_dim(i32)`; `fn check_expected_dim`, `fn check_nnz(i32, i32)`, `fn check_index(&SparseBuilder, usize, i32)`, `fn check_element(f32)` and `fn ereport(...)`.
  - `pub fn sparsevec_in_body<'m>(Mcx<'m>, &[u8], typmod: i32) -> PgResult<PgVec<'m, u8>>`.
  - `pub(crate) fn sparsevec_recv_body`.

- [ ] **Step 1: Add the dependency**

In `crates/contrib/pgvector/Cargo.toml`, under `[dependencies]` after `adt_varbit`:
```toml
numutils = { path = "../../backend/utils/adt/numutils" }
```
Run `cargo update --workspace --offline` and `git diff --stat Cargo.lock`. Expected: one line added to `pgvector`'s dependency list.

- [ ] **Step 2: Write the failing unit tests**

Create `crates/contrib/pgvector/src/sparsevec.rs` with:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // "{1:1.5,3:3.5}/5"-style text, 1-based; test values print exactly.
    fn render(v: &SparseView<'_>) -> String {
        let els: Vec<String> = (0..v.nnz()).map(|i| format!("{}:{}", v.index(i) + 1, v.value(i))).collect();
        format!("{{{}}}/{}", els.join(","), v.dim())
    }

    fn input(s: &str) -> Result<String, String> {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        match sparsevec_in_body(ctx.mcx(), s.as_bytes(), -1) {
            Ok(img) => Ok(render(&SparseView::from_payload(&img[4..]).unwrap())),
            Err(e) => Err(e.message().to_string()),
        }
    }

    // sparsevec.out: indices sort; zero values are dropped.
    #[test]
    fn input_parses_like_c() {
        assert_eq!(input("{1:1.5,3:3.5}/5").unwrap(), "{1:1.5,3:3.5}/5");
        assert_eq!(input(" { 1 : 1.5 ,  3  :  3.5  } / 5 ").unwrap(), "{1:1.5,3:3.5}/5");
        assert_eq!(input("{1:0,2:1,3:0}/3").unwrap(), "{2:1}/3");
        assert_eq!(input("{2:1,1:1}/2").unwrap(), "{1:1,2:1}/2");
        assert_eq!(input("{}/5").unwrap(), "{}/5");
        assert_eq!(input("{10:0}/3").unwrap(), "{}/3");
    }

    #[test]
    fn input_errors_match_sparsevec_out() {
        let err = |s: &str| input(s).unwrap_err();
        assert_eq!(err("{1:1,1:1}/2"), "sparsevec indices must not contain duplicates");
        assert_eq!(err("{1:1,2:1,1:1}/2"), "sparsevec indices must not contain duplicates");
        assert_eq!(err("{}/-1"), "sparsevec must have at least 1 dimension");
        assert_eq!(err("{}/1000000001"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{}/2147483648"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{}/-2147483649"), "sparsevec must have at least 1 dimension");
        assert_eq!(err("{}/9223372036854775808"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{2147483647:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{2147483648:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{-2147483649:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{0:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{1:1e-46,2:1}/2"), "\"1e-46\" is out of range for type sparsevec");
        assert_eq!(err("{1:-4e38,2:1}/2"), "\"-4e38\" is out of range for type sparsevec");
        assert_eq!(err("{1:NaN,2:1}/2"), "NaN not allowed in sparsevec");
        assert_eq!(err("{1:-Infinity,2:1}/2"), "infinite value not allowed in sparsevec");
        assert_eq!(err("{}"), "invalid input syntax for type sparsevec: \"{}\"");
        assert_eq!(err("{1:}/1"), "invalid input syntax for type sparsevec: \"{1:}/1\"");
        assert_eq!(err(""), "invalid input syntax for type sparsevec: \"\"");
        let many = format!("{{{}1:1}}/1", "1:1,".repeat(16000));
        assert_eq!(err(&many), "sparsevec cannot have more than 16000 non-zero elements");
    }

    // Review Focus 1.
    #[test]
    fn input_survives_every_prefix() {
        let lit = " { 1 : 1.5 , 3:-2e-3,4294967296:1, -1:0 } / 9223372036854775808 ";
        for end in 0..=lit.len() {
            let _ = input(&lit[..end]);
        }
    }

    #[test]
    fn strtol_saturates_like_64_bit_long() {
        assert_eq!(strtol_prefix(b"  42:"), Some((42, 4)));
        assert_eq!(strtol_prefix(b"-7"), Some((-7, 2)));
        assert_eq!(strtol_prefix(b"+0x5"), Some((0, 2)));
        assert_eq!(strtol_prefix(b"9223372036854775808"), Some((i64::MAX, 19)));
        assert_eq!(strtol_prefix(b"-9223372036854775808"), Some((i64::MIN, 20)));
        assert_eq!(strtol_prefix(b"-9223372036854775809"), Some((i64::MIN, 20)));
        assert_eq!(strtol_prefix(b"-"), None);
        assert_eq!(strtol_prefix(b" :1"), None);
    }

    #[test]
    fn strtof_underflow_is_a_nonzero_literal_read_as_zero() {
        assert!(underflowed_to_zero(b"1e-46", 0.0));
        assert!(underflowed_to_zero(b"-0.0001e-42", 0.0));
        assert!(underflowed_to_zero(b"0x1p-200", 0.0));
        assert!(!underflowed_to_zero(b"0e5", 0.0));
        assert!(!underflowed_to_zero(b"-0.000", 0.0));
        assert!(!underflowed_to_zero(b"0x0p-3", 0.0));
        assert!(!underflowed_to_zero(b"1e-45", 1.4e-45));
    }

    fn recv(bytes: &[u8], typmod: i32) -> Result<String, String> {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let mcx = ctx.mcx();
        let mut buf = stringinfo::StringInfo::new_in(mcx).unwrap();
        buf.append_bytes(bytes).unwrap();
        match sparsevec_recv_body(mcx, &mut buf, typmod) {
            Ok(img) => Ok(render(&SparseView::from_payload(&img[4..]).unwrap())),
            Err(e) => Err(e.message().to_string()),
        }
    }

    fn msg(dim: i32, nnz: i32, unused: i32, idx: &[i32], vals: &[f32]) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [dim, nnz, unused].iter().chain(idx) {
            v.extend(x.to_be_bytes());
        }
        for x in vals {
            v.extend(x.to_bits().to_be_bytes());
        }
        v
    }

    // Review Focus 2: binary input is validated like sparsevec_recv
    // (sparsevec.c:521-553), before allocating.
    #[test]
    fn recv_validates_like_c() {
        assert_eq!(recv(&msg(5, 2, 0, &[0, 2], &[1.5, -2.0]), -1).unwrap(), "{1:1.5,3:-2}/5");
        let err = |m: Vec<u8>, t: i32| recv(&m, t).unwrap_err();
        assert_eq!(err(msg(0, 0, 0, &[], &[]), -1), "sparsevec must have at least 1 dimension");
        assert_eq!(err(msg(5, -1, 0, &[], &[]), -1), "sparsevec cannot have negative number of elements");
        assert_eq!(err(msg(1_000_000_000, 16001, 0, &[], &[]), -1), "sparsevec cannot have more than 16000 non-zero elements");
        assert_eq!(err(msg(2, 3, 0, &[], &[]), -1), "sparsevec cannot have more elements than dimensions");
        assert_eq!(err(msg(5, 0, 0, &[], &[]), 4), "expected 4 dimensions, not 5");
        assert_eq!(err(msg(5, 0, 1, &[], &[]), -1), "expected unused to be 0, not 1");
        assert_eq!(err(msg(5, 2, 0, &[2, 0], &[1.0, 1.0]), -1), "sparsevec indices must be in ascending order");
        assert_eq!(err(msg(5, 2, 0, &[1, 1], &[1.0, 1.0]), -1), "sparsevec indices must not contain duplicates");
        assert_eq!(err(msg(5, 1, 0, &[5], &[1.0]), -1), "sparsevec index out of bounds");
        assert_eq!(err(msg(5, 1, 0, &[0], &[0.0]), -1), "binary representation of sparsevec cannot contain zero values");
        assert_eq!(err(msg(5, 1, 0, &[0], &[f32::NAN]), -1), "NaN not allowed in sparsevec");
        assert_eq!(err(msg(5, 2, 0, &[0], &[]), -1), "insufficient data left in message");
    }

    #[test]
    fn from_payload_rejects_corrupt_images() {
        let hdr = |dim: i32, nnz: i32, extra: usize| {
            let mut v = [dim.to_ne_bytes(), nnz.to_ne_bytes(), 0i32.to_ne_bytes()].concat();
            v.resize(12 + extra, 0);
            v
        };
        let corrupt = |p: Vec<u8>| SparseView::from_payload(&p).err().unwrap().message().to_string();
        assert_eq!(corrupt(hdr(0, 0, 0)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, -1, 0)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, 6, 48)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, 2, 8)), "corrupt sparsevec datum");
        assert_eq!(SparseView::from_payload(&hdr(5, 2, 16)).unwrap().nnz(), 2);
    }
}
```
Add `pub mod sparsevec;` to `lib.rs` (after `halfvec`). Run `UT sparsevec::`. Expected: compile errors, `cannot find function 'sparsevec_in_body'` and the like.

- [ ] **Step 3: Implement `sparsevec.rs` (type and I/O)**

Insert above the test module:
```rust
//! sparsevec.c (pgvector 0.8.7): the sparsevec type, a varlena of
//! `i32 dim, i32 nnz, i32 unused, i32 indices[nnz], f32 values[nnz]`
//! (sparsevec.h:21-48) with 0-based, strictly increasing indices, at most
//! 1e9 dimensions and 16,000 non-zero values; its I/O, casts and functions.
//! sparsevec_to_vector lives in funcs.rs (vector.c) and sparsevec_to_halfvec
//! in halfvec.rs (halfvec.c). Errors carry their sparsevec.c location.

use datum::Datum;
use mcx::{Mcx, PgVec};
use stringinfo::StringInfo;
use types_error::{
    PgError, PgResult, SqlState, ERRCODE_DATA_EXCEPTION, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_INVALID_TEXT_REPRESENTATION, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED,
};
use types_fmgr::{cstring_result, FmgrInfo, FunctionCallInfoBaseData as Fcinfo};

use crate::funcs::{detoasted_image, image_datum};
use crate::vec::{strtof_prefix, vector_isspace as sparsevec_isspace, StrtofVal};

pub const SPARSEVEC_MAX_DIM: i32 = 1_000_000_000;
pub const SPARSEVEC_MAX_NNZ: i32 = 16000;

// Payload layout after the 4-byte varlena header: i32 dim, i32 nnz, i32 unused.
pub const SPARSEVEC_PAYLOAD_HDR: usize = 12;

// An ereport/elog of sparsevec.c, located at the call's last line.
#[cold]
pub(crate) fn ereport(code: SqlState, msg: impl Into<String>, line: i32, func: &'static str) -> Box<PgError> {
    PgError::error(msg).with_sqlstate(code).with_location("sparsevec.c", line, func).into()
}

#[derive(Clone, Copy)]
pub struct SparseView<'a> {
    data: &'a [u8],
    nnz: usize,
}

impl<'a> SparseView<'a> {
    pub fn from_payload(data: &'a [u8]) -> PgResult<SparseView<'a>> {
        let corrupt = || -> Box<PgError> { PgError::error("corrupt sparsevec datum").into() };
        if data.len() < SPARSEVEC_PAYLOAD_HDR {
            return Err(corrupt());
        }
        let dim = i32::from_ne_bytes(data[0..4].try_into().unwrap());
        let nnz = i32::from_ne_bytes(data[4..8].try_into().unwrap());
        if !(1..=SPARSEVEC_MAX_DIM).contains(&dim) || !(0..=SPARSEVEC_MAX_NNZ).contains(&nnz) || nnz > dim {
            return Err(corrupt());
        }
        if data.len() < SPARSEVEC_PAYLOAD_HDR + 8 * nnz as usize {
            return Err(corrupt());
        }
        Ok(SparseView { data, nnz: nnz as usize })
    }

    #[inline]
    pub fn dim(&self) -> i32 {
        i32::from_ne_bytes(self.data[0..4].try_into().unwrap())
    }

    #[inline]
    pub fn nnz(&self) -> usize {
        self.nnz
    }

    #[inline]
    pub fn index(&self, i: usize) -> i32 {
        let off = SPARSEVEC_PAYLOAD_HDR + 4 * i;
        i32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }

    // SPARSEVEC_VALUES (sparsevec.h:44-48): the values follow the indices.
    #[inline]
    pub fn value(&self, i: usize) -> f32 {
        let off = SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        f32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }
}

pub struct SparseBuilder<'mcx> {
    img: PgVec<'mcx, u8>,
    nnz: usize,
}

// InitSparseVector (sparsevec.c:153-166): full varlena image, zeroed body.
impl<'mcx> SparseBuilder<'mcx> {
    pub fn new(mcx: Mcx<'mcx>, dim: i32, nnz: usize) -> PgResult<SparseBuilder<'mcx>> {
        let size = 4 + SPARSEVEC_PAYLOAD_HDR + 8 * nnz;
        let mut img: PgVec<'mcx, u8> = mcx::vec_with_capacity_in(mcx, size)?;
        img.resize(size, 0);
        img[..4].copy_from_slice(&((size as u32) << 2).to_ne_bytes());
        img[4..8].copy_from_slice(&dim.to_ne_bytes());
        img[8..12].copy_from_slice(&(nnz as i32).to_ne_bytes());
        Ok(SparseBuilder { img, nnz })
    }

    #[inline]
    pub fn nnz(&self) -> usize {
        self.nnz
    }

    #[inline]
    pub fn set_index(&mut self, i: usize, index: i32) {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * i;
        self.img[off..off + 4].copy_from_slice(&index.to_ne_bytes());
    }

    #[inline]
    pub fn set_value(&mut self, i: usize, value: f32) {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        self.img[off..off + 4].copy_from_slice(&value.to_ne_bytes());
    }

    #[inline]
    pub fn index(&self, i: usize) -> i32 {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * i;
        i32::from_ne_bytes(self.img[off..off + 4].try_into().unwrap())
    }

    #[inline]
    pub fn value(&self, i: usize) -> f32 {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        f32::from_ne_bytes(self.img[off..off + 4].try_into().unwrap())
    }

    pub fn image(self) -> PgVec<'mcx, u8> {
        self.img
    }
}

// SAFETY contract of callers: arg i is a non-null sparsevec varlena (strict fns).
pub(crate) unsafe fn arg_sparsevec<'a>(fcinfo: &'a Fcinfo, i: usize) -> PgResult<SparseView<'a>> {
    let v = unsafe { fcinfo.arg_varlena_packed(i)? };
    SparseView::from_payload(v.data())
}

// CheckExpectedDim (sparsevec.c:56-63).
fn check_expected_dim(typmod: i32, dim: i32) -> PgResult<()> {
    if typmod != -1 && typmod != dim {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected {typmod} dimensions, not {dim}"),
            62,
            "CheckExpectedDim",
        ));
    }
    Ok(())
}

// CheckDim (sparsevec.c:68-80).
pub(crate) fn check_dim(dim: i32) -> PgResult<()> {
    if dim < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec must have at least 1 dimension", 74, "CheckDim"));
    }
    if dim > SPARSEVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_DIM} dimensions"),
            79,
            "CheckDim",
        ));
    }
    Ok(())
}

// CheckNnz (sparsevec.c:85-102).
fn check_nnz(nnz: i32, dim: i32) -> PgResult<()> {
    if nnz < 0 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec cannot have negative number of elements", 91, "CheckNnz"));
    }
    if nnz > SPARSEVEC_MAX_NNZ {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_NNZ} non-zero elements"),
            96,
            "CheckNnz",
        ));
    }
    if nnz > dim {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            "sparsevec cannot have more elements than dimensions",
            101,
            "CheckNnz",
        ));
    }
    Ok(())
}

// CheckIndex (sparsevec.c:107-131) on the i-th index written so far.
fn check_index(b: &SparseBuilder<'_>, i: usize, dim: i32) -> PgResult<()> {
    let index = b.index(i);
    if index < 0 || index >= dim {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec index out of bounds", 116, "CheckIndex"));
    }
    if i > 0 {
        if index < b.index(i - 1) {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec indices must be in ascending order", 124, "CheckIndex"));
        }
        if index == b.index(i - 1) {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec indices must not contain duplicates", 129, "CheckIndex"));
        }
    }
    Ok(())
}

// CheckElement (sparsevec.c:136-148).
fn check_element(value: f32) -> PgResult<()> {
    if value.is_nan() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "NaN not allowed in sparsevec", 142, "CheckElement"));
    }
    if value.is_infinite() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "infinite value not allowed in sparsevec", 147, "CheckElement"));
    }
    Ok(())
}

#[cold]
fn invalid_text(lit: &[u8], detail: Option<&str>, line: i32) -> Box<PgError> {
    let mut e = PgError::error(format!(
        "invalid input syntax for type sparsevec: \"{}\"",
        String::from_utf8_lossy(lit)
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("sparsevec.c", line, "sparsevec_in");
    if let Some(d) = detail {
        e = e.with_detail(d);
    }
    e.into()
}

// strtol(pt, &stringEnd, 10) with C's 64-bit long (sparsevec.c:275, 365):
// skips isspace, takes an optional sign and decimal digits, saturates at
// i64::MIN/MAX. None when no digit follows (stringEnd == pt).
fn strtol_prefix(s: &[u8]) -> Option<(i64, usize)> {
    let mut i = 0usize;
    while i < s.len() && pg_string::isspace_c_locale(s[i]) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let first_digit = i;
    // Accumulate negatively so i64::MIN is reachable without overflow.
    let mut acc: i64 = 0;
    let mut saturated = false;
    while i < s.len() && s[i].is_ascii_digit() {
        match acc.checked_mul(10).and_then(|v| v.checked_sub((s[i] - b'0') as i64)) {
            Some(v) => acc = v,
            None => saturated = true,
        }
        i += 1;
    }
    if i == first_digit {
        return None;
    }
    let value = match (saturated, neg) {
        (true, true) => i64::MIN,
        (true, false) => i64::MAX,
        (false, true) => acc,
        (false, false) => acc.checked_neg().unwrap_or(i64::MAX),
    };
    Some((value, i))
}

// strtof set errno = ERANGE and returned zero (sparsevec.c:315): a literal
// with a nonzero significand digit that rounded to zero.
fn underflowed_to_zero(token: &[u8], value: f32) -> bool {
    if value != 0.0 {
        return false;
    }
    let t: Vec<u8> = token
        .iter()
        .copied()
        .skip_while(|c| pg_string::isspace_c_locale(*c) || *c == b'+' || *c == b'-')
        .collect();
    let (digits, exponent_mark) = if t.len() >= 2 && t[0] == b'0' && (t[1] | 0x20) == b'x' {
        (&t[2..], b'p')
    } else {
        (&t[..], b'e')
    };
    digits
        .iter()
        .take_while(|c| (**c | 0x20) != exponent_mark)
        .any(|c| c.is_ascii_hexdigit() && *c != b'0')
}

// sparsevec_in (sparsevec.c:203-406): `{index:value,...}/dim` with 1-based
// indices; zero values are dropped; the elements are sorted by index
// (qsort, sparsevec.c:393) and then checked.
pub fn sparsevec_in_body<'m>(mcx: Mcx<'m>, lit: &[u8], typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let max_nnz = 1 + lit.iter().filter(|c| **c == b',').count();
    if max_nnz > SPARSEVEC_MAX_NNZ as usize {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_NNZ} non-zero elements"),
            230,
            "sparsevec_in",
        ));
    }
    let mut elements: PgVec<'m, (i32, f32)> = mcx::vec_with_capacity_in(mcx, max_nnz)?;
    let n = lit.len();
    let mut pt = 0usize;

    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'{' {
        return Err(invalid_text(lit, Some("Vector contents must start with \"{\"."), 243));
    }
    pt += 1;
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt < n && lit[pt] == b'}' {
        pt += 1;
    } else {
        loop {
            // Safety check
            if elements.len() >= max_nnz {
                return Err(ereport(
                    ERRCODE_INVALID_TEXT_REPRESENTATION,
                    format!("ran out of buffer: \"{}\"", String::from_utf8_lossy(lit)),
                    263,
                    "sparsevec_in",
                ));
            }
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            // Check for empty string like float4in
            if pt >= n {
                return Err(invalid_text(lit, None, 272));
            }
            // Use similar logic as int2vectorin
            let Some((index, used)) = strtol_prefix(&lit[pt..]) else {
                return Err(invalid_text(lit, None, 280));
            };
            // Keep in int range for correct error message later
            let index = index.clamp(i32::MIN as i64 + 1, i32::MAX as i64) as i32;
            pt += used;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            if pt >= n || lit[pt] != b':' {
                return Err(invalid_text(lit, None, 296));
            }
            pt += 1;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            let Some((val, consumed)) = strtof_prefix(&lit[pt..]) else {
                return Err(invalid_text(lit, None, 312));
            };
            let token = &lit[pt..pt + consumed];
            // Check for range error like float4in
            let value = match val {
                StrtofVal::Ok(v) if !underflowed_to_zero(token, v) => v,
                _ => {
                    return Err(ereport(
                        ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
                        format!("\"{}\" is out of range for type sparsevec", String::from_utf8_lossy(token)),
                        318,
                        "sparsevec_in",
                    ))
                }
            };
            check_element(value)?;
            // Do not store zero values
            if value != 0.0 {
                // Convert 1-based numbering (SQL) to 0-based (C); index > i32::MIN.
                elements.push((index - 1, value));
            }
            pt += consumed;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            if pt < n && lit[pt] == b',' {
                pt += 1;
            } else if pt < n && lit[pt] == b'}' {
                pt += 1;
                break;
            } else {
                return Err(invalid_text(lit, None, 346));
            }
        }
    }

    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'/' {
        return Err(invalid_text(lit, Some("Unexpected end of input."), 357));
    }
    pt += 1;
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    let Some((dim, used)) = strtol_prefix(&lit[pt..]) else {
        return Err(invalid_text(lit, None, 370));
    };
    // Keep in int range for correct error message later
    let dim = dim.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    pt += used;
    // Only whitespace is allowed after the closing brace
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt != n {
        return Err(invalid_text(lit, Some("Junk after closing."), 388));
    }

    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;

    elements.sort_unstable_by_key(|e| e.0);
    let mut b = SparseBuilder::new(mcx, dim, elements.len())?;
    for (i, (index, value)) in elements.iter().enumerate() {
        b.set_index(i, *index);
        b.set_value(i, *value);
        check_index(&b, i, dim)?;
    }
    Ok(b.image())
}

pub fn fc_sparsevec_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict input fn — arg0 cstring, arg2 typmod.
    let lit = unsafe { fcinfo.arg_cstring(0) }.to_bytes();
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(sparsevec_in_body(fcinfo.result_mcx(), lit, typmod)?))
}

// sparsevec_out (sparsevec.c:425-473): 1-based indices, shortest floats.
pub fn fc_sparsevec_out(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut out: PgVec<'_, u8> =
        mcx::vec_with_capacity_in(mcx, (12 + ryu::FLOAT_SHORTEST_DECIMAL_LEN) * v.nnz() + 15)?;
    let mut num = [0u8; 12];
    let mut scratch = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
    mcx::vec_append_bytes(&mut out, b"{")?;
    for i in 0..v.nnz() {
        if i > 0 {
            mcx::vec_append_bytes(&mut out, b",")?;
        }
        // Convert 0-based numbering (C) to 1-based (SQL)
        let k = numutils::pg_ltoa(v.index(i) + 1, &mut num);
        mcx::vec_append_bytes(&mut out, &num[..k])?;
        mcx::vec_append_bytes(&mut out, b":")?;
        let k = ryu::float_to_shortest_decimal_bufn(v.value(i), &mut scratch);
        mcx::vec_append_bytes(&mut out, &scratch[..k])?;
    }
    mcx::vec_append_bytes(&mut out, b"}/")?;
    let k = numutils::pg_ltoa(v.dim(), &mut num);
    mcx::vec_append_bytes(&mut out, &num[..k])?;
    mcx::vec_append_bytes(&mut out, b"\0")?;
    Ok(cstring_result(out))
}

// sparsevec_typmod_in (sparsevec.c:478-504).
pub fn fc_sparsevec_typmod_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 cstring[].
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let tl = arrayfuncs::array_get_integer_typmods(mcx, arr)?;
    if tl.len() != 1 {
        return Err(ereport(ERRCODE_INVALID_PARAMETER_VALUE, "invalid type modifier", 491, "sparsevec_typmod_in"));
    }
    if tl[0] < 1 {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            "dimensions for type sparsevec must be at least 1",
            496,
            "sparsevec_typmod_in",
        ));
    }
    if tl[0] > SPARSEVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            format!("dimensions for type sparsevec cannot exceed {SPARSEVEC_MAX_DIM}"),
            501,
            "sparsevec_typmod_in",
        ));
    }
    Ok(Datum::from_i32(tl[0]))
}

// sparsevec_recv's body (sparsevec.c:511-556): int32 dim, nnz and unused,
// nnz 0-based indices, then nnz float4 values; zero values are rejected.
pub(crate) fn sparsevec_recv_body<'m>(mcx: Mcx<'m>, buf: &mut StringInfo<'_>, typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let dim = pqformat::pq_getmsgint(buf, 4)? as i32;
    let nnz = pqformat::pq_getmsgint(buf, 4)? as i32;
    let unused = pqformat::pq_getmsgint(buf, 4)? as i32;
    check_dim(dim)?;
    check_nnz(nnz, dim)?;
    check_expected_dim(typmod, dim)?;
    if unused != 0 {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected unused to be 0, not {unused}"),
            532,
            "sparsevec_recv",
        ));
    }
    let mut b = SparseBuilder::new(mcx, dim, nnz as usize)?;
    // Binary representation uses zero-based numbering for indices
    for i in 0..nnz as usize {
        b.set_index(i, pqformat::pq_getmsgint(buf, 4)? as i32);
        check_index(&b, i, dim)?;
    }
    for i in 0..nnz as usize {
        let value = pqformat::pq_getmsgfloat4(buf)?;
        check_element(value)?;
        if value == 0.0 {
            return Err(ereport(
                ERRCODE_DATA_EXCEPTION,
                "binary representation of sparsevec cannot contain zero values",
                552,
                "sparsevec_recv",
            ));
        }
        b.set_value(i, value);
    }
    Ok(b.image())
}

pub fn fc_sparsevec_recv(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: recv arg0 is the live StringInfo per the recv ABI.
    let buf = unsafe { fcinfo.arg_stringinfo(0) };
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(sparsevec_recv_body(fcinfo.result_mcx(), buf, typmod)?))
}

// sparsevec_send (sparsevec.c:561-582).
pub fn fc_sparsevec_send(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mut buf = pqformat::pq_begintypsend(fcinfo.result_mcx())?;
    pqformat::pq_sendint32(&mut buf, v.dim() as u32)?;
    pqformat::pq_sendint32(&mut buf, v.nnz() as u32)?;
    pqformat::pq_sendint32(&mut buf, 0)?; // svec->unused, always zero
    // Binary representation uses zero-based numbering for indices
    for i in 0..v.nnz() {
        pqformat::pq_sendint32(&mut buf, v.index(i) as u32)?;
    }
    for i in 0..v.nnz() {
        pqformat::pq_sendfloat4(&mut buf, v.value(i))?;
    }
    Ok(types_fmgr::varlena_result(pqformat::pq_endtypsend(buf)))
}
```

- [ ] **Step 4: Register the symbols and append the SQL**

Add to `lookup`, after the `halfvec_avg` arm:
```rust
        "sparsevec_in" => sparsevec::fc_sparsevec_in,
        "sparsevec_out" => sparsevec::fc_sparsevec_out,
        "sparsevec_typmod_in" => sparsevec::fc_sparsevec_typmod_in,
        "sparsevec_recv" => sparsevec::fc_sparsevec_recv,
        "sparsevec_send" => sparsevec::fc_sparsevec_send,
```
Then:
```bash
{ echo; scripts/pgvector/upstream-sql.sh 'sparsevec type' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
```

- [ ] **Step 5: Unit tests and the C comparison**

```bash
UT sparsevec:: lookup_cover
scripts/pgvector/build-pgrust.sh
sed -n '1,59p' crates/pgvector-0.8.7-reference/test/sql/sparsevec.sql | scripts/pgvector/sqldiff.sh pgrust -
```
Expected:
- 7 sparsevec tests and 2 `lookup_cover` tests pass.
- `sqldiff (pgrust): identical` for every input and typmod case in `sparsevec.sql` lines 1–59.

- [ ] **Step 6: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
scripts/pgvector/run-regress.sh pgrust bit halfvec vector_type hnsw_vector
git add crates/contrib/pgvector Cargo.lock
git commit -q -F - <<'EOF'
pgvector: sparsevec type and I/O

Ports sparsevec.c's type half: text input (strtol/strtof semantics,
int-range clamps, zero dropping, sorted and checked indices), output,
typmods and binary I/O with C's validation. Errors carry their
sparsevec.c location.

Co-Authored-By: <the implementing model>
EOF
```
Expected before the commit: the four regress files are `ok`.

---

### Task 6: `sparsevec` functions, every cross-type cast, and the M2 regression gate

**Files:**
- Modify: `crates/contrib/pgvector/src/sparsevec.rs` (functions and casts; extend the test module)
- Modify: `crates/contrib/pgvector/src/halfvec.rs` (`fc_sparsevec_to_halfvec`)
- Modify: `crates/contrib/pgvector/src/funcs.rs` (`fc_sparsevec_to_vector`)
- Modify: `crates/contrib/pgvector/src/lib.rs`, `extension/vector--0.8.5.sql`
- Create: `scripts/pgvector/sql/error-locations.sql`

**Interfaces:**
- Consumes:
  - Task 5's view, builder, checks and `ereport`;
  - Task 3's `cast_array_elems`, `cast_elem_f32`, `ArraySite`, `arg_vector` and `halfvec::{arg_halfvec, HalfBuilder, check_dim, ereport}`;
  - Task 2's `half_is_zero`, `half_to_float4` and `float4_to_half`.
- Produces:
  - `pub fn sparsevec_l2_squared_distance(&SparseView, &SparseView) -> f32`
  - `pub fn sparsevec_inner_product(&SparseView, &SparseView) -> f32`
  - `pub fn sparsevec_l1_distance(&SparseView, &SparseView) -> f32`
  
  M3's HNSW sparsevec support uses these three kernels. The task also produces every remaining `sparsevec.c` fmgr function, and `scripts/pgvector/sql/error-locations.sql`, which Task 8 wires into `run-diff.sh`.

- [ ] **Step 1: Write the failing unit tests**

Append inside `sparsevec.rs`'s `mod tests`:
```rust
    struct XorShift(u64);

    impl XorShift {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    // A sparse image of a random dense vector with integer values (a third
    // of them non-zero), plus the dense form.
    fn random_sparse<'m>(m: Mcx<'m>, r: &mut XorShift, dim: i32) -> (PgVec<'m, u8>, Vec<f32>) {
        let mut dense = vec![0.0f32; dim as usize];
        for x in dense.iter_mut() {
            if r.below(3) == 0 {
                *x = r.below(9) as f32 - 4.0;
            }
        }
        let nz: Vec<usize> = (0..dense.len()).filter(|&i| dense[i] != 0.0).collect();
        let mut b = SparseBuilder::new(m, dim, nz.len()).unwrap();
        for (j, &i) in nz.iter().enumerate() {
            b.set_index(j, i as i32);
            b.set_value(j, dense[i]);
        }
        (b.image(), dense)
    }

    // Review Focus 4: the merge kernels against a dense expansion over
    // disjoint, interleaved, prefix/suffix and empty index sets. Integer
    // values keep every f32 sum exact, so equality is exact.
    #[test]
    fn sparse_kernels_match_dense_expansion() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let mut r = XorShift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..5000 {
            let dim = 1 + r.below(12) as i32;
            let (ia, da) = random_sparse(m, &mut r, dim);
            let (ib, db) = random_sparse(m, &mut r, dim);
            let a = SparseView::from_payload(&ia[4..]).unwrap();
            let b = SparseView::from_payload(&ib[4..]).unwrap();
            let l2: f32 = da.iter().zip(&db).map(|(x, y)| (x - y) * (x - y)).sum();
            let ip: f32 = da.iter().zip(&db).map(|(x, y)| x * y).sum();
            let l1: f32 = da.iter().zip(&db).map(|(x, y)| (x - y).abs()).sum();
            assert_eq!(sparsevec_l2_squared_distance(&a, &b), l2, "{da:?} {db:?}");
            assert_eq!(sparsevec_inner_product(&a, &b), ip, "{da:?} {db:?}");
            assert_eq!(sparsevec_l1_distance(&a, &b), l1, "{da:?} {db:?}");
        }
    }

    // sparsevec.out: sparsevec_cmp follows Postgres array order.
    #[test]
    fn cmp_matches_sparsevec_out() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let img = |s: &str| sparsevec_in_body(m, s.as_bytes(), -1).unwrap();
        let cmp = |a: &str, b: &str| {
            let (ia, ib) = (img(a), img(b));
            sparsevec_cmp_internal(&SparseView::from_payload(&ia[4..]).unwrap(), &SparseView::from_payload(&ib[4..]).unwrap())
        };
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{1:1,2:2,3:3}/3"), 0);
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{}/3"), 1);
        assert_eq!(cmp("{}/3", "{1:1,2:2,3:3}/3"), -1);
        assert_eq!(cmp("{1:1,2:2}/2", "{1:1,2:2,3:3}/3"), -1);
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{1:1,2:2}/2"), 1);
        assert_eq!(cmp("{1:1,2:2}/2", "{1:2,2:3,3:4}/3"), -1);
        assert_eq!(cmp("{1:2,2:3}/2", "{1:1,2:2,3:3}/3"), 1);
    }
```
Run `UT sparsevec::`. Expected: compile errors for the missing kernels and `sparsevec_cmp_internal`.

- [ ] **Step 2: Implement the functions and casts**

Extend `sparsevec.rs`'s imports:
```rust
use types_error::ERRCODE_INTERNAL_ERROR;

use crate::funcs::{arg_vector, cast_array_elems, cast_elem_f32, ArraySite};
use crate::halfutils::{half_is_zero, half_to_float4};
```
Insert before the test module:
```rust
// CheckDims (sparsevec.c:44-51).
fn check_dims(a: &SparseView<'_>, b: &SparseView<'_>) -> PgResult<()> {
    if a.dim() != b.dim() {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("different sparsevec dimensions {} and {}", a.dim(), b.dim()),
            50,
            "CheckDims",
        ));
    }
    Ok(())
}

fn sparsevec_2arg(fcinfo: &Fcinfo) -> PgResult<(SparseView<'_>, SparseView<'_>)> {
    // SAFETY: strict fns — args 0 and 1 are sparsevecs.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let b = unsafe { arg_sparsevec(fcinfo, 1)? };
    Ok((a, b))
}

// sparsevec (sparsevec.c:588-598): applies the type modifier.
pub fn fc_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec, arg1 typmod.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_expected_dim(typmod, v.dim())?;
    Ok(fcinfo.arg(0))
}

// vector_to_sparsevec (sparsevec.c:603-642).
pub fn fc_vector_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, arg1 typmod.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = v.dim() as i32;
    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;
    let nnz = v.iter().filter(|x| *x != 0.0).count();
    check_nnz(nnz as i32, dim)?;
    let mut r = SparseBuilder::new(fcinfo.result_mcx(), dim, nnz)?;
    let mut j = 0usize;
    for i in 0..v.dim() {
        if v.x(i) != 0.0 {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 633, "vector_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, v.x(i));
            j += 1;
        }
    }
    Ok(image_datum(r.image()))
}

// halfvec_to_sparsevec (sparsevec.c:647-686).
pub fn fc_halfvec_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { crate::halfvec::arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = v.dim() as i32;
    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;
    let nnz = (0..v.dim()).filter(|&i| !half_is_zero(v.x(i))).count();
    check_nnz(nnz as i32, dim)?;
    let mut r = SparseBuilder::new(fcinfo.result_mcx(), dim, nnz)?;
    let mut j = 0usize;
    for i in 0..v.dim() {
        if !half_is_zero(v.x(i)) {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 677, "halfvec_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, half_to_float4(v.x(i)));
            j += 1;
        }
    }
    Ok(image_datum(r.image()))
}

// array_to_sparsevec (sparsevec.c:691-818). C converts each element twice
// (count pass, fill pass, sparsevec.c:730-799); converting once into
// `values` gives the same floats and the same first error.
pub fn fc_array_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 array, arg1 typmod.
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let typmod = fcinfo.arg_i32(1);
    let site = ArraySite { file: "sparsevec.c", func: "array_to_sparsevec", ndim_line: 710, nulls_line: 715 };
    let (elemtype, elems) = cast_array_elems(mcx, arr, &site)?;
    let n = elems.len() as i32;
    check_dim(n)?;
    check_expected_dim(typmod, n)?;
    let mut values: PgVec<'_, f32> = mcx::vec_with_capacity_in(mcx, elems.len())?;
    for d in elems.iter() {
        match cast_elem_f32(elemtype, *d)? {
            Some(v) => values.push(v),
            None => return Err(ereport(ERRCODE_DATA_EXCEPTION, "unsupported array type", 754, "array_to_sparsevec")),
        }
    }
    // IS_NOT_ZERO (sparsevec.c:727): NaN and infinities count as non-zero.
    let nnz = values.iter().filter(|v| **v != 0.0).count();
    check_nnz(nnz as i32, n)?;
    let mut r = SparseBuilder::new(mcx, n, nnz)?;
    let mut j = 0usize;
    for (i, v) in values.iter().enumerate() {
        if *v != 0.0 {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 767, "array_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, *v);
            j += 1;
        }
    }
    if j != r.nnz() {
        return Err(ereport(ERRCODE_INTERNAL_ERROR, "correctness check failed", 811, "array_to_sparsevec"));
    }
    // Check elements
    for i in 0..r.nnz() {
        check_element(r.value(i))?;
    }
    Ok(image_datum(r.image()))
}

// SparsevecL2SquaredDistance (sparsevec.c:823-866): one merge over the two
// sorted index lists.
pub fn sparsevec_l2_squared_distance(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        let mut bi = -1i32;
        for j in bpos..b.nnz() {
            bi = b.index(j);
            if ai == bi {
                let diff = a.value(i) - b.value(j);
                distance += diff * diff;
            } else if ai > bi {
                distance += b.value(j) * b.value(j);
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
        if ai != bi {
            distance += a.value(i) * a.value(i);
        }
    }
    for j in bpos..b.nnz() {
        distance += b.value(j) * b.value(j);
    }
    distance
}

// SparsevecInnerProduct (sparsevec.c:902-933).
pub fn sparsevec_inner_product(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        for j in bpos..b.nnz() {
            let bi = b.index(j);
            // Only update when the same index
            if ai == bi {
                distance += a.value(i) * b.value(j);
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
    }
    distance
}

// sparsevec_l1_distance's merge (sparsevec.c:1021-1054), factored out like
// the L2 and inner-product kernels.
pub fn sparsevec_l1_distance(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        let mut bi = -1i32;
        for j in bpos..b.nnz() {
            bi = b.index(j);
            if ai == bi {
                distance += (a.value(i) - b.value(j)).abs();
            } else if ai > bi {
                distance += b.value(j).abs();
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
        if ai != bi {
            distance += a.value(i).abs();
        }
    }
    for j in bpos..b.nnz() {
        distance += b.value(j).abs();
    }
    distance
}

// sparsevec_l2_distance (sparsevec.c:871-881).
pub fn fc_sparsevec_l2_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64((sparsevec_l2_squared_distance(&a, &b) as f64).sqrt()))
}

// sparsevec_l2_squared_distance (sparsevec.c:887-897).
pub fn fc_sparsevec_l2_squared_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_l2_squared_distance(&a, &b) as f64))
}

// sparsevec_inner_product (sparsevec.c:938-948).
pub fn fc_sparsevec_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_inner_product(&a, &b) as f64))
}

// sparsevec_negative_inner_product (sparsevec.c:953-963).
pub fn fc_sparsevec_negative_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(-sparsevec_inner_product(&a, &b) as f64))
}

// sparsevec_cosine_distance (sparsevec.c:968-1008).
pub fn fc_sparsevec_cosine_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut similarity = sparsevec_inner_product(&a, &b) as f64;
    let mut norma = 0.0f32;
    for i in 0..a.nnz() {
        norma += a.value(i) * a.value(i);
    }
    let mut normb = 0.0f32;
    for i in 0..b.nnz() {
        normb += b.value(i) * b.value(i);
    }
    // Use sqrt(a * b) over sqrt(a) * sqrt(b)
    similarity /= ((norma as f64) * (normb as f64)).sqrt();
    // Keep in range
    if similarity > 1.0 {
        similarity = 1.0;
    } else if similarity < -1.0 {
        similarity = -1.0;
    }
    Ok(Datum::from_f64(1.0 - similarity))
}

// sparsevec_l1_distance (sparsevec.c:1013-1057).
pub fn fc_sparsevec_l1_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_l1_distance(&a, &b) as f64))
}

// sparsevec_vector_dims (sparsevec.c:1064-1071); not in the 0.8.7 SQL.
pub fn fc_sparsevec_vector_dims(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    Ok(Datum::from_i32(a.dim()))
}

// sparsevec_l2_norm (sparsevec.c:1076-1089).
pub fn fc_sparsevec_l2_norm(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mut norm = 0.0f64;
    for i in 0..a.nnz() {
        norm += a.value(i) as f64 * a.value(i) as f64;
    }
    Ok(Datum::from_f64(norm.sqrt()))
}

// sparsevec_l2_normalize (sparsevec.c:1094-1158).
pub fn fc_sparsevec_l2_normalize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut r = SparseBuilder::new(mcx, a.dim(), a.nnz())?;
    let mut norm = 0.0f64;
    for i in 0..a.nnz() {
        norm += a.value(i) as f64 * a.value(i) as f64;
    }
    norm = norm.sqrt();
    // Return zero vector for zero norm
    if norm > 0.0 {
        let mut zeros = 0usize;
        for i in 0..a.nnz() {
            r.set_index(i, a.index(i));
            // C stores the double quotient into a float.
            let v = (a.value(i) as f64 / norm) as f32;
            r.set_value(i, v);
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            if v == 0.0 {
                zeros += 1;
            }
        }
        // Allocate a new vector in the unlikely event there are zeros
        if zeros > 0 {
            let mut n = SparseBuilder::new(mcx, a.dim(), a.nnz() - zeros)?;
            let mut j = 0usize;
            for i in 0..a.nnz() {
                if r.value(i) == 0.0 {
                    continue;
                }
                // Safety check
                if j >= n.nnz() {
                    return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 1144, "sparsevec_l2_normalize"));
                }
                n.set_index(j, r.index(i));
                n.set_value(j, r.value(i));
                j += 1;
            }
            return Ok(image_datum(n.image()));
        }
    }
    Ok(image_datum(r.image()))
}

// sparsevec_cmp_internal (sparsevec.c:1163-1199).
fn sparsevec_cmp_internal(a: &SparseView<'_>, b: &SparseView<'_>) -> i32 {
    let nnz = a.nnz().min(b.nnz());
    // Check values before dimensions to be consistent with Postgres arrays
    for i in 0..nnz {
        if a.index(i) < b.index(i) {
            return if a.value(i) < 0.0 { -1 } else { 1 };
        }
        if a.index(i) > b.index(i) {
            return if b.value(i) < 0.0 { 1 } else { -1 };
        }
        if a.value(i) < b.value(i) {
            return -1;
        }
        if a.value(i) > b.value(i) {
            return 1;
        }
    }
    if a.nnz() < b.nnz() && b.index(nnz) < a.dim() {
        return if b.value(nnz) < 0.0 { 1 } else { -1 };
    }
    if a.nnz() > b.nnz() && a.index(nnz) < b.dim() {
        return if a.value(nnz) < 0.0 { -1 } else { 1 };
    }
    if a.dim() < b.dim() {
        return -1;
    }
    if a.dim() > b.dim() {
        return 1;
    }
    0
}

// sparsevec_lt (sparsevec.c:1204-1212).
pub fn fc_sparsevec_lt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) < 0))
}

// sparsevec_le (sparsevec.c:1217-1225).
pub fn fc_sparsevec_le(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) <= 0))
}

// sparsevec_eq (sparsevec.c:1230-1238).
pub fn fc_sparsevec_eq(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) == 0))
}

// sparsevec_ne (sparsevec.c:1243-1251).
pub fn fc_sparsevec_ne(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) != 0))
}

// sparsevec_ge (sparsevec.c:1256-1264).
pub fn fc_sparsevec_ge(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) >= 0))
}

// sparsevec_gt (sparsevec.c:1269-1277).
pub fn fc_sparsevec_gt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) > 0))
}

// sparsevec_cmp (sparsevec.c:1282-1290).
pub fn fc_sparsevec_cmp(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_i32(sparsevec_cmp_internal(&a, &b)))
}
```

In `halfvec.rs`, add before the test module (and add `ERRCODE_INTERNAL_ERROR` to its `types_error` import):
```rust
// sparsevec_to_halfvec (halfvec.c:1199-1225).
pub fn fc_sparsevec_to_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec, arg1 typmod.
    let svec = unsafe { crate::sparsevec::arg_sparsevec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = svec.dim();
    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;
    let mut r = HalfBuilder::new(fcinfo.result_mcx(), dim as usize)?;
    for i in 0..svec.nnz() {
        let index = svec.index(i);
        // Safety check
        if index < 0 || index >= dim {
            return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 1219, "sparsevec_to_halfvec"));
        }
        r.set(index as usize, float4_to_half(svec.value(i))?);
    }
    Ok(image_datum(r.image()))
}
```

In `funcs.rs`, add after `fc_halfvec_to_vector`:
```rust
// sparsevec_to_vector (vector.c:1320-1347).
pub fn fc_sparsevec_to_vector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec, arg1 typmod.
    let svec = unsafe { crate::sparsevec::arg_sparsevec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = svec.dim();
    // dim >= 1 (SparseView); vector's limit is 16,000, checked before allocating.
    check_dim(dim as usize)?;
    check_expected_dim(typmod, dim as usize)?;
    let mut r = VecBuilder::new(fcinfo.result_mcx(), dim as usize)?;
    for i in 0..svec.nnz() {
        let index = svec.index(i);
        // Safety check
        if index < 0 || index >= dim {
            return Err(PgError::error("index out of bounds")
                .with_location("vector.c", 1343, "sparsevec_to_vector")
                .into());
        }
        r.set(index as usize, svec.value(i));
    }
    Ok(image_datum(r.image()))
}
```

- [ ] **Step 3: Register the symbols and append the SQL**

Add to `lookup`, after the `sparsevec_send` arm:
```rust
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
```
Then:
```bash
{ echo; scripts/pgvector/upstream-sql.sh 'sparsevec functions' 'sparsevec private functions' 'sparsevec cast functions' 'sparsevec casts' 'sparsevec operators' 'sparsevec opclasses' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
grep -c "AS 'MODULE_PATHNAME'" crates/contrib/pgvector/extension/vector--0.8.5.sql
```
Expected count: 108, the 38 lines before M2 plus 70 appended. Several SQL functions share one symbol: each `array_to_*` four times, and `halfvec_combine` reuses `vector_combine`. M2 adds 64 `lookup` arms: 2 bit, 35 halfvec, 27 sparsevec. The lookup test is the real check.

- [ ] **Step 4: Unit tests**

Run: `UT_M2`
Expected: every test passes. The tally:
- `lookup_cover` 2, `bitutils::` 2, `bitvec::` 2, `halfutils::` 3, `halfvec::` 6, `sparsevec::` 9;
- `funcs::` 2 (pre-existing): 26 in all.

- [ ] **Step 5: Write `scripts/pgvector/sql/error-locations.sql`**

Each statement reaches one C error site. All 49 were checked against the C reference with `\set VERBOSITY verbose`:
```sql
-- Error locations (run-diff.sh's error_locations row): one statement per
-- ereport/elog in halfvec.c, halfutils.h, sparsevec.c and bitvec.c that SQL
-- can reach. sqldiff.sh --verbose compares each LOCATION line (C function,
-- file, and the call's last line). Binary-only sites (the recv functions)
-- are pinned by unit tests instead.
-- halfvec.c
SELECT '[1,2]'::halfvec + '[3]';
SELECT '[1,2,3]'::halfvec(2);
SELECT '{}'::real[]::halfvec;
SELECT array_fill(0, ARRAY[16000])::halfvec || '[1]';
SELECT '[NaN,1]'::halfvec;
SELECT '[Infinity,1]'::halfvec;
SELECT halfvec_avg('{{2,2,4,6}}');
SELECT '1,2,3'::halfvec;
SELECT '[]'::halfvec;
SELECT ('[' || array_to_string(array_fill(1, ARRAY[16001]), ',') || ']')::halfvec;
SELECT '['::halfvec;
SELECT '[hello,1]'::halfvec;
SELECT '[65520]'::halfvec;
SELECT '[1,2,3'::halfvec;
SELECT '[1,2,3]9'::halfvec;
SELECT '[1,2,3]'::halfvec(3, 2);
SELECT '[1,2,3]'::halfvec(0);
SELECT '[1,2,3]'::halfvec(16001);
SELECT '{{1}}'::real[]::halfvec;
SELECT '{NULL}'::real[]::halfvec;
SELECT subvector('[1,2,3,4,5]'::halfvec, 1, 0);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2147483647, 10);
-- halfutils.h
SELECT '{65520}'::real[]::halfvec;
-- sparsevec.c
SELECT '{1:1}/2'::sparsevec <-> '{1:1}/3';
SELECT '{}/3'::sparsevec(2);
SELECT '{}/-1'::sparsevec;
SELECT '{}/1000000001'::sparsevec;
SELECT array_agg(n)::sparsevec FROM generate_series(1, 16001) n;
SELECT '{0:1}/1'::sparsevec;
SELECT '{1:1,1:1}/2'::sparsevec;
SELECT '{1:NaN}/1'::sparsevec;
SELECT '{1:Infinity}/1'::sparsevec;
SELECT ('{' || repeat('1:1,', 16001) || '1:1}/1')::sparsevec;
SELECT '1:1}/1'::sparsevec;
SELECT '{'::sparsevec;
SELECT '{:1}/1'::sparsevec;
SELECT '{1a:1}/1'::sparsevec;
SELECT '{1:}/1'::sparsevec;
SELECT '{1:4e38}/1'::sparsevec;
SELECT '{1:1a}/1'::sparsevec;
SELECT '{}'::sparsevec;
SELECT '{}/'::sparsevec;
SELECT '{}/1a'::sparsevec;
SELECT '{}/3'::sparsevec(3, 2);
SELECT '{}/3'::sparsevec(0);
SELECT '{}/3'::sparsevec(1000000001);
SELECT '{{1}}'::real[]::sparsevec;
SELECT '{NULL}'::real[]::sparsevec;
-- bitvec.c
SELECT hamming_distance('111', '00');
```

- [ ] **Step 6: The M2 regression and TAP gate**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust bit btree cast copy halfvec sparsevec vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 018_aggregates.pl 033_comparison.pl 034_distance_functions.pl 037_inputs.pl
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/error-locations.sql
```
Expected:
- All eight regress files are `ok`. This is the M2 exit criterion for regress.
- All four TAP tests are `ok`.
- `sqldiff (pgrust): identical` for error-locations.sql, with LOCATION lines included.

Stop conditions:
- **TAP 018.** If 018 fails only at `is($res, "[24576,24576,49152]")` (the halfvec `sum` over a parallel plan), stop and report the observed value. That saturated half-precision sum depends on how rows split across parallel workers, which is pgrust's executor and not M2 code. Every other 018 assertion must pass.
- **Regress or sqldiff.** If any regress file or the sqldiff fails:
  - read the diff;
  - fix the owning function with a unit test that reproduces it;
  - rebuild and rerun.
  
  Never edit expected outputs.

- [ ] **Step 7: Review Focus 5, huge sparse dimensions**

```bash
printf '%s\n' \
  "SELECT '{1000000000:1}/1000000000'::sparsevec <-> '{1:1}/1000000000', l2_norm('{1000000000:-2}/1000000000'::sparsevec);" \
  "SELECT l2_normalize('{1000000000:4,1:3}/1000000000'::sparsevec), '{999999999:1.5}/1000000000'::sparsevec;" \
  "SELECT '{1000000000:1}/1000000000'::sparsevec::vector;" \
  "SELECT '{1000000000:1}/1000000000'::sparsevec::halfvec;" |
  scripts/pgvector/sqldiff.sh pgrust -
```
Expected: `identical`. Both casts must error with `vector cannot have more than 16000 dimensions` and `halfvec cannot have more than 16000 dimensions`. Check `~/.cache/pgrust/pgvector-work/sqldiff/pgrust/pgrust.out`. The server log shows no memory-context growth worth a warning.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/contrib/pgvector scripts/pgvector/sql/error-locations.sql
git commit -q -F - <<'EOF'
pgvector: sparsevec functions and every cross-type cast

Ports the rest of sparsevec.c (merge-based distances, norm, normalize,
comparisons and sparsevec_ops, the casts from vector, halfvec and arrays)
plus sparsevec_to_halfvec (halfvec.c) and sparsevec_to_vector (vector.c).
Regress bit, btree, cast, copy, halfvec and sparsevec pass, as do TAP 018,
033, 034 and 037; every reachable error matches C's LOCATION.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 7: diffrunner `--pgvector` suite

**Files:**
- Create: `crates/bin/fuzzgen/src/pgvector.rs`, `crates/bin/fuzzgen/src/pgvector_deck.sql`
- Modify: `crates/bin/fuzzgen/src/lib.rs` (`pub mod pgvector;`)
- Modify: `crates/bin/fuzzgen/src/copybin.rs` (`normalize_user_oids` → `pub(crate)`)
- Modify: `crates/bin/fuzzgen/src/bin/diffrunner.rs` (`--pgvector`)
- Modify: `docs/fuzzing/rulings.toml` (append one row)

**Interfaces:**
- Consumes the fuzzgen internals:
  - `diff::{classify, Classified, DiffClass, DiffInput, StmtOutcome}`;
  - `runner::{Executor, Record}` and `ruled::{apply_ruled, RuledEntry}`;
  - `rng::Rng` (`new_pure`, `below`, `below_usize`, `range_i64`, `chance`);
  - `copybin::normalize_user_oids`.
- Produces:
  - `pub fn fuzzgen::pgvector::run_suite(a: &mut dyn Executor, b: &mut dyn Executor, table: &[RuledEntry], ulp_tol: u64, seed: u64, seeded_n: u32) -> (Vec<Record>, Vec<SectionStats>)`.
  - `pub struct SectionStats { name, cases, matches, ruled, findings }`.
  - `diffrunner --pgvector` prints one stderr line per section, in the form `diffrunner: pgvector section=<name> cases=<n> matches=<n> ruled=<n> findings=<n>`. It exits 0 with no findings and 2 with findings. Task 8 parses these lines.

- [ ] **Step 1: Write the deck**

Create `crates/bin/fuzzgen/src/pgvector_deck.sql` with this exact content. It holds 184 statements, all run on the C reference while writing this plan; all 83 errors are intended.
```sql
-- pgvector exact-differential deck (diffrunner --pgvector; pgvector Phase 1
-- spec §8.2). One statement per line, each run on both servers. A
-- `-- section: <name>` line starts a section; other `--` lines and blank
-- lines are skipped. Integer and dyadic data keep float sums exact; the
-- few non-integral distances fall under the pgvector-float-rel ruling.

-- section: io
SELECT '[1,2,3]'::vector, '[-1.5,0,2.25]'::vector, ' [ 1 , 2 ] '::vector;
SELECT '[1e-46,1e-45,-1e-40,3.4e38,-3.4e38]'::vector;
SELECT '[0x1p-3,-0x1.8p1,1.5e+2]'::vector, '[0x1p-3,-0x1.8p1,1.5e+2]'::halfvec;
SELECT '[-0,0]'::vector, '[-0,0]'::halfvec, '{1:-0}/1'::sparsevec;
SELECT '[1,2,3]'::halfvec, '[1.23456]'::halfvec, '[0.1,0.2,0.3]'::halfvec;
SELECT '[65504,-65504,65519,6e-08,5.96e-08,3e-08,2049,2051]'::halfvec;
SELECT '[1e-8,-1e-8,1e-46]'::halfvec;
SELECT '{1:1.5,3:3.5}/5'::sparsevec, ' { 3 : -2 , 1 : 1 } / 4 '::sparsevec, '{}/1'::sparsevec;
SELECT '{1:0,2:1,3:0}/3'::sparsevec, '{1:1e-40,2:-1.5e-38}/2'::sparsevec;
SELECT '{1000000000:1}/1000000000'::sparsevec, '{10:0}/3'::sparsevec;
SELECT '[1,2'::vector;
SELECT '[1,2,3]x'::halfvec;
SELECT '[hello]'::halfvec;
SELECT '[]'::halfvec;
SELECT '[NaN]'::halfvec;
SELECT '[-Infinity]'::halfvec;
SELECT '[65520]'::halfvec;
SELECT '[4e38]'::halfvec;
SELECT '[1,,2]'::halfvec;
SELECT '[1, ]'::halfvec;
SELECT '{1:1,1:2}/2'::sparsevec;
SELECT '{2:1,1:2,2:3}/2'::sparsevec;
SELECT '{0:1}/1'::sparsevec;
SELECT '{1:1e-46}/1'::sparsevec;
SELECT '{1:4e38}/1'::sparsevec;
SELECT '{1:Infinity}/1'::sparsevec;
SELECT '{1:NaN}/1'::sparsevec;
SELECT '{}/0'::sparsevec;
SELECT '{}/1000000001'::sparsevec;
SELECT '{}/9223372036854775808'::sparsevec;
SELECT '{}/-9223372036854775809'::sparsevec;
SELECT '{2147483648:1}/1'::sparsevec;
SELECT '{-2147483649:1}/1'::sparsevec;
SELECT '{1:1'::sparsevec;
SELECT '1:1}/1'::sparsevec;
SELECT '{1:1}/1 x'::sparsevec;
SELECT '{1:1}'::sparsevec;
SELECT '{1:1}/'::sparsevec;
SELECT '{a:1}/1'::sparsevec;
SELECT '{1 2}/2'::sparsevec;
SELECT '{1:}/1'::sparsevec;

-- section: typmod
SELECT '[1,2,3]'::vector(3), '[1,2,3]'::halfvec(3), '{}/3'::sparsevec(3);
SELECT '[1,2,3]'::halfvec(2);
SELECT '{}/3'::sparsevec(2);
SELECT '[1,2,3]'::halfvec(0);
SELECT '[1,2,3]'::halfvec(16001);
SELECT '{}/3'::sparsevec(1000000001);
SELECT '[1,2,3]'::halfvec(3, 2);
SELECT '{}/3'::sparsevec(3, 2);
SELECT '{"[1,2,3]","[4,5,6]"}'::halfvec(3)[];
SELECT '{"[1,2,3]"}'::halfvec(2)[];
SELECT '{"{1:1}/3","{}/3"}'::sparsevec(3)[];
SELECT '{"{}/3"}'::sparsevec(4)[];

-- section: cast
SELECT ARRAY[1,2,3]::vector, ARRAY[1,2,3]::halfvec, ARRAY[1,0,3]::sparsevec;
SELECT ARRAY[1.5,0,-2.25]::numeric[]::halfvec, ARRAY[1.5,0,-2.25]::numeric[]::sparsevec;
SELECT '{1,2,3}'::real[]::halfvec(3), '{0,0,1}'::real[]::sparsevec(3);
SELECT '{1,2,3}'::double precision[]::halfvec, '{1e-46,0,1}'::double precision[]::sparsevec;
SELECT '{65520}'::real[]::halfvec;
SELECT '{4e38}'::double precision[]::halfvec;
SELECT '{4e38}'::double precision[]::sparsevec;
SELECT '{1e-8,-1e-8}'::real[]::halfvec;
SELECT '{NaN}'::real[]::sparsevec;
SELECT '{NULL,1}'::real[]::halfvec;
SELECT '{{1,2}}'::real[]::sparsevec;
SELECT '{}'::real[]::halfvec;
SELECT '{}'::real[]::sparsevec;
SELECT '{1,2}'::real[]::sparsevec(3);
SELECT '[1,2,3]'::vector::halfvec, '[1.5,0,-2]'::vector::sparsevec;
SELECT '[0.1,0.2]'::vector::halfvec::vector;
SELECT '[65520]'::vector::halfvec;
SELECT '[1,2,3]'::halfvec::vector, '[0,1.5,0]'::halfvec::sparsevec, '[1,2,3]'::halfvec::real[];
SELECT '{2:1.5,4:-3.5}/5'::sparsevec::vector, '{2:1.5,4:-3.5}/5'::sparsevec::halfvec;
SELECT '{1:65520}/1'::sparsevec::halfvec;
SELECT '{1:1e-8}/1'::sparsevec::halfvec;
SELECT '{}/16001'::sparsevec::vector;
SELECT '{}/16001'::sparsevec::halfvec;
SELECT '{1:1}/3'::sparsevec::vector(2);
SELECT '[1,2,3]'::vector::sparsevec(2);
SELECT '[1,2,3]'::halfvec::vector(2);
SELECT '[0,0,0]'::vector::sparsevec, '[0,-0]'::halfvec::sparsevec;
SELECT array_agg(n)::halfvec IS NULL FROM generate_series(1, 16001) n;
SELECT array_agg(n)::sparsevec IS NULL FROM generate_series(1, 16001) n;

-- section: arith
SELECT '[1,2,3]'::halfvec + '[4,5,6]', '[1,2,3]'::halfvec - '[4,5,6]', '[1,2,3]'::halfvec * '[4,5,6]';
SELECT '[0.1,0.2,0.3]'::halfvec + '[0.3,0.2,0.1]', '[0.1,0.2,0.3]'::halfvec * '[3,3,3]', '[0.1]'::halfvec - '[0.3]';
SELECT '[1.5,-2.25]'::vector + '[1e-40,1e-40]', '[1,2]'::vector * '[0.5,0.25]';
SELECT '[65504]'::halfvec + '[1]', '[2048]'::halfvec + '[1]', '[2048]'::halfvec + '[3]';
SELECT '[65504]'::halfvec + '[16]';
SELECT '[-65504]'::halfvec - '[32]';
SELECT '[300]'::halfvec * '[300]';
SELECT '[1e-4]'::halfvec * '[1e-4]';
SELECT '[0]'::halfvec * '[1e-4]', '[6e-8]'::halfvec * '[1]';
SELECT '[6e-8]'::halfvec * '[0.5]';
SELECT '[1,2]'::halfvec + '[1]';
SELECT '[1,2]'::halfvec || '[3]', '[1]'::vector || '[2,3]';
SELECT array_fill(0, ARRAY[16000])::halfvec || '[1]';
SELECT sum(v) FROM (VALUES ('[1,2]'::halfvec), ('[3,4]'), ('[0.5,0.25]')) t(v);

-- section: distance
SELECT l2_distance('[0,0]'::vector, '[3,4]'), inner_product('[1,2]'::vector, '[3,4]'), cosine_distance('[1,2]'::vector, '[2,4]'), l1_distance('[0,0]'::vector, '[3,4]');
SELECT l2_distance('[0,0]'::halfvec, '[3,4]'), inner_product('[1,2]'::halfvec, '[3,4]'), cosine_distance('[1,2]'::halfvec, '[2,4]'), l1_distance('[0,0]'::halfvec, '[3,4]');
SELECT l2_distance('{}/2'::sparsevec, '{1:3,2:4}/2'), inner_product('{1:1,2:2}/2'::sparsevec, '{1:3,2:4}/2'), cosine_distance('{1:1,2:2}/2'::sparsevec, '{1:2,2:4}/2'), l1_distance('{}/2'::sparsevec, '{1:3,2:4}/2');
SELECT '[1,2,3]'::vector <-> '[3,2,1]', '[1,2,3]'::vector <#> '[3,2,1]', '[1,2,3]'::vector <=> '[3,2,1]', '[1,2,3]'::vector <+> '[3,2,1]';
SELECT '[1,2,3]'::halfvec <-> '[3,2,1]', '[1,2,3]'::halfvec <#> '[3,2,1]', '[1,2,3]'::halfvec <=> '[3,2,1]', '[1,2,3]'::halfvec <+> '[3,2,1]';
SELECT '{1:1,3:3}/3'::sparsevec <-> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <#> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <=> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <+> '{2:2,3:1}/3';
SELECT l2_distance('{1:1,3:3,5:5,7:7}/9'::sparsevec, '{2:2,4:4,6:6,8:8,9:9}/9'), l1_distance('{9:1}/9'::sparsevec, '{1:1,2:2}/9'), inner_product('{1:1,3:3,5:5}/5'::sparsevec, '{2:4,3:6,4:8}/5');
SELECT l2_distance('{1:1,2:2,3:3}/3'::sparsevec, '{3:3}/3'), l1_distance('{3:3}/3'::sparsevec, '{1:1,2:2,3:3}/3'), inner_product('{2:2}/3'::sparsevec, '{1:1,2:2,3:3}/3');
SELECT vector_l2_squared_distance('[1,2]', '[3,5]'), vector_negative_inner_product('[1,2]', '[3,5]'), vector_spherical_distance('[0.6,0.8]', '[0.8,0.6]');
SELECT halfvec_l2_squared_distance('[1,2]', '[3,5]'), halfvec_negative_inner_product('[1,2]', '[3,5]'), halfvec_spherical_distance('[0.6,0.8]', '[0.8,0.6]');
SELECT sparsevec_l2_squared_distance('{1:1}/2', '{2:5}/2'), sparsevec_negative_inner_product('{1:1,2:2}/2', '{1:3,2:5}/2');
SELECT cosine_distance('[0,0]'::vector, '[1,1]'), cosine_distance('[0,0]'::halfvec, '[1,1]'), cosine_distance('{}/2'::sparsevec, '{1:1}/2');
SELECT cosine_distance('[1,1]'::halfvec, '[-1.1,-1.1]'), cosine_distance('{1:3e38}/1'::sparsevec, '{1:3e38}/1');
SELECT inner_product('[65504]'::halfvec, '[65504]'), inner_product('{1:3e38}/1'::sparsevec, '{1:3e38}/1');
SELECT l2_distance('[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9]'::vector, '[0.9,0.8,0.7,0.6,0.5,0.4,0.3,0.2,0.1]');
SELECT l2_distance('[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9]'::halfvec, '[0.9,0.8,0.7,0.6,0.5,0.4,0.3,0.2,0.1]');
SELECT l2_distance(array_fill(1, ARRAY[16000])::vector, array_fill(2, ARRAY[16000])::vector), l1_distance(array_fill(1, ARRAY[16000])::halfvec, array_fill(2, ARRAY[16000])::halfvec);
SELECT l2_distance('[1,2]'::halfvec, '[3]');
SELECT inner_product('{1:1}/2'::sparsevec, '{1:1}/3');
SELECT l1_distance('[1,2]'::vector, '[3]');

-- section: norm
SELECT vector_norm('[3,4]'), l2_norm('[3,4]'::halfvec), l2_norm('{1:3,2:4}/2'::sparsevec), l2_norm('{}/2'::sparsevec);
SELECT l2_normalize('[3,4]'::vector), l2_normalize('[3,4]'::halfvec), l2_normalize('{1:3,2:4}/2'::sparsevec);
SELECT l2_normalize('[0,0]'::vector), l2_normalize('[0,0]'::halfvec), l2_normalize('{}/2'::sparsevec);
SELECT l2_normalize('[0.1,0.2,0.3]'::vector), l2_normalize('[0.1,0.2,0.3]'::halfvec), l2_normalize('{1:0.1,3:0.3}/4'::sparsevec);
SELECT l2_normalize('[65504]'::halfvec), l2_normalize('[6e-8]'::halfvec), l2_normalize('{1:3e38}/1'::sparsevec);
SELECT l2_normalize('{1:3e38,2:1e-37}/2'::sparsevec), l2_normalize('{2:3e37,4:3e-37,6:4e37,8:4e-37}/9'::sparsevec);
SELECT l2_norm('{1:3e37,2:4e37}/2'::sparsevec)::real, vector_norm('[1e-40,1e-40]'), l2_norm('[65504,65504]'::halfvec);

-- section: agg
SELECT avg(v), sum(v) FROM (VALUES ('[1,2,3]'::halfvec), ('[3,5,7]'), (NULL)) t(v);
SELECT avg(v), sum(v) FROM (VALUES ('[1,2,3]'::vector), ('[3,5,7]')) t(v);
SELECT avg(v) FROM (VALUES ('[0.1,0.2]'::halfvec), ('[0.3,0.4]'), ('[0.5,0.6]')) t(v);
SELECT avg(v), sum(v) FROM (SELECT '[1]'::halfvec WHERE false) t(v);
SELECT avg(v) FROM (VALUES ('[1,2]'::halfvec), ('[3]')) t(v);
SELECT sum(v) FROM (VALUES ('[65504]'::halfvec), ('[65504]')) t(v);
SELECT avg(v) FROM (VALUES ('[65504]'::halfvec), ('[65504]')) t(v);
SELECT halfvec_accum('{0}', '[1,2,3]'), halfvec_accum('{1,1,2,3}', '[1,2,3]');
SELECT halfvec_accum('{0,0}', '[1,2,3]');
SELECT halfvec_accum('{1,1.7976931348623157e308}', '[65504]');
SELECT halfvec_avg('{2,2,4,6}'), halfvec_avg('{0}');
SELECT halfvec_avg('{{2,2,4,6}}');
SELECT halfvec_avg('{1,70000}');
SELECT halfvec_combine('{1,2,3}', '{2,4,6}'), halfvec_combine('{0}', '{1,1}');

-- section: cmp
SELECT '[1,2,3]'::halfvec < '[1,2,3]', '[1,2,3]'::halfvec <= '[1,2]', '[1,2]'::halfvec = '[1,2]', '[1,2]'::halfvec <> '[1,2,3]', '[2]'::halfvec >= '[1,9]', '[0.1]'::halfvec > '[0.10001]';
SELECT halfvec_cmp('[1,2]', '[1,2,3]'), halfvec_cmp('[2,3]', '[1,2,3]'), halfvec_cmp('[-0]', '[0]');
SELECT '{1:1,2:2,3:3}/3'::sparsevec < '{1:1,2:2}/2', '{1:1}/2'::sparsevec = '{1:1}/2', '{1:1}/2'::sparsevec <> '{1:1}/3', '{1:1}/2'::sparsevec >= '{2:1}/2';
SELECT sparsevec_cmp('{1:1,2:2}/2', '{1:2,2:3,3:4}/3'), sparsevec_cmp('{1:2,2:3}/2', '{1:1,2:2,3:3}/3'), sparsevec_cmp('{2:-1}/3', '{1:1}/3'), sparsevec_cmp('{1:-1}/3', '{2:1}/3');
SELECT sparsevec_cmp('{}/3', '{3:-1}/3'), sparsevec_cmp('{3:1}/3', '{}/2'), sparsevec_cmp('{1:1}/3', '{1:1,3:-1}/3'), sparsevec_cmp('{1:1,2:5}/2', '{1:1}/1');
SELECT v FROM (VALUES ('[1,2]'::halfvec), ('[1]'), ('[0,5]'), ('[1,2,0]'), ('[-1]')) t(v) ORDER BY v;
SELECT v FROM (VALUES ('{1:1}/3'::sparsevec), ('{}/3'), ('{2:-1}/3'), ('{1:-1}/2'), ('{3:2}/3'), ('{}/2')) t(v) ORDER BY v;
SELECT DISTINCT v FROM (VALUES ('[1,2]'::halfvec), ('[1,2]'), ('[2,1]')) t(v) ORDER BY v;

-- section: misc
SELECT vector_dims('[1,2,3]'::halfvec), vector_dims('[1]'::vector);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2, 3), subvector('[1,2,3,4,5]'::halfvec, -1, 3), subvector('[1,2,3,4,5]'::halfvec, 3, 9);
SELECT subvector('[1,2,3,4,5]'::halfvec, 3, 2147483647), subvector('[1,2,3,4,5]'::halfvec, -2147483644, 2147483647);
SELECT subvector('[1,2,3,4,5]'::halfvec, 1, 0);
SELECT subvector('[1,2,3,4,5]'::halfvec, -1, 2);
SELECT subvector('[1,2,3,4,5]'::halfvec, 6, 1);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2147483647, 10);
SELECT binary_quantize('[1,0,-1,0.5,-0.5,2,-2,3,0.25]'::vector), binary_quantize('[1,0,-1,0.5,-0.5,2,-2,3,0.25]'::halfvec);
SELECT binary_quantize('[0,-0]'::halfvec), binary_quantize('[6e-8]'::halfvec), binary_quantize('[-6e-8]'::halfvec);
SELECT binary_quantize('[1,2,3,-4,5,6,-7,8,1,-2,-3,4,5,-6,7,8,-1,2,3]'::halfvec) <~> binary_quantize('[1,2,3,-4,5,6,-7,8,1,-2,-3,4,5,-6,7,8,-1,2,3]'::vector);

-- section: bit
SELECT hamming_distance('111', '111'), hamming_distance('111', '010'), hamming_distance('', ''), jaccard_distance('', '');
SELECT hamming_distance(repeat('10', 300)::bit(600), repeat('01', 300)::bit(600)), jaccard_distance(repeat('110', 200)::bit(600), repeat('011', 200)::bit(600));
SELECT '1010'::bit(4) <~> '0101', '1100'::bit(4) <%> '1010', '1111'::varbit <~> '0000'::varbit;
SELECT jaccard_distance('0000', '0000'), jaccard_distance('1000', '0100'), jaccard_distance('1100', '1000');
SELECT hamming_distance('111', '000'::varbit(4)), jaccard_distance('1111', '0000'::varbit(5));
SELECT hamming_distance('111', '0000'::varbit(4));
SELECT jaccard_distance('1111', '000');

-- section: knn
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <-> '[1,1]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <#> '[1,2]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <=> '[1,2]', i;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <-> '[1,1]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <+> '[1,2]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <=> '[1,2]', i;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <-> '{1:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <#> '{1:1,3:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <+> '{2:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <=> '{1:1}/3', i;
SELECT i FROM (VALUES (1, B'1010'), (2, B'0101'), (3, B'1110'), (4, B'1011')) t(i, v) ORDER BY v <~> '1010', i;
SELECT i FROM (VALUES (1, B'1010'), (2, B'0101'), (3, B'1110'), (4, B'1011')) t(i, v) ORDER BY v <%> '1010', i;

-- section: limits
SELECT vector_dims(array_fill(1, ARRAY[16000])::halfvec), l2_norm(array_fill(1, ARRAY[16000])::halfvec);
SELECT ('[' || array_to_string(array_fill(1, ARRAY[16001]), ',') || ']')::halfvec IS NULL;
SELECT '{1000000000:1}/1000000000'::sparsevec <-> '{1:1}/1000000000', l2_norm('{1000000000:-2}/1000000000'::sparsevec);
SELECT l2_normalize('{1000000000:4,1:3}/1000000000'::sparsevec);
SELECT '{1000000000:1}/1000000000'::sparsevec::vector;
SELECT '{1000000000:1}/1000000000'::sparsevec::halfvec;
SELECT ('{' || repeat('1:1,', 16000) || '1:1}/1')::sparsevec IS NULL;
SELECT vector_dims(array_agg(n)::sparsevec::vector) FROM generate_series(1, 16000) n;
SELECT array_agg(n % 2)::sparsevec IS NOT NULL FROM generate_series(1, 32000) n;
SELECT ('{' || string_agg(n || ':1', ',') || '}/16000')::sparsevec IS NOT NULL FROM generate_series(1, 16000) n;
```

- [ ] **Step 2: Write the suite with its tests failing first**

Create `crates/bin/fuzzgen/src/pgvector.rs` with this content. The test module first, then the implementation above it, in one file:
```rust
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
```
In `crates/bin/fuzzgen/src/lib.rs`, add `pub mod pgvector;` in alphabetical order among the `pub mod` lines. In `crates/bin/fuzzgen/src/copybin.rs`, change `fn normalize_user_oids(o: &StmtOutcome) -> StmtOutcome {` to `pub(crate) fn normalize_user_oids(o: &StmtOutcome) -> StmtOutcome {`.

Run: `UT_FUZZ pgvector::`
Expected: 3 passed, 1 failed. The failure is `pgvector_compare_rules_tolerance_and_reports_real_divergence`, with `unruled divergence candidate "pgvector-float-rel"`, because the ledger has no row yet.

- [ ] **Step 3: Add the ruling**

Append to `docs/fuzzing/rulings.toml`, after its last row, preceded by one blank line. Use today's date (`date +%F`) for `created`:
```toml
[[ruling]]
id = "pgvector-float-rel"
plane = "rows"
field = "pgvector-rel"
ref = "pgvector Phase 1 spec §2 and §8.2: distances are not bit-exact (pgvector builds with -fassociative-math and fused multiply-adds, so its f32 sums follow the compiler's order); the diffrunner --pgvector suite rules a row-set divergence whose differing numerals are non-integral on at least one side and within 1e-5 relative, floored at magnitude 1 (fuzzgen pgvector.rs REL_TOL); integral numerals, structure, row order and errors still compare exactly"
owner = "pgvector M2 exact-differential tier (scripts/pgvector/run-diff.sh)"
created = "2026-10-05"
hits = 0
```
`field = "pgvector-rel"` never occurs in sitediff's divergence fields, so sitediff can never match this row. Only the suite resolves it, by id.

Run: `UT_FUZZ pgvector:: rulings:: ruled::`
Expected: all pass. That includes `embedded_ledger_parses_and_is_the_live_file`, which pins the file to its rendered form. If it fails, its assertion diff shows the canonical rendering; match the row to it exactly.

- [ ] **Step 4: Add `--pgvector` to diffrunner**

In `crates/bin/fuzzgen/src/bin/diffrunner.rs`:
- **`Args`:** add a field `pgvector: bool,` after `dbddl: bool,`. In the default initializer, add `pgvector: false,` after `dbddl: false,`.
- **Parser:** add `"--pgvector" => args.pgvector = true,` next to `"--copytext" => args.copytext = true,`.
- **Suite condition:** extend `if args.copybin || args.copytext || args.copyopts || args.dbddl {` to `if args.copybin || args.copytext || args.copyopts || args.dbddl || args.pgvector {`.
- **Usage text:** add after the `--dbddl` block:
```
  --pgvector          run the pgvector exact-differential suite (pgvector
                      Phase 1 spec §8.2) instead of a statement stream:
                      CREATE EXTENSION vector, the fixed deck
                      (src/pgvector_deck.sql), then a seeded arm sized by
                      --count with --seed. Prints one "pgvector
                      section=<name> ..." line per section to stderr.
                      Ignores --xproto/--replay.
```
- **Dispatch:** add directly after the closing brace of the `if args.copytext { ... }` block:
```rust
    if args.pgvector {
        // pgvector exact-differential suite: the deck plus a seeded arm
        // sized by --count (deterministic in --seed).
        let table = fuzzgen::ruled::default_table();
        let (records, sections) =
            fuzzgen::pgvector::run_suite(&mut *a, &mut *b, &table, args.ulp, args.seed, args.count);
        let mut findings_out: Box<dyn Write> = match &args.findings {
            Some(path) => Box::new(
                std::fs::File::create(path).map_err(|e| format!("open {path}: {e}"))?,
            ),
            None => Box::new(std::io::stdout()),
        };
        writeln!(
            findings_out,
            "{{\"meta\":\"diffrunner\",\"suite\":\"pgvector\",\"seed\":{},\"count\":{},\"guc_pin\":{}}}",
            args.seed, args.count, args.guc_pin
        )
        .map_err(|e| e.to_string())?;
        for r in &records {
            writeln!(findings_out, "{}", r.to_jsonl(args.seed)).map_err(|e| e.to_string())?;
        }
        drop(findings_out);
        let mut findings = 0;
        for s in &sections {
            eprintln!(
                "diffrunner: pgvector section={} cases={} matches={} ruled={} findings={}",
                s.name, s.cases, s.matches, s.ruled, s.findings
            );
            findings += s.findings;
        }
        return Ok(if findings > 0 { ExitCode::from(2) } else { ExitCode::SUCCESS });
    }
```

- [ ] **Step 5: Build diffrunner and run the fuzzgen tests**

```bash
cargo build --profile fast-profile --locked --bin postgres --bin pagemask --bin diffrunner 2>&1 | grep -E '^\s+Compiling' | head
target/fast-profile/diffrunner --help 2>&1 | grep -A2 -- '--pgvector'
UT_FUZZ pgvector:: rulings:: ruled:: copybin::
```
Expected:
- The `Compiling` lines name `fuzzgen` and, at most, crates M2 changed. If `main_main` or low-level crates (`mcx`, `pgsync`, `gram_core`) recompile, building fuzzgen with the server changed feature resolution. Stop and report that.
- The help text shows `--pgvector`.
- All tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/bin/fuzzgen/src/{pgvector.rs,pgvector_deck.sql,lib.rs,copybin.rs,bin/diffrunner.rs} docs/fuzzing/rulings.toml
git commit -q -F - <<'EOF'
fuzzgen: diffrunner --pgvector exact-differential suite

A fixed 184-statement deck plus a seeded arm over vector, halfvec,
sparsevec and bit: I/O, typmods, casts, arithmetic, distances, norms,
aggregates, comparisons, KNN over integer data and the dimension/nnz
limits. Divergences in non-integral numerals within 1e-5 relative are
ruled pgvector-float-rel (pgvector's own sums are order-dependent);
everything else compares exactly.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 8: The exact-differential tier, and making it clean

**Files:**
- Modify: `scripts/pgvector/common.sh` (`PGV_LISTEN`, `DIFFRUNNER_BIN`), `scripts/pgvector/build-pgrust.sh`, `scripts/pgvector/run-all.sh`
- Create: `scripts/pgvector/run-diff.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh`

**Interfaces:**
- Consumes:
  - Task 7's `diffrunner --pgvector` and its stderr section lines;
  - Task 3's `sqldiff.sh`;
  - Task 6's `error-locations.sql`.
- Produces: `scripts/pgvector/run-diff.sh {pgrust|ref}`. It writes `$PGV_WORK/diff/<mode>/summary.tsv`, one `name<TAB>ok|FAIL` row per suite section plus an `error_locations` row: 15 rows in all. `run-all.sh` runs it as tier `diff`.

- [ ] **Step 1: Let harness servers listen on TCP on request, and build diffrunner**

In `scripts/pgvector/common.sh`:
- after the `PAGEMASK_BIN=` line, add:
```bash
DIFFRUNNER_BIN="${DIFFRUNNER_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/diffrunner}"
# Extra listen address for harness servers (default: Unix socket only).
# run-diff.sh sets 127.0.0.1 because diffrunner speaks TCP only.
PGV_LISTEN="${PGV_LISTEN:-}"
```
- in `server_start`, change the pgrust `exec` line to:
```bash
        exec "$PGRUST_BIN" -D "$data" -k "$PGV_WORK/sock" -p "$port" "${PGRUST_SERVER_OPTS[@]}" -c "listen_addresses=$PGV_LISTEN"
```
- in the `ref` and `seeded` arms, change `-c listen_addresses=` to `-c "listen_addresses=$PGV_LISTEN"`.

In `scripts/pgvector/build-pgrust.sh`:
- change the cargo line to:
```bash
(cd "$PGV_REPO" && cargo build --profile "$PGRUST_PROFILE" --locked --bin postgres --bin pagemask --bin diffrunner)
```
- after the `pagemask` check, add:
```bash
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN after the build"
```

- [ ] **Step 2: Write `scripts/pgvector/run-diff.sh`**

```bash
#!/usr/bin/env bash
# Exact-differential tier (spec §8.2): diffrunner's --pgvector suite against
# the C reference (A) and a subject (B), plus the error-location check.
# Every vector, halfvec, sparsevec and bit function, cast, operator and
# aggregate, their errors, and exact (non-index) nearest-neighbour queries
# must match; numerals non-integral on either side may differ within 1e-5
# relative (ruling pgvector-float-rel, docs/fuzzing/rulings.toml).
#
# usage: run-diff.sh {pgrust|ref}
#   pgrust: B is pgrust.  ref: B is the seeded C build (must always pass).
#   PGV_DIFF_SEED (default 1) and PGV_DIFF_COUNT (default 2000) seed and
#   size the random arm. PGV_DIFF_PERTURB_B adds one diffrunner --perturb-b
#   statement (harness self-test only).
set -euo pipefail
. "$(dirname "$0")/common.sh"
here="$(cd "$(dirname "$0")" && pwd)"

[ "$#" -eq 1 ] || die "usage: run-diff.sh {pgrust|ref}"
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/diff/$mode"
rm -rf "$out"
mkdir -p "$out"

PGV_LISTEN=127.0.0.1
trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$(port_for "$side")"
  assert_identity "$side" "$(port_for "$side")"
done

perturb=()
if [ -n "${PGV_DIFF_PERTURB_B:-}" ]; then perturb=(--perturb-b "$PGV_DIFF_PERTURB_B"); fi
set +e
"$DIFFRUNNER_BIN" --pgvector \
  --a "127.0.0.1:$(port_for ref)" --b "127.0.0.1:$(port_for "$subject")" \
  --db postgres --user postgres \
  --seed "${PGV_DIFF_SEED:-1}" --count "${PGV_DIFF_COUNT:-2000}" \
  ${perturb[@]+"${perturb[@]}"} \
  --findings "$out/findings.jsonl" 2>"$out/diffrunner.log"
rc=$?
set -e
server_stop ref
server_stop "$subject"
[ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data"
[ "$rc" -le 2 ] || die "diffrunner failed (exit $rc); see $out/diffrunner.log"

sed -n 's/^diffrunner: pgvector section=\([a-z_]*\) .* findings=\([0-9]*\)$/\1 \2/p' \
  "$out/diffrunner.log" >"$out/sections.txt"
[ -s "$out/sections.txt" ] || die "no section results in $out/diffrunner.log"
: >"$out/summary.tsv"
while read -r name findings; do
  if [ "$findings" = 0 ]; then r=ok; else r=FAIL; fi
  printf '%s\t%s\n' "$name" "$r" | tee -a "$out/summary.tsv"
done <"$out/sections.txt"

# The error F/L/R fields, which diffrunner does not compare.
PGV_LISTEN=
if "$here/sqldiff.sh" "$mode" --verbose "$here/sql/error-locations.sql" >"$out/error-locations.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'error_locations\t%s\n' "$r" | tee -a "$out/summary.tsv"

total=$(wc -l <"$out/summary.tsv" | tr -d ' ')
passed=$(grep -c $'\tok$' "$out/summary.tsv" || true)
echo "diff ($mode, subject $subject): $passed/$total clean; findings in $out/findings.jsonl"
[ "$passed" -eq "$total" ]
```
`chmod +x scripts/pgvector/run-diff.sh`.

In `scripts/pgvector/run-all.sh`:
- add `"$here/run-diff.sh" "$mode" || true` after the `run-iterscan.sh` line;
- change `for tier in regress tap bytecmp iterscan; do` to `for tier in regress tap bytecmp iterscan diff; do`.

- [ ] **Step 3: Self-tests**

Add above the `# --- runner ---` line of `scripts/pgvector/tests/harness_test.sh`:
```bash
# Exact differential, C vs C: the reference and the seeded build agree on
# every section, error locations included.
test_diff_ref() {
  local n
  if "$here/../run-diff.sh" ref >"$PGV_WORK/diff-ref.log" 2>&1; then
    pass "diff ref: exit 0"
  else
    fail "diff ref: exit 0 (see $PGV_WORK/diff-ref.log)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/diff/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 15 ]; then pass "diff ref: 15 ok"; else fail "diff ref: '$n' ok, want 15"; fi
}

# Negative control: a B side without l2_normalize(halfvec) must be reported
# in the deck section that calls it.
test_diff_detects_difference() {
  local w="$PGV_WORK/diff-neg"
  if PGV_WORK="$w" PGV_DIFF_COUNT=50 \
    PGV_DIFF_PERTURB_B="CREATE EXTENSION vector; ALTER EXTENSION vector DROP FUNCTION l2_normalize(halfvec); DROP FUNCTION l2_normalize(halfvec)" \
    "$here/../run-diff.sh" ref >"$w.log" 2>&1; then
    fail "diff flags a missing function"
  elif grep -q $'^norm\tFAIL$' "$w/diff/ref/summary.tsv" 2>/dev/null; then
    pass "diff flags a missing function"
  else
    fail "diff flags a missing function: wrong failure (see $w.log)"
  fi
}
```

- [ ] **Step 4: Run the reference side**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/tests/harness_test.sh test_diff_ref test_diff_detects_difference test_sqldiff_ref
```
Expected: `ok` for every line and `all passed`.

`run-diff.sh ref` compares C with C. Any `FAIL` there is a harness bug: fix the harness, never the expectations.

- [ ] **Step 5: Run the pgrust side and triage to clean**

```bash
scripts/pgvector/run-diff.sh pgrust
grep '"triage":"unclassified"' ~/.cache/pgrust/pgvector-work/diff/pgrust/findings.jsonl | head
```
The goal is exit 0 with all 15 rows `ok`. For each `unclassified` record:
1. Reproduce it with `echo '<sql>' | scripts/pgvector/sqldiff.sh pgrust -`.
2. If the divergence is in M2 code, fix the owning function test-first: add a unit test that reproduces it, see it fail, fix, see it pass. Then rebuild and rerun `run-diff.sh pgrust`.
3. If it is in pre-M2 `vector` code and the fix is under about 20 lines with a unit test, fix it the same way. Otherwise stop and report the record to the controller.
4. If it is in pgrust core, or is a float-order difference the ruling doesn't cover (for example a near-tie that reorders rows), stop and report the record. Never add or widen a ruling without the user's approval.

Then confirm with a second seed and a bigger arm:
```bash
PGV_DIFF_SEED=2 PGV_DIFF_COUNT=5000 scripts/pgvector/run-diff.sh pgrust
grep -c '"triage":"ruled"' ~/.cache/pgrust/pgvector-work/diff/pgrust/findings.jsonl
```
Expected: exit 0. Record the ruled count, which goes into Task 9's report.

- [ ] **Step 6: Full harness self-test, lints and commit**

```bash
scripts/pgvector/tests/harness_test.sh
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add scripts/pgvector crates/contrib/pgvector crates/bin/fuzzgen
git commit -q -F - <<'EOF'
pgvector harness: exact-differential tier (run-diff.sh)

Runs diffrunner --pgvector against the C reference and pgrust (or the
seeded C build in ref mode) over TCP, one summary row per suite section,
plus an error_locations row (sqldiff.sh --verbose over every reachable
C error site). run-all.sh runs it as tier `diff`. The pgrust side is
clean.

Co-Authored-By: <the implementing model>
EOF
```
Expected before the commit: `harness_test.sh` prints `all passed`.

---

### Task 9: Docs, cleanup, and the M2 report

**Files:**
- Modify: `crates/contrib/pgvector/src/lib.rs` (crate header)
- Delete: `crates/contrib/pgvector/sql/` (spec §4.1)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, `CLAUDE.md`
- Create: `docs/superpowers/reports/pgvector-m2-report.md`

- [ ] **Step 1: Crate header and the orphaned SQL files**

Replace the `//!` header of `crates/contrib/pgvector/src/lib.rs` with:
```rust
//! pgvector (github.com/pgvector/pgvector), ported from 0.8.5 @ 159b79a and
//! brought to 0.8.7 behavior: the vector, halfvec and sparsevec types and
//! the bit distance functions (I/O, functions, aggregates, casts, btree
//! opclasses), one module per C file. The HNSW opclasses for halfvec,
//! sparsevec and bit (M3) and ivfflat (M4) are unported; the shipped
//! extension script is the trimmed vector--0.8.5.sql grown with upstream
//! sections (scripts/pgvector/upstream-sql.sh) until M4 (spec §4.6).
//! DIVERGENCE: pg_get_loaded_modules() reports 18.6 for this library; C
//! reports PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49). Revisit in M4.
//! DIVERGENCE: errors raised by the pre-M2 vector code (vec.rs, funcs.rs)
//! carry crate-derived locations, not vector.c's; halfvec_to_vector and
//! sparsevec_to_vector inherit that through vector's CheckDim helpers.
```
Then remove the orphaned test files and confirm nothing references them:
```bash
git rm -q -r crates/contrib/pgvector/sql
grep -rn 'contrib/pgvector/sql' --include='*.rs' --include='*.sh' --include='*.toml' --include='*.md' . | grep -v '^./target' || echo none
```
Expected: `none`, apart from the `CLAUDE.md` sentence Step 3 removes.

- [ ] **Step 2: Correct the spec**

In `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`:
- **§8.2, Unit row.** Replace `Exhaustive f32→f16 test: all 2³² inputs against pgvector's C routine, compiled by the test's `build.rs`.` with:

  `Exhaustive f16 parity (test crate `pgvector_f16_parity`, whose `build.rs` compiles the vendored `halfutils.h` routines and the compiler's `_Float16` path): all 2³² f32 and 2¹⁶ f16 inputs, plus every pair of halves through + − ×.`
- **§8.2, Exact differential row.** Replace the Method cell with:

  `diffrunner's `--pgvector` suite (`crates/bin/fuzzgen/src/pgvector.rs`: a fixed SQL deck plus a seeded random arm), run by `scripts/pgvector/run-diff.sh`. Covers functions, casts, operators, aggregates, error cases and exact (non-index) nearest-neighbour queries over integer data, with special values: zero vectors, denormals, −0, f16 range edges, dimension and nnz limits, NaN/inf rejection. Also compares every reachable error's LOCATION (`sqldiff.sh --verbose`).`

  Replace the Pass rule cell with:

  `Identical. Numerals that are non-integral on either side may differ within 1e-5 relative, floored at magnitude 1 (ruling `pgvector-float-rel`: pgvector's own f32 sums depend on compiler reassociation and FMA). Other accepted divergences go in `docs/fuzzing/rulings.toml`.`
- **§8.3.** Add `run-diff.sh` (M2), `sqldiff.sh` (M2) and `upstream-sql.sh` (M2) to the script list.
- **§4.6.** After the lookup-coverage bullet, add: `(In place since M2: `lookup_covers_every_module_pathname_symbol` in `crates/contrib/pgvector/src/lib.rs`.)`

- [ ] **Step 3: Update `CLAUDE.md`**

In the "Vector search (fork work)" section:
- Replace the first paragraph and its bullet list with:
```markdown
There is an existing pgvector port (upstream 0.8.5 commit `159b79a`, brought to 0.8.7 behavior for `vector` and HNSW in M1, with `halfvec`, `sparsevec` and the `bit` functions added in M2; the SQL script is still the trimmed 0.8.5 one, grown with upstream sections by `scripts/pgvector/upstream-sql.sh`):
- `crates/contrib/pgvector`: the `vector`, `halfvec` and `sparsevec` types and the `bit` distance functions: I/O, functions, aggregates, casts and btree opclasses, one module per C file.
- `crates/contrib/pgvector_f16_parity`: test-only. Its `build.rs` compiles pgvector's C f16 routines and checks every f16, every f32 and every pair of halves against the port; it is skipped on wasm32.
- `crates/contrib/pgvector_hnsw` and `pgvector_hnsw_build`: the HNSW AM, using pgvector's page layout and GenericXLog.
- `crates/_support/types/types_hnsw`.
```
- Replace the paragraph starting with ``halfvec`, `sparsevec`, the `bit` opclasses and `ivfflat` are **not ported**`` with:
```markdown
The HNSW opclasses for `halfvec`, `sparsevec` and `bit` (M3) and `ivfflat` (M4) are **not ported**, and the extension script is trimmed to match.
```
- In the harness command block, add after the `run-iterscan.sh` line:
```bash
scripts/pgvector/run-diff.sh {pgrust|ref}                       # exact differential: diffrunner --pgvector + error LOCATIONs
scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file|->   # one SQL script on fresh ref and subject servers, diffed
scripts/pgvector/upstream-sql.sh 'SECTION' ...                  # upstream extension-script sections for the trimmed script
```

- [ ] **Step 4: Full run of every tier**

```bash
scripts/pgvector/build-reference.sh --verify
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-all.sh ref
scripts/pgvector/run-all.sh pgrust
scripts/pgvector/tests/harness_test.sh
UT_M2
UT_PARITY
UT_FUZZ pgvector:: rulings:: ruled::
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
for t in crates/pgvector-0.8.7-reference crates/postgres-18.6-reference; do git status --porcelain --ignored "$t"; done
```
Expected:
- `run-all.sh ref` passes 94/94: regress 14, TAP 48, bytecmp 10, iterscan 7, diff 15.
- `run-all.sh pgrust` passes 57/94: regress 8/14 (`bit btree cast copy halfvec sparsevec vector_type hnsw_vector`), TAP 17/48 (M1's 14 plus `018`, `033`, `034`), bytecmp 10/10, iterscan 7/7, diff 15/15.
- Every remaining failure is HNSW on the new types (M3), IVFFlat (M4) or parallel builds (M6). Confirm that from the diffs: the regress diffs now say `operator class ... does not exist for access method "hnsw"` or `access method "ivfflat" does not exist`.
- `harness_test.sh`: `all passed`.
- All unit tests pass, and both lints PASS.
- The two reference trees print nothing.

If the TAP 018 stop condition from Task 6 was hit and the user accepted it, 018 is the one expected exception. Write it into the report.

- [ ] **Step 5: Write `docs/superpowers/reports/pgvector-m2-report.md`**

Follow `docs/superpowers/reports/pgvector-m1-report.md`'s structure, filled from the Step 4 runs:
1. **Header line:** the date, the pgrust code commit (the last Task 8 commit), and the references (`pgvec`, `pgvec-seeded`).
2. **Summary:**
   - C reference X/94 and pgrust Y/94, broken down by tier;
   - the files and tests M2 fixed;
   - the gates that are also green: `harness_test.sh`, the `UT_M2`, `UT_PARITY` and `UT_FUZZ` totals, both lints, `build-reference.sh --verify`, and the clean reference trees.
3. **pgrust results:** the full table from `~/.cache/pgrust/pgvector-work/report-pgrust.md`.
4. **Remaining failures:** each with its cause and milestone, checked against the logs.
5. **What M2 established:**
   - f16 parity for every input, against both C paths;
   - halfvec arithmetic equal to native `_Float16`;
   - the `diff` tier clean on seeds 1 and 2, with the ruled counts from Task 8 Step 5;
   - error locations identical at all 49 reachable sites.
6. **Known divergences left open:**
   - pre-M2 `vector` error locations (`pgvector.c:0`; `halfvec_to_vector`/`sparsevec_to_vector` inherit them through vector's CheckDim);
   - `pg_get_loaded_modules()` version (M4);
   - the float-order ruling and what it covers;
   - anything Task 8 triage left to the user.
7. **Coverage limits:**
   - `wasm/wasm-build.sh` was not run (toolchain `nightly-2026-07-17` not installed); the parity crate is wasm-safe by construction;
   - the x86 F16C/AVX-512 dispatch paths of the C reference are not exercised on this arm64 host (expected first on Linux, M5);
   - binary-only error sites (the recv functions) are covered by unit tests, not by the LOCATION check;
   - carry forward M1's two M3 prerequisites unchanged (`pgvector-m1-report.md`, "Coverage limits").

- [ ] **Step 6: Commit**

```bash
git add crates/contrib/pgvector/src/lib.rs docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m2-report.md
git commit -q -F - <<'EOF'
docs(pgvector): M2 report; spec, CLAUDE.md and crate header at M2

Records the M2 conformance run (halfvec, sparsevec, bit functions),
corrects the spec's unit and exact-differential rows to what was built,
and removes the orphaned crates/contrib/pgvector/sql files (spec §4.1).

Co-Authored-By: <the implementing model>
EOF
git log --oneline main..HEAD
```
Expected: 10 commits on `vector/m2`, the plan plus Tasks 1–9. Do not merge or push. The controller does that after the final review and the user's go-ahead.

# pgvector Phase 1 · M3: HNSW for `halfvec`, `sparsevec` and `bit` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give pgrust's HNSW access method pgvector 0.8.7's type info and the ten HNSW opclasses for `halfvec`, `sparsevec` and `bit`. First close M1's two scan prerequisites and fuse the sparse L2 merge per target. Prove the result with five checks:
- regress `hnsw_bit`, `hnsw_halfvec` and `hnsw_sparsevec` pass;
- every HNSW TAP test passes on serial builds;
- seeded builds of every type are byte-identical to C's;
- iterative scans of every type stop where C stops, with the same per-batch memory trace;
- every SQL-reachable HNSW error site matches C, LOCATION included.

**Architecture:**
- **Type info.** `types_hnsw` gains `HnswTypeInfo` (`hnsw.h:291-297`). `pgvector_hnsw::utils` holds hnswutils.c's four static tables and resolves an opclass's support proc 3 by its C symbol (`pg_proc.prosrc`) to one of them, the way amapi resolves `hnswhandler` (spec §5.4). Every type-specific step dispatches through it: the dimension count, the value check, normalization and the maximum dimensions.
- **Extension script.** It gains the three `hnsw_*_support` functions and the ten opclasses, through a new `--only-am hnsw` mode of `upstream-sql.sh`. `pgvector`'s `lookup` gets stubs for the three symbols; nothing ever calls them.
- **Before the scan code changes:**
  - An opt-in placeholder, `pgrust.hnsw_memory`, makes pgrust scans print C's `-DHNSW_MEMORY` per-batch line, `memory: N KB, tuples: T`. The iterative-scan tier diffs those lines against the seeded C build, which closes M1 prerequisite 1.
  - A durable `hnsw_errors` row in the diff tier covers every SQL-reachable HNSW error site, which closes M1 prerequisite 2.
- **Kernels.** The sparse L2 merge fuses its scalar multiply-adds on aarch64, as both C builds do. The half kernels slice their arrays once per call.

**Tech Stack:** Rust (the pgrust workspace, `fast-profile`), bash 3.2 harness scripts, PostgreSQL 18.6 + pgvector 0.8.7 C reference builds (`pgvec`, `pgvec-seeded`), `pg_regress`, Perl TAP (`prove`), `crates/bin/pagemask`.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`: §4.3 (`HnswTypeInfo`), §5.4 (HNSW across all four types), §8.2 (tiers), §9 M3 row. Inputs:
- `docs/HANDOFF_2026-10-06.md` §1, which lists what this plan must include;
- `docs/superpowers/reports/pgvector-m2-report.md`, the "Known divergences left open" and "Coverage limits" sections;
- `docs/superpowers/reports/pgvector-m1-report.md`, "Coverage limits": the two M3 prerequisites;
- `~/.cache/pgrust/m2-sdd-ledger.md`: the M2 deferred minors. M3 takes three of them: the `hx` slicing, the sparse fusion and `grep -qxF`.

Facts verified while writing this plan (2026-10-06, macOS arm64; "C" is the reference build unless stated):
- **Support proc 3 can't be called from SQL.** C answers `SELECT hnsw_halfvec_support(NULL)` with `function hnsw_halfvec_support(unknown) does not exist`. The `lookup` stubs are therefore unreachable.
- **HNSW error sites (C, `\set VERBOSITY verbose`):**
  - `InitBuildState`:
    - `hnswbuild.c:703`: 0A000 `type not supported for hnsw index` (a varbit attribute);
    - `:709`: 22023 `column does not have dimensions`;
    - `:714`: 54000 `column cannot have more than N dimensions for hnsw index`, with N = 2000, 4000 or 64000;
    - `:719`: 22023 `ef_construction must be greater than or equal to 2 * m`.
  - `SparsevecCheckValue`, `hnswutils.c:1392`: 54000 `sparsevec cannot have more than 1000 non-zero elements for hnsw index`, at both build and insert.
  - `HnswCheckDim`, `hnswutils.c:1373`: 22000 `expected 3 dimensions, not 2`, for every type and opclass, even on an empty index.
  - **What pgrust reports today:** `pgvector_hnsw_build.c:0` for 709, 714 and 719. It has no varbit check and no nnz check.
  - The full statement list (Task 2 and Task 5) gives 22 errors on C. Both C builds agree once the seeded build's `INFO:  memory:` lines are dropped.
- **Sparse kernels**, from the disassembly of the macOS clang arm64 reference and of the Docker image's gcc arm64 `vector.so`:
  - **L2 merge.** Both builds emit `fsub` then `fmadd` for every scalar step. 12 of 12 random pairs (dim 6) equal a fused computation on arm64. On the amd64 Docker image under emulation (`OPTFLAGS=""`), they equal an unfused one.
  - **L1 merge.** Both builds emit `fabd`/`fabs` and `fadd`. There is no multiply to fuse, and pgrust's source order already matches.
  - **Tails.** The trailing loop over b-only entries is vectorized once it has 4 or more entries, and the two compilers do it differently:
    - clang uses 16-wide blocks (4 accumulators, the first seeded with the running sum), then 4-wide blocks, then a scalar tail;
    - gcc sums 4-wide blocks in one lane vector, adds that to the running sum, then runs a scalar tail.
  - **Example.** `{1:0.8976931}/9` against a vector with 8 trailing entries gives `1.4069117307662964` on C, while a sequential fused sum gives `1.4069116115570068`.
  - **Decision.** Because the two C builds disagree with each other on long tails, M3 fuses the scalar steps only and records the tails as a `DIVERGENCE` (Task 3). That follows the user's M2 choice not to emulate C's vectorized reduction for `vector`. The M3 report flags it for M5.
- **Seeded builds at scale.** A 10,000-row seeded `vector` build is byte-identical between `pgvec-seeded` and current pgrust. C's in-memory graph (6 MB) spans several 1 MB Generation blocks, so M1's unverified SelectNeighbors tie-break holds at that size. On that index, current pgrust stops iterative scans where C does at caps of 64 kB, 128 kB (relaxed, strict and `LATERAL`), 256 kB and 4 MB.
- **New-type cases on `pgvec-seeded`:**
  - **Task 6's bytecmp cases.** Each builds a 39–42-block, level-2 index, except `wide_sparsevec`: 201 blocks, level 1, 100 distinct values.
  - **Task 6's iterscan cases.** Each stops on the 64 kB cap: 139, 132, 139, 223, 663 and 608 rows removed out of 990.
  - **Task 1's `big` cases:** 751 (64 kB), 6545 (256 kB), 2669 (strict, 128 kB), 4727 (`ef_search` 100, 128 kB), 9990 (4 MB). Every loop of the `LATERAL` case hits the cap, returning 3, 3 and 2 rows.
- **pgrust core.** It accepts a `bit` opclass on a `varbit` column like C does, and `('x' || md5(..))::bit(64)` gives C's bits (`sqldiff.sh`: identical).

## Global Constraints

- **Branch:** work on `vector/m3`, cut from `main`. Merge locally with a fast-forward when done, and push only when the user asks. Work in the main checkout, not a worktree, because the disk is about 92% full.
- **Commit trailer:** every commit message ends with a `Co-Authored-By:` trailer naming the model that wrote the commit, as in M1 and M2: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. Never edit them and never build inside them. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md):
  - Keep C function names, and make comments cite C `file.c:line`. pgvector citations point into `crates/pgvector-0.8.7-reference/src/`.
  - Mark intentional differences `DIVERGENCE`.
- **Error locations:** every error that mirrors a C `ereport`/`elog` carries `.with_location("<file>", <line>, "<CFunction>")`.
  - `<line>` is the last line of the C call (the line holding its closing `);`).
  - The values in this plan's code are the verified ones; copy them exactly.
- **Extension SQL:**
  - `crates/contrib/pgvector/extension/vector--0.8.5.sql` stays the shipped script, and `default_version = '0.8.5'` stays (spec §4.6).
  - M3 appends upstream text only through `scripts/pgvector/upstream-sql.sh` (Task 5's `--only-am hnsw`), never by hand. Still no `COMMENT ON` lines and no IVFFlat blocks; those belong to M4.
- **Defaults stay stock C:** `pgrust.hnsw_build_seed` and `pgrust.hnsw_memory` are placeholder options, read through `guc_seams::get_config_option_missing_ok`. They are **never** registered in `guc_tables`. Unset, nothing is reseeded and nothing extra is printed.
- **Never widen `docs/fuzzing/rulings.toml` without the user.** Upstream expected outputs are never edited.
- **No new external dependencies.** New path dependencies on workspace crates are fine: `pgvector_hnsw` gains `guc_seams`, `adt_varbit` and `syscache_seams`.
- **Unit-test commands.** They reuse the `target/fast-profile` artifacts the server build produced. Listing `main_main` keeps feature resolution identical to the server build. Never run them with an empty filter.
  - `UT <filter...>` means:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw --lib -- <filter...>
    ```
  - `UT_M3` is every unit test M3 adds or changes (25 tests):
    ```
    UT parse_show_memory charge_norm_value type_info hnsw_check_dim sparsevec_check_value hnsw_norm_value halfutils:: sparsevec:: lookup_cover
    ```
  - `UT_KEEP` is the M1 and M2 tests these crates already have; they must stay green:
    ```
    UT tmpctx:: chain_walk update_connection seed_random level_for_uniform candidate_heaps hnsw_get_max_level parse_build_seed vector_combine bitutils:: bitvec:: halfvec:: funcs:: vec::
    ```
- **Cargo.lock:** adding a path dependency changes `Cargo.lock`.
  - After any `Cargo.toml` edit, run `cargo update --workspace --offline` before the next `--locked` command.
  - Check that `git diff --stat Cargo.lock` touches only the edited package's dependency list, then commit `Cargo.lock` with the task.
- **Server build:** `scripts/pgvector/build-pgrust.sh` builds `target/fast-profile/postgres`, `pagemask` and `diffrunner`. Every conformance check needs it rebuilt after a Rust change.
- **Harness commands** (from the repo root):
  - Regression: `scripts/pgvector/run-regress.sh pgrust <test ...>`. Results go to `~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv`, diffs to `regression.diffs`.
  - TAP: `scripts/pgvector/run-tap.sh pgrust <NNN_name.pl ...>`. Logs go to `~/.cache/pgrust/pgvector-work/tap/pgrust/prove/<name>.log`.
  - One SQL script on fresh C and subject servers: `scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file|->`.
  - Tiers: `run-bytecmp.sh`, `run-iterscan.sh`, `run-diff.sh`, each `{pgrust|ref}`. `run-all.sh {pgrust|ref}` runs all five tiers.
  - Ports: pgrust 55491, C reference 55492, Docker reference 55493, seeded C reference 55494.
- **Test lists:**
  - `M3_REGRESS` means `bit btree cast copy halfvec sparsevec vector_type hnsw_vector hnsw_bit hnsw_halfvec hnsw_sparsevec`.
  - `HNSW_TAP` (vector, must stay green) means `010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 019_storage.pl 037_inputs.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl`.
  - `M3_TAP` means `020_hnsw_bit_build_recall.pl 021_hnsw_bit_insert_recall.pl 022_hnsw_bit_vacuum_recall.pl 023_hnsw_bit_duplicates.pl 024_hnsw_halfvec_build_recall.pl 025_hnsw_halfvec_insert_recall.pl 026_hnsw_halfvec_vacuum_recall.pl 027_hnsw_halfvec_duplicates.pl 028_hnsw_sparsevec_build_recall.pl 029_hnsw_sparsevec_insert_recall.pl 030_hnsw_sparsevec_vacuum_recall.pl 031_hnsw_sparsevec_duplicates.pl 038_hnsw_sparsevec_vacuum_insert.pl`.
- **Harness scripts:** `#!/usr/bin/env bash`, `set -euo pipefail`, bash 3.2 compatible. That rules out associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, and bare `"${arr[@]}"` on a possibly empty array under `set -u` (use `${arr[@]+"${arr[@]}"}`).
- **Lints** (shell scripts, no build), from the repo root. Both must pass at the end of every task that touches Rust:
  - `bash crates/_support/seams_init/tests/lint-determinism.sh`
  - `bash crates/_support/seams_init/tests/lint-seam-installs.sh`
- **wasm:** the wasm ledger (`wasm/wasm-crate-ledger.md`) is ratchet-only, so never add a row. `wasm/wasm-build.sh` needs `nightly-2026-07-17`, which isn't installed. It is not run in M3, and the report says so.
- **Disk:** about 158 GiB free at plan time. Before Task 1, if `df -h .` shows under 120 GiB free, delete `~/.cache/pgrust/pgvector-work/tap`. It holds old TAP logs and is regenerable.
- **The plan's Rust code** mirrors patterns that compile today. If a snippet doesn't compile as written:
  - make the smallest fix and name it in the task report;
  - never change behavior to make code compile.

## Review Focus

1. **Values at a type's size limit.** A `sparsevec` with 1,000 non-zeros is 8,016 bytes and is TOASTed in the heap. Its element tuple plus neighbor tuple exceed `HNSW_MAX_SIZE`, so the build puts each neighbor tuple on the next page, and inserts after the build append pages. The pages must match C's.
   Test: the bytecmp case `wide_sparsevec` (Task 6), a build plus inserts.
2. **Normalization that drops elements.** `sparsevec_l2_normalize` drops values that underflow to zero.
   - A stored value shrinks.
   - Normalizing a query charges two vectors and a `pfree` to the scan's `tmpCtx`.
   Tests: `hnsw_norm_value_dispatches_by_type` and `charge_norm_value_matches_c` (Task 4), and the iterscan case `sparsevec_cosine_drop_64k` (Task 6).
3. **The 1,000-non-zero limit at its boundary, on both write paths, before normalization.**
   - 1,000 non-zeros index.
   - 1,001 raise 54000 at build and at insert, with cosine opclasses too, because `SparsevecCheckValue` runs before `HnswCheckNorm`.
   Tests: `sparsevec_check_value_caps_at_1000` (Task 4) and the sparsevec block of `hnsw-errors.sql` (Task 5).
4. **A support proc 3 that isn't one of pgvector's**, from a hand-written opclass or a catalog from elsewhere. C calls it and trusts the pointer it returns. pgrust must raise a clean error, never panic or guess a type.
   Test: `type_info_resolves_only_pgvector_symbols` (Task 4).
5. **Binary quantization through an expression index**, pgvector's documented pattern: `CREATE INDEX ... USING hnsw ((binary_quantize(v)::bit(64)) bit_hamming_ops)`. The index attribute is the cast's `bit(64)`, not the vector column, so its typmod gives the dimensions.
   Test: the bytecmp case `bq_expression` (Task 6).

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/_support/types/types_hnsw/src/lib.rs` | `HNSW_MAX_NNZ`, `HnswTypeInfo`; `HnswScanOpaqueData` gains `type_info` and `show_memory` and drops `max_dimensions` and `norm_is_l2` | 1, 4 |
| `crates/contrib/pgvector_hnsw/Cargo.toml`, `Cargo.lock` | `guc_seams`, `adt_varbit`, `syscache_seams` | 1, 4 |
| `crates/contrib/pgvector_hnsw/src/scan.rs` | `pgrust.hnsw_memory` and `ShowMemoryUsage`; type info in begin and get-tuple; normalization charges; drops `norm_value` | 1, 4 |
| `crates/contrib/pgvector_hnsw/src/utils.rs` | the four type-info tables, `hnsw_get_type_info`, `SparsevecCheckValue`, `hnsw_check_dim` by type, `hnsw_norm_value`; drops `check_type_supported` | 4 |
| `crates/contrib/pgvector_hnsw/src/insert.rs` | `form_index_value` and `insert_tuple_on_disk` take the type info | 4 |
| `crates/contrib/pgvector_hnsw_build/src/lib.rs` | `InitBuildState` locations, the varbit check, type info in the build | 2, 4 |
| `crates/contrib/pgvector/src/sparsevec.rs` | `fmadd_step`, the fused L2 merge, its per-target test | 3 |
| `crates/contrib/pgvector/src/halfutils.rs` | slice-once half kernels and their test | 3 |
| `crates/contrib/pgvector/src/lib.rs` | header lines; `lookup` stubs for the three `hnsw_*_support` symbols | 3, 5 |
| `crates/contrib/pgvector/extension/vector--0.8.5.sql` | the appended support functions and opclasses | 5 |
| `scripts/pgvector/upstream-sql.sh` | `--only-am AM`; `grep -qxF` | 5 |
| `scripts/pgvector/run-iterscan.sh` | the memory trace, an `ef_search` field, case arguments, `big` and the type cases | 1, 6 |
| `scripts/pgvector/run-bytecmp.sh` | `big_golomb` and the type cases | 1, 6 |
| `scripts/pgvector/sql/hnsw-errors.sql` | **new:** every SQL-reachable HNSW error site | 2, 5 |
| `scripts/pgvector/sqldiff.sh` | drops the seeded build's `memory:` INFO lines | 2 |
| `scripts/pgvector/run-diff.sh` | the `hnsw_errors` row | 2 |
| `scripts/pgvector/tests/harness_test.sh` | iterscan, diff and upstream-sql self-tests | 1, 2, 5, 6 |
| `crates/contrib/pgvector_hnsw/src/lib.rs` | crate header | 7 |
| spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m3-report.md` | docs and the M3 report | 7 |

## Execution notes

- **Task order.** Tasks run in order, 1 to 7.
  - Task 1 must land before Task 4 rewrites `scan.rs`; that ordering is M1 prerequisite 1.
  - Tasks 2 and 3 don't depend on Task 1, but they stay in sequence so the gates stay simple.
  - Task 5 needs Task 4: until the type info exists, the new opclasses would fail.
  - Task 6 needs Task 5, and Task 7 needs everything.
- **Suggested implementers:**
  - Opus for Tasks 1, 6 and 7: harness integration, the memory-trace triage, and the TAP and report gates.
  - Sonnet for Tasks 2, 3, 4 and 5: the code is spelled out.
  - Opus does every review and the final whole-branch review.
- **Expected totals after Task 7:**
  - `run-all.sh ref`: 120/120 (regress 14, TAP 48, bytecmp 21, iterscan 19, diff 18).
  - `run-all.sh pgrust`: 96/120 (regress 11, TAP 27, bytecmp 21, iterscan 19, diff 18).

---

### Task 1: Branch, `pgrust.hnsw_memory`, and the per-batch memory diff in the iterative-scan tier

**Files:**
- Modify: `crates/_support/types/types_hnsw/src/lib.rs` (`HnswScanOpaqueData`)
- Modify: `crates/contrib/pgvector_hnsw/Cargo.toml` (+`guc_seams`), `Cargo.lock`
- Modify: `crates/contrib/pgvector_hnsw/src/scan.rs`
- Modify: `scripts/pgvector/run-iterscan.sh` (rewritten), `scripts/pgvector/run-bytecmp.sh` (`big_golomb`)
- Modify: `scripts/pgvector/tests/harness_test.sh` (`test_iterscan_cases`, new `test_iterscan_memory_off`)

**Interfaces:**
- Produces:
  - `HnswScanOpaqueData::show_memory: bool`.
  - The placeholder `pgrust.hnsw_memory`: `on`, `true`, `yes` or `1` (case-insensitive) turns the trace on.
  - `run-iterscan.sh {pgrust|ref} [case ...]`.
  - The CASES line format `name:iterative:work_mem:multiplier:max_tuples:ef_search:query`. Task 6 adds lines, tables and queries.
  - `PGV_ITERSCAN_MEMORY` (default `on`; set it empty to omit the `SET`).
  - The bytecmp case `big_golomb`.

- [ ] **Step 1: Branch and commit the plan**

```bash
cd /Users/jody/dev_projects/pgrust
df -h . | tail -1
git checkout -b vector/m3
git add docs/superpowers/plans/2026-10-06-pgvector-m3-hnsw-types.md
git commit -q -F - <<'EOF'
docs(pgvector): M3 plan (HNSW for halfvec, sparsevec and bit)

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

- [ ] **Step 2: Write the failing unit test**

Append to the end of `crates/contrib/pgvector_hnsw/src/scan.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // pgrust.hnsw_memory is on only when set to a true boolean spelling.
    #[test]
    fn parse_show_memory_accepts_on_values() {
        for v in ["on", "ON", " true ", "yes", "1"] {
            assert!(parse_show_memory(Some(v.to_string())), "{v}");
        }
        for v in ["off", "0", "", "2"] {
            assert!(!parse_show_memory(Some(v.to_string())), "{v}");
        }
        assert!(!parse_show_memory(None));
    }
}
```

- [ ] **Step 3: Run it to see it fail**

Run: `UT parse_show_memory`
Expected: a compile error, `cannot find function 'parse_show_memory'`.

- [ ] **Step 4: Implement the opt-in trace**

1. In `crates/contrib/pgvector_hnsw/Cargo.toml`, add after the `guc_tables` line:
   ```toml
   guc_seams = { path = "../../backend/utils/misc/guc_seams" }
   ```
   Then run `cargo update --workspace --offline`.
2. In `crates/_support/types/types_hnsw/src/lib.rs`, add a last field to `HnswScanOpaqueData`, after `discarded`:
   ```rust
       // pgrust.hnsw_memory at hnswbeginscan: print C's -DHNSW_MEMORY
       // ShowMemoryUsage line after each search batch.
       pub show_memory: bool,
   ```
3. In `crates/contrib/pgvector_hnsw/src/scan.rs`, add above `hnswbeginscan`:
   ```rust
   // pgrust.hnsw_memory: a placeholder customized option, deliberately NOT a
   // registered GUC (like pgrust.hnsw_build_seed). Set on, scans print the line
   // C's -DHNSW_MEMORY build prints after each search batch (ShowMemoryUsage,
   // hnswscan.c:126-129, called at :243 and :290), so the iterative-scan tier
   // can diff every batch's tmpCtx size and tuple count against the seeded C
   // build. DIVERGENCE (opt-in only): stock C has no such switch, and pgrust
   // prints only the scan's lines, not -DHNSW_MEMORY's build and vacuum ones
   // (their figures depend on C's allocators, which pgrust does not model).
   fn hnsw_show_memory() -> bool {
       // Uninstalled seam (unit-test binaries without a guc boot): stock C.
       if !guc_seams::get_config_option_missing_ok::is_installed() {
           return false;
       }
       parse_show_memory(
           guc_seams::get_config_option_missing_ok::call("pgrust.hnsw_memory")
               .ok()
               .flatten(),
       )
   }

   fn parse_show_memory(value: Option<String>) -> bool {
       value.is_some_and(|v| {
           matches!(v.trim().to_ascii_lowercase().as_str(), "on" | "true" | "yes" | "1")
       })
   }

   // ShowMemoryUsage (hnswscan.c:126-129).
   fn show_memory_usage(so: &HnswScanOpaqueData<'_>) -> PgResult<()> {
       elog::ereport(types_error::INFO)
           .errmsg_internal(format!(
               "memory: {} KB, tuples: {}",
               so.tmp_ctx.mem_allocated() / 1024,
               so.tuples
           ))
           .finish(types_error::ErrorLocation::new("hnswscan.c", 128, "ShowMemoryUsage"))
   }
   ```
4. In `hnswbeginscan`'s `HnswScanOpaqueData { ... }` literal, add after `discarded: None,`:
   ```rust
           show_memory: hnsw_show_memory(),
   ```
5. In `hnswgettuple`, add the two calls where C calls `ShowMemoryUsage`:
   - after `opaque(scan).first = false;`:
     ```rust
             if opaque(scan).show_memory {
                 show_memory_usage(opaque(scan))?;
             }
     ```
   - in the `need_resume` branch, between `r?;` and `if opaque(scan).w.is_empty() {`:
     ```rust
                 if opaque(scan).show_memory {
                     show_memory_usage(opaque(scan))?;
                 }
     ```

- [ ] **Step 5: Run the unit test and rebuild**

```bash
UT parse_show_memory
scripts/pgvector/build-pgrust.sh
```
Expected: 1 passed. The server builds.

- [ ] **Step 6: Rewrite `scripts/pgvector/run-iterscan.sh`**

Replace the whole file with:
```bash
#!/usr/bin/env bash
# Iterative-scan tier (spec §8.2). Builds the same seeded HNSW indexes on the
# seeded C reference (the oracle) and on the subject, then runs iterative
# index scans that stop on hnsw.max_scan_tuples or on the scan memory cap,
# MemoryContextMemAllocated(so->tmpCtx) > work_mem *
# hnsw.scan_mem_multiplier (hnswscan.c:264). Each filter matches 10 rows and
# each scan asks for 11, so every scan runs until a limit stops it; the rows
# it returns and "Rows Removed by Filter" show where it stopped.
# Each case also compares the scan's memory trace. The seeded C build
# (-DHNSW_MEMORY) prints "memory: <KB> KB, tuples: <n>" after every search
# batch (ShowMemoryUsage, hnswscan.c:126-129, called at :243 and :290), and
# pgrust prints the same line with SET pgrust.hnsw_memory = on. So the
# modeled tmpCtx is compared after every batch, not only where a cap stops.
#
# usage: run-iterscan.sh {pgrust|ref} [case ...]   (default: every case)
#        run-iterscan.sh --list
#   pgrust: the subject is pgrust (SET pgrust.hnsw_build_seed = 42).
#   ref:    the subject is a second, fresh seeded C cluster (must always pass).
#   PGV_ITERSCAN_MEMORY= (empty) omits SET pgrust.hnsw_memory (self-test of
#           the opt-in trace; default on).
#   PGV_KEEP_DATA=1 keeps each side's cluster under $PGV_WORK/iterscan/<mode>/data
#           (default: deleted after a successful side, to save disk).
set -euo pipefail
. "$(dirname "$0")/common.sh"

# name:hnsw.iterative_scan:work_mem:hnsw.scan_mem_multiplier:hnsw.max_scan_tuples:hnsw.ef_search:query
CASES="relaxed_64k:relaxed_order:64kB:1:20000:40:knn
relaxed_64k_x2:relaxed_order:64kB:2:20000:40:knn
strict_64k:strict_order:64kB:1:20000:40:knn
relaxed_128k:relaxed_order:128kB:1:20000:40:knn
relaxed_tuples:relaxed_order:4MB:1:300:40:knn
strict_tuples:strict_order:4MB:1:300:40:knn
lateral_64k:relaxed_order:64kB:1:20000:40:lateral
big_relaxed_64k:relaxed_order:64kB:1:20000:40:bigknn
big_relaxed_256k:relaxed_order:256kB:1:20000:40:bigknn
big_strict_128k:strict_order:128kB:1:20000:40:bigknn
big_ef100_128k:relaxed_order:128kB:1:20000:100:bigknn
big_lateral_128k:relaxed_order:128kB:1:20000:40:biglateral
big_exhaust_4MB:relaxed_order:4MB:1:20000:40:bigknn"
MEMORY="${PGV_ITERSCAN_MEMORY-on}"

# tst: tie-free Golomb-ruler data (see run-bytecmp.sh l2_golomb), so the
# graph and every scan order follow from the seed-42 level stream alone.
# big: the same construction at 10,000 rows (p = 10007). Its values pass
# 2^24, so f32 rounds them, identically on both sides. Its index is
# byte-identical to C's (run-bytecmp.sh big_golomb), and C's in-memory graph
# spans several 1 MB blocks.
setup_sql() {
  cat <<'SQL'
SET max_parallel_maintenance_workers = 0;
SET maintenance_work_mem = '64MB';
SET pgrust.hnsw_build_seed = 42;
CREATE EXTENSION vector;
CREATE TABLE tst (i int4, v vector(3));
INSERT INTO tst SELECT i, ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector FROM (SELECT i, (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX tst_v_idx ON tst USING hnsw (v vector_l2_ops);
CREATE TABLE big (i int4, v vector(3));
INSERT INTO big SELECT i, ARRAY[2*10007*k + (k*k) % 10007, 0, 0]::vector FROM (SELECT i, (i * 37) % 10007 AS k FROM generate_series(0, 9999) i) s;
CREATE INDEX big_v_idx ON big USING hnsw (v vector_l2_ops);
SQL
}

query_sql() {
  case "$1" in
    knn) echo "SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> '[1000000,0,0]' LIMIT 11" ;;
    # Three rescans of one index scan (M1 review focus 4: each must start from
    # a reset tmpCtx and fresh visited/discarded state).
    lateral) echo "SELECT q.x, t.i FROM (VALUES (1000000), (250000), (1750000)) q(x) CROSS JOIN LATERAL (SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    bigknn) echo "SELECT i FROM big WHERE i % 1000 = 0 ORDER BY v <-> '[100000000,0,0]' LIMIT 11" ;;
    # Every one of the three loops hits the 128kB cap.
    biglateral) echo "SELECT q.x, t.i FROM (VALUES (100000000), (25000000), (175000000)) q(x) CROSS JOIN LATERAL (SELECT i FROM big WHERE i % 1000 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    *) die "unknown query '$1'" ;;
  esac
}

case_sql() { # case_sql ITERATIVE WORK_MEM MULTIPLIER MAX_TUPLES EF_SEARCH QUERY
  # C keeps pgrust.hnsw_memory as an unused placeholder.
  [ -z "$MEMORY" ] || echo "SET pgrust.hnsw_memory = $MEMORY;"
  cat <<SQL
SET enable_seqscan = off;
SET hnsw.iterative_scan = $1;
SET work_mem = '$2';
SET hnsw.scan_mem_multiplier = $3;
SET hnsw.max_scan_tuples = $4;
SET hnsw.ef_search = $5;
EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF, BUFFERS OFF) $(query_sql "$6");
$(query_sql "$6");
SQL
}

# The CASES line of case NAME.
case_line() {
  local line
  line="$(printf '%s\n' "$CASES" | grep "^$1:" || true)"
  [ -n "$line" ] || die "unknown case '$1' (see run-iterscan.sh --list)"
  echo "$line"
}

# run_side LABEL KIND: fresh cluster, build the indexes, run every case, stop.
# Keeps the "Rows Removed by Filter" lines and the result rows, then appends
# the memory trace in emission order (psql prints INFO on stderr).
run_side() {
  local label="$1" kind="$2" port c name iter wm mult maxt ef query
  port="$(port_for "$kind")"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  setup_sql | pgv_psql "$port" -f - >"$out/$label.setup.log" 2>&1 ||
    die "$label ($kind): setup failed; see $out/$label.setup.log"
  if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$out/$label.setup.log"; then
    die "$label: index was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
  fi
  for c in "${cases[@]}"; do
    IFS=: read -r name iter wm mult maxt ef query <<EOF
$(case_line "$c")
EOF
    case_sql "$iter" "$wm" "$mult" "$maxt" "$ef" "$query" >"$out/$label.$name.sql"
    pgv_psql "$port" -At -f "$out/$label.$name.sql" 2>"$out/$label.$name.err" |
      sed -n -e 's/^ *\(Rows Removed by Filter: [0-9]*\)$/\1/p' -e '/^[0-9][0-9|]*$/p' \
        >"$out/$label.$name.out" ||
      die "$label ($kind): case $name failed; see $out/$label.$name.err"
    sed -n 's/^.*INFO:  \(memory: [0-9]* KB, tuples: [0-9]*\)$/\1/p' \
      "$out/$label.$name.err" >>"$out/$label.$name.out"
    if [ "$kind" = seeded ] && ! grep -q '^memory: ' "$out/$label.$name.out"; then
      die "$label: case $name printed no 'memory:' lines (is $PG_VEC_SEEDED the -DHNSW_MEMORY build?)"
    fi
  done
  server_stop "$kind"
  # Nothing reads the cluster after this; PGV_KEEP_DATA=1 keeps it.
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$label"
}

if [ "${1:-}" = --list ]; then
  printf '%s\n' "$CASES" | cut -d: -f1
  exit 0
fi
[ "$#" -ge 1 ] || die "usage: run-iterscan.sh {pgrust|ref} [case ...]"
mode="$1"
shift
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
if [ "$#" -gt 0 ]; then
  cases=("$@")
else
  # shellcheck disable=SC2207 # intentional splitting: case names have no spaces
  cases=($(printf '%s\n' "$CASES" | cut -d: -f1))
fi
for c in "${cases[@]}"; do case_line "$c" >/dev/null; done
out="$PGV_WORK/iterscan/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
total=0
passed=0
for name in "${cases[@]}"; do
  total=$((total + 1))
  if [ -s "$out/oracle.$name.out" ] && cmp -s "$out/oracle.$name.out" "$out/subject.$name.out"; then
    result=ok
    passed=$((passed + 1))
  else
    result=FAIL
    diff "$out/oracle.$name.out" "$out/subject.$name.out" >"$out/diff/$name.txt" || true
  fi
  printf '%s\t%s\n' "$name" "$result" | tee -a "$out/summary.tsv"
done
echo "iterscan ($mode, subject $subject): $passed/$total identical; diffs in $out/diff"
[ "$passed" -eq "$total" ]
```

- [ ] **Step 7: Add `big_golomb` to `scripts/pgvector/run-bytecmp.sh`**

Append ` big_golomb` to the end of the `CASES="..."` string. In `case_sql`, add this arm before `spill)`:
```bash
    big_golomb)
      # 10,000 rows (p = 10007): C's in-memory graph (about 6 MB) spans
      # several 1 MB Generation blocks, so this checks that SelectNeighbors'
      # tie-break by element id still follows C's by address (M1 report).
      # run-iterscan.sh's `big` table builds this same index.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, ARRAY[2*10007*k + (k*k) % 10007, 0, 0]::vector FROM (SELECT (i * 37) % 10007 AS k FROM generate_series(0, 9999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
```

- [ ] **Step 8: Self-tests**

In `scripts/pgvector/tests/harness_test.sh`:
1. In `test_iterscan_cases`, change the declaration line `local a b t l` to `local a b t l x f n`, and append before the function's closing `}`:
   ```bash
     # big: the 64kB and 256kB caps stop early, at increasing depths; 4MB scans
     # the whole index (9,990 rows fail its filter); every LATERAL loop hits the
     # 128kB cap.
     a="$(removed big_relaxed_64k)"
     b="$(removed big_relaxed_256k)"
     if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ] && [ "$b" -lt 9990 ]; then pass "iterscan big caps stop early ($a, $b removed)"; else fail "iterscan big caps: removed '$a', '$b', want increasing and < 9990"; fi
     a="$(removed big_exhaust_4MB)"
     if [ "$a" = 9990 ]; then pass "iterscan big_exhaust_4MB scans the whole index"; else fail "iterscan big_exhaust_4MB: removed '$a', want 9990"; fi
     for x in 25000000 100000000 175000000; do
       l="$(grep -c "^$x|" "$PGV_WORK/iterscan/ref/oracle.big_lateral_128k.out" 2>/dev/null || true)"
       if [ -n "$l" ] && [ "$l" -lt 10 ]; then pass "iterscan big_lateral_128k loop $x hits the cap ($l of 10 rows)"; else fail "iterscan big_lateral_128k loop $x: '$l' rows, want < 10"; fi
     done
     # Every case compares a memory trace.
     for f in "$PGV_WORK"/iterscan/ref/oracle.*.out; do
       n="$(grep -c '^memory: ' "$f" || true)"
       if [ "$n" -ge 1 ]; then pass "iterscan $(basename "$f" .out): $n memory lines"; else fail "iterscan $(basename "$f" .out): no memory lines"; fi
     done
   ```
2. Add a new test function directly after `test_iterscan_cases`:
   ```bash
   # The memory trace is opt-in: without SET pgrust.hnsw_memory, pgrust prints
   # no memory lines, so the comparison must fail.
   test_iterscan_memory_off() {
     local w="$PGV_WORK/iterscan-nomem"
     if PGV_WORK="$w" PGV_ITERSCAN_MEMORY= "$here/../run-iterscan.sh" pgrust relaxed_64k >"$w.log" 2>&1; then
       fail "iterscan: pgrust without pgrust.hnsw_memory differs"
     elif grep -q $'^relaxed_64k\tFAIL$' "$w/iterscan/pgrust/summary.tsv" 2>/dev/null; then
       pass "iterscan: pgrust without pgrust.hnsw_memory differs"
     else
       fail "iterscan: pgrust without pgrust.hnsw_memory: wrong failure (see $w.log)"
     fi
   }
   ```

- [ ] **Step 9: Run the tiers**

```bash
scripts/pgvector/run-iterscan.sh ref
scripts/pgvector/run-iterscan.sh pgrust
scripts/pgvector/run-bytecmp.sh ref big_golomb
scripts/pgvector/run-bytecmp.sh pgrust big_golomb
scripts/pgvector/tests/harness_test.sh test_iterscan_ref test_iterscan_cases test_iterscan_memory_off
```
Expected:
- iterscan: `13/13 identical` in both modes;
- bytecmp: `1/1 identical` in both modes;
- harness: `all passed`.

**If a pgrust iterscan case fails**, open `~/.cache/pgrust/pgvector-work/iterscan/pgrust/diff/<case>.txt` and find the first differing `memory:` line. Only after Task 1 is that stream visible, and finding exactly this is the prerequisite's purpose:
- **Equal `tuples:` but different KB** means a `tmpCtx` charge differs in size or order before that batch.
  1. Record C's event stream for that query with the trace patch. Follow `crates/_support/types/types_hnsw/tests/data/README.md`: apply `hnswscan-tmpctx-trace.patch` in a copy of the reference tree outside the repo, build with `PG_CFLAGS=-DHNSW_MEMORY make OPTFLAGS=""`, and run with `HNSW_SHADOW_LOG`.
  2. Compare it with the charges `search_layer_disk` (`utils.rs`) and `scan.rs` make.
  3. Fix the charge point. Add a unit test that feeds the corrected sequence to one `AllocSetModel` and the expected `palloc`/`pfree` calls to another, and compares `format!("{:?}")` of the two. Then rerun.
- **Different `tuples:`** means the search itself differs, not the memory model: a graph or visit-order difference. Stop and report it with the diff. Do not loosen the tier.

- [ ] **Step 10: Regression check, lints, commit**

```bash
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/_support/types/types_hnsw/src/lib.rs crates/contrib/pgvector_hnsw Cargo.lock \
  scripts/pgvector/run-iterscan.sh scripts/pgvector/run-bytecmp.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector_hnsw: opt-in per-batch scan memory trace; larger iterscan tier

SET pgrust.hnsw_memory = on makes HNSW scans print the line pgvector's
-DHNSW_MEMORY build prints after each search batch (memory KB, tuples).
run-iterscan.sh now compares those lines against the seeded C build in
every case, adds hnsw.ef_search and case arguments, and gains a
10,000-row table whose index run-bytecmp.sh's new big_golomb case
proves byte-identical (M1 report: M3 prerequisite 1).

Co-Authored-By: <the implementing model>
EOF
```
Expected: regress and TAP all `ok`; both lints PASS.

---

### Task 2: HNSW error locations and the durable `hnsw_errors` row

**Files:**
- Create: `scripts/pgvector/sql/hnsw-errors.sql`
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs` (`init_build_state`)
- Modify: `scripts/pgvector/sqldiff.sh` (drop the seeded build's `memory:` INFO lines)
- Modify: `scripts/pgvector/run-diff.sh` (`hnsw_errors` row)
- Modify: `scripts/pgvector/tests/harness_test.sh` (`test_diff_ref`: 17 → 18)

**Interfaces:**
- Produces:
  - `scripts/pgvector/sql/hnsw-errors.sql`. Task 5 appends the `halfvec`, `bit` and `sparsevec` blocks.
  - The diff tier's 18th row, `hnsw_errors`.

- [ ] **Step 1: Write `scripts/pgvector/sql/hnsw-errors.sql` (the vector block)**

```sql
-- HNSW error sites (run-diff.sh's hnsw_errors row): one statement per
-- SQL-reachable ereport of InitBuildState (hnswbuild.c), HnswCheckDim and
-- SparsevecCheckValue (hnswutils.c), for every type. sqldiff.sh --verbose
-- compares each LOCATION. The scans also pin M1 Review Focus 2 and 3: a
-- wrong-dimension query errors even on an empty index, and a NULL query
-- value still scans. HnswCheckDim's build and insert calls cannot be reached
-- from SQL (a column's values always have its typmod's dimensions); unit
-- tests pin them.
SET enable_seqscan = off;
-- vector
CREATE TABLE v_nodim (val vector);
CREATE INDEX ON v_nodim USING hnsw (val vector_l2_ops);
CREATE TABLE v_wide (val vector(2001));
CREATE INDEX ON v_wide USING hnsw (val vector_l2_ops);
CREATE TABLE v (val vector(3));
CREATE INDEX ON v USING hnsw (val vector_l2_ops) WITH (m = 40, ef_construction = 64);
CREATE INDEX ON v USING hnsw (val vector_l2_ops);
CREATE INDEX ON v USING hnsw (val vector_cosine_ops);
SELECT * FROM v ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT * FROM v ORDER BY val <=> '[1,2]' LIMIT 1;
INSERT INTO v VALUES ('[1,2,3]'), ('[4,5,6]');
SELECT * FROM v ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM v ORDER BY val <-> NULL LIMIT 5) s;
SELECT * FROM v ORDER BY val <-> '[1,2,3]' LIMIT 1;
```

- [ ] **Step 2: Make `sqldiff.sh` ignore the seeded build's memory lines**

`sqldiff.sh ref` compares stock C with the seeded build, and the seeded build (`-DHNSW_MEMORY`) prints `INFO:  memory: ...` for every HNSW build and scan. In `scripts/pgvector/sqldiff.sh`:
- Add this function after the `usage` function:
  ```bash
  # The seeded build (-DHNSW_MEMORY) also prints "memory:" INFO lines for HNSW
  # builds and scans (hnswbuild.c:307, hnswscan.c:128), which stock C and
  # pgrust (without SET pgrust.hnsw_memory) do not. Drop them, and the
  # LOCATION line --verbose adds, before comparing.
  strip_hnsw_memory() {
    awk '/INFO:  (00000: )?memory: / { skip = 1; next }
      skip && /^LOCATION:  / { skip = 0; next }
      { skip = 0; print }' "$1" >"$1.tmp"
    mv "$1.tmp" "$1"
  }
  ```
- Add `strip_hnsw_memory "$out/$side.out"` on its own line directly after the line that runs `input.sql` (`(cd "$out" && pgv_psql ...) >"$out/$side.out" 2>&1 || true`).

- [ ] **Step 3: See it fail on pgrust**

```bash
scripts/pgvector/sqldiff.sh ref --verbose scripts/pgvector/sql/hnsw-errors.sql
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/hnsw-errors.sql
grep -c '^[-+]LOCATION:' ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/diff.txt
```
Expected:
- `ref`: `identical`. Without Step 2 it would differ only in the seeded `memory:` lines.
- `pgrust`: `DIFFERENT`. The only differing lines are three LOCATION pairs: C's `InitBuildState, hnswbuild.c:709`, `:714` and `:719` against pgrust's `pgvector_hnsw_build.c:0`. The `grep -c` prints `6`.

- [ ] **Step 4: Give `InitBuildState`'s errors C's locations**

In `crates/contrib/pgvector_hnsw_build/src/lib.rs` `init_build_state`, add a location to each of the three errors, directly after its `.with_sqlstate(...)` line:
- `column does not have dimensions`: `.with_location("hnswbuild.c", 709, "InitBuildState")`
- `column cannot have more than {max_dims} dimensions for hnsw index`: `.with_location("hnswbuild.c", 714, "InitBuildState")`
- `ef_construction must be greater than or equal to 2 * m`: `.with_location("hnswbuild.c", 719, "InitBuildState")`

- [ ] **Step 5: See it pass**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/hnsw-errors.sql
```
Expected: `identical`.

- [ ] **Step 6: Add the `hnsw_errors` row to the diff tier**

In `scripts/pgvector/run-diff.sh`, directly after the `printf 'error_locations\t%s\n' ...` line, add:
```bash
# HNSW's error sites, every type (M1 Review Focus 2: a wrong-dimension query
# errors even on an empty index).
if "$here/sqldiff.sh" "$mode" --verbose "$here/sql/hnsw-errors.sql" >"$out/hnsw-errors.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'hnsw_errors\t%s\n' "$r" | tee -a "$out/summary.tsv"
```
In `scripts/pgvector/tests/harness_test.sh` `test_diff_ref`, change both `17`s in the count check to `18`.

- [ ] **Step 7: Run the tier**

```bash
scripts/pgvector/run-diff.sh pgrust
scripts/pgvector/tests/harness_test.sh test_sqldiff_ref test_sqldiff_detects_difference test_diff_ref
```
Expected: `diff (pgrust, subject pgrust): 18/18 clean`; harness `all passed`.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add scripts/pgvector/sql/hnsw-errors.sql scripts/pgvector/sqldiff.sh scripts/pgvector/run-diff.sh \
  scripts/pgvector/tests/harness_test.sh crates/contrib/pgvector_hnsw_build/src/lib.rs
git commit -q -F - <<'EOF'
pgvector_hnsw_build: InitBuildState error locations; hnsw_errors diff row

The build's dimension and ef_construction errors now report C's
InitBuildState locations. scripts/pgvector/sql/hnsw-errors.sql pins every
SQL-reachable HNSW error site against C, LOCATION included, as the diff
tier's hnsw_errors row (M1 report: M3 prerequisite 2). sqldiff.sh drops
the seeded build's -DHNSW_MEMORY lines.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 3: The sparse L2 merge fused per target, and slice-once half kernels

**Files:**
- Modify: `crates/contrib/pgvector/src/sparsevec.rs` (`inner_product_step` → `fmadd_step`; `sparsevec_l2_squared_distance`; the L1 comment; a test)
- Modify: `crates/contrib/pgvector/src/halfutils.rs` (`hx` → `half_floats`; the four kernels; a test)
- Modify: `crates/contrib/pgvector/src/lib.rs` (the header's fusion line)

**Interfaces:**
- Kernel signatures are unchanged. HNSW calls these kernels through fmgr (support proc 1).

- [ ] **Step 1: Write the failing per-target L2 test**

In `crates/contrib/pgvector/src/sparsevec.rs`'s `mod tests`, add after `inner_product_matches_c_per_arch`:
```rust
    // sparsevec_l2_squared_distance against C, bit for bit. Each case gives
    // the arm64 C result and the amd64 C result:
    // - arm64: the macOS clang reference and the Docker image's gcc arm64
    //   build both round each difference, then fuse every step (fsub, fmadd);
    // - amd64: the pgvector/pgvector:0.8.7-pg18 amd64 image, unfused.
    // Every case separates the two, and none has more than 3 b-only entries
    // after a's last index; longer tails are vectorized (see fmadd_step).
    // Measured while writing the M3 plan.
    #[test]
    fn l2_squared_distance_matches_c_per_arch() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let cases = [
            (
                "{3:0.68405217,5:0.5466399,6:-0.8204614}/6",
                "{1:-0.004012036,2:0.61685055,5:-0.66298896,6:0.11534604}/6",
                "3.1873855590820312",
                "3.187385320663452",
            ),
            (
                "{1:0.20962888,3:-0.25877634,6:-0.1003009}/6",
                "{1:0.58375126,4:-0.6339017,5:0.04112337}/6",
                "0.6205154657363892",
                "0.6205155253410339",
            ),
            (
                "{3:-0.87061185,4:-0.5315948}/6",
                "{2:0.12337011,3:0.3771314,4:-0.29287863,5:-0.34002006,6:0.6459378}/6",
                "2.1619181632995605",
                "2.1619179248809814",
            ),
            (
                "{1:-0.9719157,3:-0.669007,4:-0.066198595,6:0.93480444}/6",
                "{1:0.47442326,2:-0.7231695,5:0.85055166,6:0.551655}/6",
                "3.9370648860931396",
                "3.9370646476745605",
            ),
            (
                "{2:0.72718155,3:-0.25877634,4:0.015045135}/6",
                "{1:0.2447342,2:-0.8365095,4:0.8104313,6:-0.6178536}/6",
                "3.586372137069702",
                "3.586371898651123",
            ),
        ];
        let bits = |t: &str| t.parse::<f64>().unwrap().to_bits();
        for (sa, sb, arm64, amd64) in cases {
            assert_ne!(bits(arm64), bits(amd64), "{sa} - {sb}: case does not separate fused from unfused");
            let want = if cfg!(target_arch = "aarch64") { arm64 } else { amd64 };
            let (ia, ib) = (sparsevec_in_body(m, sa.as_bytes(), -1).unwrap(), sparsevec_in_body(m, sb.as_bytes(), -1).unwrap());
            let a = SparseView::from_payload(&ia[4..]).unwrap();
            let b = SparseView::from_payload(&ib[4..]).unwrap();
            let got = sparsevec_l2_squared_distance(&a, &b) as f64;
            assert_eq!(got.to_bits(), bits(want), "{sa} - {sb}: got {got}, C {want}");
        }
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `UT l2_squared_distance_matches_c_per_arch`
Expected: on this arm64 host, FAIL on the first case with `got 3.187385320663452, C 3.1873855590820312`.

- [ ] **Step 3: Fuse the L2 merge**

In `sparsevec.rs`, replace the `inner_product_step` function and its whole comment block (from `// \`distance += x * y\` of SparsevecInnerProduct, as C's code generation does it.` through the function's closing `}`) with:
```rust
// `distance += x * y` in SparsevecInnerProduct and SparsevecL2SquaredDistance,
// as C's code generation does it.
// DIVERGENCE: C defines these sums by its code generation, not its source:
// upstream builds with -ffp-contract=fast -fassociative-math (Makefile:38).
// - The merge loops: on aarch64, where FMA is baseline, both the macOS clang
//   reference and the Docker image's gcc build emit a scalar fmadd for every
//   step (the L2 difference is rounded first: fsub, then fmadd), so pgrust
//   fuses there too. Elsewhere pgrust keeps the unfused source form. That
//   matches an x86-64 C build without FMA (OPTFLAGS="", as the amd64
//   pgvector Docker image is built); one built with -march=native differs in
//   the last bit, and in the sign of an all-underflow zero.
// - The L2 tail (b-only entries after a's last index): both compilers
//   vectorize it once it has 4 or more entries, differently. clang sums 16-
//   and 4-element blocks in lanes, the first lane seeded with the running
//   distance; gcc sums 4-element blocks in one lane vector and adds it to the
//   running distance at the end. pgrust keeps scalar source order (fused on
//   aarch64), so it equals both C builds whenever at most 3 b-only entries
//   trail, and can differ from either in the last bit otherwise.
#[inline]
fn fmadd_step(distance: f32, x: f32, y: f32) -> f32 {
    #[cfg(target_arch = "aarch64")]
    {
        x.mul_add(y, distance)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        distance + x * y
    }
}
```
In `sparsevec_inner_product`, change `distance = inner_product_step(distance, a.value(i), b.value(j));` to `distance = fmadd_step(distance, a.value(i), b.value(j));`.

Replace `sparsevec_l2_squared_distance` (its comment and body) with:
```rust
// SparsevecL2SquaredDistance (sparsevec.c:823-866): one merge over the two
// sorted index lists. Each `distance += x * x` is fmadd_step(distance, x, x).
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
                distance = fmadd_step(distance, diff, diff);
            } else if ai > bi {
                distance = fmadd_step(distance, b.value(j), b.value(j));
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
            distance = fmadd_step(distance, a.value(i), a.value(i));
        }
    }
    for j in bpos..b.nnz() {
        distance = fmadd_step(distance, b.value(j), b.value(j));
    }
    distance
}
```
Replace the two comment lines above `sparsevec_l1_distance` with:
```rust
// sparsevec_l1_distance's merge (sparsevec.c:1021-1054), factored out like
// the L2 and inner-product kernels. It has no multiply to fuse: both C builds
// emit fabd/fabs and fadd in source order, as here. A tail of 4 or more
// b-only entries is vectorized, as the L2 tail is (see fmadd_step).
```
In `crates/contrib/pgvector/src/lib.rs`'s header, replace the last two lines:
```rust
//! DIVERGENCE: the sparsevec inner product fuses multiply-adds only on
//! aarch64, matching C's codegen per target (see sparsevec.rs).
```
with:
```rust
//! DIVERGENCE: the sparsevec inner product and L2 merges fuse multiply-adds
//! only on aarch64, matching C's codegen per target; neither emulates C's
//! vectorized L2/L1 tails (see sparsevec.rs fmadd_step).
```

- [ ] **Step 4: Run the sparsevec tests**

Run: `UT sparsevec::`
Expected: 13 passed. `sparse_kernels_match_dense_expansion` still passes, because its integer data keeps every sum exact under fusion too.

- [ ] **Step 5: Write the half-kernel reference test**

In `crates/contrib/pgvector/src/halfutils.rs`'s `mod tests`, add:
```rust
    struct XorShift(u64);

    impl XorShift {
        // A random finite half (the kernels never see NaN or infinity:
        // CheckElement rejects them on input).
        fn half(&mut self) -> Half {
            loop {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                let h = self.0 as u16;
                if !half_is_nan(h) && !half_is_inf(h) {
                    return h;
                }
            }
        }
    }

    // The slice-once kernels against the per-element form they replace, bit
    // for bit, over random finite halves of every magnitude.
    #[test]
    fn kernels_match_per_element_reference() {
        let hx = |x: &[u8], i: usize| half_to_float4(Half::from_ne_bytes([x[2 * i], x[2 * i + 1]]));
        let mut r = XorShift(0x9E37_79B9_7F4A_7C15);
        for n in 0..2000usize {
            let dim = 1 + n % 40;
            let a: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
            let b: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
            let (mut l2, mut ip, mut l1) = (0.0f32, 0.0f32, 0.0f32);
            let (mut sim, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..dim {
                let (x, y) = (hx(&a, i), hx(&b, i));
                let diff = x - y;
                l2 += diff * diff;
                ip += x * y;
                l1 += (x - y).abs();
                sim += x * y;
                na += x * x;
                nb += y * y;
            }
            let cos = sim as f64 / ((na as f64) * (nb as f64)).sqrt();
            assert_eq!(halfvec_l2_squared_distance(dim, &a, &b).to_bits(), l2.to_bits());
            assert_eq!(halfvec_inner_product(dim, &a, &b).to_bits(), ip.to_bits());
            assert_eq!(halfvec_l1_distance(dim, &a, &b).to_bits(), l1.to_bits());
            assert_eq!(halfvec_cosine_similarity(dim, &a, &b).to_bits(), cos.to_bits());
        }
    }
```
Run: `UT kernels_match_per_element_reference`
Expected: PASS (it pins today's results before the refactor).

- [ ] **Step 6: Slice once per kernel**

In `halfutils.rs`, replace the `hx` function and its comment with:
```rust
// The `dim` halves of a halfvec's native-endian array, as floats. Slicing
// the array once bounds-checks it once per kernel, not once per element.
#[inline]
fn half_floats(x: &[u8], dim: usize) -> impl Iterator<Item = f32> + '_ {
    x[..2 * dim].chunks_exact(2).map(|h| half_to_float4(Half::from_ne_bytes([h[0], h[1]])))
}
```
Replace the four kernel bodies (keep each comment line as it is):
```rust
pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        let diff = a - b;
        distance += diff * diff;
    }
    distance
}
```
```rust
pub fn halfvec_inner_product(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        distance += a * b;
    }
    distance
}
```
```rust
pub fn halfvec_cosine_similarity(dim: usize, ax: &[u8], bx: &[u8]) -> f64 {
    let mut similarity = 0.0f32;
    let mut norma = 0.0f32;
    let mut normb = 0.0f32;
    for (axi, bxi) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        similarity += axi * bxi;
        norma += axi * axi;
        normb += bxi * bxi;
    }
    // Use sqrt(a * b) over sqrt(a) * sqrt(b)
    similarity as f64 / ((norma as f64) * (normb as f64)).sqrt()
}
```
```rust
pub fn halfvec_l1_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        distance += (a - b).abs();
    }
    distance
}
```

- [ ] **Step 7: Run the tests, the C comparison, lints, and commit**

```bash
UT halfutils:: sparsevec:: halfvec::
cargo test --profile fast-profile --locked -p main_main -p pgvector_f16_parity --lib -- f16_
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust halfvec sparsevec
scripts/pgvector/run-diff.sh pgrust
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/contrib/pgvector/src/sparsevec.rs crates/contrib/pgvector/src/halfutils.rs crates/contrib/pgvector/src/lib.rs
git commit -q -F - <<'EOF'
pgvector: fuse the sparse L2 merge per target; slice-once half kernels

On aarch64 both pgvector C builds (clang and gcc) round each difference
and fuse every scalar step of the sparsevec L2 merge, as the inner
product already did; pgrust now matches them bit for bit (measured
cases, amd64 unfused). The vectorized tails of 4+ b-only entries stay a
recorded divergence: the two compilers disagree there. The halfvec
kernels bounds-check their arrays once per call instead of per element.

Co-Authored-By: <the implementing model>
EOF
```
Expected:
- unit tests: halfutils 4, sparsevec 13, halfvec 7, all passed; parity 3 passed;
- regress `halfvec` and `sparsevec`: `ok`;
- diff: 18/18 clean. Its ruled counts may change slightly, since the deck's sparse L2 statements now fuse; any new finding is a stop-and-report.

---

### Task 4: `HnswTypeInfo` and type dispatch in build, insert and scan

**Files:**
- Modify: `crates/_support/types/types_hnsw/src/lib.rs` (`HNSW_MAX_NNZ`, `HnswTypeInfo`, `HnswScanOpaqueData`)
- Modify: `crates/contrib/pgvector_hnsw/Cargo.toml` (+`adt_varbit`, +`syscache_seams`), `Cargo.lock`
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs`, `insert.rs`, `scan.rs`
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs`

**Interfaces:**
- Produces, in `types_hnsw`:
  - `pub const HNSW_MAX_NNZ: i32 = 1000;`
  - `pub struct HnswTypeInfo { pub max_dimensions: i32, pub dimensions: PGFunction, pub normalize: Option<PGFunction>, pub check_value: Option<fn(&[u8]) -> PgResult<()>> }`
  - `HnswScanOpaqueData::type_info: &'static HnswTypeInfo`, replacing `max_dimensions` and `norm_is_l2`.
- Produces, in `pgvector_hnsw::utils`:
  - `pub static VECTOR_TYPE_INFO`, `HALFVEC_TYPE_INFO`, `BIT_TYPE_INFO` and `SPARSEVEC_TYPE_INFO: HnswTypeInfo`;
  - `pub fn hnsw_type_info_for_symbol(symbol: &str) -> Option<&'static HnswTypeInfo>`;
  - `pub fn hnsw_get_type_info(index: &Relation<'_>) -> PgResult<&'static HnswTypeInfo>`;
  - `pub fn hnsw_check_dim(expected: i32, type_info: &HnswTypeInfo, collation: Oid, value: Datum) -> PgResult<()>`, a new signature;
  - `pub fn hnsw_norm_value<'m>(type_info: &HnswTypeInfo, collation: Oid, mcx: Mcx<'m>, img: &[u8]) -> PgResult<PgVec<'m, u8>>`.
- Removes `utils::check_type_supported` and `scan::norm_value`.
- Changes the signatures of `insert::form_index_value(mcx, value, type_info, support)` and `insert::insert_tuple_on_disk(index, type_info, support, value, heaptid, building)`.

- [ ] **Step 1: Add `HnswTypeInfo` to `types_hnsw`**

In `crates/_support/types/types_hnsw/src/lib.rs`:
- Add `use types_error::PgResult;` and `use types_fmgr::PGFunction;` to the imports.
- Add after `pub const HNSW_MAX_DIM: usize = 2000;`:
  ```rust
  pub const HNSW_MAX_NNZ: i32 = 1000;
  ```
- Add after the `HnswSupport` struct:
  ```rust
  // HnswTypeInfo (hnsw.h:291-297): what an opclass's support proc 3 returns.
  // `normalize` is None for bit (C's NULL); `check_value` is
  // SparsevecCheckValue for sparsevec and takes the detoasted varlena image
  // (4-byte header).
  pub struct HnswTypeInfo {
      pub max_dimensions: i32,
      pub dimensions: PGFunction,
      pub normalize: Option<PGFunction>,
      pub check_value: Option<fn(&[u8]) -> PgResult<()>>,
  }
  ```
- In `HnswScanOpaqueData`, replace the two lines
  ```rust
      pub max_dimensions: i32,
      pub norm_is_l2: bool,
  ```
  with
  ```rust
      // so->typeInfo (hnswscan.c:145).
      pub type_info: &'static HnswTypeInfo,
  ```

- [ ] **Step 2: Write the failing unit tests**

In `crates/contrib/pgvector_hnsw/src/utils.rs`'s `mod tests`, replace `hnsw_check_dim_matches_c` with these tests:
```rust
    use pgvector::halfutils::{float4_to_half_unchecked, half_to_float4};
    use pgvector::halfvec::{HalfBuilder, HalfView};
    use pgvector::sparsevec::{SparseBuilder, SparseView};
    use pgvector::vec::{VecBuilder, VecView};
    use types_core::InvalidOid;

    // A bit(len) varlena image: 4-byte header, i32 bit length, the bytes.
    fn bit_image(bits: &[u8], len: i32) -> Vec<u8> {
        let mut img = vec![0u8; 8 + bits.len()];
        img[0..4].copy_from_slice(&((img.len() as u32) << 2).to_ne_bytes());
        img[4..8].copy_from_slice(&len.to_ne_bytes());
        img[8..].copy_from_slice(bits);
        img
    }

    fn sparse<'m>(m: Mcx<'m>, dim: i32, idx: &[i32], vals: &[f32]) -> PgVec<'m, u8> {
        let mut b = SparseBuilder::new(m, dim, idx.len()).unwrap();
        for (i, (x, v)) in idx.iter().zip(vals).enumerate() {
            b.set_index(i, *x);
            b.set_value(i, *v);
        }
        b.image()
    }

    // HnswGetTypeInfo's tables (hnswutils.c:1398-1458): max dimensions, and
    // which types normalize and check values. Only pgvector's three support
    // symbols resolve (Review Focus 4).
    #[test]
    fn type_info_resolves_only_pgvector_symbols() {
        assert_eq!(VECTOR_TYPE_INFO.max_dimensions, 2000);
        assert!(VECTOR_TYPE_INFO.normalize.is_some() && VECTOR_TYPE_INFO.check_value.is_none());
        let h = hnsw_type_info_for_symbol("hnsw_halfvec_support").unwrap();
        assert!(core::ptr::eq(h, &HALFVEC_TYPE_INFO));
        assert_eq!(h.max_dimensions, 4000);
        assert!(h.normalize.is_some() && h.check_value.is_none());
        let b = hnsw_type_info_for_symbol("hnsw_bit_support").unwrap();
        assert!(core::ptr::eq(b, &BIT_TYPE_INFO));
        assert_eq!(b.max_dimensions, 64000);
        assert!(b.normalize.is_none() && b.check_value.is_none());
        let s = hnsw_type_info_for_symbol("hnsw_sparsevec_support").unwrap();
        assert!(core::ptr::eq(s, &SPARSEVEC_TYPE_INFO));
        assert_eq!(s.max_dimensions, 1_000_000_000);
        assert!(s.normalize.is_some() && s.check_value.is_some());
        for other in ["hnswhandler", "ivfflat_halfvec_support", "textin", ""] {
            assert!(hnsw_type_info_for_symbol(other).is_none(), "{other}");
        }
    }

    // HnswCheckDim (hnswutils.c:1365-1374) through each type's dimensions
    // function: ERRCODE_DATA_EXCEPTION and C's text. Its build and insert
    // calls are unreachable from SQL, so these pin them.
    #[test]
    fn hnsw_check_dim_matches_c() {
        let ctx = mcx::MemoryContext::new("hnsw-test");
        let m = ctx.mcx();
        let vimg = VecBuilder::new(m, 2).unwrap().image();
        let himg = HalfBuilder::new(m, 2).unwrap().image();
        let simg = sparse(m, 2, &[1], &[1.0]);
        let bimg = bit_image(&[0b1100_0000], 2);
        let typed: [(&HnswTypeInfo, &[u8]); 4] = [
            (&VECTOR_TYPE_INFO, &vimg[..]),
            (&HALFVEC_TYPE_INFO, &himg[..]),
            (&SPARSEVEC_TYPE_INFO, &simg[..]),
            (&BIT_TYPE_INFO, &bimg[..]),
        ];
        for (info, img) in typed {
            let d = Datum::from_usize(img.as_ptr() as usize);
            assert!(hnsw_check_dim(2, info, InvalidOid, d).is_ok());
            let e = hnsw_check_dim(3, info, InvalidOid, d).unwrap_err();
            assert_eq!(e.message(), "expected 3 dimensions, not 2");
            assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);
        }
    }

    // SparsevecCheckValue (hnswutils.c:1384-1393): at most HNSW_MAX_NNZ
    // non-zero elements (Review Focus 3).
    #[test]
    fn sparsevec_check_value_caps_at_1000() {
        let ctx = mcx::MemoryContext::new("hnsw-test");
        let m = ctx.mcx();
        let of = |nnz: usize| {
            let idx: Vec<i32> = (0..nnz as i32).collect();
            sparse(m, 2000, &idx, &vec![1.0; nnz])
        };
        let check = SPARSEVEC_TYPE_INFO.check_value.unwrap();
        assert!(check(&of(1000)).is_ok());
        let e = check(&of(1001)).unwrap_err();
        assert_eq!(e.message(), "sparsevec cannot have more than 1000 non-zero elements for hnsw index");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED);
    }

    // HnswNormValue (hnswutils.c:164-168) dispatches to each type's
    // l2_normalize. sparsevec drops elements that normalize to zero
    // (sparsevec.c:1130-1154, Review Focus 2); bit has no normalize.
    #[test]
    fn hnsw_norm_value_dispatches_by_type() {
        let ctx = mcx::MemoryContext::new("hnsw-test");
        let m = ctx.mcx();
        let mut v = VecBuilder::new(m, 2).unwrap();
        v.set(0, 3.0);
        v.set(1, 4.0);
        let out = hnsw_norm_value(&VECTOR_TYPE_INFO, InvalidOid, m, &v.image()).unwrap();
        let view = VecView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (0.6, 0.8));

        let mut h = HalfBuilder::new(m, 2).unwrap();
        h.set(0, float4_to_half_unchecked(3.0));
        h.set(1, float4_to_half_unchecked(4.0));
        let out = hnsw_norm_value(&HALFVEC_TYPE_INFO, InvalidOid, m, &h.image()).unwrap();
        let view = HalfView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (float4_to_half_unchecked(0.6), float4_to_half_unchecked(0.8)));
        assert_eq!(half_to_float4(view.x(0)), 0.600_097_66);

        let out = hnsw_norm_value(&SPARSEVEC_TYPE_INFO, InvalidOid, m, &sparse(m, 2, &[0, 1], &[3.0, 4.0])).unwrap();
        let view = SparseView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.nnz(), view.value(0), view.value(1)), (2, 0.6, 0.8));
        // 1e-30 / 3e38 underflows to zero, so normalization drops it.
        let input = sparse(m, 3, &[0, 2], &[3e38, 1e-30]);
        let out = hnsw_norm_value(&SPARSEVEC_TYPE_INFO, InvalidOid, m, &input).unwrap();
        let view = SparseView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.nnz(), view.index(0), view.value(0)), (1, 0, 1.0));
        assert_eq!(out.len(), input.len() - 8);

        let e = hnsw_norm_value(&BIT_TYPE_INFO, InvalidOid, m, &bit_image(&[0x80], 1)).unwrap_err();
        assert_eq!(e.message(), "hnsw type info has no normalize function");
    }
```
Add at the end of `crates/contrib/pgvector_hnsw/src/scan.rs`'s `mod tests`:
```rust
    // GetScanValue's normalization charges: InitVector, InitHalfVector or
    // InitSparseVector of the input's size; on sparsevec_l2_normalize's
    // zero-dropping path, a second InitSparseVector and a pfree of the first
    // (sparsevec.c:1133, 1151).
    #[test]
    fn charge_norm_value_matches_c() {
        let mut got = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        let mut want = got.clone();
        charge_norm_value(&mut got, 20, 20);
        want.palloc(20);
        charge_norm_value(&mut got, 32, 24);
        want.palloc(32);
        want.palloc(24);
        want.pfree(32);
        assert_eq!(format!("{got:?}"), format!("{want:?}"));
    }
```

- [ ] **Step 3: Run them to see them fail**

Run: `UT type_info hnsw_check_dim sparsevec_check_value hnsw_norm_value charge_norm_value`
Expected: compile errors: `cannot find value 'VECTOR_TYPE_INFO'`, `cannot find function 'hnsw_type_info_for_symbol'`, `charge_norm_value`, and others.

- [ ] **Step 4: Implement the type info in `utils.rs`**

1. In `crates/contrib/pgvector_hnsw/Cargo.toml`, add after `pgvector = { path = "../pgvector" }`:
   ```toml
   adt_varbit = { path = "../../backend/utils/adt/varbit" }
   syscache_seams = { path = "../../backend/utils/cache/syscache_seams" }
   ```
   Then run `cargo update --workspace --offline`.
2. In `utils.rs`, change the `types_core` import to `use types_core::{BlockNumber, Buffer, ForkNumber, InvalidOid, Oid};`.
3. Delete `check_type_supported` and its comment, and the old `hnsw_check_dim` and its comment. In their place, insert:
   ```rust
   // HnswGetTypeInfo's default, no proc 3 (hnswutils.c:1403-1412): the
   // vector opclasses.
   pub static VECTOR_TYPE_INFO: HnswTypeInfo = HnswTypeInfo {
       max_dimensions: HNSW_MAX_DIM as i32,
       dimensions: pgvector::funcs::fc_vector_dims,
       normalize: Some(pgvector::funcs::fc_l2_normalize),
       check_value: None,
   };

   // hnsw_halfvec_support (hnswutils.c:1418-1430).
   pub static HALFVEC_TYPE_INFO: HnswTypeInfo = HnswTypeInfo {
       max_dimensions: HNSW_MAX_DIM as i32 * 2,
       dimensions: pgvector::halfvec::fc_halfvec_vector_dims,
       normalize: Some(pgvector::halfvec::fc_halfvec_l2_normalize),
       check_value: None,
   };

   // hnsw_bit_support (hnswutils.c:1432-1444): dimensions is varbit.c's
   // bitlength.
   pub static BIT_TYPE_INFO: HnswTypeInfo = HnswTypeInfo {
       max_dimensions: HNSW_MAX_DIM as i32 * 32,
       dimensions: adt_varbit::fc_bitlength,
       normalize: None,
       check_value: None,
   };

   // hnsw_sparsevec_support (hnswutils.c:1446-1458).
   pub static SPARSEVEC_TYPE_INFO: HnswTypeInfo = HnswTypeInfo {
       max_dimensions: pgvector::sparsevec::SPARSEVEC_MAX_DIM,
       dimensions: pgvector::sparsevec::fc_sparsevec_vector_dims,
       normalize: Some(pgvector::sparsevec::fc_sparsevec_l2_normalize),
       check_value: Some(sparsevec_check_value),
   };

   // SparsevecCheckValue (hnswutils.c:1384-1393).
   fn sparsevec_check_value(img: &[u8]) -> PgResult<()> {
       let v = pgvector::sparsevec::SparseView::from_payload(&img[4..])?;
       if v.nnz() > HNSW_MAX_NNZ as usize {
           return Err(PgError::error(format!(
               "sparsevec cannot have more than {HNSW_MAX_NNZ} non-zero elements for hnsw index"
           ))
           .with_sqlstate(types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED)
           .with_location("hnswutils.c", 1392, "SparsevecCheckValue")
           .into());
       }
       Ok(())
   }

   // The type info a support proc 3 names, by its C symbol. C calls the proc
   // and gets a pointer to its static HnswTypeInfo; pgrust maps the symbol to
   // the same static table, as amapi resolves hnswhandler (spec §5.4), so the
   // hnsw_*_support functions are never called through fmgr.
   pub fn hnsw_type_info_for_symbol(symbol: &str) -> Option<&'static HnswTypeInfo> {
       Some(match symbol {
           "hnsw_halfvec_support" => &HALFVEC_TYPE_INFO,
           "hnsw_bit_support" => &BIT_TYPE_INFO,
           "hnsw_sparsevec_support" => &SPARSEVEC_TYPE_INFO,
           _ => return None,
       })
   }

   // HnswGetTypeInfo (hnswutils.c:1398-1416). The symbol is pg_proc.prosrc,
   // what C's fmgr looks up in the library.
   // DIVERGENCE: C calls any proc 3 and trusts the pointer it returns; pgrust
   // knows only pgvector's three support functions and raises otherwise.
   pub fn hnsw_get_type_info(index: &Relation<'_>) -> PgResult<&'static HnswTypeInfo> {
       let procid = index_getprocid(index, HNSW_TYPE_INFO_PROC);
       if procid == InvalidOid {
           return Ok(&VECTOR_TYPE_INFO);
       }
       let cx = mcx::MemoryContext::new("hnsw type info");
       let prosrc = syscache_seams::lookup_pg_proc_prosrc::call(cx.mcx(), procid)?;
       let symbol = prosrc.as_ref().map(|s| s.as_str()).unwrap_or("");
       hnsw_type_info_for_symbol(symbol).ok_or_else(|| {
           PgError::error(format!(
               "hnsw type support function \"{symbol}\" of index \"{}\" is not supported",
               index.name()
           ))
           .with_sqlstate(types_error::ERRCODE_FEATURE_NOT_SUPPORTED)
           .into()
       })
   }

   // HnswCheckDim (hnswutils.c:1365-1374, pgvector 0.8.7).
   pub fn hnsw_check_dim(
       expected: i32,
       type_info: &HnswTypeInfo,
       collation: Oid,
       value: Datum,
   ) -> PgResult<()> {
       let dim = types_fmgr::fcinfo::direct_function_call1_coll(type_info.dimensions, collation, value)?
           .as_i32();
       if dim != expected {
           return Err(PgError::error(format!("expected {expected} dimensions, not {dim}"))
               .with_sqlstate(types_error::ERRCODE_DATA_EXCEPTION)
               .with_location("hnswutils.c", 1373, "HnswCheckDim")
               .into());
       }
       Ok(())
   }

   // HnswNormValue (hnswutils.c:164-168): the type's normalize function on a
   // detoasted image; the result is a fresh image in `mcx`.
   // DIVERGENCE: C calls a NULL normalize (bit given a norm proc by a
   // hand-written opclass) and crashes; pgrust raises instead.
   pub fn hnsw_norm_value<'m>(
       type_info: &HnswTypeInfo,
       collation: Oid,
       mcx: Mcx<'m>,
       img: &[u8],
   ) -> PgResult<PgVec<'m, u8>> {
       let Some(normalize) = type_info.normalize else {
           return Err(PgError::error("hnsw type info has no normalize function").into());
       };
       let d = types_fmgr::fcinfo::direct_function_call1_coll_in(
           normalize,
           collation,
           mcx,
           Datum::from_usize(img.as_ptr() as usize),
       )?;
       let p = d.as_usize() as *const u8;
       // SAFETY: the normalize functions return a 4-byte-header varlena image
       // allocated in `mcx`.
       let out = unsafe { core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p)) };
       let mut v: PgVec<'m, u8> = mcx::vec_with_capacity_in_infallible(mcx, out.len());
       let _ = mcx::vec_append_bytes(&mut v, out);
       Ok(v)
   }
   ```
   `HNSW_NPROCS` and `index_getprocid` stay as they are.

- [ ] **Step 5: Dispatch in `insert.rs`**

1. Replace `form_index_value` (its comment and body) with:
   ```rust
   // HnswFormIndexValue (hnswutils.c:411-433): detoast once, check the value
   // (SparsevecCheckValue for sparsevec), then, for opclasses with a norm proc,
   // skip zero-norm values (HnswCheckNorm) and normalize (HnswNormValue).
   pub fn form_index_value<'m>(
       mcx: Mcx<'m>,
       value: Datum,
       type_info: &HnswTypeInfo,
       support: &mut HnswSupport,
   ) -> PgResult<Option<PgVec<'m, u8>>> {
       let p = value.as_usize() as *const u8;
       // SAFETY: non-null varlena datum (isnull gated by caller).
       let raw = unsafe { core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p)) };
       let flat = detoast::detoast_attr(mcx, raw)?;
       let mut img: PgVec<'m, u8> = mcx::vec_with_capacity_in_infallible(mcx, flat.len());
       let _ = mcx::vec_append_bytes(&mut img, &flat);

       if let Some(check_value) = type_info.check_value {
           check_value(&img)?;
       }

       if support.normprocinfo.is_some() {
           // HnswCheckNorm.
           let d = Datum::from_usize(img.as_ptr() as usize);
           let norm = {
               let np = support.normprocinfo.as_mut().expect("some");
               types_fmgr::function_call1_coll(np, support.collation, d)?.as_f64()
           };
           if !(norm > 0.0) {
               return Ok(None);
           }
           img = hnsw_norm_value(type_info, support.collation, mcx, &img)?;
       }
       Ok(Some(img))
   }
   ```
2. In `insert_tuple_on_disk`, change the signature to
   ```rust
   pub fn insert_tuple_on_disk(
       index: &Relation<'_>,
       type_info: &HnswTypeInfo,
       support: &mut HnswSupport,
       value: &[u8],
       heaptid: &ItemPointerData,
       building: bool,
   ) -> PgResult<bool> {
   ```
   and its dimension check to
   ```rust
       // Check dimensions match index (hnswinsert.c:716-717).
       crate::utils::hnsw_check_dim(
           meta.dimensions as i32,
           type_info,
           support.collation,
           Datum::from_usize(value.as_ptr() as usize),
       )?;
   ```
3. In `hnswinsert`, replace everything from `check_type_supported(index)?;` through `insert_tuple_on_disk(index, &mut support, &img, heap_tid, false)?;` with:
   ```rust
       // HnswInsertTuple (hnswinsert.c:752-766).
       let type_info = hnsw_get_type_info(index)?;
       let insert_ctx = mcx::MemoryContext::new_bump("Hnsw insert temporary context");
       let imcx = insert_ctx.mcx();

       let mut support = init_support(index)?;
       let Some(img) = form_index_value(imcx, values[0], type_info, &mut support)? else {
           return Ok(false);
       };
       insert_tuple_on_disk(index, type_info, &mut support, &img, heap_tid, false)?;
   ```

- [ ] **Step 6: Dispatch in `scan.rs`**

1. In `hnswbeginscan`, replace `check_type_supported(index)?;` with:
   ```rust
       // hnswscan.c:145.
       let type_info = hnsw_get_type_info(index)?;
   ```
   In the `HnswScanOpaqueData` literal, replace the lines `max_dimensions: HNSW_MAX_DIM as i32,` and `norm_is_l2: false,` with `type_info,`.
2. Replace `get_scan_value` and `norm_value`, with their comments, by:
   ```rust
   // GetScanValue (hnswscan.c:97-119): detoast, then HnswNormValue for
   // opclasses with a norm proc. Scratch goes in a local bump; the returned
   // image is owned so it frees at the next rescan. C normalizes in tmpCtx,
   // so the normalize function's allocations are charged to the model.
   fn get_scan_value(
       type_info: &HnswTypeInfo,
       support: &HnswSupport,
       orderby: &ScanKeyData,
       tmp_ctx: &mut AllocSetModel,
   ) -> PgResult<Option<Vec<u8>>> {
       if orderby.sk_flags & SK_ISNULL != 0 {
           return Ok(None);
       }
       let tmp = mcx::MemoryContext::new_bump("hnsw scan value");
       let tmcx = tmp.mcx();
       let d = orderby.sk_argument;
       let p = d.as_usize() as *const u8;
       // SAFETY: non-null varlena datum.
       let raw = unsafe {
           core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p))
       };
       let flat = detoast::detoast_attr(tmcx, raw)?;

       if support.normprocinfo.is_some() {
           let normed = hnsw_norm_value(type_info, support.collation, tmcx, &flat)?;
           charge_norm_value(tmp_ctx, flat.len(), normed.len());
           return Ok(Some(normed.to_vec()));
       }
       Ok(Some(flat.to_vec()))
   }

   // HnswNormValue's allocations in tmpCtx (hnswscan.c:115): the normalize
   // function pallocs a result the size of its input (InitVector,
   // InitHalfVector, InitSparseVector); sparsevec_l2_normalize, when elements
   // normalize to zero, pallocs a smaller vector and pfrees the first
   // (sparsevec.c:1133, 1151).
   fn charge_norm_value(tmp_ctx: &mut AllocSetModel, in_len: usize, out_len: usize) {
       tmp_ctx.palloc(in_len);
       if out_len != in_len {
           tmp_ctx.palloc(out_len);
           tmp_ctx.pfree(in_len);
       }
   }
   ```
3. In `hnswgettuple`, replace
   ```rust
               let mut support = so.support.clone();
               so.value = get_scan_value(&mut support, &orderby)?;
               so.support = support;
               // HnswNormValue's palloc0(VECTOR_SIZE(dim)) in tmpCtx (hnswscan.c:115).
               if so.support.normprocinfo.is_some() {
                   if let Some(v) = so.value.as_ref() {
                       so.tmp_ctx.palloc(v.len());
                   }
               }
   ```
   with
   ```rust
               so.value = get_scan_value(so.type_info, &so.support, &orderby, &mut so.tmp_ctx)?;
   ```
4. In `get_scan_items`, change the `hnsw_check_dim(` call's arguments to `meta.dimensions as i32, so.type_info, so.support.collation, Datum::from_usize(v.as_ptr() as usize)`.
5. If `PgError` and `PgVec` are no longer used in `scan.rs`, drop them from its imports to keep the build warning-free.

- [ ] **Step 7: Dispatch in the build**

In `crates/contrib/pgvector_hnsw_build/src/lib.rs`:
1. Add `ERRCODE_FEATURE_NOT_SUPPORTED` to the `types_error` import.
2. In `struct BuildState`, add after `dimensions: i32,`:
   ```rust
       type_info: &'static HnswTypeInfo,
   ```
3. In `init_build_state`, replace everything from `let max_dims = check_type_supported(index)?;` through the closing `}` of the `if dimensions > max_dims` block with:
   ```rust
       // InitBuildState (hnswbuild.c:686-745).
       let type_info = hnsw_get_type_info(index)?;
       let m = hnsw_get_m(index);
       let ef_construction = hnsw_get_ef_construction(index);
       let dimensions = index.rd_att.attr(0).atttypmod;

       // Disallow varbit since require fixed dimensions
       if index.rd_att.attr(0).atttypid == types_core::VARBITOID {
           return Err(PgError::error("type not supported for hnsw index")
               .with_sqlstate(ERRCODE_FEATURE_NOT_SUPPORTED)
               .with_location("hnswbuild.c", 703, "InitBuildState")
               .into());
       }
       // Require column to have dimensions to be indexed
       if dimensions < 0 {
           return Err(PgError::error("column does not have dimensions")
               .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
               .with_location("hnswbuild.c", 709, "InitBuildState")
               .into());
       }
       if dimensions > type_info.max_dimensions {
           return Err(PgError::error(format!(
               "column cannot have more than {} dimensions for hnsw index",
               type_info.max_dimensions
           ))
           .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
           .with_location("hnswbuild.c", 714, "InitBuildState")
           .into());
       }
   ```
   The `ef_construction` check (with Task 2's location) stays below it. In the `Ok(BuildState { ... })` literal, add `type_info,` after `dimensions,`.
4. In `insert_tuple`, pass the type info:
   - `form_index_value(tmcx, values[0], bs.type_info, &mut support)?`;
   - `hnsw_check_dim(bs.dimensions, bs.type_info, bs.support.collation, Datum::from_usize(img.as_ptr() as usize))?`;
   - both calls `insert_tuple_on_disk(bs.index, bs.type_info, &mut support, &img, heaptid, true)`.

- [ ] **Step 8: Run the unit tests**

```bash
UT type_info hnsw_check_dim sparsevec_check_value hnsw_norm_value charge_norm_value
UT_KEEP
```
Expected: 5 passed; UT_KEEP all passed, none failed.

- [ ] **Step 9: The vector paths are unchanged end to end**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 019_storage.pl 037_inputs.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl
scripts/pgvector/run-bytecmp.sh pgrust
scripts/pgvector/run-iterscan.sh pgrust
scripts/pgvector/run-diff.sh pgrust
```
Expected:
- regress 2/2 and TAP 14/14 `ok`;
- bytecmp `11/11 identical`;
- iterscan `13/13 identical`. The memory trace confirms the normalization charge moved without changing.
- diff `18/18 clean`. `hnsw_errors` still matches for vector.

- [ ] **Step 10: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/_support/types/types_hnsw/src/lib.rs crates/contrib/pgvector_hnsw crates/contrib/pgvector_hnsw_build Cargo.lock
git commit -q -F - <<'EOF'
pgvector_hnsw: HnswTypeInfo and type dispatch (0.8.7 hnswutils.c)

Support proc 3 now resolves by its C symbol to the static type info of
hnswutils.c (vector default, halfvec, bit, sparsevec), and the build,
insert and scan take dimensions, normalization, the sparsevec nnz check
and the maximum dimensions from it. The build rejects varbit like C. The
scan charges a normalization's allocations to the modeled tmpCtx as C's
normalize functions make them.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 5: The ten opclasses in the extension script

**Files:**
- Modify: `scripts/pgvector/upstream-sql.sh` (`--only-am AM`; `grep -qxF`)
- Modify: `crates/contrib/pgvector/extension/vector--0.8.5.sql` (append)
- Modify: `crates/contrib/pgvector/src/lib.rs` (`lookup` stubs; header)
- Modify: `scripts/pgvector/sql/hnsw-errors.sql` (append the halfvec, bit and sparsevec blocks)
- Modify: `scripts/pgvector/tests/harness_test.sh` (new `test_upstream_sql_only_am`)

**Interfaces:**
- Consumes: Task 4's type info. `hnsw_get_type_info` resolves the appended `hnsw_*_support` functions by `prosrc`.
- Produces: `scripts/pgvector/upstream-sql.sh [--only-am AM] 'SECTION' ...`. M4 can use `--only-am ivfflat` the same way.

- [ ] **Step 1: Write the failing self-test**

In `scripts/pgvector/tests/harness_test.sh`, add before the `# --- runner ---` line:
```bash
# upstream-sql.sh --only-am prints just one access method's blocks: the three
# hnsw_*_support functions and the ten hnsw opclasses of M3, no ivfflat block,
# no COMMENT ON. The default mode still drops them all.
test_upstream_sql_only_am() {
  local s="$here/../upstream-sql.sh" o
  o="$("$s" --only-am hnsw 'access method private functions' 'halfvec opclasses' 'bit opclasses' 'sparsevec opclasses' 2>&1)" || true
  if [ "$(printf '%s\n' "$o" | grep -c 'USING hnsw')" = 10 ] &&
    [ "$(printf '%s\n' "$o" | grep -c '^CREATE FUNCTION hnsw_[a-z]*_support(internal)')" = 3 ] &&
    ! printf '%s\n' "$o" | grep -q 'ivfflat\|COMMENT ON\|USING btree'; then
    pass "upstream-sql --only-am hnsw: 3 functions, 10 opclasses"
  else
    fail "upstream-sql --only-am hnsw: got: $(printf '%s\n' "$o" | head -3)"
  fi
  if [ "$("$s" 'halfvec opclasses' | grep -c 'OPERATOR CLASS')" = 1 ]; then
    pass "upstream-sql default mode keeps only btree halfvec_ops"
  else
    fail "upstream-sql default mode: halfvec opclasses changed"
  fi
}
```
Run: `scripts/pgvector/tests/harness_test.sh test_upstream_sql_only_am`
Expected: `FAIL - upstream-sql --only-am hnsw: got: error: no section '-- --only-am' ...`, then `ok` for the default mode.

- [ ] **Step 2: Give `upstream-sql.sh` the `--only-am` mode**

Replace the whole of `scripts/pgvector/upstream-sql.sh` with:
```bash
#!/usr/bin/env bash
# Print named sections of the vendored upstream extension script
# (crates/pgvector-0.8.7-reference/sql/vector.sql) for the trimmed
# extension/vector--0.8.5.sql. Dropped: COMMENT ON statements (0.8.6+; they
# arrive with the verbatim script in M4, spec §4.6) and every block of an
# access method not yet ported (USING ivfflat/hnsw opclasses and the
# *_support(internal) functions). Blocks are blank-line separated; a section
# runs from its "-- <name>" header to the next header. Output follows the
# upstream file's order.
# With --only-am AM, print instead only that access method's blocks of the
# sections (its `USING AM` opclasses and `AM_*_support(internal)` functions),
# for sections whose other blocks the trimmed script already has.
# usage: upstream-sql.sh [--only-am AM] 'SECTION' ...
#   e.g. upstream-sql.sh 'bit functions' 'bit operators'
#        upstream-sql.sh --only-am hnsw 'access method private functions' 'bit opclasses'
set -euo pipefail
. "$(dirname "$0")/common.sh"
am=""
if [ "${1:-}" = --only-am ]; then
  [ "$#" -ge 2 ] || die "usage: upstream-sql.sh [--only-am AM] 'SECTION' ..."
  am="$2"
  shift 2
fi
[ "$#" -ge 1 ] || die "usage: upstream-sql.sh [--only-am AM] 'SECTION' ..."
want=""
for s in "$@"; do
  grep -qxF -- "-- $s" "$PGV_SRC/sql/vector.sql" || die "no section '-- $s' in $PGV_SRC/sql/vector.sql"
  want="$want|-- $s|"
done
awk -v want="$want" -v am="$am" '
  BEGIN { RS = ""; ORS = "\n\n" }
  /^-- / && !/\n/ { keep = index(want, "|" $0 "|") > 0; if (keep) print; next }
  !keep { next }
  am == "" && (/USING (ivfflat|hnsw)/ || /_support\(internal\)/) { next }
  am != "" && $0 !~ ("USING " am " AS") && $0 !~ ("^CREATE FUNCTION " am "_[a-z]+_support[(]internal[)]") { next }
  {
    n = split($0, line, "\n"); out = ""
    for (i = 1; i <= n; i++) if (line[i] !~ /^COMMENT ON /) out = out (out == "" ? "" : "\n") line[i]
    if (out != "") print out
  }
' "$PGV_SRC/sql/vector.sql"
```
Then:
```bash
scripts/pgvector/tests/harness_test.sh test_upstream_sql_only_am
scripts/pgvector/upstream-sql.sh 'bit functions' 'bit operators' | grep -c 'CREATE'
```
Expected: two `ok` lines and `all passed`; then `4` (default mode unchanged).

- [ ] **Step 3: Append the HNSW blocks and see the lookup test fail**

```bash
{ echo; scripts/pgvector/upstream-sql.sh --only-am hnsw 'access method private functions' 'halfvec opclasses' 'bit opclasses' 'sparsevec opclasses' | sed '$d'; } >>crates/contrib/pgvector/extension/vector--0.8.5.sql
tail -5 crates/contrib/pgvector/extension/vector--0.8.5.sql
UT lookup_cover
```
Expected:
- The script ends with the `sparsevec_l1_ops` opclass, whose last line is `FUNCTION 3 hnsw_sparsevec_support(internal);`.
- `lookup_covers_every_module_pathname_symbol` FAILs: `no lookup entry for ["hnsw_halfvec_support", "hnsw_bit_support", "hnsw_sparsevec_support"]`.

The appended section headers repeat headers already in the script (`-- access method private functions`, `-- halfvec opclasses`, `-- sparsevec opclasses`). That is harmless and temporary: M4 replaces the trimmed script with upstream's verbatim one.

- [ ] **Step 4: Add the `lookup` stubs**

In `crates/contrib/pgvector/src/lib.rs` `lookup`, add after `"hnswhandler" => fc_hnswhandler,`:
```rust
        "hnsw_halfvec_support" | "hnsw_bit_support" | "hnsw_sparsevec_support" => fc_hnsw_type_support,
```
After `fc_hnswhandler`, add:
```rust
// CREATE FUNCTION validation targets only: pgvector_hnsw resolves an
// opclass's support proc 3 by its C symbol to a static HnswTypeInfo
// (hnsw_get_type_info, spec §5.4), never through fmgr. SQL cannot call
// them, because their only argument is `internal`.
fn fc_hnsw_type_support(
    _f: Option<&mut types_fmgr::FmgrInfo>,
    _fcinfo: &mut types_fmgr::FunctionCallInfoBaseData,
) -> types_error::PgResult<datum::Datum> {
    panic!("hnsw_*_support: pgvector_hnsw resolves support proc 3 by symbol, never through fmgr")
}
```
In the crate header, replace `opclasses), one module per C file. The HNSW opclasses for halfvec,` / `sparsevec and bit (M3) and ivfflat (M4) are unported; the shipped` with:
```rust
//! opclasses), one module per C file. The HNSW opclasses for all four types
//! are shipped (M3); ivfflat (M4) is unported, and the shipped
```
Run: `UT lookup_cover`
Expected: 2 passed.

- [ ] **Step 5: Append the per-type error sites**

Append to `scripts/pgvector/sql/hnsw-errors.sql`:
```sql
-- halfvec
CREATE TABLE h_nodim (val halfvec);
CREATE INDEX ON h_nodim USING hnsw (val halfvec_l2_ops);
CREATE TABLE h_wide (val halfvec(4001));
CREATE INDEX ON h_wide USING hnsw (val halfvec_l2_ops);
CREATE TABLE h (val halfvec(3));
CREATE INDEX ON h USING hnsw (val halfvec_l2_ops);
CREATE INDEX ON h USING hnsw (val halfvec_cosine_ops);
SELECT * FROM h ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT * FROM h ORDER BY val <=> '[1,2]' LIMIT 1;
INSERT INTO h VALUES ('[1,2,3]'), ('[0,0,0]');
SELECT * FROM h ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM h ORDER BY val <=> NULL LIMIT 5) s;
-- bit
CREATE TABLE b_var (val varbit(3));
CREATE INDEX ON b_var USING hnsw (val bit_hamming_ops);
CREATE INDEX ON b_var USING hnsw ((val::bit(64001)) bit_hamming_ops);
CREATE TABLE b (val bit(3));
CREATE INDEX ON b USING hnsw (val bit_hamming_ops);
CREATE INDEX ON b USING hnsw (val bit_jaccard_ops);
SELECT * FROM b ORDER BY val <~> B'10' LIMIT 1;
SELECT * FROM b ORDER BY val <%> B'10' LIMIT 1;
INSERT INTO b VALUES (B'101');
SELECT * FROM b ORDER BY val <~> B'10' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM b ORDER BY val <~> NULL LIMIT 5) s;
-- sparsevec: SparsevecCheckValue runs at build and insert, before the norm
-- check of the cosine opclass (Review Focus 3); 1,000 non-zeros index.
CREATE TABLE s_nodim (val sparsevec);
CREATE INDEX ON s_nodim USING hnsw (val sparsevec_l2_ops);
CREATE TABLE s (val sparsevec(1001));
INSERT INTO s VALUES (array_fill(1, ARRAY[1001])::vector::sparsevec);
CREATE INDEX ON s USING hnsw (val sparsevec_l2_ops);
CREATE INDEX ON s USING hnsw (val sparsevec_cosine_ops);
TRUNCATE s;
CREATE INDEX ON s USING hnsw (val sparsevec_l2_ops);
CREATE INDEX ON s USING hnsw (val sparsevec_cosine_ops);
SELECT * FROM s ORDER BY val <-> '{1:1}/2' LIMIT 1;
INSERT INTO s VALUES (array_fill(1, ARRAY[1001])::vector::sparsevec);
INSERT INTO s VALUES ((array_fill(1, ARRAY[1000]) || 0)::vector::sparsevec);
SELECT count(*) FROM s;
SELECT * FROM s ORDER BY val <-> '{1:1}/2' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM s ORDER BY val <-> NULL LIMIT 5) s2;
```

- [ ] **Step 6: Build, compare with C, and run the M3 regression files**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh ref --verbose scripts/pgvector/sql/hnsw-errors.sql
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/hnsw-errors.sql
grep -c 'ERROR:' ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/pgrust.out
scripts/pgvector/run-regress.sh pgrust bit btree cast copy halfvec sparsevec vector_type hnsw_vector hnsw_bit hnsw_halfvec hnsw_sparsevec
```
Expected:
- both `sqldiff` runs print `identical`, and the `grep -c` prints `22`;
- regress: all 11 `ok`.

If a regress file or `sqldiff` differs:
- read `regression.diffs` or `diff.txt`;
- fix the cause in this task's code or in Task 4's dispatch;
- never edit an expected output or the SQL file to match.

A difference in pgrust core (opclass DDL, planner) is a stop-and-report.

- [ ] **Step 7: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add scripts/pgvector/upstream-sql.sh scripts/pgvector/sql/hnsw-errors.sql scripts/pgvector/tests/harness_test.sh \
  crates/contrib/pgvector/extension/vector--0.8.5.sql crates/contrib/pgvector/src/lib.rs
git commit -q -F - <<'EOF'
pgvector: HNSW opclasses for halfvec, sparsevec and bit

Appends upstream's hnsw_*_support functions and the ten HNSW opclasses
(halfvec l2/ip/cosine/l1, bit hamming/jaccard, sparsevec l2/ip/cosine/l1)
through upstream-sql.sh's new --only-am mode; lookup gets never-called
stubs for the support symbols. Regress hnsw_bit, hnsw_halfvec and
hnsw_sparsevec pass, and hnsw-errors.sql now covers every type.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 6: Byte-identical builds and iterative scans for every type, and the TAP gate

**Files:**
- Modify: `scripts/pgvector/run-bytecmp.sh` (10 cases)
- Modify: `scripts/pgvector/run-iterscan.sh` (3 tables, 6 cases)
- Modify: `scripts/pgvector/tests/harness_test.sh` (`test_iterscan_cases`)

**Interfaces:**
- Consumes: Task 5's opclasses, and Task 1's iterscan CASES format and memory trace.
- Produces: bytecmp 21 default cases and iterscan 19 cases, which Task 7's totals use.

- [ ] **Step 1: The bytecmp cases**

In `scripts/pgvector/run-bytecmp.sh`:
1. Append to the end of the `CASES="..."` string:
   ` halfvec_l2_grid halfvec_cosine_grid sparsevec_l2_grid sparsevec_l2_frac sparsevec_cosine_grid insert_sparsevec_cosine wide_sparsevec bit_hamming bit_jaccard bq_expression`
2. After the `GRID=...` line, add:
   ```bash
   # The same grid as typed arrays; _POS has no zero coordinate (cosine needs
   # a non-zero norm). MD5BITS: 64 pseudo-random bits per row.
   GRID3='ARRAY[i % 17, (i * 7) % 23, (i * 13) % 31]'
   GRID3_POS='ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31]'
   MD5BITS="('x' || substr(md5(i::text), 1, 16))::bit(64)"
   ```
3. In `case_sql`, add these arms before `spill)`:
   ```bash
       halfvec_l2_grid | halfvec_cosine_grid)
         # Small integers are exact halves. Cosine: products of normalized
         # halves are exact in f32, so C's fused sums equal pgrust's.
         local ops=halfvec_l2_ops grid="$GRID3"
         if [ "$1" = halfvec_cosine_grid ]; then ops=halfvec_cosine_ops; grid="$GRID3_POS"; fi
         cat <<SQL
   CREATE TABLE $1 (id int, v halfvec(3));
   INSERT INTO $1 SELECT i, $grid::halfvec FROM generate_series(1, 1000) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
   SQL
         ;;
       sparsevec_l2_grid | sparsevec_cosine_grid)
         # 0 to 3 non-zeros per row. Cosine: the normalized values' inner
         # products fuse on arm64 in C and in pgrust alike.
         local ops=sparsevec_l2_ops grid="$GRID3"
         if [ "$1" = sparsevec_cosine_grid ]; then ops=sparsevec_cosine_ops; grid="$GRID3_POS"; fi
         cat <<SQL
   CREATE TABLE $1 (id int, v sparsevec(3));
   INSERT INTO $1 SELECT i, $grid::vector::sparsevec FROM generate_series(1, 1000) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
   SQL
         ;;
       sparsevec_l2_frac)
         # Non-integer values, so the L2 sums round. With at most 3 non-zeros
         # no L2 tail reaches 4 entries, so C and pgrust round every step alike
         # (fused on arm64: sparsevec.rs fmadd_step).
         cat <<SQL
   CREATE TABLE $1 (id int, v sparsevec(3));
   INSERT INTO $1 SELECT i, ARRAY[(i % 17)::real * 0.37::real, ((i * 7) % 23)::real * 0.53::real, ((i * 13) % 31)::real * 0.71::real]::vector::sparsevec FROM generate_series(1, 1000) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_l2_ops);
   SQL
         ;;
       insert_sparsevec_cosine)
         # Inserts after the build normalize on the on-disk path.
         cat <<SQL
   CREATE TABLE $1 (id int, v sparsevec(3));
   INSERT INTO $1 SELECT i, $GRID3_POS::vector::sparsevec FROM generate_series(1, 800) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_cosine_ops);
   INSERT INTO $1 SELECT i, $GRID3_POS::vector::sparsevec FROM generate_series(801, 1000) i;
   SQL
         ;;
       wide_sparsevec)
         # Review Focus 1: 1,000 non-zeros in 1e9 dimensions, 8,016-byte values
         # TOASTed in the heap. Each neighbor tuple goes on the next page, and
         # the 20 inserts after the build append pages. Values are distinct.
         local sv="('{' || (SELECT string_agg((j * 999983)::text || ':' || CASE WHEN j = 1 THEN i ELSE (i * j) % 7 + 1 END, ',') FROM generate_series(1, 1000) j) || '}/1000000000')::sparsevec"
         cat <<SQL
   CREATE TABLE $1 (id int, v sparsevec(1000000000));
   INSERT INTO $1 SELECT i, $sv FROM generate_series(1, 80) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_l2_ops);
   INSERT INTO $1 SELECT i, $sv FROM generate_series(81, 100) i;
   SQL
         ;;
       bit_hamming | bit_jaccard)
         # Hamming distances are small integers: many ties, popped in C's
         # pairing-heap order.
         cat <<SQL
   CREATE TABLE $1 (id int, v bit(64));
   INSERT INTO $1 SELECT i, $MD5BITS FROM generate_series(1, 1000) i;
   CREATE INDEX $1_idx ON $1 USING hnsw (v $1_ops);
   SQL
         ;;
       bq_expression)
         # Review Focus 5: pgvector's binary-quantization pattern. The index
         # attribute is the cast's bit(64), not the vector column.
         cat <<SQL
   CREATE TABLE $1 (id int, v vector(64));
   INSERT INTO $1 SELECT i, (SELECT array_agg(CASE WHEN substr(md5(i::text || ':' || j::text), 1, 1) < '8' THEN 1 ELSE -1 END) FROM generate_series(1, 64) j)::vector(64) FROM generate_series(1, 1000) i;
   CREATE INDEX $1_idx ON $1 USING hnsw ((binary_quantize(v)::bit(64)) bit_hamming_ops);
   SQL
         ;;
   ```
   The heredoc body lines above are indented only for this plan's list. In the script, they start at column 0, like the other cases' `SQL` heredocs.

- [ ] **Step 2: The iterscan cases**

In `scripts/pgvector/run-iterscan.sh`:
1. Append these lines inside the `CASES` string, after the `big_exhaust_4MB` line; the closing `"` moves to the last of them:
   ```
   halfvec_64k:relaxed_order:64kB:1:20000:40:halfvec_l2
   halfvec_cosine_64k:relaxed_order:64kB:1:20000:40:halfvec_cosine
   sparsevec_64k:relaxed_order:64kB:1:20000:40:sparsevec_l2
   sparsevec_cosine_drop_64k:relaxed_order:64kB:1:20000:40:sparsevec_cosine_drop
   bit_hamming_64k:relaxed_order:64kB:1:20000:40:bit_hamming
   bit_jaccard_64k:relaxed_order:64kB:1:20000:40:bit_jaccard
   ```
2. In `setup_sql`, append before the closing `SQL`:
   ```sql
   CREATE TABLE hv (i int4, v halfvec(3));
   INSERT INTO hv SELECT i, ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31] FROM generate_series(0, 999) i;
   CREATE INDEX hv_l2_idx ON hv USING hnsw (v halfvec_l2_ops);
   CREATE INDEX hv_cosine_idx ON hv USING hnsw (v halfvec_cosine_ops);
   CREATE TABLE sv (i int4, v sparsevec(3));
   INSERT INTO sv SELECT i, ARRAY[i % 17, 1 + (i * 7) % 23, (i * 13) % 31]::vector::sparsevec FROM generate_series(0, 999) i;
   CREATE INDEX sv_l2_idx ON sv USING hnsw (v sparsevec_l2_ops);
   CREATE INDEX sv_cosine_idx ON sv USING hnsw (v sparsevec_cosine_ops);
   CREATE TABLE bv (i int4, v bit(64));
   INSERT INTO bv SELECT i, ('x' || substr(md5(i::text), 1, 16))::bit(64) FROM generate_series(0, 999) i;
   CREATE INDEX bv_hamming_idx ON bv USING hnsw (v bit_hamming_ops);
   CREATE INDEX bv_jaccard_idx ON bv USING hnsw (v bit_jaccard_ops);
   ```
   Add to the comment above `setup_sql`:
   ```bash
   # hv, sv, bv: one table per type, each with two indexes (the ORDER BY
   # operator picks one). Their data has ties, which pop in C's pairing-heap
   # order (M1), and every build reseeds with 42.
   ```
3. In `query_sql`, add before `*) die`:
   ```bash
       halfvec_l2) echo "SELECT i FROM hv WHERE i % 100 = 0 ORDER BY v <-> '[1000,1000,1000]' LIMIT 11" ;;
       halfvec_cosine) echo "SELECT i FROM hv WHERE i % 100 = 0 ORDER BY v <=> '[1,2,3]' LIMIT 11" ;;
       sparsevec_l2) echo "SELECT i FROM sv WHERE i % 100 = 0 ORDER BY v <-> '{1:1000,2:1000,3:1000}/3' LIMIT 11" ;;
       # 1e-30 normalizes to zero: sparsevec_l2_normalize's zero-dropping
       # path, charged to tmpCtx as two vectors and a pfree (Review Focus 2).
       sparsevec_cosine_drop) echo "SELECT i FROM sv WHERE i % 100 = 0 ORDER BY v <=> '{1:3e38,3:1e-30}/3' LIMIT 11" ;;
       bit_hamming) echo "SELECT i FROM bv WHERE i % 100 = 0 ORDER BY v <~> ('x' || substr(md5('q'), 1, 16))::bit(64) LIMIT 11" ;;
       bit_jaccard) echo "SELECT i FROM bv WHERE i % 100 = 0 ORDER BY v <%> ('x' || substr(md5('q'), 1, 16))::bit(64) LIMIT 11" ;;
   ```

- [ ] **Step 3: Self-test assertions**

In `scripts/pgvector/tests/harness_test.sh` `test_iterscan_cases`, add before the `# Every case compares a memory trace.` block:
```bash
  # The type cases stop on the 64kB cap (on the seeded build when the M3 plan
  # was written: 139, 132, 139, 223, 663 and 608 of 990 rows removed).
  for x in halfvec_64k halfvec_cosine_64k sparsevec_64k sparsevec_cosine_drop_64k bit_hamming_64k bit_jaccard_64k; do
    a="$(removed "$x")"
    if [ -n "$a" ] && [ "$a" -lt 990 ]; then pass "iterscan $x stops early ($a removed)"; else fail "iterscan $x: removed '$a', want < 990"; fi
  done
```

- [ ] **Step 4: Run both tiers in both modes**

```bash
scripts/pgvector/run-bytecmp.sh ref
scripts/pgvector/run-bytecmp.sh pgrust
scripts/pgvector/run-iterscan.sh ref
scripts/pgvector/run-iterscan.sh pgrust
scripts/pgvector/tests/harness_test.sh test_bytecmp_ref test_bytecmp_cases test_iterscan_ref test_iterscan_cases
```
Expected:
- bytecmp `21/21 identical` in both modes;
- iterscan `19/19 identical` in both modes;
- harness `all passed`. `test_bytecmp_cases` requires every new case to have at least 10 blocks and height at least 1.

If a pgrust case differs:
- **bytecmp:** `diff/<case>.idx.txt` names the first differing blocks.
- **iterscan:** the first differing line of `diff/<case>.txt`. Equal `tuples:` with different KB is a `tmpCtx` charge difference: get C's byte-level event stream with `crates/_support/types/types_hnsw/tests/data/hnswscan-tmpctx-trace.patch` (see that directory's README) and compare. Different `tuples:` is a search difference.

Report it with the evidence. Never move a case to opt-in without the user: the cases were chosen so that C's arithmetic and pgrust's agree.

- [ ] **Step 5: The TAP gate**

```bash
scripts/pgvector/run-tap.sh pgrust 020_hnsw_bit_build_recall.pl 021_hnsw_bit_insert_recall.pl 022_hnsw_bit_vacuum_recall.pl 023_hnsw_bit_duplicates.pl 024_hnsw_halfvec_build_recall.pl 025_hnsw_halfvec_insert_recall.pl 026_hnsw_halfvec_vacuum_recall.pl 027_hnsw_halfvec_duplicates.pl 028_hnsw_sparsevec_build_recall.pl 029_hnsw_sparsevec_insert_recall.pl 030_hnsw_sparsevec_vacuum_recall.pl 031_hnsw_sparsevec_duplicates.pl 038_hnsw_sparsevec_vacuum_insert.pl
for t in 020_hnsw_bit_build_recall 024_hnsw_halfvec_build_recall 028_hnsw_sparsevec_build_recall; do
  echo "$t: $(sed -n 's/^#   Failed test at .* line \([0-9]*\)\.$/\1/p' ~/.cache/pgrust/pgvector-work/tap/pgrust/prove/$t.log | sort -un | tr '\n' ' ')"
done
```
Expected:
- 10 `ok`: 021–023, 025–027, 029–031 and 038.
- 020, 024 and 028 FAIL. Their failed lines are exactly `020_hnsw_bit_build_recall: 114 131`, `024_hnsw_halfvec_build_recall: 109 126` and `028_hnsw_sparsevec_build_recall: 106 123`.
- Each of those lines is a `like($stderr, qr/using \d+ parallel workers/)`, as 012's lines 106 and 123 are: parallel builds, M6. Every serial-build assertion, recall included, passes.

Then confirm the vector HNSW tests are unchanged:
```bash
scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 019_storage.pl 037_inputs.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl
```
Expected: 14/14 `ok`.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/run-bytecmp.sh scripts/pgvector/run-iterscan.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: byte-identical and iterative-scan cases for every type

run-bytecmp.sh gains ten seeded halfvec, sparsevec and bit builds
(grids, cosine, fractional sparse L2, inserts, 1,000-non-zero sparsevecs
across pages, a binary_quantize expression index); run-iterscan.sh
gains one table per type with two opclasses each, including a query
whose normalization drops an element. All are identical to the seeded C
build; the new HNSW TAP tests pass except the parallel-build assertions.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 7: Docs, the full gate, and the M3 report

**Files:**
- Modify: `crates/contrib/pgvector_hnsw/src/lib.rs` (crate header)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, `CLAUDE.md`
- Create: `docs/superpowers/reports/pgvector-m3-report.md`

- [ ] **Step 1: The HNSW crate header**

Replace the `//!` header of `crates/contrib/pgvector_hnsw/src/lib.rs` with:
```rust
//! pgvector 0.8.7 hnsw AM (hnsw.c/hnswutils.c/hnswinsert.c/hnswscan.c/
//! hnswvacuum.c) for vector, halfvec, sparsevec and bit, serial on-disk
//! rendering; the in-memory build phase lives in pgvector_hnsw_build.
//! Support proc 3 resolves by its C symbol to hnswutils.c's static type info
//! (utils.rs hnsw_get_type_info). DIVERGENCES (recorded): no parallel build
//! (C falls back to serial when no workers launch); iterative-scan memory cap
//! is a byte-exact model of C's so->tmpCtx (types_hnsw::tmpctx), charged at
//! C's allocation points; level RNG uses the ported pg_global_prng (same
//! generator, per-backend seeding as stock C; builds reseed only with
//! pgrust.hnsw_build_seed, C's -DHNSW_MEMORY SeedRandom); pgrust.hnsw_memory
//! prints -DHNSW_MEMORY's per-batch scan line (scan.rs); an unknown support
//! proc 3 raises where C would call it.
```

- [ ] **Step 2: Correct the spec**

In `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`:
- **§5.4, the first bullet.** Append to its first line: ` (In place since M3: `hnsw_get_type_info` in `pgvector_hnsw/src/utils.rs` maps the proc's C symbol, `pg_proc.prosrc`, to the table; any other symbol raises 0A000, where C would call it.)`
- **§8.2, Byte-identical index row, Method cell.** Replace `Cases cover tie-free and tie-heavy data, `m = 4`, 2,000 dimensions, an unlogged init fork, and inserts after the build.` with `Cases cover tie-free and tie-heavy data, `m = 4`, 2,000 dimensions, an unlogged init fork, inserts after the build, and a 10,000-row build whose C graph spans several 1 MB blocks. Since M3 they also cover every `halfvec`, `sparsevec` and `bit` opclass family: integer grids, `bit(64)` from md5, a 1,000-non-zero `sparsevec` across pages, a `binary_quantize` expression index, and cosine and fractional `sparsevec` cases whose sums C and pgrust round alike.`
- **§8.2, Iterative-scan row, Method cell.** Append: ` Every case also compares the memory trace: the seeded build prints each search batch's `tmpCtx` size (KB) and tuple count (`ShowMemoryUsage`), and pgrust prints the same line under the opt-in placeholder `pgrust.hnsw_memory`. Since M3 the cases cover all four types, a 10,000-row table and `hnsw.ef_search` variants.`
  Replace its Pass rule cell with: `Same rows in the same order, the same `Rows Removed by Filter`, and the same memory lines`.
- **§8.2, Exact differential row, Method cell.** After `(`scripts/pgvector/sql/error-locations.sql`, `sqldiff.sh --verbose`)`, insert `, and every SQL-reachable HNSW error site for all four types (`scripts/pgvector/sql/hnsw-errors.sql`, M3)`.
- **§8.3.** In the script list, add a bullet `` `sql/hnsw-errors.sql` (M3) `` after the bullet that names `run-diff.sh` (M2).

- [ ] **Step 3: Update `CLAUDE.md`**

In the "Vector search (fork work)" section:
- In the first paragraph, replace `with `halfvec`, `sparsevec` and the `bit` functions added in M2;` with `with `halfvec`, `sparsevec` and the `bit` functions added in M2 and their HNSW opclasses in M3;`.
- Replace the `pgvector_hnsw` bullet with:
  ```markdown
  - `crates/contrib/pgvector_hnsw` and `pgvector_hnsw_build`: the HNSW AM for all four types, using pgvector's page layout and GenericXLog. An opclass's support proc 3 resolves by its C symbol to hnswutils.c's static type info.
  ```
- Replace `The HNSW opclasses for `halfvec`, `sparsevec` and `bit` (M3) and `ivfflat` (M4) are **not ported**, and the extension script is trimmed to match.` with `` `ivfflat` (M4) is **not ported**, and the extension script is trimmed to match.``
- In the harness command block, replace the `upstream-sql.sh` line with:
  ```bash
  scripts/pgvector/upstream-sql.sh [--only-am hnsw] 'SECTION' ...  # upstream extension-script sections for the trimmed script
  ```
- After the paragraph that starts `Stock C pgvector never seeds HNSW builds`, add: `` `SET pgrust.hnsw_memory = on` (also unregistered) makes pgrust scans print the seeded C build's per-batch `memory: N KB, tuples: T` INFO line; `run-iterscan.sh` compares them. ``
- In the baselines sentence, add `` `docs/superpowers/reports/pgvector-m3-report.md` ``.

- [ ] **Step 4: Full run of every tier**

```bash
scripts/pgvector/build-reference.sh --verify
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-all.sh ref
scripts/pgvector/run-all.sh pgrust
scripts/pgvector/tests/harness_test.sh
UT_M3
UT_KEEP
cargo test --profile fast-profile --locked -p main_main -p pgvector_f16_parity --lib -- f16_
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
for t in crates/pgvector-0.8.7-reference crates/postgres-18.6-reference; do git status --porcelain --ignored "$t"; done
```
Expected:
- `run-all.sh ref`: 120/120 (regress 14, TAP 48, bytecmp 21, iterscan 19, diff 18).
- `run-all.sh pgrust`: 96/120:
  - regress 11/14: all but the three `ivfflat_*` files;
  - TAP 27/48: M2's 17 plus the 10 of Task 6;
  - bytecmp 21/21, iterscan 19/19, diff 18/18.
- Every remaining failure is IVFFlat (M4) or parallel builds (M6). Check that against the logs:
  - the regress diffs say `access method "ivfflat" does not exist`;
  - the TAP failures are the 16 IVFFlat tests plus 012, 020, 024, 028 and 045, each failing only on `using \d+ parallel workers` (and 045's `after 0 tuples` message).
- `harness_test.sh`: `all passed`.
- `UT_M3`: 25 passed. `UT_KEEP`: all passed. Parity: 3 passed.
- Both lints PASS, and both reference trees print nothing.

- [ ] **Step 5: Write `docs/superpowers/reports/pgvector-m3-report.md`**

Follow `docs/superpowers/reports/pgvector-m2-report.md`'s structure, filled from the Step 4 runs:
1. **Header line:** the date, the pgrust code commit (Task 6's), and the references (`pgvec`, `pgvec-seeded`).
2. **Summary:**
   - C reference and pgrust totals, broken down by tier;
   - the files and tests M3 fixed;
   - the gates that are also green.
3. **pgrust results:** the full table from `~/.cache/pgrust/pgvector-work/report-pgrust.md`.
4. **Remaining failures:** each with its cause and milestone, checked against the logs.
5. **What M3 established:**
   - type info resolved by symbol for all four types;
   - byte-identical seeded builds for every opclass family, including a multi-block 10,000-row graph (M1's SelectNeighbors tie-break question, verified at that size);
   - per-batch memory traces identical to C in all 19 iterscan cases (M1 prerequisite 1);
   - every SQL-reachable HNSW error site identical, LOCATION included (M1 prerequisite 2);
   - the sparse L2 merge bit-exact per target, with the measured cases.
6. **Known divergences left open:**
   - **The vectorized tails of 4+ b-only entries** in the sparse L2/L1 kernels. Give the clang and gcc codegen facts from this plan's header, and say that C's two builds disagree there.
   - **The same class for the dense `vector` and `halfvec` kernels**, unchanged from M2.
   - **An unknown support proc 3** raises 0A000 where C calls it.
   - **For M5:** its identical-approximate-results gate compares pgrust with the Docker image's gcc build. Its tails differ from both clang's and pgrust's sequential order, so M5 may need to measure how often that changes a traversal, or decide to emulate gcc's tail.
   - Carry forward M2's open items that M3 did not touch: TAP 018's worker pin, the `float.c:0` and `pgvector.c:0` locations, `pg_get_loaded_modules()` and the findings-file block.
7. **Deferred minors:** carry every M2 ledger item M3 did not absorb into this section, so `~/.cache/pgrust/m2-sdd-ledger.md` is no longer needed. M3 absorbed:
   - the `hx` bounds checks;
   - the sparse kernel fusion;
   - `upstream-sql.sh`'s `grep -qxF`.
   Add any minors this milestone's reviews defer.
8. **Coverage limits:**
   - `wasm/wasm-build.sh` was not run (`nightly-2026-07-17` is not installed).
   - **x86:** the per-target L2 test's amd64 values were measured on the amd64 Docker image under emulation. The harness itself ran only on macOS arm64.
   - **Recall-test coverage:** parallel-build assertions (020, 024, 028) wait for M6, while their serial recall assertions pass.
   - The iterscan tier's new type tables are tie-heavy; their exact stop points rest on M1's pairing-heap tie order.

- [ ] **Step 6: Commit**

```bash
git add crates/contrib/pgvector_hnsw/src/lib.rs docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m3-report.md
git commit -q -F - <<'EOF'
docs(pgvector): M3 report; spec, CLAUDE.md and HNSW crate header at M3

Records the M3 conformance run (HNSW type info and opclasses for
halfvec, sparsevec and bit), and brings the spec's tier rows, CLAUDE.md
and the pgvector_hnsw header up to what was built.

Co-Authored-By: <the implementing model>
EOF
git log --oneline main..HEAD
```
Expected: 8 commits on `vector/m3`: the plan plus Tasks 1–7. Do not merge or push; the controller does that after the final review and the user's go-ahead.

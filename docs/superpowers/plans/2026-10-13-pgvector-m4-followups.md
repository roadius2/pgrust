# pgvector M4 follow-ups: C-exact error locations, AM library loading, `bitmap_scan`, the `insert_grid` flake and a pre-M4 repair — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every open item M4 left for the user. Five areas:
- the core follow-ups the user approved on 2026-10-13;
- the M1–M3 dev clusters that cannot `ALTER EXTENSION vector UPDATE`;
- the seeded-C self-comparison flake on `insert_grid`;
- `bitmap_scan` for the two vector AMs;
- vector.c's error locations.

It also proves each fix against C:
- the regression, TAP, bytecmp and iterscan tiers stay green;
- two new diff rows compare the fixed sites with C, LOCATION included.

**Architecture:**
- **Error locations.** Every error that mirrors a C `ereport`/`elog` gets C's `file:line, function` through `.with_location(...)`, the repo's convention. This covers:
  - reloptions `parse_one_reloption` and `parseRelOptionsInternal`;
  - guc `parse_and_validate_value`, the check hooks, `assignable_custom_variable_name` and `MarkGUCPrefixReserved`;
  - mcx's out-of-memory and alloc-size errors;
  - adt_float's overflow, underflow, zero-divide and input errors;
  - all of pgvector's vector.c sites.
- **Library loading.** `amapi::GetIndexAmRoutine` runs `fmgr_info(amhandler)` for the extension handlers, which is the first half of C's `OidFunctionCall0`. That loads the handler's library and runs its `_PG_init` once per session, exactly where C does.
  - A no-load variant serves the two callers where C never calls the handler.
  - amutils' property and phase-name functions load the same way.
- **The repair.** A general, idempotent repair script, generated from upstream `sql/vector.sql` by a test-only Rust generator, brings any pre-M4 trimmed-0.8.5 database to full membership. After that, `ALTER EXTENSION vector UPDATE` works.
  - The script ships outside the verbatim `extension/` directory.
  - It is checked on all three trimmed generations that ever shipped on `main`, and applied to the user's `pgv-016` dev cluster.
- **The flake.** The seeded-C self-comparison flake on `insert_grid` is a property of C, not of pgrust. The case moves to the opt-in known divergences, with a `DIVERGENCE` note in the port.

**Decisions this plan makes (for review):**
1. **`insert_grid` is demoted to opt-in (still runnable by name), not changed and not dropped.**
   - C breaks equal-distance ties in `HnswUpdateConnection` by raw element pointer (`CompareCandidateDistances`, `hnswutils.c:1007-1011`, `base == NULL`).
   - On an on-disk insert, the new element lives in `insertCtx` (an AllocSet, `hnswinsert.c:788`) and the loaded neighbors in `updateCtx` (a Generation context, `hnswinsert.c:551`). Their relative addresses vary from run to run, so C is not reproducible against itself: about 2 in 37 seeded C-vs-C runs differ.
   - No implementation can match that deterministically. Any change that removes those ties also removes what the case tests, which `insert_golomb` already covers tie-free.
   - Kept opt-in, it still lets a human check pgrust against C, and it passes in C's common ordering.
   - `insert_sparsevec_cosine` uses the same tie-heavy grid. Task 1 measures it with 30 C-vs-C runs and demotes it only if one differs.
2. **Library loading follows C's call sites exactly.**
   - It loads in `GetIndexAmRoutine`, which covers relcache's first index open, `DefineIndex`, opclass validation, `amvalidate` and the strategy translators.
   - It also loads in amutils' `pg_indexam_has_property`, `pg_index_has_property`, `pg_index_column_has_property` and `pg_indexam_progress_phasename` for extension AMs.
   - The `CREATE ACCESS METHOD` fence (`amcmds.c:78` never calls the handler) and the per-scan `from_relam` resolver (relcache already loaded) use a no-load variant.
   - lsyscache's name-matched AM arms (`amop.rs`) stay as they are: they run after relcache has opened an index of that AM, and this is recorded.
3. **Error-location scope.** It is the C sites listed per task, every one with a unit test or a diff statement. Three groups are out of scope and recorded:
   - other `unrecognized configuration parameter` constructions that port other C functions;
   - hand-written `out of memory` errors in non-mcx crates, each a different C site;
   - pgvector_hnsw's 13 internal-guard errors, which are not reachable from SQL.
4. **`("pgvector_hnsw", "{hnsw.c")` becomes `("pgvector_hnsw", "hnsw.c")`.** The table's header names a generator that is not in the repo, so it is edited by hand.
5. **The repair is general, not the 11-statement version.**
   - `0.8.5` labels three trimmed scripts that shipped on `main`: pgrust v0.3 (`79ad992ede`, 69 statements), M2 (`88f29c4ccf`, 190) and M3 (`c1c5da11cc`, 203). Each is an exact subset of upstream; verified 2026-10-13, with 0 statements absent from or differing from upstream.
   - Every upstream object gets an existence guard, so one script repairs all three generations. It is a no-op on a repaired or 0.8.7 database.
   - The script ships at `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql`. A generator test keeps it in sync with upstream.
6. **Two new diff rows and one grown row.**
   - `library_load` (Task 3) is new.
   - `core_error_locations` (Tasks 4–5) is new.
   - `error_locations` gains a vector.c section (Task 6).
   - Expected totals: C 130/130, pgrust 122/130 (regress 14, TAP 40/48, bytecmp 28, iterscan 19, diff 21).
7. **mcx's over-ceiling allocation panic carries a structured `PgError`** in `std` builds, which the server uses. The statement boundary already prefers a `PgError` payload (`pg_error_from_panic`), so the error goes out with C's `MemoryContextSizeFailure, mcxt.c:1180`.

**Tech Stack:** Rust (the pgrust workspace, `fast-profile`), bash 3.2 harness scripts, the PostgreSQL 18.6 + pgvector 0.8.7 C reference builds (`pgvec`, `pgvec-seeded`), `pg_regress`, Perl TAP.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (binding: §2 "Compatibility level", §4.6, §8.2 tiers). Inputs:
- `docs/HANDOFF_2026-10-13.md` §2 (the open threads);
- `docs/superpowers/reports/pgvector-m4-report.md` ("Known divergences left open", "Deferred minors");
- `~/.cache/pgrust/m4-sdd-ledger.md` (rulings).

The user's decisions on 2026-10-13:
- push (done);
- fix the old dev clusters;
- go-ahead for the core follow-ups;
- fix the report wording (done);
- kill the stray process (done);
- the `insert_grid` question is answered by Decision 1.

Facts verified while writing this plan (2026-10-13, macOS arm64):
- **Library load primitive.** `fmgr_core::fmgr_info` (`fmgr_core/src/lib.rs:896`) loads a LANGUAGE c function's library through `dfmgr::load_external_function` (`:728-746`), running `pg_init` once per session.
  - Its C-function cache is `thread_local!`, which is per backend, as in C.
  - `fmgr_seams::fmgr_info` is the seam form. `adt_amutils` already depends on `fmgr_seams`, and `amapi` depends on `fmgr_core`.
  - `pgvector`'s and `bloom`'s lookups resolve `hnswhandler`, `ivfflathandler` and `blhandler` to stubs that are never called (`pgvector/src/lib.rs:151,155`; `bloom/src/lib.rs:24`).
- **What loads `vector` first in a C session.**
  - `vector(3)` typmod input (`vector_typmod_in` through fmgr).
  - Any vector function.
  - `CREATE INDEX ... USING hnsw` (`DefineIndex`, indexcmds.c:864).
  - The first open of an hnsw/ivfflat index (relcache.c:1432 → amapi.c:38).
  - `pg_indexam_has_property` on the AM (amutils.c, `GetIndexAmRoutineByAmId`).
- **Error locations.**
  - Every location gap is a missing `.with_location`. `PgError::error`/`ereport(..).into_error()` captures the Rust site, and `source_map` turns it into `<crate>.c:0` (`types_error/src/source_map.rs`).
  - `ErrorLocation::new(file!(), line!(), ..)` also maps to line 0, which is why `MarkGUCPrefixReserved` gives `guc.c:0`.
- **C closing lines,** computed by paren balance over the vendored C:

  | File | Function | Lines |
  |---|---|---|
  | `reloptions.c` | `parse_one_reloption` | 1601, 1617, 1629, 1637, 1649, 1657, 1681 |
  | `reloptions.c` | `parseRelOptionsInternal` | 1488 |
  | `guc.c` | `parse_and_validate_value` | 3145, 3166, 3186, 3207, 3227, 3282 |
  | `guc.c` | `call_{bool,int,real,string,enum}_check_hook` | 6834, 6868, 6902, 6945, 6987 |
  | `guc.c` | `assignable_custom_variable_name` | 1139, 1156, 1169 |
  | `guc.c` | `MarkGUCPrefixReserved` | 5312 |
  | `mcxt.c` | `MemoryContextAllocationFailure` | 1167 |
  | `mcxt.c` | `MemoryContextSizeFailure` | 1180 |
  | `float.c` | `float_overflow_error`, `float_underflow_error`, `float_zero_divide_error` | 90, 98, 106 |
  | `float.c` | `float4in_internal`: empty, invalid, junk, range | 208, 295, 309, 288 |
  | `float.c` | `float8in_internal`: empty, invalid, junk, range | 414, 497, 511, 490 |

  Measured on C: `'abc'::float4` reports `float4in_internal, float.c:295` and `1e39::numeric::float4` reports `float4in_internal, float.c:288`. `numeric_float4` calls `float4in`, so it needs no edit of its own.
- **vector.c has 25 error sites,** and 24 pgrust sites lack a location (table in Task 6). Only `sparsevec_to_vector`'s `index out of bounds` (vector.c:1343) already carries one.
- **`bitmap_scan`.** pgrust returns `t` for hnsw and ivfflat; C returns `f`, because both handlers set `amgetbitmap = NULL` (`hnsw.c:315`, `ivfflat.c:232`). It is the only property mismatch for those AMs; every other amutils property was checked against C.
- **The three trimmed generations** are exact subsets of upstream `vector.sql`. Upstream has 362 statements: 213 CREATE and 149 COMMENT.
  - `vector--0.8.5--0.8.6.sql` is empty.
  - `vector--0.8.6--0.8.7.sql` is 147 COMMENTs. It does not re-apply `COMMENT ON ACCESS METHOD ivfflat`/`hnsw`.
  - pgrust supports `ALTER EXTENSION ... ADD` (`commands/extension/src/contents.rs`), `CREATE ACCESS METHOD`, opclasses `USING ivfflat`, `'$libdir/vector'`, and `CREATE EXTENSION vector VERSION '0.8.5'` once a `vector--0.8.5.sql` is staged.
  - The 11-statement repair for the latest generation matched a fresh install on 480 catalog rows. `vector.sql` has no `$$` bodies.
- **The dev clusters.**
  - `~/.cache/pgrust/pgv-016/data/pgrust` holds database `postgres` at vector 0.8.5, the v0.3 generation, without `halfvec`.
  - `~/.cache/pgrust/pgvector-work/data/pgrust` has `contrib_regression` at 0.8.7 and `postgres` without vector, so it needs nothing.

## Global Constraints

- **Branch:** work on `vector/m4-followups`, cut from `main` (`d4a642a645`). Merge locally with a fast-forward when done; push only when the user asks. Work in the main checkout, because the disk is about 92% full.
- **Commit trailer:** every commit ends with a `Co-Authored-By:` trailer naming the model that wrote it: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md): keep C function names, cite C `file.c:line` in comments, and mark intentional differences `DIVERGENCE`.
- **Error locations:** `.with_location("<file>.c", <line>, "<CFunction>")` on the `PgError`. `<line>` is the last line of the C call, the line holding its closing `);`. The lines in this plan are verified; copy them exactly. Never change a message, SQLSTATE, DETAIL, HINT or elevel to fix a location.
- **Extension SQL:** `crates/contrib/pgvector/extension/` stays upstream's, byte for byte (`shipped_scripts_are_upstream_verbatim`). The repair script lives outside it.
- **Core edits allowed (user go-ahead, 2026-10-13):**
  - `amapi`, `commands_amcmds` (the fence call only) and `adt_amutils`;
  - `guc`, `reloptions`, `mcx` and `adt_float`;
  - `types_error` (the source-map row only).

  Any other core edit a fix seems to need is a plan-defined stop: report it, do not make it.
- **Never widen `docs/fuzzing/rulings.toml`.** Upstream expected outputs are never edited.
- **No new external dependencies.** New path dependencies on workspace crates are fine. After any `Cargo.toml` edit:
  - run `cargo update --workspace --offline`;
  - check that `git diff --stat Cargo.lock` touches only the edited packages;
  - commit `Cargo.lock` with the task.
- **Unit tests:** `cargo test --profile fast-profile --locked -p main_main -p <packages> --lib -- <filters>`.
  - Listing `main_main` keeps feature resolution identical to the server build, so the `fast-profile` artifacts are reused.
  - Never run with an empty filter.
  - Each task names its exact command.
  - `UT_KEEP` (the M4 regression set) must stay green at the end of every task that touches Rust:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw -p types_ivfflat -p pgvector_ivfflat -p pgvector_ivfflat_build -p sampling --lib -- tmpctx:: chain_walk update_connection seed_random level_for_uniform candidate_heaps hnsw_get_max_level parse_build_seed vector_combine bitutils:: bitvec:: halfvec:: funcs:: vec:: parse_show_memory charge_norm_value type_info hnsw_check_dim sparsevec_check_value hnsw_norm_value halfutils:: sparsevec:: lookup_cover fmgr_stubs ivf_ block_sampler reservoir_ shipped_scripts
    ```
- **Server build:** `scripts/pgvector/build-pgrust.sh` builds `target/fast-profile/postgres`. Every conformance check needs it rebuilt after a Rust change.
- **Harness:**
  - Run everything from the repo root.
  - Ports: pgrust 55491, C reference 55492, seeded C reference 55494.
  - `scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file|->` runs one SQL file on fresh C and subject servers. It creates the extension in a separate session, so the script's own session starts with `vector` unloaded.
  - Read each tier's verdict from `~/.cache/pgrust/pgvector-work/<tier>/<mode>/summary.tsv` before a later run overwrites it. `sqldiff.sh` keeps only its last transcripts.
  - Leave no servers running.
- **Harness scripts:**
  - Use `#!/usr/bin/env bash` and `set -euo pipefail`.
  - Stay bash 3.2 compatible: no associative arrays, `${x,,}`, `mapfile`, `sed -i` or GNU-only flags.
  - Use `${arr[@]+"${arr[@]}"}` for a possibly empty array.
  - Check each changed script with `/bin/bash -n` (macOS's 3.2).
- **Lints:** both must pass at the end of every task that touches Rust:
  - `bash crates/_support/seams_init/tests/lint-determinism.sh`;
  - `bash crates/_support/seams_init/tests/lint-seam-installs.sh` (it lists 22 allowlisted entries before this plan).
- **The plan's Rust code** mirrors patterns that compile today. If a snippet doesn't compile as written, make the smallest fix and name it in the task report; never change behavior to make it compile.
- **Disk:** about 164 GiB free. Clean up scratch data directories and staged files you create.

## Review Focus

1. **A library load where C does not load, or none where C does.**
   - C loads at the first index open, `CREATE INDEX`, opclass validation, and amutils on an extension AM.
   - C does not load at `CREATE ACCESS METHOD`, at `SELECT ... FROM pg_am`, or per scan.
   - The `WARNING ... removing it` placement and count must match C. Test: `scripts/pgvector/sql/library-load.sql` (Task 3), including a negative control.
2. **A location fix that changes anything but the location.**
   - Message, SQLSTATE, DETAIL, HINT and elevel stay as they are.
   - Soft errors stay soft: `pg_input_is_valid('abc', 'float4')` and `pg_input_error_info` keep working.
   - GUC errors from `ALTER SYSTEM`/config-file paths keep their elevel.
   - Tests: the location assertions in Tasks 4–6, plus a soft-input check in Task 5.
3. **The repair on unusual databases.**
   - The extension in a non-public schema.
   - A database already at 0.8.7, which must be a no-op.
   - Each of the three shipped generations.
   - A failure halfway, which must roll back with nothing half-created.
   - Test: `scripts/pgvector/run-repair-check.sh` (Task 7).
4. **vector.c helpers made signed (`i32`).** A negative dimension, from an `i16` on the wire or a `subvector` span, still reports C's message at C's site. `halfvec`'s `CheckStateArray` override still wins over `StateArray::check`'s new vector.c location. Tests: Task 6's recv and state-array unit tests, plus the vector.c diff section.
5. **Only `insert_grid` leaves the default bytecmp list,** and it still runs by name. Test: Task 1's `--list` count (28) and `run-bytecmp.sh pgrust insert_grid`.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `scripts/pgvector/run-bytecmp.sh` | `insert_grid` opt-in; corrected tie comments | 1 |
| `crates/contrib/pgvector_hnsw/src/utils.rs`, `crates/contrib/pgvector_hnsw_build/src/lib.rs` | `DIVERGENCE` notes on the pointer tie-break | 1 |
| `crates/backend/utils/adt/adt_amutils/src/{lib,tests}.rs` | `has_amgetbitmap`; library load for extension AMs | 2, 3 |
| `crates/backend/access/index/amapi/{Cargo.toml,src/lib.rs,src/tests.rs}` | load in `GetIndexAmRoutine`; `index_am_kind_of_handler` | 3 |
| `crates/backend/commands/amcmds/src/lib.rs` | the fence uses the no-load variant | 3 |
| `scripts/pgvector/sql/library-load.sql`, `run-diff.sh`, `tests/harness_test.sh` | the `library_load` row | 3 |
| `crates/backend/access/common/reloptions/src/{lib,local,tests}.rs` | reloption locations | 4 |
| `crates/backend/utils/misc/guc/src/{registry,lib,tests}.rs` | GUC locations | 4 |
| `scripts/pgvector/sql/core-error-locations.sql`, `run-diff.sh`, `tests/harness_test.sh` | the `core_error_locations` row | 4, 5 |
| `crates/_support/mcx/src/{lib,tests}.rs` | OOM and alloc-size locations; structured panic | 5 |
| `crates/backend/utils/adt/float/src/{lib,io,tests}.rs` | float locations | 5 |
| `crates/_support/types/types_error/src/{source_map_table,source_map}.rs` | the `hnsw.c` row | 5 |
| `crates/contrib/pgvector/src/{vec,funcs,lib}.rs` | vector.c locations | 6 |
| `scripts/pgvector/sql/error-locations.sql` | the vector.c section | 6 |
| `crates/contrib/pgvector/src/repair_gen.rs` (test-only), `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` | the generated repair | 7 |
| `scripts/pgvector/run-repair-check.sh`, `scripts/pgvector/sql/repair-verify.sql` | repair check over the three generations | 7 |
| spec, `CLAUDE.md`, the M4 report, `docs/superpowers/reports/pgvector-m4-followups-report.md` | docs and the report | 8 |

## Execution notes

- **Task order.** Tasks run in order, 1 to 8.
  - Tasks 1 and 2 are independent.
  - Task 3 edits `adt_amutils` after Task 2.
  - Task 4 creates `core-error-locations.sql`, and Task 5 appends to it.
  - Task 6's float-overflow statements need Task 5's float fix.
- **Suggested implementers:**
  - Sonnet: Tasks 1, 2 and 4 (the edits are spelled out).
  - Opus: Tasks 3, 5, 6, 7 and 8 (call-site judgment, the mcx panic, the vector.c refactor, the repair generator, the report).
  - Opus does every review and the final review.
- **Plan-defined stops**, in addition to subagent-driven-development's four:
  - any core edit outside the Global Constraints list;
  - a diff statement whose C and pgrust outputs differ in anything but the location this plan fixes. Drop the statement, record it, and do not chase it into unlisted core;
  - never move a bytecmp case to opt-in, except as Task 1 specifies;
  - never widen `rulings.toml`.
- **Expected totals after Task 8:**
  - `run-all.sh ref`: 130/130 (regress 14, TAP 48, bytecmp 28, iterscan 19, diff 21).
  - `run-all.sh pgrust`: 122/130. The 8 TAP failures are the parallel builds as before.
  - If Task 1 also demotes `insert_sparsevec_cosine`, subtract 1 from the bytecmp count and both totals.

---

### Task 1: `insert_grid` becomes an opt-in known divergence

**Files:**
- Modify: `scripts/pgvector/run-bytecmp.sh`
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs` (comment only), `crates/contrib/pgvector_hnsw_build/src/lib.rs` (comment only)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (§8.2 byte-identical row)

**Interfaces:**
- Produces: the default `CASES` with 28 entries (27 if `insert_sparsevec_cosine` is demoted too); `insert_grid` stays runnable by name.

- [ ] **Step 1: Branch and commit the plan**

```bash
git switch -c vector/m4-followups main
git add docs/superpowers/plans/2026-10-13-pgvector-m4-followups.md
git commit -q -m "docs(pgvector): M4 follow-ups plan" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
(The controller may already have done this. Skip it if `git log -1 --format=%s` shows the plan commit.)

- [ ] **Step 2: Measure `insert_sparsevec_cosine` on C against C**

```bash
pass=0; fail=0
for i in $(seq 1 30); do
  if scripts/pgvector/run-bytecmp.sh ref insert_sparsevec_cosine >/dev/null 2>&1; then pass=$((pass+1)); else fail=$((fail+1)); cp -R ~/.cache/pgrust/pgvector-work/bytecmp/ref "$HOME/.cache/pgrust/pgvector-work/flake-sv-$i"; fi
done
echo "insert_sparsevec_cosine C-vs-C: $pass ok, $fail differ"
```
Expected: 30 ok, so the case stays default. If any run differs:
- `insert_sparsevec_cosine` is demoted exactly like `insert_grid` in Step 3;
- the report quotes the first differing block from the saved `flake-sv-*` copy;
- then delete the copies.

- [ ] **Step 3: Demote `insert_grid` in `run-bytecmp.sh`**

- In the `CASES=` line (line 41), remove ` insert_grid`, keeping one line.
- Replace the comment line `# Opt-in only (known divergences, run by name): spill cosine` with `# Opt-in only (known divergences, run by name): spill cosine insert_grid`.
- In the unknown-case error text, `opt-in: spill cosine`, add ` insert_grid`.
- Directly above the `insert_grid)` arm, add:
  ```bash
      # Known divergence (opt-in): on-disk inserts into a tie-heavy graph. C's
      # HnswUpdateConnection breaks equal distances by raw element pointer
      # (CompareCandidateDistances, hnswutils.c:1007-1011, base == NULL); the
      # new element lives in insertCtx (AllocSet, hnswinsert.c:788) and the
      # loaded neighbors in updateCtx (Generation, hnswinsert.c:551), whose
      # relative addresses vary per run, so C differs from itself (~2 in 37
      # seeded C-vs-C runs). pgrust ranks the new element first, C's usual order.
  ```
- In the comment above `CASES` that says `*_grid and dups have many equal distances`, append: `insert_grid, the tie-heavy on-disk insert case, is opt-in (see its arm).`
- The `big_golomb` comment claims it checks the multi-1 MB-block tie-break. Golomb data are tie-free, so replace that claim with: `It spans several 1 MB graph blocks; Golomb data are tie-free, so it does not exercise the pointer tie-break.` (Read the comment and keep its other sentences.)

If Step 2 found a flake, demote `insert_sparsevec_cosine` the same way, with its own `Known divergence` comment citing the same C lines.

- [ ] **Step 4: `DIVERGENCE` notes in the port**

In `crates/contrib/pgvector_hnsw/src/utils.rs`, directly above `pub fn select_neighbors(`:
```rust
// DIVERGENCE (tie order only): with sort_candidates, C breaks equal distances
// by raw element POINTER when base == NULL (CompareCandidateDistances,
// hnswutils.c:1007-1011), smaller address first; pgrust ranks by element id
// (pool order). On an on-disk insert (HnswUpdateConnection from
// GetUpdateIndex, hnswinsert.c:441) the new element sits in insertCtx
// (AllocSet, hnswinsert.c:788) and the loaded neighbors in updateCtx
// (Generation, hnswinsert.c:551): their relative addresses vary per run, so
// C's own tie order is not reproducible. pgrust always ranks the new element
// (lowest id) first, C's usual outcome; bytecmp insert_grid is opt-in.
```
In `crates/contrib/pgvector_hnsw_build/src/lib.rs`, directly above `fn select_neighbors_mem(`:
```rust
// Tie order: C's in-memory serial build also breaks equal distances by raw
// element pointer (base == NULL, hnswutils.c:1007-1011), but every element
// comes from one Generation context (HnswMemoryContextAlloc, hnswbuild.c:648),
// so addresses rise with insertion order within a 1 MB block; element ids
// reproduce that order.
```

- [ ] **Step 5: The spec**

In `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, §8.2's byte-identical row currently reads:

> Two cases are opt-in known divergences: spilling past `maintenance_work_mem` (the build's memory accounting) and `vector` cosine (C's `-ffp-contract=fast`).

Change it to:

> Three cases are opt-in known divergences: spilling past `maintenance_work_mem` (the build's memory accounting), `vector` cosine (C's `-ffp-contract=fast`), and `insert_grid`. In that last case, on-disk inserts into a tie-heavy graph meet C's pointer tie-break, which is not reproducible even C against C.

If the sentence's wording differs, keep its structure and make the same change.

- [ ] **Step 6: Verify**

```bash
/bin/bash -n scripts/pgvector/run-bytecmp.sh
scripts/pgvector/run-bytecmp.sh --list | wc -w
scripts/pgvector/run-bytecmp.sh ref
cat ~/.cache/pgrust/pgvector-work/bytecmp/ref/summary.tsv | awk -F'\t' '{print $2}' | sort | uniq -c
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-bytecmp.sh pgrust
cat ~/.cache/pgrust/pgvector-work/bytecmp/pgrust/summary.tsv | awk -F'\t' '{print $2}' | sort | uniq -c
scripts/pgvector/run-bytecmp.sh pgrust insert_grid; echo "insert_grid exit $?"
cargo check --profile fast-profile --locked -p main_main -p pgvector_hnsw -p pgvector_hnsw_build
```
Expected:
- `--list` gives 28;
- `ref` 28/28 and `pgrust` 28/28;
- `insert_grid` runs by name; its verdict is recorded either way;
- `cargo check` is clean.

If `ref` flakes on another case, rerun once and record both runs.

- [ ] **Step 7: Commit**

```bash
git add scripts/pgvector/run-bytecmp.sh crates/contrib/pgvector_hnsw/src/utils.rs crates/contrib/pgvector_hnsw_build/src/lib.rs docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md
git commit -q -F - <<'EOF'
pgvector: insert_grid is an opt-in known divergence; pointer tie-break notes

C's on-disk HNSW insert breaks equal distances by palloc address
(hnswutils.c:1007-1011) across two memory contexts, so seeded C differs
from itself on insert_grid (~2 in 37 runs). The case leaves the default
bytecmp list (still runnable by name), the port records the divergence,
and big_golomb's comment no longer claims tie-break coverage.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 2: `bitmap_scan` follows `amgetbitmap`

**Files:**
- Modify: `crates/backend/utils/adt/adt_amutils/src/lib.rs`, `src/tests.rs`

**Interfaces:**
- Produces: `AmFlags.has_amgetbitmap: bool`.

- [ ] **Step 1: The failing test**

In `crates/backend/utils/adt/adt_amutils/src/tests.rs`, add:
```rust
// amutils.c:368-369: bitmap_scan is amgetbitmap != NULL. Every core AM and
// bloom has one; hnsw.c:315 and ivfflat.c:232 set amgetbitmap = NULL.
#[test]
fn amgetbitmap_matches_c_handlers() {
    install_amhandler_stub();
    for oid in [BTREE_AM_OID, HASH_AM_OID, GIST_AM_OID, GIN_AM_OID, SPGIST_AM_OID, BRIN_AM_OID, 20000] {
        assert!(am_flags(oid).unwrap().has_amgetbitmap, "{oid}");
    }
    for oid in [20001, 20002] {
        let f = am_flags(oid).unwrap();
        assert!(!f.has_amgetbitmap && f.has_amgettuple, "{oid}");
    }
}
```
Run: `cargo test --profile fast-profile --locked -p main_main -p adt_amutils --lib -- amgetbitmap_matches_c_handlers`. Expected: it fails to compile, because there is no field `has_amgetbitmap`.

- [ ] **Step 2: The field and the property**

In `crates/backend/utils/adt/adt_amutils/src/lib.rs`:
- in `struct AmFlags`, after `has_amgettuple: bool,`, add `has_amgetbitmap: bool,`;
- in each of the consts `BT`, `HASH`, `GIST`, `GIN`, `SPGIST`, `BRIN` and `BLOOM`, after its `has_amgettuple` line, add `has_amgetbitmap: true,`;
- in `HNSW` and `IVFFLAT`, add `has_amgetbitmap: false,` with the comment `// hnsw.c:315 / ivfflat.c:232: amgetbitmap = NULL.`;
- replace `Prop::BitmapScan => Some(true)` with `Prop::BitmapScan => Some(routine.has_amgetbitmap)`, using the variable name the neighboring arms use for the flags.

Run the Step 1 command again. Expected: 1 passed.

- [ ] **Step 3: Regression and end to end**

```bash
cargo test --profile fast-profile --locked -p main_main -p adt_amutils --lib -- am_flag amgetbitmap extension_am_identified phasenames
scripts/pgvector/build-pgrust.sh
cat > "$HOME/.cache/pgrust/pgvector-work/amprops.sql" <<'SQL'
CREATE TABLE t (id int, v vector(3));
INSERT INTO t VALUES (1, '[1,2,3]'), (2, '[4,5,6]');
CREATE INDEX t_hnsw ON t USING hnsw (v vector_l2_ops);
CREATE INDEX t_ivf ON t USING ivfflat (v vector_l2_ops) WITH (lists = 1);
CREATE INDEX t_btree ON t (id);
SELECT i, p, pg_index_has_property(i::regclass, p) FROM unnest(ARRAY['t_hnsw','t_ivf','t_btree']) i, unnest(ARRAY['asc','desc','nulls_first','nulls_last','orderable','distance_orderable','returnable','search_array','search_nulls','clusterable','index_scan','bitmap_scan','backward_scan','can_order','can_unique','can_multi_col','can_exclude','can_include']) p ORDER BY 1, 2;
SELECT i, p, pg_index_column_has_property(i::regclass, 1, p) FROM unnest(ARRAY['t_hnsw','t_ivf','t_btree']) i, unnest(ARRAY['asc','desc','nulls_first','nulls_last','orderable','distance_orderable','returnable','search_array','search_nulls','clusterable','index_scan','bitmap_scan','backward_scan','can_order','can_unique','can_multi_col','can_exclude','can_include']) p ORDER BY 1, 2;
SELECT a.amname, p, pg_indexam_has_property(a.oid, p) FROM pg_am a, unnest(ARRAY['asc','desc','nulls_first','nulls_last','orderable','distance_orderable','returnable','search_array','search_nulls','clusterable','index_scan','bitmap_scan','backward_scan','can_order','can_unique','can_multi_col','can_exclude','can_include']) p WHERE a.amname IN ('hnsw','ivfflat','btree') ORDER BY 1, 2;
SQL
scripts/pgvector/sqldiff.sh pgrust "$HOME/.cache/pgrust/pgvector-work/amprops.sql"
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- the unit tests all pass;
- sqldiff is identical, and the `t_hnsw`/`t_ivf` `bitmap_scan` rows read `f` on both sides;
- both lints pass.

Run `UT_KEEP` too: all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/backend/utils/adt/adt_amutils/src/lib.rs crates/backend/utils/adt/adt_amutils/src/tests.rs
git commit -q -F - <<'EOF'
amutils: bitmap_scan reports amgetbitmap, false for hnsw and ivfflat

pg_index_has_property(<hnsw|ivfflat index>, 'bitmap_scan') returned t; C
returns amgetbitmap != NULL (amutils.c:368-369), which both vector AMs
leave NULL (hnsw.c:315, ivfflat.c:232). AmFlags gains has_amgetbitmap.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 3: Loading the library where C calls the AM handler

**Files:**
- Modify: `crates/backend/access/index/amapi/Cargo.toml` (+`fmgr_seams`), `src/lib.rs`, `src/tests.rs`
- Modify: `crates/backend/commands/amcmds/src/lib.rs` (one call)
- Modify: `crates/backend/utils/adt/adt_amutils/src/lib.rs`, `src/tests.rs`
- Modify: `crates/contrib/pgvector/src/lib.rs` (remove the load-timing `DIVERGENCE` at `pg_init`)
- Create: `scripts/pgvector/sql/library-load.sql`
- Modify: `scripts/pgvector/run-diff.sh`, `scripts/pgvector/tests/harness_test.sh`

**Interfaces:**
- Produces `amapi::index_am_kind_of_handler(amhandler: Oid) -> PgResult<IndexAmKind>`, which resolves without loading.
- `amapi::GetIndexAmRoutine(amhandler)` now loads an extension handler's library: `fmgr_seams::fmgr_info::call(amhandler)`, with the result discarded.
- `adt_amutils`'s `am_flags` returns `PgResult<Option<&'static AmFlags>>`.

- [ ] **Step 1: amapi, test first**

In `crates/backend/access/index/amapi/Cargo.toml` `[dependencies]`, add `fmgr_seams = { path = "../../../utils/fmgr/fmgr_seams" }`, then run `cargo update --workspace --offline`.

In `crates/backend/access/index/amapi/src/tests.rs`, add a counting `fmgr_info` stub and a test. The existing `handler_dispatch_is_the_closed_set` stubs `pg_proc_proname` (5000 → `blhandler`, 5004 → `ivfflathandler`). Seams are set once per test binary, so install guarded:
```rust
static FMGR_INFO_CALLS: std::sync::Mutex<Vec<Oid>> = std::sync::Mutex::new(Vec::new());

fn dummy_fn(_f: Option<&mut types_fmgr::FmgrInfo>, _c: &mut types_fmgr::FunctionCallInfoBaseData) -> PgResult<datum::Datum> {
    Ok(datum::Datum::from_usize(0))
}

fn install_fmgr_info_stub() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if !fmgr_seams::fmgr_info::is_installed() {
            fmgr_seams::fmgr_info::set(|oid| {
                FMGR_INFO_CALLS.lock().unwrap().push(oid);
                Ok(types_fmgr::FmgrInfo::new(dummy_fn, 0, 0, true, false))
            });
        }
    });
}

// amapi.c:38 OidFunctionCall0(amhandler): fmgr_info loads a LANGUAGE c
// handler's library (fmgr_info_C_lang, running _PG_init once per session);
// the builtin handlers and the no-load resolver never reach it.
#[test]
fn extension_handlers_load_their_library() {
    install_proname_stub();
    install_fmgr_info_stub();
    let before = |oid| FMGR_INFO_CALLS.lock().unwrap().iter().filter(|&&o| o == oid).count();
    let (bl, ivf) = (before(5000), before(5004));
    assert_eq!(GetIndexAmRoutine(5004).unwrap(), IndexAmKind::Ivfflat);
    assert_eq!(GetIndexAmRoutine(5000).unwrap(), IndexAmKind::Bloom);
    assert_eq!((before(5000), before(5004)), (bl + 1, ivf + 1));
    assert_eq!(index_am_kind_of_handler(5004).unwrap(), IndexAmKind::Ivfflat);
    assert_eq!(before(5004), ivf + 1);
    let n = FMGR_INFO_CALLS.lock().unwrap().len();
    assert_eq!(GetIndexAmRoutine(F_BTHANDLER).unwrap(), IndexAmKind::Btree);
    assert_eq!(FMGR_INFO_CALLS.lock().unwrap().len(), n);
}
```
`install_proname_stub` stands for whatever the existing test uses to install `pg_proc_proname` (and `lookup_pg_proc_fmgr`, if it does). If that is inline in `handler_dispatch_is_the_closed_set`, factor it into a guarded helper both tests call. `FmgrInfo::new`'s argument order is the one `pgvector_ivfflat_build`'s kmeans tests use; adjust if the signature differs, and name it in the report.

Run: `cargo test --profile fast-profile --locked -p main_main -p amapi --lib -- extension_handlers_load handler_dispatch_is_the_closed_set`. Expected: it fails to compile (`index_am_kind_of_handler` not found).

- [ ] **Step 2: amapi, the load and the no-load variant**

In `crates/backend/access/index/amapi/src/lib.rs`, replace `GetIndexAmRoutine` and its doc comment's last sentence with:
```rust
pub fn GetIndexAmRoutine(amhandler: Oid) -> PgResult<IndexAmKind> {
    let kind = index_am_kind_of_handler(amhandler)?;
    // amapi.c:38 OidFunctionCall0(amhandler): its fmgr_info (fmgr.c:127 ->
    // fmgr_info_C_lang) loads a LANGUAGE c handler's library and runs its
    // _PG_init once per session. The in-tree extension AMs dispatch by
    // IndexAmKind, so pgrust runs that fmgr_info and never calls the handler.
    if matches!(kind, IndexAmKind::Hnsw | IndexAmKind::Ivfflat | IndexAmKind::Bloom)
        && builtin_index_am(amhandler).is_none()
    {
        fmgr_seams::fmgr_info::call(amhandler)?;
    }
    Ok(kind)
}

/// The IndexAmKind `amhandler` dispatches to, without loading its library:
/// for the callers where C never calls the handler (CREATE ACCESS METHOD's
/// lookup_am_handler_func, amcmds.c:78) or where relcache already has
/// (from_relam's per-scan resolver).
pub fn index_am_kind_of_handler(amhandler: Oid) -> PgResult<IndexAmKind> {
    match builtin_index_am(amhandler) {
        Some(kind) => Ok(kind),
        None => resolve_extension_handler(amhandler),
    }
}
```
In `resolve_index_am_kind`, replace `GetIndexAmRoutine(amhandler).ok()` with `index_am_kind_of_handler(amhandler).ok()`.

In `crates/backend/commands/amcmds/src/lib.rs`, in the fence (`AMTYPE_INDEX => amapi::GetIndexAmRoutine(handlerOid).is_ok(),`), use `amapi::index_am_kind_of_handler(handlerOid).is_ok()`. Add to the fence's comment: `C never calls the handler here (amcmds.c:78), so the library is not loaded.`

Run the Step 1 command. Expected: both tests pass.

- [ ] **Step 3: amutils loads for extension AMs**

In `crates/backend/utils/adt/adt_amutils/src/lib.rs`, replace `extension_am_handler` and its comment with:
```rust
// C calls the handler (GetIndexAmRoutineByAmId, amutils.c:198 / amapi.c:56-84);
// a LANGUAGE C handler's C symbol is its prosrc, which survives ALTER
// FUNCTION ... RENAME. That call's fmgr_info loads the handler's library and
// runs its _PG_init once per session (fmgr.c:127 -> fmgr_info_C_lang); pgrust
// does the load and keeps the flag tables.
fn extension_am_handler(amoid: Oid) -> PgResult<Option<Vec<u8>>> {
    let Some(handler) = syscache_seams::pg_am_amhandler::call(amoid).ok().flatten() else {
        return Ok(None);
    };
    let scratch = ::mcx::MemoryContext::new("amutils handler prosrc");
    let Some(prosrc) = syscache_seams::lookup_pg_proc_prosrc::call(scratch.mcx(), handler).ok().flatten() else {
        return Ok(None);
    };
    let symbol = prosrc.as_bytes().to_vec();
    if matches!(symbol.as_slice(), b"hnswhandler" | b"ivfflathandler" | b"blhandler") {
        fmgr_seams::fmgr_info::call(handler)?;
    }
    Ok(Some(symbol))
}
```
Thread the `PgResult` through its callers:
- `am_flags` returns `PgResult<Option<&'static AmFlags>>` and uses `extension_am_handler(other)?`;
- `indexam_property` uses `am_flags(amoid)?`;
- the phase-name function's `extension_am_handler(other)` gets `?`;
- the tests use `am_flags(..).unwrap().unwrap()`, or the pattern that compiles.

Lookup failures stay `Ok(None)` as before, and only the load propagates, as C's `OidFunctionCall0` does.

In `tests.rs`'s `install_amhandler_stub`, also install a guarded counting `fmgr_seams::fmgr_info` stub, the same shape as Step 1's. Then add:
```rust
// amutils.c:198 calls the extension AM's handler: its library loads. Core AMs
// resolve without fmgr.
#[test]
fn extension_am_properties_load_the_library() {
    install_amhandler_stub();
    let calls = |oid| FMGR_INFO_CALLS.lock().unwrap().iter().filter(|&&o| o == oid).count();
    let before = calls(9998);
    assert!(am_flags(20001).unwrap().unwrap().amcanorderbyop);
    assert_eq!(calls(9998), before + 1);
    let n = FMGR_INFO_CALLS.lock().unwrap().len();
    assert!(am_flags(BTREE_AM_OID).unwrap().is_some());
    assert_eq!(FMGR_INFO_CALLS.lock().unwrap().len(), n);
}
```
Run: `cargo test --profile fast-profile --locked -p main_main -p adt_amutils --lib -- am_flag amgetbitmap extension_am phasenames`. Expected: all pass.

- [ ] **Step 4: pgvector's comment**

In `crates/contrib/pgvector/src/lib.rs`, delete the `DIVERGENCE` paragraph above `fn pg_init`, which says pgrust never loads the library when it resolves the AM handler. Keep the rest of the comment.

- [ ] **Step 5: `library-load.sql` and its diff row**

Create `scripts/pgvector/sql/library-load.sql`:
```sql
-- Where a session first loads vector (running _PG_init, which reserves the
-- hnsw and ivfflat GUC prefixes and removes their placeholders with a
-- WARNING): C loads it whenever fmgr resolves one of its functions, including
-- the AM handler (amapi.c:38 OidFunctionCall0) at CREATE INDEX (indexcmds.c:864),
-- at a session's first open of an hnsw/ivfflat index (relcache.c:1432) and
-- in amutils' property functions, but not at SELECT ... FROM pg_am.
-- sqldiff.sh creates the extension in another session; each \c below starts
-- a session with vector unloaded. Plain verbosity: core_error_locations
-- compares these messages' LOCATIONs.

-- Negative control: reading pg_am loads nothing.
SET hnsw.typo = 1;
SELECT amname FROM pg_am WHERE amname IN ('hnsw', 'ivfflat') ORDER BY 1;
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- amutils on the AM loads vector.
\c
SET hnsw.typo = 1;
SET ivfflat.typo = 1;
SELECT pg_indexam_has_property(oid, 'can_order') FROM pg_am WHERE amname = 'hnsw';
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- A session's first open of an hnsw index loads vector (the vector(3)
-- column's typmod input already loaded it in the session that made the table).
CREATE TABLE t (id int, v vector(3));
CREATE INDEX t_v_idx ON t USING hnsw (v vector_l2_ops);
\c
SET hnsw.typo = 1;
INSERT INTO t (id) VALUES (1);
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- CREATE INDEX loads vector at DefineIndex, for ivfflat too.
\c
SET ivfflat.typo = 1;
CREATE INDEX t_v_ivf ON t USING ivfflat (v vector_l2_ops) WITH (lists = 1);
SET ivfflat.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';
```
Run it on both sides:
```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/library-load.sql
cat ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/ref.out
```
Expected: identical. On C:
- The negative control prints no WARNING. Both SETs succeed and no module row appears.
- Each of the other three sections prints `WARNING:  invalid configuration parameter name "hnsw.typo", removing it` (and `ivfflat.typo` where it was set) at the load statement.
- The `typo2` SET then fails with `invalid configuration parameter name ... "hnsw" is a reserved prefix.`, and `vector | 0.8.7` is listed.

If `\c` prints differently on the two sides, use `\connect - -` instead. If both forms differ, report it (stop). Read `ref.out` and confirm C behaves as described before trusting the comparison. If C differs from this description in a section, fix the SQL's comments to say what C does and keep the section only if it still separates "loads" from "does not load".

In `scripts/pgvector/run-diff.sh`, after the `ivfflat_errors` block, add:
```bash
# Where a session first loads vector (_PG_init's prefix reservation), against
# C's fmgr loads at the AM handler call sites (M4 follow-ups, Task 3).
if "$here/sqldiff.sh" "$mode" "$here/sql/library-load.sql" >"$out/library-load.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'library_load\t%s\n' "$r" | tee -a "$out/summary.tsv"
```
In `scripts/pgvector/tests/harness_test.sh`'s `test_diff_ref`, change both `19`s to `20`.

- [ ] **Step 6: Verify**

```bash
/bin/bash -n scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
scripts/pgvector/run-diff.sh ref
scripts/pgvector/run-diff.sh pgrust
cat ~/.cache/pgrust/pgvector-work/diff/pgrust/summary.tsv
scripts/pgvector/run-regress.sh pgrust
scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 016_hnsw_inserts.pl 001_ivfflat_wal.pl 007_ivfflat_inserts.pl
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- diff 20/20 in both modes;
- regress 14/14;
- the four TAP tests ok. Their standbys open indexes on new threads, where each backend loads `vector`.
- both lints pass.

`UT_KEEP`: all pass.

- [ ] **Step 7: Commit**

```bash
git add Cargo.lock crates/backend/access/index/amapi crates/backend/commands/amcmds/src/lib.rs crates/backend/utils/adt/adt_amutils crates/contrib/pgvector/src/lib.rs scripts/pgvector/sql/library-load.sql scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
amapi, amutils: load an extension AM's library where C calls its handler

C's OidFunctionCall0(amhandler) runs fmgr_info, which loads a LANGUAGE c
handler's library and its _PG_init once per session: at a session's first
index open, CREATE INDEX, opclass validation and amutils. GetIndexAmRoutine
and amutils' property and phase-name functions now do that fmgr_info for
the extension AMs; CREATE ACCESS METHOD's fence and the per-scan resolver
use the new no-load index_am_kind_of_handler, as C never calls the handler
there. run-diff.sh's library_load row compares the load points with C.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 4: C locations for reloption and GUC errors

**Files:**
- Modify: `crates/backend/access/common/reloptions/src/lib.rs`, `src/local.rs`, `src/tests.rs`
- Modify: `crates/backend/utils/misc/guc/src/registry.rs`, `src/lib.rs`, `src/tests.rs`
- Create: `scripts/pgvector/sql/core-error-locations.sql`
- Modify: `scripts/pgvector/run-diff.sh`, `scripts/pgvector/tests/harness_test.sh`

**Interfaces:**
- Produces `scripts/pgvector/sql/core-error-locations.sql` (Task 5 appends) and the diff row `core_error_locations`.

Each site below gets `.with_location(...)` immediately after its `.into_error()` (or on the helper's `PgError`). The fixes add a location and nothing else.

| Crate file, site (current message) | `.with_location(...)` |
|---|---|
| reloptions `lib.rs` `parseRelOptions`: `unrecognized parameter "{name}"` | `("reloptions.c", 1488, "parseRelOptionsInternal")` |
| reloptions `lib.rs` `parse_one_reloption`: `parameter "{}" specified more than once` | `("reloptions.c", 1601, "parse_one_reloption")` |
| reloptions `lib.rs` `parse_opt_value` Bool: `invalid value for boolean option` | `("reloptions.c", 1617, "parse_one_reloption")` |
| … Int invalid: `invalid value for integer option` | `("reloptions.c", 1629, "parse_one_reloption")` |
| … Int out of bounds: `value {value} out of bounds for option` | `("reloptions.c", 1637, "parse_one_reloption")` |
| … Real invalid: `invalid value for floating point option` | `("reloptions.c", 1649, "parse_one_reloption")` |
| … Real out of bounds | `("reloptions.c", 1657, "parse_one_reloption")` |
| … Enum: `invalid value for enum option` | `("reloptions.c", 1681, "parse_one_reloption")` |
| reloptions `local.rs` duplicate keyword: `parameter "{kw}" specified more than once` | `("reloptions.c", 1601, "parse_one_reloption")` |
| reloptions `local.rs` `unrecognized parameter "{name}"` | `("reloptions.c", 1488, "parseRelOptionsInternal")` |
| guc `registry.rs` Bool: `parameter "{name}" requires a Boolean value` | `("guc.c", 3145, "parse_and_validate_value")` |
| guc `registry.rs` Int parse failure: `invalid_value_error(...)` | `("guc.c", 3166, "parse_and_validate_value")` |
| guc `registry.rs` Int range: `... is outside the valid range ...` | `("guc.c", 3186, "parse_and_validate_value")` |
| guc `registry.rs` Real parse failure: `invalid_value_error(...)` | `("guc.c", 3207, "parse_and_validate_value")` |
| guc `registry.rs` Real range | `("guc.c", 3227, "parse_and_validate_value")` |
| guc `registry.rs` Enum: `invalid value for parameter ...` (with the `Available values` hint) | `("guc.c", 3282, "parse_and_validate_value")` |
| guc `registry.rs` `call_bool_check_hook` failure | `("guc.c", 6834, "call_bool_check_hook")` |
| … `call_int_check_hook` | `("guc.c", 6868, "call_int_check_hook")` |
| … `call_real_check_hook` | `("guc.c", 6902, "call_real_check_hook")` |
| … `call_string_check_hook` | `("guc.c", 6945, "call_string_check_hook")` |
| … `call_enum_check_hook` | `("guc.c", 6987, "call_enum_check_hook")` |
| guc `lib.rs` `assignable_custom_variable_name`: invalid name (`Custom parameter names must be ...`) | replace `.with_funcname("assignable_custom_variable_name")` with `.with_location("guc.c", 1139, "assignable_custom_variable_name")` |
| … reserved prefix (`"{rcprefix}" is a reserved prefix.`) | the same replacement, line `1156` |
| … unrecognized (single-part name) | `unrecognized(name).with_location("guc.c", 1169, "assignable_custom_variable_name")`, at the call site; `unrecognized()` itself serves other C functions and stays unlocated |
| guc `lib.rs` `MarkGUCPrefixReserved`: the WARNING's `.finish(ErrorLocation::new(file!(), line!() as i32, "MarkGUCPrefixReserved"))` | `.finish(types_error::ErrorLocation::new("guc.c", 5312, "MarkGUCPrefixReserved"))` |

Mechanics:
- **`err(..)` callers in `registry.rs`.** `err` serves other sites too, so chain on the call: `err(ERRCODE_INVALID_PARAMETER_VALUE, format!(..)).with_location("guc.c", 3186, "parse_and_validate_value").into()`.
- **`invalid_value_error`.** Give it a `line: i32` parameter, end it with `b.into_error().with_location("guc.c", line, "parse_and_validate_value")`, and pass `3166` from the Int arm and `3207` from the Real arm.
- **The check hooks.** Add `$line:expr` to `check_hook_caller!`:
  - its error arm becomes `Err(check_hook_error($fallback(conf, newval)).with_location("guc.c", $line, stringify!($fname)).into())`;
  - each of the five invocations gains its line (6834, 6868, 6902, 6945, 6987).
  - `stringify!($fname)` yields `call_bool_check_hook` and so on, which are C's names.
- **reloptions.** Chain `.with_location` between `.into_error()` and `.into()` at each site. The pgrust-only columnar/zstd/parallel messages near `lib.rs:902-932` have no C counterpart and stay unlocated.

- [ ] **Step 1: The failing tests**

In `crates/backend/access/common/reloptions/src/tests.rs`:
- Add a helper next to `parse_heap_err` that returns the whole `PgError` (or extend `parse_heap_err` to return it as well), plus:
  ```rust
  fn loc(e: &types_error::PgError) -> (Option<&str>, i32, Option<&str>) {
      let l = e.location().expect("location");
      (l.filename.as_deref(), l.lineno, l.funcname.as_deref())
  }
  ```
- In `validation_error_texts_match_c`, after each existing message assertion, assert the location from the table, for example `(Some("reloptions.c"), 1637, Some("parse_one_reloption"))` for fillfactor out of bounds. The cases there:
  - fillfactor out of bounds (1637);
  - real out of bounds (1657);
  - invalid integer (1629);
  - invalid boolean (1617);
  - invalid float (1649);
  - duplicate (1601);
  - the two other integer cases (1629).
- Add `reloption_enum_and_unrecognized_locations`:
  - an enum reloption with a bad value gives 1681 `parse_one_reloption`. Use any heap or index reloption of enum type that the file's other tests already parse, such as `vacuum_index_cleanup = maybe` if it exists, or `buffering` for gist;
  - an unknown key gives 1488 `parseRelOptionsInternal`;
  - assert the message text too.

In `crates/backend/utils/misc/guc/src/tests.rs`:
- Add the same `loc` helper.
- Extend `invalid_values_error_at_session_source` to assert:
  - `work_mem = banana` gives 3166;
  - the `1XB` hint case gives 3166;
  - the out-of-range case gives 3186;
  - `statement_timeout` invalid gives 3166;
  - `enable_seqscan = maybe` gives 3145.

  All are `parse_and_validate_value`.
- Extend the reserved-prefix test(s) (both the `SET` path and the `GUCArrayAdd`/`check_GUC_name_for_parameter_acl` path) with `(Some("guc.c"), 1156, Some("assignable_custom_variable_name"))`.
- Extend the unrecognized single-part-name test with 1169.
- Extend the `MarkGUCPrefixReserved` warning test with `(Some("guc.c"), 5312, Some("MarkGUCPrefixReserved"))` on the emitted WARNING.
- Add `real_and_enum_errors_carry_c_locations`:
  - a real GUC (e.g. `seq_page_cost`) set to `abc` gives 3207, and set to `-1` gives 3227;
  - an enum GUC (e.g. `client_min_messages`) set to `bogus` gives 3282 with the `Available values` hint;
  - an invalid custom name `a..b` gives 1139.

  Use the API the neighboring tests use to set GUCs.
- Add a check-hook test if the file already installs a check hook somewhere. Otherwise test one hook through the GUC that has one and that other tests already exercise, e.g. `default_text_search_config` or `search_path`. If neither exists in this unit-test binary, skip it, note that in the report, and rely on the macro change plus code review.

Run:
```bash
cargo test --profile fast-profile --locked -p main_main -p reloptions -p guc --lib -- validation_error_texts_match_c reloption_enum_and_unrecognized invalid_values_error_at_session_source reserved unrecognized MarkGUCPrefixReserved real_and_enum_errors
```
Expected: the new location assertions fail; filename `guc.c`/`reloptions.c` with line 0, or no funcname.

- [ ] **Step 2: Apply the table**

Make every edit in the table above. Rerun the Step 1 command. Expected: all pass.

- [ ] **Step 3: `core-error-locations.sql` and its diff row**

Create `scripts/pgvector/sql/core-error-locations.sql`:
```sql
-- Core error LOCATIONs that pgvector SQL reaches (run-diff.sh's
-- core_error_locations row, sqldiff.sh --verbose): reloptions.c
-- parse_one_reloption / parseRelOptionsInternal, guc.c parse_and_validate_value,
-- assignable_custom_variable_name and MarkGUCPrefixReserved (M4 follow-ups,
-- Task 4); float.c and mcxt.c follow (Task 5). Placeholders come first: the
-- library loads at the first vector call, when C defines the hnsw.* and
-- ivfflat.* settings and reserves their prefixes.
SET hnsw.placeholder_a = 1;
SET ivfflat.placeholder_b = 1;
SELECT '[1]'::vector;
SET hnsw.nonexistent = 1;
SET ivfflat.nonexistent = 1;
SET ivfflat..x = 1;
SET nonexistent_param = 1;
SET hnsw.ef_search = 0;
SET hnsw.ef_search = 1001;
SET hnsw.max_scan_tuples = 0;
SET ivfflat.probes = 0;
SET ivfflat.probes = 32769;
SET ivfflat.max_probes = 0;
SET hnsw.ef_search = 'abc';
SET ivfflat.probes = 'abc';
SET ivfflat.max_probes = 99999999999;
SET hnsw.scan_mem_multiplier = 0;
SET hnsw.scan_mem_multiplier = 1001;
SET hnsw.scan_mem_multiplier = 'abc';
SET hnsw.iterative_scan = bogus;
SET ivfflat.iterative_scan = bogus;
SET ivfflat.iterative_scan = strict_order;
SET enable_seqscan = maybe;
SET hnsw.ef_search = 100;
SHOW hnsw.ef_search;
CREATE TABLE t (id int, v vector(3));
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 1);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 101);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 3);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 1001);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 'abc');
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 'abc');
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 8, m = 9);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (nope = 1);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 0);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 32769);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 'abc');
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (nope = 1);
CREATE INDEX t_h ON t USING hnsw (v vector_l2_ops) WITH (m = 8);
ALTER INDEX t_h SET (m = 1);
ALTER INDEX t_h SET (ef_construction = 'abc');
```
Run on C first, then compare:
```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/core-error-locations.sql
cat ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/diff.txt
```
Expected: identical, with every LOCATION line one of this task's sites.
- If a statement's C output differs from pgrust in anything but those LOCATIONs, for example a message, or a site this plan does not fix, delete that statement, quote both outputs in the report, and keep going (plan-defined stop rule).
- `WITH (m = 8, m = 9)` may be rejected before `parse_one_reloption`; keep it only if C reports 1601.

In `scripts/pgvector/run-diff.sh`, after the `library_load` block, add:
```bash
# Core error sites pgvector SQL reaches (reloptions.c, guc.c; float.c and
# mcxt.c from Task 5), LOCATION included (M4 follow-ups).
if "$here/sqldiff.sh" "$mode" --verbose "$here/sql/core-error-locations.sql" >"$out/core-error-locations.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'core_error_locations\t%s\n' "$r" | tee -a "$out/summary.tsv"
```
In `test_diff_ref`, change both `20`s to `21`.

- [ ] **Step 4: Verify**

```bash
/bin/bash -n scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
scripts/pgvector/run-diff.sh ref
scripts/pgvector/run-diff.sh pgrust
scripts/pgvector/run-regress.sh pgrust
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- diff 21/21 in both modes;
- regress 14/14. Upstream expected outputs print no LOCATION, so they don't move.
- both lints pass.

`UT_KEEP`: all pass. Also run `cargo test --profile fast-profile --locked -p main_main -p guc -p reloptions --lib -- tests::` once. Expected: everything that passed before still passes; report the counts before and after.

- [ ] **Step 5: Commit**

```bash
git add crates/backend/access/common/reloptions crates/backend/utils/misc/guc scripts/pgvector/sql/core-error-locations.sql scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
reloptions, guc: C error locations; core_error_locations diff row

parse_one_reloption and parseRelOptionsInternal, parse_and_validate_value,
the five call_*_check_hook functions, assignable_custom_variable_name and
MarkGUCPrefixReserved now carry C's file:line and function instead of
reloptions.c:0 / guc.c:0. run-diff.sh's core_error_locations row compares
the pgvector-reachable ones with C under VERBOSITY verbose.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 5: mcx, float.c and the `hnsw.c` source-map row

**Files:**
- Modify: `crates/_support/mcx/src/lib.rs`, `src/tests.rs`
- Modify: `crates/backend/utils/adt/float/src/lib.rs`, `src/io.rs`, `src/tests.rs`
- Modify: `crates/_support/types/types_error/src/source_map_table.rs`, `src/source_map.rs`
- Modify: `crates/contrib/pgvector_ivfflat_build/src/kmeans.rs` (test assertions only)
- Modify: `scripts/pgvector/sql/core-error-locations.sql` (append)

**Interfaces:**
- Consumes Task 4's `core-error-locations.sql`.
- Produces float and mcx errors with C locations.

- [ ] **Step 1: The failing tests**

- `crates/_support/mcx/src/tests.rs`:
  - in `oom_error_shape_matches_mcxt_c`, assert `(Some("mcxt.c"), 1167, Some("MemoryContextAllocationFailure"))`;
  - in `alloc_size_gate_matches_palloc`, assert `check_alloc_size(..).unwrap_err()`'s location is `(Some("mcxt.c"), 1180, Some("MemoryContextSizeFailure"))`;
  - change `assert_ceiling_error` to accept a structured payload. Try `types_error::pg_error_from_panic(payload)` first and check its `.message()` and location (1180, `MemoryContextSizeFailure`); keep the `String`/`&str` fallback only for non-`std` builds.
- `crates/backend/utils/adt/float/src/tests.rs`:
  - add `float_error_helpers_carry_c_locations`: `float_overflow_error()` gives `(float.c, 90, float_overflow_error)`, `float_underflow_error()` gives 98 and `float_zero_divide_error()` gives 106;
  - extend `float8in_error_surface_matches_live_pg` (or add `float_input_errors_carry_c_locations`):
    - `float4in("1e40")` gives 288 `float4in_internal`;
    - `float8in("1e400")` gives 490 `float8in_internal`;
    - `float4in("")` 208, `float4in("abc")` 295, `float4in("1.5x")` 309;
    - `float8in("")` 414, `float8in("abc")` 497, `float8in("1.5x")` 511.

    Use the functions and signatures the existing test calls.
  - add `float_soft_input_errors_stay_soft`: a soft-error (escontext) parse of `abc` as float4 returns the saved error, and its location is 295 (Review Focus 2). Use whatever soft-input API the crate's tests already use; if none, test it end to end in Step 4 with `SELECT pg_input_is_valid('abc', 'float4'), (pg_input_error_info('abc', 'float4')).message;` instead.
- `crates/_support/types/types_error/src/source_map.rs`:
  - in `rust_paths_map_to_c_basenames`, add `assert_eq!(c_basename_for_rust_path("crates/contrib/pgvector_hnsw/src/scan.rs").as_deref(), Some("hnsw.c"));`;
  - in `overrides_sorted_unique_and_non_default`, assert every VALUE matches `[A-Za-z0-9_.+-]+\.(c|l|y)`: no `{`, `(`, `)`, `/` or whitespace. Some keys are odd but harmless, so check values only.
- `crates/contrib/pgvector_ivfflat_build/src/kmeans.rs`: in `ivf_kmeans_buffers_raise_out_of_memory`, assert the location of each error it checks: 1167 `MemoryContextAllocationFailure` for `out of memory`, 1180 `MemoryContextSizeFailure` for the size error.

Run:
```bash
cargo test --profile fast-profile --locked -p main_main -p mcx -p adt_float -p types_error -p pgvector_ivfflat_build --lib -- oom_error_shape alloc_size_gate ceiling float_error_helpers float8in_error_surface float_input_errors float_soft rust_paths_map overrides_sorted ivf_kmeans_buffers
```
Expected: the new assertions fail.

- [ ] **Step 2: mcx**

In `crates/_support/mcx/src/lib.rs`:
- In `oom_named`, after `.with_detail(..)`, append `.with_location("mcxt.c", 1167, "MemoryContextAllocationFailure")`.
- `invalid_alloc_size` becomes:
  ```rust
  #[cold]
  fn invalid_alloc_size(request: usize) -> alloc::boxed::Box<PgError> {
      invalid_alloc_size_error(request).into()
  }

  // MemoryContextSizeFailure (mcxt.c:1172-1181): elog(ERROR), XX000.
  #[cold]
  fn invalid_alloc_size_error(request: usize) -> PgError {
      PgError::error(alloc::format!("invalid memory alloc request size {request}"))
          .with_location("mcxt.c", 1180, "MemoryContextSizeFailure")
  }
  ```
- `alloc_ceiling_exceeded` becomes:
  ```rust
  #[cold]
  #[inline(never)]
  fn alloc_ceiling_exceeded(size: usize) -> ! {
      // A PgError payload: the statement boundary's pg_error_from_panic keeps
      // its LOCATION (MemoryContextSizeFailure, mcxt.c:1180). no_std builds
      // keep the string form.
      #[cfg(any(feature = "std", test))]
      std::panic::panic_any(alloc::boxed::Box::new(invalid_alloc_size_error(size)));
      #[cfg(not(any(feature = "std", test)))]
      panic!("invalid memory alloc request size {size}")
  }
  ```
  If `pg_error_from_panic` accepts `PgError` rather than `Box<PgError>`, pass that form instead (read `types_error/src/pg_error.rs:585`), and name the choice in the report.

- [ ] **Step 3: float**

In `crates/backend/utils/adt/float/src/lib.rs`, append to each helper:
- `float_overflow_error`: `.with_location("float.c", 90, "float_overflow_error")`;
- `float_underflow_error`: `.with_location("float.c", 98, "float_underflow_error")`;
- `float_zero_divide_error`: `.with_location("float.c", 106, "float_zero_divide_error")`.

In `crates/backend/utils/adt/float/src/io.rs`, give `invalid_input` and `out_of_range` a C site, and pass it from each caller:
```rust
#[cold]
#[inline(never)]
fn invalid_input(type_name: &str, orig_string: &str, line: i32, func: &'static str) -> PgError {
    PgError::error(format!(
        "invalid input syntax for type {type_name}: \"{orig_string}\""
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("float.c", line, func)
}

#[cold]
#[inline(never)]
fn out_of_range(errnumber: &str, fixed_type: &str, line: i32, func: &'static str) -> PgError {
    PgError::error(format!(
        "\"{errnumber}\" is out of range for type {fixed_type}"
    ))
    .with_sqlstate(ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE)
    .with_location("float.c", line, func)
}
```
The call sites, each by its condition. Line numbers are approximate; match by condition.

| `io.rs` site (condition) | C site | arguments |
|---|---|---|
| float8 path, `start >= bytes.len()` (empty after leading space) | `float8in_internal` `*num == '\0'` | `414, "float8in_internal"` |
| float8 path, `out_of_range(token, "double precision")` | range | `490, "float8in_internal"` |
| float8 path, parse `None` | `endptr == num` | `497, "float8in_internal"` |
| float8 path, `end != bytes.len()` (junk) | `*endptr != '\0'` | `511, "float8in_internal"` |
| float4 path, `start >= bytes.len()` | empty | `208, "float4in_internal"` |
| float4 path, `out_of_range(token, "real")` | range | `288, "float4in_internal"` |
| float4 path, parse `None` | invalid | `295, "float4in_internal"` |
| float4 path, `end != bytes.len()` | junk | `309, "float4in_internal"` |

The function is the C function, not `type_name`. `float8in_internal` also serves the geometric types, which pass their own `type_name`, and C reports `float8in_internal` for them too.

Rerun the Step 1 command. Expected: all pass.

- [ ] **Step 4: The source-map row, and the SQL**

In `crates/_support/types/types_error/src/source_map_table.rs`, change `("pgvector_hnsw", "{hnsw.c"),` to `("pgvector_hnsw", "hnsw.c"),`. Leave the header line and the other rows alone.

Append to `scripts/pgvector/sql/core-error-locations.sql`:
```sql
-- float.c (float_overflow_error, float_underflow_error,
-- float_zero_divide_error, float4in_internal, float8in_internal) and mcxt.c
-- (MemoryContextSizeFailure), M4 follow-ups, Task 5.
SELECT '1e38'::float4 * 10::float4;
SELECT '1e-38'::float4 * '1e-10'::float4;
SELECT 1e300::float8 * 1e300::float8;
SELECT 1.0::float8 / 0.0::float8;
SELECT 1.0::float4 / 0.0::float4;
SELECT 1e39::numeric::float4;
SELECT '1e40'::float4;
SELECT '1e400'::float8;
SELECT ''::float4;
SELECT 'abc'::float4;
SELECT '1.5x'::float4;
SELECT ''::float8;
SELECT 'abc'::float8;
SELECT '1.5x'::float8;
SELECT pg_input_is_valid('abc', 'float4'), (pg_input_error_info('abc', 'float4')).message;
SELECT '[3e38]'::vector + '[3e38]';
SELECT (repeat('x', 500000000) || repeat('y', 600000000));
```

- [ ] **Step 5: Verify**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/core-error-locations.sql
cat ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/diff.txt
scripts/pgvector/run-diff.sh ref
scripts/pgvector/run-diff.sh pgrust
scripts/pgvector/run-regress.sh pgrust
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- sqldiff identical;
- diff 21/21 in both modes;
- regress 14/14;
- both lints pass.

The plan-defined stop rule applies to any statement that differs in anything but these LOCATIONs. If the `repeat` statement still shows `mcxt.c:0` on pgrust, find which catcher builds that error. The research saw `mcxt.c:0`, not `postgres.c:0`, so something other than main_loop converts the panic. Make that catcher prefer `pg_error_from_panic` only if it lives in a crate on the allowed list; otherwise drop the statement, keep the unit test, and record the catcher's location.

`UT_KEEP`: all pass. Also run `cargo test --profile fast-profile --locked -p main_main -p mcx -p adt_float -p adt_numeric -p types_error --lib -- tests::`. Expected: everything that passed before still passes; report the counts.

- [ ] **Step 6: Commit**

```bash
git add crates/_support/mcx crates/backend/utils/adt/float crates/_support/types/types_error crates/contrib/pgvector_ivfflat_build/src/kmeans.rs scripts/pgvector/sql/core-error-locations.sql
git commit -q -F - <<'EOF'
mcx, float, types_error: C error locations; the hnsw.c source-map row

mcx's out of memory and invalid alloc request size errors carry
MemoryContextAllocationFailure, mcxt.c:1167 and MemoryContextSizeFailure,
mcxt.c:1180 (the over-ceiling panic now carries a PgError payload);
adt_float's overflow, underflow, zero-divide and float4/float8 input
errors carry float.c's lines, which also fixes numeric_float4 and every
pgvector overflow; the pgvector_hnsw source-map row loses its stray '{'.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 6: vector.c's error locations

**Files:**
- Modify: `crates/contrib/pgvector/src/vec.rs`, `src/funcs.rs`, `src/lib.rs` (header)
- Modify: `scripts/pgvector/sql/error-locations.sql`

**Interfaces:**
- Produces `pub(crate) fn ereport(code, msg, line, func) -> Box<PgError>` in `vec.rs` (file `vector.c`).
- `check_dim(dim: i32)` and `check_expected_dim(typmod: i32, dim: i32)` become signed, like `halfvec.rs`.
- Produces `pub(crate) fn vector_recv_body(...)`, extracted for tests.

C sites (vector.c, closing line; the function is the C function, so the static helpers report their own names, as `halfvec.rs` does):

| C function | line | message / SQLSTATE | pgrust site |
|---|---|---|---|
| CheckDims | 76 | `different vector dimensions %d and %d` 22000 | `vec.rs` `check_dims` |
| CheckExpectedDim | 88 | `expected %d dimensions, not %d` 22000 | `vec.rs` `check_expected_dim` |
| CheckDim | 100 | `vector must have at least 1 dimension` 22000 | `vec.rs` `check_dim` |
| CheckDim | 105 | `vector cannot have more than %d dimensions` 54000 | `vec.rs` `check_dim` |
| CheckElement | 117 | `NaN not allowed in vector` 22000 | `vec.rs` `check_element` |
| CheckElement | 122 | `infinite value not allowed in vector` 22000 | `vec.rs` `check_element` |
| CheckStateArray | 169 | `%s: expected state array` XX000 | `funcs.rs` `StateArray::check` |
| vector_in | 194 | invalid input + DETAIL `Vector contents must start with "[".` 22P02 | `parse_vector` `[` missing |
| vector_in | 204 | `vector must have at least 1 dimension` 22000 | `parse_vector` `[]` |
| vector_in | 214 | `vector cannot have more than %d dimensions` 54000 | `parse_vector` `dim == VECTOR_MAX_DIM` |
| vector_in | 223 | invalid input 22P02 | `parse_vector` `pt >= n` |
| vector_in | 234 | invalid input 22P02 | `parse_vector` `strtof_prefix` None |
| vector_in | 240 | `"%s" is out of range for type vector` 22003 | `parse_vector` `Erange` |
| vector_in | 260 | invalid input 22P02 | `parse_vector` bad separator |
| vector_in | 271 | invalid input + DETAIL `Junk after closing right brace.` 22P02 | `parse_vector` junk |
| vector_typmod_in | 356 | `invalid type modifier` 22023 | `funcs.rs` `fc_vector_typmod_in` |
| vector_typmod_in | 361 | `dimensions for type vector must be at least 1` 22023 | same |
| vector_typmod_in | 366 | `dimensions for type vector cannot exceed %d` 22023 | same |
| vector_recv | 393 | `expected unused to be 0, not %d` 22000 | `funcs.rs` `fc_vector_recv` |
| array_to_vector | 459 | `array must be 1-D` 22000 | `funcs.rs` `fc_array_to_vector` |
| array_to_vector | 464 | `array must not contain nulls` 22004 | same |
| array_to_vector | 498 | `unsupported array type` 22000 | same |
| subvector | 998 | `vector must have at least 1 dimension` 22000 (count < 1) | `funcs.rs` `fc_subvector` |
| subvector | 1015 | `vector must have at least 1 dimension` 22000 (start > dim) | same |

`sparsevec_to_vector`'s `index out of bounds` (1343) already has its location.

- [ ] **Step 1: The failing tests**

In `crates/contrib/pgvector/src/vec.rs`'s tests:
- Add a helper that returns the parse error as a `PgError` (alongside the existing `parse()` that returns its message), plus a `loc` helper like Task 4's.
- Add `parse_errors_carry_vector_c_locations`, asserting `(Some("vector.c"), line, Some(func))` for:
  - `1,2,3` → 194 `vector_in`;
  - `[]` → 204 `vector_in`;
  - a 16,001-element literal → 214 `vector_in`;
  - `[` → 223 `vector_in`;
  - `[hello,1]` → 234 `vector_in`;
  - `[4e38]` → 240 `vector_in`;
  - `[1,2,3` → 260 `vector_in`;
  - `[1,2,3]9` → 271 `vector_in`;
  - `[NaN,1]` → 117 `CheckElement`;
  - `[Infinity,1]` → 122 `CheckElement`;
  - and, through `parse_vector(.., typmod = 2, ..)` on `[1,2,3]`, → 88 `CheckExpectedDim`.

In `crates/contrib/pgvector/src/funcs.rs`'s tests:
- Extend `vector_combine_matches_0_8_7` to assert:
  - `expected 1 dimensions, not 2` at 88 `CheckExpectedDim`;
  - `vector cannot have more than 16000 dimensions` at 105 `CheckDim`.
- Add `vector_recv_errors_carry_vector_c_locations`, using `vector_recv_body` from Step 2 on hand-built wire images (`i16 dim`, `i16 unused`, `f32` big-endian values):
  - `dim = 0` → 100 `CheckDim`;
  - `dim = -1` → 100 `CheckDim`;
  - `dim = 16001` → 105 `CheckDim`;
  - typmod 2 with `dim = 3` → 88 `CheckExpectedDim`;
  - `unused = 1` → 393 `vector_recv`;
  - a NaN element → 117 `CheckElement`;
  - an infinite element → 122.

  Messages must equal today's.
- Add `state_array_location_vector_and_halfvec`: `StateArray::check` on a wrong-shaped array gives 169 `CheckStateArray` in vector.c, and halfvec's `check_state_array` wrapper on the same input still gives `halfvec.c` 173.
- Add `array_to_vector_unsupported_type_location`: call the array-cast path with an `int2[]` (or `text[]`) image, built the way the existing `real_array_casts_to_vector` builds arrays; it gives 498 `array_to_vector`. If no helper can build a non-castable array image, skip this test and say so; the site has the same structure as `halfvec`'s.

Run: `cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- parse_errors_carry vector_combine_matches vector_recv_errors state_array_location array_to_vector_unsupported`. Expected: it fails to compile (`vector_recv_body` not found) or fails on the locations.

- [ ] **Step 2: The helpers**

In `vec.rs`, model the helpers on `halfvec.rs`:
```rust
// An ereport/elog of vector.c, located at the call's last line.
#[cold]
pub(crate) fn ereport(code: SqlState, msg: impl Into<String>, line: i32, func: &'static str) -> Box<PgError> {
    PgError::error(msg).with_sqlstate(code).with_location("vector.c", line, func).into()
}

// CheckDim (vector.c:96-106). Signed like C's int: a negative dimension
// (an i16 off the wire, a subvector span) reports "at least 1 dimension".
pub fn check_dim(dim: i32) -> PgResult<()> {
    if dim < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "vector must have at least 1 dimension", 100, "CheckDim"));
    }
    if dim > VECTOR_MAX_DIM as i32 {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("vector cannot have more than {VECTOR_MAX_DIM} dimensions"),
            105,
            "CheckDim",
        ));
    }
    Ok(())
}

// CheckExpectedDim (vector.c:83-89).
pub fn check_expected_dim(typmod: i32, dim: i32) -> PgResult<()> {
    if typmod != -1 && typmod != dim {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, format!("expected {typmod} dimensions, not {dim}"), 88, "CheckExpectedDim"));
    }
    Ok(())
}

// CheckElement (vector.c:111-123).
pub fn check_element(value: f32) -> PgResult<()> {
    if value.is_nan() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "NaN not allowed in vector", 117, "CheckElement"));
    }
    if value.is_infinite() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "infinite value not allowed in vector", 122, "CheckElement"));
    }
    Ok(())
}

// CheckDims (vector.c:71-77).
pub fn check_dims(a: &VecView<'_>, b: &VecView<'_>) -> PgResult<()> {
    if a.dim() != b.dim() {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("different vector dimensions {} and {}", a.dim(), b.dim()),
            76,
            "CheckDims",
        ));
    }
    Ok(())
}

// vector_in's invalid-input ereports (vector.c:191-271), at `line`.
#[cold]
fn invalid_text(lit: &[u8], detail: Option<&str>, line: i32) -> Box<PgError> {
    let mut e = PgError::error(format!(
        "invalid input syntax for type vector: \"{}\"",
        String::from_utf8_lossy(lit)
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("vector.c", line, "vector_in");
    if let Some(d) = detail {
        e = e.with_detail(d);
    }
    e.into()
}
```
Delete `dim_error` and `max_dim_error`. In `parse_vector`:
- `[` missing gives `invalid_text(lit, Some("Vector contents must start with \"[\"."), 194)`;
- `[]` gives `ereport(ERRCODE_DATA_EXCEPTION, "vector must have at least 1 dimension", 204, "vector_in")`;
- `dim == VECTOR_MAX_DIM` gives `ereport(ERRCODE_PROGRAM_LIMIT_EXCEEDED, format!("vector cannot have more than {VECTOR_MAX_DIM} dimensions"), 214, "vector_in")`;
- `pt >= n` gives `invalid_text(lit, None, 223)`;
- `strtof_prefix` None gives `invalid_text(lit, None, 234)`;
- the Erange arm keeps its message and gains `.with_location("vector.c", 240, "vector_in")` (or uses `ereport(ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE, ..., 240, "vector_in")`);
- the bad separator gives `invalid_text(lit, None, 260)`;
- the junk gives `invalid_text(lit, Some("Junk after closing right brace."), 271)`;
- the final checks become `check_dim(dim as i32)?; check_expected_dim(typmod, dim as i32)?;`.

Fix every caller of `check_dim`/`check_expected_dim` in `funcs.rs` to pass `i32` (`v.dim() as i32` and so on). The `check_dim(0)` stand-ins for negative values go away:
- **`fc_vector_recv`:** call `check_dim(dim as i32)` directly on the `i16` from the wire.
- **`fc_subvector`:**
  - `count < 1` raises `ereport(ERRCODE_DATA_EXCEPTION, "vector must have at least 1 dimension", 998, "subvector")`;
  - `start > a->dim` raises the same message at `1015`;
  - the final span check is `check_dim(dim)` on the signed span, which gives `CheckDim` 100 for a negative one.
  - Read vector.c:985-1020 and keep C's order of the checks, including its `end` arithmetic.
- **`fc_vector_typmod_in`:** the three messages get `.with_location("vector.c", 356 | 361 | 366, "vector_typmod_in")`.
- **`fc_vector_recv`:** move the body into `pub(crate) fn vector_recv_body<'m>(mcx: Mcx<'m>, buf: &mut StringInfo, typmod: i32) -> PgResult<...>`, shaped like `halfvec_recv_body` (`halfvec.rs`). `expected unused to be 0` gets `.with_location("vector.c", 393, "vector_recv")`, and `fc_vector_recv` calls the body.
- **`fc_array_to_vector`:** replace its private prologue with the shared `cast_array_elems(mcx, arr, &ArraySite { file: "vector.c", func: "array_to_vector", ndim_line: 459, nulls_line: 464 })` and `cast_elem_f32`, as `halfvec`'s array cast does. Its `unsupported array type` is `ereport(ERRCODE_DATA_EXCEPTION, "unsupported array type", 498, "array_to_vector")`. Delete the comment that explains the old duplication.
- **`StateArray::check`:** add `.with_location("vector.c", 169, "CheckStateArray")`, and add to its comment: `halfvec's check_state_array overrides this with halfvec.c:173.`

In `crates/contrib/pgvector/src/lib.rs`'s header, delete the `DIVERGENCE` paragraph about pre-M2 vector errors carrying crate-derived locations.

Rerun the Step 1 command. Expected: all pass. Then run `UT_KEEP` (it includes `vec::` and `funcs::`): all pass.

- [ ] **Step 3: The vector.c diff section**

In `scripts/pgvector/sql/error-locations.sql`:
- In the header's first sentence, change `in halfvec.c, halfutils.h, sparsevec.c and bitvec.c` to `in vector.c, halfvec.c, halfutils.h, sparsevec.c and bitvec.c`.
- Insert before `-- halfvec.c`:
  ```sql
  -- vector.c
  SELECT '[1,2]'::vector + '[3]';
  SELECT '[1,2,3]'::vector(2);
  SELECT '[1,2,3]'::vector(3)::vector(2);
  SELECT '{}'::real[]::vector;
  SELECT array_fill(0, ARRAY[16000])::vector || '[1]';
  SELECT '[NaN,1]'::vector;
  SELECT '[Infinity,1]'::vector;
  SELECT '{NaN}'::real[]::vector;
  SELECT vector_avg('{1,NaN}'::float8[]);
  SELECT vector_avg('{{2,2,4,6}}'::float8[]);
  SELECT vector_accum('{{1,2}}'::float8[], '[1]');
  SELECT vector_combine('{}'::float8[], '{1,2}'::float8[]);
  SELECT vector_combine('{1,1,2}'::float8[], '{1,1,2,3}'::float8[]);
  SELECT halfvec_combine('{1,1,2}'::float8[], '{1,1,2,3}'::float8[]);
  SELECT vector_combine(array_fill(1::float8, ARRAY[16002]), '{0}'::float8[]);
  SELECT avg(v) FROM (VALUES ('[1,2]'::vector), ('[1]')) t(v);
  SELECT '1,2,3'::vector;
  SELECT '[]'::vector;
  SELECT ('[' || array_to_string(array_fill(1, ARRAY[16001]), ',') || ']')::vector;
  SELECT '['::vector;
  SELECT '[hello,1]'::vector;
  SELECT '[4e38]'::vector;
  SELECT '[1,2,3'::vector;
  SELECT '[1,2,3]9'::vector;
  SELECT '[1,2,3]'::vector(3, 2);
  SELECT '[1,2,3]'::vector(0);
  SELECT '[1,2,3]'::vector(16001);
  SELECT '{{1}}'::real[]::vector;
  SELECT '{NULL}'::real[]::vector;
  SELECT subvector('[1,2,3,4,5]'::vector, 1, 0);
  SELECT subvector('[1,2,3,4,5]'::vector, 2147483647, 10);
  SELECT subvector('[1,2,3,4,5]'::vector, -2, 1);
  SELECT '[1,2]'::halfvec::vector(3);
  SELECT '{1:1}/2'::sparsevec::vector(3);
  SELECT '{1:1}/16001'::sparsevec::vector;
  SELECT l2_normalize('[3e38,3e38]'::vector);
  SELECT '[3e38]'::vector * '[10]';
  ```

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/sqldiff.sh ref --verbose scripts/pgvector/sql/error-locations.sql
grep -c '^LOCATION:' ~/.cache/pgrust/pgvector-work/sqldiff/ref/ref.out
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/error-locations.sql
cat ~/.cache/pgrust/pgvector-work/sqldiff/pgrust/diff.txt
```
Expected:
- `ref` identical (seeded C against C);
- `pgrust` identical;
- the LOCATION count is 49 + the number of vector.c statements that error on C (report it).

Before keeping each new statement, read its C LOCATION in `ref.out`:
- A statement that does not error on C, or errors somewhere other than the site its comment intends, gets fixed or dropped.
- A statement whose C output differs from pgrust in anything but a vector.c LOCATION gets dropped and recorded (the stop rule).
- The two float statements expect `float_overflow_error, float.c:90` (Task 5).

- [ ] **Step 4: Verify**

```bash
scripts/pgvector/run-diff.sh ref
scripts/pgvector/run-diff.sh pgrust
scripts/pgvector/run-regress.sh pgrust
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- diff 21/21 in both modes;
- regress 14/14;
- both lints pass;
- `UT_KEEP` all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/contrib/pgvector/src scripts/pgvector/sql/error-locations.sql
git commit -q -F - <<'EOF'
pgvector: vector.c error locations

Every SQL- and wire-reachable error of the vector type now carries
vector.c's file:line and function (CheckDims/CheckExpectedDim/CheckDim/
CheckElement/CheckStateArray, vector_in, vector_typmod_in, vector_recv,
array_to_vector, subvector), as M2 did for halfvec.c and sparsevec.c.
check_dim is signed like C's int, vector_recv's body is testable, and
array_to_vector shares cast_array_elems. error-locations.sql gains a
vector.c section; the pre-M2 DIVERGENCE note is gone.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 7: The pre-M4 repair

**Files:**
- Create: `crates/contrib/pgvector/src/repair_gen.rs` (test-only module)
- Modify: `crates/contrib/pgvector/src/lib.rs` (`#[cfg(test)] mod repair_gen;`)
- Create: `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` (generated)
- Create: `scripts/pgvector/run-repair-check.sh`, `scripts/pgvector/sql/repair-verify.sql`

**Interfaces:**
- Produces the repair file and the check script.
- Run the repair as a superuser in each database whose `vector` is at extversion `0.8.5`: `psql -d <db> -f crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql`, then `ALTER EXTENSION vector UPDATE;`.

The repair is one `DO $repair$ ... $repair$;` block:
- **Preconditions.** `vector` must be installed (else EXCEPTION). If `extversion <> '0.8.5'`, it raises a NOTICE (`vector is at <v>; nothing to repair`) and returns.
- **Search path.** It sets `search_path` to the extension's schema plus `pg_catalog` for the block, with `set_config(.., true)`, and restores it at the end.
- **The objects.** In upstream `sql/vector.sql`'s statement order, each non-COMMENT statement becomes a guarded block that creates the object and adds it to the extension only if it is missing.
- **Counts.** It keeps a count of created objects and ends with `RAISE NOTICE 'vector: created % missing objects; now run ALTER EXTENSION vector UPDATE', n;`.

It runs as one atomic statement, so any failure leaves the database unchanged.

The generator's mapping (statement → guard and actions). `<...>` are captured from the statement text, and `MODULE_PATHNAME` becomes `$libdir/vector`:

| statement | guard (object missing) | actions |
|---|---|---|
| `CREATE TYPE <t>;` (shell) | `to_regtype('<t>') IS NULL` | the statement |
| `CREATE TYPE <t> (` … | `NOT (SELECT typisdefined FROM pg_type WHERE oid = to_regtype('<t>'))` | the statement; `ALTER EXTENSION vector ADD TYPE <t>;` |
| `CREATE FUNCTION <f>(<args>)` … | `to_regprocedure('<f>(<args>)') IS NULL` | the statement; `ALTER EXTENSION vector ADD FUNCTION <f>(<args>);` |
| `CREATE AGGREGATE <f>(<args>)` … | `to_regprocedure('<f>(<args>)') IS NULL` | the statement; `ALTER EXTENSION vector ADD AGGREGATE <f>(<args>);` |
| `CREATE CAST (<a> AS <b>)` … | `NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('<a>') AND casttarget = to_regtype('<b>'))` | the statement; `ALTER EXTENSION vector ADD CAST (<a> AS <b>);` |
| `CREATE OPERATOR <op> (` … `LEFTARG = <l>`, `RIGHTARG = <r>` | `to_regoperator('<op>(<l>,<r>)') IS NULL` | the statement; `ALTER EXTENSION vector ADD OPERATOR <op> (<l>, <r>);` |
| `CREATE OPERATOR CLASS <c> [DEFAULT] FOR TYPE <t> USING <am>` … | `NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = '<c>' AND a.amname = '<am>' AND c.opcnamespace = v_nsp_oid)` | the statement; `ALTER EXTENSION vector ADD OPERATOR CLASS <c> USING <am>;`; `ALTER EXTENSION vector ADD OPERATOR FAMILY <c> USING <am>;` |
| `CREATE ACCESS METHOD <am> TYPE INDEX HANDLER <h>;` | `NOT EXISTS (SELECT 1 FROM pg_am WHERE amname = '<am>')` | the statement; `ALTER EXTENSION vector ADD ACCESS METHOD <am>;`; upstream's `COMMENT ON ACCESS METHOD <am> IS ...;`, which `vector--0.8.6--0.8.7.sql` does not re-apply |
| `COMMENT ON ...` | (skipped; the update script applies the rest) | — |
| anything else | the generator panics, so a new upstream statement kind fails the test | — |

Each guarded block reads `IF <guard> THEN <actions> n := n + 1; END IF;`. `v_nsp_oid` is the extension's `extnamespace`.

- [ ] **Step 1: The generator and its tests (test-only)**

Create `crates/contrib/pgvector/src/repair_gen.rs`:
```rust
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
        return Some((
            format!("NOT (SELECT typisdefined FROM pg_type WHERE oid = to_regtype('{name}'))"),
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
        return Some((
            format!("to_regoperator('{op}({l},{r})') IS NULL"),
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

const FOOTER: &str = "\
  PERFORM set_config('search_path', v_old_path, true);
  RAISE NOTICE 'vector: created % missing objects; now run ALTER EXTENSION vector UPDATE', n;
END
$repair$;
";

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
#[test]
fn repair_covers_every_upstream_object() {
    let stmts = statements(UPSTREAM);
    let objects = stmts.iter().filter(|s| !s.starts_with("COMMENT ON ")).count();
    assert_eq!(objects, 213, "upstream vector.sql 0.8.7 has 213 CREATE statements");
    let script = generate(UPSTREAM);
    assert_eq!(script.matches("  IF ").count(), objects);
    for s in stmts.iter().filter(|s| !s.starts_with("COMMENT ON ")) {
        let indented: String = s.lines().map(|l| format!("    {l}\n")).collect();
        assert!(script.contains(&indented), "missing verbatim: {s}");
    }
}
```
In `crates/contrib/pgvector/src/lib.rs`, add `#[cfg(test)]\nmod repair_gen;` next to the other module declarations.

Run:
```bash
mkdir -p crates/contrib/pgvector/repair
PGVECTOR_REGEN_REPAIR=1 cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_
cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_
```
Expected: 2 passed both times. The second run, without the env var, proves the committed file equals the generator's output. Read the generated file, and fix the generator before going on in any of these cases:
- the `CREATE OPERATOR` argument parse caught a commutator or negator as an arg;
- a `CREATE TYPE` shell detection misfired;
- a guard names a type with quotes or a schema.

Name each fix in the report.

`include_str!` of the reference tree reads, never writes, so the reference tree stays clean. If that path doesn't resolve from the crate, use the path `shipped_scripts_are_upstream_verbatim` uses (`concat!(env!("CARGO_MANIFEST_DIR"), "/../../pgvector-0.8.7-reference/sql/vector.sql")` with `std::fs::read_to_string`).

- [ ] **Step 2: The verification SQL**

Create `scripts/pgvector/sql/repair-verify.sql`, one sorted row per catalog fact, no OIDs:
```sql
\pset format unaligned
\pset tuples_only on
WITH ext AS (SELECT oid FROM pg_extension WHERE extname = 'vector'),
m AS (SELECT d.classid, d.objid, d.objsubid FROM pg_depend d, ext WHERE d.refclassid = 'pg_extension'::regclass AND d.refobjid = ext.oid AND d.deptype = 'e')
SELECT 'member|' || pg_describe_object(classid, objid, objsubid) || '|' || coalesce(obj_description(objid, (SELECT relname FROM pg_class WHERE oid = classid)), '') FROM m
UNION ALL
SELECT 'funcdef|' || md5(pg_get_functiondef(objid)) || '|' || pg_describe_object(classid, objid, 0) FROM m JOIN pg_proc pr ON pr.oid = m.objid WHERE classid = 'pg_proc'::regclass AND pr.prokind = 'f'
UNION ALL
SELECT 'amop|' || opf.opfname || '|' || am.amname || '|' || ao.amopstrategy::text || '|' || ao.amoppurpose::text || '|' || ao.amopopr::regoperator::text || '|' || ao.amopsortfamily::text FROM pg_amop ao JOIN pg_opfamily opf ON opf.oid = ao.amopfamily JOIN pg_am am ON am.oid = opf.opfmethod WHERE opf.opfname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'amproc|' || opf.opfname || '|' || am.amname || '|' || ap.amprocnum::text || '|' || ap.amproc::regprocedure::text FROM pg_amproc ap JOIN pg_opfamily opf ON opf.oid = ap.amprocfamily JOIN pg_am am ON am.oid = opf.opfmethod WHERE opf.opfname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'opclass|' || oc.opcname || '|' || am.amname || '|' || oc.opcdefault::text || '|' || oc.opcintype::regtype::text || '|' || (SELECT opfname FROM pg_opfamily f WHERE f.oid = oc.opcfamily) FROM pg_opclass oc JOIN pg_am am ON am.oid = oc.opcmethod WHERE oc.opcname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'ext|' || extversion || '|' || extrelocatable::text FROM pg_extension WHERE extname = 'vector'
ORDER BY 1;
```
(The `amopsortfamily::text` and `amop` rows compare OIDs of `float_ops`, which are stable catalog OIDs. If they differ between databases, replace the column with `(SELECT opfname FROM pg_opfamily WHERE oid = ao.amopsortfamily)`.)

- [ ] **Step 3: `run-repair-check.sh`**

Create `scripts/pgvector/run-repair-check.sh` (pgrust only):
```bash
#!/usr/bin/env bash
# Pre-M4 repair check (M4 follow-ups, Task 7). pgrust builds before M4
# installed a trimmed vector 0.8.5; three generations shipped on main
# (pgrust v0.3 79ad992ede, M2 88f29c4ccf, M3 c1c5da11cc). For each: make a
# database at that 0.8.5 with a table and an hnsw index, prove ALTER
# EXTENSION vector UPDATE fails, run the repair twice (the second run
# creates nothing), update, and compare the catalog with a fresh CREATE
# EXTENSION vector (sql/repair-verify.sql); the data and the index survive
# and an ivfflat index builds. Once more with the extension in schema s, and
# once on a fresh 0.8.7 install (nothing to repair).
# pgrust only: a scratch server under $PGV_WORK/repair-check; the staged old
# script is removed on exit.
# usage: run-repair-check.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

repair="$PGV_REPO/crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql"
verify="$here/sql/repair-verify.sql"
staged="$(dirname "$PGRUST_BIN")/share/extension"
PGV_WORK="$PGV_WORK/repair-check"
export PGV_WORK
out="$PGV_WORK/out"
rm -rf "$out"
mkdir -p "$out"

cleanup() {
  rm -f "$staged/vector--0.8.5.sql"
  "$here/server.sh" stop pgrust >/dev/null 2>&1 || true
}
trap cleanup EXIT

q() { "$here/server.sh" psql pgrust "$@"; }

passed=0
total=0
result() {
  total=$((total + 1))
  if [ "$2" = ok ]; then passed=$((passed + 1)); fi
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/summary.tsv"
}

# A fresh 0.8.7 install's catalog. $1 database, $2 schema ("" = public).
fresh_catalog() {
  q -c "CREATE DATABASE $1" >/dev/null
  if [ -n "$2" ]; then q -d "$1" -c "CREATE SCHEMA $2" >/dev/null; fi
  q -d "$1" -c "CREATE EXTENSION vector${2:+ SCHEMA $2}" >/dev/null
  q -d "$1" -f "$verify" >"$out/$1.txt"
}

# $1 label, $2 revision, $3 database, $4 schema ("" = public), $5 reference catalog.
check() {
  local label="$1" rev="$2" db="$3" schema="$4" ref="$5"
  local log="$out/$3.log" path="public${4:+, $4}"
  git -C "$PGV_REPO" show "$rev:crates/contrib/pgvector/extension/vector--0.8.5.sql" >"$staged/vector--0.8.5.sql"
  if ! {
    q -c "CREATE DATABASE $db" &&
      { [ -z "$schema" ] || q -d "$db" -c "CREATE SCHEMA $schema"; } &&
      q -d "$db" -c "CREATE EXTENSION vector VERSION '0.8.5'${schema:+ SCHEMA $schema}" &&
      q -d "$db" -c "SET search_path = $path; CREATE TABLE public.t (id int, v vector(3)); INSERT INTO public.t SELECT i, ARRAY[i % 7, i % 11, i % 13] FROM generate_series(1, 500) i; CREATE INDEX t_hnsw ON public.t USING hnsw (v vector_l2_ops);"
  } >"$log" 2>&1; then
    result "$label" "FAIL (setup; $log)"
    return
  fi
  if q -d "$db" -c "ALTER EXTENSION vector UPDATE" >>"$log" 2>&1; then
    result "$label" "FAIL (UPDATE worked before the repair; $log)"
    return
  fi
  if ! q -d "$db" -f "$repair" >"$out/$db.repair1" 2>&1 ||
    ! grep -q 'created [1-9][0-9]* missing objects' "$out/$db.repair1"; then
    result "$label" "FAIL (repair; $out/$db.repair1)"
    return
  fi
  if ! q -d "$db" -f "$repair" >"$out/$db.repair2" 2>&1 ||
    ! grep -q 'created 0 missing objects' "$out/$db.repair2"; then
    result "$label" "FAIL (second repair not a no-op; $out/$db.repair2)"
    return
  fi
  if ! q -d "$db" -c "ALTER EXTENSION vector UPDATE" >>"$log" 2>&1; then
    result "$label" "FAIL (UPDATE after the repair; $log)"
    return
  fi
  q -d "$db" -f "$verify" >"$out/$db.txt"
  if ! diff "$ref" "$out/$db.txt" >"$out/$db.diff"; then
    result "$label" "FAIL (catalog differs from a fresh install; $out/$db.diff)"
    return
  fi
  if ! {
    [ "$(q -d "$db" -At -c "SELECT count(*) FROM public.t")" = 500 ] &&
      q -d "$db" -At -c "SET search_path = $path; SET enable_seqscan = off; EXPLAIN (COSTS OFF) SELECT id FROM public.t ORDER BY v <-> '[1,1,1]' LIMIT 3" | grep -q 'Index Scan using t_hnsw' &&
      [ "$(q -d "$db" -At -c "SET search_path = $path; SELECT count(*) FROM (SELECT id FROM public.t ORDER BY v <-> '[1,1,1]' LIMIT 3) s")" = 3 ] &&
      q -d "$db" -c "SET search_path = $path; CREATE INDEX t_ivf ON public.t USING ivfflat (v vector_l2_ops) WITH (lists = 4)"
  } >>"$log" 2>&1; then
    result "$label" "FAIL (data or indexes after the update; $log)"
    return
  fi
  result "$label" ok
}

"$here/server.sh" fresh pgrust >"$out/server.log" 2>&1
fresh_catalog fresh ""
fresh_catalog fresh_s s
for rev in 79ad992ede 88f29c4ccf c1c5da11cc; do
  check "$rev" "$rev" "g_$rev" "" "$out/fresh.txt"
done
check "79ad992ede/schema" 79ad992ede g_schema s "$out/fresh_s.txt"
if q -d fresh -f "$repair" 2>&1 | grep -q 'nothing to repair'; then
  result "0.8.7/no-op" ok
else
  result "0.8.7/no-op" "FAIL (repair on a 0.8.7 install did not say nothing to repair)"
fi
echo "$passed/$total passed"
[ "$passed" = "$total" ]
```
Every step's output stays in `$PGV_WORK/repair-check/out/`, so a failure names its log. `server.sh psql` passes `-d postgres` before the caller's arguments, and psql keeps the last `-d`; if that does not hold, call `"$PG_TOOLS/bin/psql"` with `pgv_psql`'s flags directly and name it in the report.

```bash
/bin/bash -n scripts/pgvector/run-repair-check.sh
chmod +x scripts/pgvector/run-repair-check.sh
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-repair-check.sh
ls target/fast-profile/share/extension/vector--0.8.5.sql 2>&1 | tail -1
```
Expected:
- `5/5 passed` (three generations, the schema case, the 0.8.7 no-op);
- the staged `vector--0.8.5.sql` is gone (`No such file or directory`).

If a generation fails, read its log:
- A guard or ADD form pgrust does not support, or a statement kind a DO block cannot run, is fixed in the generator, not by hand, and named in the report.
- A pgrust core gap (an unsupported `ALTER EXTENSION ADD` object kind, `to_regoperator` missing) is a plan-defined stop.

- [ ] **Step 4: Repair the user's dev cluster**

`~/.cache/pgrust/pgv-016/data/pgrust` holds database `postgres` at vector 0.8.5 (the v0.3 generation). Back it up, then repair it with a server on a free port:
```bash
cp -R ~/.cache/pgrust/pgv-016/data/pgrust ~/.cache/pgrust/pgv-016/data/pgrust.pre-repair
```
1. Start `target/fast-profile/postgres` on that directory, the way `server.sh start pgrust` starts its own (same environment and flags), on port 55495 with its own socket directory under `~/.cache/pgrust/pgv-016`. Read `server.sh` for the command.
2. For each database whose `vector` is at 0.8.5 (query `pg_database`, then `pg_extension` in each), run the repair file, then `ALTER EXTENSION vector UPDATE`, then `SELECT extversion`.
3. Stop the server.
4. Expected: every such database reports `0.8.7`. Record the databases and their NOTICE counts in the report.
5. Then delete `pgrust.pre-repair`. It stays only if something failed; in that case restore it with `rm -rf pgrust && mv pgrust.pre-repair pgrust` and report.

`~/.cache/pgrust/pgvector-work/data/pgrust` needs no repair (`contrib_regression` is at 0.8.7); confirm that with the same query if the server is started anyway, but do not change it.

- [ ] **Step 5: Verify and commit**

```bash
cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_ shipped_scripts lookup_cover fmgr_stubs
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git status --porcelain --ignored crates/pgvector-0.8.7-reference crates/postgres-18.6-reference
```
Expected:
- all pass;
- both lints pass;
- the reference trees print nothing.

`UT_KEEP`: all pass.
```bash
git add crates/contrib/pgvector/src/repair_gen.rs crates/contrib/pgvector/src/lib.rs crates/contrib/pgvector/repair scripts/pgvector/run-repair-check.sh scripts/pgvector/sql/repair-verify.sql
git commit -q -F - <<'EOF'
pgvector: repair script for databases made by pre-M4 pgrust builds

pgrust shipped a trimmed vector 0.8.5 install script (three generations
on main), so ALTER EXTENSION vector UPDATE fails on those databases.
repair/pgrust-trimmed-0.8.5.sql, generated from upstream vector.sql by a
test-only generator that keeps it in sync, creates exactly the missing
upstream objects as extension members, atomically and idempotently.
run-repair-check.sh proves it on all three generations (and in another
schema) against a fresh install.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 8: Docs, the full gate and the report

**Files:**
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m4-report.md`
- Create: `docs/superpowers/reports/pgvector-m4-followups-report.md`

- [ ] **Step 1: The spec**

- **§8.2 Exact differential row.** Where it lists the error sites it compares, add vector.c to the M2 list, and add: `, the core error sites pgvector SQL reaches (reloptions.c, guc.c, float.c, mcxt.c; scripts/pgvector/sql/core-error-locations.sql), and where a session first loads vector (scripts/pgvector/sql/library-load.sql)`. Remove the clause that says pre-M2 vector.c and core float.c are outside it.
- **§8.3.** Add bullets for:
  - `sql/core-error-locations.sql` and `sql/library-load.sql` (M4 follow-ups);
  - `run-repair-check.sh` (M4 follow-ups).

- [ ] **Step 2: `CLAUDE.md`**

In "Vector search (fork work)":
- Add a short paragraph after the crate bullets:

  > **Pre-M4 databases:** pgrust builds before M4 installed a trimmed `vector` 0.8.5, so `ALTER EXTENSION vector UPDATE` fails on their databases. To fix one, run `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` (generated; `PGVECTOR_REGEN_REPAIR=1` regenerates it) as a superuser in each affected database, then run `ALTER EXTENSION vector UPDATE`.

- Add to the harness command block:
  ```bash
  scripts/pgvector/run-repair-check.sh                           # the pre-M4 repair on all three trimmed 0.8.5 generations
  ```
- Where the text mentions the opt-in bytecmp cases or the diff rows, if it does, bring it up to date (`insert_grid` opt-in; `library_load`, `core_error_locations`).

- [ ] **Step 3: The M4 report**

In `docs/superpowers/reports/pgvector-m4-report.md`, add a short section `## Resolved after M4 (2026-10-13)` before "Deferred minors". It lists each item this plan closed, one line each, with a pointer to `pgvector-m4-followups-report.md`:
- the `insert_grid` flake;
- `bitmap_scan`;
- `_PG_init` load timing;
- the `reloptions.c:0`/`guc.c:0`/`float.c:0`/`mcxt.c` locations;
- `pgvector.c:0`;
- the `{hnsw.c` row;
- the M1–M3 upgrade gap, now repairable.

Do not rewrite the M4 numbers; the report is a snapshot.

- [ ] **Step 4: The full gate**

```bash
scripts/pgvector/build-reference.sh --verify
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-all.sh ref
scripts/pgvector/run-all.sh pgrust
scripts/pgvector/tests/harness_test.sh
scripts/pgvector/run-repair-check.sh
```
Then the unit tests:
- `UT_KEEP`;
- `cargo test --profile fast-profile --locked -p main_main -p adt_amutils -p amapi -p reloptions -p guc -p mcx -p adt_float -p types_error -p pgvector --lib -- amgetbitmap extension_handlers_load extension_am_properties validation_error_texts reloption_enum invalid_values_error real_and_enum reserved MarkGUCPrefixReserved oom_error_shape alloc_size_gate ceiling float_error_helpers float_input_errors rust_paths_map overrides_sorted parse_errors_carry vector_recv_errors state_array_location repair_`;
- both lints;
- `git status --porcelain --ignored` on both reference trees.

Expected:
- `ref` 130/130 (regress 14, TAP 48, bytecmp 28, iterscan 19, diff 21);
- `pgrust` 122/130, with the 8 TAP failures the parallel builds as in M4 (check each against its log);
- harness `all passed`;
- repair check 5/5;
- the unit tests and lints pass, and the reference trees print nothing.

If `ref` shows a bytecmp flake on a default case, rerun once and record both runs.

- [ ] **Step 5: `docs/superpowers/reports/pgvector-m4-followups-report.md`**

Use the M4 report's style, filled from the Step 4 runs:
1. **Header line:** the date, the last code commit, and the references.
2. **Summary:** totals by tier for C and pgrust, and what changed.
3. **Results:** the full table from `~/.cache/pgrust/pgvector-work/report-pgrust.md`.
4. **What was fixed,** one subsection per task. Each gives the C behavior, the change, and the test that pins it:
   - the `insert_grid` analysis (the two memory contexts) and Task 1's measurement of `insert_sparsevec_cosine`;
   - `bitmap_scan`;
   - library loading, with the `library-load.sql` sections and what C prints;
   - the error-location table: every site, its C `file:line, function`, and its unit test or diff statement;
   - vector.c;
   - the repair, with the three generations, the schema case and the dev cluster.
5. **Statements dropped from diff files** under the stop rule, with both outputs, or "none".
6. **Still open:**
   - lsyscache's name-matched AM arms (Decision 2);
   - other `unrecognized configuration parameter` constructions;
   - hand-written OOMs in other crates;
   - pgvector_hnsw's 13 unlocated internal guards (now `hnsw.c:0`);
   - the remaining source-map garbage keys (harmless);
   - the M4 report's deferred minors, by pointer.
7. **Coverage limits:**
   - macOS arm64 only;
   - wasm not built;
   - check hooks covered by unit tests only, where no pgvector GUC has one.

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m4-report.md docs/superpowers/reports/pgvector-m4-followups-report.md
git commit -q -F - <<'EOF'
docs(pgvector): M4 follow-ups report; spec, CLAUDE.md and the M4 report

Records the follow-up gate (C 130/130, pgrust 122/130), every C error
location fixed, the AM library-load points, the insert_grid divergence,
and the pre-M4 repair with how to run it.

Co-Authored-By: <the implementing model>
EOF
git log --oneline main..HEAD
```
Expected: 9 commits on `vector/m4-followups`: the plan and Tasks 1–8. Do not merge or push; the controller does that after the final review and the user's go-ahead.

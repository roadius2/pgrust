# pgvector Phase 1 · M4: IVFFlat (serial) and the verbatim 0.8.7 extension script Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port pgvector 0.8.7's IVFFlat access method (serial build), ship upstream's verbatim `vector--0.8.7.sql` and all upgrade scripts, and prove the result with five checks:
- all 14 regression files pass;
- every IVFFlat TAP test passes, except the three assertions per build-recall test that need a parallel build (M6);
- the lookup-coverage test passes against every shipped script;
- seeded IVFFlat builds of every opclass family the plan can make exact are byte-identical to C's;
- every SQL-reachable IVFFlat error site matches C, LOCATION included.

**Architecture:**
- **Three crates, mirroring HNSW (spec §4.4):**
  - `types_ivfflat` holds the `ivfflat.h` vocabulary: constants, the page layout, `IvfflatTypeInfo` and the scan state. relscan's `IndexScanOpaque` can then hold the scan state without a cycle.
  - `pgvector_ivfflat` holds `ivfutils.c`, `ivfinsert.c`, `ivfscan.c` and `ivfvacuum.c`, plus the heap tuplesort both build and scan use.
  - `pgvector_ivfflat_build` holds `ivfbuild.c` and `ivfkmeans.c`. It is dispatched from `catalog_index`, like `pgvector_hnsw_build`.
- **Core hooks are spec §4.5's table:** `IndexAmKind::Ivfflat`, dispatch arms, the planner, reloptions, GUCs and `seams_init`.
- **Two small extractions break dependency cycles:**
  - ANALYZE's block sampler moves into its own `sampling` crate (C's `utils/misc/sampling.c`).
  - The scan's tuplesort is held type-erased. `tuplesort` depends on relscan, which holds the scan state.
- **Support proc 5 resolves by its C symbol** (`pg_proc.prosrc`, `LANGUAGE c` only) to `ivfutils.c`'s three static type-info tables, the way M3 resolves HNSW's proc 3. A shared `pgvector::support_proc_symbol` adds GIN's `prolang` check, which HNSW's resolution also gains.
- **Extension script.** Once the AM works, the trimmed `vector--0.8.5.sql` is replaced by upstream's `sql/vector.sql`, verbatim, as `vector--0.8.7.sql`. All 41 upstream upgrade scripts ship with it, and `default_version` becomes `0.8.7`.
  - `pg_get_loaded_modules()` reports `vector`'s `0.8.7`.
  - `_PG_init`'s GUC-prefix reservations are ported.
- **A seeded oracle.** The seeded C reference gains `-DIVFFLAT_BENCH`, the only configuration in which C seeds IVFFlat builds (`SeedRandom(42)`, `ivfbuild.c:1075-1077`). `pgrust.ivfflat_build_seed` mirrors it, so the byte-identical tier covers IVFFlat.

**Decisions this plan makes (for review):**
1. **All 41 upstream upgrade scripts ship**, not only the six 0.8.x ones spec §4.6 names.
   - C's `DATA` installs all of them, so `pg_extension_update_paths` and `pg_available_extension_versions` then match C.
   - Every C symbol they reference is already in the install script.
2. **IVFFlat becomes a gated part of the byte-identical tier.** Spec §8.2 said it would be "attempted but nondeterministic".
   - The seeded oracle gains `-DIVFFLAT_BENCH`, and pgrust gains the opt-in `pgrust.ivfflat_build_seed`.
   - Eight L2 and Hamming cases run on designed data.
   - Cosine and inner product stay out: C's normalization sums are vectorized, and M2's ruling is not to emulate that. Task 8 updates the spec.
3. **Type info is resolved on every call, not cached in `rd_amcache`.** The handoff asked to consider caching it.
   - pgrust's `rd_amcache` is one typed field per access method in core `RelationData`, so caching would add core fields.
   - The saving is one `pg_proc` syscache probe per insert. Each insert already computes a distance to every list's center.
   - C also calls the support proc on every insert. The report records this as a performance item.
4. **`_PG_init`'s GUC-prefix reservations are ported for both `hnsw` and `ivfflat`** (`vector.c:59-65`), through `pgvector`'s `pg_init`. HNSW behavior changes too: `SET hnsw.x = ...` after the library loads now errors, as it does in C.
5. **The durable `ivfflat_errors` row covers pgvector's own error sites only.** Reloption and GUC errors stay in the regression files, because pgrust core reports their LOCATION as `reloptions.c:0` and `guc.c:0`. That is a new core follow-up, which needs your go-ahead.
6. **The block sampler moves into its own `sampling` crate** (C's `utils/misc/sampling.c`). This is a small core extraction; ANALYZE re-exports it unchanged.
7. **`pg_get_loaded_modules()` reads the version from a side registry in `dfmgr`.** A new `BuiltinLibraryEntry` field would touch 58 call sites.

**Tech Stack:** Rust (the pgrust workspace, `fast-profile`), bash 3.2 harness scripts, PostgreSQL 18.6 + pgvector 0.8.7 C reference builds (`pgvec`, `pgvec-seeded`), `pg_regress`, Perl TAP (`prove`), `crates/bin/pagemask`.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`: §4.4–§4.6, §5.4 (wording), §6 (IVFFlat), §8.2 (tiers), §9 M4 row. Inputs:
- `docs/HANDOFF_2026-10-07.md` §1, which lists what this plan must include;
- `docs/superpowers/reports/pgvector-m3-report.md`: "Remaining failures", "Known divergences left open", "Deferred minors";
- `~/.cache/pgrust/m3-sdd-ledger.md`: every M3 ruling and parked item.

Facts verified while writing this plan (2026-10-07, macOS arm64; "C" is the reference build unless stated):
- **C seeds IVFFlat only under `-DIVFFLAT_BENCH`.** A scratch build with `PG_CFLAGS="-DHNSW_MEMORY -DIVFFLAT_BENCH"` built the same 1,000-row table's index twice; the two files were identical after `pagemask generic`.
  - That build also prints `INFO:  <step>: N ms` for `sample rows`, `k-means`, `assign tuples`, `sort tuples` and `load tuples` during builds, and for `GetScanLists`/`GetScanItems` during scans.
  - Stock C draws from the backend's unseeded `pg_global_prng_state`, so stock builds are not reproducible.
- **Byte-identical case designs, built on that seeded C.** Data are well-separated groups of 4 integer points (`256 × grid + {0,1}²`). Every k-means comparison that matters then has a clear winner, and the stored values are computed by element-wise sums and one division, which C and pgrust round alike.

  | Case | Data | Lists | Blocks | What it exercises |
  |---|---|---|---|---|
  | `ivf_l2_groups` | 1,000 rows, 10 groups | 10 | 12 | k-means centers are exact half-integers, e.g. `(256.5, 0.5)` |
  | `ivf_big` | 20,000 rows, 10 groups | 10 | 92 | 10,000 reservoir samples, with group means like `(256.48819, 0.511819)` |
  | `ivf_halfvec_groups` | as `ivf_l2_groups`, `halfvec` | 10 | 12 | `1024.5` rounds to the half `1024` |
  | `ivf_bit_hamming` | `bit(64)` from md5 | 10 | 12 | integer distances, thresholded centers |
  | `ivf_insert` | 600 rows, then 400 inserts | 2 | 8 | lists start on blocks 2 and 4; inserts append blocks 6 and 7 and move the insert pages there |
  | `ivf_unlogged` | as `ivf_l2_groups`, unlogged | 10 | 12 + 12 | the init fork's random centers |
  | `ivf_vacuum` | 3,000 rows; delete a third; `VACUUM` | 10 | 22 | each list's insert page goes back to its first page |
  | `ivf_wide` | 50 rows of `vector(2000)` | 5 | 56 | one list item per page, one entry per page |
- **IVFFlat error sites (C, `\set VERBOSITY verbose`):**
  - `InitBuildState`:
    - `ivfbuild.c:358`: 0A000 `type not supported for ivfflat index` (a `varbit` column);
    - `:364`: 22023 `column does not have dimensions`;
    - `:369`: 54000 `column cannot have more than N dimensions for ivfflat index`, with N = 2000, 4000 or 64000;
    - `:384`: 22023 `dimensions must be greater than one for this opclass` (cosine and ip, which have a k-means norm proc).
  - `IvfflatCheckMemoryUsage`, `ivfutils.c:133`: 54000 `memory required is 8 MB, maintenance_work_mem is 1 MB` (`vector(2000)`, `lists = 1000`).
  - `IvfflatCheckExpectedDim`, `ivfutils.c:289`: 22000 `expected 3 dimensions, not 2`, from the scan, even on an empty index.
  - The NOTICE `ivfflat index created with little data` (DETAIL `This will cause low recall.`, HINT `Drop the index until the table has more data.`) is at `ComputeCenters, ivfbuild.c:477`.
  - Task 6's `ivfflat-errors.sql` gives 18 errors and 4 NOTICEs on C.
- **Core error locations differ.** pgrust reports `reloptions.c:0` and `guc.c:0` where C reports `parse_one_reloption, reloptions.c:1637` and `parse_and_validate_value, guc.c:3186`/`:3282` (measured with the `hnsw` equivalents on today's pgrust). That is a core gap, like M2's `float.c:0`, so the durable errors file leaves those statements to the regression files.
- **GUC prefixes.** After the library loads, C answers `SET ivfflat.nonexistent = 1` and `SET hnsw.nonexistent = 1` with 42602 `invalid configuration parameter name ...`, DETAIL `"ivfflat" is a reserved prefix.` (`_PG_init`, `vector.c:59-65`: `hnsw.c:111` and `ivfflat.c:58`). pgrust accepts both as placeholders, because `pgvector` registers no `pg_init`.
- **`pg_get_loaded_modules()`** on C: `vector | 0.8.7 | vector.dylib`. pgrust reports `PG_COMPAT_VERSION` (18.6) for every builtin library.
- **Upgrade scripts.** All 41 upstream upgrade scripts reference only C symbols the install script also references (104 distinct). The six 0.8.x scripts reference none. `module_pathname_symbols` finds 114 `CREATE FUNCTION ... AS 'MODULE_PATHNAME'` statements in upstream `sql/vector.sql`.
- **Crate graph (`cargo tree`):**
  - `tuplesort` depends on `types_relscan`, so the scan state cannot hold a `Tuplesort`.
  - `commands_analyze` depends on `indexam` and `pgvector_hnsw`, so `pgvector_ivfflat_build` cannot use ANALYZE's sampler where it lives today.
  - `tuplesort`, `tupdesc` and `backend_progress` depend on none of `indexam`, `amapi`, `planner`, `catalog_index` or `seams_init`.
- **Sampler seeding.** C seeds the reservoir with `(uint64) pg_prng_uint32(&pg_global_prng_state)` (`sampling.c:139`). pgrust's ANALYZE passes `next_u64()` (`analyze/src/lib.rs:1816`), which is unobservable there. IVFFlat must pass `next_u32() as u64`.
- **pgrust's tuplesort ports `sort_template.h`** with C's equal-key order as a stated parity requirement (`tuplesort/src/qsort.rs`). Its `putvalues`/`getvalues` form the same minimal tuples `puttupleslot` does.
- **`main_main/build.rs` copies but never deletes** staged extension files. Its determinism lint skips `build.rs` and `#[cfg(test)]` items.

## Global Constraints

- **Branch:** work on `vector/m4`, cut from `main`. Merge locally with a fast-forward when done, and push only when the user asks. Work in the main checkout, not a worktree, because the disk is about 92% full.
- **Commit trailer:** every commit message ends with a `Co-Authored-By:` trailer naming the model that wrote the commit, as in M1–M3: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. Never edit them and never build inside them. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md):
  - Keep C function names, and make comments cite C `file.c:line`. pgvector citations point into `crates/pgvector-0.8.7-reference/src/`.
  - Mark intentional differences `DIVERGENCE`.
- **Error locations:** every error that mirrors a C `ereport`/`elog` carries `.with_location("<file>", <line>, "<CFunction>")`.
  - `<line>` is the last line of the C call (the line holding its closing `);`).
  - The values in this plan's code are the verified ones; copy them exactly.
- **Extension SQL:**
  - Until Task 5, `crates/contrib/pgvector/extension/` is not touched.
  - From Task 5 on, it holds only upstream's files, byte for byte: `vector.control`, `vector--0.8.7.sql` (upstream `sql/vector.sql`) and the 41 `vector--A--B.sql` upgrade scripts. A unit test enforces this, and nothing ever edits them by hand.
- **Defaults stay stock C:** `pgrust.ivfflat_build_seed` is a placeholder option, read through `guc_seams::get_config_option_missing_ok`, as `pgrust.hnsw_build_seed` is. It is **never** registered in `guc_tables`. Unset, nothing is reseeded.
- **Never widen `docs/fuzzing/rulings.toml` without the user.** Upstream expected outputs are never edited.
- **No new external dependencies.** New path dependencies on workspace crates are fine. New workspace members: `types_ivfflat`, `pgvector_ivfflat`, `pgvector_ivfflat_build` and `sampling`.
- **Core edits** are spec §4.5's hook points plus five small ones this plan names:
  - the `sampling` extraction;
  - the `dfmgr` library-version registry and `pg_get_loaded_modules`;
  - `main_main/build.rs` pruning stale staged files;
  - `pgvector`'s `pg_init`;
  - `pgvector::support_proc_symbol`.

  The open core follow-ups need the user's go-ahead and stay out: `float.c:0`, `numeric_float4`, `pgvector.c:0`, the `"{hnsw.c"` row, and the new `guc.c:0`/`reloptions.c:0`.
- **Unit-test commands.** They reuse the `target/fast-profile` artifacts the server build produced. Listing `main_main` keeps feature resolution identical to the server build. Never run them with an empty filter.
  - `UT <filter...>` means (Task 1 drops `-p pgvector_ivfflat_build -p sampling`, which Task 2 creates):
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw -p types_ivfflat -p pgvector_ivfflat -p pgvector_ivfflat_build -p sampling --lib -- <filter...>
    ```
  - `UT_CORE <filter...>` means:
    ```bash
    cargo test --profile fast-profile --locked -p main_main -p guc_tables -p types_relscan -p adt_amutils -p amapi -p types_error -p dfmgr --lib -- <filter...>
    ```
  - Every new M4 unit test's name starts with `ivf_`, except the moved sampler tests and Task 5's script tests. `UT_M4` is every unit test M4 adds or changes:
    ```
    UT ivf_ block_sampler reservoir_ lookup_cover fmgr_stubs shipped_scripts
    UT_CORE ivf_ amsearcharray_is_btree_only handler_dispatch_is_the_closed_set extension_am_identified_by_handler_prosrc phasenames_match_c overrides_sorted_unique_and_non_default library_version table_counts_match_compiled_backend_shape
    ```
  - `UT_KEEP` is the M1–M3 tests these crates already have, M3's final-wave `fmgr_stubs` included; they must stay green:
    ```
    UT tmpctx:: chain_walk update_connection seed_random level_for_uniform candidate_heaps hnsw_get_max_level parse_build_seed vector_combine bitutils:: bitvec:: halfvec:: funcs:: vec:: parse_show_memory charge_norm_value type_info hnsw_check_dim sparsevec_check_value hnsw_norm_value halfutils:: sparsevec:: lookup_cover fmgr_stubs
    ```
- **Cargo.lock:** adding a path dependency or a crate changes `Cargo.lock`.
  - After any `Cargo.toml` edit, run `cargo update --workspace --offline` before the next `--locked` command.
  - Check that `git diff --stat Cargo.lock` touches only the edited packages, then commit `Cargo.lock` with the task.
- **Server build:** `scripts/pgvector/build-pgrust.sh` builds `target/fast-profile/postgres`, `pagemask` and `diffrunner`. Every conformance check needs it rebuilt after a Rust change.
- **Harness commands** (from the repo root):
  - Regression: `scripts/pgvector/run-regress.sh pgrust [test ...]` (no test: all 14). Results go to `~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv`, diffs to `regression.diffs`.
  - TAP: `scripts/pgvector/run-tap.sh pgrust <NNN_name.pl ...>`. Logs go to `~/.cache/pgrust/pgvector-work/tap/pgrust/prove/<name>.log`.
  - One SQL script on fresh C and subject servers: `scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file|->`.
  - Tiers: `run-bytecmp.sh`, `run-iterscan.sh`, `run-diff.sh`, each `{pgrust|ref}`. `run-all.sh {pgrust|ref}` runs all five tiers.
  - Ports: pgrust 55491, C reference 55492, Docker reference 55493, seeded C reference 55494.
  - **Evidence on disk:** read a tier's verdict from `~/.cache/pgrust/pgvector-work/<tier>/<mode>/summary.tsv` before a later run overwrites it. `run-tap.sh` wipes `tap/pgrust` on every invocation, and `sqldiff.sh` keeps only its last run's transcripts.
- **Test lists:**
  - `IVF_REGRESS` means `ivfflat_bit ivfflat_halfvec ivfflat_vector`.
  - `IVF_TAP` means `001_ivfflat_wal.pl 002_ivfflat_vacuum.pl 003_ivfflat_vector_build_recall.pl 004_ivfflat_vector_insert_recall.pl 005_ivfflat_query_recall.pl 006_ivfflat_lists.pl 007_ivfflat_inserts.pl 008_ivfflat_centers.pl 009_ivfflat_filtering.pl 032_ivfflat_halfvec_build_recall.pl 035_ivfflat_bit_build_recall.pl 036_ivfflat_bit_centers.pl 040_ivfflat_cost.pl 041_ivfflat_iterative_scan.pl 042_ivfflat_iterative_scan_recall.pl 048_ivfflat_vacuum_insert.pl`.
  - `KEEP_TAP` (the 27 that pass today; must stay green) means `010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 018_aggregates.pl 019_storage.pl 021_hnsw_bit_insert_recall.pl 022_hnsw_bit_vacuum_recall.pl 023_hnsw_bit_duplicates.pl 025_hnsw_halfvec_insert_recall.pl 026_hnsw_halfvec_vacuum_recall.pl 027_hnsw_halfvec_duplicates.pl 029_hnsw_sparsevec_insert_recall.pl 030_hnsw_sparsevec_vacuum_recall.pl 031_hnsw_sparsevec_duplicates.pl 033_comparison.pl 034_distance_functions.pl 037_inputs.pl 038_hnsw_sparsevec_vacuum_insert.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl`.
  - **Parallel-only assertions (M6):** `003` line 127 (×3 opclasses), `032` line 131 (×3) and `035` line 116 (×1): `like($stderr, qr/using \d+ parallel workers/)`. Every other assertion of those three tests must pass.
- **Harness scripts:** `#!/usr/bin/env bash`, `set -euo pipefail`, bash 3.2 compatible. That rules out associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, and bare `"${arr[@]}"` on a possibly empty array under `set -u` (use `${arr[@]+"${arr[@]}"}`).
- **Lints** (shell scripts, no build), from the repo root. Both must pass at the end of every task that touches Rust:
  - `bash crates/_support/seams_init/tests/lint-determinism.sh`
  - `bash crates/_support/seams_init/tests/lint-seam-installs.sh` (its 21 stale-allowlist WARNs predate M4)
- **wasm:** the wasm ledger (`wasm/wasm-crate-ledger.md`) is ratchet-only, so never add a row. `wasm/wasm-build.sh` needs `nightly-2026-07-17`, which isn't installed. It is not run in M4, and the report says so.
- **Disk:** about 155 GiB free at plan time. Before Task 1, if `df -h .` shows under 120 GiB free, delete `~/.cache/pgrust/pgvector-work/tap`. It holds old TAP logs and is regenerable.
- **The plan's Rust code** mirrors patterns that compile today. If a snippet doesn't compile as written:
  - make the smallest fix and name it in the task report;
  - never change behavior to make code compile.

## Review Focus

1. **Fewer distinct values than lists.** Duplicates and tiny tables (TAP `008`, `036`, every regression file's little-data case) drive k-means++ to all-zero weights. It then picks sample 0 again and again, and empty clusters get random centers (`ivfkmeans.c:78-84`, `:224-227`).
   - Build assignment, insert and scan must all break distance ties toward the first list, with a strict `<`, so equal vectors are stored in and probed from the same list.
   - Tests: `ivf_kmeans_fills_every_center_from_duplicates` (Task 2) and TAP `008`/`036` (Task 7).
2. **NULL, zero and wrong-dimension queries.**
   - A NULL query scores every list and item 0 (`ZeroDistance`), so a scan returns the first `probes` lists in pairing-heap order.
   - A zero query on a cosine index normalizes to zero and scores NaN, and the rows come back in the sort's tie order.
   - A wrong dimension raises 22000 at `ivfutils.c:289`, even on an empty index.
   - Test: `scripts/pgvector/sql/ivfflat-errors.sql` (Task 6), with an `EXPLAIN` that pins each NULL query to the index scan.
3. **Lists spanning several pages, and a full insert page.** Insert extends a list under the extension lock, and `IvfflatUpdateList` only moves an insert page forward past a concurrent vacuum (`ivfutils.c:256-265`). Vacuum resets it to the first page with room.
   - Tests: bytecmp `ivf_insert`, `ivf_vacuum` and `ivf_wide` (Task 6), and TAP `002`, `007` and `048` (Task 7).
4. **`maintenance_work_mem` arithmetic at its edges.** C checks its own size arithmetic (`VECTOR_ARRAY_SIZE`, the Elkan buffers), not real allocations.
   - The regression files' 1/5/6/29 MB cases must not error, and `lists = 1000` on `vector(2000)` at 1 MB must raise C's exact text.
   - Tests: `ivf_memory_usage_error_matches_c` and `ivf_vector_array_bounds_and_sizes` (Task 1), the regression files (Task 5), and `ivfflat-errors.sql` (Task 6).
5. **A support proc 5 that isn't pgvector's**, from a hand-written opclass or a non-C function whose body text names a pgvector symbol. C calls it and trusts the pointer it returns; pgrust must raise a clean error, never panic or guess a type.
   - Test: `ivf_type_info_resolves_only_pgvector_symbols` (Task 1).

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/_support/types/types_ivfflat/` | **new:** `ivfflat.h` constants, page layout, `IvfflatTypeInfo`, `ListInfo`, the scan state | 1 |
| `crates/_support/types/rel/src/reloptions.rs` | `IvfflatOptions`, `RdOptions::Ivfflat` | 1 |
| `crates/backend/utils/cache/relcache/src/initfile.rs` | the `Ivfflat` options codec (tag 12) | 1 |
| `crates/contrib/pgvector/src/lib.rs`, `Cargo.toml` | `support_proc_symbol`; Task 5: header, stubs, `pg_init`, the library version, script tests | 1, 5 |
| `crates/contrib/pgvector_hnsw/src/utils.rs` | `hnsw_get_type_info` uses `support_proc_symbol` | 1 |
| `crates/contrib/pgvector_ivfflat/` | **new:** `utils.rs` (ivfutils.c), `sort.rs` (heap tuplesort), `insert.rs`, `scan.rs`, `vacuum.rs`, `lib.rs` (ivfflat.c's validate and GUC storage) | 1, 3 |
| `crates/backend/utils/misc/sampling/` | **new:** `sampling.c` (moved from `commands_analyze`) | 2 |
| `crates/backend/commands/analyze/` | re-exports `sampling` | 2 |
| `crates/contrib/pgvector_ivfflat_build/` | **new:** `lib.rs` (ivfbuild.c), `kmeans.rs` (ivfkmeans.c) | 2 |
| `crates/backend/access/index/relscan/` | `IndexAmKind::Ivfflat`, `IndexScanOpaque::Ivfflat` | 3 |
| `crates/backend/utils/misc/guc_tables/src/{vars,tables,tests}.rs` | `ivfflat.probes`, `ivfflat.iterative_scan`, `ivfflat.max_probes` | 3 |
| `crates/_support/seams_init/` | `pgvector_ivfflat::init_seams()` | 3 |
| `amapi`, `indexam`, `catalog_index`, `planner` (`plancat.rs`, `selfuncs.rs`), `relcache_build`, `reloptions`, `lsyscache`, `adt_amutils`, `vacuumlazy`, `types_error` | spec §4.5's hook points | 4 |
| `crates/contrib/pgvector/extension/` | upstream's control file, `vector--0.8.7.sql` and 41 upgrade scripts | 5 |
| `crates/backend/main/main_main/build.rs` | prunes stale staged files | 5 |
| `crates/backend/utils/fmgr/dfmgr/src/lib.rs`, `crates/backend/commands/extension/src/funcs.rs` | the library-version registry; `pg_get_loaded_modules` | 5 |
| `scripts/pgvector/{common.sh,build-reference.sh,run-bytecmp.sh,run-diff.sh}`, `sql/ivfflat-errors.sql`, `tests/harness_test.sh` | the seeded oracle, IVFFlat bytecmp cases, the `ivfflat_errors` row | 6 |
| `scripts/pgvector/upstream-sql.sh` | **deleted** (moot once the script is verbatim) | 8 |
| spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m4-report.md` | docs and the M4 report | 8 |

## Execution notes

- **Task order.** Tasks run in order, 1 to 8.
  - Tasks 1–3 build the IVFFlat crates. Task 3 also adds the relscan variant its scan needs. Each task compiles and has unit tests, but nothing reaches IVFFlat from SQL until Task 5.
  - Task 4 wires every remaining hook point, and Task 5 ships the script. Task 5 is the first end-to-end run.
  - Task 6 (byte identity, errors) runs before Task 7 (TAP), so page-format bugs surface where they are cheapest to find.
- **Suggested implementers:**
  - Sonnet for Tasks 1–4: the code is spelled out.
  - Opus for Tasks 5–8: the first end-to-end run, the harness, the TAP gate and the report. Each of these may need to fix ported code.
  - Opus does every review and the final whole-branch review.
- **Plan-defined stops**, in addition to subagent-driven-development's four:
  - a bytecmp IVFFlat case that differs in anything but data a later task fixes (never move it to opt-in);
  - any pgrust core difference that would need a core edit outside this plan's list;
  - never widen `rulings.toml`.
- **Expected totals after Task 8:**
  - `run-all.sh ref`: 129/129 (regress 14, TAP 48, bytecmp 29, iterscan 19, diff 19).
  - `run-all.sh pgrust`: 121/129 (regress 14, TAP 40, bytecmp 29, iterscan 19, diff 19).
  - The 8 TAP failures left are all parallel builds (M6): `003`, `032` and `035` on their parallel lines only, and `012`, `020`, `024`, `028` and `045` as before.

---
### Task 1: `types_ivfflat`, `ivfutils.c` and type info resolved by symbol

**Files:**
- Create: `crates/_support/types/types_ivfflat/Cargo.toml`, `src/lib.rs`
- Create: `crates/contrib/pgvector_ivfflat/Cargo.toml`, `src/lib.rs`, `src/utils.rs`, `src/sort.rs`
- Modify: `Cargo.toml` (workspace members), `Cargo.lock`
- Modify: `crates/_support/types/rel/src/reloptions.rs` (`IvfflatOptions`, `RdOptions::Ivfflat`)
- Modify: `crates/backend/utils/cache/relcache/src/initfile.rs` (the options codec)
- Modify: `crates/contrib/pgvector/Cargo.toml` (+`syscache_seams`), `crates/contrib/pgvector/src/lib.rs` (`support_proc_symbol`)
- Modify: `crates/contrib/pgvector_hnsw/Cargo.toml` (−`syscache_seams`), `crates/contrib/pgvector_hnsw/src/utils.rs` (`hnsw_get_type_info`)

**Interfaces:**
- Produces, in `types_ivfflat`:
  - constants: `IVFFLAT_MAX_DIM`, `IVFFLAT_{DISTANCE,NORM,KMEANS_DISTANCE,KMEANS_NORM,TYPE_INFO}_PROC: u16`, `IVFFLAT_VERSION`, `IVFFLAT_MAGIC_NUMBER`, `IVFFLAT_PAGE_ID`, `IVFFLAT_{METAPAGE,HEAD}_BLKNO`, `IVFFLAT_{DEFAULT,MIN,MAX}_LISTS: i32`, `IVFFLAT_DEFAULT_PROBES`, `PROGRESS_IVFFLAT_PHASE_{KMEANS,ASSIGN,LOAD}: i64`, `IVFFLAT_ITERATIVE_SCAN_{OFF,RELAXED}: i32`, `PAGE_CONTENTS_OFFSET`, `IVFFLAT_PAGE_OPAQUE_SIZE`, `IVFFLAT_METAPAGE_SIZE`, `IVFFLAT_LIST_CENTER_OFFSET`, `FIRST_OFFSET_NUMBER`;
  - `pub const fn maxalign(n: usize) -> usize`, `pub const fn ivfflat_list_size(itemsize: usize) -> usize`;
  - `pub struct IvfflatMetaPage { magic_number: u32, version: u32, dimensions: u16, lists: u16 }` with `read(page: &[u8])` and `write(&self, page: &mut [u8])`;
  - `page_nextblkno`, `set_page_nextblkno`, `page_id`, `set_page_id` (page special space); `list_start_page`, `list_insert_page`, `set_list_start_page`, `set_list_insert_page`, `list_center` (list items);
  - `pub struct ListInfo { pub blkno: BlockNumber, pub offno: u16 }`;
  - `pub struct IvfflatTypeInfo { max_dimensions: i32, dimensions: PGFunction, normalize: Option<PGFunction>, item_size: fn(i32) -> usize, update_center: fn(&mut [u8], i32, &[f32]), sum_center: fn(&[u8], i32, &mut [f32]) -> PgResult<()> }`;
  - `pub struct IvfflatScanList { start_page, distance: f64 }`, `compare_lists`, `ScanListHeap`, `new_scan_list_heap()`;
  - `pub struct IvfflatScanOpaqueData` (fields below).
- Produces, in `pgvector_ivfflat::utils`:
  - `elog_error`;
  - `VectorArray<'m>` (`init`, `get`, `get_mut`, `datum`, `set`; pub `length`, `maxlen`, `dim`, `itemsize`);
  - `vector_array_size`, `varsize_any`, `datum_image`, `detoast_value`;
  - `ivfflat_get_lists`, `index_getprocinfo`, `ivfflat_optional_proc_info`, `index_collation`;
  - `ivfflat_norm_value`, `ivfflat_check_norm`, `ivfflat_norm_vectors`, `ivfflat_check_memory_usage`;
  - `buffer_page`, `page_max_offset`, `page_item`, `ivfflat_new_buffer`, `ivfflat_init_page`, `XlogPage<'s>`, `ivfflat_init_register_page`, `ivfflat_append_page`, `ivfflat_get_meta_page_info`, `ivfflat_update_list`;
  - `ivfflat_check_expected_dim`, `ivfflat_check_dim`;
  - `VECTOR_TYPE_INFO`, `HALFVEC_TYPE_INFO`, `BIT_TYPE_INFO`, `ivfflat_type_info_for_symbol`, `ivfflat_get_type_info`.
- Produces, in `pgvector_ivfflat::sort`: `HeapSort` (`begin`, `put`, `perform`, `next`, `reset`), `INT4_LESS_OPERATOR`, `FLOAT8_LESS_OPERATOR`.
- Produces `pgvector::support_proc_symbol(procid: Oid) -> PgResult<Option<String>>`.
- Produces `types_rel::reloptions::IvfflatOptions { pub lists: i32 }`, `RdOptions::Ivfflat`, `RdOptions::ivfflat(&self) -> Option<&IvfflatOptions>`.

- [ ] **Step 1: Branch and commit the plan**

```bash
git switch -c vector/m4 main
git add docs/superpowers/plans/2026-10-07-pgvector-m4-ivfflat.md
git commit -q -m "docs(pgvector): M4 plan (IVFFlat serial port, verbatim 0.8.7 script)" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
df -h . | tail -1
```
Expected: a new branch with one commit. If under 120 GiB is free, delete `~/.cache/pgrust/pgvector-work/tap` first.

- [ ] **Step 2: Create `types_ivfflat`**

`crates/_support/types/types_ivfflat/Cargo.toml`:
```toml
[package]
name = "types_ivfflat"
version = "0.1.0"
edition.workspace = true
publish.workspace = true

[dependencies]
types_core = { path = "../types_core" }
types_error = { path = "../types_error" }
types_fmgr = { path = "../fmgr" }
types_storage = { path = "../types_storage" }
pairingheap = { path = "../../../backend/lib/pairingheap" }
```

`crates/_support/types/types_ivfflat/src/lib.rs`:
```rust
//! ivfflat.h vocabulary (pgvector 0.8.7): constants, the page layout, the
//! type info an opclass's support proc 5 names, and the scan state. The scan
//! state lives here so relscan's IndexScanOpaque can hold it without a cycle
//! through the AM crates (types_hnsw's pattern).

use pairingheap::PairingHeap;
use types_core::{BlockNumber, Oid, BLCKSZ};
use types_error::PgResult;
use types_fmgr::{FmgrInfo, PGFunction};
use types_storage::buf::BufferAccessStrategy;

pub const IVFFLAT_MAX_DIM: i32 = 2000;

// Support functions (ivfflat.h:40-44).
pub const IVFFLAT_DISTANCE_PROC: u16 = 1;
pub const IVFFLAT_NORM_PROC: u16 = 2;
pub const IVFFLAT_KMEANS_DISTANCE_PROC: u16 = 3;
pub const IVFFLAT_KMEANS_NORM_PROC: u16 = 4;
pub const IVFFLAT_TYPE_INFO_PROC: u16 = 5;

pub const IVFFLAT_VERSION: u32 = 1;
pub const IVFFLAT_MAGIC_NUMBER: u32 = 0x14FF1A7;
pub const IVFFLAT_PAGE_ID: u16 = 0xFF84;

// Preserved page numbers (ivfflat.h:51-52).
pub const IVFFLAT_METAPAGE_BLKNO: BlockNumber = 0;
pub const IVFFLAT_HEAD_BLKNO: BlockNumber = 1;

// IVFFlat parameters (ivfflat.h:55-58).
pub const IVFFLAT_DEFAULT_LISTS: i32 = 100;
pub const IVFFLAT_MIN_LISTS: i32 = 1;
pub const IVFFLAT_MAX_LISTS: i32 = 32768;
pub const IVFFLAT_DEFAULT_PROBES: i32 = 1;

// Build phases (ivfflat.h:60-64); 1 is PROGRESS_CREATEIDX_SUBPHASE_INITIALIZE.
pub const PROGRESS_IVFFLAT_PHASE_KMEANS: i64 = 2;
pub const PROGRESS_IVFFLAT_PHASE_ASSIGN: i64 = 3;
pub const PROGRESS_IVFFLAT_PHASE_LOAD: i64 = 4;

// IvfflatIterativeScanMode (ivfflat.h:114-118).
pub const IVFFLAT_ITERATIVE_SCAN_OFF: i32 = 0;
pub const IVFFLAT_ITERATIVE_SCAN_RELAXED: i32 = 1;

// ---- page layout (ivfflat.h:252-276) ----

// PageGetContents: MAXALIGN(SizeOfPageHeaderData).
pub const PAGE_CONTENTS_OFFSET: usize = 24;
// sizeof(IvfflatPageOpaqueData), the page's special space.
pub const IVFFLAT_PAGE_OPAQUE_SIZE: usize = 8;
// sizeof(IvfflatMetaPageData).
pub const IVFFLAT_METAPAGE_SIZE: usize = 12;
// offsetof(IvfflatListData, center).
pub const IVFFLAT_LIST_CENTER_OFFSET: usize = 8;
// FirstOffsetNumber (off.h).
pub const FIRST_OFFSET_NUMBER: u16 = 1;

pub const fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

// IVFFLAT_LIST_SIZE (ivfflat.h:66).
pub const fn ivfflat_list_size(itemsize: usize) -> usize {
    IVFFLAT_LIST_CENTER_OFFSET + itemsize
}

// IvfflatMetaPageData (ivfflat.h:252-258), at PageGetContents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IvfflatMetaPage {
    pub magic_number: u32,
    pub version: u32,
    pub dimensions: u16,
    pub lists: u16,
}

impl IvfflatMetaPage {
    pub fn read(page: &[u8]) -> IvfflatMetaPage {
        let c = &page[PAGE_CONTENTS_OFFSET..PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE];
        IvfflatMetaPage {
            magic_number: u32::from_ne_bytes(c[0..4].try_into().unwrap()),
            version: u32::from_ne_bytes(c[4..8].try_into().unwrap()),
            dimensions: u16::from_ne_bytes(c[8..10].try_into().unwrap()),
            lists: u16::from_ne_bytes(c[10..12].try_into().unwrap()),
        }
    }

    pub fn write(&self, page: &mut [u8]) {
        let c = &mut page[PAGE_CONTENTS_OFFSET..PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE];
        c[0..4].copy_from_slice(&self.magic_number.to_ne_bytes());
        c[4..8].copy_from_slice(&self.version.to_ne_bytes());
        c[8..10].copy_from_slice(&self.dimensions.to_ne_bytes());
        c[10..12].copy_from_slice(&self.lists.to_ne_bytes());
    }
}

// IvfflatPageOpaqueData (ivfflat.h:262-267): nextblkno, unused, page_id, in
// the page's last 8 bytes.
pub fn page_nextblkno(page: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(page[BLCKSZ - 8..BLCKSZ - 4].try_into().unwrap())
}

pub fn set_page_nextblkno(page: &mut [u8], blkno: BlockNumber) {
    page[BLCKSZ - 8..BLCKSZ - 4].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn page_id(page: &[u8]) -> u16 {
    u16::from_ne_bytes(page[BLCKSZ - 2..BLCKSZ].try_into().unwrap())
}

pub fn set_page_id(page: &mut [u8], id: u16) {
    page[BLCKSZ - 2..BLCKSZ].copy_from_slice(&id.to_ne_bytes());
}

// IvfflatListData (ivfflat.h:271-276): startPage, insertPage, then the
// center, a varlena image of the indexed type.
pub fn list_start_page(item: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(item[0..4].try_into().unwrap())
}

pub fn list_insert_page(item: &[u8]) -> BlockNumber {
    u32::from_ne_bytes(item[4..8].try_into().unwrap())
}

pub fn set_list_start_page(item: &mut [u8], blkno: BlockNumber) {
    item[0..4].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn set_list_insert_page(item: &mut [u8], blkno: BlockNumber) {
    item[4..8].copy_from_slice(&blkno.to_ne_bytes());
}

pub fn list_center(item: &[u8]) -> &[u8] {
    &item[IVFFLAT_LIST_CENTER_OFFSET..]
}

// ListInfo (ivfflat.h:131-135): where a list item lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListInfo {
    pub blkno: BlockNumber,
    pub offno: u16,
}

// IvfflatTypeInfo (ivfflat.h:188-196): what an opclass's support proc 5
// returns. `normalize` is None for bit (C's NULL). The center functions work
// on a VectorArray slot (MAXALIGNed, at least item_size(dimensions) bytes)
// and a float accumulator of `dimensions` entries; sum_center raises C's
// dimension mismatch.
pub struct IvfflatTypeInfo {
    pub max_dimensions: i32,
    pub dimensions: PGFunction,
    pub normalize: Option<PGFunction>,
    pub item_size: fn(i32) -> usize,
    pub update_center: fn(&mut [u8], i32, &[f32]),
    pub sum_center: fn(&[u8], i32, &mut [f32]) -> PgResult<()>,
}

// IvfflatScanList (ivfflat.h:280-285), without the embedded heap node.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IvfflatScanList {
    pub start_page: BlockNumber,
    pub distance: f64,
}

// CompareLists (ivfscan.c:32-42): pairingheap_first is the furthest list.
pub fn compare_lists(a: &IvfflatScanList, b: &IvfflatScanList) -> i32 {
    if a.distance > b.distance {
        1
    } else if a.distance < b.distance {
        -1
    } else {
        0
    }
}

pub type ScanListHeap =
    PairingHeap<IvfflatScanList, fn(&IvfflatScanList, &IvfflatScanList) -> i32>;

pub fn new_scan_list_heap() -> ScanListHeap {
    ScanListHeap::new(compare_lists)
}

// C IvfflatScanOpaqueData (ivfflat.h:287-315).
pub struct IvfflatScanOpaqueData {
    pub type_info: &'static IvfflatTypeInfo,
    pub probes: i32,
    pub max_probes: i32,
    pub dimensions: i32,
    pub first: bool,
    // so->value: the detoasted (and, with a norm proc, normalized) query
    // image; None for a NULL query (C's PointerGetDatum(NULL)).
    pub value: Option<Vec<u8>>,
    // so->distfunc == ZeroDistance (ivfscan.c:192-196): a NULL query.
    pub zero_distance: bool,
    // Support functions
    pub procinfo: FmgrInfo,
    pub normprocinfo: Option<FmgrInfo>,
    pub collation: Oid,
    pub bas: BufferAccessStrategy,
    // Lists
    pub list_queue: ScanListHeap,
    pub list_pages: Vec<BlockNumber>,
    pub list_index: i32,
    // so->sortstate, with its tuple descriptor and memory context
    // (pgvector_ivfflat::sort::HeapSort). Type-erased: tuplesort depends on
    // relscan, which holds this struct.
    pub sortstate: Option<Box<dyn core::any::Any>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use types_core::InvalidBlockNumber;

    // IvfflatMetaPageData at PageGetContents (ivfflat.h:252-258); the bytes
    // are block 0 of an index built by the seeded C reference.
    #[test]
    fn ivf_metapage_round_trips_c_layout() {
        let mut page = vec![0u8; BLCKSZ];
        let meta = IvfflatMetaPage {
            magic_number: IVFFLAT_MAGIC_NUMBER,
            version: IVFFLAT_VERSION,
            dimensions: 3,
            lists: 10,
        };
        meta.write(&mut page);
        if cfg!(target_endian = "little") {
            assert_eq!(&page[24..36], &[0xA7, 0xF1, 0x4F, 0x01, 1, 0, 0, 0, 3, 0, 10, 0]);
        }
        assert_eq!(IvfflatMetaPage::read(&page), meta);
    }

    // IvfflatPageOpaqueData in the special space, and IvfflatListData's
    // fields (ivfflat.h:262-276).
    #[test]
    fn ivf_list_item_and_opaque_layout_match_c() {
        let mut page = vec![0u8; BLCKSZ];
        set_page_nextblkno(&mut page, 7);
        set_page_id(&mut page, IVFFLAT_PAGE_ID);
        if cfg!(target_endian = "little") {
            assert_eq!(&page[BLCKSZ - 8..], &[7, 0, 0, 0, 0, 0, 0x84, 0xFF]);
        }
        assert_eq!((page_nextblkno(&page), page_id(&page)), (7, IVFFLAT_PAGE_ID));
        let mut item = vec![0u8; ivfflat_list_size(maxalign(20))];
        set_list_start_page(&mut item, 2);
        set_list_insert_page(&mut item, InvalidBlockNumber);
        assert_eq!((list_start_page(&item), list_insert_page(&item)), (2, InvalidBlockNumber));
        assert_eq!(item.len(), 32);
        assert_eq!(list_center(&item).len(), 24);
    }

    // CompareLists (ivfscan.c:32-42): the heap pops the furthest list first.
    #[test]
    fn ivf_compare_lists_pops_furthest_first() {
        let mut h = new_scan_list_heap();
        for (p, d) in [(1, 2.0), (2, 5.0), (3, 1.0)] {
            h.add(IvfflatScanList { start_page: p, distance: d });
        }
        assert_eq!(h.first().unwrap().start_page, 2);
        let order: Vec<BlockNumber> =
            core::iter::from_fn(|| h.remove_first().map(|l| l.start_page)).collect();
        assert_eq!(order, [2, 1, 3]);
    }
}
```

- [ ] **Step 3: `IvfflatOptions` and its relcache codec**

In `crates/_support/types/rel/src/reloptions.rs`:
- After the `HnswOptions` struct, add:
  ```rust
  // IvfflatOptions (pgvector ivfflat.h:138-142).
  #[derive(Clone, Copy, Debug, Eq, PartialEq)]
  pub struct IvfflatOptions {
      pub lists: i32,
  }
  ```
- In `pub enum RdOptions`, add `Ivfflat(IvfflatOptions),` after `Hnsw(HnswOptions),`.
- In `fillfactor`, replace `| RdOptions::Pgrcolumnar(_) | RdOptions::Hnsw(_) | RdOptions::Bloom(_) => None,` with `| RdOptions::Pgrcolumnar(_) | RdOptions::Hnsw(_) | RdOptions::Ivfflat(_) | RdOptions::Bloom(_) => None,`.
- After the `hnsw()` accessor, add:
  ```rust
      #[inline]
      pub fn ivfflat(&self) -> Option<&IvfflatOptions> {
          match self {
              RdOptions::Ivfflat(o) => Some(o),
              _ => None,
          }
      }
  ```

In `crates/backend/utils/cache/relcache/src/initfile.rs`:
- In `put_options`, after the `Some(RdOptions::Hnsw(o)) => { ... }` arm, add:
  ```rust
          Some(RdOptions::Ivfflat(o)) => {
              put_u8(buf, 12);
              put_i32(buf, o.lists);
          }
  ```
- In `parse_options`, after the `11 => { ... }` arm (before `_ => None,`), add:
  ```rust
          12 => Some(Some(RdOptions::Ivfflat(types_rel::reloptions::IvfflatOptions {
              lists: rd.i32()?,
          }))),
  ```

`RELCACHE_INIT_FORMAT` stays 2: no existing encoding changes, and an older binary rejects tag 12 as unparsable and rebuilds the file.

If `cargo check` reports another non-exhaustive `match` on `RdOptions`, add an `Ivfflat` arm mirroring that match's `Hnsw` arm, and name it in the task report.

- [ ] **Step 4: `pgvector::support_proc_symbol`, and HNSW uses it**

In `crates/contrib/pgvector/Cargo.toml`, add after the `numutils` line:
```toml
syscache_seams = { path = "../../backend/utils/cache/syscache_seams" }
```

In `crates/contrib/pgvector/src/lib.rs`, add after `const LIBRARY: &str = "vector";`:
```rust

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
```

In `crates/contrib/pgvector_hnsw/src/utils.rs`, replace the body of `hnsw_get_type_info` and its comment:
```rust
// HnswGetTypeInfo (hnswutils.c:1398-1416). The symbol is a LANGUAGE c proc's
// pg_proc.prosrc (pgvector::support_proc_symbol), what C's fmgr looks up in
// the library.
// DIVERGENCE: C calls any proc 3 and trusts the pointer it returns; pgrust
// knows only pgvector's three support functions and raises otherwise.
pub fn hnsw_get_type_info(index: &Relation<'_>) -> PgResult<&'static HnswTypeInfo> {
    let procid = index_getprocid(index, HNSW_TYPE_INFO_PROC);
    if procid == InvalidOid {
        return Ok(&VECTOR_TYPE_INFO);
    }
    let symbol = pgvector::support_proc_symbol(procid)?.unwrap_or_default();
    hnsw_type_info_for_symbol(&symbol).ok_or_else(|| {
        PgError::error(format!(
            "hnsw type support function \"{symbol}\" of index \"{}\" is not supported",
            index.name()
        ))
        .with_sqlstate(types_error::ERRCODE_FEATURE_NOT_SUPPORTED)
        .into()
    })
}
```
In `hnsw_type_info_for_symbol`'s comment, replace `the way amapi resolves hnswhandler (spec §5.4)` with `(spec §5.4)`. That closes M3's two `utils.rs` minors (the missing `prolang` check and the comment).

Remove the `syscache_seams = ...` line from `crates/contrib/pgvector_hnsw/Cargo.toml`; nothing else in that crate uses it (`grep -rn syscache_seams crates/contrib/pgvector_hnsw/src` must print nothing).

- [ ] **Step 5: Create `pgvector_ivfflat` with `sort.rs` and `utils.rs`**

`crates/contrib/pgvector_ivfflat/Cargo.toml`:
```toml
[package]
name = "pgvector_ivfflat"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

[lib]
name = "pgvector_ivfflat"
path = "src/lib.rs"

[dependencies]
mcx = { path = "../../_support/mcx" }
datum = { path = "../../_support/types/datum" }
types_core = { path = "../../_support/types/types_core" }
types_error = { path = "../../_support/types/types_error" }
types_fmgr = { path = "../../_support/types/fmgr" }
types_tuple = { path = "../../_support/types/types_tuple" }
types_rel = { path = "../../_support/types/rel" }
types_storage = { path = "../../_support/types/types_storage" }
types_ivfflat = { path = "../../_support/types/types_ivfflat" }
bufmgr = { path = "../../backend/storage/buffer/bufmgr" }
bufmgr_seams = { path = "../../backend/storage/buffer/bufmgr_seams" }
generic_xlog = { path = "../../backend/access/transam/generic_xlog" }
fmgr_seams = { path = "../../backend/utils/fmgr/fmgr_seams" }
init_small = { path = "../../backend/utils/init/init_small" }
detoast = { path = "../../backend/access/common/detoast" }
tupdesc = { path = "../../backend/access/common/tupdesc" }
tuplesort = { path = "../../backend/utils/sort/tuplesort" }
pgvector = { path = "../pgvector" }
adt_varbit = { path = "../../backend/utils/adt/varbit" }
```

`crates/contrib/pgvector_ivfflat/src/lib.rs` (Task 3 replaces it with the full crate root):
```rust
//! pgvector 0.8.7 ivfflat AM for vector, halfvec and bit (Task 3 adds
//! ivfinsert.c, ivfscan.c and ivfvacuum.c).

pub mod sort;
pub mod utils;
```

`crates/contrib/pgvector_ivfflat/src/sort.rs`:
```rust
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
```

`crates/contrib/pgvector_ivfflat/src/utils.rs`:
```rust
//! ivfutils.c (pgvector 0.8.7), with ivfflat.h's inline VectorArrayGet and
//! VectorArraySet: vector arrays, support procs, page and GenericXLog
//! helpers, dimension checks, and the type info an opclass's support proc 5
//! names.

use core::ptr::NonNull;

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_SHARE};
use datum::Datum;
use generic_xlog::{
    GenericXLogAbort, GenericXLogFinish, GenericXLogStart, GenericXLogState,
    GENERIC_XLOG_FULL_IMAGE,
};
use mcx::{Mcx, PgBox, PgVec};
use types_core::{BlockNumber, Buffer, ForkNumber, InvalidBlockNumber, InvalidOid, Oid, BLCKSZ};
use types_error::{PgError, PgResult};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_storage::bufpage::{PageMut, PageRef};
use types_storage::storage::ReadBufferMode;

// An elog(ERROR, ...) at its C call site (XX000).
#[cold]
pub fn elog_error(
    msg: impl Into<String>,
    file: &'static str,
    line: i32,
    func: &'static str,
) -> Box<PgError> {
    PgError::error(msg).with_location(file, line, func).into()
}

// ---- vector arrays (ivfutils.c:21-53, ivfflat.h:319-343) ----

// sizeof(VectorArrayData): int length, maxlen, dim; Size itemsize; char *items.
const VECTOR_ARRAY_DATA_SIZE: usize = 32;

// VECTOR_ARRAY_SIZE (ivfflat.h:319).
pub fn vector_array_size(length: i32, size: usize) -> PgResult<usize> {
    mcx::add_size(VECTOR_ARRAY_DATA_SIZE, mcx::mul_size(length as usize, maxalign(size))?)
}

// VectorArrayData (ivfflat.h:120-127): `maxlen` slots of `itemsize`
// (MAXALIGNed) bytes, zeroed; the first `length` hold varlena images.
pub struct VectorArray<'m> {
    pub length: i32,
    pub maxlen: i32,
    pub dim: i32,
    pub itemsize: usize,
    items: PgVec<'m, u8>,
}

impl<'m> VectorArray<'m> {
    // VectorArrayInit (ivfutils.c:24-43): palloc_extended with
    // MCXT_ALLOC_ZERO | MCXT_ALLOC_HUGE.
    pub fn init(mcx: Mcx<'m>, maxlen: i32, dimensions: i32, itemsize: usize) -> PgResult<VectorArray<'m>> {
        // Safety check
        if maxlen < 1 || dimensions < 1 || itemsize == 0 {
            return Err(elog_error("cannot create vector array", "ivfutils.c", 31, "VectorArrayInit"));
        }
        // Ensure items are aligned to prevent UB
        let itemsize = maxalign(itemsize);
        let len = mcx::mul_size(maxlen as usize, itemsize)?;
        let mut items: PgVec<'m, u8> = mcx::vec_with_capacity_huge_in(mcx, len)?;
        items.resize(len, 0);
        Ok(VectorArray { length: 0, maxlen, dim: dimensions, itemsize, items })
    }

    fn slot(&self, offset: i32) -> PgResult<usize> {
        // Safety check
        if offset < 0 || offset >= self.maxlen {
            return Err(elog_error("index out of bounds", "ivfflat.h", 328, "VectorArrayGet"));
        }
        Ok(offset as usize * self.itemsize)
    }

    // VectorArrayGet (ivfflat.h:323-331): the whole slot.
    pub fn get(&self, offset: i32) -> PgResult<&[u8]> {
        let start = self.slot(offset)?;
        Ok(&self.items[start..start + self.itemsize])
    }

    pub fn get_mut(&mut self, offset: i32) -> PgResult<&mut [u8]> {
        let start = self.slot(offset)?;
        Ok(&mut self.items[start..start + self.itemsize])
    }

    // PointerGetDatum(VectorArrayGet(arr, offset)).
    pub fn datum(&self, offset: i32) -> PgResult<Datum> {
        Ok(Datum::from_usize(self.get(offset)?.as_ptr() as usize))
    }

    // VectorArraySet (ivfflat.h:333-343): copies `val`'s varlena image.
    pub fn set(&mut self, offset: i32, val: &[u8]) -> PgResult<()> {
        let size = varsize_any(val);
        // Safety check
        if size > self.itemsize {
            return Err(elog_error("item too large", "ivfflat.h", 340, "VectorArraySet"));
        }
        self.get_mut(offset)?[..size].copy_from_slice(&val[..size]);
        Ok(())
    }
}

// VARSIZE_ANY of a varlena image.
pub fn varsize_any(val: &[u8]) -> usize {
    // SAFETY: `val` starts with a varlena header and holds the whole datum.
    unsafe { types_tuple::varatt::varsize_any(val.as_ptr()) }
}

// The varlena image a by-reference Datum points at.
pub fn datum_image<'a>(d: Datum) -> &'a [u8] {
    let p = d.as_usize() as *const u8;
    // SAFETY: a non-null varlena datum the caller keeps alive.
    unsafe { core::slice::from_raw_parts(p, types_tuple::varatt::varsize_any(p)) }
}

// PG_DETOAST_DATUM: a flat copy (4-byte header) of a non-null varlena datum.
pub fn detoast_value<'m>(mcx: Mcx<'m>, value: Datum) -> PgResult<PgVec<'m, u8>> {
    detoast::detoast_attr(mcx, datum_image(value))
}

// ---- support procs (ivfutils.c:55-120) ----

// IvfflatGetLists (ivfutils.c:58-67).
pub fn ivfflat_get_lists(index: &Relation<'_>) -> i32 {
    index
        .rd_options
        .as_ref()
        .and_then(|o| o.ivfflat())
        .map(|o| o.lists)
        .unwrap_or(IVFFLAT_DEFAULT_LISTS)
}

// index_getprocid(index, 1, procnum): ivfflat indexes have one column.
fn index_getprocid(index: &Relation<'_>, procnum: u16) -> Oid {
    index.rd_support.get(procnum as usize - 1).copied().unwrap_or(InvalidOid)
}

// index_getprocinfo(index, 1, procnum) (indexam.c:917-955). Not reachable
// from SQL: CREATE OPERATOR CLASS checks the procs an ivfflat opclass has.
pub fn index_getprocinfo(index: &Relation<'_>, procnum: u16) -> PgResult<FmgrInfo> {
    let procid = index_getprocid(index, procnum);
    if procid == InvalidOid {
        return Err(elog_error(
            format!(
                "missing support function {procnum} for attribute 1 of index \"{}\"",
                index.name()
            ),
            "indexam.c",
            947,
            "index_getprocinfo",
        ));
    }
    fmgr_seams::fmgr_info::call(procid)
}

// IvfflatOptionalProcInfo (ivfutils.c:72-79).
pub fn ivfflat_optional_proc_info(index: &Relation<'_>, procnum: u16) -> PgResult<Option<FmgrInfo>> {
    if index_getprocid(index, procnum) == InvalidOid {
        return Ok(None);
    }
    index_getprocinfo(index, procnum).map(Some)
}

// index->rd_indcollation[0].
pub fn index_collation(index: &Relation<'_>) -> Oid {
    index.rd_indcollation.first().copied().unwrap_or(InvalidOid)
}

// IvfflatNormValue (ivfutils.c:84-88): the type's normalize function on a
// detoasted image; the result is a fresh image in `mcx`.
// DIVERGENCE: C calls a NULL normalize (bit given a norm proc by a
// hand-written opclass) and crashes; pgrust raises.
pub fn ivfflat_norm_value<'m>(
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    mcx: Mcx<'m>,
    img: &[u8],
) -> PgResult<PgVec<'m, u8>> {
    let Some(normalize) = type_info.normalize else {
        return Err(PgError::error("ivfflat type info has no normalize function").into());
    };
    let d = types_fmgr::fcinfo::direct_function_call1_coll_in(
        normalize,
        collation,
        mcx,
        Datum::from_usize(img.as_ptr() as usize),
    )?;
    let out = datum_image(d);
    let mut v: PgVec<'m, u8> = mcx::vec_with_capacity_in_infallible(mcx, out.len());
    let _ = mcx::vec_append_bytes(&mut v, out);
    Ok(v)
}

// IvfflatCheckNorm (ivfutils.c:93-97): a non-zero norm.
pub fn ivfflat_check_norm(procinfo: &mut FmgrInfo, collation: Oid, value: Datum) -> PgResult<bool> {
    Ok(types_fmgr::function_call1_coll(procinfo, collation, value)?.as_f64() > 0.0)
}

// IvfflatNormVectors (ivfutils.c:102-120). C normalizes in tmpCtx and resets
// it after each vector; here each result lives in a per-vector bump context.
pub fn ivfflat_norm_vectors(
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    arr: &mut VectorArray<'_>,
) -> PgResult<()> {
    for i in 0..arr.length {
        let tmp = mcx::MemoryContext::new_bump("Ivfflat norm temporary context");
        let value = arr.get(i)?;
        let new_value = ivfflat_norm_value(type_info, collation, tmp.mcx(), &value[..varsize_any(value)])?;
        // Safety check
        ivfflat_check_dim(arr.dim, type_info, collation, Datum::from_usize(new_value.as_ptr() as usize))?;
        arr.set(i, &new_value)?;
    }
    Ok(())
}

// IvfflatCheckMemoryUsage (ivfutils.c:125-134).
pub fn ivfflat_check_memory_usage(total_size: usize) -> PgResult<()> {
    check_memory_usage(total_size, init_small::globals::maintenance_work_mem())
}

fn check_memory_usage(total_size: usize, maintenance_work_mem: i32) -> PgResult<()> {
    // Add one to error message to ceil
    if total_size / 1024 > maintenance_work_mem as usize {
        return Err(PgError::error(format!(
            "memory required is {} MB, maintenance_work_mem is {} MB",
            total_size / (1024 * 1024) + 1,
            maintenance_work_mem / 1024
        ))
        .with_sqlstate(types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED)
        .with_location("ivfutils.c", 133, "IvfflatCheckMemoryUsage")
        .into());
    }
    Ok(())
}

// ---- pages (ivfutils.c:136-281) ----

// BufferGetPage, under the caller's content lock.
pub fn buffer_page<'a>(buf: Buffer) -> &'a [u8] {
    // SAFETY: the caller holds a content lock on `buf`.
    unsafe { core::slice::from_raw_parts(bufmgr_seams::buffer_get_page::call(buf).as_ptr(), BLCKSZ) }
}

fn page_ref(page: &[u8]) -> PageRef<'_> {
    // SAFETY: `page` is a whole page image.
    unsafe { PageRef::from_raw(NonNull::new_unchecked(page.as_ptr() as *mut u8)) }
}

// PageGetMaxOffsetNumber.
pub fn page_max_offset(page: &[u8]) -> u16 {
    page_ref(page).max_offset_number()
}

// PageGetItem(page, PageGetItemId(page, offno)).
pub fn page_item(page: &[u8], offno: u16) -> &[u8] {
    let pr = page_ref(page);
    let (p, len) = pr.item_raw(pr.item_id(offno));
    // SAFETY: item_raw bounds the item within the page.
    unsafe { core::slice::from_raw_parts(p, len as usize) }
}

// IvfflatNewBuffer (ivfutils.c:139-146): ReadBufferExtended(P_NEW), then an
// exclusive lock. P_NEW skips the relation extension lock, as in C; insert
// takes that lock itself.
pub fn ivfflat_new_buffer(index: &Relation<'_>, fork_num: ForkNumber) -> PgResult<Buffer> {
    let buf = bufmgr::ReadBufferExtended(index, fork_num, InvalidBlockNumber, ReadBufferMode::Normal, None)?;
    LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
    Ok(buf)
}

// IvfflatInitPage (ivfutils.c:151-157), on a page image.
pub fn ivfflat_init_page(page: &mut [u8]) {
    {
        // SAFETY: `page` is a whole BLCKSZ image the caller owns.
        let mut pm = unsafe { PageMut::from_raw(NonNull::new_unchecked(page.as_mut_ptr())) };
        pm.init(IVFFLAT_PAGE_OPAQUE_SIZE);
    }
    set_page_nextblkno(page, InvalidBlockNumber);
    set_page_id(page, IVFFLAT_PAGE_ID);
}

// A buffer registered in a GenericXLog record: C's (Buffer, Page,
// GenericXLogState *) triple. `page` points at the registered image inside
// the boxed state, which stays put until GenericXLogFinish or Abort.
pub struct XlogPage<'s> {
    pub buf: Buffer,
    state: PgBox<'s, GenericXLogState>,
    page: *mut u8,
}

impl<'s> XlogPage<'s> {
    // GenericXLogStart, then GenericXLogRegisterBuffer.
    pub fn register(index: &Relation<'_>, buf: Buffer, flags: i32, mcx: Mcx<'s>) -> PgResult<XlogPage<'s>> {
        let mut state = GenericXLogStart(mcx, index)?;
        let page = state.register_buffer(buf, flags)?.as_mut_ptr();
        Ok(XlogPage { buf, state, page })
    }

    pub fn page(&self) -> &[u8] {
        // SAFETY: the registered image, a whole page.
        unsafe { core::slice::from_raw_parts(self.page, BLCKSZ) }
    }

    pub fn page_mut(&mut self) -> &mut [u8] {
        // SAFETY: the registered image, a whole page, exclusively ours.
        unsafe { core::slice::from_raw_parts_mut(self.page, BLCKSZ) }
    }

    fn page_mut_ref(&mut self) -> PageMut<'_> {
        // SAFETY: as page_mut.
        unsafe { PageMut::from_raw(NonNull::new_unchecked(self.page)) }
    }

    // PageGetFreeSpace.
    pub fn free_space(&self) -> usize {
        page_ref(self.page()).free_space()
    }

    // PageAddItem(page, item, size, InvalidOffsetNumber, false, false);
    // None is InvalidOffsetNumber.
    pub fn add_item(&mut self, item: &[u8]) -> Option<u16> {
        self.page_mut_ref().add_item(item, 0, 0)
    }

    // PageGetItem on the registered image, writable.
    pub fn item_mut(&mut self, offno: u16) -> &mut [u8] {
        let pr = page_ref(self.page());
        let (p, len) = pr.item_raw(pr.item_id(offno));
        // SAFETY: item_raw bounds the item within the registered image.
        unsafe { core::slice::from_raw_parts_mut(p as *mut u8, len as usize) }
    }

    // PageIndexMultiDelete.
    pub fn index_multi_delete(&mut self, offnos: &[u16]) {
        self.page_mut_ref().index_multi_delete(offnos);
    }

    // ((PageHeader) page)->pd_lower = pd_lower.
    pub fn set_pd_lower(&mut self, pd_lower: u16) {
        self.page_mut_ref().set_pd_lower(pd_lower);
    }

    // IvfflatCommitBuffer (ivfutils.c:173-178).
    pub fn commit(self) -> PgResult<()> {
        GenericXLogFinish(self.state)?;
        UnlockReleaseBuffer(self.buf)
    }

    // GenericXLogAbort, then UnlockReleaseBuffer.
    pub fn abort(self) -> PgResult<()> {
        GenericXLogAbort(self.state);
        UnlockReleaseBuffer(self.buf)
    }

    // The shared tail of IvfflatAppendPage (ivfutils.c:185-207) and
    // InsertTuple's new page (ivfinsert.c:143-171): register `newbuf` as a
    // full image in this record, link this page to it, initialize it,
    // commit, unlock this buffer, and start a new record on `newbuf`
    // registered with `next_flags`.
    pub fn link_new_page(
        mut self,
        index: &Relation<'_>,
        newbuf: Buffer,
        next_flags: i32,
        mcx: Mcx<'s>,
    ) -> PgResult<XlogPage<'s>> {
        let newpage = self.state.register_buffer(newbuf, GENERIC_XLOG_FULL_IMAGE)?.as_mut_ptr();
        set_page_nextblkno(self.page_mut(), bufmgr::BufferGetBlockNumber(newbuf));
        // SAFETY: newbuf's registered image, a whole page.
        ivfflat_init_page(unsafe { core::slice::from_raw_parts_mut(newpage, BLCKSZ) });
        GenericXLogFinish(self.state)?;
        UnlockReleaseBuffer(self.buf)?;
        XlogPage::register(index, newbuf, next_flags, mcx)
    }
}

// IvfflatInitRegisterPage (ivfutils.c:162-168).
pub fn ivfflat_init_register_page<'s>(index: &Relation<'_>, buf: Buffer, mcx: Mcx<'s>) -> PgResult<XlogPage<'s>> {
    let mut xp = XlogPage::register(index, buf, GENERIC_XLOG_FULL_IMAGE, mcx)?;
    ivfflat_init_page(xp.page_mut());
    Ok(xp)
}

// IvfflatAppendPage (ivfutils.c:185-207).
pub fn ivfflat_append_page<'s>(
    index: &Relation<'_>,
    xp: XlogPage<'s>,
    fork_num: ForkNumber,
    mcx: Mcx<'s>,
) -> PgResult<XlogPage<'s>> {
    // Get new buffer
    let newbuf = ivfflat_new_buffer(index, fork_num)?;
    xp.link_new_page(index, newbuf, GENERIC_XLOG_FULL_IMAGE, mcx)
}

// IvfflatGetMetaPageInfo (ivfutils.c:212-234): (lists, dimensions). C raises
// with the share lock held; this releases it first.
pub fn ivfflat_get_meta_page_info(index: &Relation<'_>) -> PgResult<(i32, i32)> {
    let buf = bufmgr::ReadBuffer(index, IVFFLAT_METAPAGE_BLKNO)?;
    LockBuffer(buf, BUFFER_LOCK_SHARE)?;
    let metap = IvfflatMetaPage::read(buffer_page(buf));
    UnlockReleaseBuffer(buf)?;
    if metap.magic_number != IVFFLAT_MAGIC_NUMBER {
        return Err(elog_error("ivfflat index is not valid", "ivfutils.c", 225, "IvfflatGetMetaPageInfo"));
    }
    Ok((metap.lists as i32, metap.dimensions as i32))
}

// IvfflatUpdateList (ivfutils.c:239-281): update the start or insert page of
// a list.
pub fn ivfflat_update_list(
    index: &Relation<'_>,
    list_info: ListInfo,
    insert_page: BlockNumber,
    original_insert_page: BlockNumber,
    start_page: BlockNumber,
    fork_num: ForkNumber,
) -> PgResult<()> {
    let buf = bufmgr::ReadBufferExtended(index, fork_num, list_info.blkno, ReadBufferMode::Normal, None)?;
    LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
    let xcx = mcx::MemoryContext::new("Ivfflat update list");
    let mut xp = XlogPage::register(index, buf, 0, xcx.mcx())?;
    let mut changed = false;
    {
        let list = xp.item_mut(list_info.offno);
        if insert_page != InvalidBlockNumber && insert_page != list_insert_page(list) {
            // Skip update if insert page is lower than original insert page
            // This is needed to prevent insert from overwriting vacuum
            if original_insert_page == InvalidBlockNumber || insert_page >= original_insert_page {
                set_list_insert_page(list, insert_page);
                changed = true;
            }
        }
        if start_page != InvalidBlockNumber && start_page != list_start_page(list) {
            set_list_start_page(list, start_page);
            changed = true;
        }
    }
    // Only commit if changed
    if changed {
        xp.commit()
    } else {
        xp.abort()
    }
}

// ---- dimension checks (ivfutils.c:283-301) ----

// IvfflatCheckExpectedDim (ivfutils.c:283-290).
pub fn ivfflat_check_expected_dim(expected: i32, dim: i32) -> PgResult<()> {
    if dim != expected {
        return Err(PgError::error(format!("expected {expected} dimensions, not {dim}"))
            .with_sqlstate(types_error::ERRCODE_DATA_EXCEPTION)
            .with_location("ivfutils.c", 289, "IvfflatCheckExpectedDim")
            .into());
    }
    Ok(())
}

// IvfflatCheckDim (ivfutils.c:295-301).
pub fn ivfflat_check_dim(
    expected: i32,
    type_info: &IvfflatTypeInfo,
    collation: Oid,
    value: Datum,
) -> PgResult<()> {
    let dim = types_fmgr::fcinfo::direct_function_call1_coll(type_info.dimensions, collation, value)?.as_i32();
    ivfflat_check_expected_dim(expected, dim)
}

// ---- type info (ivfutils.c:303-460) ----

// VectorItemSize, HalfvecItemSize and BitItemSize (ivfutils.c:309-325):
// VECTOR_SIZE, HALFVEC_SIZE and VARBITTOTALLEN.
fn vector_item_size(dimensions: i32) -> usize {
    8 + 4 * dimensions as usize
}

fn halfvec_item_size(dimensions: i32) -> usize {
    8 + 2 * dimensions as usize
}

fn bit_item_size(dimensions: i32) -> usize {
    8 + (dimensions as usize).div_ceil(8)
}

// SET_VARSIZE (4-byte header).
fn set_varsize(v: &mut [u8], len: usize) {
    v[0..4].copy_from_slice(&types_tuple::varatt::set_varsize_4b_word(len as u32).to_ne_bytes());
}

// VectorUpdateCenter (ivfutils.c:327-337). The `unused` field keeps what the
// slot held, as in C.
fn vector_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    set_varsize(v, vector_item_size(dimensions));
    v[4..6].copy_from_slice(&(dimensions as i16).to_ne_bytes());
    for i in 0..dimensions as usize {
        v[8 + 4 * i..12 + 4 * i].copy_from_slice(&x[i].to_ne_bytes());
    }
}

// HalfvecUpdateCenter (ivfutils.c:339-349).
fn halfvec_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    set_varsize(v, halfvec_item_size(dimensions));
    v[4..6].copy_from_slice(&(dimensions as i16).to_ne_bytes());
    for i in 0..dimensions as usize {
        let h = pgvector::halfutils::float4_to_half_unchecked(x[i]);
        v[8 + 2 * i..10 + 2 * i].copy_from_slice(&h.to_ne_bytes());
    }
}

// BitUpdateCenter (ivfutils.c:351-365): a bit is set when its mean exceeds
// 0.5, most significant bit first.
fn bit_update_center(v: &mut [u8], dimensions: i32, x: &[f32]) {
    let total = bit_item_size(dimensions);
    set_varsize(v, total);
    v[4..8].copy_from_slice(&dimensions.to_ne_bytes());
    let nx = &mut v[8..total];
    nx.fill(0);
    for i in 0..dimensions as usize {
        nx[i / 8] |= ((x[i] > 0.5) as u8) << (7 - (i % 8));
    }
}

// VectorSumCenter (ivfutils.c:367-378).
fn vector_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i16::from_ne_bytes([v[4], v[5]]) as i32)?;
    for i in 0..dimensions as usize {
        x[i] += f32::from_ne_bytes(v[8 + 4 * i..12 + 4 * i].try_into().unwrap());
    }
    Ok(())
}

// HalfvecSumCenter (ivfutils.c:380-391).
fn halfvec_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i16::from_ne_bytes([v[4], v[5]]) as i32)?;
    for i in 0..dimensions as usize {
        let h = u16::from_ne_bytes([v[8 + 2 * i], v[9 + 2 * i]]);
        x[i] += pgvector::halfutils::half_to_float4(h);
    }
    Ok(())
}

// BitSumCenter (ivfutils.c:393-403).
fn bit_sum_center(v: &[u8], dimensions: i32, x: &mut [f32]) -> PgResult<()> {
    // Safety check
    ivfflat_check_expected_dim(dimensions, i32::from_ne_bytes(v[4..8].try_into().unwrap()))?;
    for i in 0..dimensions as usize {
        x[i] += ((v[8 + i / 8] >> (7 - (i % 8))) & 0x01) as f32;
    }
    Ok(())
}

// IvfflatGetTypeInfo's default, no proc 5 (ivfutils.c:415-422): the vector
// opclasses.
pub static VECTOR_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM,
    dimensions: pgvector::funcs::fc_vector_dims,
    normalize: Some(pgvector::funcs::fc_l2_normalize),
    item_size: vector_item_size,
    update_center: vector_update_center,
    sum_center: vector_sum_center,
};

// ivfflat_halfvec_support (ivfutils.c:430-444).
pub static HALFVEC_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM * 2,
    dimensions: pgvector::halfvec::fc_halfvec_vector_dims,
    normalize: Some(pgvector::halfvec::fc_halfvec_l2_normalize),
    item_size: halfvec_item_size,
    update_center: halfvec_update_center,
    sum_center: halfvec_sum_center,
};

// ivfflat_bit_support (ivfutils.c:446-460): dimensions is varbit.c's
// bitlength.
pub static BIT_TYPE_INFO: IvfflatTypeInfo = IvfflatTypeInfo {
    max_dimensions: IVFFLAT_MAX_DIM * 32,
    dimensions: adt_varbit::fc_bitlength,
    normalize: None,
    item_size: bit_item_size,
    update_center: bit_update_center,
    sum_center: bit_sum_center,
};

// The type info a support proc 5 names, by its C symbol. C calls the proc and
// gets a pointer to its static IvfflatTypeInfo; pgrust maps the symbol to the
// same table (spec §6), so the ivfflat_*_support functions are never called
// through fmgr.
pub fn ivfflat_type_info_for_symbol(symbol: &str) -> Option<&'static IvfflatTypeInfo> {
    Some(match symbol {
        "ivfflat_halfvec_support" => &HALFVEC_TYPE_INFO,
        "ivfflat_bit_support" => &BIT_TYPE_INFO,
        _ => return None,
    })
}

// IvfflatGetTypeInfo (ivfutils.c:405-428). The symbol is a LANGUAGE c proc's
// pg_proc.prosrc (pgvector::support_proc_symbol).
// DIVERGENCE: C calls any proc 5 and trusts the pointer it returns; pgrust
// knows only pgvector's two support functions and raises otherwise.
pub fn ivfflat_get_type_info(index: &Relation<'_>) -> PgResult<&'static IvfflatTypeInfo> {
    let procid = index_getprocid(index, IVFFLAT_TYPE_INFO_PROC);
    if procid == InvalidOid {
        return Ok(&VECTOR_TYPE_INFO);
    }
    let symbol = pgvector::support_proc_symbol(procid)?.unwrap_or_default();
    ivfflat_type_info_for_symbol(&symbol).ok_or_else(|| {
        PgError::error(format!(
            "ivfflat type support function \"{symbol}\" of index \"{}\" is not supported",
            index.name()
        ))
        .with_sqlstate(types_error::ERRCODE_FEATURE_NOT_SUPPORTED)
        .into()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use pgvector::halfutils::{float4_to_half_unchecked, half_to_float4};
    use pgvector::halfvec::{HalfBuilder, HalfView};
    use pgvector::vec::{VecBuilder, VecView};

    // A bit(len) varlena image: 4-byte header, i32 bit length, the bytes.
    fn bit_image(bits: &[u8], len: i32) -> Vec<u8> {
        let mut img = vec![0u8; 8 + bits.len()];
        let hdr = ((img.len() as u32) << 2).to_ne_bytes();
        img[0..4].copy_from_slice(&hdr);
        img[4..8].copy_from_slice(&len.to_ne_bytes());
        img[8..].copy_from_slice(bits);
        img
    }

    // IvfflatGetTypeInfo's tables (ivfutils.c:405-460). Only pgvector's two
    // support symbols resolve (Review Focus 5).
    #[test]
    fn ivf_type_info_resolves_only_pgvector_symbols() {
        assert_eq!(VECTOR_TYPE_INFO.max_dimensions, 2000);
        assert!(VECTOR_TYPE_INFO.normalize.is_some());
        let h = ivfflat_type_info_for_symbol("ivfflat_halfvec_support").unwrap();
        assert!(core::ptr::eq(h, &HALFVEC_TYPE_INFO));
        assert_eq!(h.max_dimensions, 4000);
        assert!(h.normalize.is_some());
        let b = ivfflat_type_info_for_symbol("ivfflat_bit_support").unwrap();
        assert!(core::ptr::eq(b, &BIT_TYPE_INFO));
        assert_eq!(b.max_dimensions, 64000);
        assert!(b.normalize.is_none());
        for other in ["ivfflathandler", "hnsw_halfvec_support", "ivfflat_sparsevec_support", "textin", ""] {
            assert!(ivfflat_type_info_for_symbol(other).is_none(), "{other}");
        }
    }

    // VECTOR_SIZE, HALFVEC_SIZE, VARBITTOTALLEN, VECTOR_ARRAY_SIZE and
    // IVFFLAT_LIST_SIZE.
    #[test]
    fn ivf_item_sizes_match_c_macros() {
        assert_eq!((VECTOR_TYPE_INFO.item_size)(3), 20);
        assert_eq!((VECTOR_TYPE_INFO.item_size)(2000), 8008);
        assert_eq!((HALFVEC_TYPE_INFO.item_size)(3), 14);
        assert_eq!((BIT_TYPE_INFO.item_size)(64), 16);
        assert_eq!((BIT_TYPE_INFO.item_size)(65), 17);
        assert_eq!(vector_array_size(100, 8008).unwrap(), 32 + 100 * 8008);
        assert_eq!(vector_array_size(10, 20).unwrap(), 32 + 10 * 24);
        assert_eq!(maxalign(ivfflat_list_size(maxalign(20))), 32);
    }

    // The center images UpdateCenter writes (ivfutils.c:327-365).
    #[test]
    fn ivf_update_center_writes_c_images() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let xs = [1.5f32, -2.0, 0.25];
        let mut v = vec![0u8; 24];
        (VECTOR_TYPE_INFO.update_center)(&mut v, 3, &xs);
        let mut want = VecBuilder::new(m, 3).unwrap();
        for (i, x) in xs.iter().enumerate() {
            want.set(i, *x);
        }
        assert_eq!(&v[..20], &want.image()[..]);

        let hs = [1.0f32, 1024.5, 0.1];
        let mut h = vec![0u8; 16];
        (HALFVEC_TYPE_INFO.update_center)(&mut h, 3, &hs);
        let mut want = HalfBuilder::new(m, 3).unwrap();
        for (i, x) in hs.iter().enumerate() {
            want.set(i, float4_to_half_unchecked(*x));
        }
        assert_eq!(&h[..14], &want.image()[..]);
        // 1024.5 rounds to the half 1024 (round to nearest even).
        assert_eq!(half_to_float4(HalfView::from_payload(&h[4..14]).unwrap().x(1)), 1024.0);

        // 0.5 is not above 0.5; bits fill from the most significant end.
        let mut b = vec![0xFFu8; 16];
        (BIT_TYPE_INFO.update_center)(&mut b, 10, &[0.6, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 0.51, 1.0, 0.4]);
        assert_eq!(&b[..10], &bit_image(&[0b1001_0001, 0b1000_0000], 10)[..]);
    }

    // SumCenter (ivfutils.c:367-403) adds each element after checking the
    // dimensions (IvfflatCheckExpectedDim, ivfutils.c:289).
    #[test]
    fn ivf_sum_center_accumulates_and_checks_dims() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut v = VecBuilder::new(m, 2).unwrap();
        v.set(0, 1.5);
        v.set(1, -1.0);
        let img = v.image();
        let mut x = [0.25f32, 0.0];
        (VECTOR_TYPE_INFO.sum_center)(&img, 2, &mut x).unwrap();
        assert_eq!(x, [1.75, -1.0]);
        let e = (VECTOR_TYPE_INFO.sum_center)(&img, 3, &mut [0.0; 3]).unwrap_err();
        assert_eq!(e.message(), "expected 3 dimensions, not 2");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);

        let mut h = HalfBuilder::new(m, 2).unwrap();
        h.set(0, float4_to_half_unchecked(0.5));
        h.set(1, float4_to_half_unchecked(2.0));
        let mut x = [1.0f32, 1.0];
        (HALFVEC_TYPE_INFO.sum_center)(&h.image(), 2, &mut x).unwrap();
        assert_eq!(x, [1.5, 3.0]);

        let b = bit_image(&[0b1010_0000], 3);
        let mut x = [0.0f32; 3];
        (BIT_TYPE_INFO.sum_center)(&b, 3, &mut x).unwrap();
        assert_eq!(x, [1.0, 0.0, 1.0]);
        assert!((BIT_TYPE_INFO.sum_center)(&b, 4, &mut [0.0; 4]).is_err());
    }

    // VectorArrayInit, VectorArrayGet and VectorArraySet (ivfutils.c:24-43,
    // ivfflat.h:323-343).
    #[test]
    fn ivf_vector_array_bounds_and_sizes() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        assert_eq!(VectorArray::init(m, 0, 3, 20).err().unwrap().message(), "cannot create vector array");
        let mut arr = VectorArray::init(m, 2, 3, 20).unwrap();
        assert_eq!(arr.itemsize, 24);
        assert!(arr.get(1).unwrap().iter().all(|&b| b == 0));
        let mut v = VecBuilder::new(m, 3).unwrap();
        v.set(2, 7.0);
        let img = v.image();
        arr.set(1, &img).unwrap();
        assert_eq!(&arr.get(1).unwrap()[..20], &img[..]);
        assert_eq!(arr.get(2).err().unwrap().message(), "index out of bounds");
        assert_eq!(arr.get(-1).err().unwrap().message(), "index out of bounds");
        let big = VecBuilder::new(m, 5).unwrap().image();
        assert_eq!(arr.set(0, &big).err().unwrap().message(), "item too large");
    }

    // IvfflatCheckExpectedDim (ivfutils.c:283-290) through each type's
    // dimensions function.
    #[test]
    fn ivf_check_dim_matches_c() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let vimg = VecBuilder::new(m, 2).unwrap().image();
        let himg = HalfBuilder::new(m, 2).unwrap().image();
        let bimg = bit_image(&[0b1100_0000], 2);
        let typed: [(&IvfflatTypeInfo, &[u8]); 3] =
            [(&VECTOR_TYPE_INFO, &vimg[..]), (&HALFVEC_TYPE_INFO, &himg[..]), (&BIT_TYPE_INFO, &bimg[..])];
        for (info, img) in typed {
            let d = Datum::from_usize(img.as_ptr() as usize);
            assert!(ivfflat_check_dim(2, info, InvalidOid, d).is_ok());
            let e = ivfflat_check_dim(3, info, InvalidOid, d).unwrap_err();
            assert_eq!(e.message(), "expected 3 dimensions, not 2");
            assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);
        }
    }

    // IvfflatCheckMemoryUsage (ivfutils.c:125-134): kilobytes against
    // maintenance_work_mem, the message rounding up (Review Focus 4).
    #[test]
    fn ivf_memory_usage_error_matches_c() {
        assert!(check_memory_usage(1024 * 1024, 1024).is_ok());
        assert!(check_memory_usage(1024 * 1024 + 1023, 1024).is_ok());
        let e = check_memory_usage(1024 * 1024 + 1024, 1024).unwrap_err();
        assert_eq!(e.message(), "memory required is 2 MB, maintenance_work_mem is 1 MB");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_PROGRAM_LIMIT_EXCEEDED);
        // ivfflat-errors.sql: lists = 1000 on vector(2000) at 1 MB, as C.
        let e = check_memory_usage(vector_array_size(1000, 8008).unwrap(), 1024).unwrap_err();
        assert_eq!(e.message(), "memory required is 8 MB, maintenance_work_mem is 1 MB");
    }

    // IvfflatNormValue (ivfutils.c:84-88): each type's l2_normalize; bit has
    // none.
    #[test]
    fn ivf_norm_value_dispatches_by_type() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut v = VecBuilder::new(m, 2).unwrap();
        v.set(0, 3.0);
        v.set(1, 4.0);
        let out = ivfflat_norm_value(&VECTOR_TYPE_INFO, InvalidOid, m, &v.image()).unwrap();
        let view = VecView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (0.6, 0.8));

        let mut h = HalfBuilder::new(m, 2).unwrap();
        h.set(0, float4_to_half_unchecked(3.0));
        h.set(1, float4_to_half_unchecked(4.0));
        let out = ivfflat_norm_value(&HALFVEC_TYPE_INFO, InvalidOid, m, &h.image()).unwrap();
        let view = HalfView::from_payload(&out[4..]).unwrap();
        assert_eq!((view.x(0), view.x(1)), (float4_to_half_unchecked(0.6), float4_to_half_unchecked(0.8)));

        let e = ivfflat_norm_value(&BIT_TYPE_INFO, InvalidOid, m, &bit_image(&[0x80], 1)).unwrap_err();
        assert_eq!(e.message(), "ivfflat type info has no normalize function");
    }
}
```

- [ ] **Step 6: Add the workspace members and build**

In the root `Cargo.toml` `members` list, after `"crates/_support/types/types_hnsw",` add:
```toml
    "crates/_support/types/types_ivfflat",
    "crates/contrib/pgvector_ivfflat",
```
Then:
```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
cargo check --profile fast-profile --locked -p main_main -p types_ivfflat -p pgvector_ivfflat
```
Expected: `Cargo.lock` gains the two packages, and the dependency lists of `pgvector` (+`syscache_seams`) and `pgvector_hnsw` (−`syscache_seams`) change; nothing else changes. `cargo check` succeeds. `main_main` does not yet depend on `pgvector_ivfflat`, so that check covers the `RdOptions` changes and HNSW.

- [ ] **Step 7: Run the unit tests**

```bash
cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw -p types_ivfflat -p pgvector_ivfflat --lib -- ivf_
```
Expected: 11 passed (`types_ivfflat` 3, `pgvector_ivfflat` 8).

Then run `UT_KEEP` with the same reduced package list (Task 2 adds the other two packages). Expected: all pass, including HNSW's `type_info_resolves_only_pgvector_symbols`.

- [ ] **Step 8: HNSW still resolves its type info end to end**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust hnsw_vector hnsw_bit hnsw_halfvec hnsw_sparsevec
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/hnsw-errors.sql
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- regress 4/4: `hnsw_get_type_info` now goes through `support_proc_symbol`;
- sqldiff identical, 22 `ERROR` lines on each side;
- both lints PASS.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/_support/types/types_ivfflat crates/contrib/pgvector_ivfflat \
  crates/_support/types/rel/src/reloptions.rs crates/backend/utils/cache/relcache/src/initfile.rs \
  crates/contrib/pgvector/Cargo.toml crates/contrib/pgvector/src/lib.rs \
  crates/contrib/pgvector_hnsw/Cargo.toml crates/contrib/pgvector_hnsw/src/utils.rs
git commit -q -F - <<'EOF'
pgvector: types_ivfflat and ivfutils.c; type info resolved by C symbol

The ivfflat.h vocabulary (constants, page layout, type info, scan state)
in its own crate, and pgvector_ivfflat's utils (vector arrays, support
procs, GenericXLog page helpers, dimension and memory checks, the three
static type-info tables) and the heap tuplesort the build and scan share.
Support procs resolve through pgvector::support_proc_symbol, which adds
GIN's prolang check; HNSW's proc 3 uses it too.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 2: The `sampling` crate, `ivfkmeans.c` and the serial `ivfbuild.c`

**Files:**
- Create: `crates/backend/utils/misc/sampling/Cargo.toml`; move `crates/backend/commands/analyze/src/sampling.rs` to `crates/backend/utils/misc/sampling/src/lib.rs`
- Modify: `crates/backend/commands/analyze/Cargo.toml`, `crates/backend/commands/analyze/src/lib.rs` (re-export)
- Create: `crates/contrib/pgvector_ivfflat_build/Cargo.toml`, `src/lib.rs`, `src/kmeans.rs`
- Modify: `Cargo.toml` (workspace members), `Cargo.lock`

**Interfaces:**
- Consumes Task 1's `pgvector_ivfflat::utils::*`, `pgvector_ivfflat::sort::{HeapSort, INT4_LESS_OPERATOR}` and `types_ivfflat::*`.
- Produces `sampling` (package and crate): `BlockSamplerData`, `block_sampler_init`, `ReservoirStateData`, `reservoir_init_selection_state(seed: u64, n: u32)`, `reservoir_get_next_s`, `sampler_random_fract`. These are unchanged and still reachable as `commands_analyze::sampling::*`.
- Produces, in `pgvector_ivfflat_build`:
  - `pub struct IndexBuildResult { pub heap_tuples: f64, pub index_tuples: f64 }`;
  - `pub fn ivfflatbuild<'mcx>(mcx: Mcx<'mcx>, heap: &Relation<'mcx>, index: &Relation<'mcx>, index_info: &mut IndexInfo<'mcx>) -> PgResult<IndexBuildResult>`;
  - `pub fn ivfflatbuildempty(index: &Relation<'_>) -> PgResult<()>`.
- Produces, in `pgvector_ivfflat_build::kmeans`: `pub struct KmeansProcs { collation, distance: Option<FmgrInfo>, kmeans_norm: Option<FmgrInfo>, norm: Option<FmgrInfo> }`, `pub fn ivfflat_kmeans(...)`, and `pub fn kmeans(procs: &mut KmeansProcs, ...)`.

- [ ] **Step 1: Move the sampler into its own crate**

```bash
mkdir -p crates/backend/utils/misc/sampling/src
git mv crates/backend/commands/analyze/src/sampling.rs crates/backend/utils/misc/sampling/src/lib.rs
```
Prepend to `crates/backend/utils/misc/sampling/src/lib.rs`:
```rust
//! sampling.c (utils/misc): BlockSampler (Knuth's Algorithm S) and Vitter's
//! reservoir sampling. ANALYZE and pgvector's IVFFlat build share it; its own
//! crate keeps pgvector_ivfflat_build off commands_analyze, which sits above
//! indexam.

```
`crates/backend/utils/misc/sampling/Cargo.toml`:
```toml
[package]
name = "sampling"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

[dependencies]
pg_prng = { path = "../../../../common/pg_prng" }
types_core = { path = "../../../../_support/types/types_core" }
```
In `crates/backend/commands/analyze/src/lib.rs`, replace `pub mod sampling;` with `pub use ::sampling;`. In `crates/backend/commands/analyze/Cargo.toml` `[dependencies]`, add `sampling = { path = "../../utils/misc/sampling" }`. In the root `Cargo.toml` `members`, add `"crates/backend/utils/misc/sampling",` after `"crates/backend/commands/analyze",`.

ANALYZE keeps seeding its reservoir with `next_u64()` (`analyze/src/lib.rs:1816`) where C uses `pg_prng_uint32` (`sampling.c:139`). That is unobservable in ANALYZE, which no one seeds, and this task leaves it alone. The report records it.

- [ ] **Step 2: Create `pgvector_ivfflat_build`'s manifest and `kmeans.rs`**

`crates/contrib/pgvector_ivfflat_build/Cargo.toml`:
```toml
[package]
name = "pgvector_ivfflat_build"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

[lib]
name = "pgvector_ivfflat_build"
path = "src/lib.rs"

[dependencies]
mcx = { path = "../../_support/mcx" }
datum = { path = "../../_support/types/datum" }
types_core = { path = "../../_support/types/types_core" }
types_error = { path = "../../_support/types/types_error" }
types_fmgr = { path = "../../_support/types/fmgr" }
types_tuple = { path = "../../_support/types/types_tuple" }
types_rel = { path = "../../_support/types/rel" }
types_storage = { path = "../../_support/types/types_storage" }
types_ivfflat = { path = "../../_support/types/types_ivfflat" }
bufmgr = { path = "../../backend/storage/buffer/bufmgr" }
xloginsert = { path = "../../backend/access/transam/xloginsert" }
execindexing = { path = "../../backend/executor/execindexing" }
nbtree = { path = "../../backend/access/nbtree/nbtree" }
backend_progress = { path = "../../backend/utils/activity/backend_progress" }
init_small = { path = "../../backend/utils/init/init_small" }
elog = { path = "../../backend/utils/error/elog" }
postgres_seams = { path = "../../backend/tcop/postgres_seams" }
guc_seams = { path = "../../backend/utils/misc/guc_seams" }
pg_prng = { path = "../../common/pg_prng" }
sampling = { path = "../../backend/utils/misc/sampling" }
pgvector_ivfflat = { path = "../pgvector_ivfflat" }

[dev-dependencies]
pgvector = { path = "../pgvector" }
```

`crates/contrib/pgvector_ivfflat_build/src/kmeans.rs`:
```rust
//! ivfkmeans.c (pgvector 0.8.7): k-means++ initialization, Elkan's k-means
//! over the sampled vectors (spherical for inner product and cosine), then
//! the center checks.

use datum::Datum;
use pgvector_ivfflat::utils::{
    elog_error, index_collation, index_getprocinfo, ivfflat_check_memory_usage, ivfflat_norm_vectors,
    ivfflat_optional_proc_info, vector_array_size, VectorArray,
};
use types_core::Oid;
use types_error::PgResult;
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;

// The support procs ivfkmeans.c fetches from the index: the k-means distance
// (proc 3; InitCenters and ElkanKmeans, reached only when there are samples),
// the optional k-means norm (proc 4) and the optional norm (proc 2,
// CheckNorms). Fetched once up front instead of in each callee.
pub struct KmeansProcs {
    pub collation: Oid,
    pub distance: Option<FmgrInfo>,
    pub kmeans_norm: Option<FmgrInfo>,
    pub norm: Option<FmgrInfo>,
}

// RandomDouble() and RandomInt() (ivfflat.h:84-86): pg_global_prng_state.
fn random_double() -> f64 {
    pg_prng::global_prng(|p| p.next_f64())
}

fn random_int() -> u32 {
    pg_prng::global_prng(|p| p.next_u32())
}

// DatumGetFloat8(FunctionCall2Coll(procinfo, collation, a, b)).
fn distance(procinfo: &mut FmgrInfo, collation: Oid, a: Datum, b: Datum) -> PgResult<f64> {
    Ok(types_fmgr::function_call2_coll(procinfo, collation, a, b)?.as_f64())
}

// InitCenters (ivfkmeans.c:23-91): k-means++.
fn init_centers(
    procinfo: &mut FmgrInfo,
    collation: Oid,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    lower_bound: &mut [f32],
) -> PgResult<()> {
    let num_centers = centers.maxlen;
    let num_samples = samples.length;
    let nc = num_centers as usize;
    let mut weight = vec![f32::MAX; num_samples as usize];

    // Choose an initial center uniformly at random
    let first = (random_int() % samples.length as u32) as i32;
    centers.set(0, samples.get(first)?)?;
    centers.length += 1;

    for i in 0..num_centers {
        postgres_seams::check_for_interrupts::call()?;

        let mut sum = 0.0f64;
        for j in 0..num_samples {
            let vec = samples.datum(j)?;
            // Only need to compute distance for new center
            let mut d = distance(procinfo, collation, vec, centers.datum(i)?)?;
            // Set lower bound
            lower_bound[j as usize * nc + i as usize] = d as f32;
            // Use distance squared for weighted probability distribution
            d *= d;
            if d < weight[j as usize] as f64 {
                weight[j as usize] = d as f32;
            }
            sum += weight[j as usize] as f64;
        }

        // Only compute lower bound on last iteration
        if i + 1 == num_centers {
            break;
        }

        // Choose new center using weighted probability distribution.
        let mut choice = sum * random_double();
        let mut j = 0;
        while j < num_samples - 1 {
            choice -= weight[j as usize] as f64;
            if choice <= 0.0 {
                break;
            }
            j += 1;
        }

        centers.set(i + 1, samples.get(j)?)?;
        centers.length += 1;
    }
    Ok(())
}

// NormCenters (ivfkmeans.c:96-105).
fn norm_centers(type_info: &IvfflatTypeInfo, collation: Oid, centers: &mut VectorArray<'_>) -> PgResult<()> {
    ivfflat_norm_vectors(type_info, collation, centers)
}

// RandomCenters (ivfkmeans.c:110-133): quick approach if we have no data.
fn random_centers(procs: &KmeansProcs, centers: &mut VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let dimensions = centers.dim;
    let mut x = vec![0.0f32; dimensions as usize];

    // Fill with random data
    while centers.length < centers.maxlen {
        for xi in x.iter_mut() {
            *xi = random_double() as f32;
        }
        let center = centers.get_mut(centers.length)?;
        (type_info.update_center)(center, dimensions, &x);
        centers.length += 1;
    }

    if procs.kmeans_norm.is_some() {
        norm_centers(type_info, procs.collation, centers)?;
    }
    Ok(())
}

// SumCenters (ivfkmeans.c:151-160).
fn sum_centers(
    samples: &VectorArray<'_>,
    agg: &mut [f32],
    closest_centers: &[i32],
    type_info: &IvfflatTypeInfo,
) -> PgResult<()> {
    let dim = samples.dim as usize;
    for i in 0..samples.length {
        let c = closest_centers[i as usize] as usize;
        (type_info.sum_center)(samples.get(i)?, samples.dim, &mut agg[c * dim..(c + 1) * dim])?;
    }
    Ok(())
}

// UpdateCenters (ivfkmeans.c:165-174).
fn update_centers(agg: &[f32], centers: &mut VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let d = centers.dim;
    let dim = d as usize;
    for i in 0..centers.length {
        let x = &agg[i as usize * dim..(i as usize + 1) * dim];
        (type_info.update_center)(centers.get_mut(i)?, d, x);
    }
    Ok(())
}

// ComputeNewCenters (ivfkmeans.c:179-236).
#[allow(clippy::too_many_arguments)]
fn compute_new_centers(
    samples: &VectorArray<'_>,
    agg: &mut [f32],
    new_centers: &mut VectorArray<'_>,
    center_counts: &mut [i32],
    closest_centers: &[i32],
    kmeans_norm: bool,
    collation: Oid,
    type_info: &IvfflatTypeInfo,
) -> PgResult<()> {
    let dimensions = new_centers.dim as usize;
    let num_centers = new_centers.length as usize;

    // Reset sum and count
    for i in 0..num_centers {
        agg[i * dimensions..(i + 1) * dimensions].fill(0.0);
        center_counts[i] = 0;
    }

    // Increment sum of closest center
    sum_centers(samples, agg, closest_centers, type_info)?;

    // Increment count of closest center
    for i in 0..samples.length as usize {
        center_counts[closest_centers[i] as usize] += 1;
    }

    // Divide sum by count
    for i in 0..num_centers {
        let x = &mut agg[i * dimensions..(i + 1) * dimensions];
        if center_counts[i] > 0 {
            // Double avoids overflow, but requires more memory
            for xj in x.iter_mut() {
                if xj.is_infinite() {
                    *xj = if *xj > 0.0 { f32::MAX } else { -f32::MAX };
                }
            }
            for xj in x.iter_mut() {
                *xj /= center_counts[i] as f32;
            }
        } else {
            // TODO Handle empty centers properly
            for xj in x.iter_mut() {
                *xj = random_double() as f32;
            }
        }
    }

    // Set new centers
    update_centers(agg, new_centers, type_info)?;

    // Normalize if needed
    if kmeans_norm {
        norm_centers(type_info, collation, new_centers)?;
    }
    Ok(())
}

// ElkanKmeans (ivfkmeans.c:238-485). Elkan requires the distance to satisfy
// the triangle inequality: L2 distance for L2 (not L2 squared like the index
// scan), angular distance for inner product and cosine.
fn elkan_kmeans(
    procs: &mut KmeansProcs,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    let dimensions = centers.dim;
    let num_centers = centers.maxlen;
    let num_samples = samples.length;
    let nc = num_centers as usize;
    let ns = num_samples as usize;

    // Calculate allocation sizes
    let new_centers_size = vector_array_size(num_centers, centers.itemsize)?;
    let agg_size = mcx::mul_size(4, mcx::mul_size(nc, dimensions as usize)?)?;
    let center_counts_size = mcx::mul_size(4, nc)?;
    let closest_centers_size = mcx::mul_size(4, ns)?;
    let lower_bound_size = mcx::mul_size(4, mcx::mul_size(ns, nc)?)?;
    let upper_bound_size = mcx::mul_size(4, ns)?;
    let s_size = mcx::mul_size(4, nc)?;
    let halfcdist_size = mcx::mul_size(4, mcx::mul_size(nc, nc)?)?;
    let newcdist_size = mcx::mul_size(4, nc)?;

    // Calculate total size
    let mut total_size = memory_used;
    for size in [
        new_centers_size,
        agg_size,
        center_counts_size,
        closest_centers_size,
        lower_bound_size,
        upper_bound_size,
        s_size,
        halfcdist_size,
        newcdist_size,
    ] {
        total_size = mcx::add_size(total_size, size)?;
    }

    // Check memory requirements
    ivfflat_check_memory_usage(total_size)?;

    // Ensure indexing does not overflow
    if num_centers > i32::MAX / num_centers {
        return Err(elog_error("Indexing overflow detected. Please report a bug.", "ivfkmeans.c", 294, "ElkanKmeans"));
    }

    // Set support functions
    let collation = procs.collation;
    let kmeans_norm = procs.kmeans_norm.is_some();
    let procinfo = procs.distance.as_mut().expect("k-means distance proc, fetched with samples");

    // Allocate space
    // Use float instead of double to save memory
    let mut agg = vec![0.0f32; nc * dimensions as usize];
    let mut center_counts = vec![0i32; nc];
    let mut closest_centers = vec![0i32; ns];
    let mut lower_bound = vec![0.0f32; ns * nc];
    let mut upper_bound = vec![0.0f32; ns];
    let mut s = vec![0.0f32; nc];
    let mut halfcdist = vec![0.0f32; nc * nc];
    let mut newcdist = vec![0.0f32; nc];

    // Initialize new centers
    let new_centers_cx = mcx::MemoryContext::new("Ivfflat kmeans new centers");
    let mut new_centers = VectorArray::init(new_centers_cx.mcx(), num_centers, dimensions, centers.itemsize)?;
    new_centers.length = num_centers;

    // Pick initial centers
    init_centers(procinfo, collation, samples, centers, &mut lower_bound)?;

    // Assign each x to its closest initial center c(x) = argmin d(x,c)
    for j in 0..ns {
        let mut min_distance = f32::MAX;
        let mut closest_center = 0i32;

        // Find closest center
        for k in 0..nc {
            // TODO Use Lemma 1 in k-means++ initialization
            let d = lower_bound[j * nc + k];
            if d < min_distance {
                min_distance = d;
                closest_center = k as i32;
            }
        }

        upper_bound[j] = min_distance;
        closest_centers[j] = closest_center;
    }

    // Give 500 iterations to converge
    for iteration in 0..500 {
        let mut changes = 0;

        // Can take a while, so ensure we can interrupt
        postgres_seams::check_for_interrupts::call()?;

        // Step 1: For all centers, compute distance
        for j in 0..nc {
            let vec = centers.datum(j as i32)?;
            for k in j + 1..nc {
                let d = (0.5 * distance(procinfo, collation, vec, centers.datum(k as i32)?)?) as f32;
                halfcdist[j * nc + k] = d;
                halfcdist[k * nc + j] = d;
            }
        }

        // For all centers c, compute s(c)
        for j in 0..nc {
            let mut min_distance = f32::MAX;
            for k in 0..nc {
                if j == k {
                    continue;
                }
                let d = halfcdist[j * nc + k];
                if d < min_distance {
                    min_distance = d;
                }
            }
            s[j] = min_distance;
        }

        let rjreset = iteration != 0;

        for j in 0..ns {
            // Step 2: Identify all points x such that u(x) <= s(c(x))
            if upper_bound[j] <= s[closest_centers[j] as usize] {
                continue;
            }

            let mut rj = rjreset;

            for k in 0..nc {
                // Step 3: For all remaining points x and centers c
                if k as i32 == closest_centers[j] {
                    continue;
                }
                if upper_bound[j] <= lower_bound[j * nc + k] {
                    continue;
                }
                if upper_bound[j] <= halfcdist[closest_centers[j] as usize * nc + k] {
                    continue;
                }

                let vec = samples.datum(j as i32)?;

                // Step 3a
                let dxcx: f32 = if rj {
                    let c = closest_centers[j];
                    let dxcx = distance(procinfo, collation, vec, centers.datum(c)?)? as f32;
                    // d(x,c(x)) computed, which is a form of d(x,c)
                    lower_bound[j * nc + c as usize] = dxcx;
                    upper_bound[j] = dxcx;
                    rj = false;
                    dxcx
                } else {
                    upper_bound[j]
                };

                // Step 3b
                if dxcx > lower_bound[j * nc + k] || dxcx > halfcdist[closest_centers[j] as usize * nc + k] {
                    let dxc = distance(procinfo, collation, vec, centers.datum(k as i32)?)? as f32;

                    // d(x,c) calculated
                    lower_bound[j * nc + k] = dxc;

                    if dxc < dxcx {
                        closest_centers[j] = k as i32;
                        // c(x) changed
                        upper_bound[j] = dxc;
                        changes += 1;
                    }
                }
            }
        }

        // Step 4: For each center c, let m(c) be mean of all points assigned
        compute_new_centers(
            samples,
            &mut agg,
            &mut new_centers,
            &mut center_counts,
            &closest_centers,
            kmeans_norm,
            collation,
            type_info,
        )?;

        // Step 5
        for j in 0..nc {
            newcdist[j] =
                distance(procinfo, collation, centers.datum(j as i32)?, new_centers.datum(j as i32)?)? as f32;
        }

        for j in 0..ns {
            for k in 0..nc {
                let mut d = lower_bound[j * nc + k] - newcdist[k];
                if d < 0.0 {
                    d = 0.0;
                }
                lower_bound[j * nc + k] = d;
            }
        }

        // Step 6
        // We reset r(x) before Step 3 in the next iteration
        for j in 0..ns {
            upper_bound[j] += newcdist[closest_centers[j] as usize];
        }

        // Step 7
        for j in 0..num_centers {
            centers.set(j, new_centers.get(j)?)?;
        }

        if changes == 0 && iteration != 0 {
            break;
        }
    }
    Ok(())
}

// CheckElements (ivfkmeans.c:490-512): ensure no NaN or infinite values.
fn check_elements(centers: &VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let mut scratch = vec![0.0f32; centers.dim as usize];
    for i in 0..centers.length {
        scratch.fill(0.0);
        // /fp:fast may not propagate NaN with MSVC, but that's alright
        (type_info.sum_center)(centers.get(i)?, centers.dim, &mut scratch)?;
        for x in scratch.iter() {
            if x.is_nan() {
                return Err(elog_error("NaN detected. Please report a bug.", "ivfkmeans.c", 506, "CheckElements"));
            }
            if x.is_infinite() {
                return Err(elog_error(
                    "Infinite value detected. Please report a bug.",
                    "ivfkmeans.c",
                    509,
                    "CheckElements",
                ));
            }
        }
    }
    Ok(())
}

// CheckNorms (ivfkmeans.c:517-534): ensure no zero vectors for cosine
// distance. It checks NORM_PROC, not KMEANS_NORM_PROC.
fn check_norms(centers: &VectorArray<'_>, procs: &mut KmeansProcs) -> PgResult<()> {
    let collation = procs.collation;
    let Some(normprocinfo) = procs.norm.as_mut() else {
        return Ok(());
    };
    for i in 0..centers.length {
        let norm = types_fmgr::function_call1_coll(normprocinfo, collation, centers.datum(i)?)?.as_f64();
        if norm == 0.0 {
            return Err(elog_error("Zero norm detected. Please report a bug.", "ivfkmeans.c", 532, "CheckNorms"));
        }
    }
    Ok(())
}

// CheckCenters (ivfkmeans.c:539-547): detect issues with centers.
fn check_centers(centers: &VectorArray<'_>, type_info: &IvfflatTypeInfo, procs: &mut KmeansProcs) -> PgResult<()> {
    if centers.length != centers.maxlen {
        return Err(elog_error("Not enough centers. Please report a bug.", "ivfkmeans.c", 543, "CheckCenters"));
    }
    check_elements(centers, type_info)?;
    check_norms(centers, procs)
}

// IvfflatKmeans (ivfkmeans.c:549-570): spherical k-means for inner product
// and cosine.
pub fn ivfflat_kmeans(
    index: &Relation<'_>,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    let mut procs = KmeansProcs {
        collation: index_collation(index),
        distance: if samples.length == 0 {
            None
        } else {
            Some(index_getprocinfo(index, IVFFLAT_KMEANS_DISTANCE_PROC)?)
        },
        kmeans_norm: ivfflat_optional_proc_info(index, IVFFLAT_KMEANS_NORM_PROC)?,
        norm: ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?,
    };
    kmeans(&mut procs, samples, centers, type_info, memory_used)
}

// IvfflatKmeans' body over resolved support procs.
pub fn kmeans(
    procs: &mut KmeansProcs,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    if samples.length == 0 {
        random_centers(procs, centers, type_info)?;
    } else {
        elkan_kmeans(procs, samples, centers, type_info, memory_used)?;
    }
    check_centers(centers, type_info, procs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgvector::vec::{VecBuilder, VecView};
    use pgvector_ivfflat::utils::VECTOR_TYPE_INFO;

    fn l2_procs() -> KmeansProcs {
        KmeansProcs {
            collation: 0,
            distance: Some(types_fmgr::FmgrInfo::new(pgvector::funcs::fc_l2_distance, 1, 2, true, false)),
            kmeans_norm: None,
            norm: None,
        }
    }

    fn sample_array<'m>(m: mcx::Mcx<'m>, maxlen: i32, points: &[[f32; 2]]) -> VectorArray<'m> {
        let mut arr = VectorArray::init(m, maxlen, 2, 16).unwrap();
        for (i, p) in points.iter().enumerate() {
            let mut b = VecBuilder::new(m, 2).unwrap();
            b.set(0, p[0]);
            b.set(1, p[1]);
            arr.set(i as i32, &b.image()).unwrap();
            arr.length += 1;
        }
        arr
    }

    fn center(arr: &VectorArray<'_>, i: i32) -> [f32; 2] {
        let v = VecView::from_payload(&arr.get(i).unwrap()[4..16]).unwrap();
        [v.x(0), v.x(1)]
    }

    // Elkan k-means (ivfkmeans.c:246-485) on two well-separated groups of
    // four points ends on each group's exact mean (the bytecmp cases' shape).
    #[test]
    fn ivf_kmeans_converges_to_group_means() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let pts = [
            [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0],
            [256.0, 0.0], [257.0, 0.0], [256.0, 1.0], [257.0, 1.0],
        ];
        let samples = sample_array(m, 8, &pts);
        let mut centers = VectorArray::init(m, 2, 2, 16).unwrap();
        pg_prng::global_prng(|p| p.seed(42));
        kmeans(&mut l2_procs(), &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        let mut got = [center(&centers, 0), center(&centers, 1)];
        got.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(got, [[0.5, 0.5], [256.5, 0.5]]);
    }

    // Review Focus 1: three distinct values for five centers. k-means++ runs
    // out of weight and picks sample 0 again (ivfkmeans.c:78-84), empty
    // clusters get random centers (:224-227), and every center is filled and
    // finite (CheckCenters).
    #[test]
    fn ivf_kmeans_fills_every_center_from_duplicates() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let pts = [[1.0, 1.0], [1.0, 1.0], [1.0, 1.0], [5.0, 5.0], [5.0, 5.0], [9.0, 9.0]];
        let samples = sample_array(m, 6, &pts);
        let mut centers = VectorArray::init(m, 5, 2, 16).unwrap();
        pg_prng::global_prng(|p| p.seed(7));
        kmeans(&mut l2_procs(), &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        assert_eq!(centers.length, 5);
        let got: Vec<[f32; 2]> = (0..5).map(|i| center(&centers, i)).collect();
        for p in [[1.0, 1.0], [5.0, 5.0], [9.0, 9.0]] {
            assert!(got.contains(&p), "{got:?}");
        }
    }

    // RandomCenters (ivfkmeans.c:110-133): with no samples (an empty table, or
    // an unlogged index's init fork) every center comes from RandomDouble, in
    // center order, then dimension order.
    #[test]
    fn ivf_kmeans_random_centers_without_samples() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let samples = VectorArray::init(m, 1, 2, 16).unwrap();
        let mut centers = VectorArray::init(m, 3, 2, 16).unwrap();
        let mut procs = KmeansProcs { collation: 0, distance: None, kmeans_norm: None, norm: None };
        pg_prng::global_prng(|p| p.seed(42));
        kmeans(&mut procs, &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        let mut want = pg_prng::PgPrng::seeded(42);
        for i in 0..3 {
            assert_eq!(center(&centers, i), [want.next_f64() as f32, want.next_f64() as f32]);
        }
    }

    // CheckCenters and CheckElements (ivfkmeans.c:490-547).
    #[test]
    fn ivf_check_centers_rejects_short_arrays_and_nan() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut procs = KmeansProcs { collation: 0, distance: None, kmeans_norm: None, norm: None };
        let short = sample_array(m, 2, &[[1.0, 1.0]]);
        let e = check_centers(&short, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "Not enough centers. Please report a bug.");
        let nan = sample_array(m, 2, &[[1.0, 1.0], [f32::NAN, 0.0]]);
        let e = check_centers(&nan, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "NaN detected. Please report a bug.");
        let inf = sample_array(m, 1, &[[f32::INFINITY, 0.0]]);
        let e = check_centers(&inf, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "Infinite value detected. Please report a bug.");
    }
}
```

- [ ] **Step 3: Write `pgvector_ivfflat_build/src/lib.rs`**

```rust
//! pgvector 0.8.7 ivfbuild.c, serial rendering. It samples rows (ANALYZE's
//! block sampler and reservoir), runs k-means (kmeans.rs, ivfkmeans.c) and
//! writes the metapage and list pages. Then it assigns every row to its
//! nearest center, sorts by list and loads the entry pages. Every page goes
//! through GenericXLog, as in C.
//! DIVERGENCE (recorded): no parallel build (plan_create_index_workers is
//! unported, M6); C builds serially when no workers launch.
//! pgrust.ivfflat_build_seed reseeds a build the way C's -DIVFFLAT_BENCH
//! build does (opt-in).

pub mod kmeans;

use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgvector_ivfflat::sort::{HeapSort, INT4_LESS_OPERATOR};
use pgvector_ivfflat::utils::*;
use types_core::{ForkNumber, InvalidBlockNumber, Oid, INT4OID, TIDOID, VARBITOID};
use types_error::{
    PgError, PgResult, ERRCODE_FEATURE_NOT_SUPPORTED, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED, NOTICE,
};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_tuple::itemptr::ItemPointerData;

pub struct IndexBuildResult {
    pub heap_tuples: f64,
    pub index_tuples: f64,
}

// IvfflatBuildState (ivfflat.h:198-250), the serial fields.
struct BuildState<'a, 'm, 'mcx> {
    // Info
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    type_info: &'static IvfflatTypeInfo,

    // Settings
    dimensions: i32,
    lists: i32,

    // Statistics
    indtuples: f64,
    reltuples: f64,

    // Support functions
    procinfo: FmgrInfo,
    normprocinfo: Option<FmgrInfo>,
    kmeansnormprocinfo: Option<FmgrInfo>,
    collation: Oid,

    // Variables
    cx: Mcx<'m>,
    samples: Option<VectorArray<'m>>,
    centers: VectorArray<'m>,
    list_info: Vec<ListInfo>,
    itemsize: usize,

    // Sampling
    rstate: Option<sampling::ReservoirStateData>,
    samplerows: f64,
    rowstoskip: f64,

    // Sorting
    sortstate: Option<HeapSort>,

    // Memory
    memory_used: usize,
}

// AddSample (ivfbuild.c:56-105).
fn add_sample(bs: &mut BuildState<'_, '_, '_>, values: &[Datum]) -> PgResult<()> {
    // C allocates in buildstate->tmpCtx and resets it after each row.
    let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");

    // Detoast once for all calls
    let value = detoast_value(tmp.mcx(), values[0])?;
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Check dimensions match index
    ivfflat_check_dim(bs.dimensions, bs.type_info, bs.collation, d)?;

    // Check with KMEANS_NORM_PROC that the value can be normalized since
    // spherical distance function expects unit vectors
    if let Some(np) = bs.kmeansnormprocinfo.as_mut() {
        if !ivfflat_check_norm(np, bs.collation, d)? {
            return Ok(());
        }
    }

    let samples = bs.samples.as_mut().expect("samples allocated");
    let targsamples = samples.maxlen;
    if samples.length < targsamples {
        samples.set(samples.length, &value)?;
        samples.length += 1;
    } else {
        let rstate = bs.rstate.as_mut().expect("reservoir initialized");
        if bs.rowstoskip < 0.0 {
            bs.rowstoskip = sampling::reservoir_get_next_s(rstate, bs.samplerows, targsamples as u32);
        }

        if bs.rowstoskip <= 0.0 {
            let k = (targsamples as f64 * sampling::sampler_random_fract(&mut rstate.randstate)) as i32;
            debug_assert!(k >= 0 && k < targsamples);
            samples.set(k, &value)?;
        }

        bs.rowstoskip -= 1.0;
    }

    // Increment after reservoir_get_next_S
    bs.samplerows += 1.0;
    Ok(())
}

// SampleRows (ivfbuild.c:135-159): sample rows with the same logic as
// ANALYZE. SampleCallback (ivfbuild.c:110-130) is the closure.
fn sample_rows<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<()> {
    let targsamples = bs.samples.as_ref().expect("samples allocated").maxlen;
    let totalblocks = bufmgr::RelationGetNumberOfBlocksInFork(heap, ForkNumber::MAIN_FORKNUM)?;

    bs.samplerows = 0.0;
    bs.rowstoskip = -1.0;

    let (mut sampler, _) =
        sampling::block_sampler_init(totalblocks, targsamples as u32, pg_prng::global_prng(|p| p.next_u32()));

    // reservoir_init_selection_state seeds from pg_prng_uint32
    // (sampling.c:139).
    bs.rstate = Some(sampling::reservoir_init_selection_state(
        pg_prng::global_prng(|p| p.next_u32()) as u64,
        targsamples as u32,
    ));

    let index = bs.index;
    while sampler.has_more() {
        let targblock = sampler.next();
        let mut inner_err: Option<Box<PgError>> = None;
        // BuildState is threaded via raw pointer: the callback is FnMut and
        // borrows would alias bs.
        let bs_ptr: *mut BuildState<'_, '_, 'mcx> = &mut *bs;

        // Set anyvisible to false like table_index_build_scan
        let r = execindexing::table_index_build_range_scan(
            mcx,
            heap,
            index,
            index_info,
            false,
            false,
            false,
            targblock,
            1,
            |_index_rel, _tid, values, isnull, _alive| {
                // Skip nulls
                if isnull[0] {
                    return Ok(());
                }
                // SAFETY: single-threaded serial build; bs outlives the scan.
                let bs = unsafe { &mut *bs_ptr };
                add_sample(bs, values).map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat sample failed").into()
                })
            },
        );
        if let Err(e) = r {
            return Err(inner_err.unwrap_or(e));
        }
    }

    // Normalize if needed
    if bs.kmeansnormprocinfo.is_some() {
        ivfflat_norm_vectors(bs.type_info, bs.collation, bs.samples.as_mut().expect("samples allocated"))?;
    }
    Ok(())
}

// AddTupleToSort (ivfbuild.c:164-225).
fn add_tuple_to_sort(bs: &mut BuildState<'_, '_, '_>, tid: &ItemPointerData, values: &[Datum]) -> PgResult<()> {
    let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");
    let mut min_distance = f64::MAX;
    let mut closest_center = 0i32;

    // Detoast once for all calls
    let mut value = detoast_value(tmp.mcx(), values[0])?;

    // Normalize if needed
    if let Some(np) = bs.normprocinfo.as_mut() {
        if !ivfflat_check_norm(np, bs.collation, Datum::from_usize(value.as_ptr() as usize))? {
            return Ok(());
        }
        value = ivfflat_norm_value(bs.type_info, bs.collation, tmp.mcx(), &value)?;
    }
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Check dimensions match index
    ivfflat_check_dim(bs.dimensions, bs.type_info, bs.collation, d)?;

    // Find the list that minimizes the distance
    for i in 0..bs.centers.length {
        let distance = types_fmgr::function_call2_coll(&mut bs.procinfo, bs.collation, d, bs.centers.datum(i)?)?
            .as_f64();
        if distance < min_distance {
            min_distance = distance;
            closest_center = i;
        }
    }

    // Add tuple to sort; tuplesort copies the input, the TID by reference
    // (TIDOID)
    let tid_datum = Datum::from_usize(tid as *const ItemPointerData as usize);
    bs.sortstate
        .as_mut()
        .expect("sort state")
        .put(&[Datum::from_i32(closest_center), tid_datum, d])?;

    bs.indtuples += 1.0;
    Ok(())
}

// GetNextTuple (ivfbuild.c:255-272): the next (list, index tuple) from the
// sort; list -1 once it is drained.
fn get_next_tuple<'m>(
    sort: &mut HeapSort,
    index: &Relation<'_>,
    cx: Mcx<'m>,
) -> PgResult<(i32, Option<nbtree::itup::ItupBuf<'m>>)> {
    let mut values = [Datum::from_usize(0); 3];
    if !sort.next(&mut values)? {
        return Ok((-1, None));
    }
    let list = values[0].as_i32();

    // Form the index tuple
    let mut itup = nbtree::itup::index_form_tuple(cx, &index.rd_att, &values[2..3], &[false])?;
    // SAFETY: the sort's TID column, 6 bytes at a by-reference datum.
    let tid = unsafe { core::ptr::read_unaligned(values[1].as_usize() as *const ItemPointerData) };
    // SAFETY: t_tid is the first 6 bytes of the image (itup.h).
    unsafe { itup.as_mut_ptr().cast::<ItemPointerData>().write_unaligned(tid) };
    Ok((list, Some(itup)))
}

#[cold]
fn add_item_failed(index: &Relation<'_>, line: i32, func: &'static str) -> Box<PgError> {
    elog_error(format!("failed to add index item to \"{}\"", index.name()), "ivfbuild.c", line, func)
}

// InsertTuples (ivfbuild.c:277-337): create the initial entry pages, one
// chain per list.
fn insert_tuples(index: &Relation<'_>, bs: &mut BuildState<'_, '_, '_>, fork_num: ForkNumber) -> PgResult<()> {
    let mut inserted: i64 = 0;
    let load_cx = mcx::MemoryContext::new("Ivfflat load context");
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let sort = bs.sortstate.as_mut().expect("sort state");

    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_LOAD,
    );
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_TOTAL,
        bs.indtuples as i64,
    );

    let (mut list, mut itup) = get_next_tuple(sort, index, load_cx.mcx())?;

    for i in 0..bs.centers.length {
        // Can take a while, so ensure we can interrupt
        // Needs to be called when no buffer locks are held
        postgres_seams::check_for_interrupts::call()?;

        let buf = ivfflat_new_buffer(index, fork_num)?;
        let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;
        let start_page = bufmgr::BufferGetBlockNumber(xp.buf);

        // Get all tuples for list
        while list == i {
            let t = itup.take().expect("a tuple for this list");
            // Check for free space
            let itemsz = t.size();
            if xp.free_space() < itemsz {
                xp = ivfflat_append_page(index, xp, fork_num, xcx.mcx())?;
            }

            // Add the item
            // SAFETY: the formed tuple's MAXALIGNed image.
            let item = unsafe { core::slice::from_raw_parts(t.as_ptr(), itemsz) };
            if xp.add_item(item).is_none() {
                return Err(add_item_failed(index, 321, "InsertTuples"));
            }
            drop(t);

            inserted += 1;
            backend_progress::pgstat_progress_update_param(
                backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_DONE,
                inserted,
            );

            (list, itup) = get_next_tuple(sort, index, load_cx.mcx())?;
        }

        let insert_page = bufmgr::BufferGetBlockNumber(xp.buf);
        xp.commit()?;

        // Set the start and insert pages
        ivfflat_update_list(index, bs.list_info[i as usize], insert_page, InvalidBlockNumber, start_page, fork_num)?;
    }
    Ok(())
}

// InitBuildState (ivfbuild.c:342-418).
fn init_build_state<'a, 'm, 'mcx>(
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    cx: Mcx<'m>,
) -> PgResult<BuildState<'a, 'm, 'mcx>> {
    let type_info = ivfflat_get_type_info(index)?;
    let lists = ivfflat_get_lists(index);
    let dimensions = index.rd_att.attr(0).atttypmod;

    // Disallow varbit since require fixed dimensions
    if index.rd_att.attr(0).atttypid == VARBITOID {
        return Err(PgError::error("type not supported for ivfflat index")
            .with_sqlstate(ERRCODE_FEATURE_NOT_SUPPORTED)
            .with_location("ivfbuild.c", 358, "InitBuildState")
            .into());
    }

    // Require column to have dimensions to be indexed
    if dimensions < 0 {
        return Err(PgError::error("column does not have dimensions")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .with_location("ivfbuild.c", 364, "InitBuildState")
            .into());
    }

    if dimensions > type_info.max_dimensions {
        return Err(PgError::error(format!(
            "column cannot have more than {} dimensions for ivfflat index",
            type_info.max_dimensions
        ))
        .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
        .with_location("ivfbuild.c", 369, "InitBuildState")
        .into());
    }

    // Get support functions
    let procinfo = index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?;
    let normprocinfo = ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?;
    let kmeansnormprocinfo = ivfflat_optional_proc_info(index, IVFFLAT_KMEANS_NORM_PROC)?;
    let collation = index_collation(index);

    // Require more than one dimension for spherical k-means
    if kmeansnormprocinfo.is_some() && dimensions == 1 {
        return Err(PgError::error("dimensions must be greater than one for this opclass")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .with_location("ivfbuild.c", 384, "InitBuildState")
            .into());
    }

    let itemsize = (type_info.item_size)(dimensions);
    let memory_used = mcx::add_size(0, vector_array_size(lists, itemsize)?)?;
    ivfflat_check_memory_usage(memory_used)?;
    let centers = VectorArray::init(cx, lists, dimensions, itemsize)?;

    Ok(BuildState {
        heap,
        index,
        type_info,
        dimensions,
        lists,
        indtuples: 0.0,
        reltuples: 0.0,
        procinfo,
        normprocinfo,
        kmeansnormprocinfo,
        collation,
        cx,
        samples: None,
        centers,
        list_info: vec![ListInfo { blkno: InvalidBlockNumber, offno: 0 }; lists as usize],
        itemsize,
        rstate: None,
        samplerows: 0.0,
        rowstoskip: -1.0,
        sortstate: None,
        memory_used,
    })
}

// ComputeCenters (ivfbuild.c:440-486).
fn compute_centers<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
) -> PgResult<()> {
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_KMEANS,
    );

    // Skip samples for unlogged table
    let num_samples: i32 = match bs.heap {
        None => 1,
        Some(heap) => {
            let max_tuples = bufmgr::RelationGetNumberOfBlocksInFork(heap, ForkNumber::MAIN_FORKNUM)? as i64
                * types_storage::bufpage::MaxHeapTuplesPerPage as i64;

            // Target 50 samples per list, with at least 10000 samples
            // The number of samples has a large effect on index build time
            let mut n = bs.lists * 50;
            if n < 10000 {
                n = 10000;
            }

            // Save memory since will not have more than max tuples
            (n as i64).min(max_tuples).max(1) as i32
        }
    };

    // Sample rows
    bs.memory_used = mcx::add_size(bs.memory_used, vector_array_size(num_samples, bs.itemsize)?)?;
    ivfflat_check_memory_usage(bs.memory_used)?;
    bs.samples = Some(VectorArray::init(bs.cx, num_samples, bs.dimensions, bs.itemsize)?);
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        sample_rows(bs, mcx, heap, index_info)?;

        if bs.samples.as_ref().expect("samples allocated").length < bs.lists {
            elog::ereport(NOTICE)
                .errmsg("ivfflat index created with little data")
                .errdetail("This will cause low recall.")
                .errhint("Drop the index until the table has more data.")
                .finish(types_error::ErrorLocation::new("ivfbuild.c", 477, "ComputeCenters"))?;
        }
    }

    // Calculate centers
    let samples = bs.samples.take().expect("samples allocated");
    kmeans::ivfflat_kmeans(bs.index, &samples, &mut bs.centers, bs.type_info, bs.memory_used)?;

    // Free samples before we allocate more memory
    drop(samples);
    Ok(())
}

// CreateMetaPage (ivfbuild.c:491-512).
fn create_meta_page(index: &Relation<'_>, dimensions: i32, lists: i32, fork_num: ForkNumber) -> PgResult<()> {
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let buf = ivfflat_new_buffer(index, fork_num)?;
    let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;

    // Set metapage data
    IvfflatMetaPage {
        magic_number: IVFFLAT_MAGIC_NUMBER,
        version: IVFFLAT_VERSION,
        dimensions: dimensions as u16,
        lists: lists as u16,
    }
    .write(xp.page_mut());
    xp.set_pd_lower((PAGE_CONTENTS_OFFSET + IVFFLAT_METAPAGE_SIZE) as u16);

    xp.commit()
}

// CreateListPages (ivfbuild.c:517-562).
fn create_list_pages(
    index: &Relation<'_>,
    centers: &VectorArray<'_>,
    lists: i32,
    fork_num: ForkNumber,
    list_info: &mut [ListInfo],
) -> PgResult<()> {
    let xcx = mcx::MemoryContext::new("Ivfflat build xlog");
    let list_size = maxalign(ivfflat_list_size(centers.itemsize));
    let mut list = vec![0u8; list_size];

    let buf = ivfflat_new_buffer(index, fork_num)?;
    let mut xp = ivfflat_init_register_page(index, buf, xcx.mcx())?;

    for i in 0..lists {
        // Zero memory for each list
        list.fill(0);

        // Load list
        set_list_start_page(&mut list, InvalidBlockNumber);
        set_list_insert_page(&mut list, InvalidBlockNumber);
        let center = centers.get(i)?;
        let size = varsize_any(center);
        list[IVFFLAT_LIST_CENTER_OFFSET..IVFFLAT_LIST_CENTER_OFFSET + size].copy_from_slice(&center[..size]);

        // Ensure free space
        if xp.free_space() < list_size {
            xp = ivfflat_append_page(index, xp, fork_num, xcx.mcx())?;
        }

        // Add the item
        let Some(offno) = xp.add_item(&list) else {
            return Err(add_item_failed(index, 552, "CreateListPages"));
        };

        // Save location info
        list_info[i as usize] = ListInfo { blkno: bufmgr::BufferGetBlockNumber(xp.buf), offno };
    }

    xp.commit()
}

// AssignTuples (ivfbuild.c:977-1018), serial. BuildCallback
// (ivfbuild.c:230-250) is the closure.
fn assign_tuples<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
) -> PgResult<()> {
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_ASSIGN,
    );

    // Begin serial tuplesort (InitBuildSortState, ivfbuild.c:612-621, over
    // InitBuildState's sortdesc, ivfbuild.c:386-390)
    let value_type = bs.index.rd_att.attr(0).atttypid;
    bs.sortstate = Some(HeapSort::begin(
        "Ivfflat build sort",
        &[("list", INT4OID), ("tid", TIDOID), ("vector", value_type)],
        INT4_LESS_OPERATOR,
        init_small::globals::maintenance_work_mem(),
    )?);

    // Add tuples to sort
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        let index = bs.index;
        let mut inner_err: Option<Box<PgError>> = None;
        // BuildState is threaded via raw pointer: the callback is FnMut and
        // borrows would alias bs.
        let bs_ptr: *mut BuildState<'_, '_, 'mcx> = &mut *bs;
        let reltuples = execindexing::table_index_build_scan(
            mcx,
            heap,
            index,
            index_info,
            true,
            /* progress */ true,
            |_index_rel, tid, values, isnull, _alive| {
                // Skip nulls
                if isnull[0] {
                    return Ok(());
                }
                // SAFETY: single-threaded serial build; bs outlives the scan.
                let bs = unsafe { &mut *bs_ptr };
                add_tuple_to_sort(bs, tid, values).map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat build insert failed").into()
                })
            },
        );
        match reltuples {
            Ok(n) => bs.reltuples = n,
            Err(e) => return Err(inner_err.unwrap_or(e)),
        }
    }
    Ok(())
}

// CreateEntryPages (ivfbuild.c:1023-1041).
fn create_entry_pages<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<()> {
    // Assign
    assign_tuples(bs, mcx, index_info)?;

    // Sort
    bs.sortstate.as_mut().expect("sort state").perform()?;

    // Load
    let index = bs.index;
    insert_tuples(index, bs, fork_num)?;

    // End sort
    bs.sortstate = None;
    Ok(())
}

// BuildIndex (ivfbuild.c:1046-1064).
fn build_index<'mcx>(
    mcx: Mcx<'mcx>,
    heap: Option<&Relation<'mcx>>,
    index: &Relation<'mcx>,
    mut index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<IndexBuildResult> {
    let build_cx = mcx::MemoryContext::new("Ivfflat build context");
    let mut bs = init_build_state(heap, index, build_cx.mcx())?;

    compute_centers(&mut bs, mcx, index_info.as_deref_mut())?;

    // Create pages
    create_meta_page(index, bs.dimensions, bs.lists, fork_num)?;
    create_list_pages(index, &bs.centers, bs.lists, fork_num, &mut bs.list_info)?;
    create_entry_pages(&mut bs, mcx, index_info, fork_num)?;

    // Write WAL for initialization fork since GenericXLog functions do not
    if fork_num == ForkNumber::INIT_FORKNUM {
        let nblocks = bufmgr::RelationGetNumberOfBlocksInFork(index, fork_num)?;
        xloginsert::log_newpage_range(index, fork_num, 0, nblocks, true)?;
    }

    Ok(IndexBuildResult { heap_tuples: bs.reltuples, index_tuples: bs.indtuples })
}

// pgrust.ivfflat_build_seed: a placeholder customized option, deliberately
// NOT a registered GUC (pgrust.hnsw_build_seed's pattern; a pg_settings row
// would break byte-identical regression outputs). Set to N, ivfflatbuild calls
// SeedRandom(N) where C calls SeedRandom(42) under #ifdef IVFFLAT_BENCH
// (ivfbuild.c:1075-1077). Unset (the default) is stock C, which never seeds;
// ivfflatbuildempty never seeds, as in C.
// DIVERGENCE (opt-in only): stock C has no such switch; the byte-identical
// tier sets 42 to match the -DIVFFLAT_BENCH reference build.
fn ivfflat_build_seed() -> Option<u64> {
    // Uninstalled seam (unit-test binaries without a guc boot): stock C.
    if !guc_seams::get_config_option_missing_ok::is_installed() {
        return None;
    }
    parse_build_seed(
        guc_seams::get_config_option_missing_ok::call("pgrust.ivfflat_build_seed")
            .ok()
            .flatten(),
    )
}

fn parse_build_seed(value: Option<String>) -> Option<u64> {
    value.and_then(|v| v.trim().parse::<u64>().ok())
}

// ivfflatbuild (ivfbuild.c:1069-1086).
pub fn ivfflatbuild<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<IndexBuildResult> {
    // SeedRandom(42) under #ifdef IVFFLAT_BENCH, before BuildIndex.
    if let Some(seed) = ivfflat_build_seed() {
        pg_prng::global_prng(|p| p.seed(seed));
    }
    build_index(mcx, Some(heap), index, Some(index_info), ForkNumber::MAIN_FORKNUM)
}

// ivfflatbuildempty (ivfbuild.c:1091-1098): the unlogged init fork, built
// with no heap (one sample slot, so random centers).
pub fn ivfflatbuildempty(index: &Relation<'_>) -> PgResult<()> {
    let owner = mcx::MemoryContext::new_bump("ivfflat buildempty");
    build_index(owner.mcx(), None, index, None, ForkNumber::INIT_FORKNUM)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // pgrust.ivfflat_build_seed parses as an unsigned integer, like
    // pgrust.hnsw_build_seed.
    #[test]
    fn ivf_parse_build_seed_accepts_only_unsigned_integers() {
        assert_eq!(parse_build_seed(None), None);
        assert_eq!(parse_build_seed(Some("42".to_string())), Some(42));
        assert_eq!(parse_build_seed(Some(" 7 ".to_string())), Some(7));
        assert_eq!(parse_build_seed(Some(String::new())), None);
        assert_eq!(parse_build_seed(Some("-1".to_string())), None);
        assert_eq!(parse_build_seed(Some("abc".to_string())), None);
    }
}
```

`mcx::add_size(0, ...)` in `init_build_state` mirrors C's `add_size(buildstate->memoryUsed, ...)` with `memoryUsed = 0` (`ivfbuild.c:397-400`).

- [ ] **Step 4: Add the build crate to the workspace and build**

In the root `Cargo.toml` `members`, after `"crates/contrib/pgvector_ivfflat",` add `"crates/contrib/pgvector_ivfflat_build",`. Then:
```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
cargo check --profile fast-profile --locked -p main_main -p sampling -p pgvector_ivfflat_build
```
Expected: `Cargo.lock` gains `sampling` and `pgvector_ivfflat_build`, and `commands_analyze` gains `sampling`. `cargo check` succeeds.

- [ ] **Step 5: Run the unit tests**

```bash
UT ivf_ block_sampler reservoir_
```
Expected: 19 passed:
- `types_ivfflat` 3 and `pgvector_ivfflat` 8 (Task 1);
- `pgvector_ivfflat_build` 5: four `kmeans` tests and `ivf_parse_build_seed_accepts_only_unsigned_integers`;
- `sampling` 3: the moved `block_sampler_*` and `reservoir_*` tests.

Then `UT_KEEP`: all pass.

- [ ] **Step 6: ANALYZE still samples**

```bash
scripts/pgvector/build-pgrust.sh
printf "CREATE TABLE a (i int, v vector(2));\nINSERT INTO a SELECT i, ARRAY[i, i %% 7] FROM generate_series(1, 50000) i;\nANALYZE a;\nSELECT reltuples FROM pg_class WHERE relname = 'a';\nSELECT count(*) FROM pg_stats WHERE tablename = 'a';\n" | scripts/pgvector/sqldiff.sh pgrust -
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected: sqldiff identical (`reltuples` 50000 and 2 `pg_stats` rows on both sides), and both lints PASS. The build crate is not yet reachable from `main_main`; Task 4 wires it.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/backend/utils/misc/sampling crates/backend/commands/analyze crates/contrib/pgvector_ivfflat_build
git commit -q -F - <<'EOF'
pgvector: ivfkmeans.c and the serial ivfbuild.c; sampling in its own crate

ANALYZE's block sampler and reservoir move to a `sampling` crate (C's
utils/misc/sampling.c) so the IVFFlat build can use them without a cycle
through commands_analyze. pgvector_ivfflat_build ports ivfbuild.c's serial
build (sampling, k-means, meta/list/entry pages through GenericXLog,
progress subphases, the little-data NOTICE) and ivfkmeans.c (k-means++,
Elkan, center checks), with an opt-in pgrust.ivfflat_build_seed for C's
-DIVFFLAT_BENCH SeedRandom.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 3: Insert, scan, vacuum, the relscan variant and the `ivfflat.*` settings

**Files:**
- Modify: `crates/backend/access/index/relscan/Cargo.toml` (+`types_ivfflat`), `crates/backend/access/index/relscan/src/lib.rs`
- Create: `crates/contrib/pgvector_ivfflat/src/insert.rs`, `src/scan.rs`, `src/vacuum.rs`
- Modify: `crates/contrib/pgvector_ivfflat/src/lib.rs` (replaced), `crates/contrib/pgvector_ivfflat/Cargo.toml`
- Modify: `crates/backend/utils/misc/guc_tables/src/vars.rs`, `src/tables.rs`, `src/tests.rs`
- Modify: `crates/_support/seams_init/Cargo.toml`, `crates/_support/seams_init/src/lib.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes Task 1's `types_ivfflat::*`, `pgvector_ivfflat::utils::*` and `pgvector_ivfflat::sort::{HeapSort, FLOAT8_LESS_OPERATOR}`.
- Produces `types_relscan::IndexAmKind::Ivfflat` and `IndexScanOpaque::Ivfflat(PgBox<'mcx, types_ivfflat::IvfflatScanOpaqueData>)`.
- Produces, in `pgvector_ivfflat` (signatures match HNSW's, so Task 4's dispatch arms mirror HNSW's):
  - `pub fn ivfflatinsert<'mcx>(mcx: Mcx<'mcx>, index: &Relation<'mcx>, values: &[Datum], isnull: &[bool], heap_tid: &ItemPointerData, heap: &Relation<'mcx>) -> PgResult<bool>`;
  - `pub fn ivfflatbeginscan<'mcx>(mcx: Mcx<'mcx>, index: &Relation<'mcx>, nkeys: i32, norderbys: i32) -> PgResult<IndexScanDescData<'mcx>>`;
  - `pub fn ivfflatrescan(scan: &mut IndexScanDescData<'_>, keys: Option<&[ScanKeyData]>, orderbys: Option<&[ScanKeyData]>) -> PgResult<()>`;
  - `pub fn ivfflatgettuple(scan: &mut IndexScanDescData<'_>, direction: ScanDirection) -> PgResult<bool>`;
  - `pub fn ivfflatendscan(scan: &mut IndexScanDescData<'_>) -> PgResult<()>`;
  - `pub fn ivfflatbulkdelete<'mcx>(info: &IndexVacuumInfo<'_, 'mcx>, istat: Option<IndexBulkDeleteResult>, dead_items: &[ItemPointerData]) -> PgResult<IndexBulkDeleteResult>`;
  - `pub fn ivfflatbulkdelete_collect<'mcx>(info: &IndexVacuumInfo<'_, 'mcx>, callback: &mut (dyn FnMut(&ItemPointerData) -> PgResult<()> + '_)) -> PgResult<IndexBulkDeleteResult>`;
  - `pub fn ivfflatvacuumcleanup<'mcx>(info: &IndexVacuumInfo<'_, 'mcx>, istat: Option<IndexBulkDeleteResult>) -> PgResult<Option<IndexBulkDeleteResult>>`;
  - `pub fn ivfflatvalidate(opclassoid: Oid) -> PgResult<bool>`;
  - `pub fn init_seams()`.
- Produces `guc_tables::vars::{ivfflat_probes, ivfflat_max_probes}: GucIntVar` and `ivfflat_iterative_scan: GucEnumVar`.

- [ ] **Step 1: The relscan variant**

In `crates/backend/access/index/relscan/Cargo.toml`, after the `types_hnsw` line add:
```toml
types_ivfflat = { path = "../../../../_support/types/types_ivfflat" }
```
In `crates/backend/access/index/relscan/src/lib.rs`:
- In `pub enum IndexAmKind`, add `Ivfflat,` after `Hnsw,`.
- In each capability method, add an `IndexAmKind::Ivfflat => <value>,` line right after the `IndexAmKind::Hnsw => ...,` line. The values come from `ivfflathandler` (`ivfflat.c:184-319`):

  | method | value | method | value |
  |---|---|---|---|
  | `ampredlocks` | `false` | `amoptsprocnum` | `0` |
  | `amsummarizing` | `false` | `amstorage` | `false` |
  | `amsearcharray` | `false` | `amcanorder` | `false` |
  | `amclusterable` | `false` | `amcanhash` | `false` |
  | `has_ammarkpos` | `false` | `amcanorderbyop` | `true` |
  | `has_amrestrpos` | `false` | `has_aminsertcleanup` | `false` |
  | `amstrategies` | `0` | `amsupport` | `5` |
- In `unported_index_am`'s panic message, replace `btree+hash+gin+gist+spgist+brin+hnsw+bloom` with `btree+hash+gin+gist+spgist+brin+hnsw+ivfflat+bloom`.
- In `pub enum IndexScanOpaque`, add after the `Hnsw(...)` variant:
  ```rust
      Ivfflat(PgBox<'mcx, ::types_ivfflat::IvfflatScanOpaqueData>),
  ```
- In `IndexScanOpaque::kind`, add `IndexScanOpaque::Ivfflat(_) => IndexAmKind::Ivfflat,` after the `Hnsw` arm.
- In `mod am_property_tests`, add `assert!(!IndexAmKind::Ivfflat.amsearcharray());` after the `Hnsw` line of `amsearcharray_is_btree_only`, and add the test:
  ```rust
      // ivfflathandler (ivfflat.c:184-319).
      #[test]
      fn ivf_am_properties_match_ivfflathandler() {
          let k = IndexAmKind::Ivfflat;
          assert_eq!(k.amstrategies(), 0);
          assert_eq!(k.amsupport(), 5);
          assert_eq!(k.amoptsprocnum(), 0);
          assert!(k.amcanorderbyop() && !k.amcanorder() && !k.amcanhash());
          assert!(!k.ampredlocks() && !k.amsummarizing() && !k.amsearcharray() && !k.amclusterable());
          assert!(!k.has_ammarkpos() && !k.has_amrestrpos() && !k.amstorage() && !k.has_aminsertcleanup());
      }
  ```

Most dispatch matches outside relscan end in a wildcard arm, so they still compile; Task 4 adds their IVFFlat arms. No catalog can name an ivfflat AM until Task 5. If any other crate's `match` on `IndexAmKind` or `IndexScanOpaque` fails to compile, add an arm mirroring its `Hnsw` arm, and name it in the report.

- [ ] **Step 2: The `ivfflat.*` settings**

In `crates/backend/utils/misc/guc_tables/src/vars.rs`, after the four `hnsw_*` slots add:
```rust
// pgvector ivfflat.* (contrib GUCs, defined statically; C defines them at
// module load, ivfflat.c:45-56).
pub static ivfflat_probes: GucIntVar = GucSlot::new("ivfflat_probes");
pub static ivfflat_iterative_scan: GucEnumVar = GucSlot::new("ivfflat_iterative_scan");
pub static ivfflat_max_probes: GucIntVar = GucSlot::new("ivfflat_max_probes");
```
In `crates/backend/utils/misc/guc_tables/src/tables.rs`:
- After `hnsw_iterative_scan_options`, add:
  ```rust
  // ivfflat_iterative_scan_options (ivfflat.c:29-33).
  pub static ivfflat_iterative_scan_options: &[config_enum_entry] = &[
      config_enum_entry { name: "off", val: 0, hidden: false },
      config_enum_entry { name: "relaxed_order", val: 1, hidden: false },
  ];
  ```
- After the `hnsw.max_scan_tuples` `GucIntSetting` row, add:
  ```rust
      GucIntSetting { name: "ivfflat.probes", context: PGC_USERSET, group: CUSTOM_OPTIONS, short_desc: Some("Sets the number of probes"), long_desc: Some("Valid range is 1..lists."), flags: 0, variable: &vars::ivfflat_probes, boot_val: GucDefaultValue::Int(1), min: 1, max: 32768, check_hook: None, assign_hook: None, show_hook: None },
      GucIntSetting { name: "ivfflat.max_probes", context: PGC_USERSET, group: CUSTOM_OPTIONS, short_desc: Some("Sets the max number of probes for iterative scans"), long_desc: None, flags: 0, variable: &vars::ivfflat_max_probes, boot_val: GucDefaultValue::Int(32768), min: 1, max: 32768, check_hook: None, assign_hook: None, show_hook: None },
  ```
- After the `hnsw.iterative_scan` `GucEnumSetting` row, add:
  ```rust
      GucEnumSetting { name: "ivfflat.iterative_scan", context: PGC_USERSET, group: CUSTOM_OPTIONS, short_desc: Some("Sets the mode for iterative scans"), long_desc: None, flags: 0, variable: &vars::ivfflat_iterative_scan, boot_val: GucDefaultValue::Enum(0), options: GucEnumOptions::Inline(ivfflat_iterative_scan_options), check_hook: None, assign_hook: None, show_hook: None },
  ```

In `crates/backend/utils/misc/guc_tables/src/tests.rs`, `table_counts_match_compiled_backend_shape`:
- After the comment line `// statically here).` that ends the `pgvector hnsw.*` entry, add `// pgvector ivfflat.*: Int +2 / Enum +1 (likewise).`.
- Change `assert_eq!(ConfigureNamesInt.len(), 182);` to `184`, `assert_eq!(ConfigureNamesEnum.len(), 50);` to `51`, and `assert_eq!(all_settings().count(), 500 + 2 * test_validator);` to `503 + 2 * test_validator`.

If the totals differ from what the file says, keep the file's arithmetic and add exactly +2 Int and +1 Enum. Report the numbers you found.

- [ ] **Step 3: `ivfinsert.c`**

Add to `crates/contrib/pgvector_ivfflat/Cargo.toml` `[dependencies]`:
```toml
types_relscan = { path = "../../backend/access/index/relscan" }
types_scan = { path = "../../_support/types/types_scan" }
types_snapshot = { path = "../../_support/types/snapshot" }
types_nbtree = { path = "../../_support/types/types_nbtree" }
lmgr = { path = "../../backend/storage/lmgr/lmgr" }
vacuum_seams = { path = "../../backend/commands/vacuum_seams" }
xact = { path = "../../backend/access/transam/xact" }
nbtree = { path = "../../backend/access/nbtree/nbtree" }
guc_tables = { path = "../../backend/utils/misc/guc_tables" }
```

`crates/contrib/pgvector_ivfflat/src/insert.rs`:
```rust
//! ivfinsert.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_SHARE};
use datum::Datum;
use mcx::Mcx;
use types_core::{BlockNumber, ForkNumber, InvalidBlockNumber, Oid};
use types_error::{PgError, PgResult};
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;
use types_storage::lock::ExclusiveLock;
use types_tuple::itemptr::ItemPointerData;

use crate::utils::*;

// FindInsertPage (ivfinsert.c:19-67): the list whose center is nearest
// `value`, and that list's insert page. A tie keeps the first list.
fn find_insert_page(
    index: &Relation<'_>,
    value: Datum,
    procinfo: &mut FmgrInfo,
    collation: Oid,
) -> PgResult<(BlockNumber, ListInfo)> {
    let mut min_distance = f64::MAX;
    let mut nextblkno = IVFFLAT_HEAD_BLKNO;
    let mut insert_page = InvalidBlockNumber;

    // Avoid compiler warning
    let mut list_info = ListInfo { blkno: nextblkno, offno: FIRST_OFFSET_NUMBER };

    // Search all list pages
    while nextblkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, nextblkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let maxoffno = page_max_offset(cpage);

        for offno in FIRST_OFFSET_NUMBER..=maxoffno {
            let list = page_item(cpage, offno);
            let center = Datum::from_usize(list_center(list).as_ptr() as usize);
            let distance = types_fmgr::function_call2_coll(procinfo, collation, value, center)?.as_f64();

            if distance < min_distance || insert_page == InvalidBlockNumber {
                insert_page = list_insert_page(list);
                list_info = ListInfo { blkno: nextblkno, offno };
                min_distance = distance;
            }
        }

        nextblkno = page_nextblkno(cpage);
        UnlockReleaseBuffer(cbuf)?;
    }
    Ok((insert_page, list_info))
}

// InsertTuple (ivfinsert.c:72-184).
fn insert_tuple(
    index: &Relation<'_>,
    values: &[Datum],
    isnull: &[bool],
    heap_tid: &ItemPointerData,
    cx: Mcx<'_>,
) -> PgResult<()> {
    let type_info = ivfflat_get_type_info(index)?;
    let collation = index_collation(index);

    // Detoast once for all calls
    let mut value = detoast_value(cx, values[0])?;

    // Normalize if needed
    if let Some(mut normprocinfo) = ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)? {
        if !ivfflat_check_norm(&mut normprocinfo, collation, Datum::from_usize(value.as_ptr() as usize))? {
            return Ok(());
        }
        value = ivfflat_norm_value(type_info, collation, cx, &value)?;
    }
    let d = Datum::from_usize(value.as_ptr() as usize);

    // Ensure index is valid
    let (_, dimensions) = ivfflat_get_meta_page_info(index)?;

    // Check dimensions match index
    ivfflat_check_dim(dimensions, type_info, collation, d)?;

    // Find the insert page - sets the page and list info
    let mut procinfo = index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?;
    let (mut insert_page, list_info) = find_insert_page(index, d, &mut procinfo, collation)?;
    // DIVERGENCE: C only asserts this (ivfinsert.c:110). InvalidBlockNumber
    // is P_NEW to ReadBuffer, which would extend the index from a corrupt
    // list item.
    if insert_page == InvalidBlockNumber {
        return Err(PgError::error(format!(
            "ivfflat index \"{}\" has a list with no insert page",
            index.name()
        ))
        .with_sqlstate(types_error::ERRCODE_INDEX_CORRUPTED)
        .into());
    }
    let original_insert_page = insert_page;

    // Form tuple
    let mut itup = nbtree::itup::index_form_tuple(cx, &index.rd_att, &[d], &isnull[..1])?;
    // SAFETY: t_tid is the first 6 bytes of the image (itup.h).
    unsafe { itup.as_mut_ptr().cast::<ItemPointerData>().write_unaligned(*heap_tid) };

    // Get tuple size
    let itemsz = itup.size();
    // SAFETY: the formed tuple's MAXALIGNed image.
    let item = unsafe { core::slice::from_raw_parts(itup.as_ptr(), itemsz) };

    let xcx = mcx::MemoryContext::new("Ivfflat insert xlog");

    // Find a page to insert the item
    let mut xp = loop {
        let buf = bufmgr::ReadBuffer(index, insert_page)?;
        LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
        let xp = XlogPage::register(index, buf, 0, xcx.mcx())?;

        if xp.free_space() >= itemsz {
            break xp;
        }

        insert_page = page_nextblkno(xp.page());

        if insert_page != InvalidBlockNumber {
            // Move to next page
            xp.abort()?;
            continue;
        }

        // Add a new page
        lmgr::LockRelationForExtension(index, ExclusiveLock)?;
        let newbuf = ivfflat_new_buffer(index, ForkNumber::MAIN_FORKNUM)?;
        lmgr::UnlockRelationForExtension(index, ExclusiveLock)?;

        // Update insert page
        insert_page = bufmgr::BufferGetBlockNumber(newbuf);

        // Init the new page, link the previous buffer to it, commit, unlock
        // the previous buffer and prepare the new one
        break xp.link_new_page(index, newbuf, 0, xcx.mcx())?;
    };

    // Add to next offset
    if xp.add_item(item).is_none() {
        return Err(elog_error(
            format!("failed to add index item to \"{}\"", index.name()),
            "ivfinsert.c",
            177,
            "InsertTuple",
        ));
    }

    xp.commit()?;

    // Update the insert page
    if insert_page != original_insert_page {
        ivfflat_update_list(
            index,
            list_info,
            insert_page,
            original_insert_page,
            InvalidBlockNumber,
            ForkNumber::MAIN_FORKNUM,
        )?;
    }
    Ok(())
}

// ivfflatinsert (ivfinsert.c:186-222).
pub fn ivfflatinsert<'mcx>(
    _mcx: Mcx<'mcx>,
    index: &Relation<'mcx>,
    values: &[Datum],
    isnull: &[bool],
    heap_tid: &ItemPointerData,
    _heap: &Relation<'mcx>,
) -> PgResult<bool> {
    // Skip nulls
    if isnull.first().copied().unwrap_or(true) {
        return Ok(false);
    }

    // Use memory context since detoast, IvfflatNormValue, and
    // index_form_tuple can allocate
    let insert_ctx = mcx::MemoryContext::new("Ivfflat insert temporary context");

    // Insert tuple
    insert_tuple(index, values, isnull, heap_tid, insert_ctx.mcx())?;

    Ok(false)
}
```

- [ ] **Step 4: `ivfscan.c`**

`crates/contrib/pgvector_ivfflat/src/scan.rs`:
```rust
//! ivfscan.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, UnlockReleaseBuffer, BUFFER_LOCK_SHARE};
use datum::Datum;
use mcx::{Mcx, PgBox};
use types_core::{ForkNumber, InvalidBlockNumber, FLOAT8OID, TIDOID};
use types_error::PgResult;
use types_ivfflat::*;
use types_rel::Relation;
use types_relscan::{relation_get_index_scan, IndexScanDescData, IndexScanOpaque};
use types_scan::scankey::{ScanKeyData, SK_ISNULL};
use types_snapshot::IsMVCCSnapshot;
use types_storage::buf::BufferAccessStrategyType;
use types_storage::storage::ReadBufferMode;
use types_tuple::itemptr::ItemPointerData;

use crate::sort::{HeapSort, FLOAT8_LESS_OPERATOR};
use crate::utils::*;

fn opaque<'a>(scan: &'a mut IndexScanDescData<'_>) -> &'a mut IvfflatScanOpaqueData {
    match &mut scan.opaque {
        IndexScanOpaque::Ivfflat(so) => so,
        _ => unreachable!("non-ivfflat scan opaque"),
    }
}

// so->sortstate.
fn scan_sort(so: &mut IvfflatScanOpaqueData) -> &mut HeapSort {
    so.sortstate
        .as_mut()
        .and_then(|s| s.downcast_mut::<HeapSort>())
        .expect("ivfflat scan sort state")
}

// so->distfunc: ZeroDistance (ivfscan.c:192-196) for a NULL query, else
// FunctionCall2Coll with the index's distance proc.
fn scan_distance(so: &mut IvfflatScanOpaqueData, a: Datum, b: Datum) -> PgResult<f64> {
    if so.zero_distance {
        return Ok(0.0);
    }
    Ok(types_fmgr::function_call2_coll(&mut so.procinfo, so.collation, a, b)?.as_f64())
}

// The query image as a Datum; 0 for a NULL query, which ZeroDistance never
// reads.
fn value_datum(value: &Option<Vec<u8>>) -> Datum {
    value.as_ref().map_or(Datum::from_usize(0), |v| Datum::from_usize(v.as_ptr() as usize))
}

// GetScanLists (ivfscan.c:47-118): the nearest max_probes lists, nearest
// first in so->listPages.
fn get_scan_lists(index: &Relation<'_>, so: &mut IvfflatScanOpaqueData, value: Datum) -> PgResult<()> {
    let mut nextblkno = IVFFLAT_HEAD_BLKNO;
    let mut list_count: i32 = 0;
    let mut max_distance = f64::MAX;

    // Search all list pages
    while nextblkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, nextblkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let maxoffno = page_max_offset(cpage);

        for offno in FIRST_OFFSET_NUMBER..=maxoffno {
            let list = page_item(cpage, offno);

            // Use procinfo from the index instead of scan key for performance
            let center = Datum::from_usize(list_center(list).as_ptr() as usize);
            let distance = scan_distance(so, center, value)?;

            if list_count < so.max_probes {
                list_count += 1;

                // Add to heap
                so.list_queue.add(IvfflatScanList { start_page: list_start_page(list), distance });

                // Calculate max distance
                if list_count == so.max_probes {
                    max_distance = so.list_queue.first().expect("queued lists").distance;
                }
            } else if distance < max_distance {
                // Remove
                so.list_queue.remove_first();

                // Reuse
                so.list_queue.add(IvfflatScanList { start_page: list_start_page(list), distance });

                // Update max distance
                max_distance = so.list_queue.first().expect("queued lists").distance;
            }
        }

        nextblkno = page_nextblkno(cpage);
        UnlockReleaseBuffer(cbuf)?;
    }

    for i in (0..list_count as usize).rev() {
        so.list_pages[i] = so.list_queue.remove_first().expect("a queued list").start_page;
    }

    debug_assert!(so.list_queue.is_empty());
    Ok(())
}

// GetScanItems (ivfscan.c:123-187): the items of the next `probes` lists,
// sorted by distance.
fn get_scan_items(index: &Relation<'_>, so: &mut IvfflatScanOpaqueData, value: Datum) -> PgResult<()> {
    let mut batch_probes = 0;

    scan_sort(so).reset()?;

    // Search closest probes lists. C's `(++batchProbes) <= so->probes` runs
    // only when the first test passes.
    while so.list_index < so.max_probes && {
        batch_probes += 1;
        batch_probes <= so.probes
    } {
        let mut search_page = so.list_pages[so.list_index as usize];
        so.list_index += 1;

        // Search all entry pages for list
        while search_page != InvalidBlockNumber {
            let buf = bufmgr::ReadBufferExtended(
                index,
                ForkNumber::MAIN_FORKNUM,
                search_page,
                ReadBufferMode::Normal,
                so.bas.clone(),
            )?;
            LockBuffer(buf, BUFFER_LOCK_SHARE)?;
            let page = buffer_page(buf);
            let maxoffno = page_max_offset(page);

            for offno in FIRST_OFFSET_NUMBER..=maxoffno {
                let itup = page_item(page, offno).as_ptr();
                let mut isnull = false;
                // SAFETY: an index tuple on a share-locked page; one column.
                let datum = unsafe { nbtree::itup::index_getattr(itup, 1, &index.rd_att, &mut isnull) };

                // Add virtual tuple: (distance, &itup->t_tid). Use procinfo
                // from the index instead of scan key for performance
                let distance = scan_distance(so, datum, value)?;
                scan_sort(so).put(&[Datum::from_f64(distance), Datum::from_usize(itup as usize)])?;
            }

            search_page = page_nextblkno(page);
            UnlockReleaseBuffer(buf)?;
        }
    }

    scan_sort(so).perform()
}

// GetScanValue (ivfscan.c:198-236). C uses the scan key's value in place
// (it asserts that value is neither compressed nor toasted); this keeps a
// detoasted copy, as hnsw's scan does.
fn get_scan_value(so: &mut IvfflatScanOpaqueData, orderby: &ScanKeyData) -> PgResult<Option<Vec<u8>>> {
    if orderby.sk_flags & SK_ISNULL != 0 {
        so.zero_distance = true;
        return Ok(None);
    }
    so.zero_distance = false;

    let tmp = mcx::MemoryContext::new_bump("Ivfflat scan value");
    let mut value = detoast_value(tmp.mcx(), orderby.sk_argument)?;

    // Normalize if needed
    if so.normprocinfo.is_some() {
        value = ivfflat_norm_value(so.type_info, so.collation, tmp.mcx(), &value)?;
    }

    // Check dimensions match index
    ivfflat_check_dim(so.dimensions, so.type_info, so.collation, Datum::from_usize(value.as_ptr() as usize))?;

    Ok(Some(value.to_vec()))
}

// ivfflatbeginscan (ivfscan.c:252-333).
pub fn ivfflatbeginscan<'mcx>(
    mcx: Mcx<'mcx>,
    index: &Relation<'mcx>,
    nkeys: i32,
    norderbys: i32,
) -> PgResult<IndexScanDescData<'mcx>> {
    let mut probes = guc_tables::vars::ivfflat_probes.read();

    // Get lists and dimensions from metapage
    let (lists, dimensions) = ivfflat_get_meta_page_info(index)?;

    let mut max_probes = if guc_tables::vars::ivfflat_iterative_scan.read() != IVFFLAT_ITERATIVE_SCAN_OFF {
        guc_tables::vars::ivfflat_max_probes.read().max(probes)
    } else {
        probes
    };

    if probes > lists {
        probes = lists;
    }

    if max_probes > lists {
        max_probes = lists;
    }

    let so = IvfflatScanOpaqueData {
        type_info: ivfflat_get_type_info(index)?,
        probes,
        max_probes,
        dimensions,
        first: true,
        value: None,
        zero_distance: false,
        // Set support functions
        procinfo: index_getprocinfo(index, IVFFLAT_DISTANCE_PROC)?,
        normprocinfo: ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?,
        collation: index_collation(index),
        // Reuse same set of shared buffers for scan
        bas: bufmgr::GetAccessStrategy(BufferAccessStrategyType::BasBulkread),
        list_queue: new_scan_list_heap(),
        list_pages: vec![InvalidBlockNumber; max_probes as usize],
        list_index: 0,
        // Prep sort: (distance float8, heaptid tid), by distance
        sortstate: Some(Box::new(HeapSort::begin(
            "Ivfflat scan temporary context",
            &[("distance", FLOAT8OID), ("heaptid", TIDOID)],
            FLOAT8_LESS_OPERATOR,
            init_small::globals::work_mem(),
        )?)),
    };

    relation_get_index_scan(
        mcx,
        index,
        nkeys,
        norderbys,
        IndexScanOpaque::Ivfflat(PgBox::new_in(so, mcx)),
        xact::TransactionStartedDuringRecovery(),
    )
}

// ivfflatrescan (ivfscan.c:335-358).
pub fn ivfflatrescan(
    scan: &mut IndexScanDescData<'_>,
    keys: Option<&[ScanKeyData]>,
    orderbys: Option<&[ScanKeyData]>,
) -> PgResult<()> {
    {
        let so = opaque(scan);
        so.first = true;
        so.list_queue.reset();
        so.list_index = 0;
        so.value = None;
    }

    let nkeys = scan.numberOfKeys;
    let norderbys = scan.numberOfOrderBys;
    if let Some(keys) = keys {
        if nkeys > 0 {
            for (dst, src) in scan.keyData.iter_mut().zip(keys.iter()) {
                *dst = src.clone();
            }
        }
    }
    if let Some(orderbys) = orderbys {
        if norderbys > 0 {
            for (dst, src) in scan.orderByData.iter_mut().zip(orderbys.iter()) {
                *dst = src.clone();
            }
        }
    }
    Ok(())
}

// ivfflatgettuple (ivfscan.c:360-417). The index can be scanned backward,
// but Postgres doesn't support backward scan on operators.
pub fn ivfflatgettuple(scan: &mut IndexScanDescData<'_>, _direction: types_scan::ScanDirection) -> PgResult<bool> {
    let index = scan.indexRelation.as_ref().expect("index open").alias();

    if opaque(scan).first {
        // Count index scan for stats
        scan.xs_pgstat_index_scans += 1;
        scan.xs_nsearches += 1;

        // Safety check
        if scan.numberOfOrderBys == 0 || scan.orderByData.is_empty() {
            return Err(elog_error("cannot scan ivfflat index without order", "ivfscan.c", 389, "ivfflatgettuple"));
        }

        // Requires MVCC-compliant snapshot as not able to pin during sorting
        if let Some(snap) = scan.xs_snapshot.as_ref() {
            if !IsMVCCSnapshot(snap) {
                return Err(elog_error(
                    "non-MVCC snapshots are not supported with ivfflat",
                    "ivfscan.c",
                    394,
                    "ivfflatgettuple",
                ));
            }
        }

        let orderby = scan.orderByData[0].clone();
        let so = opaque(scan);
        let value = get_scan_value(so, &orderby)?;
        let d = value_datum(&value);
        get_scan_lists(&index, so, d)?;
        get_scan_items(&index, so, d)?;
        so.first = false;
        // The image does not move with the Vec, so d stays valid.
        so.value = value;
    }

    let so = opaque(scan);
    let mut values = [Datum::from_usize(0); 2];
    while !scan_sort(so).next(&mut values)? {
        if so.list_index == so.max_probes {
            return Ok(false);
        }
        let d = value_datum(&so.value);
        get_scan_items(&index, so, d)?;
    }

    // SAFETY: the sort's TID column, 6 bytes at a by-reference datum.
    let heaptid = unsafe { core::ptr::read_unaligned(values[1].as_usize() as *const ItemPointerData) };

    scan.xs_heaptid = heaptid;
    scan.xs_recheck = false;
    scan.xs_recheckorderby = false;
    // The order-by value is not returned (amcanorderbyop, distance only).
    for v in scan.xs_orderbynulls.iter_mut() {
        *v = true;
    }
    Ok(true)
}

// ivfflatendscan (ivfscan.c:419-434): tuplesort_end, then the scan's memory.
pub fn ivfflatendscan(scan: &mut IndexScanDescData<'_>) -> PgResult<()> {
    let so = opaque(scan);
    so.sortstate = None;
    so.value = None;
    Ok(())
}
```

- [ ] **Step 5: `ivfvacuum.c`**

`crates/contrib/pgvector_ivfflat/src/vacuum.rs`:
```rust
//! ivfvacuum.c (pgvector 0.8.7).

use bufmgr::{LockBuffer, LockBufferForCleanup, UnlockReleaseBuffer, BUFFER_LOCK_SHARE};
use nbtree::IndexVacuumInfo;
use types_core::{BlockNumber, ForkNumber, InvalidBlockNumber};
use types_error::PgResult;
use types_ivfflat::*;
use types_nbtree::genam::IndexBulkDeleteResult;
use types_storage::buf::BufferAccessStrategyType;
use types_storage::storage::ReadBufferMode;
use types_tuple::itemptr::{ItemPointerData, ItemPointerGetBlockNumberNoCheck, ItemPointerGetOffsetNumberNoCheck};

use crate::utils::*;

fn tid_cmp(a: &ItemPointerData, b: &ItemPointerData) -> core::cmp::Ordering {
    (ItemPointerGetBlockNumberNoCheck(a), ItemPointerGetOffsetNumberNoCheck(a))
        .cmp(&(ItemPointerGetBlockNumberNoCheck(b), ItemPointerGetOffsetNumberNoCheck(b)))
}

// The callback lazy vacuum passes C's ambulkdelete: is this heap TID dead?
fn tid_reaped(dead_items: &[ItemPointerData], tid: &ItemPointerData) -> bool {
    dead_items.binary_search_by(|probe| tid_cmp(probe, tid)).is_ok()
}

// ivfflatbulkdelete (ivfvacuum.c:15-143). pgrust hands ambulkdelete the
// sorted dead-TID array its callback tests (indexam's split).
pub fn ivfflatbulkdelete<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    istat: Option<IndexBulkDeleteResult>,
    dead_items: &[ItemPointerData],
) -> PgResult<IndexBulkDeleteResult> {
    bulk_delete(info, istat, &mut |tid| Ok(tid_reaped(dead_items, tid)))
}

// validate_index's ambulkdelete: the same walk with a collect-only callback
// (validate_index_callback, index.c) that records every heap TID and deletes
// nothing.
pub fn ivfflatbulkdelete_collect<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    callback: &mut (dyn FnMut(&ItemPointerData) -> PgResult<()> + '_),
) -> PgResult<IndexBulkDeleteResult> {
    bulk_delete(info, None, &mut |tid| {
        callback(tid)?;
        Ok(false)
    })
}

// ivfflatbulkdelete's body over its callback.
fn bulk_delete(
    info: &IndexVacuumInfo<'_, '_>,
    istat: Option<IndexBulkDeleteResult>,
    callback: &mut dyn FnMut(&ItemPointerData) -> PgResult<bool>,
) -> PgResult<IndexBulkDeleteResult> {
    let index = info.index;
    let mut blkno = IVFFLAT_HEAD_BLKNO;
    let bas = bufmgr::GetAccessStrategy(BufferAccessStrategyType::BasBulkread);
    let mut stats = istat.unwrap_or_default();
    let xcx = mcx::MemoryContext::new("Ivfflat vacuum xlog");

    // Iterate over list pages
    while blkno != InvalidBlockNumber {
        let cbuf = bufmgr::ReadBuffer(index, blkno)?;
        LockBuffer(cbuf, BUFFER_LOCK_SHARE)?;
        let cpage = buffer_page(cbuf);
        let cmaxoffno = page_max_offset(cpage);

        // Iterate over lists
        let list_pages: Vec<BlockNumber> =
            (FIRST_OFFSET_NUMBER..=cmaxoffno).map(|o| list_start_page(page_item(cpage, o))).collect();

        let list_blkno = blkno;
        blkno = page_nextblkno(cpage);

        UnlockReleaseBuffer(cbuf)?;

        for (coffno, &start_page) in (FIRST_OFFSET_NUMBER..).zip(list_pages.iter()) {
            let mut search_page = start_page;
            let mut insert_page = InvalidBlockNumber;

            // Iterate over entry pages
            while search_page != InvalidBlockNumber {
                vacuum_seams::vacuum_delay_point::call(false)?;

                let buf = bufmgr::ReadBufferExtended(
                    index,
                    ForkNumber::MAIN_FORKNUM,
                    search_page,
                    ReadBufferMode::Normal,
                    bas.clone(),
                )?;

                // ambulkdelete cannot delete entries from pages that are
                // pinned by other backends
                LockBufferForCleanup(buf)?;

                let mut xp = XlogPage::register(index, buf, 0, xcx.mcx())?;
                let maxoffno = page_max_offset(xp.page());
                let mut deletable: Vec<u16> = Vec::new();

                // Find deleted tuples
                for offno in FIRST_OFFSET_NUMBER..=maxoffno {
                    // SAFETY: t_tid is the first 6 bytes of the index tuple.
                    let htup = unsafe {
                        core::ptr::read_unaligned(page_item(xp.page(), offno).as_ptr() as *const ItemPointerData)
                    };
                    if callback(&htup)? {
                        deletable.push(offno);
                        stats.tuples_removed += 1.0;
                    } else {
                        stats.num_index_tuples += 1.0;
                    }
                }

                // Set to first free page
                // Must be set before searchPage is updated
                if insert_page == InvalidBlockNumber && !deletable.is_empty() {
                    insert_page = search_page;
                }

                search_page = page_nextblkno(xp.page());

                if !deletable.is_empty() {
                    // Delete tuples
                    xp.index_multi_delete(&deletable);
                    xp.commit()?;
                } else {
                    xp.abort()?;
                }
            }

            // Update after all tuples deleted. We don't add or delete items
            // from lists pages, so offset won't change.
            if insert_page != InvalidBlockNumber {
                ivfflat_update_list(
                    index,
                    ListInfo { blkno: list_blkno, offno: coffno },
                    insert_page,
                    InvalidBlockNumber,
                    InvalidBlockNumber,
                    ForkNumber::MAIN_FORKNUM,
                )?;
            }
        }
    }

    bufmgr::FreeAccessStrategy(bas);
    Ok(stats)
}

// ivfflatvacuumcleanup (ivfvacuum.c:145-164).
pub fn ivfflatvacuumcleanup<'mcx>(
    info: &IndexVacuumInfo<'_, 'mcx>,
    istat: Option<IndexBulkDeleteResult>,
) -> PgResult<Option<IndexBulkDeleteResult>> {
    if info.analyze_only {
        return Ok(istat);
    }

    // stats is NULL if ambulkdelete not called
    // OK to return NULL if index not changed
    let Some(mut stats) = istat else {
        return Ok(None);
    };

    stats.num_pages = bufmgr::RelationGetNumberOfBlocksInFork(info.index, ForkNumber::MAIN_FORKNUM)?;
    Ok(Some(stats))
}
```

- [ ] **Step 6: The crate root and the GUC storage**

Replace `crates/contrib/pgvector_ivfflat/src/lib.rs` with:
```rust
//! pgvector 0.8.7 ivfflat AM (ivfflat.c, ivfutils.c, ivfinsert.c,
//! ivfscan.c, ivfvacuum.c) for vector, halfvec and bit; the build
//! (ivfbuild.c, ivfkmeans.c) lives in pgvector_ivfflat_build. Support proc 5
//! resolves by its C symbol to ivfutils.c's static type info
//! (utils.rs ivfflat_get_type_info). Elsewhere: ivfflat.c's cost estimate is
//! planner selfuncs.rs ivfflatcostestimate, its reloptions are reloptions'
//! ivfflatoptions, and its build phase names are adt_amutils'.
//! DIVERGENCES (recorded): no parallel build (M6); an unknown support proc 5
//! raises where C would call it; the scan keeps a detoasted copy of the query
//! where C uses it in place; an insert into a list with no insert page raises
//! where C asserts.

pub mod insert;
pub mod scan;
pub mod sort;
pub mod utils;
pub mod vacuum;

pub use insert::ivfflatinsert;
pub use scan::{ivfflatbeginscan, ivfflatendscan, ivfflatgettuple, ivfflatrescan};
pub use vacuum::{ivfflatbulkdelete, ivfflatbulkdelete_collect, ivfflatvacuumcleanup};

use types_core::Oid;
use types_error::PgResult;

// ivfflatvalidate (ivfflat.c:172-176).
pub fn ivfflatvalidate(_opclassoid: Oid) -> PgResult<bool> {
    Ok(true)
}

use std::cell::Cell;
thread_local! {
    static PROBES: Cell<i32> = const { Cell::new(1) };
    static ITERATIVE_SCAN: Cell<i32> = const { Cell::new(0) };
    static MAX_PROBES: Cell<i32> = const { Cell::new(32768) };
}

// ivfflat.* GUC backing (C's ivfflat_probes &co globals, ivfflat.c:24-26).
pub fn init_seams() {
    use guc_tables::GucVarAccessors;
    guc_tables::vars::ivfflat_probes.install_if_absent(GucVarAccessors {
        get: || PROBES.with(|c| c.get()),
        set: |v| PROBES.with(|c| c.set(v)),
    });
    guc_tables::vars::ivfflat_iterative_scan.install_if_absent(GucVarAccessors {
        get: || ITERATIVE_SCAN.with(|c| c.get()),
        set: |v| ITERATIVE_SCAN.with(|c| c.set(v)),
    });
    guc_tables::vars::ivfflat_max_probes.install_if_absent(GucVarAccessors {
        get: || MAX_PROBES.with(|c| c.get()),
        set: |v| MAX_PROBES.with(|c| c.set(v)),
    });
}
```

In `crates/_support/seams_init/Cargo.toml`, add after the `pgvector_hnsw` line:
```toml
pgvector_ivfflat = { path = "../../contrib/pgvector_ivfflat" }
```
In `crates/_support/seams_init/src/lib.rs`, add `pgvector_ivfflat::init_seams();` after `pgvector_hnsw::init_seams();`.

- [ ] **Step 7: Build and run the tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
UT ivf_ block_sampler reservoir_
UT_KEEP
UT_CORE ivf_ amsearcharray_is_btree_only table_counts_match_compiled_backend_shape
```
Expected:
- `Cargo.lock` changes only `pgvector_ivfflat`'s, `types_relscan`'s and `seams_init`'s dependency lists.
- `UT`: 19 passed (as Task 2).
- `UT_KEEP`: all pass.
- `UT_CORE`: 3 passed: `ivf_am_properties_match_ivfflathandler`, `amsearcharray_is_btree_only` and `table_counts_match_compiled_backend_shape`.

- [ ] **Step 8: Nothing else moved**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust
printf "SELECT '[1]'::vector;\nSHOW ivfflat.probes;\nSHOW ivfflat.iterative_scan;\nSHOW ivfflat.max_probes;\nSET ivfflat.probes = 10;\nSHOW ivfflat.probes;\n" | scripts/pgvector/sqldiff.sh pgrust -
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- regress 11/14, the same 11 as before: the `ivfflat_*` files still fail on `access method "ivfflat" does not exist`.
- sqldiff identical: `[1]`, then `1`, `off`, `32768` and `10`. C defines its GUCs when a session first loads the library (`_PG_init`); the `SELECT` loads it, because sqldiff creates the extension in another session. pgrust's are static.
- Both lints PASS.

- [ ] **Step 9: Commit**

```bash
git add Cargo.lock crates/backend/access/index/relscan crates/contrib/pgvector_ivfflat \
  crates/backend/utils/misc/guc_tables/src crates/_support/seams_init
git commit -q -F - <<'EOF'
pgvector: ivfinsert.c, ivfscan.c, ivfvacuum.c and the ivfflat.* settings

IndexAmKind::Ivfflat with ivfflathandler's properties and an
IndexScanOpaque arm; the insert (nearest list, page chain, extension
lock, IvfflatUpdateList), the scan (pairing-heap list choice, per-batch
heap tuplesort, iterative probes) and the vacuum (cleanup locks, insert
page reset); ivfflat.probes, ivfflat.iterative_scan and
ivfflat.max_probes, with their storage installed by seams_init.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 4: The core hook points

**Files:**
- Modify: `crates/backend/access/index/amapi/Cargo.toml`, `src/lib.rs`, `src/tests.rs`
- Modify: `crates/backend/access/index/indexam/Cargo.toml`, `src/lib.rs`
- Modify: `crates/backend/catalog/catalog_index/Cargo.toml`, `src/lib.rs`
- Modify: `crates/backend/optimizer/plan/planner/Cargo.toml`, `src/plancat.rs`, `src/selfuncs.rs`
- Modify: `crates/backend/utils/cache/relcache_build/src/index.rs`
- Modify: `crates/backend/access/common/reloptions/src/lib.rs`
- Modify: `crates/backend/utils/cache/lsyscache/src/amop.rs`
- Modify: `crates/backend/utils/adt/adt_amutils/src/lib.rs`, `src/tests.rs`
- Modify: `crates/backend/access/heap/vacuumlazy/src/morsels.rs`
- Modify: `crates/_support/types/types_error/src/source_map_table.rs`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes Task 2's `pgvector_ivfflat_build::{ivfflatbuild, ivfflatbuildempty}` and Task 3's `pgvector_ivfflat::*` entry points, `ivfflatvalidate` and `utils::ivfflat_get_meta_page_info`.
- Produces `reloptions::RELOPT_KIND_IVFFLAT` (`1 << 14`) and `ivfflatoptions`.
- Produces `fn ivfflatcostestimate` (planner `selfuncs.rs`) and `fn ivfflat_phasename` (`adt_amutils`).

Most of the matches this task extends end in an `#[allow(unreachable_patterns)] _ => ...` arm, which compiles but misroutes without these arms: `catalog_index`'s build falls to `gistbuild`, and `amparallelvacuumoptions` falls to `VACUUM_OPTION_NO_PARALLEL`. Add every arm below, even though nothing fails to compile without it.

- [ ] **Step 1: `amapi`**

In `crates/backend/access/index/amapi/Cargo.toml`, add after the `pgvector_hnsw` line:
```toml
pgvector_ivfflat = { path = "../../../../contrib/pgvector_ivfflat" }
```
In `crates/backend/access/index/amapi/src/lib.rs`:
- In `resolve_extension_handler`, add `b"ivfflathandler" => return Ok(IndexAmKind::Ivfflat),` after the `b"hnswhandler"` arm. This is also `CREATE ACCESS METHOD`'s gate (`amcmds/src/lib.rs:204`).
- Add an `IndexAmKind::Ivfflat` arm after each `IndexAmKind::Hnsw` arm:
  - `IndexAmTranslateStrategy`: `IndexAmKind::Ivfflat => COMPARE_INVALID,`
  - `IndexAmTranslateCompareType`: `IndexAmKind::Ivfflat => InvalidStrategy,`
  - `amvalidate`: `IndexAmKind::Ivfflat => pgvector_ivfflat::ivfflatvalidate(opclassoid),`
  - `am_adjust_members`: `IndexAmKind::Ivfflat => Ok(()),` (`amadjustmembers = NULL`)
- In `amparallelvacuumoptions`, replace `IndexAmKind::Hash | IndexAmKind::Hnsw => VACUUM_OPTION_PARALLEL_BULKDEL,` with `IndexAmKind::Hash | IndexAmKind::Hnsw | IndexAmKind::Ivfflat => VACUUM_OPTION_PARALLEL_BULKDEL,` (`ivfflat.c:212`).

In `crates/backend/access/index/amapi/src/tests.rs`, `handler_dispatch_is_the_closed_set`:
- add `5004 => Some(named("ivfflathandler")),` to the `pg_proc_proname` stub;
- after `assert_eq!(GetIndexAmRoutine(5000).unwrap(), IndexAmKind::Bloom);` add `assert_eq!(GetIndexAmRoutine(5004).unwrap(), IndexAmKind::Ivfflat);`.

- [ ] **Step 2: `indexam`**

In `crates/backend/access/index/indexam/Cargo.toml`, add after the `pgvector_hnsw` line:
```toml
pgvector_ivfflat = { path = "../../../../contrib/pgvector_ivfflat" }
```
In `crates/backend/access/index/indexam/src/lib.rs`, add after each `Hnsw` arm:

| function | the `Ivfflat` arm |
|---|---|
| `index_bulk_delete` | `IndexAmKind::Ivfflat => pgvector_ivfflat::ivfflatbulkdelete(info, istat, dead_items),` |
| `index_bulk_delete_collect` | `IndexAmKind::Ivfflat => pgvector_ivfflat::ivfflatbulkdelete_collect(info, callback),` |
| `index_vacuum_cleanup` | `IndexAmKind::Ivfflat => pgvector_ivfflat::ivfflatvacuumcleanup(info, istat),` |
| `am_getbitmap` | `IndexScanOpaque::Ivfflat(_) => Err(missing_procedure("amgetbitmap", scan.index_rel())),` |
| `am_beginscan` | `IndexAmKind::Ivfflat => pgvector_ivfflat::ivfflatbeginscan(mcx, indexRelation, nkeys, norderbys),` |
| `am_rescan` | `IndexScanOpaque::Ivfflat(_) => pgvector_ivfflat::ivfflatrescan(scan, keys, orderbys),` |
| `am_endscan` | `IndexScanOpaque::Ivfflat(_) => pgvector_ivfflat::ivfflatendscan(scan),` |
| `am_markpos` | `IndexScanOpaque::Ivfflat(_) => unreachable!("has_ammarkpos gate"),` |
| `am_restrpos` | `IndexScanOpaque::Ivfflat(_) => unreachable!("has_amrestrpos gate"),` |
| `am_gettuple` | `IndexScanOpaque::Ivfflat(_) => pgvector_ivfflat::ivfflatgettuple(scan, direction),` |
| `am_insert_cleanup` | `IndexAmKind::Ivfflat => unreachable!("ivfflat lacks aminsertcleanup (guarded)"),` |

In `am_insert`, after the `IndexAmKind::Hnsw => { ... }` arm, add:
```rust
        IndexAmKind::Ivfflat => {
            debug_assert!(checkUnique == IndexUniqueCheck::UNIQUE_CHECK_NO);
            pgvector_ivfflat::ivfflatinsert(mcx, indexRelation, values, isnull, heap_t_ctid, heapRelation)
        }
```

- [ ] **Step 3: `catalog_index`**

In `crates/backend/catalog/catalog_index/Cargo.toml`, add after the `pgvector_hnsw_build` line:
```toml
pgvector_ivfflat_build = { path = "../../../contrib/pgvector_ivfflat_build" }
```
In `crates/backend/catalog/catalog_index/src/lib.rs`:
- In the build dispatch, after the `types_relscan::IndexAmKind::Hnsw => { ... }` arm, add:
  ```rust
          types_relscan::IndexAmKind::Ivfflat => {
              let r = pgvector_ivfflat_build::ivfflatbuild(mcx, heapRelation, indexRelation, indexInfo)?;
              (r.heap_tuples, r.index_tuples)
          }
  ```
- In the build-empty dispatch, after `types_relscan::IndexAmKind::Hnsw => pgvector_hnsw_build::hnswbuildempty(indexRelation)?,`, add:
  ```rust
                  types_relscan::IndexAmKind::Ivfflat => pgvector_ivfflat_build::ivfflatbuildempty(indexRelation)?,
  ```

- [ ] **Step 4: The planner**

In `crates/backend/optimizer/plan/planner/Cargo.toml`, add after the `pgvector_hnsw` line:
```toml
pgvector_ivfflat = { path = "../../../../contrib/pgvector_ivfflat" }
```
In `crates/backend/optimizer/plan/planner/src/plancat.rs`:
- After `let am_is_hnsw = am_kind == types_relscan::IndexAmKind::Hnsw;`, add `let am_is_ivfflat = am_kind == types_relscan::IndexAmKind::Ivfflat;`.
- Replace `info.amcanorderbyop = am_is_gist || am_is_spgist || am_is_hnsw;` with `info.amcanorderbyop = am_is_gist || am_is_spgist || am_is_hnsw || am_is_ivfflat;`.
- In the `info.amoptionalkey = ...` expression, replace `|| am_is_hnsw` with `|| am_is_hnsw\n                    || am_is_ivfflat` (a new line, same indentation).
- Replace `info.amhasgetbitmap = !am_is_hnsw;` with `info.amhasgetbitmap = !am_is_hnsw && !am_is_ivfflat;`.

The other flags already give `ivfflathandler`'s values: `amsearcharray`, `amsearchnulls`, `amcanparallel` and `amcanmarkpos` are false, `amhasgettuple` is true, and `canreturn` and the tree height take their `_` arms.

In `crates/backend/optimizer/plan/planner/src/selfuncs.rs`:
- In the `amcostestimate` dispatch, after `types_relscan::IndexAmKind::Hnsw => hnswcostestimate(run, path_id, loop_count),`, add `types_relscan::IndexAmKind::Ivfflat => ivfflatcostestimate(run, path_id, loop_count),`.
- After `fn hnswcostestimate`, add:
```rust

// ivfflatcostestimate (pgvector ivfflat.c:82-151). genericcostestimate
// keeps its spc_random_page_cost to itself here, so the tablespace's page
// costs are read again (C's costs.spc_random_page_cost and
// get_tablespace_page_costs(reltablespace, NULL, &spc_seq_page_cost)).
fn ivfflatcostestimate(
    run: &mut PlannerRun<'_>,
    path_id: types_pathnodes::PathId,
    loop_count: f64,
) -> PgResult<AmCostEstimate> {
    let (has_orderbys, indexoid, reltablespace, rel_pages) = {
        let PathNode::IndexPath(ip) = run.root.path(path_id) else {
            panic!("ivfflatcostestimate: not an IndexPath")
        };
        let index = ip.indexinfo.as_ref().expect("indexinfo set");
        let rel_pages = index
            .rel
            .as_ref()
            .map(|r| run.root.rel(*r).pages as f64)
            .unwrap_or(0.0);
        (!ip.indexorderbys.is_empty(), index.indexoid, index.reltablespace, rel_pages)
    };

    // Never use index without order
    if !has_orderbys {
        // See "On disable_cost" thread on pgsql-hackers
        run.root.path_mut(path_id).base_mut().disabled_nodes = 2;
        return Ok(AmCostEstimate {
            index_startup_cost: f64::INFINITY,
            index_total_cost: f64::INFINITY,
            index_selectivity: 0.0,
            index_correlation: 0.0,
            index_pages: 0.0,
        });
    }

    let mut costs = GenericCosts {
        num_index_tuples: 0.0,
        num_sa_scans: 1.0,
        index_startup_cost: 0.0,
        index_total_cost: 0.0,
        index_selectivity: 0.0,
        index_correlation: 0.0,
        num_index_pages: 0.0,
    };
    genericcostestimate(run, path_id, loop_count, &mut costs)?;

    let lists = {
        let mcx = run.mcx;
        let index_rel = indexam::index_open(mcx, indexoid, types_rel::NoLock)?;
        pgvector_ivfflat::utils::ivfflat_get_meta_page_info(&index_rel)?.0
    };

    // Get the ratio of lists that we need to visit
    let mut ratio = guc_tables::vars::ivfflat_probes.read() as f64 / lists as f64;
    if ratio > 1.0 {
        ratio = 1.0;
    }

    let (spc_random_page_cost, spc_seq_page_cost) =
        crate::costsize::get_tablespace_page_costs(run.mcx, reltablespace)?;
    let sequential_ratio = 0.5;

    // Change some page cost from random to sequential
    costs.index_total_cost -=
        sequential_ratio * costs.num_index_pages * (spc_random_page_cost - spc_seq_page_cost);

    // Startup cost is cost before returning the first row
    costs.index_startup_cost = costs.index_total_cost * ratio;

    // Adjust cost if needed since TOAST not included in seq scan cost
    let startup_pages = costs.num_index_pages * ratio;
    if startup_pages > rel_pages && ratio < 0.5 {
        // Change rest of page cost from random to sequential
        costs.index_startup_cost -=
            (1.0 - sequential_ratio) * startup_pages * (spc_random_page_cost - spc_seq_page_cost);

        // Remove cost of extra pages
        costs.index_startup_cost -= (startup_pages - rel_pages) * spc_seq_page_cost;
    }

    Ok(AmCostEstimate {
        index_startup_cost: costs.index_startup_cost,
        index_total_cost: costs.index_total_cost,
        index_selectivity: costs.index_selectivity,
        index_correlation: costs.index_correlation,
        index_pages: costs.num_index_pages,
    })
}
```

- [ ] **Step 5: `relcache_build`, `reloptions`, `lsyscache`, `vacuumlazy` and the source map**

In `crates/backend/utils/cache/relcache_build/src/index.rs`:
- In the `amsupport` match, after the `Hnsw => 3,` arm, add:
  ```rust
          // pgvector ivfflat: distance, norm, k-means distance, k-means norm,
          // type-info.
          types_relscan::IndexAmKind::Ivfflat => 5,
  ```
- In the no-slot-0-preload condition, add `|| am_kind == types_relscan::IndexAmKind::Ivfflat` after the `Hnsw` line.

In `crates/backend/access/common/reloptions/src/lib.rs`:
- In the `use ::types_rel::{...}` list, add `IvfflatOptions` after `HnswOptions`.
- After `pub const RELOPT_KIND_HNSW: relopt_kind = 1 << 12;` and the `RELOPT_KIND_BLOOM` line, add:
  ```rust
  // pgvector ivfflat: C add_reloption_kind() at module load; static here.
  pub const RELOPT_KIND_IVFFLAT: relopt_kind = 1 << 14;
  ```
- After the row `i("ef_construction", RELOPT_KIND_HNSW, AEL, 64, 4, 1000),`, add `i("lists", RELOPT_KIND_IVFFLAT, AEL, 100, 1, 32768),` (`ivfflat.c:41-43`).
- In the extension-handler dispatch, add `Some("ivfflathandler") => ivfflatoptions(mcx, options, validate),` after the `hnswhandler` arm.
- After `fn hnswoptions`, add:
  ```rust

  // ivfflatoptions (pgvector ivfflat.c:153-167).
  fn ivfflatoptions<'mcx>(
      mcx: Mcx<'mcx>,
      options: Option<&[u8]>,
      validate: bool,
  ) -> PgResult<Option<RdOptions>> {
      let values = parseRelOptions(mcx, options, validate, RELOPT_KIND_IVFFLAT)?;
      if values.is_empty() {
          return Ok(None);
      }
      let mut out = IvfflatOptions { lists: 100 };
      for v in values.iter() {
          match v.def.name {
              "lists" => out.lists = v.int_val(),
              other => {
                  if validate {
                      panic!("reloption \"{other}\" not found in parse table");
                  }
              }
          }
      }
      Ok(Some(RdOptions::Ivfflat(out)))
  }
  ```

In `crates/backend/utils/cache/lsyscache/src/amop.rs`, replace each of the four `Some(b"hnswhandler") | Some(b"blhandler")` patterns with `Some(b"hnswhandler") | Some(b"ivfflathandler") | Some(b"blhandler")`. They are in `index_am_translate_strategy`, `index_am_translate_cmptype`, `index_am_consistent_flags` and `get_opmethod_canorder`, and `ivfflathandler`'s values are theirs: no strategy translation, `amconsistentequality = amconsistentordering = amcanorder = false`.

In `crates/backend/access/heap/vacuumlazy/src/morsels.rs`, in `admit()`, add `| K::Ivfflat` after `| K::Hnsw`: its bulk delete and cleanup now go through `indexam`.

In `crates/_support/types/types_error/src/source_map_table.rs`, after the `("pgvector_hnsw", "{hnsw.c"),` row, add:
```rust
    ("pgvector_ivfflat", "ivfflat.c"),
    ("pgvector_ivfflat_build", "ivfbuild.c"),
```
The table's header says it is generated, but its generator is not in the repo; HNSW's row was added by hand. Every IVFFlat error that mirrors C carries an explicit location; these rows only name the file for the pgrust-only errors. The malformed `"{hnsw.c"` row is a core follow-up that waits for the user.

- [ ] **Step 6: `adt_amutils`**

In `crates/backend/utils/adt/adt_amutils/src/lib.rs`:
- After the `const HNSW: AmFlags = AmFlags { ... };` block, add an identical block named `IVFFLAT`. Every flag matches `ivfflathandler`: `amcanorderbyop` and `has_amgettuple` true, `has_ambuildphasename` true, the rest false.
- Add `Some(b"ivfflathandler") => Some(&IVFFLAT),` after `Some(b"hnswhandler") => Some(&HNSW),`.
- In `fc_pg_indexam_progress_phasename`, replace the arm `_ => match am_flags(amoid) {` and its body with an arm that tries ivfflat's handler first:
  ```rust
          other => match extension_am_handler(other).as_deref() {
              Some(b"ivfflathandler") => ivfflat_phasename(phasenum),
              _ => match am_flags(amoid) {
                  Some(_) => None,
                  None if is_index_am(amoid)? => {
                      // unported: out-of-core index AM build-phase-name lane.
                      return Err(Box::new(
                          PgError::error(format!(
                              "build phase names for non-core access method {amoid} \
                               are not yet implemented"
                          ))
                          .with_sqlstate(ERRCODE_FEATURE_NOT_SUPPORTED),
                      ));
                  }
                  None => None,
              },
          },
  ```
- After `fn gin_phasename`, add:
  ```rust

  // ivfflatbuildphasename (pgvector ivfflat.c:61-80).
  fn ivfflat_phasename(phasenum: i64) -> Option<&'static str> {
      match phasenum {
          1 => Some("initializing"),
          2 => Some("performing k-means"),
          3 => Some("assigning tuples"),
          4 => Some("loading tuples"),
          _ => None,
      }
  }
  ```
HNSW's build phase names stay unreported: a pre-existing gap the report records. C names `initializing` and `loading tuples` (`hnswbuildphasename`, `hnsw.c:117-129`).

In `crates/backend/utils/adt/adt_amutils/src/tests.rs`:
- In `install_amhandler_stub`, add `20002 => Some(9997),` to the `pg_am_amhandler` stub and `9997 => Some(mcx::PgString::from_str_in("ivfflathandler", mcx)?),` to the `lookup_pg_proc_prosrc` stub.
- At the end of `extension_am_identified_by_handler_prosrc`, add:
  ```rust
      let ivfflat = am_flags(20002).unwrap();
      assert!(ivfflat.amcanorderbyop && ivfflat.has_ambuildphasename && !ivfflat.amcanmulticol);
  ```
- At the end of `phasenames_match_c`, add:
  ```rust
      assert_eq!(ivfflat_phasename(1), Some("initializing"));
      assert_eq!(ivfflat_phasename(2), Some("performing k-means"));
      assert_eq!(ivfflat_phasename(3), Some("assigning tuples"));
      assert_eq!(ivfflat_phasename(4), Some("loading tuples"));
      assert_eq!(ivfflat_phasename(5), None);
  ```

- [ ] **Step 7: Build and run the tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
UT_CORE ivf_ amsearcharray_is_btree_only table_counts_match_compiled_backend_shape handler_dispatch_is_the_closed_set extension_am_identified_by_handler_prosrc phasenames_match_c overrides_sorted_unique_and_non_default
UT_KEEP
```
Expected:
- `Cargo.lock` changes only the dependency lists of `amapi`, `indexam`, `catalog_index` and `planner`.
- `UT_CORE`: 7 passed.
- `UT_KEEP`: all pass.

Then the server, which now links every IVFFlat crate:
```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected:
- regress 11/14, unchanged. The extension script has no `ivfflat` access method until Task 5, so this proves nothing else moved.
- Both lints PASS.

- [ ] **Step 8: Commit**

```bash
git add Cargo.lock crates/backend/access/index/amapi crates/backend/access/index/indexam \
  crates/backend/catalog/catalog_index crates/backend/optimizer/plan/planner \
  crates/backend/utils/cache/relcache_build/src/index.rs crates/backend/access/common/reloptions/src/lib.rs \
  crates/backend/utils/cache/lsyscache/src/amop.rs crates/backend/utils/adt/adt_amutils \
  crates/backend/access/heap/vacuumlazy/src/morsels.rs crates/_support/types/types_error/src/source_map_table.rs
git commit -q -F - <<'EOF'
pgvector: wire IVFFlat into core (spec §4.5)

ivfflathandler resolves to IndexAmKind::Ivfflat; indexam, catalog_index,
the planner (capabilities and ivfflatcostestimate), relcache's support
count, the lists reloption, lsyscache's AM arms, amutils' flags and
build-phase names, vacuum's morsel admission and the error source map
all gain their IVFFlat arms.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 5: The verbatim 0.8.7 extension, `_PG_init` and the library version

**Files:**
- Delete: `crates/contrib/pgvector/extension/vector--0.8.5.sql`
- Create: `crates/contrib/pgvector/extension/vector--0.8.7.sql` and the 41 `vector--A--B.sql` upgrade scripts (upstream copies)
- Modify: `crates/contrib/pgvector/extension/vector.control` (upstream copy)
- Modify: `crates/contrib/pgvector/src/lib.rs` (header, `lookup` stubs, `pg_init`, the library version, the script tests)
- Modify: `crates/backend/utils/fmgr/dfmgr/src/lib.rs` (`register_library_version`, `library_version`)
- Modify: `crates/backend/commands/extension/src/funcs.rs` (`pg_get_loaded_modules`)
- Modify: `crates/backend/main/main_main/build.rs` (prune stale staged files)

**Interfaces:**
- Produces `dfmgr::register_library_version(name: &'static str, version: &'static str)` and `dfmgr::library_version(name: &str) -> Option<&'static str>`.
- `pgvector`'s `lookup` resolves `ivfflathandler`, `ivfflat_halfvec_support` and `ivfflat_bit_support` to stubs that raise XX000.
- `CREATE EXTENSION vector` installs upstream's 0.8.7 script, so `ivfflat` and its seven opclasses exist from here on.

- [ ] **Step 1: The library-version registry (test first)**

In `crates/backend/utils/fmgr/dfmgr/src/lib.rs`'s `#[cfg(test)] mod tests`, add:
```rust
    // PG_MODULE_MAGIC_EXT's .version: what pg_get_loaded_modules reports.
    #[test]
    fn library_version_registers_and_defaults_to_none() {
        assert_eq!(library_version("tver"), None);
        register_library_version("tver", "1.2.3");
        assert_eq!(library_version("tver"), Some("1.2.3"));
        register_library_version("tver", "1.2.4");
        assert_eq!(library_version("tver"), Some("1.2.4"));
    }
```
Run `UT_CORE library_version`. Expected: FAIL to compile (`library_version` not found).

Then add, after `static BUILTIN_LIBRARIES ...`:
```rust

// PG_MODULE_MAGIC_EXT's .version (fmgr.h), the version pg_get_loaded_modules
// reports for a library. A builtin library that registers none reports
// PG_VERSION, as C's contrib modules (PG_MODULE_MAGIC_EXT(.version =
// PG_VERSION)) do.
static LIBRARY_VERSIONS: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());

pub fn register_library_version(name: &'static str, version: &'static str) {
    let mut versions = LIBRARY_VERSIONS.lock().unwrap();
    match versions.iter_mut().find(|(n, _)| *n == name) {
        Some(entry) => entry.1 = version,
        None => versions.push((name, version)),
    }
}

pub fn library_version(name: &str) -> Option<&'static str> {
    LIBRARY_VERSIONS.lock().unwrap().iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}
```
Run `UT_CORE library_version`. Expected: 1 passed.

In `crates/backend/commands/extension/src/funcs.rs`, replace the comment above `fc_pg_get_loaded_modules` with:
```rust
// pg_get_loaded_modules (extension.c:2974-3018). No-dlopen carve: modules are
// builtin registry entries. module_name is the entry's name; version is the
// one the library registered for PG_MODULE_MAGIC_EXT's .version (pgvector's
// 0.8.7, vector.c:49), else PG_VERSION, which every other shipped module's
// PG_MODULE_MAGIC_EXT carries. file_name is the basename of the filename the
// library was opened under (:3007-3011).
```
and replace `text_datum(mcx, guc_tables::consts::PG_COMPAT_VERSION)?,` with:
```rust
            text_datum(
                mcx,
                dfmgr::library_version(module.module_name).unwrap_or(guc_tables::consts::PG_COMPAT_VERSION),
            )?,
```

- [ ] **Step 2: `pgvector`: stubs, `_PG_init`, the version, the header**

In `crates/contrib/pgvector/src/lib.rs`:
- Replace the `//!` header lines 1–9 (through `//! reports PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49). Revisit in M4.`) with:
  ```rust
  //! pgvector (github.com/pgvector/pgvector), ported from 0.8.5 @ 159b79a and
  //! brought to 0.8.7 behavior: the vector, halfvec and sparsevec types and
  //! the bit distance functions (I/O, functions, aggregates, casts, btree
  //! opclasses), one module per C file. The access methods are the
  //! pgvector_hnsw and pgvector_ivfflat crates. extension/ holds upstream
  //! 0.8.7's files verbatim: vector--0.8.7.sql (its sql/vector.sql), the 41
  //! upgrade scripts and vector.control (spec §4.6). The library reports
  //! PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49) and reserves the hnsw and
  //! ivfflat GUC prefixes on load, as _PG_init does.
  ```
  The remaining `DIVERGENCE` lines stay.
- In `lookup`, add after `"hnsw_sparsevec_support" => fc_hnsw_sparsevec_support,`:
  ```rust
          "ivfflathandler" => fc_ivfflathandler,
          "ivfflat_halfvec_support" => fc_ivfflat_halfvec_support,
          "ivfflat_bit_support" => fc_ivfflat_bit_support,
  ```
- In the comment above `fc_hnswhandler`, replace `hnsw.c:266-401` with `hnsw.c:266-402` (M3's parked citation).
- After `fn fc_hnsw_sparsevec_support`, add:
  ```rust

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
  fn pg_init() -> types_error::PgResult<()> {
      guc::MarkGUCPrefixReserved("hnsw");
      guc::MarkGUCPrefixReserved("ivfflat");
      Ok(())
  }
  ```
- Replace `init_seams` with:
  ```rust
  pub fn init_seams() {
      dfmgr::register_builtin_library(dfmgr::BuiltinLibraryEntry {
          name: LIBRARY,
          lookup,
          pg_init: Some(pg_init),
      });
      // PG_MODULE_MAGIC_EXT(.name = "vector", .version = "0.8.7") (vector.c:49).
      dfmgr::register_library_version(LIBRARY, "0.8.7");
  }
  ```

- [ ] **Step 3: Ship upstream's extension files**

```bash
git rm -q crates/contrib/pgvector/extension/vector--0.8.5.sql
cp crates/pgvector-0.8.7-reference/sql/vector.sql crates/contrib/pgvector/extension/vector--0.8.7.sql
cp crates/pgvector-0.8.7-reference/sql/vector--*--*.sql crates/contrib/pgvector/extension/
cp crates/pgvector-0.8.7-reference/vector.control crates/contrib/pgvector/extension/vector.control
ls crates/contrib/pgvector/extension | wc -l
git -C crates/pgvector-0.8.7-reference status --porcelain --ignored .
```
Expected: 43 files (the control file, the install script and 41 upgrade scripts), and nothing printed for the reference tree.

Spec §4.6 names only the six 0.8.x upgrade scripts. All 41 ship because:
- that is what C installs (`DATA = $(wildcard sql/*--*--*.sql)`), so `pg_extension_update_paths` and `pg_available_extension_versions` match C;
- every C symbol they reference is in the install script (plan facts).

- [ ] **Step 4: The script tests**

In `crates/contrib/pgvector/src/lib.rs`'s `mod tests`:
- Replace the `SHIPPED_SCRIPTS` constant and its comment with:
  ```rust
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
  ```
- Replace `lookup_covers_every_module_pathname_symbol` with:
  ```rust
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
  ```
- In `fmgr_stubs_raise_internal_error`, extend the symbol list to:
  ```rust
          for symbol in [
              "hnswhandler",
              "hnsw_halfvec_support",
              "hnsw_bit_support",
              "hnsw_sparsevec_support",
              "ivfflathandler",
              "ivfflat_halfvec_support",
              "ivfflat_bit_support",
          ] {
  ```

- [ ] **Step 5: Prune stale staged files**

In `crates/backend/main/main_main/build.rs`:
- Add `use std::collections::HashSet;` and `use std::ffi::OsString;` to the imports.
- Before the `for crate_entry in ...` loop, add:
  ```rust
      // The file names each staged directory should hold.
      let mut sources: [HashSet<OsString>; 2] = [HashSet::new(), HashSet::new()];
  ```
- Change `for (sub, staged) in [("extension", &staged), ("tsearch_data", &staged_tsdata)] {` to `for (k, (sub, staged)) in [("extension", &staged), ("tsearch_data", &staged_tsdata)].into_iter().enumerate() {`.
- Replace the line `copy_if_changed(&src, &staged.join(src.file_name().unwrap()));` with:
  ```rust
                  let name = src.file_name().unwrap().to_owned();
                  copy_if_changed(&src, &staged.join(&name));
                  sources[k].insert(name);
  ```
- After the loop, add:
  ```rust

      // Remove staged files whose source is gone (a renamed or deleted
      // script, like pgvector's vector--0.8.5.sql at M4), so CREATE EXTENSION
      // cannot find a version the tree no longer ships. Only this script
      // writes these directories.
      for (k, staged) in [&staged, &staged_tsdata].into_iter().enumerate() {
          for f in std::fs::read_dir(staged).unwrap() {
              let p = f.unwrap().path();
              if p.is_file() && !sources[k].contains(p.file_name().unwrap()) {
                  std::fs::remove_file(&p).unwrap();
              }
          }
      }
  ```

- [ ] **Step 6: Build and run the unit tests**

```bash
scripts/pgvector/build-pgrust.sh
ls target/fast-profile/share/extension | grep -c '^vector'
ls target/fast-profile/share/extension/vector--0.8.5.sql 2>&1 | tail -1
sed -n "s/^default_version = '\(.*\)'$/\1/p" target/fast-profile/share/extension/vector.control
UT lookup_cover fmgr_stubs shipped_scripts
UT_CORE library_version
UT_KEEP
```
Expected:
- 43 staged `vector*` files, `vector--0.8.5.sql` gone (`No such file or directory`), and `0.8.7`.
- `UT`: 4 passed (`lookup_coverage_parser_reads_link_symbols`, `lookup_covers_every_module_pathname_symbol`, `fmgr_stubs_raise_internal_error`, `shipped_scripts_are_upstream_verbatim`).
- `UT_CORE`: 1 passed. `UT_KEEP`: all pass.

If the staged `vector--0.8.5.sql` is still there, cargo did not rerun `build.rs`; `touch crates/backend/main/main_main/build.rs` and rebuild, and name that in the report.

- [ ] **Step 7: All 14 regression files, and the extension around them**

```bash
scripts/pgvector/run-regress.sh pgrust
cat ~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv
```
Expected: 14/14 ok, `ivfflat_bit`, `ivfflat_halfvec` and `ivfflat_vector` included.

These are the regression files' first IVFFlat run. Before changing any code, read `regression.diffs` against Review Focus 1, 2 and 4: TRUNCATE's little-data NOTICE, the NULL-query counts, the 1/5/6/29 MB memory cases, and the `lists`, `ivfflat.*` and dimension errors.

The extension's metadata, update path, loaded-module version and GUC prefixes, against C (plain verbosity: LOCATION lines differ in core, plan facts):
```bash
cat >"$HOME/.cache/pgrust/pgvector-work/m4-ext.sql" <<'SQL'
SELECT extversion FROM pg_extension WHERE extname = 'vector';
SELECT version, superuser, trusted, relocatable, schema, requires FROM pg_available_extension_versions WHERE name = 'vector' ORDER BY version;
SELECT count(*) AS paths FROM pg_extension_update_paths('vector') WHERE path IS NOT NULL;
SELECT source, path FROM pg_extension_update_paths('vector') WHERE target = '0.8.7' AND source IN ('0.8.1', '0.8.6') ORDER BY source;
SELECT amname, obj_description(oid, 'pg_am') FROM pg_am WHERE amname IN ('hnsw', 'ivfflat') ORDER BY amname;
SELECT count(*) AS member_comments FROM pg_description d JOIN pg_depend dep ON dep.classid = d.classoid AND dep.objid = d.objoid AND d.objsubid = 0 WHERE dep.refclassid = 'pg_extension'::regclass AND dep.refobjid = (SELECT oid FROM pg_extension WHERE extname = 'vector') AND dep.deptype = 'e';
UPDATE pg_extension SET extversion = '0.8.6' WHERE extname = 'vector';
ALTER EXTENSION vector UPDATE;
SELECT extversion FROM pg_extension WHERE extname = 'vector';
SELECT '[1]'::vector;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';
SET ivfflat.nonexistent = 1;
SET hnsw.nonexistent = 1;
DROP EXTENSION vector;
CREATE EXTENSION vector VERSION '0.8.5';
CREATE EXTENSION vector VERSION '0.8.6';
CREATE EXTENSION vector;
SELECT extversion FROM pg_extension WHERE extname = 'vector';
SQL
scripts/pgvector/sqldiff.sh pgrust "$HOME/.cache/pgrust/pgvector-work/m4-ext.sql"
```
Expected: identical. That includes:
- `0.8.7` and one available version;
- the same path counts;
- `ALTER EXTENSION vector UPDATE` running `vector--0.8.6--0.8.7.sql` back to `0.8.7`;
- `vector | 0.8.7`;
- both `reserved prefix` errors;
- `CREATE EXTENSION ... VERSION '0.8.5'` and `'0.8.6'` failing alike, because no stale 0.8.5 script remains.

If pgrust refuses the direct `UPDATE pg_extension` where C allows it, that is a core difference. Record it in the report, drop the `UPDATE`, `ALTER EXTENSION` and following `SELECT extversion` lines, and let `pg_extension_update_paths` stand for the update path.

An IVFFlat smoke script of every opclass family, with `probes` equal to `lists`. Every list is then scanned, so results do not depend on the (unseeded) centers, and only distances print, so tie order cannot differ:
```bash
cat >"$HOME/.cache/pgrust/pgvector-work/m4-ivf.sql" <<'SQL'
SET enable_seqscan = off;
CREATE TABLE t (id int, v vector(3), h halfvec(3), b bit(8));
INSERT INTO t SELECT i, ARRAY[i % 7, i % 11, i % 13], ARRAY[i % 5, i % 3, 1 + i % 2], (i % 256)::bit(8) FROM generate_series(1, 2000) i;
CREATE INDEX t_v_l2 ON t USING ivfflat (v vector_l2_ops) WITH (lists = 4);
CREATE INDEX t_v_ip ON t USING ivfflat (v vector_ip_ops) WITH (lists = 4);
CREATE INDEX t_v_cos ON t USING ivfflat (v vector_cosine_ops) WITH (lists = 4);
CREATE INDEX t_h_l2 ON t USING ivfflat (h halfvec_l2_ops) WITH (lists = 4);
CREATE INDEX t_h_ip ON t USING ivfflat (h halfvec_ip_ops) WITH (lists = 4);
CREATE INDEX t_h_cos ON t USING ivfflat (h halfvec_cosine_ops) WITH (lists = 4);
CREATE INDEX t_b_ham ON t USING ivfflat (b bit_hamming_ops) WITH (lists = 4);
SET ivfflat.probes = 4;
EXPLAIN (COSTS OFF) SELECT v <-> '[3,3,3]' FROM t ORDER BY v <-> '[3,3,3]' LIMIT 5;
SELECT v <-> '[3,3,3]' FROM t ORDER BY v <-> '[3,3,3]' LIMIT 5;
SELECT v <#> '[3,3,3]' FROM t ORDER BY v <#> '[3,3,3]' LIMIT 5;
SELECT v <=> '[3,3,3]' FROM t ORDER BY v <=> '[3,3,3]' LIMIT 5;
SELECT h <-> '[2,1,1]' FROM t ORDER BY h <-> '[2,1,1]' LIMIT 5;
SELECT h <#> '[2,1,1]' FROM t ORDER BY h <#> '[2,1,1]' LIMIT 5;
SELECT h <=> '[2,1,1]' FROM t ORDER BY h <=> '[2,1,1]' LIMIT 5;
SELECT b <~> '10101010' FROM t ORDER BY b <~> '10101010' LIMIT 5;
SELECT count(*) FROM (SELECT 1 FROM t ORDER BY v <-> '[3,3,3]') s;
SELECT count(*) FROM (SELECT 1 FROM t ORDER BY v <=> '[3,3,3]') s;
DELETE FROM t WHERE id % 3 = 0;
VACUUM t;
INSERT INTO t SELECT i, ARRAY[i % 5, i % 9, i % 4], ARRAY[1, 2, 3], (i % 128)::bit(8) FROM generate_series(2001, 2500) i;
SELECT count(*) FROM (SELECT 1 FROM t ORDER BY v <-> '[3,3,3]') s;
SELECT count(*) FROM (SELECT 1 FROM t ORDER BY v <=> '[3,3,3]') s;
SELECT v <-> '[1,8,2]' FROM t ORDER BY v <-> '[1,8,2]' LIMIT 5;
SELECT b <~> '00000001' FROM t ORDER BY b <~> '00000001' LIMIT 5;
SQL
scripts/pgvector/sqldiff.sh pgrust "$HOME/.cache/pgrust/pgvector-work/m4-ivf.sql"
```
Expected: identical. Cosine counts leave out the zero vectors.

Then:
```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected: both PASS.

If a regression file or sqldiff differs, find the first differing statement, read the matching C code, and fix the port. Add a unit test that pins the fix when the code allows one, and name each fix in the report. A difference rooted in pgrust core outside this plan's edit list is a plan-defined stop.

- [ ] **Step 8: Commit**

```bash
git add crates/contrib/pgvector crates/backend/utils/fmgr/dfmgr/src/lib.rs \
  crates/backend/commands/extension/src/funcs.rs crates/backend/main/main_main/build.rs
git commit -q -F - <<'EOF'
pgvector: ship upstream's 0.8.7 extension verbatim; _PG_init and version

extension/ now holds upstream 0.8.7's vector.control, vector--0.8.7.sql
and all 41 upgrade scripts, byte for byte (a unit test checks), and the
lookup table resolves the ivfflat handler and support symbols. Loading
the library reserves the hnsw and ivfflat GUC prefixes, as _PG_init does,
and pg_get_loaded_modules reports vector's 0.8.7 through a new dfmgr
version registry. main_main's build script prunes staged files whose
source is gone. All 14 regression files pass.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 6: Byte-identical IVFFlat builds and the `ivfflat_errors` row

**Files:**
- Modify: `scripts/pgvector/build-reference.sh` (`-DIVFFLAT_BENCH`, two `--verify` checks), `scripts/pgvector/common.sh` (comment)
- Modify: `scripts/pgvector/run-bytecmp.sh` (8 IVFFlat cases, the seed `SET`, the per-case oracle marker, the lists column)
- Modify: `scripts/pgvector/sqldiff.sh` (`strip_seeded_info`)
- Create: `scripts/pgvector/sql/ivfflat-errors.sql`
- Modify: `scripts/pgvector/run-diff.sh` (the `ivfflat_errors` row)
- Modify: `scripts/pgvector/tests/harness_test.sh`

**Interfaces:**
- Consumes Task 2's `pgrust.ivfflat_build_seed` and Task 5's shipped AM.
- Produces bytecmp cases `ivf_l2_groups ivf_big ivf_halfvec_groups ivf_bit_hamming ivf_insert ivf_unlogged ivf_vacuum ivf_wide` and the diff row `ivfflat_errors`.

- [ ] **Step 1: The seeded oracle seeds IVFFlat too**

In `scripts/pgvector/build-reference.sh`, inside `verify_reference`:
- Replace the `hnsw_memory_marker` function and its comment with:
  ```bash
    # Count a format string compiled into a pgvector library: HNSW_MEMORY's
    # FlushPages INFO "memory: %zu MB" (hnswbuild.c:306-308) and IVFFLAT_BENCH's
    # IvfflatBench INFO "%s: %.3f ms" (ivfflat.h:71-84).
    lib_marker() {
      local lib
      for lib in "$2/vector.dylib" "$2/vector.so"; do
        if [ -f "$lib" ]; then
          strings "$lib" | grep -cF -- "$1" || true
          return
        fi
      done
      echo missing
    }
  ```
- Replace the two `expect ... HNSW_MEMORY` lines with:
  ```bash
    seeded_lib="$("$PG_VEC_SEEDED/bin/pg_config" --pkglibdir 2>/dev/null || true)"
    ref_lib="$("$PG_VEC/bin/pg_config" --pkglibdir 2>/dev/null || true)"
    expect "seeded pgvector built with HNSW_MEMORY" "$(lib_marker 'memory: %zu MB' "$seeded_lib")" "1"
    expect "reference pgvector built without HNSW_MEMORY" "$(lib_marker 'memory: %zu MB' "$ref_lib")" "0"
    expect "seeded pgvector built with IVFFLAT_BENCH" "$(lib_marker '%s: %.3f ms' "$seeded_lib")" "1"
    expect "reference pgvector built without IVFFLAT_BENCH" "$(lib_marker '%s: %.3f ms' "$ref_lib")" "0"
  ```
  and add `seeded_lib ref_lib` to the function's `local` line.
- In step 4 (the seeded install), replace the comment's first sentence with `# 4. Seeded reference install (oracle of run-bytecmp.sh and run-iterscan.sh): the same pgvector built with -DHNSW_MEMORY -DIVFFLAT_BENCH, the only C configuration that seeds HNSW and IVFFlat builds (SeedRandom(42), hnswbuild.c:1134-1136, ivfbuild.c:1075-1077).` Then change `PG_CFLAGS="-DHNSW_MEMORY"` to `PG_CFLAGS="-DHNSW_MEMORY -DIVFFLAT_BENCH"` and the `echo` to `building seeded pgvector 0.8.7 (-DHNSW_MEMORY -DIVFFLAT_BENCH) in $build`.

In `scripts/pgvector/common.sh`, replace the `$PG_VEC_SEEDED` comment lines with:
```bash
#   $PG_VEC_SEEDED  Same as $PG_VEC, but pgvector is built with -DHNSW_MEMORY
#              -DIVFFLAT_BENCH, the only configuration in which C seeds HNSW
#              and IVFFlat builds (SeedRandom(42), hnswbuild.c:1134-1136,
#              ivfbuild.c:1075-1077). Oracle of the byte-identical and
#              iterative-scan tiers; nothing else uses it.
```

Rebuild the seeded install (about two minutes):
```bash
rm -rf "$HOME/.cache/pgrust/pgref-18.6/pgvec-seeded" "$HOME/.cache/pgrust/pgref-18.6/build/pgvector-seeded"
scripts/pgvector/build-reference.sh
```
Expected: the verify step prints 13 `ok` lines and no `FAIL`.

- [ ] **Step 2: Strip the seeded build's IVFFlat timing lines in sqldiff**

`sqldiff.sh ref` runs the seeded build as its subject. Its IVFFlat builds and scans now also print `INFO:  <step>: N ms`. In `scripts/pgvector/sqldiff.sh`, replace `strip_hnsw_memory`, its comment and its call with:
```bash
# The seeded build (-DHNSW_MEMORY -DIVFFLAT_BENCH) also prints INFO lines that
# stock C and pgrust do not: "memory:" for HNSW builds, scans and vacuums
# (hnswbuild.c:307, hnswscan.c:128, hnswvacuum.c:171), and IvfflatBench's
# "<step>: N ms" for IVFFlat builds and scans (ivfflat.h:71-84). Drop them,
# and the LOCATION line --verbose adds, before comparing.
strip_seeded_info() {
  awk '/INFO:  (00000: )?(memory: |(sample rows|k-means|assign tuples|sort tuples|load tuples|GetScanLists|GetScanItems): [0-9.]+ ms)/ { skip = 1; next }
    skip && /^LOCATION:  / { skip = 0; next }
    { skip = 0; print }' "$1" >"$1.tmp"
  mv "$1.tmp" "$1"
}
```
In the per-side loop, change `strip_hnsw_memory "$out/$side.out"` to `strip_seeded_info "$out/$side.out"`.

- [ ] **Step 3: The IVFFlat cases in `run-bytecmp.sh`**

In `scripts/pgvector/run-bytecmp.sh`:
- Header:
  - its first line becomes `# Byte-identical HNSW and IVFFlat tier (spec §8.2). Build the same indexes on the`;
  - in the `pgrust:` usage line, after `SET pgrust.hnsw_build_seed = $PGV_BYTECMP_SEED (default 42)`, add `and SET pgrust.ivfflat_build_seed`;
  - replace the paragraph that begins `The oracle is $PG_VEC_SEEDED` with:
  ```bash
  # The oracle is $PG_VEC_SEEDED (see common.sh): stock C pgvector seeds neither
  # access method (SeedRandom(42) sits under #ifdef HNSW_MEMORY and #ifdef
  # IVFFLAT_BENCH), so only that build is deterministic. HNSW data are mostly
  # integer-valued so float math is exact or rounds once (identical under FMA
  # and reassociation); the non-integer cases (sparsevec_l2_frac and the cosine
  # cases) say in their comments why C and pgrust round alike. IVFFlat data
  # (ivf_*) are well-separated groups of four integer points: every k-means and
  # assignment comparison has a clear winner, and the stored centers are
  # element-wise sums divided once, which C and pgrust round alike. Every case
  # runs in its own session; insert_* cases build and then insert in ONE
  # session, because on-disk inserts draw levels from the stream the build
  # left behind.
  ```
- Append the eight cases to `CASES`, keeping it one line:
  `... bq_expression ivf_l2_groups ivf_big ivf_halfvec_groups ivf_bit_hamming ivf_insert ivf_unlogged ivf_vacuum ivf_wide"`.
- After the `MD5BITS=...` line, add:
  ```bash
  # IVFFlat groups: ten groups of four points, 256 apart; a group's mean is
  # its corner + (0.5, 0.5).
  IVF_GROUPS='ARRAY[256 * ((i % 10) % 5) + (i / 10) % 2, 256 * ((i % 10) / 5) + ((i / 10) / 2) % 2, 0]'
  IVF_GROUPS2='ARRAY[256 * (i % 2) + (i / 2) % 2, ((i / 2) / 2) % 2, 0]'
  ```
- In `case_sql`, after the `pgrust.hnsw_build_seed` line, add:
  ```bash
    [ -z "$SEED" ] || echo "SET pgrust.ivfflat_build_seed = $SEED;"
  ```
- Before the `spill)` arm, add:
  ```bash
      ivf_l2_groups)
        cat <<SQL
  CREATE TABLE $1 (id int, v vector(3));
  INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 999) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
  SQL
        ;;
      ivf_big)
        # 20,000 rows and 10,000 samples: the reservoir replaces samples
        # (reservoir_get_next_S, sampler_random_fract), so the group means are
        # inexact, but every comparison still has a clear winner.
        cat <<SQL
  CREATE TABLE $1 (id int, v vector(3));
  INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 19999) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
  SQL
        ;;
      ivf_halfvec_groups)
        # 1024.5 is not a half: the centers 1024.5 round to 1024 on both sides.
        cat <<SQL
  CREATE TABLE $1 (id int, v halfvec(3));
  INSERT INTO $1 SELECT i, $IVF_GROUPS::halfvec FROM generate_series(0, 999) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v halfvec_l2_ops) WITH (lists = 10);
  SQL
        ;;
      ivf_bit_hamming)
        # Hamming distances are integers; bit centers threshold their means.
        cat <<SQL
  CREATE TABLE $1 (id int, v bit(64));
  INSERT INTO $1 SELECT i, $MD5BITS FROM generate_series(1, 1000) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v bit_hamming_ops) WITH (lists = 10);
  SQL
        ;;
      ivf_insert)
        # Two lists of two pages each; the inserts fill them, append a page to
        # each (under the extension lock) and move the insert pages there.
        cat <<SQL
  CREATE TABLE $1 (id int, v vector(3));
  INSERT INTO $1 SELECT i, $IVF_GROUPS2::vector FROM generate_series(0, 599) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 2);
  INSERT INTO $1 SELECT i, $IVF_GROUPS2::vector FROM generate_series(600, 999) i;
  SQL
        ;;
      ivf_unlogged)
        # Also compares the init fork ivfflatbuildempty writes: no heap, so
        # random centers from the stream the main-fork build left behind.
        cat <<SQL
  CREATE UNLOGGED TABLE $1 (id int, v vector(3));
  INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 999) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
  SELECT 'rel|idx_init|' || pg_relation_filepath('$1_idx') || '_init';
  SQL
        ;;
      ivf_vacuum)
        # ivfflatbulkdelete compacts every entry page and moves each list's
        # insert page back to its first page with deletions.
        cat <<SQL
  CREATE TABLE $1 (id int, v vector(3)) WITH (autovacuum_enabled = false);
  INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 2999) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
  DELETE FROM $1 WHERE id % 3 = 0;
  VACUUM $1;
  SQL
        ;;
      ivf_wide)
        # 2,000 dimensions: one list item per page (IvfflatAppendPage in
        # CreateListPages) and one entry per page.
        cat <<SQL
  CREATE TABLE $1 (id int, v vector(2000));
  INSERT INTO $1 SELECT i, (ARRAY[256 * (i % 5) + (i / 5) % 2, ((i / 5) / 2) % 2] || array_fill(0, ARRAY[1998]))::vector FROM generate_series(0, 49) i;
  CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 5);
  SQL
        ;;
  ```
  The heredoc bodies start at column 0, like the existing cases'. The indentation above is only this plan's list nesting.
- In `run_side`, replace the seeded-oracle check and its comment with:
  ```bash
      # The seeded build reports its HNSW graph memory (HNSW_MEMORY's
      # FlushPages elog) or its IVFFlat k-means time (IVFFLAT_BENCH's
      # IvfflatBench), which proves the seeded library built the index.
      marker='INFO:  memory: '
      case "$c" in ivf_*) marker='INFO:  k-means: ' ;; esac
      if [ "$kind" = seeded ] && ! grep -q "$marker" "$dir/$c.err"; then
        die "$label: case $c was not built by the seeded C pgvector (no '${marker% }' line)"
      fi
  ```
  and add `marker` to `run_side`'s `local` line.
- After `entry_level()`, add:
  ```bash

  # IvfflatMetaPageData.lists (uint16 at page offset 24 + 10).
  meta_lists() {
    od -An -tu2 -j34 -N2 "$1" | tr -d ' '
  }
  ```
- In the summary loop, replace the `printf '%s\t%s\t%s\t%s\n' "$c" "${name#"$c".}" ... "$(entry_level "$f")" >>"$out/cases.tsv"` line with:
  ```bash
      # Column 4: the HNSW graph height, or an IVFFlat index's lists.
      case "$c" in ivf_*) col4="$(meta_lists "$f")" ;; *) col4="$(entry_level "$f")" ;; esac
      printf '%s\t%s\t%s\t%s\n' "$c" "${name#"$c".}" "$(($(wc -c <"$f") / 8192))" "$col4" >>"$out/cases.tsv"
  ```
  The summary loop is at the top level, so `col4` is a plain assignment.

- [ ] **Step 4: `ivfflat-errors.sql` and its diff row**

Create `scripts/pgvector/sql/ivfflat-errors.sql`:
```sql
-- Every SQL-reachable IVFFlat error site, LOCATION included, for the diff
-- tier (sqldiff.sh --verbose): InitBuildState (ivfbuild.c:358, :364, :369,
-- :384), IvfflatCheckMemoryUsage (ivfutils.c:133), IvfflatCheckExpectedDim
-- (ivfutils.c:289, from the scan, even on an empty index) and the
-- little-data NOTICE (ivfbuild.c:477). Errors raised by core (the lists
-- reloption, the ivfflat.* settings) are left to the regression files:
-- pgrust reports their LOCATION as reloptions.c:0 / guc.c:0. NULL queries
-- use the (SELECT NULL::type) form so they reach the index scan (a literal
-- NULL is constant-folded), and EXPLAIN pins that plan; a zero query on a
-- cosine index orders NaN distances by the sort's tie order.
SET enable_seqscan = off;

-- vector
CREATE TABLE v_nodim (val vector);
CREATE INDEX ON v_nodim USING ivfflat (val vector_l2_ops);
CREATE TABLE v_wide (val vector(2001));
CREATE INDEX ON v_wide USING ivfflat (val vector_l2_ops);
CREATE TABLE v_one (val vector(1));
CREATE INDEX ON v_one USING ivfflat (val vector_cosine_ops);
CREATE INDEX ON v_one USING ivfflat (val vector_ip_ops);
CREATE INDEX ON v_one USING ivfflat (val vector_l2_ops) WITH (lists = 1);
CREATE TABLE v (val vector(3));
CREATE INDEX v_l2 ON v USING ivfflat (val vector_l2_ops) WITH (lists = 1);
SELECT * FROM v ORDER BY val <-> '[1,2]';
INSERT INTO v VALUES ('[1,2,3]'), ('[1,1,1]'), ('[0,0,0]'), (NULL);
SELECT * FROM v ORDER BY val <-> '[1,2]';
SELECT * FROM v ORDER BY val <-> '[3,3,3]';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM v ORDER BY val <-> (SELECT NULL::vector)) t;
SELECT COUNT(*) FROM (SELECT * FROM v ORDER BY val <-> (SELECT NULL::vector)) t;
CREATE INDEX v_cos ON v USING ivfflat (val vector_cosine_ops) WITH (lists = 1);
SELECT * FROM v ORDER BY val <=> '[0,0,0]';
SELECT * FROM v ORDER BY val <=> '[1,2]';
SET maintenance_work_mem = '1MB';
CREATE TABLE v_mem (val vector(2000));
CREATE INDEX ON v_mem USING ivfflat (val vector_l2_ops) WITH (lists = 1000);
RESET maintenance_work_mem;

-- halfvec
CREATE TABLE h_nodim (val halfvec);
CREATE INDEX ON h_nodim USING ivfflat (val halfvec_l2_ops);
CREATE TABLE h_wide (val halfvec(4001));
CREATE INDEX ON h_wide USING ivfflat (val halfvec_l2_ops);
CREATE TABLE h_one (val halfvec(1));
CREATE INDEX ON h_one USING ivfflat (val halfvec_cosine_ops);
CREATE TABLE h (val halfvec(3));
CREATE INDEX h_l2 ON h USING ivfflat (val halfvec_l2_ops) WITH (lists = 1);
SELECT * FROM h ORDER BY val <-> '[1,2]';
INSERT INTO h VALUES ('[1,2,3]'), ('[1,1,1]'), ('[0,0,0]'), (NULL);
SELECT * FROM h ORDER BY val <-> '[1,2]';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM h ORDER BY val <-> (SELECT NULL::halfvec)) t;
SELECT COUNT(*) FROM (SELECT * FROM h ORDER BY val <-> (SELECT NULL::halfvec)) t;

-- bit
CREATE TABLE b_var (val varbit(3));
CREATE INDEX ON b_var USING ivfflat (val bit_hamming_ops);
CREATE TABLE b_wide (val bit(64001));
CREATE INDEX ON b_wide USING ivfflat (val bit_hamming_ops);
CREATE INDEX ON b_var USING ivfflat ((val::bit(64001)) bit_hamming_ops);
CREATE TABLE b (val bit(3));
CREATE INDEX b_ham ON b USING ivfflat (val bit_hamming_ops) WITH (lists = 1);
SELECT * FROM b ORDER BY val <~> '01';
INSERT INTO b VALUES ('000'), ('100'), ('111'), (NULL);
SELECT * FROM b ORDER BY val <~> '01';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM b ORDER BY val <~> (SELECT NULL::bit)) t;
SELECT COUNT(*) FROM (SELECT * FROM b ORDER BY val <~> (SELECT NULL::bit)) t;
```
On C this file gives 18 errors and 4 NOTICEs (plan facts).

In `scripts/pgvector/run-diff.sh`, after the `hnsw_errors` block, add:
```bash
# IVFFlat's error sites, every type (M4 Review Focus 2: a wrong-dimension
# query errors even on an empty index).
if "$here/sqldiff.sh" "$mode" --verbose "$here/sql/ivfflat-errors.sql" >"$out/ivfflat-errors.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'ivfflat_errors\t%s\n' "$r" | tee -a "$out/summary.tsv"
```

- [ ] **Step 5: The harness self-tests**

In `scripts/pgvector/tests/harness_test.sh`:
- In `test_diff_ref`, change both `18`s to `19`.
- Before `test_bytecmp_cases`, add:
  ```bash
  # Blocks and lists of each IVFFlat case's relation on the seeded C build
  # (M4 plan facts): lists start on blocks 2 and 4 and inserts append two
  # pages (ivf_insert), one list item and one entry per page (ivf_wide).
  ivf_want() {
    case "$1" in
      ivf_l2_groups:idx | ivf_halfvec_groups:idx | ivf_bit_hamming:idx | ivf_unlogged:idx | ivf_unlogged:idx_init) echo "12 10" ;;
      ivf_big:idx) echo "92 10" ;;
      ivf_insert:idx) echo "8 2" ;;
      ivf_vacuum:idx) echo "22 10" ;;
      ivf_wide:idx) echo "56 5" ;;
      *) echo "unknown case" ;;
    esac
  }
  ```
- In `test_bytecmp_cases`, add as the first arm of the `case`:
  ```bash
        ivf_*) [ "$blocks $level" = "$(ivf_want "$c:$label")" ] && pass "bytecmp $c $label: $blocks blocks, $level lists" ||
          fail "bytecmp $c $label: $blocks blocks, $level lists, want $(ivf_want "$c:$label")" ;;
  ```
  and add `IVFFlat block and list counts` to the comment above the function.
- After `test_bytecmp_knob_off`, add:
  ```bash

  # The IVFFlat seed is opt-in too: without SET pgrust.ivfflat_build_seed the
  # k-means++ picks, and so the list order, differ from the seeded oracle.
  test_bytecmp_ivf_knob_off() {
    local w="$PGV_WORK/bytecmp-ivf-noseed"
    if PGV_WORK="$w" PGV_BYTECMP_SEED= "$here/../run-bytecmp.sh" pgrust ivf_l2_groups >"$w.log" 2>&1; then
      fail "bytecmp: unseeded pgrust ivfflat differs from the seeded oracle"
    elif grep -q $'^ivf_l2_groups\tFAIL$' "$w/bytecmp/pgrust/summary.tsv" 2>/dev/null; then
      pass "bytecmp: unseeded pgrust ivfflat differs from the seeded oracle"
    else
      fail "bytecmp: unseeded pgrust ivfflat: wrong failure (see $w.log)"
    fi
  }
  ```

- [ ] **Step 6: Run the tiers**

```bash
bash -n scripts/pgvector/run-bytecmp.sh scripts/pgvector/sqldiff.sh scripts/pgvector/run-diff.sh scripts/pgvector/build-reference.sh scripts/pgvector/tests/harness_test.sh
/bin/bash -n scripts/pgvector/run-bytecmp.sh
scripts/pgvector/run-bytecmp.sh ref
cat ~/.cache/pgrust/pgvector-work/bytecmp/ref/cases.tsv | grep '^ivf_'
scripts/pgvector/run-bytecmp.sh pgrust
cat ~/.cache/pgrust/pgvector-work/bytecmp/pgrust/summary.tsv
scripts/pgvector/sqldiff.sh ref --verbose scripts/pgvector/sql/ivfflat-errors.sql
grep -c '^psql:input.sql:[0-9]*: ERROR' ~/.cache/pgrust/pgvector-work/sqldiff/ref/ref.out
scripts/pgvector/sqldiff.sh pgrust --verbose scripts/pgvector/sql/ivfflat-errors.sql
scripts/pgvector/run-diff.sh ref
scripts/pgvector/run-diff.sh pgrust
scripts/pgvector/run-iterscan.sh ref
scripts/pgvector/run-iterscan.sh pgrust
scripts/pgvector/tests/harness_test.sh
```
Expected:
- `bash -n` is silent. `/bin/bash` is macOS's bash 3.2.
- `run-bytecmp.sh ref`: 29/29, with the IVFFlat rows of `cases.tsv` as `ivf_want` lists them.
- `run-bytecmp.sh pgrust`: 29/29: the 21 HNSW cases still identical, and all 8 IVFFlat cases identical.
- sqldiff `ref`: identical, 18 errors. sqldiff `pgrust`: identical. The ERROR count pattern may need adjusting to the transcript's prefix; count the errors either way.
- `run-diff.sh`: 19/19 in both modes.
- `run-iterscan.sh`: 19/19 in both modes; the seeded rebuild must not move an HNSW trace.
- `harness_test.sh`: `all passed`.

Read each verdict from its `summary.tsv` before the next run overwrites it.

If an IVFFlat bytecmp case differs, `diff/<case>.idx.txt` names the first differing block and page offset. Decode both sides' list page (block 1: each item's `startPage`, `insertPage` and center) and the first differing entry page, then compare the stage that wrote it:
- **block 0** is the metapage;
- **block 1 centers** come from sampling or k-means: `reservoir_init_selection_state`'s u32 seed, the RNG draw order in `init_centers`, the float casts in Elkan;
- **entry tuple order within a list** comes from the sort's tie order;
- **the entry tuple bytes themselves** come from `index_form_tuple`'s packing of short varlenas;
- **list or insert pages** come from `ivfflat_update_list`.

Fix the port and add a unit test where the code allows one. Never move a case to opt-in (plan-defined stop).

- [ ] **Step 7: Commit**

```bash
git add scripts/pgvector
git commit -q -F - <<'EOF'
pgvector: byte-identical IVFFlat builds; ivfflat_errors diff row

The seeded C reference now also builds with -DIVFFLAT_BENCH, the only
configuration that seeds IVFFlat builds; run-bytecmp.sh gains eight
IVFFlat cases (exact group data, reservoir sampling, halfvec, bit,
inserts appending pages, an unlogged init fork, vacuum, 2,000
dimensions) built with SET pgrust.ivfflat_build_seed = 42, and
run-diff.sh's ivfflat_errors row compares every SQL-reachable IVFFlat
error site with C, LOCATION included.

Co-Authored-By: <the implementing model>
EOF
```

---

### Task 7: The IVFFlat TAP gate

**Files:**
- Modify: whatever the triage below fixes (port code in `pgvector_ivfflat*`, with a unit test per fix where the code allows one)

**Interfaces:**
- Consumes everything above. Produces no new names.

- [ ] **Step 1: Run every IVFFlat TAP test**

```bash
scripts/pgvector/run-tap.sh pgrust $IVF_TAP
cat ~/.cache/pgrust/pgvector-work/tap/pgrust/summary.tsv
for t in 003_ivfflat_vector_build_recall 032_ivfflat_halfvec_build_recall 035_ivfflat_bit_build_recall; do
  echo "== $t"; grep -n '^not ok\|^# *Failed test\|at t/.* line' ~/.cache/pgrust/pgvector-work/tap/pgrust/prove/$t.log | head -20
done
```
(`$IVF_TAP` is the Global Constraints list.) Expected:
- 13 `ok`: `001`, `002`, `004`–`009`, `036` and `040`–`042`, `048`.
- `003`, `032` and `035` FAIL only on `like($stderr, qr/using \d+ parallel workers/)`: `003` line 127 three times, `032` line 131 three times, `035` line 116 once. Each of their other assertions, recall after the "parallel" build included, passes. pgrust logs `building index "idx" on table "tst" serially` there (M6).

Copy `summary.tsv` and each failing test's log aside before any rerun, because `run-tap.sh` wipes `tap/pgrust` on every invocation.

- [ ] **Step 2: Triage anything else**

For each other failure, take the first failing assertion in `prove/<name>.log`, reproduce it with a small SQL script under `sqldiff.sh pgrust` (C against pgrust), find the C code that differs, and fix the port. The likeliest causes, by test:

| Test | What it pins | Where to look |
|---|---|---|
| `001` WAL | a pgrust standby replays the GenericXLog records exactly (100,000 rows, deletes, vacuum, inserts) | full-image vs delta registration (`XlogPage::register` flags), `link_new_page`'s registration order, the init fork's `log_newpage_range` |
| `002` vacuum size | the index does not grow after delete-all, `VACUUM`, re-insert | `bulk_delete`'s insert-page reset, `ivfflat_update_list`'s `original_insert_page` guard, `insert_tuple`'s page walk |
| `003`/`032`/`035` recall, `004`, `005`, `036`, `042` | nearest lists and ties: equal vectors stored in and probed from one list | `find_insert_page` and `add_tuple_to_sort` (strict `<`, first wins), `get_scan_lists`' heap and `max_distance`, `get_scan_value`'s normalization |
| `006` lists, `009` filtering, `040` cost | plan choice | `ivfflatcostestimate` against `ivfflat.c:85-151`; `EXPLAIN` both servers with the test's data and settings |
| `007` inserts | counts after inserts, `idx_scan` | `ivfflatinsert` (extension lock, new page) and `xs_pgstat_index_scans` |
| `008` centers | duplicate centers build without `Not enough centers` / `Zero norm` | `init_centers`' zero-weight pick, `compute_new_centers`' empty clusters, `check_norms` |
| `041` iterative | `max_probes` bounds the scan | `ivfflatbeginscan`'s `max_probes`, `get_scan_items`' `batch_probes` short-circuit, `ivfflatgettuple`'s `list_index == max_probes` |
| `048` vacuum+insert | concurrent insert, delete, scan and vacuum | `LockBufferForCleanup`, buffer release on every path, `ivfflat_update_list` |

Rules:
- Pin every fix with a unit test where the code allows one; otherwise with a sqldiff script whose transcript the report quotes.
- Rerun the failing test, then all of `$IVF_TAP` once more at the end.
- A difference rooted in pgrust core outside this plan's edit list is a plan-defined stop.
- If a test fails on time rather than on an assertion (a TAP command timing out), measure the build or scan with `\timing` on both servers before changing anything, and report the numbers.

- [ ] **Step 3: Nothing else regressed**

```bash
scripts/pgvector/run-tap.sh pgrust $KEEP_TAP
cat ~/.cache/pgrust/pgvector-work/tap/pgrust/summary.tsv
scripts/pgvector/run-regress.sh pgrust
```
(`$KEEP_TAP` is the Global Constraints list.) Expected: 27/27 TAP ok, and regress 14/14.

If Step 2 changed Rust, also rerun `scripts/pgvector/run-bytecmp.sh pgrust` (29/29) and `scripts/pgvector/run-diff.sh pgrust` (19/19), `UT ivf_`, `UT_KEEP` and both lints.

- [ ] **Step 4: Commit (only if Step 2 changed anything)**

```bash
git add crates/contrib/pgvector_ivfflat crates/contrib/pgvector_ivfflat_build crates/_support/types/types_ivfflat
git commit -q -F - <<'EOF'
pgvector: IVFFlat TAP fixes

<one line per fix: the test, the C behavior, the change>

Co-Authored-By: <the implementing model>
EOF
```
If nothing changed, record the run in the task report and skip the commit.

---

### Task 8: Docs, the full gate and the M4 report

**Files:**
- Delete: `scripts/pgvector/upstream-sql.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (drop `test_upstream_sql_only_am`)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, `CLAUDE.md`
- Create: `docs/superpowers/reports/pgvector-m4-report.md`

- [ ] **Step 1: Retire `upstream-sql.sh`**

The extension script is verbatim now, so nothing grows it.
```bash
git rm -q scripts/pgvector/upstream-sql.sh
```
In `scripts/pgvector/tests/harness_test.sh`, delete `test_upstream_sql_only_am` and the comment block above it.

- [ ] **Step 2: Correct the spec**

In `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`:
- **§3, Extension files bullet.** Append: ` (M4 ships all 41 upstream upgrade scripts, as C's `DATA` installs them.)`
- **§4.6, first bullet.** Append: ` (Done in M4: extension/ is upstream's, byte for byte, which `shipped_scripts_are_upstream_verbatim` checks.)`
- **§4.6, lookup-coverage bullet.** Replace its parenthetical with: `(In place since M2: `lookup_covers_every_module_pathname_symbol` in `crates/contrib/pgvector/src/lib.rs`; since M4 it reads every shipped file.)`
- **§5.1, Version bullet.** Append: ` (M4: `default_version = '0.8.7'`, and `pg_get_loaded_modules()` reports 0.8.7 through dfmgr's library-version registry.)`
- **§5.4, first bullet.** Replace it, up to the colon before its type list, with: `**Support proc 3 (type info)** resolves by its C symbol (the `pg_proc.prosrc` of a `LANGUAGE c` function, `pgvector::support_proc_symbol`) to a static Rust table, and is never called through fmgr. This is in place since M3 as `hnsw_get_type_info` (`pgvector_hnsw/src/utils.rs`). Any other symbol raises 0A000, where C would call it:` That also closes M3's split-parenthetical minor.
- **§5.4, build seed bullet on IVFFlat.** Replace `IVFFlat is similar: `SeedRandom(42)` runs only under `#ifdef IVFFLAT_BENCH` (`ivfbuild.c:1075-1077`). This matters in M4.` with: `IVFFlat is similar: `SeedRandom(42)` runs only under `#ifdef IVFFLAT_BENCH` (`ivfbuild.c:1075-1077`). Since M4, `pgvec-seeded` is also built with `-DIVFFLAT_BENCH`, and the placeholder `pgrust.ivfflat_build_seed` makes `ivfflatbuild` call `SeedRandom(<value>)` there.`
- **§6, Support procs bullet.** Replace `Proc 5 resolves by name, as with HNSW.` with `Proc 5 resolves by its C symbol, as HNSW's proc 3 does (`ivfflat_get_type_info`, `pgvector_ivfflat/src/utils.rs`).`
- **§8.1, `pgvec-seeded` bullet.** After `pgvector is built with `-DHNSW_MEMORY`, the only C configuration that seeds HNSW builds (§5.4)`, insert `and, since M4, `-DIVFFLAT_BENCH`, the only one that seeds IVFFlat builds`.
- **§8.2, Byte-identical index row.**
  - In the Method cell, replace `IVFFlat is attempted but is nondeterministic in C.` with: `Since M4 the tier also covers IVFFlat, with pgrust setting `pgrust.ivfflat_build_seed = 42`. Its eight cases use well-separated groups of integer points, so every k-means comparison has a clear winner. They cover reservoir sampling, `halfvec`, `bit`, inserts that append pages, an unlogged init fork, vacuum and 2,000 dimensions.`
  - In the Gated cell, replace `HNSW yes` with `HNSW and IVFFlat yes`.
- **§8.2, Exact differential row.** After the `scripts/pgvector/sql/hnsw-errors.sql` parenthetical, insert `, and every SQL-reachable IVFFlat error site (`scripts/pgvector/sql/ivfflat-errors.sql`, M4)`.
- **§8.3.**
  - Add a bullet `` `sql/ivfflat-errors.sql` (M4) `` after the one for `sql/hnsw-errors.sql`.
  - In the bullet naming `upstream-sql.sh` (M2), append `; removed in M4 once the script was verbatim`.

- [ ] **Step 3: Update `CLAUDE.md`**

In the "Vector search (fork work)" section:
- **Opening paragraph.** Replace `and their HNSW opclasses in M3; the SQL script is still the trimmed 0.8.5 one, grown with upstream sections by `scripts/pgvector/upstream-sql.sh`):` with `and their HNSW opclasses in M3, and IVFFlat (serial builds) in M4. The extension directory is upstream 0.8.7's, verbatim:`.
- **New bullets**, after the `pgvector_hnsw` bullet:
  ```markdown
  - `crates/contrib/pgvector_ivfflat` and `pgvector_ivfflat_build`: the IVFFlat AM for `vector`, `halfvec` and `bit`, using pgvector's page layout and GenericXLog. Builds are serial: k-means over samples from ANALYZE's block sampler, which now lives in its own `sampling` crate. An opclass's support proc 5 resolves by its C symbol, as HNSW's proc 3 does.
  - `crates/_support/types/types_ivfflat`.
  ```
- **Status line.** Replace `` `ivfflat` (M4) is **not ported**, and the extension script is trimmed to match.`` with `Parallel index builds (M6) are **not ported**: both AMs build serially.`
- **Harness command block.**
  - Delete the `upstream-sql.sh` line.
  - Replace the `run-bytecmp.sh` line with:
  ```bash
  scripts/pgvector/run-bytecmp.sh {pgrust|ref} [case ...]         # HNSW and IVFFlat pages vs the seeded C build (pgvec-seeded, -DHNSW_MEMORY -DIVFFLAT_BENCH)
  ```
- **The seed paragraph.**
  - Replace `Stock C pgvector never seeds HNSW builds; `SET pgrust.hnsw_build_seed = 42` (an unregistered placeholder option) makes pgrust match the seeded C build.` with `Stock C pgvector never seeds HNSW or IVFFlat builds. `SET pgrust.hnsw_build_seed = 42` and `SET pgrust.ivfflat_build_seed = 42` (unregistered placeholder options) make pgrust match the seeded C build.`
  - Add `` `docs/superpowers/reports/pgvector-m4-report.md` `` to the baselines sentence.

- [ ] **Step 4: Full run of every tier**

```bash
scripts/pgvector/build-reference.sh --verify
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-all.sh ref
scripts/pgvector/run-all.sh pgrust
scripts/pgvector/tests/harness_test.sh
UT ivf_ block_sampler reservoir_ lookup_cover fmgr_stubs shipped_scripts
UT_CORE ivf_ amsearcharray_is_btree_only handler_dispatch_is_the_closed_set extension_am_identified_by_handler_prosrc phasenames_match_c overrides_sorted_unique_and_non_default library_version table_counts_match_compiled_backend_shape
UT_KEEP
cargo test --profile fast-profile --locked -p main_main -p pgvector_f16_parity --lib -- f16_
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
for t in crates/pgvector-0.8.7-reference crates/postgres-18.6-reference; do git status --porcelain --ignored "$t"; done
```
Expected:
- `build-reference.sh --verify`: 13 `ok`.
- `run-all.sh ref`: 129/129 (regress 14, TAP 48, bytecmp 29, iterscan 19, diff 19).
- `run-all.sh pgrust`: 121/129 (regress 14, TAP 40, bytecmp 29, iterscan 19, diff 19).
  - The 8 TAP failures are parallel builds only (M6). `003`, `032` and `035` fail on their `using \d+ parallel workers` lines. `012`, `020`, `024` and `028` fail on the same lines as in M3, and `045` on lines 26–27 as in M3. Check each against its log.
- `harness_test.sh`: `all passed`.
- `UT`: 23 passed (19 + 4); `UT_CORE`: 8 passed; `UT_KEEP`: all pass; parity: 3 passed.
- Both lints PASS, and both reference trees print nothing.

- [ ] **Step 5: Write `docs/superpowers/reports/pgvector-m4-report.md`**

Follow `docs/superpowers/reports/pgvector-m3-report.md`'s structure, filled from the Step 4 runs:
1. **Header line:** the date, the pgrust code commit (the last code commit before this one), and the references (`pgvec`; `pgvec-seeded` with `-DHNSW_MEMORY -DIVFFLAT_BENCH`).
2. **Summary:**
   - C and pgrust totals by tier;
   - what M4 fixed: the three `ivfflat_*` regression files and 13 IVFFlat TAP tests, plus the serial assertions of `003`, `032` and `035`;
   - the gates that are also green.
3. **pgrust results:** the full table from `~/.cache/pgrust/pgvector-work/report-pgrust.md`.
4. **Remaining failures:** each with its cause and milestone, checked against the logs.
5. **What M4 established:**
   - IVFFlat ported serially for `vector`, `halfvec` and `bit`, with support proc 5 resolved by C symbol (and HNSW's proc 3 now checked for `LANGUAGE c`);
   - seeded IVFFlat builds byte-identical to C in all eight cases, with each case's blocks and lists;
   - every SQL-reachable IVFFlat error site identical, LOCATION included (18 errors, 4 NOTICEs);
   - upstream's extension directory shipped verbatim, the update path from 0.8.6, the GUC prefixes reserved, and `pg_get_loaded_modules()` at 0.8.7;
   - any Task 5–7 fix, with its test.
6. **Known divergences left open:**
   - core LOCATIONs `reloptions.c:0` and `guc.c:0` (a new core follow-up);
   - the static `ivfflat.*` and `hnsw.*` GUCs exist before the library loads, while C defines them at `_PG_init`;
   - an unknown or non-C support proc 5 raises where C calls it;
   - the scan keeps a detoasted copy of the query;
   - an insert into a list with no insert page raises where C asserts;
   - no parallel build (M6);
   - HNSW's build phase names are not reported (pre-existing);
   - ANALYZE's reservoir is seeded from a u64 where C uses a u32 (unobservable; IVFFlat uses the u32);
   - type info is resolved on every call rather than cached in `rd_amcache` (the plan's decision and its reason);
   - for M5: C's cosine and inner-product k-means normalize with vectorized norm sums, which pgrust sums in source order (M2's no-reduction-emulation ruling), so those opclasses are not in the byte-identical tier.
   - Carry forward the M3 report's open items that M4 did not touch.
7. **Deferred minors:** carry every M3 minor M4 did not absorb. M4 absorbed:
   - `fmgr_stubs` in `UT_KEEP`;
   - `hnsw_get_type_info`'s missing `prolang` check and its `utils.rs:116` comment;
   - `pgvector/src/lib.rs`'s `hnsw.c:266-402` citation;
   - CLAUDE.md's `run-bytecmp.sh [case ...]`;
   - spec §5.4's split parenthetical and "by function name";
   - `strip_hnsw_memory`'s comment, which now names `hnswvacuum.c:171`.

   Add any minors this milestone's reviews defer.
8. **Coverage limits:**
   - `wasm/wasm-build.sh` was not run (`nightly-2026-07-17` is not installed).
   - The harness ran only on macOS arm64.
   - The cosine and inner-product IVFFlat opclasses are covered by regress, TAP and sqldiff, not by byte identity.
   - Recall assertions after a parallel build ran against serial builds.

- [ ] **Step 6: Commit**

```bash
git add -A scripts/pgvector docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m4-report.md
git commit -q -F - <<'EOF'
docs(pgvector): M4 report; spec and CLAUDE.md at M4; retire upstream-sql.sh

Records the M4 conformance run (IVFFlat serial port, verbatim 0.8.7
extension), corrects the spec's type-info wording to the C symbol, and
brings the tier rows, CLAUDE.md and the harness up to what was built.

Co-Authored-By: <the implementing model>
EOF
git log --oneline main..HEAD
```
Expected: 8 or 9 commits on `vector/m4`: the plan, Tasks 1–6, Task 7's (if any) and Task 8's. Do not merge or push; the controller does that after the final review and the user's go-ahead.

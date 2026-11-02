# pgvector conformance after M6

Date: 2026-10-22 · pgrust commit: `54ed0e89d8` (the server binary is Task 6's build of `5b65193d8a`'s code; Task 7 and Task 8's commit `36bec2f137` change no Rust outside two comment headers; the final review fixes of 2026-11-02 come after, in What M6 built) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`, Apple clang 21.0.0), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY -DIVFFLAT_BENCH), Docker `pgvector/pgvector:0.8.7-pg18` and `pgvector/pgvector:0.8.1-pg18`, both linux/arm64 and pinned by digest · Plan: `docs/superpowers/plans/2026-10-22-pgvector-m6-parallel-builds.md` · Ledger: `~/.cache/pgrust/m6-sdd-ledger.md`

## Summary
- **C reference:** 139/139 (regress 14/14, TAP 48/48, bytecmp 29/29, iterscan 19/19, diff 22/22, ondisk 7/7).
- **pgrust:** 139/139 (regress 14/14, TAP 48/48, bytecmp 29/29, iterscan 19/19, diff 22/22, ondisk 7/7).
  - Before M6 the totals were C 137/137 and pgrust 129/137 (`docs/HANDOFF_2026-10-22.md` before `36bec2f137`). M6 adds two rows, bytecmp's `spill` (promoted from opt-in, Task 5) and the diff tier's `parallel_build` (Task 3), and the 8 TAP files that failed before, `003`, `012`, `020`, `024`, `028`, `032`, `035` and `045`, now pass.
  - In this run's pgrust TAP logs those 8 files print 36 `using N parallel workers` lines (3, 8, 4, 8, 8, 3, 1 and 1), the 36 lines that failed before M6, and `045` prints `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`, the 37th (`~/.cache/pgrust/m6-evidence/task8/tap-pgrust-prove/`).
  - Neither report has a `(run-X.sh exited N)` or `(no results ...)` row. The on-disk reverse rows are as in M5: every row `ok`, `identical` or `clean` in both modes, and `clean_pgrust_rows` lists the three pgrust-only `pg_proc` rows for pgrust and `none` for `ref`.
- **Spec §9's M6 exit criteria hold:** "Loom tests pass; recall TAP tests pass with workers forced; build time against pgvector reported." The 5 loom models pass; the 8 parallel TAP files pass in `run-all.sh pgrust` and in three more runs; the build-time table is below (Build time).
- **No plan-defined stop was hit** (Findings).
- **The final whole-branch review and Task 8's docs review asked for fixes, made in one commit on 2026-11-02** (Final review fixes, under What M6 built). Its checks, on AC power with the machine awake: `parallel_build` identical twice in each mode with five new REINDEX and in-transaction cases, diff 22/22 in both modes, bytecmp 29/29, iterscan 19/19, the 8 parallel TAP files 8/8, five `harness_test.sh` self-tests (9 ok), and the unit tests, clippy, the lints and the 5 loom models pass. `run-all.sh` was not rerun.
- **Gate table.** Every command ran under `caffeinate -dimsu` from the repo root, each log in `~/.cache/pgrust/m6-evidence/task8/`. "Asleep" sums, within the run's window, the time from each `Sleep` entry of `pmset -g log` to the next `DarkWake` (`timings-asleep.tsv`; the events are in `<name>.sleep.txt`).

  | Command | Verdict | Wall | Asleep (Sleep cycles) | Log |
  |---|---|---|---|---|
  | `scripts/pgvector/build-pgrust.sh` | built; cargo finished in 1.79 s with nothing to compile | 3 s | 0 s (0) | `build-pgrust.log` |
  | `scripts/pgvector/run-all.sh ref` | **139/139** | 1769 s | 1263 s (11) | `run-all-ref.log`, `report-ref.md`, `summary-ref-<tier>.tsv`, `reverse-ref.tsv` |
  | `scripts/pgvector/run-all.sh pgrust` | **139/139** | 4486 s | 3654 s (18) | `run-all-pgrust.log`, `report-pgrust.md`, `summary-pgrust-<tier>.tsv`, `reverse-pgrust.tsv`, `tap-pgrust-prove/` |
  | `scripts/pgvector/tests/harness_test.sh` | all passed, **201 ok**, 0 FAIL | 1094 s | 583 s (11) | `harness-test.log` |
  | `scripts/pgvector/run-repair-check.sh` | **6/6** | 205 s | 201 s (1) | `repair-check.log`, `summary-repair-check.tsv` |
  | the 8 parallel TAP files, run 1 | **8/8** | 772 s | 634 s (3) | `tap8-run1.log`, `summary-tap8-run1.tsv`, `tap8-run1-prove/` |
  | the 8 parallel TAP files, run 2 | **8/8** | 1415 s | 1260 s (3) | `tap8-run2.log`, `summary-tap8-run2.tsv`, `tap8-run2-prove/` |
  | the 8 parallel TAP files, run 3 | **8/8** | 845 s | 697 s (4) | `tap8-run3.log`, `summary-tap8-run3.tsv`, `tap8-run3-prove/` |
  | `cargo check -p pgvector_hnsw -p pgvector_ivfflat` | exit 0, no diagnostic in either crate | 430 s | 400 s (1) | `cargo-check.log` |
  | unit tests (below) | all pass | 2–1054 s | (below) | `unit-<crate>.log` |
  | loom (below) | **5 passed** | 3 s | 0 s (0) | `loom.log` |
  | `cargo test -p seams_init --test lint_seam_installs` | 1 passed | 206 s | 102 s (3) | `lint-seam-installs.log` |
  | `cargo test -p seams_init --test lint_determinism` | 11 passed | 23 s | 0 s (0) | `lint-determinism.log` |
  | `bash crates/_support/seams_init/tests/lint-determinism.sh` | `lint-determinism PASS (0 violations; 43 warning(s))` | 19 s | 0 s (0) | `lint-determinism-sh.log` |
  | `cargo test -p seams_init --test lint_walker_arms` | 1 passed | 2 s | 0 s (0) | `lint-walker-arms.log` |
  | `cargo test -p catalog_index --test lint_inplace_locks` | 1 passed | 6 s | 0 s (0) | `lint-inplace-locks.log` |
  | `PGRUST_VECTOR_INDEXBUILD_POOL=0 scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/parallel-build.sql` | **DIFFERENT** (exit 1), as expected | 16 s | 0 s (0) | `killswitch.log`, `killswitch/` |
  | `scripts/pgvector/run-buildtime.sh pgrust` | a report (Build time) | 228 s | 151 s (2) | `buildtime.log`; the report in `~/.cache/pgrust/m6-evidence/buildtime/` |
  | `~/.cache/pgrust/pgvector-rust-check/run.sh 55491` | **26/26** | 1 s | 0 s (0) | `client-check.log`, `client-setup.log` |
  | `git status --porcelain --ignored crates/pgvector-0.8.7-reference crates/postgres-18.6-reference` | empty | — | — | `reference-trees.log` (0 lines) |

- **`harness_test.sh`: 201 ok.** That is the 195 before M6 (`~/.cache/pgrust/v031-evidence/harness_test.log`) plus Task 2's 2 (`sqldiff: parallel builds compare equal`, `sqldiff: parallel build kept, participant lines dropped`), Task 7's 3 (`buildtime ref: exit 0`, `4 timed rows`, `workers launched as forced`) and 1 from `spill`'s promotion: `test_bytecmp_cases` checks every default case, so it now prints `bytecmp spill: 79 blocks, height 2`. Two existing checks changed their counts: `bytecmp ref: 29 ok` (was 28) and `diff ref: 22 ok` (was 21). The ok lines were diffed against the pre-M6 log; the only other changed lines are the self-test capture's Generic record counts (545,022 records, 2,624 with full-page images, 75,386 from the open transaction), which vary per capture.
- **The 8 parallel TAP files, three more runs** (`PGV_WORK=$HOME/.cache/pgrust/m6-work scripts/pgvector/run-tap.sh pgrust 003_… 012_… 020_… 024_… 028_… 032_… 035_… 045_…`): 8/8 each time. Every run's prove logs have the same counts: `using N parallel workers` 3, 8, 4, 8, 8, 3, 1 and 1; `after 0 tuples` 0, 4, 2, 4, 4, 0, 0 and 1; no `not ok`; `Result: PASS` for each file. In run 1, `045` printed `using 1 parallel workers`, the NOTICE, `worker processed 471 tuples` and `leader processed 529 tuples`.
- **Unit tests** (each `cargo test -p <crate> --lib`, the planner's filtered to `index_workers`): `planner` 2 passed (203 filtered out), `types_relscan` 3, `execindexing` 18, `catalog_index` 5, `pgvector_parallel` 1, `pgvector_hnsw_graph` 16, `pgvector_hnsw_build` 2, `pgvector_ivfflat_build` 7, `pgvector_hnsw` 13, `pgvector_ivfflat` 8, `types_hnsw` 6; 0 failed in each. The longest walls are compiles that a sleep interrupted (`pgvector_hnsw`: 1054 s, 1051 s of it asleep; `catalog_index`: 299 s, 287 s asleep; `planner`: 77 s, 26 s asleep); no other unit run was asleep.
- **Loom** (`RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`): `a_flush_waits_for_the_insert_in_flight`, `concurrent_first_inserts_serialize_on_the_entry_lock`, `concurrent_duplicates_merge_into_one_element`, `concurrent_inserts_under_a_shared_entry_lock_stay_reachable` and `three_inserts_with_an_entry_point_raise` ok; `5 passed ... finished in 0.28s`.
  - Task 4's per-model runs (`~/.cache/pgrust/m6-evidence/task4/loom-per-model.log`, `loom-iterations.log`; each model alone with `--exact`; shell wall including cargo's startup, then libtest's time, then loom iterations counted with a temporary counter that was reverted): entry-lock first inserts 1 s, 0.01 s, 181; shared-entry-lock reachability 1 s, 0.02 s, 494; duplicate merge 1 s, 0.01 s, 238; flush waits for the insert in flight 0 s, 0.00 s, 41; three inserts raising the entry point 0 s, 0.12 s, 1942. Preemption bounds as written (3, 3, 3, 3, 2).
  - **The red check** (`task4/loom-red-check.log`): with the exclusive entry-lock branch skipped and the entry point raised check-then-act after the shared guard drops, `concurrent_first_inserts_serialize_on_the_entry_lock` fails at `tests/loom.rs:52` (`left: 0, right: 1`: both first inserts saw no entry point, and the level-0 insert stored its entry point last); the other four pass. The brief's literal variant (the shared lock only, unconditionally) trips the `unreachable!` guard at `algo.rs:113` on any first insert instead of showing the race, so it is recorded but not the red check. `algo.rs` was restored byte for byte (`cmp`).
- **Lints:** `lint_seam_installs`, `lint_determinism` (11 tests), `lint_walker_arms` and `lint_inplace_locks` pass. `lint-determinism.sh` reports 0 violations and 43 warnings, the same 43 as before M6; the ledger's only new row is Task 2's `env` row for the kill switch.
- **The kill switch reached the server through `sqldiff.sh`.** `server_start` (`common.sh`) runs the pgrust server in a subshell of the script, so a variable in the script's environment is in the server's. The result shows it took effect: pgrust's transcript (`task8/killswitch/pgrust.out`) has 12 lines ending `parallel workers`, all of them index.c's `with request for N parallel workers` (5 IVFFlat builds, 7 HNSW), and no `using N parallel workers` line, where C's (`killswitch/ref.out`) has 12. The diff (`killswitch/diff.txt`) only removes lines: the 12 `using` lines and `h_disk`'s NOTICE, DETAIL and HINT (a serial 4 MB build of that table never overflows, as Task 6 found). Every build ran serially, and every other line, the errors and counts included, matched C.
- **The VERBOSE spot-check** (Task 5's review): the bytecmp `spill` case's SQL (`maintenance_work_mem = '1MB'`, the 2,003-row Golomb table, the HNSW index) under `\set VERBOSITY verbose` on fresh servers (`server.sh fresh`), with `SET pgrust.hnsw_build_seed = 42` on pgrust. Every server printed `LOCATION:  InsertTuple, hnswbuild.c:545` under the NOTICE, with C's DETAIL and HINT:
  - pgrust (`PostgreSQL 18.6 (pgrust 0.3.1)`): `after 1436 tuples` (`task8/verbose-notice-pgrust.out`);
  - stock C (`ref`, which never seeds HNSW builds): `after 1432 tuples` (`verbose-notice-ref.out`). Its levels are random, so its flush point varies;
  - so the seeded C build (`seeded`, port 55494, the bytecmp oracle) ran it too: `after 1436 tuples`, followed by its `-DHNSW_MEMORY` line `INFO:  00000: memory: 1 MB` (`verbose-notice-seeded.out`).
  
  All three servers were stopped afterwards.
- **The pgvector-rust client check** (`~/.cache/pgrust/pgvector-rust-check/run.sh`, against `PGV_LISTEN=localhost scripts/pgvector/server.sh fresh pgrust` with the database `pgvector_rust_test`): `test result: ok. 26 passed; 0 failed`, as before M6. Its crates were already compiled, so the run took 1 s.
- **Commits** (from `353c552351` on, on `vector/m6`, not pushed):

  | Commit | Task | Change |
  |---|---|---|
  | `353c552351` | — | the M6 plan |
  | `15843b0a96` | — | the pre-M6 handoff (rewritten by `36bec2f137`) |
  | `0ac809f915` | 1 | `plan_create_index_workers`, index.c's request line, the build scan's parallel arm |
  | `647aa3f05e` | 2 | `pgvector_parallel` and the kill switch; the harness pin and participant filter |
  | `0d659ec9ff` | 3 | parallel IVFFlat builds; the `parallel_build` diff row |
  | `0a35c5045b` | 3, fix round | one rows mailbox per worker; the backpressure case `ivf_wide` |
  | `04edf76b36` | 4 | `pgvector_hnsw_graph`: C's lock protocol, C's memory arithmetic, loom models |
  | `a5a1e6d672` | 5 | the serial HNSW build on the graph, with C's flush point; `spill` promoted |
  | `5b65193d8a` | 6 | parallel HNSW builds; `parallel_build` covers CIC, errors, cancel and duplicates |
  | `54ed0e89d8` | 7 | `run-buildtime.sh`, the build-time report |
  | `36bec2f137` | 8 | the comment headers, the spec, CLAUDE.md, this report and the handoff |
  | this commit | final review | the final review's fixes (What M6 built) |

- **How the run was produced.**
  - `build-pgrust.sh` found `target/fast-profile/postgres` current (cargo compiled nothing in 1.79 s; the binary's mtime, 16:12:21 on 2026-10-22, is Task 6's build and did not change). Task 7 changed only `scripts/`.
  - Order: `build-pgrust.sh`, `run-all.sh ref` (17:28:24-17:57:53), `run-all.sh pgrust` (17:59:27-19:14:13), `harness_test.sh`, `run-repair-check.sh`, the three TAP runs, the kill switch, the VERBOSE spot-check, `run-buildtime.sh pgrust`, the client check. Each verdict was copied to `~/.cache/pgrust/m6-evidence/task8/` before the next command ran.
  - **One overlap:** the machine slept for most of `run-all.sh pgrust`, so to use its waking minutes the cargo commands (`cargo check`, unit tests, loom, lints and the reference-tree check) ran alongside it, 18:35:23-19:12:52. `run-all.sh pgrust`'s wall time includes that contention; its verdicts are counts. Everything from `harness_test.sh` on ran alone, the build-time rerun included.
  - **Power:** on battery the whole time, 65% discharging at the start and 61% at the build-time rerun, with the lid closed (`ioreg`: `AppleClamshellState = Yes`). macOS entered Maintenance Sleep / DarkWake cycles during most long commands, and `caffeinate -dimsu` does not prevent sleep on battery with the lid closed. The longest single sleep, 1051 s from 18:49:50 to 19:07:21, fell inside `run-all.sh pgrust` (and the cargo commands beside it). So every wall time above is as measured, mostly sleep; the verdicts are counts and do not depend on it. The pre-M6 timings with the machine awake were about 330 s for `run-all.sh ref`, 465 s for `run-all.sh pgrust` and 6 minutes for `harness_test.sh` (the plan's Execution notes).
  - The `ondisk` tier reused the cached capture (key `8342dca63fe99622`; neither run-all log has a `capture:` line), so no Docker container was created. `harness_test.sh` made its own capture under `pgvector-work/selftest`.

## pgrust results
| Tier | Test | Result |
|---|---|---|
| regress | bit | ok |
| regress | btree | ok |
| regress | cast | ok |
| regress | copy | ok |
| regress | halfvec | ok |
| regress | hnsw_bit | ok |
| regress | hnsw_halfvec | ok |
| regress | hnsw_sparsevec | ok |
| regress | hnsw_vector | ok |
| regress | ivfflat_bit | ok |
| regress | ivfflat_halfvec | ok |
| regress | ivfflat_vector | ok |
| regress | sparsevec | ok |
| regress | vector_type | ok |
| tap | 001_ivfflat_wal | ok |
| tap | 002_ivfflat_vacuum | ok |
| tap | 003_ivfflat_vector_build_recall | ok |
| tap | 004_ivfflat_vector_insert_recall | ok |
| tap | 005_ivfflat_query_recall | ok |
| tap | 006_ivfflat_lists | ok |
| tap | 007_ivfflat_inserts | ok |
| tap | 008_ivfflat_centers | ok |
| tap | 009_ivfflat_filtering | ok |
| tap | 010_hnsw_wal | ok |
| tap | 011_hnsw_vacuum | ok |
| tap | 012_hnsw_vector_build_recall | ok |
| tap | 013_hnsw_vector_insert_recall | ok |
| tap | 014_hnsw_vector_vacuum_recall | ok |
| tap | 015_hnsw_vector_duplicates | ok |
| tap | 016_hnsw_inserts | ok |
| tap | 017_hnsw_filtering | ok |
| tap | 018_aggregates | ok |
| tap | 019_storage | ok |
| tap | 020_hnsw_bit_build_recall | ok |
| tap | 021_hnsw_bit_insert_recall | ok |
| tap | 022_hnsw_bit_vacuum_recall | ok |
| tap | 023_hnsw_bit_duplicates | ok |
| tap | 024_hnsw_halfvec_build_recall | ok |
| tap | 025_hnsw_halfvec_insert_recall | ok |
| tap | 026_hnsw_halfvec_vacuum_recall | ok |
| tap | 027_hnsw_halfvec_duplicates | ok |
| tap | 028_hnsw_sparsevec_build_recall | ok |
| tap | 029_hnsw_sparsevec_insert_recall | ok |
| tap | 030_hnsw_sparsevec_vacuum_recall | ok |
| tap | 031_hnsw_sparsevec_duplicates | ok |
| tap | 032_ivfflat_halfvec_build_recall | ok |
| tap | 033_comparison | ok |
| tap | 034_distance_functions | ok |
| tap | 035_ivfflat_bit_build_recall | ok |
| tap | 036_ivfflat_bit_centers | ok |
| tap | 037_inputs | ok |
| tap | 038_hnsw_sparsevec_vacuum_insert | ok |
| tap | 039_hnsw_cost | ok |
| tap | 040_ivfflat_cost | ok |
| tap | 041_ivfflat_iterative_scan | ok |
| tap | 042_ivfflat_iterative_scan_recall | ok |
| tap | 043_hnsw_iterative_scan | ok |
| tap | 044_hnsw_iterative_scan_recall | ok |
| tap | 045_hnsw_low_memory_build | ok |
| tap | 046_hnsw_vacuum_scan | ok |
| tap | 047_hnsw_vacuum_insert | ok |
| tap | 048_ivfflat_vacuum_insert | ok |
| bytecmp | l2_golomb | ok |
| bytecmp | l1_golomb | ok |
| bytecmp | m4_golomb | ok |
| bytecmp | wide_golomb | ok |
| bytecmp | unlogged_golomb | ok |
| bytecmp | insert_golomb | ok |
| bytecmp | l2_grid | ok |
| bytecmp | ip_grid | ok |
| bytecmp | dups | ok |
| bytecmp | big_golomb | ok |
| bytecmp | spill | ok |
| bytecmp | halfvec_l2_grid | ok |
| bytecmp | halfvec_cosine_grid | ok |
| bytecmp | sparsevec_l2_grid | ok |
| bytecmp | sparsevec_l2_frac | ok |
| bytecmp | sparsevec_cosine_grid | ok |
| bytecmp | insert_sparsevec_cosine | ok |
| bytecmp | wide_sparsevec | ok |
| bytecmp | bit_hamming | ok |
| bytecmp | bit_jaccard | ok |
| bytecmp | bq_expression | ok |
| bytecmp | ivf_l2_groups | ok |
| bytecmp | ivf_big | ok |
| bytecmp | ivf_halfvec_groups | ok |
| bytecmp | ivf_bit_hamming | ok |
| bytecmp | ivf_insert | ok |
| bytecmp | ivf_unlogged | ok |
| bytecmp | ivf_vacuum | ok |
| bytecmp | ivf_wide | ok |
| iterscan | relaxed_64k | ok |
| iterscan | relaxed_64k_x2 | ok |
| iterscan | strict_64k | ok |
| iterscan | relaxed_128k | ok |
| iterscan | relaxed_tuples | ok |
| iterscan | strict_tuples | ok |
| iterscan | lateral_64k | ok |
| iterscan | big_relaxed_64k | ok |
| iterscan | big_relaxed_256k | ok |
| iterscan | big_strict_128k | ok |
| iterscan | big_ef100_128k | ok |
| iterscan | big_lateral_128k | ok |
| iterscan | big_exhaust_4MB | ok |
| iterscan | halfvec_64k | ok |
| iterscan | halfvec_cosine_64k | ok |
| iterscan | sparsevec_64k | ok |
| iterscan | sparsevec_cosine_drop_64k | ok |
| iterscan | bit_hamming_64k | ok |
| iterscan | bit_jaccard_64k | ok |
| diff | setup | ok |
| diff | io | ok |
| diff | typmod | ok |
| diff | cast | ok |
| diff | arith | ok |
| diff | distance | ok |
| diff | norm | ok |
| diff | normalize | ok |
| diff | denormal | ok |
| diff | agg | ok |
| diff | cmp | ok |
| diff | misc | ok |
| diff | bit | ok |
| diff | knn | ok |
| diff | limits | ok |
| diff | seeded | ok |
| diff | error_locations | ok |
| diff | hnsw_errors | ok |
| diff | ivfflat_errors | ok |
| diff | library_load | ok |
| diff | core_error_locations | ok |
| diff | parallel_build | ok |
| ondisk | clean_queries | ok |
| ondisk | clean_followup | ok |
| ondisk | crash_pages | ok |
| ondisk | crash_queries | ok |
| ondisk | crash_followup | ok |
| ondisk | v081_update | ok |
| ondisk | v081_followup | ok |

**139/139 passed** (`~/.cache/pgrust/m6-evidence/task8/report-pgrust.md`). No row fails, so this report has no "Remaining failures" section.

## What M6 built
**Task 1, the core (`0ac809f915`).** `plan_create_index_workers` is ported step by step from `planner.c:7097-7239` into `crates/backend/optimizer/plan/planner/src/index_workers.rs`, behind the new slot `planner_seams::plan_create_index_workers` (set in `planner::init_seams`), including the 32 MB-per-participant clamp that B-tree's pool helper omits. `IndexAmKind::amcanbuildparallel` (`types_relscan`) is true for HNSW and IVFFlat only. `index_build` gains C's `parallel` argument, its four callers pass it, and it prints index.c's two DEBUG1 lines (`index.c:3066` "serially", `index.c:3072` "with request for N parallel workers"). `execindexing`'s build scan gains the parallel-scan arm (`heapam_handler.c:1272-1281`) and the re-export `table_index_build_scan_parallel`, with a SnapshotAny lane for CREATE INDEX and a serialized-MVCC-snapshot lane for CREATE INDEX CONCURRENTLY. `tableam` moved from `execindexing`'s dev-dependencies to its dependencies (a controller ruling; no cycle).

**Task 2, `pgvector_parallel` and the harness (`647aa3f05e`).** The new crate `crates/contrib/pgvector_parallel` holds what `hnswbuild.c` and `ivfbuild.c` share: the kill switch (`vector_parallel_builds_enabled`, `PGRUST_VECTOR_INDEXBUILD_POOL=0`), `begin_parallel` (enter parallel mode, `CreateParallelContext("postgres", <pgrust entry>, n)`, the scan snapshot, `table_parallelscan_initialize`, the AM's shared state as the context's private `Arc`, `LaunchParallelWorkers`, C's "using N parallel workers", and the serial back-out when none launched), `end_parallel`, the worker prologue (`worker_open` / `worker_close` with C's lock modes), `report_processed` (the per-participant DEBUG1 lines) and the latch wake. `lint-determinism.allow` gains its one `env` row (budget 1, `DST-REVIEW(m6-vector-parallel)`). The harness pins `min_parallel_table_scan_size=8MB` for pgrust servers (`common.sh`'s `PGRUST_SERVER_OPTS`, `run-tap.sh`'s shim), and `sqldiff.sh` drops each participant's `leader/worker processed N tuples` line and every `CONTEXT:  parallel worker` line. New self-test: `test_sqldiff_parallel_counts` (2 checks).

**Task 3, the parallel IVFFlat build (`0d659ec9ff`, fix round `0a35c5045b`).** `crates/contrib/pgvector_ivfflat_build/src/parallel_build.rs`: the leader samples, runs k-means and writes the list pages as before; then every participant, the leader included, scans its share of the heap through the parallel scan, assigns each row to its nearest center and feeds `(list, tid, value)` to the leader's one tuplesort. Workers batch their rows (64 KB of values per batch) and send them through their own bounded mailbox (64 batches), parking interruptibly when it is full; the leader drains every mailbox while it scans and after every participant is done. The parallel context ends right after the scan. `scripts/pgvector/sql/parallel-build.sql` (new) holds the IVFFlat cases (request and `using` lines, the four builds that must stay serial, the reloption past the memory clamp, CREATE INDEX CONCURRENTLY, an unlogged table, and `ivf_wide`, the backpressure case), and `run-diff.sh` gains the `parallel_build` row (diff 21 → 22). It was the first end-to-end run of a launched worker building an index, and the plan's stop for a worker that cannot open the build's relations or run the scan did not fire.

**Task 4, `pgvector_hnsw_graph` (`04edf76b36`).** A new leaf crate, `crates/contrib/pgvector_hnsw_graph`, depending only on `pgsync` and `pairingheap` (plus `loom` under `cfg(loom)`): an append-only arena of u32 handles (`arena.rs`); C's lock set on `pgsync` (`flush_lock`, `allocator`, `entry_wait_lock`, `entry_lock`, `list` for `graph->lock`, one `RwLock<Links>` per element) and `Graph::insert`, InsertTuple's in-memory phase with C's lock order (`hnswbuild.c:437-580`, checked line by line in review); the in-memory algorithm, moved verbatim from `pgvector_hnsw_build` with C's locks added (`algo.rs`); and `memory.rs`, a model of `generation.c`'s block arithmetic for serial builds and of the parallel area's MAXALIGNed bump allocation with its 1 MB margin, so the flush point is C's. 16 unit tests and 5 loom models (`tests/loom.rs`).

**Task 5, the serial HNSW build on the graph (`a5a1e6d672`).** `pgvector_hnsw_build` builds through `Graph::insert`; its own copy of the in-memory algorithm is deleted. The serial flush point moved from pgrust's estimate (1840 tuples in `spill`) to C's (1436), pinned by the unit test `serial_flush_point_matches_c_at_one_megabyte`. The NOTICE now reports C's location (`hnswbuild.c:545`, `InsertTuple`), and the build reports C's progress (`PROGRESS_HNSW_PHASE_LOAD`, a new constant in `types_hnsw`, and tuples done). bytecmp's `spill` switched to tie-free Golomb data (p = 2003) and, after six identical runs, became a default case (bytecmp 28 → 29).

**Task 6, the parallel HNSW build (`5b65193d8a`).** `crates/contrib/pgvector_hnsw_build/src/parallel_build.rs`: `compute_parallel_workers` (the kill switch, then C's planner zero check and `max_parallel_maintenance_workers` or the capped reloption), and `build_graph_parallel`: every participant, the leader included, scans its share and inserts into one shared `Graph` under C's parallel accounting (`maintenance_work_mem` − 3 MB, 1 MB margin). Whichever participant overflows prints the NOTICE and flushes; everyone then inserts on disk. The leader flushes the graph if nobody did, in parallel mode, before the context ends, as C's BuildGraph does; `log_newpage_range` runs after. `parallel-build.sql` gains the HNSW cases: request and `using` lines, an on-disk build (`after 0 tuples`), CREATE INDEX CONCURRENTLY, a temp table and a parallel-unsafe expression (serial), a `binary_quantize` expression, an unlogged table, duplicates, a row error (`sv_bad`) and a `statement_timeout` cancel.

**Task 7, the build-time report (`54ed0e89d8`).** `scripts/pgvector/run-buildtime.sh {pgrust|ref}` times HNSW and IVFFlat builds of one table (100,000 rows of `vector(128)` by default) on a fresh C reference and a fresh subject, serially and with 1, 2 and 4 workers forced through the `parallel_workers` reloption, and writes `report.md` and `summary.tsv` with each server's launched count and time. New self-test: `test_buildtime_ref` (3 checks). A report, not a gate.

**Task 8 (`36bec2f137`):** the gate below, the comment headers of `pgvector_hnsw` and `pgvector_ivfflat`, the spec in place (§7.1 with ruling M6-1, §7.2, §7.3, §8.2, §8.3), CLAUDE.md, this report and the handoff.

**Final review fixes (this commit, 2026-11-02).** The final whole-branch review (the commits after `15843b0a96` through `36bec2f137`) returned two Important findings and triaged 15 deferred minors as fix-before-merge; Task 8's docs review returned two Important one-liners and six minors. One commit makes the fixes the controller ruled on (ledger):
- **IVFFlat's leader scans with a fresh `IndexInfo`** (`BuildIndexInfo` and `ii_Concurrent`, `ivfbuild.c:677-678`), as C's leader and pgrust's HNSW leader do; it used `index_build`'s. `assign_tuples_parallel` now takes the caller's `ii_Concurrent` flag instead of its `IndexInfo` (Decision 10's text is corrected to match).
- **The build scan's parallel arm raises an ERROR when the heap relation is not the file the parallel scan was initialized for** (DIVERGENCE: C asserts this in `table_beginscan_parallel`, `tableam.c:172`), so a participant whose relcache missed an in-transaction relfilenode change fails instead of scanning a stale file. An MVCC parallel scan without a serialized snapshot is now an ERROR too, where it scanned under SnapshotAny without a word.
- **HNSW's leader raises an ERROR when fewer participants than launched counted themselves done** (`hnswbuild.c:769-782`); it was a `debug_assert_eq!`, which `fast-profile` compiles out.
- **Smaller code changes:** `Leader` is `#[must_use]`; `pgvector_hnsw_build` drops `types_fmgr`, `types_nodes` and the `pgvector` dev-dependency (`Cargo.lock` refreshed); `pgvector_parallel` drops a needless `clone()`, a double blank line and a wrong citation, and its kill-switch check uses `is_none_or` (a clippy error that a forced re-check surfaced: the crate's earlier clippy runs had replayed cached results).
- **Comments:** the wait-event DIVERGENCE at the three wait sites; IVFFlat's in-flight batches outside `maintenance_work_mem` and the reason for one mailbox per worker; the tie-order qualifier (`algo.rs`: C's pointers rise with allocation only within one 1 MB Generation block); the arena's under-lock segment allocation and uncounted slot arrays (DIVERGENCE); `flush_pages`' DIVERGENCE (C resets `graphCtx` at the flush, and a cancel can raise mid-flush under `pgsync`'s lock); the leader's `bs.graph`; the stale headers of `build_scan.rs` and `pgvector_ivfflat_build`.
- **Scripts:** `run-buildtime.sh` stops its servers and removes its clusters unless `PGV_KEEP_DATA=1`; `test_buildtime_ref`'s redirection order; `sqldiff.sh` also drops `--verbose`'s `DEBUG:  00000:` count lines and the `LOCATION:` line after each.
- **New `parallel-build.sql` cases**, each identical to C in both modes, twice: `REINDEX INDEX` on a parallel-built IVFFlat index (`using 1 parallel workers`; 20000 rows through the index), `REINDEX INDEX` on an overflowing HNSW index (`using 2`, `after 0 tuples`; the nearest row 2500), `REINDEX INDEX CONCURRENTLY` on an HNSW index (`h_ric_ccnew`'s request line, `using 2`; 2500), and two one-transaction builds: `CREATE TABLE`, `INSERT` and an HNSW build (`using 2`; 2500), and `TRUNCATE`, `INSERT` and an IVFFlat build on a copy of `h` (`using 1`; 5000 rows). The `big` timeout case now runs at DEBUG1 and shows the request line and `using 2` before the timeout ERROR. An unfiltered run (`~/.cache/pgrust/m6-evidence/final-fix/unfiltered-pgrust.out`) shows pgrust's workers processing rows in every one of these builds, so they saw the leader's uncommitted relfilenodes and the new locator check did not fire.
- **The `--verbose` run** (evidence, not a gate): `sqldiff.sh ref --verbose` is identical; `sqldiff.sh pgrust --verbose` differs in one line, the statement timeout's `LOCATION:  ProcessInterrupts, postgres.c:3445`, where C reports `:3446` (`final-fix/verbose-pgrust-diff.txt`; Still open). Every other `LOCATION` line matches, the build lines' included.
- **wasm:** `wasm/wasm-build.sh` was not run. Its pinned toolchain `nightly-2026-07-17` is not installed (`rustup toolchain list` shows only `stable` and `1.96.0`), and the script installs it when it is missing, which this wave was not to do (`final-fix/wasm-attempt.log`).
- Evidence: `~/.cache/pgrust/m6-evidence/final-fix/`; the record: `.superpowers/sdd/2026-10-22-pgvector-m6-parallel-builds/final-fix-report.md`.

## Rulings
**The plan's decisions, as applied** (`docs/superpowers/plans/2026-10-22-pgvector-m6-parallel-builds.md`, "Decisions this plan makes"; the user approved them with the plan). Each says what it would cost if it were wrong.
- **M6-1 (Decision 1): launched workers, not the morsel pool. It amends spec §7.1.** Both AMs launch through `pgvector_parallel::begin_parallel`, `CreateParallelContext` to `DestroyParallelContext`, C's shape. The reasons: the pool's smallest claim is one heap block against a 2 ms target, and a block of HNSW inserts can take ten times that; the pool falls back to serial when no thread claims the work within 100 ms, so whether a build went parallel would depend on load, while 36 of the 37 TAP lines M6 had to pass check `using N parallel workers`; launched workers bring C's error, cancel and cleanup paths and C's `CONTEXT:  parallel worker`; and neither spawns raw threads. The spec now says so in place (§7.1). *If wrong:* a pool channel can be added later in vacuumlazy's shape. The per-row work (graph inserts, center assignment) does not depend on the mechanism.
- **Decision 2: the entry points register under `"postgres"` with pgrust names,** `pgrust_hnsw_parallel_build_main` and `pgrust_ivfflat_parallel_build_main`, since `LookupParallelWorkerFunction` accepts only `"postgres"`. *If wrong:* a rename, if the `parallel` crate ever resolves by library. Neither name is visible to SQL.
- **Decision 3: the graph keeps Rust values and charges C's sizes to a model of C's allocator** (`pgvector_hnsw_graph::memory`). Measured: the 1436 unit test, `spill`'s six identical runs, and TAP `045`'s `after 0 tuples`. *If wrong:* flush points drift from C's, which bytecmp's `spill` and TAP `045` would catch. The bytes actually held differ from C's in any case: a serial build that spills keeps about `maintenance_work_mem` of Rust-side graph through its on-disk phase (Findings).
- **Decision 4: the serial build moved onto the graph crate,** as C has one code path for both. The gate held: bytecmp 28/28 (then 29/29) and iterscan 19/19 after Task 5 and after Task 6, and again in this run. *If wrong:* a serial byte-identity regression, which those two tiers catch.
- **Decision 5: `spill` got tie-free data and was promoted** after six identical runs (three `ref`, three `pgrust`; both servers flush after 1436 tuples). *If wrong:* a flaky gated case. It would fail C against C first, and `run-all.sh ref` runs it.
- **Decision 6: IVFFlat streams rows to one leader sort, and the parallel context ends after the scan.** Applied with one mailbox per worker (controller ruling, below). *If wrong:* the order within a list already differs from C's (a recorded DIVERGENCE); and the leader's drain and its one sort are serial work that C spreads over its participants. Whether that limits scaling is not measured beyond the Build time table.
- **Decision 7: `plan_create_index_workers` lives in `planner`** (`index_workers.rs`) behind `planner_seams::plan_create_index_workers`, used by `index_build` and by both AMs, as in C. *If wrong:* moving a seam; no behaviour changes.
- **Decision 8: `amcanbuildparallel` is true only for HNSW and IVFFlat, and there is no `ii_ParallelWorkers` field.** *If wrong:* btree, GIN and BRIN builds keep printing `serially` at DEBUG1 where C prints `with request for N parallel workers`; and an AM that wanted the count from `IndexInfo` would need the field (none does today; pgvector never reads it).
- **Decision 9: CREATE INDEX CONCURRENTLY builds in parallel, as in C,** through the parallel scan's serialized MVCC snapshot. Covered by `ivf_cic` and `h_cic` in `parallel-build.sql`, identical to C. *If wrong:* a CIC build that sees the wrong rows. Only those two cases cover it; no TAP test does.
- **Decision 10: parallel scans drop `ii_BrokenHotChain`, as C's do** (`hnswbuild.c:804`, `ivfbuild.c:677` build a fresh `IndexInfo` for every participant's scan, the leader's included, and nothing propagates the flag). Bug-compatible. IVFFlat's leader scanned with `index_build`'s `IndexInfo` until the final review fixes; it now builds its own, as C's does. IVFFlat's serial sampling scan still runs on `index_build`'s `IndexInfo`, so a broken HOT chain in a sampled block sets the flag and `indcheckxmin`, as in C (`ivfbuild.c:151-152`); an HNSW parallel build never sets it. *If wrong:* pgrust shares C's exposure: a parallel-built HNSW index over broken HOT chains, or an IVFFlat index whose broken chains lie outside the sampled blocks, is not marked `indcheckxmin`, so a snapshot older than the build could use it. Fixing it in pgrust alone would diverge from C.
- **Decision 11: the kill switch is `PGRUST_VECTOR_INDEXBUILD_POOL=0`,** read on the leader at each build. It cost one `env` row in `lint-determinism.allow` (budget 1), the plan's only ledger change; `lint_determinism` still reports 43 warnings. *If wrong:* one ledger row.
- **Decision 12: the harness pins `min_parallel_table_scan_size=8MB` for pgrust servers.** pgrust ships 1 MB. *If wrong:* the pin hides pgrust's own default from the harness: at 1 MB, TAP `002`, `005`, `006` and `041`-`044` and the on-disk follow-up's `REINDEX` would build in parallel on pgrust and serially on C.
- **Decision 13: the diff tier's `parallel_build` row** (21 → 22 rows). `sqldiff.sh` drops the per-participant counts and every `CONTEXT:  parallel worker` line. *If wrong:* a wrong count split or a missing worker context line would pass the row. Tasks 3 and 6 checked them unfiltered once (`m6-evidence/task3/context-*.out`, `task6/extra-participants/`): the counts sum to the table's rows, and every worker line carries the context.
- **Decision 14: build time is a report, not a gate** (`run-buildtime.sh`, Build time below). *If wrong:* a performance regression would pass every gate.

**Controller rulings that changed what was built or measured** (from the ledger, `~/.cache/pgrust/m6-sdd-ledger.md`):
- **One mailbox per IVFFlat worker** (Task 3's fix round, `0a35c5045b`), where the plan had one shared mailbox. `pgsync`'s mailbox has a single sender-wake slot (`mailbox.rs:56`, `:184`, `:192-194`), so with two workers parked on one full mailbox a pop wakes only the last, and liveness rested on the 1000 ms recheck (a hang with `PGRUST_MQ_RECHECK_MS<=0`). Spec §7.3 asks only for streaming to one leader sort, which per-worker mailboxes keep, and widening `pgsync` would be a core edit. The fix also added `ivf_wide`, the one case that fills the mailboxes. *If wrong:* the structural change in `parallel_build.rs`.
- **`ivf_unsafe` and `h_unsafe` stay as the plan wrote them,** although both servers then error `column does not have dimensions` (a function's return type has no typmod). Review Focus 5 asks only that the build print `serially` like C, which it does before the error, and the error text is compared too. *If wrong:* one expression-index build that never completes; serial expression builds are covered by bytecmp's `bq_expression`.
- **`tableam` became a normal dependency of `execindexing`** (Task 1; it was a dev-dependency). The plan's own build-scan signature names `tableam::ParallelTableScanDescShared`, so the edit is implied by `execindexing`'s place in the core-edit list, and `tableam` does not depend on `execindexing` (no cycle). *If wrong:* one Cargo line.
- **`cargo clippy -p <crate> --no-deps -- -D warnings`** stands in for the plan's clippy commands, since unrelated workspace crates already fail clippy at HEAD. *If wrong:* nothing; the intent is the crate's own warnings.
- **The build-time rerun** (Task 7's runs overlapped Maintenance Sleep on battery): this task reran `run-buildtime.sh pgrust` once with the power state and sleep events recorded, and the report carries the numbers as measured (Build time). *If wrong:* a rerun on AC power, which the script makes a one-line job.
- **The final review's fixes are one commit** (2026-11-02): both Important findings adopted (IVFFlat's leader `IndexInfo`; the parallel scan's locator ERROR, with REINDEX and in-transaction cases in `parallel-build.sql`), without the optional worker-side check of the index's locator; the triage's 15 fix-before-merge minors and Task 8's review findings fixed alongside; the arena's under-lock allocation and the SipHash visited set parked (Still open); `wasm/wasm-build.sh` attempted once, without installing its toolchain. *If wrong:* one commit to amend.
- **The handoff is rewritten in place:** `docs/HANDOFF_2026-10-22.md` predated M6 on the same date; its v0.3.1-merge summary moves into the new §3. *If wrong:* a `git revert` (history keeps the pre-M6 file).

## Findings and known divergences
- **No plan-defined stop was hit.** No launched worker blocked on a lock its leader held; no default bytecmp or iterscan case differed after Task 5 or 6; no TAP recall assertion failed in any run; no loom model failed; no core edit fell outside the plan's list; and the determinism ledger gained only Task 2's row.
- **Entry-point names (DIVERGENCE, Decision 2).** C calls `CreateParallelContext("vector", "HnswParallelBuildMain" | "IvfflatParallelBuildMain", n)`. pgrust registers `pgrust_hnsw_parallel_build_main` and `pgrust_ivfflat_parallel_build_main` under `"postgres"`, and it passes the shared state as the context's private `Arc`, not through `shm_toc` keys (`pgvector_parallel/src/lib.rs:5-12`).
- **The graph's accounting model, and when its arena is freed (DIVERGENCE, Decision 3).** The flush point is C's, computed from C's allocation sizes and C's allocator arithmetic; the bytes pgrust actually holds differ. The arena is freed with the graph at the end of the build, not at the flush, where C resets the serial graph's memory context (`hnswbuild.c:315`). So a serial build that spills keeps about `maintenance_work_mem` of Rust-side graph through its on-disk phase (Task 5's report). One ordering difference cannot be observed: C draws an element's level after the element's own 128-byte allocation (`hnswutils.c:248-251`) and the graph draws it before; only a failure of that allocation could tell them apart, and the 1 MB margin makes that unreachable.
- **IVFFlat (DIVERGENCE, spec §7.3, Decision 6).** List membership is C's. The order within a list differs, which C's own sort (keyed on the list only, `ivfbuild.c:612-621`) does not fix either. Each worker streams its batches through its own bounded mailbox (the controller's ruling in Rulings), and the parallel phase ends right after the scan, where C keeps its context alive for the tape merge. In-flight batches (up to 64 batches of 64 KB per worker, plus each participant's partial batch) sit outside `maintenance_work_mem`, and the leader drains the mailboxes only from its scan callback on non-null rows and after the scan (the module's DIVERGENCE note says so since the final review fixes).
- **`amcanbuildparallel` is false for btree, GIN and BRIN (DIVERGENCE, Decision 8).** C says true for all three. pgrust builds them serially, so `index_build` prints `serially` for them where C requests workers.
- **No `ii_ParallelWorkers` field (Decision 8).** `index_build` keeps the count in a local; no pgrust AM reads it, and pgvector never does.
- **The kill switch (DIVERGENCE, spec §7.1, Decision 11).** `PGRUST_VECTOR_INDEXBUILD_POOL=0` in the server's environment makes every HNSW and IVFFlat build serial; C has no such switch. It is read on the leader at each build (`compute_parallel_workers`, `assign_tuples`); workers never read it. index.c's `with request for N parallel workers` still prints, since the core's worker count does not consult it (the check in Summary).
- **Wait events (DIVERGENCE).** C's leader sleeps on `workersdonecv` as `WAIT_EVENT_PARALLEL_CREATE_INDEX_SCAN` (`ivfbuild.c:649-650`, `hnswbuild.c:783-784`). pgrust's leaders wait as `PARALLEL_FINISH` (IVFFlat through `wait_parallel_finish_quantum`, HNSW through `WaitForParallelWorkersToFinish`), and so does an IVFFlat worker parked on its full mailbox, which has no C counterpart. Visible only in `pg_stat_activity`; marked DIVERGENCE at the three sites since the final review fixes.
- **A relayed worker error can carry the leader's current context lines (a core divergence found in Task 3's review).** C's `CreateParallelContext` saves the leader's `error_context_stack` (`parallel.c:195`), and `HandleParallelMessage` swaps it in around `ThrowErrorData` when it rethrows a worker's message (`parallel.c:1193-1200`). pgrust's `parallel` crate does not, so a worker error that the leader processes inside one of its own context callbacks gains that callback's lines. Task 3's check (`~/.cache/pgrust/m6-evidence/task3/worker-error-{pgrust,ref}.out`) built an IVFFlat expression index on a PARALLEL SAFE PL/pgSQL function that raises in workers only: pgrust prints one more CONTEXT line than C, `PL/pgSQL function leader_only(vector) line 2 at statement block`. No gated case hits it: the leader runs no context callback while it waits on a build scan, and `sv_bad`'s error is the leader's own. The crate is upstream-owned and outside M6's edit list (Still open).
- **Bug-compatible: parallel scans drop `ii_BrokenHotChain`, as C's do (Decision 10).** Every participant, the leader included, scans with a fresh `IndexInfo` (`hnswbuild.c:804`, `ivfbuild.c:677`; IVFFlat's leader since the final review fixes), and nothing carries the flag back to `index_build`. So a parallel-built HNSW index never gets `indcheckxmin` (`index.c:3154`), and a parallel-built IVFFlat index gets it only when its serial sampling scan, which uses `index_build`'s `IndexInfo`, meets a broken HOT chain (`ivfbuild.c:151-152`), as in C. pgrust reproduces both.
- **`spill`'s outcome: promoted.** With tie-free data (the Golomb ruler, p = 2003) it gave six identical runs, and both servers flushed after 1436 tuples in each (`~/.cache/pgrust/m6-evidence/task5/spill-{ref,pgrust}-{1,2,3}.log`). It is a default case now, and this task's runs pass it in both modes. The VERBOSE spot-check (Summary) shows the NOTICE at C's location, `InsertTuple, hnswbuild.c:545`, on pgrust, stock C and seeded C. Stock C flushed after 1432 tuples there: unseeded, its levels and so its flush point vary, which is why the gated comparison is against the seeded build.

## Build time
`scripts/pgvector/run-buildtime.sh pgrust`, rerun by this task (controller ruling), 20:27:08 to 20:30:56 on 2026-10-22: 100,000 rows of `vector(128)`; HNSW `m = 16`, `ef_construction = 64`; IVFFlat `lists = 100`; `maintenance_work_mem = 2GB`; one run per cell, C first, then pgrust; workers forced with the `parallel_workers` reloption (0 = serial). As measured (`~/.cache/pgrust/m6-evidence/buildtime/report.md`, `summary.tsv`):

| AM | workers | C launched | C ms | pgrust launched | pgrust ms | pgrust / C |
|---|---|---|---|---|---|---|
| hnsw | 0 | 0 | 68400.7 | 0 | 14124.1 | 0.21 |
| hnsw | 1 | 1 | 6896.6 | 1 | 10205.5 | 1.48 |
| hnsw | 2 | 2 | 5070.6 | 2 | 94536.9 | 18.64 |
| hnsw | 4 | 4 | 2013.8 | 4 | 3491.2 | 1.73 |
| ivfflat | 0 | 0 | 1499.0 | 0 | 2947.1 | 1.97 |
| ivfflat | 1 | 1 | 1012.6 | 1 | 1382.5 | 1.37 |
| ivfflat | 2 | 2 | 668.1 | 2 | 997.4 | 1.49 |
| ivfflat | 4 | 4 | 641.8 | 4 | 813.2 | 1.27 |

- **Power:** `pmset -g batt` read `Battery Power`, 61%, discharging, before and after the run (`task8/buildtime-pmset-batt-{before,after}.txt`). Nothing else ran.
- **Sleep events** (`task8/buildtime.sleep.txt`): Maintenance Sleep from 20:27:23 to a DarkWake at 20:28:25, and again from 20:29:10 to 20:30:39 (151 s asleep in all). By the per-build logs' modification times, the first falls inside C's serial HNSW build (20:27:14 to 20:28:28) and the second inside pgrust's 2-worker HNSW build (20:29:05 to 20:30:39). Those two cells measure mostly sleep. No sleep event falls inside any other cell.
- **Every launched count equals the request** (0, 1, 2 and 4 on both servers), so each parallel cell really ran with that many workers.
- **The range observed:** Task 7's two runs (same script and parameters, on battery at 66% with Maintenance Sleep in both; `~/.cache/pgrust/m6-evidence/task7/report-run1-with-sleep.log` and `report-run2.log`, the latter also in Task 7's report) gave, for C and pgrust in ms:
  - HNSW 0 workers: C 9258.0 and 55161.8, pgrust 28946.8 and 18415.9; 1 worker: C 6700.1 and 6482.2, pgrust 9901.4 and 9549.9; 2: C 5649.2 and 5303.1, pgrust 9349.3 and 29365.4; 4: C 3989.3 and 2060.8, pgrust 6147.1 and 5568.9.
  - IVFFlat 0 workers: C 574.2 and 1317.4, pgrust 43286.0 and 2230.3; 1: C 427.0 and 594.8, pgrust 625.9 and 1015.7; 2: C 394.6 and 632.6, pgrust 536.4 and 815.3; 4: C 446.9 and 659.3, pgrust 495.9 and 801.0.
- **Reading it.** In this run's six rows where neither cell holds a sleep (HNSW with 1 and 4 workers, IVFFlat with 0, 1, 2 and 4), pgrust / C is 1.27 to 1.97. Times fall as workers rise where no sleep intervened: C's HNSW 6896.6, 5070.6 and 2013.8 ms for 1, 2 and 4 workers; pgrust's IVFFlat 2947.1, 1382.5, 997.4 and 813.2 ms for 0, 1, 2 and 4. This run's two outliers (C's serial HNSW, pgrust's 2-worker HNSW) lie inside sleeps; Task 7's outliers (for example pgrust's serial IVFFlat at 43286.0 ms against 2230.3 ms in its other run) came from runs that overlapped Maintenance Sleep too, though its report does not place each sleep in a cell. The table is a report, not a gate (Decision 14). A run on AC power with the machine awake is one command: `scripts/pgvector/run-buildtime.sh pgrust`.

## Coverage limits
- **macOS arm64 only,** for pgrust and for the local C reference, as before. No x86_64 run.
- **The wasm build was not run** (`wasm/wasm-build.sh`). M6 changed non-test Rust, but wasm builds serially by construction: `IsUnderPostmaster` is false there, so `plan_create_index_workers` returns 0 and neither AM launches. The final fix wave tried once: the pinned `nightly-2026-07-17` is not installed, and the script would install it, so it was not run.
- **REINDEX, REINDEX CONCURRENTLY and in-transaction relfilenode changes are covered** since the final review fixes: `parallel-build.sql`'s `ivf_re`, `h_re`, `h_ric`, `t1` and `h2` cases are identical to C, and pgrust's workers process rows in each (`final-fix/unfiltered-pgrust.out`).
- **The morsel-pool channel was not built** (ruling M6-1). B-tree's pool arm is untouched.
- **No run has a worker perform the HNSW overflow flush.** In every `after 0 tuples` case Task 6 examined (TAP `045`, TAP `012`/`020`/`024`/`028`'s on-disk blocks, `h_disk`), the leader won the first-insert race and flushed, and the workers then inserted on disk (Task 6's review). The flush code is the same function for every participant, but no evidence shows it running in a worker.
- **An HNSW row error inside a worker is not exercised.** `sv_bad`'s bad row is read by the leader on both servers (its ERROR carries no `parallel worker` context, Task 6's extra check), so a worker's ERROR relay is covered only by Task 3's IVFFlat check (`~/.cache/pgrust/m6-evidence/task3/worker-error-*.out`), where the session survives and no index is left, as in C.
- **Loom's view of the graph:**
  - It does not model the arena's `OnceLock` publication: under `cfg(loom)` `pgsync::OnceLock` is std's. Publication is serialized under the allocator mutex, which loom does model, and a handle becomes reachable only through a neighbor list after its publish completed.
  - No loom model reaches `update_connection`'s shrink path (the in-place rewrite of a full neighbor list under the neighbor's write lock); a model with `m = 1` would.
- **Mailbox backpressure is exercised only by `ivf_wide`:** 4 parks per run (2 per worker) in both measured runs, counted with temporary instrumentation during Task 3's fix round and removed before the commit (`~/.cache/pgrust/m6-evidence/task3/fix1-instr-run{1,2}.log`). Every other IVFFlat case parks 0 times.
- **Recall is the check for parallel builds** (spec §7.2's Determinism): they are nondeterministic in C too, so bytecmp and iterscan stay serial and seeded, and `parallel-build.sql` compares messages and counts, not pages.

## Still open
**Not in M6** (the plan's Decision 15, and what review found outside its edit list):
- **A morsel-pool channel** for vector builds (ruling M6-1 leaves it possible, in vacuumlazy's shape).
- **HNSW build phase names:** pgrust's `pg_stat_progress_create_index` phase name for HNSW is still NULL (handoff §4). Task 5 added the phase number, `PROGRESS_HNSW_PHASE_LOAD`.
- **The stale banner of `crates/backend/access/nbtree/nbtsort/src/pool.rs`,** which still says `index_build` is serial-only. The file is upstream-owned, so it is left alone.
- **The `parallel` crate's `error_context_stack`** (Findings): `HandleParallelMessage` should rethrow a worker's message under the context stack saved at `CreateParallelContext` (`parallel.c:195`, `:1193-1200`). A core edit in an upstream-owned crate, outside M6's list.
- **`ProcessInterrupts`' statement-timeout location:** pgrust reports `postgres.c:3445` where C reports `:3446`, the line where its multi-line `ereport` closes (the final fix wave's `--verbose` run, `~/.cache/pgrust/m6-evidence/final-fix/verbose-pgrust-diff.txt`). A core edit outside M6's list; the gated comparisons do not print locations.

**The final whole-branch review's parked items** (controller rulings, ledger, 2026-11-02):
- **The arena's segment allocation** (`pgvector_hnsw_graph/src/arena.rs`): segment k's `1024 << k` slots are allocated inside `publish`, under the allocator lock, a one-time stall at each segment boundary that touches about 144 bytes per slot, and the slot arrays are not charged to the memory model. Marked DIVERGENCE; a later change could allocate them outside the lock. The stall grows with the segment and matters only at tens of GB of `maintenance_work_mem`.
- **The visited set is a SipHash set rebuilt for every search:** performance work for M7 or later. pgrust's HNSW builds took about 1.5× C's in the Build time table, measured on battery.
- **A vector index on a `pgrcolumnar` table launches workers** before the build scan refuses the table; the error is the serial build's.
- **Worker threads keep their PRNG state from one task to the next** (pre-existing `parallel`-crate behaviour); HNSW levels stay random.

**Deferred review minors that may stay open** (from the M6 ledger, as the final review triaged them; line numbers are at the final review's fix commit; the other minors this list held at `36bec2f137` are fixed):
- `crates/backend/optimizer/plan/planner/src/index_workers.rs` (Task 1): the unit tests cover only the early return and the clamp; the temp-table, parallel-unsafe and reloption branches are covered only by server runs (`parallel-build.sql`, against C). No RED was captured for `amcanbuildparallel`.
- `crates/contrib/pgvector_parallel/src/lib.rs:91-94` (Task 2): no back-out when the context gets no DSM segment (C `hnswbuild.c:987-995`; pgrust's analogue is `parallel::nworkers(pcxt) == 0` after `InitializeParallelDSM`), and no DIVERGENCE note. The outcome is the same through `end_parallel`; only the shared-state setup is wasted.
- `scripts/pgvector/run-tap.sh:16` (Task 2): a comment bullet of about 170 characters.
- `crates/contrib/pgvector_hnsw_graph/tests/loom.rs` (Task 4): no model reaches `update_connection`'s shrink path (Coverage limits).
- `crates/contrib/pgvector_hnsw_graph/src/memory.rs:104-116` (Task 4): `GenerationModel`'s block doubling and power-of-two rounding are unreachable at 1 MB / 1 MB / 1 MB and untested. (`~/.cache/pgrust/m6-evidence/task4/clippy-loom.log` lacks a command header.)
- `~/.cache/pgrust/m6-evidence/task5/bytecmp-ref-29.tsv` (Task 5) holds only the last `ref spill` row; that task's 29-case `ref` claim rests on `step6-harness-bytecmp.log`. Task 8's `summary-ref-bytecmp.tsv` has all 29 rows.
- `scripts/pgvector/run-buildtime.sh` (Task 7): no `CHECKPOINT` before a timed build, so a checkpoint on either server can overlap the next cell; and the EXIT trap stops the servers one after the other, so a `server_stop` timeout's `die` skips the second stop (`run-bytecmp.sh` does the same).
- This report (Task 8): the `AppleClamshellState = Yes` reading under How the run was produced comes from `task6/step5-sleep-stalls.txt`, captured before Task 8; and the per-build log copies in `m6-evidence/buildtime/` lost their modification times (`cp` without `-p`), so the sleep placement under Build time can be rebuilt only from `pgvector-work/buildtime/pgrust` until the next run deletes it.

**Carried from earlier milestones:** the M5 and M5 follow-ups reports' "Still open" lists stand; M6 closed none of them (handoff §3 lists the ones still tracked).

# pgvector M6: parallel HNSW and IVFFlat builds — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meet M6's exit criteria (spec §9): port `plan_create_index_workers`, the IVFFlat parallel build and the HNSW shared graph. Loom tests on the graph's lock protocol pass, the 8 parallel TAP tests pass with workers forced (`run-all.sh pgrust` reaches every row), and build time against C pgvector is reported.

**Architecture:**
- **Workers are launched backends** from the thread-native `parallel` crate: `CreateParallelContext` → `InitializeParallelDSM` → `LaunchParallelWorkers` → `WaitForParallelWorkersToFinish` → `DestroyParallelContext`, the same shape as C. Each worker is a full backend: it restores the leader's snapshot, GUCs and lock group, can `ereport` (its NOTICE and DEBUG1 lines reach the client, with `CONTEXT:  parallel worker`), and opens its own relations.
- **One shared crate, `pgvector_parallel`,** holds what `hnswbuild.c` and `ivfbuild.c` both do: the kill switch, the launch and teardown, the worker prologue and the per-participant DEBUG1 lines.
- **HNSW:** a new leaf crate, `pgvector_hnsw_graph`, holds the in-memory graph, C's lock protocol (`flushLock`, `allocatorLock`, `entryLock`, `entryWaitLock`, `graph->lock`, one lock per element) and the in-memory insert algorithm, all on `pgsync`. The serial build and every parallel participant insert through it. It depends only on `pgsync` and `pairingheap`, so `tests/loom.rs` explores the real protocol.
- **IVFFlat:** the leader samples, runs k-means and writes the list pages as now. Workers scan, assign each row to its nearest center and stream `(list, tid, value)` batches to the leader, which feeds its one tuplesort (spec §7.3).
- **Core:** `plan_create_index_workers` is ported into the `planner` crate behind a `planner_seams` slot; `index_build` gains C's `parallel` argument and prints C's "with request for N parallel workers" line; `IndexAmKind` gains `amcanbuildparallel`; `execindexing`'s build scan gains the parallel-scan arm (`heapam_handler.c:1272-1281`).

**Decisions this plan makes (for review):**
1. **Launched workers, not the morsel pool (amends spec §7.1).** The spec asked a spike to check that the pool can host heavy per-row work. The survey answers it from the code (`~/.cache/pgrust/m6-research/C-worker-mechanism.md`):
   - The pool's smallest claim is one heap block against a 2 ms target (`runtime/src/sizing.rs:23-24`); one block of HNSW inserts can take ten times that, at Utility class, without preemption.
   - The pool falls back to serial when no thread claims the work within 100 ms (`nbtsort/src/pool.rs:153-161`), so whether a build goes parallel would depend on load. 36 of the 37 failing TAP lines check `using \d+ parallel workers`, which C prints from the number of workers actually launched.
   - B-tree's pool arm, the only index-build precedent, is off by default, has no in-tree server test, and its entry point is a stub.
   - Launched workers give C's error, cancel and cleanup paths (shared with Gather, which every parallel query uses), and C's `CONTEXT:  parallel worker` on forwarded messages.
   - Neither mechanism spawns raw threads: bgworkers go through `launch_backend`'s sanctioned spawn door, so the `lint_determinism` spawn budget is untouched.
   Recorded as ruling M6-1 in the spec's errata (Task 8). A pool channel can be added later in vacuumlazy's shape without changing the per-row work.
2. **Entry points register under `"postgres"` with pgrust names** (`pgrust_hnsw_parallel_build_main`, `pgrust_ivfflat_parallel_build_main`). `LookupParallelWorkerFunction` accepts only `"postgres"` (`parallel/src/lib.rs:333-374`), as for parallel COPY and vacuum. The upstream-owned `parallel` crate is not edited. DIVERGENCE: C's library is `"vector"` and its functions are `HnswParallelBuildMain` / `IvfflatParallelBuildMain`; neither name is visible to SQL.
3. **The HNSW graph keeps Rust values and charges C's sizes to a model of C's allocator.** C's flush point depends on its allocator: a Generation context with 1 MB blocks in a serial build (`hnswbuild.c:649-658, 732-737`), MAXALIGNed bump allocations from `maintenance_work_mem` − 3 MB with a 1 MB margin in a parallel build (`hnswbuild.c:509-510, 663-681, 961-969`). `pgvector_hnsw_graph::memory` models both exactly, so flush points are C's:
   - **Verified while planning:** a simulation of C's seeded serial build (`pg_prng` seeded with 42, `generation.c`'s arithmetic, C's four per-element allocations) flushes the `spill` case's `vector(3)` graph after **1436** tuples, C's observed count (pgrust prints 1840 today). Task 5 pins it as a unit test.
   - TAP `045` needs the parallel model: at `3073kB` the area is 1024 bytes, so the first insert flushes ("after 0 tuples").
   - DIVERGENCE: the bytes actually held differ from C's, and the arena is freed with the build, not at the flush (C resets `graphCtx` in a serial build).
4. **The serial build moves onto the same graph crate,** as C has one code path for both. The in-memory algorithm moves verbatim (closer-flag write-backs, element-id tie order). The gate: every default bytecmp case and every iterscan case stays identical.
5. **`spill` gets tie-free data and is promoted if it passes.** Its grid data is tie-heavy, and after the flush its rows go through on-disk inserts, whose tie-break is not reproducible even C against C (`insert_grid`'s reason). Task 5 switches it to the Golomb ruler with p = 2003 (exact in `float4`) and promotes it to a default case only if `ref` and `pgrust` both pass it three times; otherwise it stays opt-in with the measured reason.
6. **IVFFlat streams rows to one leader sort** (spec §7.3). DIVERGENCE: C's participants sort their shares and the leader merges their tapes; pgrust's `Tuplesort` is thread-affine. The order within a list differs, which C's own sort (keyed on the list only, `ivfbuild.c:612-621`) does not fix either. The parallel context ends right after the scan, since no shared tapes need it.
7. **`plan_create_index_workers` lives in the `planner` crate** (`planner/src/index_workers.rs`) behind `planner_seams::plan_create_index_workers`, used by `index_build` and by both AMs, as in C (`hnswbuild.c:1076`, `ivfbuild.c:987`). It is C's function step by step, including the 32 MB-per-participant clamp that B-tree's pool helper omits.
8. **`amcanbuildparallel` is true only for HNSW and IVFFlat.** DIVERGENCE: C also says true for btree, GIN and BRIN, but pgrust builds those serially, so `index_build` keeps printing "serially" for them. No `ii_ParallelWorkers` field is added: no pgrust AM reads it (pgvector never does), so `index_build` keeps the count in a local.
9. **CREATE INDEX CONCURRENTLY builds in parallel, as in C**, through the parallel scan's serialized MVCC snapshot. No TAP test covers it; `sql/parallel-build.sql` does (Review Focus 2).
10. **`ii_BrokenHotChain` is dropped by parallel builds, as in C** (`hnswbuild.c:804`, `ivfbuild.c:677` build fresh `IndexInfo`s; nothing propagates the flag). Bug-compatible; the M6 report records it.
11. **The kill switch is `PGRUST_VECTOR_INDEXBUILD_POOL=0`** (spec §7.1's name). It costs one new `env` row in `lint-determinism.allow` (budget 1, `DST-REVIEW(m6-vector-parallel)`), the only ledger change in this plan.
12. **The harness pins `min_parallel_table_scan_size=8MB` for pgrust servers.** pgrust ships 1 MB (`guc_tables/src/tables.rs:988`; C's is 8 MB). Once pgrust honours `max_parallel_maintenance_workers`, TAP `002`, `005`, `006`, `041`, `042`, `043` and `044` and the on-disk follow-up's `REINDEX` would build in parallel on pgrust and serially on C. The pin joins the existing `max_parallel_workers_per_gather=2` pin (`common.sh:64`, `run-tap.sh:30`).
13. **A new diff-tier row, `parallel_build`,** runs `sql/parallel-build.sql` through `sqldiff.sh`. sqldiff drops the per-participant `leader processed N tuples` / `worker processed N tuples` lines and every `CONTEXT:  parallel worker` line, since their counts and order vary run to run in C too. The diff tier goes from 21 to 22 rows.
14. **Build time is a report, not a gate:** `scripts/pgvector/run-buildtime.sh` (Task 7) times HNSW and IVFFlat builds with 0, 1, 2 and 4 workers on pgrust and on C.
15. **Not in M6:** a morsel-pool channel; HNSW build phase names (handoff §5); the wasm build (it builds serially: `IsUnderPostmaster` is false there); the approximate differential (M7).

**Tech Stack:** Rust 1.96 (`rust-toolchain.toml`), `pgsync` (native and `cfg(loom)`, loom 0.7), the `parallel` crate, bash 3.2 harness scripts, the PostgreSQL 18.6 + pgvector 0.8.7 C reference (`pgvec`, `pgvec-seeded`).

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`. Binding sections:
- §7 (Parallel builds), as amended by Decision 1;
- §8.2's Unit row (loom tests), TAP row and Byte-identical row;
- §9's M6 row.

Inputs:
- `docs/HANDOFF_2026-10-20.md` §1, §2 (the M6 survey) and §4;
- the planning research in `~/.cache/pgrust/m6-research/`:
  - `A-c-parallel-builds.md`: C's behaviour, line by line;
  - `B-tap-tests.md`: the failing TAP lines and which harness builds could go parallel;
  - `C-worker-mechanism.md`: the mechanism comparison behind Decision 1;
  - `D-pgrust-build-crates.md`: the current build crates;
  - `E-core-utilities.md`: planner pieces, GUCs, pgsync and loom, the determinism lint.

**Facts verified while writing this plan** (2026-10-22, macOS arm64, `main` = `abb86f4cfa`):
- **The 37 failing TAP lines.** 36 are `like($stderr, qr/using \d+ parallel workers/)` in `003`, `012`, `020`, `024`, `028`, `032`, `035` and `045`; the 37th is `045:27`, `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`. The tests force workers themselves (`SET min_parallel_table_scan_size = 1` or `ALTER TABLE ... SET (parallel_workers = N)`), so they need no harness change.
- **C's counts in those tests** (from the `ref` prove logs): IVFFlat requests the planner's count, 1 at the default `maintenance_work_mem` (`using 1 parallel workers`); HNSW uses the planner only as a zero check and requests `max_parallel_maintenance_workers` or the capped reloption (`using 2`, and `using 1` in `045`).
- **pgrust's pieces exist:**
  - `parallel::register_parallel_worker_entrypoint` de-duplicates by name (`parallel/src/lib.rs:326-331`), so a build may register just before `CreateParallelContext`.
  - `WaitForParallelWorkersToFinish` returns at once when every worker has finished (`parallel/src/lib.rs:1097-1160`), so calling it twice is safe.
  - `tableam::table_parallelscan_initialize` / `ParallelTableScanDescShared` (`tableam/src/lib.rs:2195-2252`) and `heapam::heap_beginscan(.., parallel_scan: Option<NonNull<ParallelBlockTableScanDescData>>, ..)` (`heapam/src/lib.rs:1538-1546`) are used by parallel seqscan.
  - `execindexing::build_scan`'s core passes `None` for the parallel scan (`build_scan.rs:231-239`).
  - `InitPostmasterChild` sets `IsUnderPostmaster` and seeds the thread's PRNG (`miscinit/src/process.rs:124`), so launched workers draw independent HNSW levels, as C's do.
  - `table_relation_estimate_size` has C's 10-page minimum for never-vacuumed tables (`tableam/src/lib.rs:3225`).
  - `pgsync::mailbox` has `try_recv` and `send_interruptible` (`pgsync/src/mailbox.rs:164-262`) and is not matched by any determinism-lint pattern; `pgsync::Mutex`, `RwLock` and `OnceLock::{get,set}` are not either.
- **No dependency cycles:** `cargo tree --offline` shows that `parallel`, `xact`, `snapmgr`, `tableam`, `table`, `indexam`, `backend_status`, `lmgr_proc`, `latch`, `planner_seams` and `condition_variable` reach none of `catalog_index`, `nbtsort` or the two build crates; `planner` already reaches `execindexing`, and `execindexing` does not reach `planner`; `pairingheap` has no dependencies.
- **C's allocator constants (LP64, PG 18.6):** `sizeof(MemoryContextData)` = 80 (`memnodes.h:117-134`), so `MAXALIGN(sizeof(GenerationContext))` = 128 (`generation.c:59-72`); `Generation_BLOCKHDRSZ` = 56 (`generation.c:46, 87-96`); `Generation_CHUNKHDRSZ` = 8; `allocChunkLimit` = 65536 for 1 MB blocks (`generation.c:256-259`). The reference build has no `USE_ASSERT_CHECKING` (`pg_config.h:679`), so chunks carry no sentinel byte.
- **C's per-element allocations, in order** (`hnswutils.c:205-270`, `hnswbuild.c:556-557`): `sizeof(HnswElementData)` = 128, the neighbor-pointer array `8 × (level + 1)`, one `HNSW_NEIGHBOR_ARRAY_SIZE(lm)` = `8 + 16 × lm` per layer from 0 to `level`, then the value (`VARSIZE_ANY`).
- **C's line numbers** (a multi-line `ereport` reports the line where the call closes, as pgrust's `hnswbuild.c:703` for "type not supported" shows): `index.c:3066` "serially", `index.c:3072` "with request for %d parallel workers", `hnswbuild.c:1054` and `ivfbuild.c:961` "using %d parallel workers", `hnswbuild.c:828/830` and `ivfbuild.c:709/711` "leader/worker processed", `hnswbuild.c:545` the NOTICE, `hnswbuild.c:672/676` the shared allocator's errors.

## Global Constraints

- **Branch:** `vector/m6`, cut from `main` (`abb86f4cfa`); this plan is its first commit. Work in the main checkout. Merge locally with a fast-forward when done; push only when the user asks.
- **Commit trailer:** every commit ends with a `Co-Authored-By:` trailer naming the model that wrote it: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md): keep C names, cite C `file.c:line`, mark intentional differences `DIVERGENCE`, and give errors and messages C's file, line and function (`.with_location("hnswbuild.c", 1054, "HnswBeginParallel")` or `ErrorLocation::new("hnswbuild.c", 545, "InsertTuple")`).
- **Locks and threads:** every lock is `pgsync`'s (`Mutex`, `RwLock`, `OnceLock::get`/`set`, `mailbox`). No `std::sync::{Mutex, RwLock, Condvar, OnceLock}`, no `Condvar` of any kind, no thread spawn, no zero-argument `.join()` and no `get_or_init` outside `tests/`. `std::sync::Arc` and `std::sync::atomic` are fine.
- **`pgvector_hnsw_graph` depends only on `pgsync` and `pairingheap`** (plus `loom` under `cfg(loom)` for its tests). Loom compiles the whole dependency cone, and backend crates' statics do not build under loom (`pgrc2_write/tests/loom.rs:15-30`).
- **The determinism ledger** gains exactly one row: `env	crates/contrib/pgvector_parallel/src/lib.rs	1	DST-REVIEW(m6-vector-parallel): ...` (Task 2). No other new row and no raised budget; a task that would need one stops and asks.
- **Never widen `docs/fuzzing/rulings.toml`.** Never edit upstream expected outputs. No `-- pgrust:` annotations.
- **Serial builds stay byte-identical:** all 28 default bytecmp cases and all 19 iterscan cases stay identical through every task.
- **Core edits** are limited to: `planner` (a new `index_workers.rs`, one `init_seams` line, and `execindexing` in its `Cargo.toml`), `planner_seams` (one slot), `types_relscan` (`amcanbuildparallel`), `catalog_index` (`index_build`'s `parallel` argument, its four callers and its `Cargo.toml`), `execindexing` (`build_scan.rs`'s parallel arm and its re-export), and the workspace `Cargo.toml` / `Cargo.lock`. Any other core edit is a plan-defined stop.
- **Harness scripts:**
  - Use `#!/usr/bin/env bash` and `set -euo pipefail` (a sourced library has `# shellcheck shell=bash` instead).
  - Stay bash 3.2 compatible: no associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, or `case` inside `$( )`.
  - Use `${arr[@]+"${arr[@]}"}` for a possibly empty array.
  - Check each changed script with `/bin/bash -n` (macOS's 3.2).
- **Ports:** pgrust 55491, C reference 55492, Docker 55493, seeded C 55494. Leave no servers running. Docker is used only by `run-all.sh`'s on-disk tier, with the existing rules: the two digest-pinned images, the container `pgrust-pgvector-ondisk`, never `docker system prune`, never remove images.
- **Verdicts:** read each tier's from `~/.cache/pgrust/pgvector-work/<tier>/<mode>/summary.tsv` before a later run overwrites it. `run-tap.sh` wipes `tap/<mode>`, so single-test runs use `PGV_WORK=$HOME/.cache/pgrust/m6-work` (at most 80 bytes).
- **Server build:** `scripts/pgvector/build-pgrust.sh` after every Rust change before a server-level check; it builds `postgres`, `pagemask` and `diffrunner` with `--profile fast-profile --locked`. After adding crates, run `cargo build --profile fast-profile --bin postgres` once without `--locked` so `Cargo.lock` gains the new packages, and commit `Cargo.lock`.
- **Unit tests and lints:** each task's commands are in its steps; never run a test command with an empty filter. The lints are `cargo test -p seams_init --test lint_seam_installs`, `--test lint_determinism`, `--test lint_walker_arms`, and `cargo test -p catalog_index --test lint_inplace_locks`. If `psql`/`libpq` linking fails while building lint tests, put `$HOME/.cache/pgrust/pgref-18.6/pg/bin` first on `PATH`, set `PQ_LIB_DIR=$HOME/.cache/pgrust/pgref-18.6/pg/lib`, and `cargo clean -p pq-sys` once.
- **Evidence** goes to `~/.cache/pgrust/m6-evidence/` (logs, summaries, the build-time report); the ledger is `~/.cache/pgrust/m6-sdd-ledger.md`.

## Review Focus

The five input classes most likely to bite a pgvector user once builds go parallel that no TAP test exercises. Each one's test is in the owning task, as a case in `scripts/pgvector/sql/parallel-build.sql` compared with C.

1. **A row error inside a worker** (a `sparsevec` with more than 1,000 non-zeros under an HNSW index). The build must fail with C's ERROR text, leave no parallel worker behind, and leave no index. A hang or a crash here is the worst outcome.
   - Test: the `sv_bad` case (Task 6).
2. **CREATE INDEX CONCURRENTLY with workers forced.** It must print C's message lines and build an index that answers like C's.
   - Test: the `ivf` CIC case (Task 3) and the `h` CIC case (Task 6).
3. **`statement_timeout` firing mid-build.** It must cancel like C ("canceling statement due to statement timeout") and leave no worker running.
   - Test: the `big` timeout case (Task 6).
4. **Duplicate-heavy data built in parallel.** Concurrent duplicate merges must lose no heap TID: an index scan that reads every row returns every row.
   - Test: the loom model `concurrent_duplicates_merge_into_one_element` (Task 4) and the `d` case (Task 6).
5. **Builds that must stay serial, and one that must not.** A temp table, a parallel-unsafe index expression, `parallel_workers = 0` and `maintenance_work_mem` under 64 MB print "serially" like C; an unlogged table's main fork builds in parallel and its init fork does not.
   - Test: the gating cases (Task 3 for IVFFlat, Task 6 for HNSW).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/backend/optimizer/plan/planner/src/index_workers.rs` (new) | `plan_create_index_workers` (`planner.c:7097-7239`) | 1 |
| `crates/backend/optimizer/plan/planner/src/lib.rs` | `pub mod index_workers;`; the seam set in `init_seams` | 1 |
| `crates/backend/optimizer/plan/planner_seams/src/lib.rs` | the `plan_create_index_workers` slot | 1 |
| `crates/backend/access/index/relscan/src/lib.rs` | `IndexAmKind::amcanbuildparallel` and its test | 1 |
| `crates/backend/catalog/catalog_index/src/{lib.rs,reindex.rs,concurrent.rs}`, `Cargo.toml` | `index_build(.., parallel)`, index.c's two DEBUG1 lines, the four callers | 1 |
| `crates/backend/executor/execindexing/src/{build_scan.rs,lib.rs}` | `table_index_build_scan_parallel` | 1 |
| `crates/contrib/pgvector_parallel/` (new: `Cargo.toml`, `src/lib.rs`) | the kill switch, launch and teardown, the worker prologue, DEBUG1 lines, latch wake | 2 |
| `crates/_support/seams_init/tests/lint-determinism.allow` | the kill switch's `env` row | 2 |
| `scripts/pgvector/common.sh`, `run-tap.sh` | the `min_parallel_table_scan_size=8MB` pin | 2 |
| `scripts/pgvector/sqldiff.sh` | drop participant-count and `CONTEXT:  parallel worker` lines | 2 |
| `crates/contrib/pgvector_ivfflat_build/src/{lib.rs,parallel_build.rs}`, `Cargo.toml` | the parallel assign phase | 3 |
| `scripts/pgvector/sql/parallel-build.sql` (new), `run-diff.sh` | the `parallel_build` diff row; IVFFlat cases (3), HNSW cases (6) | 3, 6 |
| `crates/contrib/pgvector_hnsw_graph/` (new: `Cargo.toml`, `src/{lib.rs,arena.rs,memory.rs,algo.rs,tests.rs}`, `tests/loom.rs`) | the shared graph, C's lock protocol, the in-memory algorithm, C's memory arithmetic, loom models | 4 |
| `crates/contrib/pgvector_hnsw_build/src/lib.rs`, `Cargo.toml` | the serial build on the graph crate; progress; the 1436 test | 5 |
| `crates/_support/types/types_hnsw/src/lib.rs` | `PROGRESS_HNSW_PHASE_LOAD` | 5 |
| `scripts/pgvector/run-bytecmp.sh` | `spill`'s tie-free data (and its promotion, if it passes) | 5 |
| `crates/contrib/pgvector_hnsw_build/src/parallel_build.rs` (new) | the parallel HNSW build | 6 |
| `scripts/pgvector/run-buildtime.sh` (new) | the build-time report | 7 |
| `scripts/pgvector/tests/harness_test.sh` | self-tests: sqldiff's filter (2), the diff row count (3), the build-time report (7), spill (5, if promoted) | 2, 3, 5, 7 |
| `Cargo.toml` (workspace members), `Cargo.lock` | the three new crates | 2, 4 |
| spec errata, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m6-report.md` (new), `docs/HANDOFF_<date>.md` (new) | docs and the record | 8 |

## Execution notes

- **Task order:** 1 to 8.
  - Task 1 (core) and Task 2 (shared plumbing, harness) come first; nothing goes parallel yet.
  - Task 3 is the first end-to-end parallel build (IVFFlat). It proves the mechanism on a live server before the HNSW work depends on it: if a launched worker cannot open the build's relations or run the scan, stop there.
  - Tasks 4 and 5 move the serial HNSW build onto the new graph crate, with the bytecmp and iterscan tiers as the gate.
  - Task 6 makes HNSW parallel. Task 7 adds the build-time report. Task 8 runs everything and writes the record.
- **Suggested implementers:**
  - Sonnet: Tasks 1, 2 and 7 (the code is spelled out, the risk is mechanical).
  - Opus: Tasks 3, 4, 5, 6 and 8 (concurrency, byte identity, first end-to-end runs, the report).
  - Opus does every review and the final whole-branch review. Task 4's review reads the lock protocol against `hnswbuild.c:437-580` line by line.
- **Plan-defined stops**, in addition to subagent-driven-development's four:
  - a launched worker blocks on a relation lock its leader holds (lock-group semantics are core; do not switch workers to `NoLock`);
  - any default bytecmp case or iterscan case differs after Task 5 or 6;
  - a TAP recall assertion on a parallel build fails in two of three runs while C passes (a real bug, not noise; stop if the cause is outside this plan's files);
  - a loom model fails or deadlocks (never weaken a model to make it pass);
  - any core edit outside the list in Global Constraints, a new determinism-ledger row beyond Task 2's, or a raised budget.
- **Expected totals after Task 8:**
  - `run-all.sh ref`: 138/138 (regress 14, TAP 48, bytecmp 28, iterscan 19, diff 22, ondisk 7); 139/139 if Task 5 promotes `spill` (bytecmp 29).
  - `run-all.sh pgrust`: the same counts, all passing.
- **Timings to expect** (2026-10-20 evidence): `run-all.sh ref` about 330 s, `run-all.sh pgrust` about 465 s, the full `harness_test.sh` about 6 minutes, the 8 parallel TAP files about 95 s on pgrust.

---

### Task 1: The worker count, index.c's messages and the parallel build scan

**Files:**
- Create: `crates/backend/optimizer/plan/planner/src/index_workers.rs`
- Modify: `crates/backend/optimizer/plan/planner/src/lib.rs` (`pub mod index_workers;`, `init_seams`), `crates/backend/optimizer/plan/planner/Cargo.toml` (+`execindexing`)
- Modify: `crates/backend/optimizer/plan/planner_seams/src/lib.rs` (one slot)
- Modify: `crates/backend/access/index/relscan/src/lib.rs` (`amcanbuildparallel` and a test in `am_property_tests`)
- Modify: `crates/backend/catalog/catalog_index/src/lib.rs` (`index_build`, its header comment, `index_create`'s call at :1057, `index_build_dummy` at :1595), `src/reindex.rs:350`, `src/concurrent.rs:336`, `Cargo.toml` (+`planner_seams`)
- Modify: `crates/backend/executor/execindexing/src/build_scan.rs`, `src/lib.rs` (re-export)

**Interfaces:**
- Produces:
  - `planner_seams::plan_create_index_workers::call(table_oid: Oid, index_oid: Oid) -> PgResult<i32>` (installed by `planner::init_seams`).
  - `types_relscan::IndexAmKind::amcanbuildparallel(self) -> bool` (`const fn`).
  - `catalog_index::index_build(mcx, heapRelation, indexRelation, indexInfo, isreindex: bool, parallel: bool) -> PgResult<()>`.
  - `execindexing::table_index_build_scan_parallel<'mcx, F>(mcx: Mcx<'mcx>, heap_relation: &Relation<'mcx>, index_relation: &Relation<'mcx>, index_info: &mut IndexInfo<'mcx>, progress: bool, parallel_scan: &tableam::ParallelTableScanDescShared, callback: F) -> PgResult<f64>`, with the same `F` bound as `table_index_build_scan`.

- [ ] **Step 1: Write the failing tests**

In `crates/backend/access/index/relscan/src/lib.rs`, inside `mod am_property_tests` (after `ivf_am_properties_match_ivfflathandler`):

```rust
    // amcanbuildparallel (amapi.h:274): only the vector AMs build in parallel
    // in pgrust (hnsw.c:351, ivfflat.c:268).
    #[test]
    fn amcanbuildparallel_is_the_vector_ams_only() {
        assert!(IndexAmKind::Hnsw.amcanbuildparallel());
        assert!(IndexAmKind::Ivfflat.amcanbuildparallel());
        for k in [
            IndexAmKind::Btree,
            IndexAmKind::Hash,
            IndexAmKind::Gin,
            IndexAmKind::Gist,
            IndexAmKind::Spgist,
            IndexAmKind::Brin,
            IndexAmKind::Bloom,
        ] {
            assert!(!k.amcanbuildparallel(), "{k:?}");
        }
    }
```

Create `crates/backend/optimizer/plan/planner/src/index_workers.rs` with only its tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // planner.c:7128-7133: a standalone backend (unit tests run outside a
    // postmaster) gets no workers, before any catalog or GUC access.
    #[test]
    fn index_workers_are_zero_outside_a_postmaster() {
        assert!(!init_small::globals::IsUnderPostmaster());
        assert_eq!(plan_create_index_workers(1, 2).unwrap(), 0);
    }

    // planner.c:7220-7232: each participant (the leader counts) needs 32 MB of
    // maintenance_work_mem (kB here).
    #[test]
    fn memory_clamp_matches_planner_c() {
        assert_eq!(clamp_for_maintenance_work_mem(2, 65536), 1);
        assert_eq!(clamp_for_maintenance_work_mem(2, 98304), 2);
        assert_eq!(clamp_for_maintenance_work_mem(1, 65535), 0);
        assert_eq!(clamp_for_maintenance_work_mem(1, 65536), 1);
        assert_eq!(clamp_for_maintenance_work_mem(4, 1048576), 4);
        assert_eq!(clamp_for_maintenance_work_mem(0, 64), 0);
    }
}
```

Add `pub mod index_workers;` to `crates/backend/optimizer/plan/planner/src/lib.rs` after `pub mod indxpath;` (line 18).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p types_relscan --lib amcanbuildparallel_is_the_vector_ams_only`
Expected: FAIL to compile, "no method named `amcanbuildparallel`".

Run: `cargo test -p planner --lib index_workers`
Expected: FAIL to compile, "cannot find function `plan_create_index_workers`".

- [ ] **Step 3: Implement `amcanbuildparallel` and `plan_create_index_workers`**

In `crates/backend/access/index/relscan/src/lib.rs`, add after `amsearcharray` (keep the explicit-arm style of its neighbours):

```rust
    /// amcanbuildparallel (amapi.h:274). C sets it for btree (nbtree.c:137),
    /// GIN (ginutil.c:60), BRIN (brin.c:272) and both pgvector AMs
    /// (hnsw.c:351, ivfflat.c:268). DIVERGENCE: pgrust builds btree, GIN and
    /// BRIN serially, so only the vector AMs answer true, and index_build's
    /// DEBUG1 keeps saying "serially" for the others.
    pub const fn amcanbuildparallel(self) -> bool {
        match self {
            IndexAmKind::Btree => false,
            IndexAmKind::Hash => false,
            IndexAmKind::Gin => false,
            IndexAmKind::Gist => false,
            IndexAmKind::Spgist => false,
            IndexAmKind::Brin => false,
            IndexAmKind::Hnsw => true,
            IndexAmKind::Ivfflat => true,
            IndexAmKind::Bloom => false,
            #[cfg(feature = "mock")]
            IndexAmKind::Mock => false,
        }
    }
```

Put the body of `index_workers.rs` above its test module:

```rust
//! plan_create_index_workers (planner.c:7097-7239): how many workers a CREATE
//! INDEX may use. index_build asks it for an AM that can build in parallel
//! (index.c:3056-3060); pgvector's builds ask it themselves
//! (hnswbuild.c:1076, ivfbuild.c:987).
//!
//! C builds a dummy Query and PlannerInfo and calls build_simple_rel only to
//! get a base RelOptInfo carrying the table's parallel_workers reloption
//! (planner.c:7135-7172, plancat.c:208). Here that RelOptInfo is built
//! directly; nothing else of the planner runs.

use mcx::Mcx;
use types_core::{Oid, RELPERSISTENCE_TEMP};
use types_error::PgResult;
use types_nodes::NodeList;
use types_pathnodes::RelOptInfo;
use types_rel::lock::NoLock;
use types_rel::Relation;

/// The number of parallel worker processes (the leader not counted) to
/// request for a CREATE INDEX of `index_oid` on `table_oid`. The caller holds
/// locks on both (planner.c:7110-7111).
pub fn plan_create_index_workers(table_oid: Oid, index_oid: Oid) -> PgResult<i32> {
    // We don't allow performing parallel operation in standalone backend or
    // when parallelism is disabled (planner.c:7128-7133).
    if !init_small::globals::IsUnderPostmaster() {
        return Ok(0);
    }
    let max_parallel_maintenance_workers =
        guc_tables::vars::max_parallel_maintenance_workers.read();
    if max_parallel_maintenance_workers == 0 {
        return Ok(0);
    }

    let cx = mcx::MemoryContext::new("plan_create_index_workers");
    let mcx = cx.mcx();
    // Rels are assumed already locked by the caller (planner.c:7174-7176).
    let heap = table::table_open(mcx, table_oid, NoLock)?;
    let index = indexam::index_open(mcx, index_oid, NoLock)?;
    let parallel_workers = index_workers(mcx, &heap, &index, max_parallel_maintenance_workers);
    indexam::index_close(index, NoLock)?;
    table::table_close(heap, NoLock)?;
    parallel_workers
}

fn index_workers(
    mcx: Mcx<'_>,
    heap: &Relation<'_>,
    index: &Relation<'_>,
    max_parallel_maintenance_workers: i32,
) -> PgResult<i32> {
    // Determine if it's safe to proceed: parallel workers can't access the
    // leader's temporary tables, and the index expressions and predicate must
    // be parallel safe (planner.c:7178-7191).
    if heap.rd_rel.relpersistence == RELPERSISTENCE_TEMP
        || !is_parallel_safe_list(execindexing::RelationGetIndexExpressions(mcx, index)?)?
        || !is_parallel_safe_list(execindexing::RelationGetIndexPredicate(mcx, index)?)?
    {
        return Ok(0);
    }

    // build_simple_rel's base rel, with the reloption get_relation_info loads
    // (planner.c:7168-7172, plancat.c:208).
    let mut rel = RelOptInfo::new(mcx);
    rel.rel_parallel_workers = heap.get_parallel_workers(-1);

    // If parallel_workers storage parameter is set for the table, accept that
    // as the number of parallel worker processes to launch (though still cap
    // at max_parallel_maintenance_workers). We deliberately do not consider
    // any other factor when parallel_workers is set (planner.c:7193-7205).
    if rel.rel_parallel_workers != -1 {
        return Ok(rel.rel_parallel_workers.min(max_parallel_maintenance_workers));
    }

    // Estimate heap relation size ourselves, since rel->pages cannot be
    // trusted (planner.c:7207-7211).
    let (heap_blocks, _reltuples, _allvisfrac) = crate::plancat::estimate_rel_size(heap, None, 0)?;

    // Determine number of workers to scan the heap relation using generic
    // model (planner.c:7213-7218).
    let parallel_workers = ::allpaths::compute_parallel_worker(
        &rel,
        heap_blocks as f64,
        -1.0,
        max_parallel_maintenance_workers,
    );
    Ok(clamp_for_maintenance_work_mem(
        parallel_workers,
        init_small::globals::maintenance_work_mem(),
    ))
}

/// Cap workers based on available maintenance_work_mem as needed: each
/// tuplesort participant, the leader included, gets an even share, and 32 MB
/// apiece is the floor (planner.c:7220-7232). `maintenance_work_mem` is in kB.
fn clamp_for_maintenance_work_mem(mut parallel_workers: i32, maintenance_work_mem: i32) -> i32 {
    while parallel_workers > 0 && maintenance_work_mem / (parallel_workers + 1) < 32 * 1024 {
        parallel_workers -= 1;
    }
    parallel_workers
}

/// is_parallel_safe(root, (Node *) list) (clauses.c:767) under C's zeroed
/// PlannerGlobal: maxParallelHazard is 0, not PROPARALLEL_SAFE, so the walker
/// always runs, and there are no initplan params.
fn is_parallel_safe_list(list: NodeList<'_>) -> PgResult<bool> {
    for node in list.iter() {
        if !clauses::is_parallel_safe(0, true, Vec::new(), node)? {
            return Ok(false);
        }
    }
    Ok(true)
}
```

Add `execindexing = { path = "../../../executor/execindexing" }` to `[dependencies]` in `crates/backend/optimizer/plan/planner/Cargo.toml` (it is already in `planner`'s cone, and `execindexing` does not reach `planner`, so no cycle).

In `crates/backend/optimizer/plan/planner_seams/src/lib.rs`, after the `planner` slot:

```rust
seam_core::seam!(
    // plan_create_index_workers (planner.c:7114): index_build's worker count
    // (index.c:3056-3060) and pgvector's builds' (hnswbuild.c:1076,
    // ivfbuild.c:987).
    pub fn plan_create_index_workers(table_oid: Oid, index_oid: Oid) -> PgResult<i32>
);
```

In `planner::init_seams` (`crates/backend/optimizer/plan/planner/src/lib.rs:148`), after the `planner_seams::planner::set(..)` call:

```rust
    planner_seams::plan_create_index_workers::set(index_workers::plan_create_index_workers);
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p types_relscan --lib amcanbuildparallel_is_the_vector_ams_only`
Expected: PASS.

Run: `cargo test -p planner --lib index_workers`
Expected: PASS, 2 tests.

- [ ] **Step 5: `index_build` takes C's `parallel` argument and prints C's lines**

In `crates/backend/catalog/catalog_index/src/lib.rs`, replace the header comment above `index_build` (the seven lines starting `// index_build: btree only, serial only`) and the function's opening through the end of the existing DEBUG1 `ereport` call with:

```rust
// index_build (index.c:3030-3176). DIVERGENCE: C stores the worker count in
// indexInfo->ii_ParallelWorkers for btree, GIN and BRIN's parallel builds;
// pgrust builds those serially (IndexAmKind::amcanbuildparallel), and
// pgvector's AMs ask plan_create_index_workers themselves (hnswbuild.c:1076,
// ivfbuild.c:987), so the count only picks index.c's DEBUG1 line and
// IndexInfo has no ii_ParallelWorkers field.
pub fn index_build<'mcx>(
    mcx: Mcx<'mcx>,
    heapRelation: &Relation<'mcx>,
    indexRelation: &Relation<'mcx>,
    indexInfo: &mut IndexInfo<'mcx>,
    isreindex: bool,
    parallel: bool,
) -> PgResult<()> {
    let am_kind = types_relscan::IndexAmKind::from_relam(indexRelation.rd_rel.relam);

    // Determine worker process details for parallel CREATE INDEX. Note that
    // planner considers parallel safety for us (index.c:3050-3060).
    let mut parallel_workers = 0;
    if parallel && miscinit::IsNormalProcessingMode() && am_kind.amcanbuildparallel() {
        parallel_workers = planner_seams::plan_create_index_workers::call(
            heapRelation.rd_id,
            indexRelation.rd_id,
        )?;
    }

    // index.c:3062-3072; errmsg_internal: not translated.
    let (message, line) = if parallel_workers == 0 {
        (
            format!(
                "building index \"{}\" on table \"{}\" serially",
                indexRelation.name(),
                heapRelation.name()
            ),
            3066,
        )
    } else {
        (
            format!(
                "building index \"{}\" on table \"{}\" with request for {} parallel workers",
                indexRelation.name(),
                heapRelation.name(),
                parallel_workers
            ),
            3072,
        )
    };
    elog_seams::ereport::call(
        PgError::new(types_error::DEBUG1, message).with_location("index.c", line, "index_build"),
    )?;

    let guard = miscinit::SecContextGuard::security_restricted(heapRelation.rd_rel.relowner);
    let save_nestlevel = guc::NewGUCNestLevel();
    guc::RestrictSearchPath()?;
```

This keeps C's order: the worker count and the message come before the switch to the table owner (index.c:3056-3083). Everything after `guc::RestrictSearchPath()?;` (the progress report onward) is unchanged.

Then update the four callers:
- `lib.rs:1057` (`index_create`, C index.c:1286 passes `parallel = true`): `index_build(mcx, heapRelation, &indexRelation, indexInfo, false, true)?;`
- `lib.rs:1595` (`index_build_dummy`, which is C's RelationTruncateIndexes, heap.c:3608, `parallel = false`): `index_build(mcx, heap_relation, index_relation, &mut indexInfo, isreindex, false)`
- `reindex.rs:350` (`reindex_index`, index.c:3843): `crate::index_build(mcx, &heapRelation, &iRel, &mut indexInfo, true, true)?;`
- `concurrent.rs:336` (`index_concurrently_build`, index.c:1542): `crate::index_build(mcx, &heapRel, &indexRelation, &mut indexInfo, false, true)?;`

Add `planner_seams = { path = "../../optimizer/plan/planner_seams" }` to `crates/backend/catalog/catalog_index/Cargo.toml`. Confirm there are no other callers: `grep -rn 'index_build(' crates --include=*.rs | grep -v -e reference -e 'fn index_build' -e index_build_dummy -e '//'` must list only the four lines above.

- [ ] **Step 6: The build scan's parallel arm**

In `crates/backend/executor/execindexing/src/build_scan.rs`:

1. Replace line 4 (`// Loud: concurrent builds, parallel scans.`) with `// Lanes: SnapshotAny or a concurrent build's MVCC snapshot, block ranges, and a parallel heap scan (heapam_handler.c:1272-1281).` and add `use std::ptr::NonNull;` to the imports.
2. Replace `heapam_scan_get_blocks_done` with:

```rust
// heapam_scan_get_blocks_done (heapam_handler.c:1987-2022): blocks completed
// so far, wrapping when the scan did not start at block 0. In a parallel
// scan, workers can be concurrently reading blocks further ahead than what
// is reported.
fn heapam_scan_get_blocks_done(
    scan: &heapam::HeapScanDescData<'_>,
    parallel: Option<&tableam_vocab::ParallelBlockTableScanDescData>,
) -> BlockNumber {
    let (startblock, nblocks) = match parallel {
        Some(p) => (p.phs_startblock.load(core::sync::atomic::Ordering::Relaxed), p.phs_nblocks),
        None => (scan.rs_startblock, scan.rs_nblocks),
    };
    if scan.rs_cblock > startblock {
        scan.rs_cblock - startblock
    } else {
        nblocks - startblock + scan.rs_cblock
    }
}
```

3. Add a `parallel_scan: Option<&tableam::ParallelTableScanDescShared>` parameter to `build_range_scan_core`, after `snapshot`. `table_index_build_range_scan_with_xmin` and `table_index_build_scan_with_snapshot` pass `None` for it.
4. Add after `table_index_build_scan_with_snapshot`:

```rust
/// table_index_build_scan with a parallel heap scan (C's `scan` argument,
/// heapam_handler.c:1172-1300): every participant of a parallel build calls
/// this with the same shared descriptor, which hands out the blocks. A
/// parallel scan always allows syncscan (heapam_handler.c:1280).
pub fn table_index_build_scan_parallel<'mcx, F>(
    mcx: Mcx<'mcx>,
    heap_relation: &Relation<'mcx>,
    index_relation: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
    progress: bool,
    parallel_scan: &tableam::ParallelTableScanDescShared,
    callback: F,
) -> PgResult<f64>
where
    F: FnMut(&Relation<'mcx>, &ItemPointerData, &[Datum], &[bool], bool) -> PgResult<()>,
{
    build_range_scan_core(
        mcx,
        heap_relation,
        index_relation,
        index_info,
        true,
        false,
        progress,
        0,
        InvalidBlockNumber,
        None,
        None,
        Some(parallel_scan),
        callback,
    )
}
```

5. In `build_range_scan_core`, replace the `let (registered, owned) = if let Some(snap) = snapshot {` block's head so the parallel scan is tried first:

```rust
    // Concurrent builds scan under a registered MVCC snapshot; OldestXmin is
    // only for the SnapshotAny lane's HTSV routing. A parallel scan carries
    // its snapshot (heapam_handler.c:1272-1281): SnapshotAny, or the leader's
    // MVCC snapshot, restored and registered for this scan the way
    // table_beginscan_parallel does (its SO_TEMP_SNAPSHOT), and dropped at
    // the end.
    let (registered, owned) = if let Some(p) = parallel_scan {
        debug_assert!(allow_sync);
        // heapam_handler.c:1293: SnapshotAny exactly when OldestXmin is used.
        debug_assert!(p.pscan.phs_snapshot_any != concurrent);
        match p.snapshot.as_ref() {
            Some(serialized) if !p.pscan.phs_snapshot_any => {
                let snap = snapmgr::RestoreSnapshot(serialized);
                (Some(snapmgr::RegisterSnapshot(Some(&snap))?.expect("registered snapshot")), true)
            }
            _ => (None, false),
        }
    } else if let Some(snap) = snapshot {
        (Some(snap.clone()), false)
    } else if concurrent {
```

   (the rest of the block is unchanged).
6. In the `heapam::heap_beginscan(..)` call, replace the `None` parallel argument with `parallel_scan.map(|p| NonNull::from(&p.pscan)),`.
7. Both progress reports of the scan's size use the parallel descriptor's block count when there is one (heapam_handler.c:1298-1313, 1712-1726): in the `PROGRESS_SCAN_BLOCKS_TOTAL` report and in the final `PROGRESS_SCAN_BLOCKS_DONE` report, replace `scan.rs_nblocks` with `parallel_scan.map_or(scan.rs_nblocks, |p| p.pscan.phs_nblocks)`.
8. The per-tuple progress call becomes `heapam_scan_get_blocks_done(&scan, parallel_scan.map(|p| &p.pscan))`.

In `crates/backend/executor/execindexing/src/lib.rs`, add `table_index_build_scan_parallel` to the `pub use build_scan::{..}` list.

- [ ] **Step 7: Compile, the existing tests, and the seam lint**

Run: `cargo check -p catalog_index -p execindexing -p planner -p types_relscan`
Expected: no errors.

Run: `cargo test -p execindexing --lib` and `cargo test -p catalog_index --lib`
Expected: PASS (the counts the crates had before; record them in the report).

Run: `cargo test -p seams_init --test lint_seam_installs`
Expected: PASS (the new slot is set by `planner::init_seams`).

- [ ] **Step 8: A server shows C's request line, still building serially**

Run: `scripts/pgvector/build-pgrust.sh && scripts/pgvector/server.sh fresh pgrust`, then:

```bash
scripts/pgvector/server.sh psql pgrust -f - <<'SQL' 2>&1 | grep -E 'DEBUG|ERROR'
CREATE EXTENSION vector;
CREATE TABLE t (id int, v vector(3));
INSERT INTO t SELECT i, ARRAY[i % 7, i % 11, i % 13] FROM generate_series(1, 20000) i;
SET min_parallel_table_scan_size = 1;
SET client_min_messages = debug1;
CREATE INDEX t_ivf ON t USING ivfflat (v vector_l2_ops) WITH (lists = 10);
CREATE INDEX t_hnsw ON t USING hnsw (v vector_l2_ops);
CREATE INDEX t_id ON t (id);
SQL
scripts/pgvector/server.sh stop pgrust
```

Expected, and nothing else matching:
```
DEBUG:  building index "t_ivf" on table "t" with request for 1 parallel workers
DEBUG:  building index "t_hnsw" on table "t" with request for 1 parallel workers
DEBUG:  building index "t_id" on table "t" serially
```
(1 worker: `maintenance_work_mem` is 64 MB, so the planner's clamp allows one. No "using N parallel workers" yet: the AMs still build serially.)

- [ ] **Step 9: Commit**

```bash
git add crates/backend/optimizer/plan/planner crates/backend/optimizer/plan/planner_seams \
  crates/backend/access/index/relscan crates/backend/catalog/catalog_index crates/backend/executor/execindexing Cargo.lock
git commit -m "feat(core): plan_create_index_workers, index.c's parallel request line, the build scan's parallel arm

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 2: `pgvector_parallel`, the kill switch, and the harness pin and filter

**Files:**
- Create: `crates/contrib/pgvector_parallel/Cargo.toml`, `crates/contrib/pgvector_parallel/src/lib.rs`
- Modify: `Cargo.toml` (workspace member after `"crates/contrib/pgvector_ivfflat_build",`), `Cargo.lock`
- Modify: `crates/_support/seams_init/tests/lint-determinism.allow` (one `env` row)
- Modify: `scripts/pgvector/common.sh:59-64`, `scripts/pgvector/run-tap.sh:15-30`, `scripts/pgvector/sqldiff.sh`
- Test: `scripts/pgvector/tests/harness_test.sh` (`test_sqldiff_parallel_counts`)

**Interfaces:**
- Consumes: `planner_seams::plan_create_index_workers` (Task 1).
- Produces (`pgvector_parallel`):
  - `pub fn vector_parallel_builds_enabled() -> bool`
  - `pub fn plan_create_index_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32>`
  - `pub fn max_parallel_maintenance_workers() -> i32`
  - `pub struct Leader { pub pcxt: parallel::ParallelContextId, pub nworkers_launched: i32, /* private: snapshot */ }`
  - `pub fn begin_parallel<T: Any + Send + Sync>(entry_name: &'static str, entry: parallel::ParallelWorkerEntry, request: i32, heap: &Relation<'_>, isconcurrent: bool, make_shared: impl FnOnce(tableam::ParallelTableScanDescShared) -> PgResult<Arc<T>>, using_at: (&'static str, i32, &'static str)) -> PgResult<Option<(Leader, Arc<T>)>>`
  - `pub fn end_parallel(leader: Leader) -> PgResult<()>`
  - `pub fn leader_query_text() -> Option<String>`
  - `pub fn with_worker_activity<R>(query_text: Option<&str>, f: impl FnOnce() -> PgResult<R>) -> PgResult<R>`
  - `pub struct WorkerRelations<'mcx> { pub heap: Relation<'mcx>, pub index: Relation<'mcx>, /* private: lock modes */ }`
  - `pub fn worker_open<'mcx>(mcx: Mcx<'mcx>, heaprelid: Oid, indexrelid: Oid, isconcurrent: bool) -> PgResult<WorkerRelations<'mcx>>`
  - `pub fn worker_close(rels: WorkerRelations<'_>) -> PgResult<()>`
  - `pub fn report_processed(leader: bool, reltuples: f64, file: &'static str, leader_line: i32, worker_line: i32, func: &'static str) -> PgResult<()>`
  - `pub fn wake_proc(procno: usize)` and `pub fn my_proc_number() -> usize`
- Produces (harness): sqldiff drops `^psql:...: DEBUG:  (leader|worker) processed N tuples$` and `^CONTEXT:  parallel worker$` lines before comparing.

- [ ] **Step 1: Write the failing test**

Create `crates/contrib/pgvector_parallel/Cargo.toml`:

```toml
[package]
name = "pgvector_parallel"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

# pgvector's parallel-build plumbing (hnswbuild.c and ivfbuild.c's
# *BeginParallel, *EndParallel and *ParallelBuildMain prologue), shared by
# pgvector_hnsw_build and pgvector_ivfflat_build (M6).

[lib]
name = "pgvector_parallel"
path = "src/lib.rs"

[dependencies]
mcx = { path = "../../_support/mcx" }
types_core = { path = "../../_support/types/types_core" }
types_error = { path = "../../_support/types/types_error" }
types_rel = { path = "../../_support/types/rel" }
parallel = { path = "../../backend/access/transam/parallel" }
xact = { path = "../../backend/access/transam/xact" }
snapmgr = { path = "../../backend/utils/time/snapmgr" }
tableam = { path = "../../backend/access/table/tableam" }
table = { path = "../../backend/access/table/table" }
indexam = { path = "../../backend/access/index/indexam" }
elog = { path = "../../backend/utils/error/elog" }
backend_status = { path = "../../backend/utils/activity/backend_status" }
planner_seams = { path = "../../backend/optimizer/plan/planner_seams" }
guc_tables = { path = "../../backend/utils/misc/guc_tables" }
latch = { path = "../../backend/storage/ipc/latch" }
lmgr_proc = { path = "../../backend/storage/lmgr/lmgr_proc" }
init_small = { path = "../../backend/utils/init/init_small" }
```

Create `crates/contrib/pgvector_parallel/src/lib.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // PGRUST_VECTOR_INDEXBUILD_POOL=0 (whitespace allowed) is the only value
    // that turns parallel vector builds off.
    #[test]
    fn kill_switch_reads_only_zero_as_off() {
        assert!(kill_switch_allows(None));
        assert!(kill_switch_allows(Some("1")));
        assert!(kill_switch_allows(Some("")));
        assert!(kill_switch_allows(Some("off")));
        assert!(!kill_switch_allows(Some("0")));
        assert!(!kill_switch_allows(Some(" 0 ")));
    }
}
```

Add `"crates/contrib/pgvector_parallel",` to the workspace `members` in `Cargo.toml`, after `"crates/contrib/pgvector_ivfflat_build",`.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pgvector_parallel --lib kill_switch_reads_only_zero_as_off`
Expected: FAIL to compile, "cannot find function `kill_switch_allows`".

- [ ] **Step 3: Implement `pgvector_parallel`**

Put this above the test module in `src/lib.rs`:

```rust
//! pgvector's parallel index builds' shared plumbing: what hnswbuild.c and
//! ivfbuild.c's *BeginParallel, *EndParallel and *ParallelBuildMain have in
//! common, over pgrust's thread-native parallel crate.
//!
//! DIVERGENCE (ruling M6-1): C launches the workers with
//! CreateParallelContext("vector", "HnswParallelBuildMain" or
//! "IvfflatParallelBuildMain", n) and hands them a DSM segment through
//! shm_toc keys (hnswbuild.c:947-1037, ivfbuild.c:856-943). pgrust has no
//! dynamic loader: LookupParallelWorkerFunction accepts only "postgres"
//! (parallel/src/lib.rs:333-374), so the entry points register there under
//! pgrust names, and the shared state is an Arc set as the context's private
//! payload. Neither name is visible to SQL.
//! DIVERGENCE (spec §7.1): PGRUST_VECTOR_INDEXBUILD_POOL=0 turns both AMs'
//! parallel builds off; C has no such switch.

use std::any::Any;
use std::sync::Arc;

use mcx::Mcx;
use parallel::{ParallelContextId, ParallelWorkerEntry};
use tableam::ParallelTableScanDescShared;
use types_core::Oid;
use types_error::{ErrorLocation, PgResult, DEBUG1};
use types_rel::lock::{AccessExclusiveLock, RowExclusiveLock, ShareLock, ShareUpdateExclusiveLock, LOCKMODE};
use types_rel::Relation;

/// The kill switch (spec §7.1, named after B-tree's
/// PGRUST_RUNTIME_INDEXBUILD_POOL): PGRUST_VECTOR_INDEXBUILD_POOL=0 makes
/// every HNSW and IVFFlat build serial. Read at each build.
pub fn vector_parallel_builds_enabled() -> bool {
    kill_switch_allows(std::env::var("PGRUST_VECTOR_INDEXBUILD_POOL").ok().as_deref())
}

fn kill_switch_allows(value: Option<&str>) -> bool {
    value.map_or(true, |v| v.trim() != "0")
}

/// plan_create_index_workers (planner.c:7114), through the planner's seam.
pub fn plan_create_index_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32> {
    planner_seams::plan_create_index_workers::call(heap.rd_id, index.rd_id)
}

/// The max_parallel_maintenance_workers GUC (HNSW's request, hnswbuild.c:1083-1085).
pub fn max_parallel_maintenance_workers() -> i32 {
    guc_tables::vars::max_parallel_maintenance_workers.read()
}

/// HnswLeader / IvfflatLeader (hnsw.h:276-283, ivfflat.h:178-186): what the
/// leader keeps to end the parallel phase.
pub struct Leader {
    pub pcxt: ParallelContextId,
    pub nworkers_launched: i32,
    snapshot: Option<snapmgr::Snapshot>,
}

/// The steps HnswBeginParallel and IvfflatBeginParallel share
/// (hnswbuild.c:929-1054, ivfbuild.c:835-961): enter parallel mode, create the
/// context, take the scan's snapshot (an MVCC snapshot for CREATE INDEX
/// CONCURRENTLY, else SnapshotAny), initialize the parallel heap scan, build
/// the AM's shared state around it (`make_shared`), launch the workers, and
/// print C's "using %d parallel workers" at `using_at`. None when no worker
/// launched: the parallel phase is already ended, and the caller builds
/// serially, as C does (hnswbuild.c:1046-1051, ivfbuild.c:953-958).
pub fn begin_parallel<T: Any + Send + Sync>(
    entry_name: &'static str,
    entry: ParallelWorkerEntry,
    request: i32,
    heap: &Relation<'_>,
    isconcurrent: bool,
    make_shared: impl FnOnce(ParallelTableScanDescShared) -> PgResult<Arc<T>>,
    using_at: (&'static str, i32, &'static str),
) -> PgResult<Option<(Leader, Arc<T>)>> {
    debug_assert!(request > 0);
    // A worker looks its entry point up by name; registering twice is a no-op.
    parallel::register_parallel_worker_entrypoint(entry_name, entry);

    xact::EnterParallelMode();
    let pcxt = parallel::CreateParallelContext("postgres", entry_name, request)?;

    // Prepare for scan of the base relation. In a normal index build, we use
    // SnapshotAny because we must retrieve all tuples and do our own time qual
    // checks. In a concurrent build, we take a regular MVCC snapshot and index
    // whatever's live according to that.
    let snapshot = if isconcurrent {
        snapmgr::RegisterSnapshot(Some(&snapmgr::GetTransactionSnapshot()?))?
    } else {
        None
    };

    parallel::InitializeParallelDSM(pcxt)?;
    let mut pscan = ParallelTableScanDescShared::default();
    tableam::table_parallelscan_initialize(heap, &mut pscan, &snapshot.clone())?;
    let shared = make_shared(pscan)?;
    parallel::set_private(pcxt, Arc::clone(&shared) as Arc<dyn Any + Send + Sync>);

    // Launch workers, saving status for leader/caller
    parallel::LaunchParallelWorkers(pcxt)?;
    let leader = Leader {
        pcxt,
        nworkers_launched: parallel::nworkers_launched(pcxt),
        snapshot,
    };

    // If no workers were successfully launched, back out (do serial build)
    if leader.nworkers_launched == 0 {
        end_parallel(leader)?;
        return Ok(None);
    }

    // Log participants
    let (file, line, func) = using_at;
    elog::ereport(DEBUG1)
        .errmsg(format!("using {} parallel workers", leader.nworkers_launched))
        .finish(ErrorLocation::new(file, line, func))?;
    Ok(Some((leader, shared)))
}

/// HnswEndParallel / IvfflatEndParallel (hnswbuild.c:891-902, ivfbuild.c:788-799).
pub fn end_parallel(leader: Leader) -> PgResult<()> {
    // Shutdown worker processes
    parallel::WaitForParallelWorkersToFinish(leader.pcxt)?;
    // Free last reference to MVCC snapshot, if one was used
    if leader.snapshot.is_some() {
        snapmgr::UnregisterSnapshot(leader.snapshot.as_ref());
    }
    parallel::DestroyParallelContext(leader.pcxt)?;
    xact::ExitParallelMode();
    Ok(())
}

/// The leader's statement, for the workers' debug_query_string
/// (PARALLEL_KEY_QUERY_TEXT, hnswbuild.c:975-982, ivfbuild.c:877-885).
pub fn leader_query_text() -> Option<String> {
    elog::with_debug_query_string(|q| q.map(str::to_owned))
}

/// Set debug_query_string for the worker and report the leader's statement
/// for monitoring, then run `f` (hnswbuild.c:853-857, ivfbuild.c:740-744).
pub fn with_worker_activity<R>(
    query_text: Option<&str>,
    f: impl FnOnce() -> PgResult<R>,
) -> PgResult<R> {
    let _scope = query_text.map(elog::debug_query_string_scope);
    backend_status::pgstat_report_activity(backend_status::BackendState::STATE_RUNNING, query_text);
    f()
}

/// A worker's heap and index, opened with the build's lock modes.
pub struct WorkerRelations<'mcx> {
    pub heap: Relation<'mcx>,
    pub index: Relation<'mcx>,
    heap_lock: LOCKMODE,
    index_lock: LOCKMODE,
}

/// Open relations using lock modes known to be obtained by index.c
/// (hnswbuild.c:862-876, ivfbuild.c:749-763). The worker is a member of its
/// leader's lock group, so these never wait for the leader's own locks.
pub fn worker_open<'mcx>(
    mcx: Mcx<'mcx>,
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
) -> PgResult<WorkerRelations<'mcx>> {
    let (heap_lock, index_lock) = if !isconcurrent {
        (ShareLock, AccessExclusiveLock)
    } else {
        (ShareUpdateExclusiveLock, RowExclusiveLock)
    };
    let heap = table::table_open(mcx, heaprelid, heap_lock)?;
    let index = indexam::index_open(mcx, indexrelid, index_lock)?;
    Ok(WorkerRelations { heap, index, heap_lock, index_lock })
}

/// hnswbuild.c:884-885, ivfbuild.c:780-781.
pub fn worker_close(rels: WorkerRelations<'_>) -> PgResult<()> {
    indexam::index_close(rels.index, rels.index_lock)?;
    table::table_close(rels.heap, rels.heap_lock)
}

/// Log statistics: "leader processed %lld tuples" / "worker processed %lld
/// tuples", the heap tuples this participant scanned (hnswbuild.c:826-830,
/// ivfbuild.c:707-711). A worker's line reaches the client through its
/// leader, with "CONTEXT:  parallel worker".
pub fn report_processed(
    leader: bool,
    reltuples: f64,
    file: &'static str,
    leader_line: i32,
    worker_line: i32,
    func: &'static str,
) -> PgResult<()> {
    let (who, line) = if leader { ("leader", leader_line) } else { ("worker", worker_line) };
    elog::ereport(DEBUG1)
        .errmsg(format!("{who} processed {} tuples", reltuples as i64))
        .finish(ErrorLocation::new(file, line, func))
}

/// Wake a backend sleeping on its latch, the role ConditionVariableSignal
/// plays for C's workersdonecv: the IVFFlat leader waiting for rows, or a
/// worker waiting for room in the leader's mailbox. `fn(usize)` so a mailbox
/// can call it.
pub fn wake_proc(procno: usize) {
    latch::set_latch(&lmgr_proc::GetPGProcByNumber(procno as types_core::ProcNumber).procLatch);
}

/// This backend's ProcNumber, as `wake_proc`'s argument.
pub fn my_proc_number() -> usize {
    init_small::globals::MyProcNumber() as usize
}
```

If `cargo check` reports that `ProcNumber` is not in `types_core`'s root, import it from the path `condition_variable/src/lib.rs` uses (`use types_core::{ProcNumber, INVALID_PROC_NUMBER};` there); if `LOCKMODE` or a lock constant is not re-exported by `types_rel::lock`, take it from `types_storage::lock` (`types_storage/src/lock.rs:22-30`) and add the dependency.

- [ ] **Step 4: Run the test and the build**

Run: `cargo test -p pgvector_parallel --lib kill_switch_reads_only_zero_as_off`
Expected: PASS.

Run: `cargo check -p pgvector_parallel`
Expected: no errors or warnings.

- [ ] **Step 5: The kill switch's ledger row**

Add this row to `crates/_support/seams_init/tests/lint-determinism.allow`, directly after the `env	crates/backend/access/nbtree/nbtsort/src/pool.rs	6	...` row (tab-separated; the annotation is one line):

```
env	crates/contrib/pgvector_parallel/src/lib.rs	1	DST-REVIEW(m6-vector-parallel): M6 kill switch PGRUST_VECTOR_INDEXBUILD_POOL=0 (spec §7.1) turns parallel HNSW/IVFFlat builds off; read once per CREATE INDEX on the leader, never in a worker; same class as nbtsort pool.rs's PGRUST_RUNTIME_INDEXBUILD_POOL row; knobs-registry routing (s2.5) rides the chartered DST migration
```

Run: `cargo test -p seams_init --test lint_determinism`
Expected: PASS, with the same 43 warnings as on `main` (`grep -c WARN` on the output of `bash crates/_support/seams_init/tests/lint-determinism.sh`).

- [ ] **Step 6: Write the failing harness self-test**

Insert above `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# sqldiff drops a parallel build's per-participant lines (their counts and
# order vary run to run, C against C), so two C servers agree on a forced
# parallel IVFFlat build; the transcript still shows the build went parallel.
test_sqldiff_parallel_counts() {
  local log="$PGV_WORK/sqldiff-parallel.log"
  if "$here/../sqldiff.sh" ref - >"$log" 2>&1 <<'SQL'; then
CREATE TABLE t (id int, v vector(3));
INSERT INTO t SELECT i, ARRAY[i % 7, i % 11, i % 13] FROM generate_series(1, 20000) i;
SET min_parallel_table_scan_size = 1;
SET client_min_messages = debug1;
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
SQL
    pass "sqldiff: parallel builds compare equal"
  else
    fail "sqldiff: parallel builds compare equal (see $log)"
  fi
  if grep -q 'using 1 parallel workers' "$PGV_WORK/sqldiff/ref/ref.out" &&
    ! grep -Eq 'processed [0-9]+ tuples|CONTEXT:  parallel worker' "$PGV_WORK/sqldiff/ref/ref.out"; then
    pass "sqldiff: parallel build kept, participant lines dropped"
  else
    fail "sqldiff: parallel build kept, participant lines dropped (see $PGV_WORK/sqldiff/ref/ref.out)"
  fi
}
```

Run: `scripts/pgvector/tests/harness_test.sh test_sqldiff_parallel_counts`
Expected: the first check FAILs when the participants' counts differ between the two C servers. (It may pass by chance when they match; the second check FAILs either way, because `processed` lines are still present.)

- [ ] **Step 7: The sqldiff filter and the server pin**

In `scripts/pgvector/sqldiff.sh`, add after `strip_seeded_info`:

```bash
# A parallel build prints one "leader processed N tuples" / "worker processed
# N tuples" DEBUG1 line per participant (hnswbuild.c:826-830,
# ivfbuild.c:707-711), and every message a worker sends carries
# "CONTEXT:  parallel worker" (parallel.c:1179-1186). The counts and the order
# vary run to run, C against C, and which participant raises a NOTICE or an
# ERROR is not fixed, so drop both kinds of line before comparing.
strip_parallel_participants() {
  awk '/^psql:[^:]*:[0-9]+: DEBUG:  (leader|worker) processed [0-9]+ tuples$/ { next }
    /^CONTEXT:  parallel worker$/ { next }
    { print }' "$1" >"$1.tmp"
  mv "$1.tmp" "$1"
}
```

and call it right after `strip_seeded_info "$out/$side.out"`: `strip_parallel_participants "$out/$side.out"`.

In `scripts/pgvector/common.sh`, replace the comment at lines 59-63 and the array at line 64 with:

```bash
# Two pins of C's defaults, both deliberate pgrust changes:
#  - max_parallel_workers_per_gather=2: pgrust ships 4 (guc_tables/src/tables.rs:971),
#    which changes parallel plans pgvector's tests depend on (TAP 018's
#    saturating halfvec sum);
#  - min_parallel_table_scan_size=8MB: pgrust ships 1MB (tables.rs:988), which
#    would make TAP 002, 005, 006 and 041-044 and the on-disk follow-up's
#    REINDEX build in parallel on pgrust but serially on C (M6).
# fuzzgen's runner::C_PARITY_GUC_PIN pins the same. Keep in step with
# run-tap.sh's shim.
PGRUST_SERVER_OPTS=(-c listen_addresses= -c io_method=sync -c max_stack_depth=60000 -c max_parallel_workers_per_gather=2 -c min_parallel_table_scan_size=8MB)
```

In `scripts/pgvector/run-tap.sh`, change the shim comment's last bullet to `#  - The shim applies pgrust's runtime settings (README quickstart), plus C's max_parallel_workers_per_gather and min_parallel_table_scan_size (see PGRUST_SERVER_OPTS in common.sh).` and append ` -c min_parallel_table_scan_size=8MB` to the shim's `exec` line.

Run: `/bin/bash -n scripts/pgvector/sqldiff.sh scripts/pgvector/common.sh scripts/pgvector/run-tap.sh scripts/pgvector/tests/harness_test.sh`
Expected: no output.

- [ ] **Step 8: Run the self-tests**

Run: `scripts/pgvector/tests/harness_test.sh test_sqldiff_parallel_counts test_sqldiff_ref test_sqldiff_detects_difference test_tap_ref test_tap_identity`
Expected: every line `ok`, then `all passed`.

- [ ] **Step 9: Commit**

```bash
git add crates/contrib/pgvector_parallel Cargo.toml Cargo.lock crates/_support/seams_init/tests/lint-determinism.allow \
  scripts/pgvector/common.sh scripts/pgvector/run-tap.sh scripts/pgvector/sqldiff.sh scripts/pgvector/tests/harness_test.sh
git commit -m "feat(pgvector): pgvector_parallel plumbing and kill switch; harness pins C's min_parallel_table_scan_size and filters participant lines

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 3: The parallel IVFFlat build, end to end

**Files:**
- Create: `crates/contrib/pgvector_ivfflat_build/src/parallel_build.rs`
- Modify: `crates/contrib/pgvector_ivfflat_build/src/lib.rs` (header, `mod parallel_build;`, `assign_tuple` split out of `add_tuple_to_sort`, `assign_tuples`), `Cargo.toml` (+`pgvector_parallel`, `parallel`, `pgsync`, `tableam`), `Cargo.lock`
- Create: `scripts/pgvector/sql/parallel-build.sql` (the IVFFlat part)
- Modify: `scripts/pgvector/run-diff.sh` (the `parallel_build` row), `scripts/pgvector/tests/harness_test.sh` (`test_diff_ref` wants 22)

**Interfaces:**
- Consumes: `execindexing::table_index_build_scan_parallel` (Task 1); `pgvector_parallel::{vector_parallel_builds_enabled, plan_create_index_workers, begin_parallel, end_parallel, leader_query_text, with_worker_activity, worker_open, worker_close, report_processed, wake_proc, my_proc_number}` (Task 2).
- Produces: `fn assign_tuple<'t>(bs: &mut BuildState<'_, '_, '_>, tmp: Mcx<'t>, values: &[Datum]) -> PgResult<Option<(i32, PgVec<'t, u8>)>>` in `lib.rs`; `pub(crate) fn assign_tuples_parallel<'mcx>(bs: &mut BuildState<'_, '_, 'mcx>, mcx: Mcx<'mcx>, heap: &Relation<'mcx>, index_info: &mut IndexInfo<'mcx>, request: i32) -> PgResult<bool>` in `parallel_build.rs`; the diff row `parallel_build`.

- [ ] **Step 1: Write the failing test**

Create `crates/contrib/pgvector_ivfflat_build/src/parallel_build.rs` with only its test module, and add `mod parallel_build;` under `pub mod kmeans;` in `lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // A batch keeps each value 8-byte aligned (the sort reads a varlena header
    // in place) and byte-identical, with its list and TID.
    #[test]
    fn assign_batch_keeps_values_aligned() {
        let mut b = AssignBatch::new();
        let v1: Vec<u8> = (0..20u8).collect();
        let v2: Vec<u8> = (100..109u8).collect();
        b.push(3, ItemPointerData::new(7, 2), &v1);
        b.push(5, ItemPointerData::new(8, 1), &v2);
        assert_eq!(b.len(), 2);
        assert_eq!(b.bytes(), 40);
        assert_eq!(b.lists, vec![3, 5]);
        assert_eq!(b.tids, vec![ItemPointerData::new(7, 2), ItemPointerData::new(8, 1)]);
        for (i, v) in [&v1, &v2].into_iter().enumerate() {
            let p = b.value_ptr(i);
            assert_eq!(p as usize % 8, 0);
            // SAFETY: the batch holds v.len() bytes at p.
            assert_eq!(unsafe { core::slice::from_raw_parts(p, v.len()) }, &v[..]);
        }
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pgvector_ivfflat_build --lib assign_batch_keeps_values_aligned`
Expected: FAIL to compile, "cannot find type `AssignBatch`".

- [ ] **Step 3: Split `assign_tuple` out of `add_tuple_to_sort`**

In `lib.rs`, replace the two header lines `//! DIVERGENCE (recorded): no parallel build (plan_create_index_workers is` / `//! unported, M6); C builds serially when no workers launch.` with `//! The assign phase runs in parallel when the planner allows workers (parallel_build.rs).`, change `use mcx::Mcx;` to `use mcx::{Mcx, PgVec};`, and replace `add_tuple_to_sort` with:

```rust
// AddTupleToSort (ivfbuild.c:164-225) up to the sort: the nearest center's
// list and the (possibly normalized) value the sort stores, or None when the
// norm check skips the row. A parallel worker sends the pair to its leader.
fn assign_tuple<'t>(
    bs: &mut BuildState<'_, '_, '_>,
    tmp: Mcx<'t>,
    values: &[Datum],
) -> PgResult<Option<(i32, PgVec<'t, u8>)>> {
    let mut min_distance = f64::MAX;
    let mut closest_center = 0i32;

    // Detoast once for all calls
    let mut value = detoast_value(tmp, values[0])?;

    // Normalize if needed
    if let Some(np) = bs.normprocinfo.as_mut() {
        if !ivfflat_check_norm(np, bs.collation, Datum::from_usize(value.as_ptr() as usize))? {
            return Ok(None);
        }
        value = ivfflat_norm_value(bs.type_info, bs.collation, tmp, &value)?;
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
    Ok(Some((closest_center, value)))
}

// AddTupleToSort (ivfbuild.c:164-225).
fn add_tuple_to_sort(bs: &mut BuildState<'_, '_, '_>, tid: &ItemPointerData, values: &[Datum]) -> PgResult<()> {
    let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");
    let Some((closest_center, value)) = assign_tuple(bs, tmp.mcx(), values)? else {
        return Ok(());
    };

    // Add tuple to sort; tuplesort copies the input, the TID by reference
    // (TIDOID)
    let tid_datum = Datum::from_usize(tid as *const ItemPointerData as usize);
    bs.sortstate.as_mut().expect("sort state").put(&[
        Datum::from_i32(closest_center),
        tid_datum,
        Datum::from_usize(value.as_ptr() as usize),
    ])?;

    bs.indtuples += 1.0;
    Ok(())
}
```

Run: `cargo test -p pgvector_ivfflat_build --lib ivf_` (the existing 6 tests; the new one still fails to compile only if `parallel_build.rs` is not yet written, so run this after Step 4 if needed).

- [ ] **Step 4: The parallel assign phase**

Put this above the test module in `parallel_build.rs`:

```rust
//! ivfbuild.c's parallel assign phase: IvfflatBeginParallel,
//! IvfflatParallelBuildMain, IvfflatParallelScanAndSort, ParallelHeapScan and
//! IvfflatEndParallel (ivfbuild.c:626-972), on pgvector_parallel's launched
//! workers. Sampling, k-means, the list pages and the load stay serial on the
//! leader, as in C.
//!
//! DIVERGENCE (spec §7.3): C's participants each sort their share into a tape,
//! and the leader merges the tapes through a SharedFileSet (ivfbuild.c:661-720,
//! 994-1000). pgrust's Tuplesort is thread-affine (nbtsort/src/pool.rs:20-44),
//! so each worker assigns its rows to their nearest center and sends
//! (list, tid, value) batches to the leader, which feeds its one sort. List
//! membership is C's. The order within a list differs, which C's own sort,
//! keyed on the list only (ivfbuild.c:612-621), does not fix either. The
//! parallel phase ends after the scan: there are no shared tapes to keep
//! alive through the load.

use std::sync::Arc;

use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgsync::{mailbox, MailboxReceiver, MailboxSender, Mutex, TryRecv};
use pgvector_ivfflat::sort::HeapSort;
use tableam::ParallelTableScanDescShared;
use types_core::Oid;
use types_error::{PgError, PgResult};
use types_rel::Relation;
use types_tuple::itemptr::ItemPointerData;

use crate::{add_tuple_to_sort, assign_tuple, init_build_state, BuildState};

const ENTRY_NAME: &str = "pgrust_ivfflat_parallel_build_main";
/// A worker sends its batch once the values reach this many bytes.
const BATCH_BYTES: usize = 64 * 1024;
/// Batches in flight before a worker waits for the leader.
const MAILBOX_BATCHES: usize = 64;

/// IvfflatShared (ivfflat.h:151-173), the centers (PARALLEL_KEY_IVFFLAT_CENTERS)
/// and the rows' channel to the leader.
struct IvfflatShared {
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
    pscan: ParallelTableScanDescShared,
    /// The leader's centers->items (ivfbuild.c:925-926), `itemsize` bytes per list.
    centers: Vec<u8>,
    rows: MailboxSender<AssignBatch>,
    /// The leader's ProcNumber, to wake it when rows arrive or a worker finishes.
    leader: usize,
    query_text: Option<String>,
    stats: Mutex<IvfflatStats>,
}

#[derive(Default)]
struct IvfflatStats {
    nparticipantsdone: i32,
    reltuples: f64,
    indtuples: f64,
}

/// Rows a participant assigned: the list, the heap TID and the value the sort
/// stores, each value 8-byte aligned.
struct AssignBatch {
    lists: Vec<i32>,
    tids: Vec<ItemPointerData>,
    offsets: Vec<usize>,
    words: Vec<u64>,
}

impl AssignBatch {
    fn new() -> AssignBatch {
        AssignBatch { lists: Vec::new(), tids: Vec::new(), offsets: Vec::new(), words: Vec::new() }
    }

    fn push(&mut self, list: i32, tid: ItemPointerData, value: &[u8]) {
        let offset = self.words.len();
        self.words.resize(offset + value.len().div_ceil(8), 0);
        // SAFETY: the words just added span at least value.len() bytes, and
        // u8 has no alignment or validity requirement.
        unsafe {
            core::slice::from_raw_parts_mut(self.words[offset..].as_mut_ptr().cast::<u8>(), value.len())
        }
        .copy_from_slice(value);
        self.lists.push(list);
        self.tids.push(tid);
        self.offsets.push(offset);
    }

    fn len(&self) -> usize {
        self.lists.len()
    }

    fn bytes(&self) -> usize {
        self.words.len() * 8
    }

    fn value_ptr(&self, i: usize) -> *const u8 {
        self.words[self.offsets[i]..].as_ptr().cast::<u8>()
    }

    /// Add every row to the leader's sort, as AddTupleToSort does
    /// (ivfbuild.c:206-222); the sort copies the TID and the value.
    fn put_all(&self, sort: &mut HeapSort) -> PgResult<()> {
        for i in 0..self.len() {
            sort.put(&[
                Datum::from_i32(self.lists[i]),
                Datum::from_usize(&self.tids[i] as *const ItemPointerData as usize),
                Datum::from_usize(self.value_ptr(i) as usize),
            ])?;
        }
        Ok(())
    }
}

/// IvfflatBeginParallel, IvfflatLeaderParticipateAsWorker, ParallelHeapScan
/// and IvfflatEndParallel (ivfbuild.c:626-656, 813-972). Ok(false) when no
/// worker launched: the caller then scans serially (ivfbuild.c:953-958).
pub(crate) fn assign_tuples_parallel<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
    request: i32,
) -> PgResult<bool> {
    let isconcurrent = index_info.ii_Concurrent;
    let (heaprelid, indexrelid) = (heap.rd_id, bs.index.rd_id);
    // Store centers in shared memory (ivfbuild.c:925-926)
    let mut centers = Vec::with_capacity(bs.centers.maxlen as usize * bs.centers.itemsize);
    for i in 0..bs.centers.maxlen {
        centers.extend_from_slice(bs.centers.get(i)?);
    }
    let (rows, receiver) = mailbox(Some(MAILBOX_BATCHES));
    let Some((leader, shared)) = pgvector_parallel::begin_parallel(
        ENTRY_NAME,
        ivfflat_parallel_build_main,
        request,
        heap,
        isconcurrent,
        move |pscan| {
            Ok(Arc::new(IvfflatShared {
                heaprelid,
                indexrelid,
                isconcurrent,
                pscan,
                centers,
                rows,
                leader: pgvector_parallel::my_proc_number(),
                query_text: pgvector_parallel::leader_query_text(),
                stats: Mutex::new(IvfflatStats::default()),
            }))
        },
        ("ivfbuild.c", 961, "IvfflatBeginParallel"),
    )?
    else {
        return Ok(false);
    };
    let nparticipants = leader.nworkers_launched + 1;

    // IvfflatLeaderParticipateAsWorker (ivfbuild.c:813-830): the leader's own
    // rows go straight into its sort, and the workers' batches are fed in
    // between them.
    leader_scan(bs, mcx, heap, index_info, &shared, &receiver)?;

    // ivfbuild.c:971
    parallel::WaitForParallelWorkersToAttach(leader.pcxt)?;

    // ParallelHeapScan (ivfbuild.c:626-656): wait for every participant,
    // feeding their rows to the sort. A worker's error is raised by
    // check_for_interrupts here.
    let sort = bs.sortstate.as_mut().expect("sort state");
    loop {
        drain(&receiver, sort)?;
        let (done, reltuples, indtuples) = {
            let s = pgsync::lock(&shared.stats);
            (s.nparticipantsdone, s.reltuples, s.indtuples)
        };
        if done == nparticipants {
            // Every participant sent its last batch before counting itself done.
            drain(&receiver, sort)?;
            bs.reltuples = reltuples;
            bs.indtuples = indtuples;
            break;
        }
        parallel::wait_parallel_finish_quantum()?;
        postgres_seams::check_for_interrupts::call()?;
    }

    pgvector_parallel::end_parallel(leader)?;
    Ok(true)
}

/// The leader's share of the scan (IvfflatParallelScanAndSort with
/// progress = true, ivfbuild.c:661-720).
fn leader_scan<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
    shared: &IvfflatShared,
    receiver: &MailboxReceiver<AssignBatch>,
) -> PgResult<()> {
    let index = bs.index;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        index_info,
        /* progress */ true,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            // Skip nulls
            if isnull[0] {
                return Ok(());
            }
            add_tuple_to_sort(bs, tid, values)
                .and_then(|()| drain(receiver, bs.sortstate.as_mut().expect("sort state")))
                .map_err(|e| {
                    inner_err = Some(e);
                    PgError::error("ivfflat build insert failed").into()
                })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };

    // Record statistics (ivfbuild.c:698-705)
    {
        let mut s = pgsync::lock(&shared.stats);
        s.nparticipantsdone += 1;
        s.reltuples += reltuples;
        s.indtuples += bs.indtuples;
    }
    pgvector_parallel::report_processed(true, reltuples, "ivfbuild.c", 709, 711, "IvfflatParallelScanAndSort")
}

/// Feed every batch waiting in the mailbox to the leader's sort.
fn drain(receiver: &MailboxReceiver<AssignBatch>, sort: &mut HeapSort) -> PgResult<()> {
    loop {
        match receiver.try_recv() {
            TryRecv::Msg(batch) => batch.put_all(sort)?,
            TryRecv::Empty | TryRecv::Disconnected => return Ok(()),
        }
    }
}

/// IvfflatParallelBuildMain (ivfbuild.c:725-783).
fn ivfflat_parallel_build_main(pshared: &parallel::ParallelShared) -> PgResult<()> {
    let shared = pshared
        .private()
        .expect("ivfflat parallel build worker without its shared state")
        .downcast::<IvfflatShared>()
        .unwrap_or_else(|_| panic!("ivfflat parallel build private state is not IvfflatShared"));
    pgvector_parallel::with_worker_activity(shared.query_text.as_deref(), || {
        let cx = mcx::MemoryContext::new("Ivfflat parallel build worker");
        let mcx = cx.mcx();
        let rels = pgvector_parallel::worker_open(mcx, shared.heaprelid, shared.indexrelid, shared.isconcurrent)?;
        worker_scan(mcx, &rels.heap, &rels.index, &shared)?;
        pgvector_parallel::worker_close(rels)
    })
}

/// IvfflatParallelScanAndSort (ivfbuild.c:661-720), a worker's side.
fn worker_scan<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    shared: &IvfflatShared,
) -> PgResult<()> {
    // Join parallel scan
    let mut index_info = execindexing::BuildIndexInfo(mcx, index)?;
    index_info.ii_Concurrent = shared.isconcurrent;
    let build_cx = mcx::MemoryContext::new("Ivfflat build context");
    let mut bs = init_build_state(Some(heap), index, build_cx.mcx())?;
    // The leader's centers (ivfbuild.c:680-681)
    let itemsize = bs.centers.itemsize;
    for i in 0..bs.centers.maxlen {
        let start = i as usize * itemsize;
        bs.centers.get_mut(i)?.copy_from_slice(&shared.centers[start..start + itemsize]);
    }
    bs.centers.length = bs.centers.maxlen;

    let mut batch = AssignBatch::new();
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        &mut index_info,
        /* progress */ false,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            // Skip nulls
            if isnull[0] {
                return Ok(());
            }
            let r = (|| -> PgResult<()> {
                let tmp = mcx::MemoryContext::new_bump("Ivfflat build temporary context");
                if let Some((list, value)) = assign_tuple(&mut bs, tmp.mcx(), values)? {
                    batch.push(list, *tid, &value);
                    bs.indtuples += 1.0;
                    if batch.bytes() >= BATCH_BYTES {
                        send(shared, std::mem::replace(&mut batch, AssignBatch::new()))?;
                    }
                }
                Ok(())
            })();
            r.map_err(|e| {
                inner_err = Some(e);
                PgError::error("ivfflat build insert failed").into()
            })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };
    if batch.len() > 0 {
        send(shared, batch)?;
    }

    // Record statistics (ivfbuild.c:698-705)
    {
        let mut s = pgsync::lock(&shared.stats);
        s.nparticipantsdone += 1;
        s.reltuples += reltuples;
        s.indtuples += bs.indtuples;
    }
    pgvector_parallel::report_processed(false, reltuples, "ivfbuild.c", 709, 711, "IvfflatParallelScanAndSort")?;
    // Notify leader (ivfbuild.c:714)
    pgvector_parallel::wake_proc(shared.leader);
    Ok(())
}

/// Send a batch to the leader, waiting (interruptibly, on this worker's
/// latch) while the leader's mailbox is full.
fn send(shared: &IvfflatShared, batch: AssignBatch) -> PgResult<()> {
    let wake = (pgvector_parallel::wake_proc as fn(usize), pgvector_parallel::my_proc_number());
    match shared.rows.send_interruptible(batch, wake, || {
        parallel::wait_parallel_finish_quantum()?;
        postgres_seams::check_for_interrupts::call()
    })? {
        Ok(()) => {
            pgvector_parallel::wake_proc(shared.leader);
            Ok(())
        }
        // The leader dropped its receiver: it is unwinding, and this worker
        // is about to be terminated.
        Err(_) => Err(PgError::error("ivfflat parallel build leader stopped receiving rows").into()),
    }
}
```

In `lib.rs`, replace `assign_tuples` with:

```rust
// AssignTuples (ivfbuild.c:977-1018). BuildCallback (ivfbuild.c:230-250) is
// the serial closure; parallel_build.rs is the parallel arm.
fn assign_tuples<'mcx>(
    bs: &mut BuildState<'_, '_, 'mcx>,
    mcx: Mcx<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
) -> PgResult<()> {
    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_IVFFLAT_PHASE_ASSIGN,
    );

    // Calculate parallel workers (ivfbuild.c:986-987), behind the kill switch
    let parallel_workers = match bs.heap {
        Some(heap) if pgvector_parallel::vector_parallel_builds_enabled() => {
            pgvector_parallel::plan_create_index_workers(heap, bs.index)?
        }
        _ => 0,
    };

    // Begin the leader's tuplesort (InitBuildSortState, ivfbuild.c:612-621,
    // over InitBuildState's sortdesc, ivfbuild.c:386-390); a parallel build
    // feeds it every participant's rows.
    let value_type = bs.index.rd_att.attr(0).atttypid;
    bs.sortstate = Some(HeapSort::begin(
        "Ivfflat build sort",
        &[("list", INT4OID), ("tid", TIDOID), ("vector", value_type)],
        INT4_LESS_OPERATOR,
        init_small::globals::maintenance_work_mem(),
    )?);

    // Add tuples to sort
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        // Attempt to launch parallel worker scan when required (ivfbuild.c:990-991)
        if parallel_workers > 0
            && parallel_build::assign_tuples_parallel(bs, mcx, heap, &mut *index_info, parallel_workers)?
        {
            return Ok(());
        }

        let index = bs.index;
        let mut inner_err: Option<Box<PgError>> = None;
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
```

(The serial closure now borrows `bs` directly: `heap` and `index` are copies of `bs`'s references, so the old raw pointer is not needed. Keep the raw pointer in `sample_rows`, which this task does not touch.)

Add to `Cargo.toml` `[dependencies]`: `pgvector_parallel = { path = "../pgvector_parallel" }`, `parallel = { path = "../../backend/access/transam/parallel" }`, `pgsync = { path = "../../_support/pgsync" }`, `tableam = { path = "../../backend/access/table/tableam" }`.

- [ ] **Step 5: Unit tests**

Run: `cargo test -p pgvector_ivfflat_build --lib`
Expected: PASS, 7 tests (the 6 existing and `assign_batch_keeps_values_aligned`).

Run: `cargo build --profile fast-profile --bin postgres` once without `--locked`, then `scripts/pgvector/build-pgrust.sh`.
Expected: both build.

- [ ] **Step 6: The IVFFlat TAP gate, three times**

Run, three times:

```bash
PGV_WORK=$HOME/.cache/pgrust/m6-work scripts/pgvector/run-tap.sh pgrust \
  001_ivfflat_wal.pl 003_ivfflat_vector_build_recall.pl 032_ivfflat_halfvec_build_recall.pl 035_ivfflat_bit_build_recall.pl
grep -h 'parallel workers\|processed' $HOME/.cache/pgrust/m6-work/tap/pgrust/prove/003_ivfflat_vector_build_recall.log | head
```

Expected: `tap (pgrust): 4/4 passed` each time. The 003 prove log shows `with request for 1 parallel workers`, `using 1 parallel workers`, one `leader processed` and one `worker processed` line per parallel build, summing to 100000. `001` is the only TAP test where C goes parallel at stock settings (about 2128 heap pages); it now builds in parallel on pgrust too, then replays on the standby.

If a run hangs, look at `pg_locks` in the TAP node's database while it hangs: a worker waiting on a relation lock its leader holds is a plan-defined stop.

- [ ] **Step 7: The `parallel_build` diff row**

Create `scripts/pgvector/sql/parallel-build.sql`:

```sql
-- Parallel IVFFlat and HNSW builds against C (M6). Workers are forced the
-- way pgvector's TAP tests force them (min_parallel_table_scan_size = 1, or
-- the parallel_workers reloption). sqldiff drops the per-participant
-- "leader/worker processed N tuples" lines and every "CONTEXT:  parallel
-- worker" line; what is compared is index.c's request line, pgvector's
-- "using N parallel workers", NOTICEs, errors and query results. DEBUG1 is on
-- only around the builds.

SET min_parallel_table_scan_size = 1;
SET enable_seqscan = off;

-- IVFFlat ---------------------------------------------------------------------
-- 20000 rows; the planner allows one worker at the default 64MB.
CREATE TABLE ivf (id int, v vector(3));
INSERT INTO ivf SELECT i, ARRAY[i % 7, (i * 3) % 11, (i * 5) % 13] FROM generate_series(1, 20000) i;
SET client_min_messages = debug1;
CREATE INDEX ivf_l2 ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
-- Every row is in some list: probing every list returns every row.
SET ivfflat.probes = 10;
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_l2;

-- The reloption skips the memory clamp: two workers even at 4MB
-- (planner.c:7193-7205).
ALTER TABLE ivf SET (parallel_workers = 2);
SET maintenance_work_mem = '4MB';
SET client_min_messages = debug1;
CREATE INDEX ivf_rel ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
RESET maintenance_work_mem;
ALTER TABLE ivf RESET (parallel_workers);
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_rel;

-- Review Focus 2: CREATE INDEX CONCURRENTLY with workers.
SET client_min_messages = debug1;
CREATE INDEX CONCURRENTLY ivf_cic ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_cic;

-- Review Focus 5: builds that stay serial.
-- Under 64MB of maintenance_work_mem the planner allows no worker
-- (planner.c:7220-7232).
SET maintenance_work_mem = '32MB';
SET client_min_messages = debug1;
CREATE INDEX ivf_mem ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
RESET maintenance_work_mem;
DROP INDEX ivf_mem;
-- parallel_workers = 0 (planner.c:7193-7205).
ALTER TABLE ivf SET (parallel_workers = 0);
SET client_min_messages = debug1;
CREATE INDEX ivf_zero ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
ALTER TABLE ivf RESET (parallel_workers);
DROP INDEX ivf_zero;
-- A temp table (planner.c:7185).
CREATE TEMP TABLE ivf_temp AS SELECT * FROM ivf;
SET client_min_messages = debug1;
CREATE INDEX ivf_temp_l2 ON ivf_temp USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
DROP TABLE ivf_temp;
-- A parallel-unsafe index expression (planner.c:7186-7190); PL/pgSQL so
-- neither server inlines it away.
CREATE FUNCTION unsafe_v(v vector) RETURNS vector LANGUAGE plpgsql IMMUTABLE PARALLEL UNSAFE
  AS $$ BEGIN RETURN v; END $$;
SET client_min_messages = debug1;
CREATE INDEX ivf_unsafe ON ivf USING ivfflat (unsafe_v(v) vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
DROP INDEX ivf_unsafe;
-- An unlogged table: the main fork builds in parallel, the init fork
-- serially (ivfbuild.c:1094-1097).
CREATE UNLOGGED TABLE ivf_unlogged AS SELECT * FROM ivf;
SET client_min_messages = debug1;
CREATE INDEX ivf_unlogged_l2 ON ivf_unlogged USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
SELECT count(*) FROM (SELECT id FROM ivf_unlogged ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP TABLE ivf_unlogged;
RESET ivfflat.probes;
```

In `scripts/pgvector/run-diff.sh`, add after the `core_error_locations` row (before `total=`):

```bash
# Parallel IVFFlat and HNSW builds (M6): index.c's request line, pgvector's
# "using N parallel workers", NOTICEs, errors and results, with each
# participant's own lines dropped (sqldiff.sh).
if "$here/sqldiff.sh" "$mode" "$here/sql/parallel-build.sql" >"$out/parallel-build.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'parallel_build\t%s\n' "$r" | tee -a "$out/summary.tsv"
```

In `scripts/pgvector/tests/harness_test.sh`'s `test_diff_ref`, change both `21`s to `22`.

Run: `/bin/bash -n scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh`, then `scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/parallel-build.sql` and `scripts/pgvector/sqldiff.sh ref scripts/pgvector/sql/parallel-build.sql`.
Expected: `identical` both times. In `$PGV_WORK/sqldiff/pgrust/pgrust.out`: four `with request for` lines, for `ivf_l2` (1), `ivf_rel` (2), `ivf_cic` (1) and `ivf_unlogged_l2` (1), each followed by `using N parallel workers` with the same N; and `serially` for `ivf_mem`, `ivf_zero`, `ivf_temp_l2` and `ivf_unsafe`. Every count is `20000`.

- [ ] **Step 8: The tiers that must not move**

Run: `scripts/pgvector/run-diff.sh pgrust`, `scripts/pgvector/run-diff.sh ref`, `scripts/pgvector/run-bytecmp.sh pgrust`, `scripts/pgvector/run-iterscan.sh pgrust`, `scripts/pgvector/run-regress.sh pgrust`.
Expected: diff 22/22 both modes; bytecmp 28/28 identical; iterscan 19/19; regress 14/14.

Run: `scripts/pgvector/tests/harness_test.sh test_diff_ref test_sqldiff_parallel_counts`
Expected: `all passed`.

Run: `cargo test -p seams_init --test lint_determinism`
Expected: PASS, 43 warnings.

- [ ] **Step 9: Commit**

```bash
git add crates/contrib/pgvector_ivfflat_build Cargo.lock scripts/pgvector/sql/parallel-build.sql \
  scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
git commit -m "feat(pgvector): parallel IVFFlat builds on launched workers; the parallel_build diff row

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 4: `pgvector_hnsw_graph`: the shared graph, C's lock protocol and C's memory arithmetic

**Files:**
- Create: `crates/contrib/pgvector_hnsw_graph/Cargo.toml`
- Create: `crates/contrib/pgvector_hnsw_graph/src/{lib.rs,arena.rs,memory.rs,algo.rs,tests.rs}`
- Create: `crates/contrib/pgvector_hnsw_graph/tests/loom.rs`
- Modify: `Cargo.toml` (workspace member after `"crates/contrib/pgvector_hnsw_build",`), `Cargo.lock`

**Interfaces:**
- Produces (`pgvector_hnsw_graph`):
  - `pub const HNSW_HEAPTIDS: usize = 10;` `pub type HeapTid = [u8; 6];`
  - `pub struct Candidate { pub element: u32, pub distance: f32, pub closer: bool }` (`Copy`), `pub struct NeighborArray { pub items: Vec<Candidate>, pub closer_set: bool }`
  - `pub struct Links { pub heaptids: [HeapTid; HNSW_HEAPTIDS], pub heaptids_len: u8, pub neighbors: Vec<NeighborArray> }` (`neighbors[level - lc]` is layer `lc`)
  - `pub struct Element { pub level: u8, .. }` with `pub fn value(&self) -> &[u8]` (8-byte aligned) and `pub fn links(&self) -> RwLockReadGuard<'_, Links>`
  - `pub struct InsertParams { pub m: i32, pub ef_construction: i32 }`, `pub enum Inserted { InMemory, OnDisk }`, `pub enum InsertError<E> { Callback(E), Graph(GraphError) }`
  - `pub struct Graph` with `new(Accounting)`, `element(u32) -> &Element`, `element_count() -> u32`, `head_newest_first() -> Vec<u32>`, `entry_point() -> Option<u32>`, `indtuples() -> f64`, `add_indtuple() -> f64`, `memory_used() -> usize`, `flushed() -> bool`, `insert<E>(&self, value: &[u8], heaptid: HeapTid, params: &InsertParams, draw_level: impl FnOnce() -> u8, distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>, on_overflow: impl FnOnce(&Graph) -> Result<(), E>) -> Result<Inserted, InsertError<E>>`, `flush_if_needed<E>(&self, flush: impl FnOnce(&Graph) -> Result<(), E>) -> Result<(), E>`
  - `pub mod memory` with `Accounting::{serial(usize), parallel(usize), parallel_area_size(i32) -> usize, used(), total(), is_full(), charge_element(level: u8, m: i32, value_len: usize) -> Result<(), GraphError>}`, `GenerationModel::{new(usize, usize, usize), alloc(usize), mem_allocated()}`, `pub enum GraphError { AllocationTooLarge, OutOfMemory }`, `pub const fn maxalign`, `pub const fn layer_m`, `pub const fn neighbor_array_size`, `pub const ELEMENT_DATA_SIZE: usize = 128`, `pub const HNSW_MAX_GRAPH_MEMORY: usize`.

The in-memory algorithm in `algo.rs` is `pgvector_hnsw_build/src/lib.rs:80-497` (`search_layer_mem` through `insert_tuple_in_memory`) moved verbatim, with three changes only: element access goes through `Graph::element`; C's locks are taken where C takes them (cited per site); and `MemCandidate` becomes the `Copy` type `Candidate`. Task 5 deletes the originals.

- [ ] **Step 1: The crate skeleton and its failing tests**

`crates/contrib/pgvector_hnsw_graph/Cargo.toml`:

```toml
[package]
name = "pgvector_hnsw_graph"
version = "0.0.0"
edition.workspace = true
publish.workspace = true

# pgvector's in-memory HNSW build graph and its lock protocol, shared by the
# serial and parallel builds (M6). Only pgsync and pairingheap: tests/loom.rs
# compiles this crate's whole dependency cone under --cfg loom, and backend
# crates' statics do not build there.

[lib]
name = "pgvector_hnsw_graph"
path = "src/lib.rs"

[dependencies]
pgsync = { path = "../../_support/pgsync" }
pairingheap = { path = "../../backend/lib/pairingheap" }

[target.'cfg(loom)'.dev-dependencies]
loom = "0.7"

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(loom)"] }
```

Add `"crates/contrib/pgvector_hnsw_graph",` to the workspace `members` after `"crates/contrib/pgvector_hnsw_build",`.

`src/tests.rs`:

```rust
use super::*;
use crate::memory::{layer_m, neighbor_array_size, ELEMENT_DATA_SIZE};

fn dist(a: &[u8], b: &[u8]) -> Result<f64, ()> {
    Ok((a[0] as f64 - b[0] as f64).abs())
}

fn params() -> InsertParams {
    InsertParams { m: 2, ef_construction: 4 }
}

fn tid(n: u8) -> HeapTid {
    [0, 0, 0, n, 0, 1]
}

fn insert(g: &Graph, x: u8, level: u8) -> Inserted {
    match g.insert(&[x], tid(x), &params(), || level, &mut dist, |_| Ok(())) {
        Ok(r) => r,
        Err(_) => panic!("insert of {x} failed"),
    }
}

fn layer_ids(g: &Graph, id: u32, lc: i32) -> Vec<u32> {
    let e = g.element(id);
    let links = e.links();
    links.neighbors[(e.level as i32 - lc) as usize].items.iter().map(|c| c.element).collect()
}

#[test]
fn arena_handles_cross_segments() {
    assert_eq!(arena::locate(0), (0, 0));
    assert_eq!(arena::locate(1023), (0, 1023));
    assert_eq!(arena::locate(1024), (1, 0));
    assert_eq!(arena::locate(3071), (1, 2047));
    assert_eq!(arena::locate(3072), (2, 0));
    assert_eq!(arena::locate(u32::MAX).0, 22);
}

// GenerationContextCreate(.., 1 MB, 1 MB, 1 MB) (hnswbuild.c:732-737): the
// keeper block shares the context's malloc, minus MAXALIGN(sizeof(
// GenerationContext)) = 128 (generation.c:229, 268).
#[test]
fn generation_model_starts_with_cs_keeper_block() {
    let g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    assert_eq!(g.mem_allocated(), (1 << 20) - 128);
}

// The keeper block holds 1048448 - 56 (block header) bytes; a 1000-byte
// request is a 1008-byte chunk with its header, so 1040 fit and the 1041st
// takes a new 1 MB block (generation.c:461-500).
#[test]
fn generation_model_adds_a_block_when_the_keeper_fills() {
    let mut g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    for _ in 0..1040 {
        g.alloc(1000);
    }
    assert_eq!(g.mem_allocated(), 1_048_448);
    g.alloc(1000);
    assert_eq!(g.mem_allocated(), 1_048_448 + (1 << 20));
}

// allocChunkLimit is 65536 for 1 MB blocks (generation.c:256-259); a larger
// chunk gets a block of its own (GenerationAllocLarge, generation.c:343-372).
#[test]
fn generation_model_gives_large_chunks_their_own_block() {
    let mut g = GenerationModel::new(1 << 20, 1 << 20, 1 << 20);
    g.alloc(65536);
    assert_eq!(g.mem_allocated(), 1_048_448);
    g.alloc(65537);
    assert_eq!(g.mem_allocated(), 1_048_448 + 65544 + 8 + 56);
}

// InitGraph sets memoryUsed = 0 (hnswbuild.c:625); HnswMemoryContextAlloc
// updates it to the context's block total after each allocation (:655).
#[test]
fn serial_accounting_is_zero_until_the_first_allocation() {
    let mut a = Accounting::serial(1 << 20);
    assert_eq!(a.used(), 0);
    assert!(!a.is_full());
    a.charge_element(0, 16, 20).unwrap();
    assert_eq!(a.used(), 1_048_448);
    assert!(!a.is_full());
}

// HnswSharedMemoryAlloc (hnswbuild.c:663-681): the MAXALIGNed sizes of
// HnswInitElement's allocations and the value (vector(3) is 20 bytes).
#[test]
fn parallel_accounting_charges_maxaligned_c_sizes() {
    let mut a = Accounting::parallel(2 << 20);
    a.charge_element(0, 16, 20).unwrap();
    assert_eq!(a.used(), ELEMENT_DATA_SIZE + 8 + neighbor_array_size(layer_m(16, 0)) + 24);
    assert_eq!(a.used(), 680);
}

// hnswbuild.c:961-969: maintenance_work_mem minus 3 MB, only when above 3 MB.
#[test]
fn parallel_area_matches_hnswbuild_c() {
    assert_eq!(Accounting::parallel_area_size(3073), 1024);
    assert_eq!(Accounting::parallel_area_size(3072), 3 << 20);
    assert_eq!(Accounting::parallel_area_size(4096), 1 << 20);
    assert_eq!(Accounting::parallel_area_size(65536), (64 << 20) - (3 << 20));
}

// The 1 MB parallel margin (hnswbuild.c:509-510, 533): an area of 1 MB or
// less is full before the first tuple (TAP 045's "after 0 tuples").
#[test]
fn parallel_margin_fills_a_small_area_before_the_first_tuple() {
    assert!(Accounting::parallel(1 << 20).is_full());
    assert!(!Accounting::parallel((1 << 20) + 1).is_full());
    assert!(Accounting::parallel(1024).is_full());
}

// hnswbuild.c:671-676.
#[test]
fn parallel_allocation_errors_match_c() {
    assert_eq!(
        Accounting::parallel(64 << 20).charge_element(0, 16, (1 << 20) + 1),
        Err(GraphError::AllocationTooLarge)
    );
    assert_eq!(Accounting::parallel(100).charge_element(0, 16, 20), Err(GraphError::OutOfMemory));
}

#[test]
fn first_insert_becomes_the_entry_point() {
    let g = Graph::new(Accounting::serial(64 << 20));
    assert_eq!(insert(&g, 10, 0), Inserted::InMemory);
    assert_eq!(g.entry_point(), Some(0));
    assert_eq!(g.head_newest_first(), vec![0]);
}

#[test]
fn a_higher_level_takes_over_the_entry_point_and_links_both_ways() {
    let g = Graph::new(Accounting::serial(64 << 20));
    insert(&g, 10, 0);
    insert(&g, 20, 1);
    assert_eq!(g.entry_point(), Some(1));
    assert_eq!(layer_ids(&g, 0, 0), vec![1]);
    assert_eq!(layer_ids(&g, 1, 0), vec![0]);
    assert_eq!(layer_ids(&g, 1, 1), Vec::<u32>::new());
    assert_eq!(g.head_newest_first(), vec![1, 0]);
}

// FindDuplicateInMemory / AddDuplicateInMemory (hnswbuild.c:321-364): the
// first element takes ten TIDs; the eleventh copy finds it full and becomes
// an element of its own. Merged copies stay allocated (C never frees them).
#[test]
fn duplicates_merge_heap_tids_until_ten() {
    let g = Graph::new(Accounting::serial(64 << 20));
    for _ in 0..11 {
        insert(&g, 5, 0);
    }
    assert_eq!(g.element(0).links().heaptids_len, 10);
    assert_eq!(g.head_newest_first(), vec![10, 0]);
    assert_eq!(g.element_count(), 11);
}

// hnswbuild.c:533-552: the first tuple that does not fit flushes (once), and
// it and every later tuple go to disk; none draws a level.
#[test]
fn overflow_flushes_once_then_goes_to_disk() {
    let g = Graph::new(Accounting::parallel(1 << 20));
    let mut flushes = 0;
    for x in 0..3u8 {
        let r = g.insert(
            &[x],
            tid(x),
            &params(),
            || -> u8 { panic!("a tuple that goes to disk draws no level here") },
            &mut dist,
            |gr| {
                flushes += 1;
                assert_eq!(gr.indtuples(), 0.0);
                Ok(())
            },
        );
        assert_eq!(r.unwrap_or_else(|_| panic!("insert")), Inserted::OnDisk);
    }
    assert_eq!(flushes, 1);
    assert!(g.flushed());
    assert_eq!(g.element_count(), 0);
}

// BuildGraph's final flush (hnswbuild.c:1119-1120) runs once, and later
// inserts go to disk.
#[test]
fn flush_if_needed_flushes_once() {
    let g = Graph::new(Accounting::serial(64 << 20));
    insert(&g, 1, 0);
    let mut n = 0;
    g.flush_if_needed(|gr| {
        n += 1;
        assert_eq!(gr.head_newest_first(), vec![0]);
        Ok::<(), ()>(())
    })
    .unwrap();
    g.flush_if_needed(|_| {
        n += 1;
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(n, 1);
    assert_eq!(insert(&g, 2, 0), Inserted::OnDisk);
}

// C HnswUpdateConnection mutates neighbors->items[i].closer through the
// shared candidate-list pointers; these pin that the in-memory port writes
// the computed closer flags (and the flagged newHc) back into the neighbor
// array instead of leaving stale flags for later pruning. Moved from
// pgvector_hnsw_build (M1), with one-dimensional float4 values in place of
// vector images and vector.c's squared L2 distance.
fn f32_value(x: f32) -> [u8; 4] {
    x.to_le_bytes()
}

fn l2_squared(a: &[u8], b: &[u8]) -> Result<f64, ()> {
    let x = f32::from_le_bytes(a[..4].try_into().unwrap());
    let y = f32::from_le_bytes(b[..4].try_into().unwrap());
    let d = x - y;
    Ok((d * d) as f64)
}

// New candidate survives selection: the pruned slot is replaced by the newHc
// carrying its computed closer=true, and the surviving original neighbor's
// recomputed closer=true is written back.
#[test]
fn update_connection_writes_back_closer_flags_on_replace() {
    let g = Graph::new(Accounting::serial(64 << 20));
    // element 0 at 1.0, element 1 at 1.1, element 2 (new) at -1.05; the owner
    // is conceptually at 0.0, distances squared below.
    for x in [1.0f32, 1.1, -1.05] {
        g.push_unlinked(0, &f32_value(x));
    }
    // Stale flags deliberately wrong (false); C recomputes in place.
    let mut neighbors = vec![
        Candidate { element: 0, distance: 1.0, closer: false },
        Candidate { element: 1, distance: 1.21, closer: false },
    ];
    let mut closer_set = false;
    g.update_connection(&mut neighbors, &mut closer_set, 2, 1.1025, 2, &mut l2_squared).unwrap();
    assert!(closer_set, "sortCandidates=true sets closerSet");
    // Selection keeps 0 (closer) and new 2 (closer vs {0}: d(2,0)^2=4.2 > 1.1025);
    // 1 is never popped (r fills first), pruned = leftover 1 -> replaced by newHc.
    let flags: Vec<(u32, bool)> = neighbors.iter().map(|n| (n.element, n.closer)).collect();
    assert_eq!(flags, vec![(0, true), (2, true)]);
}

// New candidate is itself pruned (kept-neighbor case): no replacement, but
// the surviving array must carry the freshly computed flags, including a
// false flag for the not-closer neighbor (wd-kept).
#[test]
fn update_connection_writes_back_closer_flags_without_replace() {
    let g = Graph::new(Accounting::serial(64 << 20));
    // 0 at 1.0, 1 at 1.1, new 2 at 1.2: 1 and 2 are both not-closer to 0;
    // wd-fill keeps 1, prunes the new candidate 2.
    for x in [1.0f32, 1.1, 1.2] {
        g.push_unlinked(0, &f32_value(x));
    }
    // Stale flags deliberately wrong (true).
    let mut neighbors = vec![
        Candidate { element: 0, distance: 1.0, closer: true },
        Candidate { element: 1, distance: 1.21, closer: true },
    ];
    let mut closer_set = false;
    g.update_connection(&mut neighbors, &mut closer_set, 2, 1.44, 2, &mut l2_squared).unwrap();
    assert!(closer_set);
    let flags: Vec<(u32, bool)> = neighbors.iter().map(|n| (n.element, n.closer)).collect();
    assert_eq!(flags, vec![(0, true), (1, false)]);
}
```

`src/lib.rs`, for now: the module list `mod algo; mod arena; pub mod memory; #[cfg(test)] mod tests;` and empty `algo.rs`, `arena.rs`, `memory.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pgvector_hnsw_graph --lib`
Expected: FAIL to compile (`Graph`, `Accounting`, `arena::locate` and the rest are not defined).

- [ ] **Step 3: `arena.rs` and `memory.rs`**

`src/arena.rs`:

```rust
//! Append-only element storage addressed by u32 handles, the role of C's
//! relptr into the graph's memory (hnsw.h:144-152). Segment k holds 1024 << k
//! slots, so a handle never moves once published and no segment is ever
//! reallocated; slots and segments are pgsync OnceLocks, set once and read
//! without a lock.

use pgsync::OnceLock;

const BASE_SHIFT: u32 = 10;
const BASE: u64 = 1 << BASE_SHIFT;
/// Segments 0..=22 cover every u32 handle.
const SEGMENTS: usize = 23;

pub(crate) struct Arena<T> {
    segments: [OnceLock<Box<[OnceLock<T>]>>; SEGMENTS],
}

/// A handle's segment and its slot in that segment.
pub(crate) fn locate(id: u32) -> (usize, usize) {
    let n = id as u64 + BASE;
    let k = (63 - n.leading_zeros()) - BASE_SHIFT;
    (k as usize, (n - (BASE << k)) as usize)
}

impl<T> Arena<T> {
    pub(crate) fn new() -> Arena<T> {
        Arena { segments: std::array::from_fn(|_| OnceLock::new()) }
    }

    /// Store `value` at `id`. The caller hands out ids in sequence under the
    /// graph's allocator lock, so no two calls race for a slot or a segment.
    pub(crate) fn publish(&self, id: u32, value: T) {
        let (k, slot) = locate(id);
        if self.segments[k].get().is_none() {
            let fresh: Box<[OnceLock<T>]> = (0..(BASE << k)).map(|_| OnceLock::new()).collect();
            if self.segments[k].set(fresh).is_err() {
                unreachable!("hnsw graph segment {k} created twice");
            }
        }
        let segment = self.segments[k].get().expect("segment just created");
        if segment[slot].set(value).is_err() {
            unreachable!("hnsw graph element {id} stored twice");
        }
    }

    pub(crate) fn get(&self, id: u32) -> &T {
        let (k, slot) = locate(id);
        self.segments[k]
            .get()
            .and_then(|segment| segment[slot].get())
            .unwrap_or_else(|| panic!("hnsw graph element {id} is not allocated"))
    }
}
```

`src/memory.rs`:

```rust
//! The graph's memory accounting: C's arithmetic over C's allocation sizes,
//! so a build flushes where C's does. C counts the bytes its allocator holds:
//! the graph's Generation context's blocks in a serial build
//! (HnswMemoryContextAlloc, hnswbuild.c:649-658), the MAXALIGNed bump
//! allocations from the shared area in a parallel build
//! (HnswSharedMemoryAlloc, hnswbuild.c:663-681). All sizes are LP64 PG 18.6.

/// MAXALIGN (pg_config.h MAXIMUM_ALIGNOF 8).
pub const fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

/// sizeof(HnswElementData) (hnsw.h:182-198); types_hnsw's
/// HNSW_ELEMENT_DATA_SIZE, which the scan-memory model uses too.
pub const ELEMENT_DATA_SIZE: usize = 128;
/// sizeof(HnswNeighborArrayPtr) (hnsw.h:178): a pointer / relptr union.
pub const NEIGHBOR_ARRAY_PTR_SIZE: usize = 8;
/// HNSW_MAX_GRAPH_MEMORY (hnswbuild.c:83): SIZE_MAX / 2.
pub const HNSW_MAX_GRAPH_MEMORY: usize = usize::MAX / 2;

/// HNSW_NEIGHBOR_ARRAY_SIZE(lm) (hnsw.h:84): offsetof(items) = 8, plus 16
/// per HnswCandidate (an element pointer, a float4 distance, a bool closer).
pub const fn neighbor_array_size(lm: i32) -> usize {
    8 + 16 * lm as usize
}

/// HnswGetLayerM (hnsw.h:127).
pub const fn layer_m(m: i32, layer: i32) -> i32 {
    if layer == 0 {
        m * 2
    } else {
        m
    }
}

/// MAXALIGN(sizeof(GenerationContext)): MemoryContextData (80,
/// memnodes.h:117-134), four uint32s, two block pointers and a dlist_head
/// (generation.c:59-72).
const GENERATION_CONTEXT_SIZE: usize = 128;
/// Generation_BLOCKHDRSZ = MAXALIGN(sizeof(GenerationBlock)) (generation.c:46, 87-96).
const GENERATION_BLOCKHDRSZ: usize = 56;
/// Generation_CHUNKHDRSZ = sizeof(MemoryChunk) (generation.c:47).
const GENERATION_CHUNKHDRSZ: usize = 8;
/// generation.c:49.
const GENERATION_CHUNK_FRACTION: usize = 8;
/// memutils_memorychunk.h:96.
const MEMORYCHUNK_MAX_VALUE: usize = 0x3FFF_FFFF;

/// generation.c's block arithmetic, as MemoryContextMemAllocated sees it.
/// Without MEMORY_CONTEXT_CHECKING (the reference and Docker builds have no
/// USE_ASSERT_CHECKING), a chunk is MAXALIGN(size) plus its header. A build
/// frees nothing, so generation.c's freeblock reuse (generation.c:571-587)
/// never applies.
#[derive(Clone, Debug)]
pub struct GenerationModel {
    mem_allocated: usize,
    free: usize,
    next_block_size: usize,
    max_block_size: usize,
    alloc_chunk_limit: usize,
}

impl GenerationModel {
    /// GenerationContextCreate (generation.c:160-271).
    pub fn new(min_context_size: usize, init_block_size: usize, max_block_size: usize) -> GenerationModel {
        // Determine size of initial block
        let mut alloc_size = GENERATION_CONTEXT_SIZE + GENERATION_BLOCKHDRSZ + GENERATION_CHUNKHDRSZ;
        alloc_size = if min_context_size != 0 {
            alloc_size.max(min_context_size)
        } else {
            alloc_size.max(init_block_size)
        };
        // The keeper block follows the context header in the same malloc.
        let first_block_size = alloc_size - GENERATION_CONTEXT_SIZE;
        let mut alloc_chunk_limit = max_block_size.min(MEMORYCHUNK_MAX_VALUE);
        while alloc_chunk_limit + GENERATION_CHUNKHDRSZ
            > (max_block_size - GENERATION_BLOCKHDRSZ) / GENERATION_CHUNK_FRACTION
        {
            alloc_chunk_limit >>= 1;
        }
        GenerationModel {
            mem_allocated: first_block_size,
            free: first_block_size - GENERATION_BLOCKHDRSZ,
            next_block_size: init_block_size,
            max_block_size,
            alloc_chunk_limit,
        }
    }

    /// MemoryContextMemAllocated(context, false).
    pub fn mem_allocated(&self) -> usize {
        self.mem_allocated
    }

    /// GenerationAlloc (generation.c:527-600).
    pub fn alloc(&mut self, size: usize) {
        let chunk_size = maxalign(size);
        if chunk_size > self.alloc_chunk_limit {
            // GenerationAllocLarge (generation.c:343-372): a dedicated block.
            self.mem_allocated += chunk_size + GENERATION_CHUNKHDRSZ + GENERATION_BLOCKHDRSZ;
            return;
        }
        let required_size = chunk_size + GENERATION_CHUNKHDRSZ;
        if self.free < required_size {
            // GenerationAllocFromNewBlock (generation.c:461-500): the first
            // block has initBlockSize, doubling up to maxBlockSize, rounded
            // up to a power of 2 if the chunk needs more.
            let mut blksize = self.next_block_size;
            self.next_block_size = (self.next_block_size << 1).min(self.max_block_size);
            let need = chunk_size + GENERATION_CHUNKHDRSZ + GENERATION_BLOCKHDRSZ;
            if blksize < need {
                blksize = need.next_power_of_two();
            }
            self.mem_allocated += blksize;
            self.free = blksize - GENERATION_BLOCKHDRSZ;
        }
        self.free -= required_size;
    }
}

/// HnswSharedMemoryAlloc's errors (hnswbuild.c:671-676). The 1 MB margin
/// keeps them unreachable for any value HNSW can index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphError {
    /// "hnsw allocation too large"
    AllocationTooLarge,
    /// "hnsw allocator out of memory"
    OutOfMemory,
}

#[derive(Clone, Debug)]
enum Mode {
    Serial(GenerationModel),
    Parallel,
}

/// The graph's memoryUsed and memoryTotal (hnsw.h:245-247) and the allocator
/// behind them.
#[derive(Clone, Debug)]
pub struct Accounting {
    mode: Mode,
    used: usize,
    total: usize,
}

/// In a parallel build, add a margin so allocations never fail (hnswbuild.c:509-510).
const PARALLEL_MEMORY_MARGIN: usize = 1024 * 1024;
/// HnswSharedMemoryAlloc's largest chunk (hnswbuild.c:671).
const MAX_SHARED_ALLOCATION: usize = 1024 * 1024;

impl Accounting {
    /// A serial build's graph: InitGraph(&graphData, NULL,
    /// maintenance_work_mem * 1024) (hnswbuild.c:727) over the 1 MB-block
    /// Generation context (hnswbuild.c:732-737).
    pub fn serial(memory_total: usize) -> Accounting {
        Accounting {
            mode: Mode::Serial(GenerationModel::new(1024 * 1024, 1024 * 1024, 1024 * 1024)),
            used: 0,
            total: memory_total.min(HNSW_MAX_GRAPH_MEMORY),
        }
    }

    /// A parallel build's graph: InitGraph(&hnswshared->graphData, hnswarea,
    /// esthnswarea) (hnswbuild.c:1012-1013).
    pub fn parallel(area_size: usize) -> Accounting {
        Accounting { mode: Mode::Parallel, used: 0, total: area_size.min(HNSW_MAX_GRAPH_MEMORY) }
    }

    /// esthnswarea (hnswbuild.c:961-969): leave space for other objects in
    /// shared memory; maintenance_work_mem is in kB.
    pub fn parallel_area_size(maintenance_work_mem: i32) -> usize {
        let mut esthnswarea = (maintenance_work_mem.max(0) as usize).saturating_mul(1024);
        let estother = 3 * 1024 * 1024;
        if esthnswarea > estother {
            esthnswarea -= estother;
        }
        esthnswarea.min(HNSW_MAX_GRAPH_MEMORY)
    }

    /// memoryUsed.
    pub fn used(&self) -> usize {
        self.used
    }

    /// memoryTotal.
    pub fn total(&self) -> usize {
        self.total
    }

    /// InsertTuple's flush test, `add_size(memoryUsed, memoryMargin) >=
    /// memoryTotal` (hnswbuild.c:533); the margin is 0 in a serial build.
    pub fn is_full(&self) -> bool {
        let margin = match self.mode {
            Mode::Serial(_) => 0,
            Mode::Parallel => PARALLEL_MEMORY_MARGIN,
        };
        self.used.saturating_add(margin) >= self.total
    }

    /// One allocation from the graph's allocator.
    fn alloc(&mut self, size: usize) -> Result<(), GraphError> {
        match &mut self.mode {
            // HnswMemoryContextAlloc: memoryUsed is the context's block total.
            Mode::Serial(generation) => {
                generation.alloc(size);
                self.used = generation.mem_allocated();
                Ok(())
            }
            // HnswSharedMemoryAlloc: a bump of MAXALIGN(size).
            Mode::Parallel => {
                let aligned_size = maxalign(size);
                if aligned_size > MAX_SHARED_ALLOCATION {
                    return Err(GraphError::AllocationTooLarge);
                }
                let new_memory_used = self.used + aligned_size;
                if new_memory_used > self.total {
                    return Err(GraphError::OutOfMemory);
                }
                self.used = new_memory_used;
                Ok(())
            }
        }
    }

    /// HnswInitElement's allocations and the value's (hnswutils.c:205-270,
    /// hnswbuild.c:556-557), in C's order: the element, its neighbor-array
    /// pointers, one neighbor array per layer from 0 to `level`, the value.
    pub fn charge_element(&mut self, level: u8, m: i32, value_len: usize) -> Result<(), GraphError> {
        self.alloc(ELEMENT_DATA_SIZE)?;
        self.alloc(NEIGHBOR_ARRAY_PTR_SIZE * (level as usize + 1))?;
        for lc in 0..=level as i32 {
            self.alloc(neighbor_array_size(layer_m(m, lc)))?;
        }
        self.alloc(value_len)
    }
}
```

- [ ] **Step 4: `lib.rs` and `algo.rs`**

`src/lib.rs` (replacing the skeleton):

```rust
//! pgvector 0.8.7's in-memory HNSW build graph: hnswbuild.c's InsertTuple (its
//! in-memory phase), InsertTupleInMemory and UpdateGraphInMemory, and the
//! hnswutils.c search they call with index == NULL. The serial build and every
//! participant of a parallel build insert through one Graph.
//!
//! C keeps the graph in a Generation context (serial) or one DSM area
//! (parallel), addressed by relptr and guarded by an LWLock per element plus
//! entryLock, entryWaitLock, allocatorLock, flushLock and the graph->lock
//! spinlock (hnsw.h:182-252). Here elements live in an append-only arena
//! addressed by u32 handles (relptr's role), and the locks are pgsync's, so
//! tests/loom.rs explores this code's lock protocol. The lock order is C's
//! (hnswbuild.c:437-580):
//!   flush_lock (read) -> allocator (released before the next step)
//!   flush_lock (read) -> entry_wait_lock (transient) -> entry_lock (read or
//!   write) -> { one element's links at a time | list }
//! An overflowing inserter releases both, then takes flush_lock (write)
//! holding nothing else. Every guard is released on the error path too (C
//! needs LWLockReleaseAll for that).
//!
//! DIVERGENCE: C's flush point counts C's allocations. This graph stores Rust
//! values and charges C's sizes to a model of C's allocator (memory.rs), so
//! the flush point is C's while the bytes actually held differ; and the arena
//! is freed with the graph, not when the graph is flushed (C resets graphCtx
//! in a serial build, hnswbuild.c:315).

mod algo;
mod arena;
pub mod memory;
#[cfg(test)]
mod tests;

use pgsync::{Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub use memory::{Accounting, GenerationModel, GraphError};

/// HNSW_HEAPTIDS (hnsw.h:69).
pub const HNSW_HEAPTIDS: usize = 10;

/// A heap TID as its 6-byte ItemPointerData image.
pub type HeapTid = [u8; 6];

/// HnswCandidate (hnsw.h:206-211), with an element handle for the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub element: u32,
    pub distance: f32,
    pub closer: bool,
}

/// HnswNeighborArray (hnsw.h:213-218).
#[derive(Clone, Debug, Default)]
pub struct NeighborArray {
    pub items: Vec<Candidate>,
    pub closer_set: bool,
}

/// What C's element LWLock guards: "It must be held when reading or modifying
/// the element's neighbors or 'heaptids'" (hnswbuild.c:20-21).
/// `neighbors[level - lc]` holds layer lc.
pub struct Links {
    pub heaptids: [HeapTid; HNSW_HEAPTIDS],
    pub heaptids_len: u8,
    pub neighbors: Vec<NeighborArray>,
}

/// HnswElementData (hnsw.h:182-198). The level and the value never change
/// once the element is allocated, so they are read without its lock, as in C.
pub struct Element {
    pub level: u8,
    /// The value's varlena image in 8-byte words, so a distance function can
    /// read its header in place.
    value: Box<[u64]>,
    value_len: usize,
    links: RwLock<Links>,
}

impl Element {
    fn new(level: u8, value: &[u8], links: Links) -> Element {
        let mut words = vec![0u64; value.len().div_ceil(8)].into_boxed_slice();
        // SAFETY: `words` spans at least value.len() bytes, and u8 has no
        // alignment or validity requirement.
        unsafe { core::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), value.len()) }
            .copy_from_slice(value);
        Element { level, value: words, value_len: value.len(), links: RwLock::new(links) }
    }

    /// HnswGetValue: the value's image.
    pub fn value(&self) -> &[u8] {
        // SAFETY: `value` holds value_len initialized bytes (Element::new).
        unsafe { core::slice::from_raw_parts(self.value.as_ptr().cast::<u8>(), self.value_len) }
    }

    /// The element's links under its lock in shared mode.
    pub fn links(&self) -> RwLockReadGuard<'_, Links> {
        read(&self.links)
    }

    fn links_mut(&self) -> RwLockWriteGuard<'_, Links> {
        write(&self.links)
    }
}

/// The build's m and efConstruction.
pub struct InsertParams {
    pub m: i32,
    pub ef_construction: i32,
}

/// Where Graph::insert put the tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inserted {
    InMemory,
    /// The graph is flushed: the caller inserts the tuple on disk
    /// (HnswInsertTupleOnDisk(.., building = true)).
    OnDisk,
}

#[derive(Debug)]
pub enum InsertError<E> {
    /// A callback's error: the distance function's or on_overflow's.
    Callback(E),
    Graph(GraphError),
}

struct Allocator {
    accounting: Accounting,
    /// Elements allocated so far; the next element's handle.
    len: u32,
}

struct List {
    /// graph->head, oldest first (C prepends; head_newest_first walks it C's way).
    head: Vec<u32>,
    indtuples: f64,
}

/// HnswGraph (hnsw.h:232-252).
pub struct Graph {
    elements: arena::Arena<Element>,
    /// graph->lock (a spinlock in C): head and indtuples.
    list: Mutex<List>,
    /// entryLock, guarding entryPoint.
    entry_lock: RwLock<Option<u32>>,
    /// entryWaitLock.
    entry_wait_lock: Mutex<()>,
    /// allocatorLock, guarding memoryUsed and the element count.
    allocator: Mutex<Allocator>,
    /// flushLock, guarding flushed.
    flush_lock: RwLock<bool>,
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

impl Graph {
    /// InitGraph (hnswbuild.c:618-634).
    pub fn new(accounting: Accounting) -> Graph {
        Graph {
            elements: arena::Arena::new(),
            list: Mutex::new(List { head: Vec::new(), indtuples: 0.0 }),
            entry_lock: RwLock::new(None),
            entry_wait_lock: Mutex::new(()),
            allocator: Mutex::new(Allocator { accounting, len: 0 }),
            flush_lock: RwLock::new(false),
        }
    }

    pub fn element(&self, id: u32) -> &Element {
        self.elements.get(id)
    }

    /// Elements allocated so far, including copies merged into a duplicate.
    pub fn element_count(&self) -> u32 {
        pgsync::lock(&self.allocator).len
    }

    /// graph->head as CreateGraphPages walks it (hnswbuild.c:177-186): newest first.
    pub fn head_newest_first(&self) -> Vec<u32> {
        pgsync::lock(&self.list).head.iter().rev().copied().collect()
    }

    pub fn entry_point(&self) -> Option<u32> {
        *read(&self.entry_lock)
    }

    pub fn indtuples(&self) -> f64 {
        pgsync::lock(&self.list).indtuples
    }

    /// `++graph->indtuples` under graph->lock (BuildCallback,
    /// hnswbuild.c:603-607); the new count.
    pub fn add_indtuple(&self) -> f64 {
        let mut list = pgsync::lock(&self.list);
        list.indtuples += 1.0;
        list.indtuples
    }

    pub fn memory_used(&self) -> usize {
        pgsync::lock(&self.allocator).accounting.used()
    }

    pub fn flushed(&self) -> bool {
        *read(&self.flush_lock)
    }

    /// InsertTuple's in-memory phase (hnswbuild.c:509-577), for a formed and
    /// dimension-checked value. `draw_level` is HnswInitElement's level draw,
    /// called only when the element is allocated. `distance` is the support
    /// function on two values. `on_overflow` runs at most once per graph, with
    /// flush_lock held exclusively, when the graph no longer fits (C's NOTICE
    /// and FlushPages); it must not call insert, flush_if_needed or flushed.
    pub fn insert<E>(
        &self,
        value: &[u8],
        heaptid: HeapTid,
        params: &InsertParams,
        draw_level: impl FnOnce() -> u8,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
        on_overflow: impl FnOnce(&Graph) -> Result<(), E>,
    ) -> Result<Inserted, InsertError<E>> {
        // Ensure graph not flushed when inserting
        let flushed = read(&self.flush_lock);

        // Are we in the on-disk phase?
        if *flushed {
            return Ok(Inserted::OnDisk);
        }

        // In a parallel build, the HnswElement is allocated from the shared
        // memory area, so we need to coordinate with other processes.
        let mut allocator = pgsync::lock(&self.allocator);

        // Check that we have enough memory available for the new element now
        // that we have the allocator lock, and flush pages if needed.
        if allocator.accounting.is_full() {
            drop(allocator);
            drop(flushed);
            let mut flushed = write(&self.flush_lock);
            if !*flushed {
                on_overflow(self).map_err(InsertError::Callback)?;
                *flushed = true;
            }
            return Ok(Inserted::OnDisk);
        }

        // Ok, we can proceed to allocate the element
        let level = draw_level();
        allocator.accounting.charge_element(level, params.m, value.len()).map_err(InsertError::Graph)?;
        let id = allocator.len;
        allocator.len += 1;
        let mut heaptids = [[0u8; 6]; HNSW_HEAPTIDS];
        heaptids[0] = heaptid;
        let neighbors = (0..=level).map(|_| NeighborArray::default()).collect();
        self.elements.publish(id, Element::new(level, value, Links { heaptids, heaptids_len: 1, neighbors }));

        // We have now allocated the space needed for the element, so we don't
        // need the allocator lock anymore.
        drop(allocator);

        // Insert tuple
        self.insert_in_memory(id, params, distance).map_err(InsertError::Callback)?;

        // Release flush lock
        drop(flushed);
        Ok(Inserted::InMemory)
    }

    /// BuildGraph's `if (!graph->flushed) FlushPages(buildstate)`
    /// (hnswbuild.c:1119-1120), once every participant has finished.
    pub fn flush_if_needed<E>(&self, flush: impl FnOnce(&Graph) -> Result<(), E>) -> Result<(), E> {
        let mut flushed = write(&self.flush_lock);
        if !*flushed {
            flush(self)?;
            *flushed = true;
        }
        Ok(())
    }

    /// An element allocated but not linked, for the algorithm's unit tests.
    #[cfg(test)]
    pub(crate) fn push_unlinked(&self, level: u8, value: &[u8]) -> u32 {
        let mut allocator = pgsync::lock(&self.allocator);
        let id = allocator.len;
        allocator.len += 1;
        let neighbors = (0..=level).map(|_| NeighborArray::default()).collect();
        self.elements.publish(
            id,
            Element::new(level, value, Links { heaptids: [[0u8; 6]; HNSW_HEAPTIDS], heaptids_len: 0, neighbors }),
        );
        id
    }
}
```

`src/algo.rs`:

```rust
//! The in-memory search and graph update: hnswutils.c's HnswSearchLayer,
//! SelectNeighbors, CheckElementCloser, HnswUpdateConnection and
//! HnswFindElementNeighbors with index == NULL, and hnswbuild.c's
//! InsertTupleInMemory, UpdateGraphInMemory, FindDuplicateInMemory and
//! UpdateNeighborsInMemory. Moved from pgvector_hnsw_build (M1), with C's
//! locks added where C takes them.

use std::collections::HashSet;

use pairingheap::PairingHeap;
use pgsync::{RwLockReadGuard, RwLockWriteGuard};

use crate::memory::layer_m;
use crate::{read, write, Candidate, Graph, InsertParams, NeighborArray, HNSW_HEAPTIDS};

type CandHeap = PairingHeap<(f64, u32), fn(&(f64, u32), &(f64, u32)) -> i32>;

// CompareNearestCandidates (hnswutils.c:632-641): the nearest candidate first.
fn compare_nearest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        1
    } else if a.0 > b.0 {
        -1
    } else {
        0
    }
}

// CompareFurthestCandidates (hnswutils.c:662-671): the furthest candidate first.
fn compare_furthest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        -1
    } else if a.0 > b.0 {
        1
    } else {
        0
    }
}

fn nearest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_nearest_candidates)
}

fn furthest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_furthest_candidates)
}

/// entryLock, held in shared or exclusive mode.
enum EntryGuard<'g> {
    Shared(RwLockReadGuard<'g, Option<u32>>),
    Exclusive(RwLockWriteGuard<'g, Option<u32>>),
}

impl EntryGuard<'_> {
    fn entry_point(&self) -> Option<u32> {
        match self {
            EntryGuard::Shared(g) => **g,
            EntryGuard::Exclusive(g) => **g,
        }
    }
}

impl Graph {
    // InsertTupleInMemory (hnswbuild.c:437-480) and UpdateGraphInMemory
    // (hnswbuild.c:413-432).
    pub(crate) fn insert_in_memory<E>(
        &self,
        element: u32,
        p: &InsertParams,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let level = self.element(element).level;

        // Wait if another process needs exclusive lock on entry lock
        drop(pgsync::lock(&self.entry_wait_lock));

        // Get entry point
        let mut guard = EntryGuard::Shared(read(&self.entry_lock));
        let mut entry_point = guard.entry_point();

        // Prevent concurrent inserts when likely updating entry point
        if entry_point.map_or(true, |ep| level > self.element(ep).level) {
            // Release shared lock
            drop(guard);
            // Tell other processes to wait and get exclusive lock
            let wait = pgsync::lock(&self.entry_wait_lock);
            guard = EntryGuard::Exclusive(write(&self.entry_lock));
            drop(wait);
            // Get latest entry point after lock is acquired
            entry_point = guard.entry_point();
        }

        // Find neighbors for element
        self.find_element_neighbors(element, entry_point, p, distance)?;

        // Look for duplicate
        if self.find_duplicate(element) {
            return Ok(());
        }

        // Add element (AddElementInMemory, hnswbuild.c:369-376)
        pgsync::lock(&self.list).head.push(element);

        // Update neighbors
        self.update_neighbors(element, p.m, distance)?;

        // Update entry point if needed (already have lock)
        if entry_point.map_or(true, |ep| level > self.element(ep).level) {
            match &mut guard {
                EntryGuard::Exclusive(g) => **g = Some(element),
                // The entry point only rises under the exclusive lock, so a
                // shared holder never sees an element above it.
                EntryGuard::Shared(_) => unreachable!("entry point raised under a shared entry lock"),
            }
        }

        // Release entry lock
        drop(guard);
        Ok(())
    }

    // HnswFindElementNeighbors (hnswutils.c:1316-1364), in memory.
    fn find_element_neighbors<E>(
        &self,
        element: u32,
        entry_point: Option<u32>,
        p: &InsertParams,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let e = self.element(element);
        let q = e.value();
        let Some(entry_point) = entry_point else { return Ok(()) };

        let entry_level = self.element(entry_point).level as i32;
        let ep_dist = distance(q, self.element(entry_point).value())?;
        let mut ep: Vec<(u32, f64)> = vec![(entry_point, ep_dist)];

        let mut lc = entry_level;
        while lc >= e.level as i32 + 1 {
            ep = self.search_layer(q, ep, 1, lc, distance)?;
            lc -= 1;
        }

        let level = (e.level as i32).min(entry_level);
        let mut lc = level;
        loop {
            let lm = layer_m(p.m, lc);
            let w = self.search_layer(q, ep.clone(), p.ef_construction, lc, distance)?;

            let mut lw: Vec<Candidate> =
                w.iter().map(|&(id, d)| Candidate { element: id, distance: d as f32, closer: false }).collect();

            let layer_idx = (e.level as i32 - lc) as usize;
            // The new element is not reachable yet, so no other participant
            // reads its links (C writes them unlocked, hnswutils.c:1173-1181);
            // its own lock is uncontended.
            let mut closer_set = e.links().neighbors[layer_idx].closer_set;
            let mut selected: Vec<Candidate> = Vec::new();
            self.select_neighbors(&mut lw, lm, &mut closer_set, None, None, false, &mut selected, distance)?;
            e.links_mut().neighbors[layer_idx] = NeighborArray { items: selected, closer_set };

            ep = w;
            if lc == 0 {
                break;
            }
            lc -= 1;
        }
        Ok(())
    }

    // HnswSearchLayer (hnswutils.c:828-990) with index == NULL. The
    // candidate's neighborhood is copied under its lock in shared mode
    // (HnswLoadUnvisitedFromMemory, hnswutils.c:737-759).
    fn search_layer<E>(
        &self,
        q: &[u8],
        ep: Vec<(u32, f64)>,
        ef: i32,
        lc: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<Vec<(u32, f64)>, E> {
        let mut visited: HashSet<u32> = HashSet::new();
        let mut c_heap = nearest_candidate_heap();
        let mut w_heap = furthest_candidate_heap();
        let mut wlen: i32 = 0;

        for &(e, d) in &ep {
            visited.insert(e);
            c_heap.add((d, e));
            w_heap.add((d, e));
            wlen += 1;
        }

        while let Some((c_dist, c_elem)) = c_heap.remove_first() {
            let (f_dist, _) = *w_heap.first().expect("W nonempty");
            if c_dist > f_dist {
                break;
            }
            let c = self.element(c_elem);
            let neighbor_ids: Vec<u32> = {
                let links = c.links();
                links.neighbors[(c.level as i32 - lc) as usize].items.iter().map(|hc| hc.element).collect()
            };

            for e in neighbor_ids {
                if !visited.insert(e) {
                    continue;
                }
                let always_add = wlen < ef;
                let (f_dist, _) = *w_heap.first().expect("W nonempty");
                let e_distance = distance(q, self.element(e).value())?;
                if !(e_distance < f_dist || always_add) {
                    continue;
                }
                // Make robust to issues
                if (self.element(e).level as i32) < lc {
                    continue;
                }
                c_heap.add((e_distance, e));
                w_heap.add((e_distance, e));
                wlen += 1;
                if wlen > ef {
                    w_heap.remove_first();
                }
            }
        }

        let mut w: Vec<(u32, f64)> = Vec::new();
        while let Some((d, e)) = w_heap.remove_first() {
            w.push((e, d));
        }
        Ok(w)
    }

    // CheckElementCloser (hnswutils.c:1043-1062): reads only values.
    fn check_element_closer<E>(
        &self,
        e: &Candidate,
        r: &[Candidate],
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<bool, E> {
        let e_value = self.element(e.element).value();
        for ri in r {
            let d = distance(e_value, self.element(ri.element).value())? as f32;
            if d <= e.distance {
                return Ok(false);
            }
        }
        Ok(true)
    }

    // SelectNeighbors (hnswutils.c:1065-1158). C's candidate list holds
    // pointers into the caller's neighbor array, so e->closer updates land in
    // that array; `c` is taken mutably and the computed closer flags are
    // written back by origin index. Tie order: equal distances break by element
    // handle (hnswutils.c:996-1038): C's pointer order within the serial
    // build's Generation context, C's relptr order in a parallel build's area;
    // handles rise with allocation order in both.
    #[allow(clippy::too_many_arguments)]
    fn select_neighbors<E>(
        &self,
        c: &mut [Candidate],
        lm: i32,
        closer_set: &mut bool,
        new_candidate: Option<usize>,
        pruned: Option<&mut Option<Candidate>>,
        sort_candidates: bool,
        out: &mut Vec<Candidate>,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        out.clear();
        if c.len() as i32 <= lm {
            out.extend_from_slice(c);
            return Ok(());
        }

        let mut w: Vec<Candidate> = c.to_vec();
        let mut w_is_new: Vec<bool> = vec![false; w.len()];
        // Origin index into `c` for closer-flag write-back.
        let mut w_src: Vec<usize> = (0..w.len()).collect();
        if let Some(nc) = new_candidate {
            w_is_new[nc] = true;
        }
        if sort_candidates {
            let mut order: Vec<usize> = (0..w.len()).collect();
            order.sort_by(|&a, &b| {
                w[b].distance
                    .partial_cmp(&w[a].distance)
                    .unwrap_or(core::cmp::Ordering::Equal)
                    .then_with(|| w[b].element.cmp(&w[a].element))
            });
            let neww: Vec<Candidate> = order.iter().map(|&i| w[i]).collect();
            let newn: Vec<bool> = order.iter().map(|&i| w_is_new[i]).collect();
            let news: Vec<usize> = order.iter().map(|&i| w_src[i]).collect();
            w = neww;
            w_is_new = newn;
            w_src = news;
        }

        let must_calculate = !*closer_set;
        let mut wd: Vec<Candidate> = Vec::with_capacity(w.len());
        let mut added: Vec<Candidate> = Vec::new();
        let mut removed_any = false;

        while !w.is_empty() && (out.len() as i32) < lm {
            let mut e = w.pop().expect("nonempty");
            let e_is_new = w_is_new.pop().expect("nonempty");
            let e_src = w_src.pop().expect("nonempty");

            if must_calculate {
                e.closer = self.check_element_closer(&e, out, distance)?;
            } else if !added.is_empty() {
                if e.closer {
                    e.closer = self.check_element_closer(&e, &added, distance)?;
                    if !e.closer {
                        removed_any = true;
                    }
                } else if removed_any {
                    e.closer = self.check_element_closer(&e, out, distance)?;
                    if e.closer {
                        added.push(e);
                    }
                }
            } else if e_is_new {
                e.closer = self.check_element_closer(&e, out, distance)?;
                if e.closer {
                    added.push(e);
                }
            }

            // C writes e->closer through the shared pointer; mirror into `c`.
            c[e_src].closer = e.closer;

            if e.closer {
                out.push(e);
            } else {
                wd.push(e);
            }
        }

        *closer_set = sort_candidates;

        let mut wdoff = 0usize;
        while wdoff < wd.len() && (out.len() as i32) < lm {
            out.push(wd[wdoff]);
            wdoff += 1;
        }
        if let Some(p) = pruned {
            *p = if wdoff < wd.len() { Some(wd[wdoff]) } else { w.first().copied() };
        }
        Ok(())
    }

    // HnswUpdateConnection (hnswutils.c:1186-1234), in memory: link
    // `new_element` into `neighbors`, which the caller holds under the
    // neighbor's lock in exclusive mode. C's SelectNeighbors candidate list
    // aliases neighbors->items, so the closer flags it computes persist in the
    // array (and the replacement newHc carries its computed flag); both
    // write-backs are mirrored here.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_connection<E>(
        &self,
        neighbors: &mut Vec<Candidate>,
        closer_set: &mut bool,
        new_element: u32,
        new_distance: f32,
        lm: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let new_hc = Candidate { element: new_element, distance: new_distance, closer: false };
        if (neighbors.len() as i32) < lm {
            neighbors.push(new_hc);
            return Ok(());
        }

        // Shrink connections.
        let mut c: Vec<Candidate> = neighbors.clone();
        c.push(new_hc);
        let new_idx = c.len() - 1;
        let mut pruned: Option<Candidate> = None;
        let mut selected: Vec<Candidate> = Vec::new();
        self.select_neighbors(&mut c, lm, closer_set, Some(new_idx), Some(&mut pruned), true, &mut selected, distance)?;
        // Closer flags computed in place land in the neighbor array even on
        // the pruned == NULL early return (c[0..len] is index-aligned with
        // neighbors).
        for (slot, cand) in neighbors.iter_mut().zip(c.iter()) {
            slot.closer = cand.closer;
        }
        // Should not happen (C returns without linking).
        let Some(pruned) = pruned else { return Ok(()) };
        for slot in neighbors.iter_mut() {
            if slot.element == pruned.element {
                *slot = c[new_idx];
                break;
            }
        }
        Ok(())
    }

    // UpdateNeighborsInMemory (hnswbuild.c:381-408): one element lock at a
    // time; the distance function runs under the neighbor's exclusive lock, as
    // in C.
    fn update_neighbors<E>(
        &self,
        e: u32,
        m: i32,
        distance: &mut impl FnMut(&[u8], &[u8]) -> Result<f64, E>,
    ) -> Result<(), E> {
        let element = self.element(e);
        let level = element.level as i32;
        for lc in (0..=level).rev() {
            let lm = layer_m(m, lc);
            // Copy neighbors to local memory
            let items: Vec<Candidate> = element.links().neighbors[(level - lc) as usize].items.clone();
            for hc in items {
                let neighbor = self.element(hc.element);
                let mut links = neighbor.links_mut();
                let na = &mut links.neighbors[(neighbor.level as i32 - lc) as usize];
                self.update_connection(&mut na.items, &mut na.closer_set, e, hc.distance, lm, distance)?;
            }
        }
        Ok(())
    }

    // FindDuplicateInMemory and AddDuplicateInMemory (hnswbuild.c:321-364).
    fn find_duplicate(&self, element: u32) -> bool {
        let e = self.element(element);
        // The new element is private: its layer-0 neighbors and TID are read
        // before taking any other element's lock.
        let (neighbor_ids, tid) = {
            let links = e.links();
            let ids: Vec<u32> = links.neighbors[e.level as usize].items.iter().map(|hc| hc.element).collect();
            (ids, links.heaptids[0])
        };
        for dup in neighbor_ids {
            let d = self.element(dup);
            // Exit early since ordered by distance
            if e.value() != d.value() {
                return false;
            }
            // Check for space
            let mut links = d.links_mut();
            if (links.heaptids_len as usize) < HNSW_HEAPTIDS {
                let n = links.heaptids_len as usize;
                links.heaptids[n] = tid;
                links.heaptids_len += 1;
                return true;
            }
        }
        false
    }
}
```

- [ ] **Step 5: Run the unit tests**

Run: `cargo test -p pgvector_hnsw_graph --lib`
Expected: PASS, 16 tests.

Run: `cargo clippy -p pgvector_hnsw_graph --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 6: The loom models**

`tests/loom.rs`:

```rust
//! Loom models of the HNSW build graph's lock protocol (hnswbuild.c:437-580):
//! the real Graph::insert, its entry-point protocol, element locks,
//! duplicate merge and flush, explored under loom's interleavings.
//!
//! Run: RUSTFLAGS="--cfg loom" cargo test -p pgvector_hnsw_graph --test loom --release
//!
//! Values are one byte and the distance is their absolute difference. Handles
//! depend on allocation order, so the models find elements by value.
#![cfg(loom)]

use std::collections::HashSet;
use std::sync::Arc;

use pgvector_hnsw_graph::{Accounting, Graph, HeapTid, InsertParams, Inserted};

fn dist(a: &[u8], b: &[u8]) -> Result<f64, ()> {
    Ok((a[0] as f64 - b[0] as f64).abs())
}

fn params() -> InsertParams {
    InsertParams { m: 2, ef_construction: 4 }
}

fn tid(n: u8) -> HeapTid {
    [0, 0, 0, n, 0, 1]
}

fn insert(g: &Graph, x: u8, level: u8) -> Inserted {
    match g.insert(&[x], tid(x), &params(), || level, &mut dist, |_| Ok(())) {
        Ok(r) => r,
        Err(_) => panic!("insert of {x} failed"),
    }
}

fn by_value(g: &Graph, x: u8) -> u32 {
    (0..g.element_count()).find(|&id| g.element(id).value() == [x]).expect("an element with that value")
}

fn layer0(g: &Graph, id: u32) -> Vec<u32> {
    let e = g.element(id);
    let links = e.links();
    links.neighbors[e.level as usize].items.iter().map(|c| c.element).collect()
}

/// After concurrent inserts: the entry point has the top level, every element
/// on the head list is reachable from it, and no neighbor array names its own
/// element or one element twice.
fn assert_well_formed(g: &Graph) {
    let head = g.head_newest_first();
    let ep = g.entry_point().expect("an entry point");
    let top = head.iter().map(|&e| g.element(e).level).max().expect("elements");
    assert_eq!(g.element(ep).level, top);
    let mut seen = HashSet::new();
    let mut stack = vec![ep];
    while let Some(e) = stack.pop() {
        if !seen.insert(e) {
            continue;
        }
        let links = g.element(e).links();
        for na in &links.neighbors {
            let mut ids = HashSet::new();
            for c in &na.items {
                assert_ne!(c.element, e, "element {e} names itself");
                assert!(ids.insert(c.element), "element {e} names {} twice", c.element);
                stack.push(c.element);
            }
        }
    }
    for e in &head {
        assert!(seen.contains(e), "element {e} is unreachable");
    }
}

fn model(bound: usize, f: impl Fn() + Sync + Send + 'static) {
    let mut b = loom::model::Builder::new();
    b.preemption_bound = Some(bound);
    b.check(f);
}

// Two first inserts race for the empty graph's entry point: both take
// entryLock exclusively, one after the other, and link each other.
#[test]
fn concurrent_first_inserts_serialize_on_the_entry_lock() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 10, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 20, 1));
        assert_eq!(t1.join().unwrap(), Inserted::InMemory);
        assert_eq!(t2.join().unwrap(), Inserted::InMemory);
        assert_eq!(g.head_newest_first().len(), 2);
        assert_well_formed(&g);
        let (a, b) = (by_value(&g, 10), by_value(&g, 20));
        assert_eq!(layer0(&g, a), vec![b]);
        assert_eq!(layer0(&g, b), vec![a]);
    });
}

// Two level-0 inserts share entryLock and search from the same entry point
// at once: each may miss the other, but both link to it and it to both.
#[test]
fn concurrent_inserts_under_a_shared_entry_lock_stay_reachable() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 50, 1);
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 40, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 60, 0));
        t1.join().unwrap();
        t2.join().unwrap();
        assert_well_formed(&g);
        let mut l0 = layer0(&g, by_value(&g, 50));
        l0.sort_unstable();
        let mut want = vec![by_value(&g, 40), by_value(&g, 60)];
        want.sort_unstable();
        assert_eq!(l0, want);
    });
}

// Review Focus 4: two inserts of a value already in the graph both merge
// their heap TIDs into it under its lock; neither becomes an element.
#[test]
fn concurrent_duplicates_merge_into_one_element() {
    model(3, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 5, 0);
        let g1 = Arc::clone(&g);
        let t1 = loom::thread::spawn(move || insert(&g1, 5, 0));
        let g2 = Arc::clone(&g);
        let t2 = loom::thread::spawn(move || insert(&g2, 5, 0));
        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(g.head_newest_first(), vec![0]);
        assert_eq!(g.element(0).links().heaptids_len, 3);
    });
}

// One insert fits the graph and the other overflows it: the overflowing one
// flushes exactly once, only after the in-memory insert finished (flushLock),
// and goes to disk.
#[test]
fn a_flush_waits_for_the_insert_in_flight() {
    model(3, || {
        // C's serial accounting: the first element's allocation brings
        // memoryUsed to the 1 MB keeper block (1048448 bytes), so a graph of
        // that size is full after one element.
        let g = Arc::new(Graph::new(Accounting::serial((1 << 20) - 128)));
        let flushes = Arc::new(loom::sync::atomic::AtomicUsize::new(0));
        let run = |x: u8| {
            let g = Arc::clone(&g);
            let flushes = Arc::clone(&flushes);
            loom::thread::spawn(move || {
                g.insert(&[x], tid(x), &params(), || 0, &mut dist, |gr| {
                    // Under the exclusive flush lock: the in-memory insert completed.
                    assert_eq!(gr.head_newest_first().len(), 1);
                    assert!(gr.entry_point().is_some());
                    flushes.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
                .unwrap_or_else(|_| panic!("insert of {x} failed"))
            })
        };
        let (t1, t2) = (run(1), run(2));
        let mut results = vec![t1.join().unwrap(), t2.join().unwrap()];
        results.sort_by_key(|r| *r == Inserted::OnDisk);
        assert_eq!(results, vec![Inserted::InMemory, Inserted::OnDisk]);
        assert_eq!(flushes.load(loom::sync::atomic::Ordering::SeqCst), 1);
        assert!(g.flushed());
    });
}

// Three inserts over a one-element graph, one of them raising the entry
// point: shared and exclusive entry-lock paths interleave.
#[test]
fn three_inserts_with_an_entry_point_raise() {
    model(2, || {
        let g = Arc::new(Graph::new(Accounting::serial(64 << 20)));
        insert(&g, 30, 0);
        let handles: Vec<_> = [(20u8, 0u8), (40, 1), (35, 0)]
            .into_iter()
            .map(|(x, l)| {
                let g = Arc::clone(&g);
                loom::thread::spawn(move || insert(&g, x, l))
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap(), Inserted::InMemory);
        }
        assert_eq!(g.head_newest_first().len(), 4);
        assert_well_formed(&g);
        assert_eq!(g.entry_point(), Some(by_value(&g, 40)));
    });
}
```

Run: `RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`
Expected: PASS, 5 tests. Record the wall time in the task report. If a model takes more than 15 minutes, lower its `preemption_bound` by one and record the change; never remove an assertion.

**Red check (do it, then revert it):** in `insert_in_memory`, temporarily take the shared entry lock unconditionally (skip the exclusive branch). Expected: `concurrent_first_inserts_serialize_on_the_entry_lock` fails (two first inserts both see no entry point). Record the failure in the task report, then restore the code and rerun the loom tests green.

- [ ] **Step 7: The lints**

Run: `cargo test -p seams_init --test lint_determinism`
Expected: PASS, still 43 warnings (no new site: `pgsync` locks, no `std::sync` locks, no spawn or join outside `tests/`).

- [ ] **Step 8: Commit**

```bash
git add crates/contrib/pgvector_hnsw_graph Cargo.toml Cargo.lock
git commit -m "feat(pgvector): pgvector_hnsw_graph, the shared HNSW build graph with C's lock protocol, C's memory arithmetic and loom models

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 5: The serial HNSW build on the graph crate, with C's flush point

**Files:**
- Modify (rewrite): `crates/contrib/pgvector_hnsw_build/src/lib.rs`
- Modify: `crates/contrib/pgvector_hnsw_build/Cargo.toml` (+`pgvector_hnsw_graph`, `backend_progress`), `Cargo.lock`
- Modify: `crates/_support/types/types_hnsw/src/lib.rs` (`PROGRESS_HNSW_PHASE_LOAD`)
- Modify: `scripts/pgvector/run-bytecmp.sh` (`spill`'s data; its promotion if it passes)

**Interfaces:**
- Consumes: `pgvector_hnsw_graph::{Accounting, Element, Graph, GraphError, InsertError, InsertParams, Inserted}` (Task 4).
- Produces, crate-private for Task 6: `struct BuildState<'a, 'mcx>` (with `graph: &'a Graph`), `fn init_build_state<'a, 'mcx>(heap: Option<&'a Relation<'mcx>>, index: &'a Relation<'mcx>, fork_num: ForkNumber, graph: &'a Graph) -> PgResult<BuildState<'a, 'mcx>>`, `fn build_callback(bs: &mut BuildState<'_, '_>, values: &[Datum], isnull: &[bool], tid: &ItemPointerData) -> PgResult<()>`, `fn flush_pages(bs: &BuildState<'_, '_>, graph: &Graph) -> PgResult<()>`, `fn serial_scan<'mcx>(bs: &mut BuildState<'_, 'mcx>, mcx: Mcx<'mcx>, heap: &Relation<'mcx>, index_info: &mut IndexInfo<'mcx>) -> PgResult<f64>`.
- Produces: `types_hnsw::PROGRESS_HNSW_PHASE_LOAD: i64 = 2`.

The page-writing code (`create_meta_page`, `build_append_page`, `page_add`, the tuple layouts) is unchanged; what changes is where the elements come from. Byte identity of every serial build is this task's gate.

- [ ] **Step 1: Write the failing test**

Add `pgvector_hnsw_graph = { path = "../pgvector_hnsw_graph" }` and `backend_progress = { path = "../../backend/utils/activity/backend_progress" }` to `[dependencies]` in `crates/contrib/pgvector_hnsw_build/Cargo.toml`. In `lib.rs`'s `mod tests`, add (the two `update_connection_*` tests and their helpers `support` and `graph_1d` move to the graph crate in Step 3; leave them until then):

```rust
    // C's seeded build (-DHNSW_MEMORY's SeedRandom(42), hnswbuild.c:1134-1136)
    // of bytecmp's `spill` rows (vector(3), 20-byte values, m = 16) at
    // maintenance_work_mem = 1MB prints "hnsw graph no longer fits into
    // maintenance_work_mem after 1436 tuples". The serial accounting must
    // flush at the same tuple, testing before each tuple's level draw as C
    // does (hnswbuild.c:533, 552-556).
    #[test]
    fn serial_flush_point_matches_c_at_one_megabyte() {
        seed_random(42);
        let m = 16;
        let (ml, max_level) = (hnsw_get_ml(m), hnsw_get_max_level(m));
        let mut accounting = pgvector_hnsw_graph::Accounting::serial(1024 * 1024);
        let mut tuples = 0;
        while !accounting.is_full() {
            accounting.charge_element(random_level(ml, max_level), m, 20).unwrap();
            tuples += 1;
        }
        assert_eq!(tuples, 1436);
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p pgvector_hnsw_build --lib serial_flush_point_matches_c_at_one_megabyte`
Expected: PASS: the model was verified against this count while planning (Decision 3). It pins the serial accounting before the build starts using it. If it fails, stop: the model and C disagree, and nothing later in this task can be trusted.

- [ ] **Step 3: Rewrite `lib.rs` onto the graph crate**

Add to `crates/_support/types/types_hnsw/src/lib.rs`, after the `HNSW_UPDATE_ENTRY_*` constants:

```rust
/// PROGRESS_HNSW_PHASE_LOAD (hnsw.h:76): BuildGraph's subphase (hnswbuild.c:1096).
pub const PROGRESS_HNSW_PHASE_LOAD: i64 = 2;
```

Replace `crates/contrib/pgvector_hnsw_build/src/lib.rs` with:

```rust
//! pgvector 0.8.7 hnswbuild.c. The in-memory graph phase runs on
//! pgvector_hnsw_graph (InsertTuple and InsertTupleInMemory, with C's lock
//! protocol and C's memory accounting); the graph is flushed to disk when it
//! no longer fits maintenance_work_mem, and later tuples are inserted on
//! disk. parallel_build.rs runs the same insert path in launched workers.

use bufmgr::{LockBuffer, MarkBufferDirty, UnlockReleaseBuffer, BUFFER_LOCK_EXCLUSIVE, BUFFER_LOCK_UNLOCK};
use datum::Datum;
use execindexing::IndexInfo;
use mcx::Mcx;
use pgvector_hnsw::insert::{form_index_value, insert_tuple_on_disk, random_level, seed_random};
use pgvector_hnsw::layout::*;
use pgvector_hnsw::utils::*;
use pgvector_hnsw_graph::{Accounting, Element, Graph, GraphError, InsertError, InsertParams, Inserted};
use types_core::{BlockNumber, Buffer, ForkNumber};
use types_error::{PgError, PgResult, ERRCODE_FEATURE_NOT_SUPPORTED, ERRCODE_INVALID_PARAMETER_VALUE, ERRCODE_PROGRAM_LIMIT_EXCEEDED, NOTICE};
use types_hnsw::*;
use types_rel::Relation;
use types_storage::bufpage::PageMut;
use types_tuple::itemptr::ItemPointerData;

pub struct IndexBuildResult {
    pub heap_tuples: f64,
    pub index_tuples: f64,
}

// HnswBuildState (hnsw.h:311-347): one participant's state. `graph` is the
// build's own (serial) or the shared one (parallel_build.rs).
struct BuildState<'a, 'mcx> {
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    fork_num: ForkNumber,
    m: i32,
    ef_construction: i32,
    dimensions: i32,
    type_info: &'static HnswTypeInfo,
    ml: f64,
    max_level: i32,
    support: HnswSupport,
    graph: &'a Graph,
    reltuples: f64,
}

// An element's on-disk location, assigned by CreateGraphPages.
#[derive(Clone, Copy)]
struct Placement {
    blkno: BlockNumber,
    offno: u16,
    neighbor_page: BlockNumber,
    neighbor_offno: u16,
}

const UNPLACED: Placement = Placement {
    blkno: INVALID_BLOCK,
    offno: INVALID_OFFSET,
    neighbor_page: INVALID_BLOCK,
    neighbor_offno: INVALID_OFFSET,
};

// ---- flush to disk ----
```

followed, unchanged, by the current `create_meta_page`, `build_append_page` and `page_add` (their signatures take `bs: &BuildState<'_, '_>` where they took `&BuildState<'_, '_, '_>`), and then:

```rust
fn serialize_element_tuple(buf: &mut Vec<u8>, e: &Element, p: Placement) {
    let value = e.value();
    let links = e.links();
    let size = element_tuple_size(value.len());
    buf.clear();
    buf.resize(size, 0);
    buf[0] = HNSW_ELEMENT_TUPLE_TYPE;
    buf[1] = e.level;
    buf[2] = 0;
    // Start at one to make it easier to find issues (HnswInitElement, hnswutils.c:262)
    buf[3] = 1;
    for i in 0..HNSW_HEAPTIDS {
        let b = if i < links.heaptids_len as usize {
            links.heaptids[i]
        } else {
            itemptr_encode(INVALID_BLOCK, INVALID_OFFSET)
        };
        buf[4 + i * 6..4 + i * 6 + 6].copy_from_slice(&b);
    }
    buf[4 + HNSW_HEAPTIDS * 6..4 + HNSW_HEAPTIDS * 6 + 6]
        .copy_from_slice(&itemptr_encode(p.neighbor_page, p.neighbor_offno));
    buf[ELEMENT_DATA_OFFSET..ELEMENT_DATA_OFFSET + value.len()].copy_from_slice(value);
}

fn serialize_neighbor_tuple(buf: &mut Vec<u8>, e: &Element, placements: &[Placement], m: i32) {
    let size = neighbor_tuple_size(e.level, m);
    buf.clear();
    buf.resize(size, 0);
    buf[0] = HNSW_NEIGHBOR_TUPLE_TYPE;
    // The element's version
    buf[1] = 1;
    let links = e.links();
    let mut idx = 0usize;
    for lc in (0..=e.level as i32).rev() {
        let na = &links.neighbors[(e.level as i32 - lc) as usize];
        let lm = hnsw_get_layer_m(m, lc);
        for i in 0..lm as usize {
            let b = if i < na.items.len() {
                let p = placements[na.items[i].element as usize];
                itemptr_encode(p.blkno, p.offno)
            } else {
                itemptr_encode(INVALID_BLOCK, INVALID_OFFSET)
            };
            buf[NEIGHBOR_TIDS_OFFSET + idx * 6..NEIGHBOR_TIDS_OFFSET + idx * 6 + 6].copy_from_slice(&b);
            idx += 1;
        }
    }
    buf[2..4].copy_from_slice(&(idx as u16).to_ne_bytes());
}

// CreateGraphPages (hnswbuild.c:150-248): graph->head, newest first
// (duplicates merged away are not on it).
fn create_graph_pages(bs: &BuildState<'_, '_>, graph: &Graph) -> PgResult<Vec<Placement>> {
    let max_size = HNSW_MAX_SIZE;
    let mut placements = vec![UNPLACED; graph.element_count() as usize];
    let mut etup: Vec<u8> = Vec::new();
    let mut ntup_placeholder: Vec<u8> = Vec::new();

    let mut buf = new_buffer(bs.index, bs.fork_num)?;
    init_page(buf);

    for id in graph.head_newest_first() {
        let e = graph.element(id);
        let etup_size = element_tuple_size(e.value().len());
        let ntup_size = neighbor_tuple_size(e.level, bs.m);
        let combined_size = etup_size + ntup_size + SIZE_OF_ITEM_ID;
        if etup_size > HNSW_TUPLE_ALLOC_SIZE {
            return Err(PgError::error("index tuple too large")
                .with_sqlstate(ERRCODE_PROGRAM_LIMIT_EXCEEDED)
                .into());
        }

        // SAFETY: exclusive lock held on buf.
        let free = unsafe {
            types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
        }
        .free_space();
        if free < etup_size || (combined_size <= max_size && free < combined_size) {
            build_append_page(bs.index, &mut buf, bs.fork_num)?;
        }

        let placement = {
            // SAFETY: lock held.
            let pr = unsafe {
                types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
            };
            let blkno = bufmgr::BufferGetBlockNumber(buf);
            let offno = pr.max_offset_number() + 1;
            if combined_size <= max_size {
                Placement { blkno, offno, neighbor_page: blkno, neighbor_offno: offno + 1 }
            } else {
                Placement { blkno, offno, neighbor_page: blkno + 1, neighbor_offno: 1 }
            }
        };
        placements[id as usize] = placement;

        serialize_element_tuple(&mut etup, e, placement);
        page_add(bs.index, buf, &etup, placement.offno)?;

        // SAFETY: lock held.
        let free = unsafe {
            types_storage::bufpage::PageRef::from_raw(bufmgr_seams::buffer_get_page::call(buf))
        }
        .free_space();
        if free < ntup_size {
            build_append_page(bs.index, &mut buf, bs.fork_num)?;
        }
        ntup_placeholder.clear();
        ntup_placeholder.resize(ntup_size, 0);
        ntup_placeholder[0] = HNSW_NEIGHBOR_TUPLE_TYPE;
        page_add(bs.index, buf, &ntup_placeholder, placement.neighbor_offno)?;
    }

    let insert_page = bufmgr::BufferGetBlockNumber(buf);
    MarkBufferDirty(buf)?;
    UnlockReleaseBuffer(buf)?;

    let entry = graph.entry_point().map(|ep| {
        let p = placements[ep as usize];
        (p.blkno, p.offno, graph.element(ep).level as i16)
    });
    update_meta_page(bs.index, HNSW_UPDATE_ENTRY_ALWAYS, entry, insert_page, bs.fork_num, true)?;
    Ok(placements)
}

// WriteNeighborTuples (hnswbuild.c:253-298).
fn write_neighbor_tuples(bs: &BuildState<'_, '_>, graph: &Graph, placements: &[Placement]) -> PgResult<()> {
    let mut ntup: Vec<u8> = Vec::new();
    // Same head-list walk as create_graph_pages: duplicates have no tuples.
    for id in graph.head_newest_first() {
        postgres_seams::check_for_interrupts::call()?;
        let e = graph.element(id);
        let p = placements[id as usize];
        let ntup_size = neighbor_tuple_size(e.level, bs.m);
        serialize_neighbor_tuple(&mut ntup, e, placements, bs.m);
        let buf = bufmgr::ReadBufferExtended(
            bs.index,
            bs.fork_num,
            p.neighbor_page,
            types_storage::storage::ReadBufferMode::Normal,
            None,
        )?;
        LockBuffer(buf, BUFFER_LOCK_EXCLUSIVE)?;
        // SAFETY: exclusive lock held.
        let mut pm = unsafe { PageMut::from_raw(bufmgr_seams::buffer_get_page::call(buf)) };
        if !pm.index_tuple_overwrite(p.neighbor_offno, &ntup[..ntup_size]) {
            return Err(PgError::error(format!("failed to add index item to \"{}\"", bs.index.name())).into());
        }
        MarkBufferDirty(buf)?;
        UnlockReleaseBuffer(buf)?;
    }
    Ok(())
}

// FlushPages (hnswbuild.c:303-316), called with the graph's flush lock held
// exclusively; the graph sets `flushed`.
fn flush_pages(bs: &BuildState<'_, '_>, graph: &Graph) -> PgResult<()> {
    create_meta_page(bs)?;
    let placements = create_graph_pages(bs, graph)?;
    write_neighbor_tuples(bs, graph, &placements)
}

// HnswSharedMemoryAlloc's errors (hnswbuild.c:671-676).
fn graph_error(e: GraphError) -> Box<PgError> {
    let (message, line) = match e {
        GraphError::AllocationTooLarge => ("hnsw allocation too large", 672),
        GraphError::OutOfMemory => ("hnsw allocator out of memory", 676),
    };
    PgError::error(message).with_location("hnswbuild.c", line, "HnswSharedMemoryAlloc").into()
}

// InsertTuple (hnswbuild.c:485-580): form and check the value, then the
// graph's in-memory insert, or the on-disk insert once the graph is flushed.
fn insert_tuple(
    bs: &mut BuildState<'_, '_>,
    values: &[Datum],
    isnull: &[bool],
    heaptid: &ItemPointerData,
) -> PgResult<bool> {
    if isnull.first().copied().unwrap_or(true) {
        return Ok(false);
    }
    let tmp = mcx::MemoryContext::new_bump("Hnsw build temporary context");
    let mut support = bs.support.clone();

    // Form index value
    let Some(img) = form_index_value(tmp.mcx(), values[0], bs.type_info, &mut support)? else {
        bs.support = support;
        return Ok(false);
    };

    // Check dimensions match index
    hnsw_check_dim(bs.dimensions, bs.type_info, support.collation, Datum::from_usize(img.as_ptr() as usize))?;

    let state: &BuildState<'_, '_> = bs;
    let params = InsertParams { m: state.m, ef_construction: state.ef_construction };
    let inserted = state.graph.insert(
        &img,
        ipd_to_bytes(heaptid),
        &params,
        || random_level(state.ml, state.max_level),
        &mut |a: &[u8], b: &[u8]| {
            get_distance(&mut support, Datum::from_usize(a.as_ptr() as usize), Datum::from_usize(b.as_ptr() as usize))
        },
        |graph| {
            elog::ereport(NOTICE)
                .errmsg(format!(
                    "hnsw graph no longer fits into maintenance_work_mem after {} tuples",
                    graph.indtuples() as i64
                ))
                .errdetail("Building will take significantly more time.".to_string())
                .errhint("Increase maintenance_work_mem to speed up builds.".to_string())
                .finish(types_error::ErrorLocation::new("hnswbuild.c", 545, "InsertTuple"))?;
            flush_pages(state, graph)
        },
    );
    let r = match inserted {
        Ok(Inserted::InMemory) => Ok(true),
        Ok(Inserted::OnDisk) => {
            insert_tuple_on_disk(state.index, state.type_info, &mut support, &img, heaptid, true).map(|_| true)
        }
        Err(InsertError::Callback(e)) => Err(e),
        Err(InsertError::Graph(e)) => Err(graph_error(e)),
    };
    bs.support = support;
    r
}

// BuildCallback (hnswbuild.c:585-612).
fn build_callback(
    bs: &mut BuildState<'_, '_>,
    values: &[Datum],
    isnull: &[bool],
    tid: &ItemPointerData,
) -> PgResult<()> {
    // Insert tuple (nulls are skipped there)
    if insert_tuple(bs, values, isnull, tid)? {
        // Update progress
        let indtuples = bs.graph.add_indtuple();
        backend_progress::pgstat_progress_update_param(
            backend_progress::progress::PROGRESS_CREATEIDX_TUPLES_DONE,
            indtuples as i64,
        );
    }
    Ok(())
}
```

followed by `init_build_state` with its signature changed to

```rust
fn init_build_state<'a, 'mcx>(
    heap: Option<&'a Relation<'mcx>>,
    index: &'a Relation<'mcx>,
    fork_num: ForkNumber,
    graph: &'a Graph,
) -> PgResult<BuildState<'a, 'mcx>> {
```

its checks unchanged, and its `Ok(BuildState { .. })` ending in `support, graph, reltuples: 0.0, })` (the `Graph { .. }` literal and the `gmcx` parameter are gone); then `hnsw_build_seed` and `parse_build_seed` unchanged; then:

```rust
// The serial scan: table_index_build_scan with BuildCallback
// (hnswbuild.c:1111-1113).
fn serial_scan<'mcx>(
    bs: &mut BuildState<'_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index_info: &mut IndexInfo<'mcx>,
) -> PgResult<f64> {
    let index = bs.index;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan(
        mcx,
        heap,
        index,
        index_info,
        true,
        /* progress */ true,
        |_index_rel, tid, values, isnull, _alive| {
            build_callback(bs, values, isnull, tid).map_err(|e| {
                inner_err = Some(e);
                PgError::error("hnsw build insert failed").into()
            })
        },
    );
    match reltuples {
        Ok(n) => Ok(n),
        Err(e) => Err(inner_err.unwrap_or(e)),
    }
}

// BuildIndex (hnswbuild.c:1131-1146) and BuildGraph (hnswbuild.c:1092-1125).
fn build_index<'mcx>(
    mcx: Mcx<'mcx>,
    heap: Option<&Relation<'mcx>>,
    index: &Relation<'mcx>,
    index_info: Option<&mut IndexInfo<'mcx>>,
    fork_num: ForkNumber,
) -> PgResult<IndexBuildResult> {
    // SeedRandom before InitBuildState (hnswbuild.c:1134-1136).
    if let Some(seed) = hnsw_build_seed() {
        seed_random(seed);
    }
    // InitBuildState's graph: memoryTotal = maintenance_work_mem * 1024
    // (hnswbuild.c:727).
    let graph = Graph::new(Accounting::serial(
        (init_small::globals::maintenance_work_mem().max(0) as usize).saturating_mul(1024),
    ));
    let mut bs = init_build_state(heap, index, fork_num, &graph)?;

    backend_progress::pgstat_progress_update_param(
        backend_progress::progress::PROGRESS_CREATEIDX_SUBPHASE,
        PROGRESS_HNSW_PHASE_LOAD,
    );

    // Add tuples to graph
    let mut indtuples = 0.0;
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        bs.reltuples = serial_scan(&mut bs, mcx, heap, index_info)?;
        indtuples = graph.indtuples();
    }

    // Flush pages
    graph.flush_if_needed(|g| flush_pages(&bs, g))?;

    // Write WAL for initialization fork since GenericXLog functions do not
    if relation_needs_wal(index) || fork_num == ForkNumber::INIT_FORKNUM {
        let nblocks = bufmgr::RelationGetNumberOfBlocksInFork(index, fork_num)?;
        xloginsert::log_newpage_range(index, fork_num, 0, nblocks, true)?;
    }

    Ok(IndexBuildResult { heap_tuples: bs.reltuples, index_tuples: indtuples })
}
```

then `hnswbuild` and `hnswbuildempty` unchanged, and the test module holding `parse_build_seed_accepts_only_unsigned_integers` and `serial_flush_point_matches_c_at_one_megabyte` only (the `update_connection_*` tests and their helpers now live in `pgvector_hnsw_graph`'s tests).

- [ ] **Step 4: Unit tests, lints**

Run: `cargo test -p pgvector_hnsw_build --lib`
Expected: PASS, 2 tests.

Run: `cargo test -p pgvector_hnsw_graph --lib` and `cargo clippy -p pgvector_hnsw_build -- -D warnings`
Expected: PASS; no warnings.

Run: `cargo test -p seams_init --test lint_determinism`
Expected: PASS, 43 warnings.

- [ ] **Step 5: Every serial build stays byte-identical**

Run: `scripts/pgvector/build-pgrust.sh`, then `scripts/pgvector/run-bytecmp.sh pgrust`, `scripts/pgvector/run-iterscan.sh pgrust`, `scripts/pgvector/run-regress.sh pgrust` and `scripts/pgvector/run-diff.sh pgrust`.
Expected: bytecmp 28/28 identical; iterscan 19/19; regress 14/14; diff 22/22. Any difference is a plan-defined stop (the in-memory algorithm was to move verbatim).

Run: `PGV_WORK=$HOME/.cache/pgrust/m6-work scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 021_hnsw_bit_insert_recall.pl 022_hnsw_bit_vacuum_recall.pl 023_hnsw_bit_duplicates.pl 025_hnsw_halfvec_insert_recall.pl 026_hnsw_halfvec_vacuum_recall.pl 027_hnsw_halfvec_duplicates.pl 029_hnsw_sparsevec_insert_recall.pl 030_hnsw_sparsevec_vacuum_recall.pl 031_hnsw_sparsevec_duplicates.pl 038_hnsw_sparsevec_vacuum_insert.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl`
Expected: 22/22 passed.

- [ ] **Step 6: `spill` with tie-free data**

In `scripts/pgvector/run-bytecmp.sh`, replace the `spill)` arm with:

```bash
    spill)
      # Spills at maintenance_work_mem = 1MB: C's Generation context fills its
      # 1 MB keeper block after 1436 elements (pgvector_hnsw_build's
      # serial_flush_point_matches_c_at_one_megabyte), and the other 567 rows
      # go through on-disk inserts. The Golomb ruler (p = 2003, every value
      # below 2^24) keeps those inserts free of distance ties, whose order C's
      # pointer tie-break makes irreproducible (insert_grid).
      cat <<SQL
SET maintenance_work_mem = '1MB';
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, ARRAY[2*2003*k + (k*k) % 2003, 0, 0]::vector FROM (SELECT (i * 37) % 2003 AS k FROM generate_series(0, 2002) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
```

Run three times each: `scripts/pgvector/run-bytecmp.sh ref spill` and `scripts/pgvector/run-bytecmp.sh pgrust spill`, and check that both sides flushed: `grep -rh 'no longer fits' $HOME/.cache/pgrust/pgvector-work/bytecmp/` shows `after 1436 tuples` from both servers.

- **If all six runs are identical,** promote the case: move `spill` from the opt-in list into `CASES` (after `big_golomb`), update the opt-in comments (the `Opt-in only` line, the header lines that list the opt-in cases, and the `die` message's opt-in list) to `cosine insert_grid`, and run `scripts/pgvector/tests/harness_test.sh test_bytecmp_ref test_bytecmp_cases`. Expected: `all passed` (the generic row check needs at least 10 blocks and height 1; spill's index has about 60 blocks).
- **If any run differs,** keep `spill` opt-in, write the measured reason into its comment (which pages differ, and whether the flush count matched), and record it in the task report.

- [ ] **Step 7: Commit**

```bash
git add crates/contrib/pgvector_hnsw_build crates/_support/types/types_hnsw Cargo.lock scripts/pgvector/run-bytecmp.sh scripts/pgvector/tests/harness_test.sh
git commit -m "feat(pgvector): the serial HNSW build on pgvector_hnsw_graph with C's flush point; spill gets tie-free data

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 6: The parallel HNSW build

**Files:**
- Create: `crates/contrib/pgvector_hnsw_build/src/parallel_build.rs`
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs` (`mod parallel_build;`, `build_index`), `Cargo.toml` (+`pgvector_parallel`, `parallel`, `pgsync`, `tableam`), `Cargo.lock`
- Modify: `scripts/pgvector/sql/parallel-build.sql` (the HNSW part)

**Interfaces:**
- Consumes: Task 5's `BuildState`, `init_build_state`, `build_callback`, `flush_pages`, `serial_scan`; Task 4's `Graph`, `Accounting::{parallel, parallel_area_size}`; Task 2's `pgvector_parallel` functions; Task 1's `execindexing::table_index_build_scan_parallel`.
- Produces: `pub(crate) fn compute_parallel_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32>`, `pub(crate) fn build_graph_parallel<'mcx>(bs: &BuildState<'_, 'mcx>, mcx: Mcx<'mcx>, heap: &Relation<'mcx>, isconcurrent: bool, request: i32) -> PgResult<Option<(f64, f64)>>`.

- [ ] **Step 1: The HNSW cases (the failing test)**

Append to `scripts/pgvector/sql/parallel-build.sql`:

```sql

-- HNSW ------------------------------------------------------------------------
-- HNSW asks the planner only whether workers are allowed, then requests
-- max_parallel_maintenance_workers (hnswbuild.c:1071-1086): one worker in
-- index.c's request line, two launched.
SET hnsw.ef_search = 200;
CREATE TABLE h (id int, v vector(3));
INSERT INTO h SELECT i, ARRAY[i % 23, (i * 7) % 29, (i * 11) % 31] FROM generate_series(1, 5000) i;
SET client_min_messages = debug1;
CREATE INDEX h_l2 ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
-- Each value is unique, so a row's nearest neighbor is itself.
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_l2;

-- 4MB with parallel_workers = 2: the shared area is 1MB, no more than the
-- 1MB margin, so the first tuple flushes ("after 0 tuples", TAP 012's on-disk
-- block) and every tuple is inserted on disk.
ALTER TABLE h SET (parallel_workers = 2);
SET maintenance_work_mem = '4MB';
SET client_min_messages = debug1;
CREATE INDEX h_disk ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
RESET maintenance_work_mem;
ALTER TABLE h RESET (parallel_workers);
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_disk;

-- Review Focus 2: CREATE INDEX CONCURRENTLY with workers.
SET client_min_messages = debug1;
CREATE INDEX CONCURRENTLY h_cic ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_cic;

-- Review Focus 5: a temp table and a parallel-unsafe expression stay serial;
-- a parallel-safe expression and an unlogged table's main fork go parallel.
CREATE TEMP TABLE h_temp AS SELECT * FROM h;
SET client_min_messages = debug1;
CREATE INDEX h_temp_l2 ON h_temp USING hnsw (v vector_l2_ops);
RESET client_min_messages;
DROP TABLE h_temp;
SET client_min_messages = debug1;
CREATE INDEX h_unsafe ON h USING hnsw (unsafe_v(v) vector_l2_ops);
RESET client_min_messages;
DROP INDEX h_unsafe;
SET client_min_messages = debug1;
CREATE INDEX h_bq ON h USING hnsw ((binary_quantize(v)::bit(3)) bit_hamming_ops);
RESET client_min_messages;
DROP INDEX h_bq;
CREATE UNLOGGED TABLE h_unlogged AS SELECT * FROM h;
SET client_min_messages = debug1;
CREATE INDEX h_unlogged_l2 ON h_unlogged USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SELECT id FROM h_unlogged ORDER BY v <-> (SELECT v FROM h_unlogged WHERE id = 2500) LIMIT 1;
DROP TABLE h_unlogged;

-- Review Focus 4: duplicate-heavy data built in parallel loses no heap TID:
-- an iterative scan reads every row back through the index.
CREATE TABLE d (id int, v vector(3));
INSERT INTO d SELECT i, ARRAY[i % 20, 0, 0] FROM generate_series(1, 2000) i;
SET client_min_messages = debug1;
CREATE INDEX d_l2 ON d USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SET hnsw.iterative_scan = relaxed_order;
SELECT count(*) FROM (SELECT id FROM d ORDER BY v <-> '[0,0,0]' LIMIT 2000) s;
RESET hnsw.iterative_scan;

-- Review Focus 1: a row error inside a worker. HNSW indexes at most 1000
-- non-zeros (hnswutils.c:1392); whichever participant reads the last row
-- raises C's ERROR, and the build leaves no index and no worker.
CREATE TABLE sv_bad (id int, v sparsevec(2000));
INSERT INTO sv_bad SELECT i, ('{' || (i % 2000) + 1 || ':1}/2000')::sparsevec FROM generate_series(1, 5000) i;
INSERT INTO sv_bad SELECT 5001, (SELECT '{' || string_agg(j || ':1', ',') || '}/2000' FROM generate_series(1, 1001) j)::sparsevec;
SET client_min_messages = debug1;
CREATE INDEX sv_bad_l2 ON sv_bad USING hnsw (v sparsevec_l2_ops);
RESET client_min_messages;
SELECT count(*) FROM pg_class WHERE relname = 'sv_bad_l2';
SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'parallel worker';

-- Review Focus 3: statement_timeout fires mid-build (512MB keeps the graph
-- in memory, so no NOTICE races the timeout).
CREATE TABLE big (id int, v vector(3));
INSERT INTO big SELECT i, ARRAY[i % 101, (i * 7) % 103, (i * 11) % 107] FROM generate_series(1, 100000) i;
SET maintenance_work_mem = '512MB';
SET statement_timeout = '200ms';
CREATE INDEX big_l2 ON big USING hnsw (v vector_l2_ops);
RESET statement_timeout;
RESET maintenance_work_mem;
SELECT count(*) FROM pg_class WHERE relname = 'big_l2';
SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'parallel worker';
RESET hnsw.ef_search;
```

Run: `scripts/pgvector/sqldiff.sh ref scripts/pgvector/sql/parallel-build.sql`
Expected: `identical` (C against C).

Run: `scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/parallel-build.sql`
Expected: DIFFERENT: pgrust prints no `using 2 parallel workers` for the HNSW builds yet, and no NOTICE `after 0 tuples` for `h_disk` (a serial 4MB build flushes later).

- [ ] **Step 2: The parallel build**

Create `crates/contrib/pgvector_hnsw_build/src/parallel_build.rs`:

```rust
//! hnswbuild.c's parallel build: HnswBeginParallel, HnswParallelBuildMain,
//! HnswParallelScanAndInsert, ParallelHeapScan and HnswEndParallel
//! (hnswbuild.c:762-1065), on pgvector_parallel's launched workers. Every
//! participant, the leader included, scans its share of the heap and inserts
//! into one shared pgvector_hnsw_graph::Graph under C's parallel accounting
//! (maintenance_work_mem - 3 MB, with a 1 MB margin). Whichever participant
//! overflows the graph prints the NOTICE and flushes it; from then on every
//! participant inserts on disk (pgvector_hnsw's on-disk path, which concurrent
//! backends already share; HNSW_UPDATE_LOCK does not exclude members of one
//! lock group, in C either). The leader flushes the graph if nobody did, and
//! WAL-logs the index once every participant has finished
//! (hnswbuild.c:1119-1143).

use std::sync::Arc;

use mcx::Mcx;
use pgsync::Mutex;
use pgvector_hnsw_graph::{Accounting, Graph};
use tableam::ParallelTableScanDescShared;
use types_core::{ForkNumber, Oid};
use types_error::{PgError, PgResult};
use types_rel::Relation;

use crate::{build_callback, flush_pages, init_build_state, BuildState};

const ENTRY_NAME: &str = "pgrust_hnsw_parallel_build_main";

/// HnswShared (hnsw.h:254-274): graphData, the parallel heap scan and the
/// participants' statistics.
struct HnswShared {
    heaprelid: Oid,
    indexrelid: Oid,
    isconcurrent: bool,
    pscan: ParallelTableScanDescShared,
    graph: Graph,
    query_text: Option<String>,
    stats: Mutex<HnswStats>,
}

#[derive(Default)]
struct HnswStats {
    nparticipantsdone: i32,
    reltuples: f64,
}

/// ComputeParallelWorkers (hnswbuild.c:1070-1086), behind the kill switch.
pub(crate) fn compute_parallel_workers(heap: &Relation<'_>, index: &Relation<'_>) -> PgResult<i32> {
    if !pgvector_parallel::vector_parallel_builds_enabled() {
        return Ok(0);
    }
    // Make sure it's safe to use parallel workers
    if pgvector_parallel::plan_create_index_workers(heap, index)? == 0 {
        return Ok(0);
    }
    let max_parallel_maintenance_workers = pgvector_parallel::max_parallel_maintenance_workers();
    // Use parallel_workers storage parameter on table if set
    let parallel_workers = heap.get_parallel_workers(-1);
    if parallel_workers != -1 {
        return Ok(parallel_workers.min(max_parallel_maintenance_workers));
    }
    Ok(max_parallel_maintenance_workers)
}

/// HnswBeginParallel, HnswLeaderParticipateAsWorker, ParallelHeapScan, the
/// leader's FlushPages and HnswEndParallel (hnswbuild.c:762-790, 891-1065,
/// 1107-1124). Returns (reltuples, indtuples), or None when no worker
/// launched: the caller then builds serially (hnswbuild.c:1046-1051).
pub(crate) fn build_graph_parallel<'mcx>(
    bs: &BuildState<'_, 'mcx>,
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    isconcurrent: bool,
    request: i32,
) -> PgResult<Option<(f64, f64)>> {
    // Leave space for other objects in shared memory (hnswbuild.c:961-969)
    let area = Accounting::parallel_area_size(init_small::globals::maintenance_work_mem());
    let (heaprelid, indexrelid) = (heap.rd_id, bs.index.rd_id);
    let Some((leader, shared)) = pgvector_parallel::begin_parallel(
        ENTRY_NAME,
        hnsw_parallel_build_main,
        request,
        heap,
        isconcurrent,
        move |pscan| {
            Ok(Arc::new(HnswShared {
                heaprelid,
                indexrelid,
                isconcurrent,
                pscan,
                graph: Graph::new(Accounting::parallel(area)),
                query_text: pgvector_parallel::leader_query_text(),
                stats: Mutex::new(HnswStats::default()),
            }))
        },
        ("hnswbuild.c", 1054, "HnswBeginParallel"),
    )?
    else {
        return Ok(None);
    };

    // HnswLeaderParticipateAsWorker (hnswbuild.c:917-923)
    scan_and_insert(mcx, heap, bs.index, &shared, true)?;

    // hnswbuild.c:1064
    parallel::WaitForParallelWorkersToAttach(leader.pcxt)?;

    // ParallelHeapScan (hnswbuild.c:762-790): C sleeps on workersdonecv until
    // every participant is done; the workers' exit is the same point here.
    parallel::WaitForParallelWorkersToFinish(leader.pcxt)?;
    let (participants, reltuples) = {
        let stats = pgsync::lock(&shared.stats);
        (stats.nparticipantsdone, stats.reltuples)
    };
    debug_assert_eq!(participants, leader.nworkers_launched + 1);
    let indtuples = shared.graph.indtuples();

    // Flush pages (hnswbuild.c:1119-1120), the leader's state on the shared graph
    shared.graph.flush_if_needed(|graph| flush_pages(bs, graph))?;

    pgvector_parallel::end_parallel(leader)?;
    Ok(Some((reltuples, indtuples)))
}

/// HnswParallelScanAndInsert (hnswbuild.c:795-836).
fn scan_and_insert<'mcx>(
    mcx: Mcx<'mcx>,
    heap: &Relation<'mcx>,
    index: &Relation<'mcx>,
    shared: &HnswShared,
    progress: bool,
) -> PgResult<()> {
    // Join parallel scan
    let mut index_info = execindexing::BuildIndexInfo(mcx, index)?;
    index_info.ii_Concurrent = shared.isconcurrent;
    let mut bs = init_build_state(Some(heap), index, ForkNumber::MAIN_FORKNUM, &shared.graph)?;
    let mut inner_err: Option<Box<PgError>> = None;
    let reltuples = execindexing::table_index_build_scan_parallel(
        mcx,
        heap,
        index,
        &mut index_info,
        progress,
        &shared.pscan,
        |_index_rel, tid, values, isnull, _alive| {
            build_callback(&mut bs, values, isnull, tid).map_err(|e| {
                inner_err = Some(e);
                PgError::error("hnsw build insert failed").into()
            })
        },
    );
    let reltuples = match reltuples {
        Ok(n) => n,
        Err(e) => return Err(inner_err.unwrap_or(e)),
    };

    // Record statistics
    {
        let mut stats = pgsync::lock(&shared.stats);
        stats.nparticipantsdone += 1;
        stats.reltuples += reltuples;
    }

    // Log statistics
    pgvector_parallel::report_processed(progress, reltuples, "hnswbuild.c", 828, 830, "HnswParallelScanAndInsert")
}

/// HnswParallelBuildMain (hnswbuild.c:841-886).
fn hnsw_parallel_build_main(pshared: &parallel::ParallelShared) -> PgResult<()> {
    let shared = pshared
        .private()
        .expect("hnsw parallel build worker without its shared state")
        .downcast::<HnswShared>()
        .unwrap_or_else(|_| panic!("hnsw parallel build private state is not HnswShared"));
    pgvector_parallel::with_worker_activity(shared.query_text.as_deref(), || {
        let cx = mcx::MemoryContext::new("Hnsw parallel build worker");
        let mcx = cx.mcx();
        let rels = pgvector_parallel::worker_open(mcx, shared.heaprelid, shared.indexrelid, shared.isconcurrent)?;
        // Perform inserts
        scan_and_insert(mcx, &rels.heap, &rels.index, &shared, false)?;
        // Close relations within worker
        pgvector_parallel::worker_close(rels)
    })
}
```

In `lib.rs`: add `mod parallel_build;` after the `use` lines, and in `build_index` replace the `// Add tuples to graph` block and the `// Flush pages` line with:

```rust
    // Add tuples to graph: calculate parallel workers, attempt to launch a
    // parallel build when required, else scan serially (hnswbuild.c:1099-1115).
    let mut indtuples = 0.0;
    let mut built_in_parallel = false;
    if let (Some(heap), Some(index_info)) = (bs.heap, index_info) {
        let parallel_workers = parallel_build::compute_parallel_workers(heap, index)?;
        let built = if parallel_workers > 0 {
            parallel_build::build_graph_parallel(&bs, mcx, heap, index_info.ii_Concurrent, parallel_workers)?
        } else {
            None
        };
        match built {
            Some((reltuples, n)) => {
                bs.reltuples = reltuples;
                indtuples = n;
                built_in_parallel = true;
            }
            None => {
                bs.reltuples = serial_scan(&mut bs, mcx, heap, index_info)?;
                indtuples = graph.indtuples();
            }
        }
    }

    // Flush pages (a parallel build flushed its shared graph already)
    if !built_in_parallel {
        graph.flush_if_needed(|g| flush_pages(&bs, g))?;
    }
```

Add to `Cargo.toml` `[dependencies]`: `pgvector_parallel = { path = "../pgvector_parallel" }`, `parallel = { path = "../../backend/access/transam/parallel" }`, `pgsync = { path = "../../_support/pgsync" }`, `tableam = { path = "../../backend/access/table/tableam" }`.

- [ ] **Step 3: Build and unit tests**

Run: `cargo test -p pgvector_hnsw_build --lib` and `cargo clippy -p pgvector_hnsw_build -- -D warnings`
Expected: PASS, 2 tests; no warnings.

Run: `scripts/pgvector/build-pgrust.sh`
Expected: builds.

- [ ] **Step 4: The parallel-build cases match C**

Run: `scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/parallel-build.sql`
Expected: `identical`. In `$PGV_WORK/sqldiff/pgrust/pgrust.out`:
- `h_l2`, `h_cic`, `h_bq`, `h_unlogged_l2` and `d_l2`: `with request for 1 parallel workers`, then `using 2 parallel workers`;
- `h_disk`: `with request for 2 parallel workers`, `using 2 parallel workers`, then the NOTICE `hnsw graph no longer fits into maintenance_work_mem after 0 tuples` with its DETAIL and HINT;
- `h_temp_l2` and `h_unsafe`: `serially`;
- the nearest-row queries return `2500`, the duplicate count is `2000`;
- `sv_bad_l2`: `ERROR:  sparsevec cannot have more than 1000 non-zero elements for hnsw index`, then `0` and `0`;
- `big_l2`: `ERROR:  canceling statement due to statement timeout`, then `0` and `0`.

Run it twice more. Expected: `identical` both times (participant counts and which participant raised the NOTICE or the ERROR vary; the filtered transcript must not).

- [ ] **Step 5: The HNSW TAP gate, three times**

Run, three times:

```bash
PGV_WORK=$HOME/.cache/pgrust/m6-work scripts/pgvector/run-tap.sh pgrust \
  012_hnsw_vector_build_recall.pl 020_hnsw_bit_build_recall.pl 024_hnsw_halfvec_build_recall.pl \
  028_hnsw_sparsevec_build_recall.pl 045_hnsw_low_memory_build.pl
```

Expected: `tap (pgrust): 5/5 passed` each time. The 045 prove log shows `with request for 1 parallel workers`, `using 1 parallel workers` and the NOTICE `after 0 tuples`; the 012 log's in-memory blocks show `using 2 parallel workers` and no NOTICE, its on-disk blocks `using 2` and `after 0 tuples`.

A recall assertion that fails in two of the three runs is a plan-defined stop (C passes these repeatedly; a parallel graph that loses recall is a bug).

- [ ] **Step 6: The tiers that must not move, and the whole TAP suite**

Run: `scripts/pgvector/run-bytecmp.sh pgrust` (all default cases identical), `scripts/pgvector/run-iterscan.sh pgrust` (19/19), `scripts/pgvector/run-diff.sh pgrust` (22/22), `scripts/pgvector/run-diff.sh ref` (22/22).

Run: `PGV_WORK=$HOME/.cache/pgrust/m6-work scripts/pgvector/run-tap.sh pgrust`
Expected: `tap (pgrust): 48/48 passed`.

Run: `cargo test -p seams_init --test lint_determinism` and `RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`
Expected: PASS (43 warnings); 5 loom tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/contrib/pgvector_hnsw_build Cargo.lock scripts/pgvector/sql/parallel-build.sql
git commit -m "feat(pgvector): parallel HNSW builds on the shared graph; parallel_build covers CIC, errors, cancel and duplicates

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 7: The build-time report

**Files:**
- Create: `scripts/pgvector/run-buildtime.sh`
- Test: `scripts/pgvector/tests/harness_test.sh` (`test_buildtime_ref`)

**Interfaces:**
- Produces: `run-buildtime.sh {pgrust|ref}` → `$PGV_WORK/buildtime/<mode>/summary.tsv` (one row per AM and worker count: `am, workers, c_ms, c_launched, subject_ms, subject_launched`) and `report.md` (a Markdown table, also printed). Environment: `PGV_BUILDTIME_ROWS` (default 100000), `PGV_BUILDTIME_DIM` (128), `PGV_BUILDTIME_WORKERS` (`"0 1 2 4"`).

- [ ] **Step 1: Write the failing self-test**

Insert above `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# The build-time report, C against C at a toy size: every row is timed, and a
# forced worker count launches that many workers on both servers.
test_buildtime_ref() {
  local log="$PGV_WORK/buildtime-ref.log" f="$PGV_WORK/buildtime/ref/summary.tsv"
  if PGV_BUILDTIME_ROWS=20000 PGV_BUILDTIME_DIM=8 PGV_BUILDTIME_WORKERS="0 2" \
    "$here/../run-buildtime.sh" ref >"$log" 2>&1; then
    pass "buildtime ref: exit 0"
  else
    fail "buildtime ref: exit 0 (see $log)"
  fi
  if [ "$(wc -l <"$f" 2>/dev/null | tr -d ' ')" = 4 ] &&
    awk -F'\t' '$3 !~ /^[0-9.]+$/ || $5 !~ /^[0-9.]+$/ { bad = 1 } END { exit bad }' "$f"; then
    pass "buildtime ref: 4 timed rows"
  else
    fail "buildtime ref: 4 timed rows (see $f)"
  fi
  if awk -F'\t' '$2 == 2 && ($4 != 2 || $6 != 2) { bad = 1 }
      $2 == 0 && ($4 != 0 || $6 != 0) { bad = 1 } END { exit bad }' "$f"; then
    pass "buildtime ref: workers launched as forced"
  else
    fail "buildtime ref: workers launched as forced (see $f)"
  fi
}
```

Run: `scripts/pgvector/tests/harness_test.sh test_buildtime_ref`
Expected: FAIL ("No such file or directory" for `run-buildtime.sh`).

- [ ] **Step 2: The script**

Create `scripts/pgvector/run-buildtime.sh` (and `chmod +x` it):

```bash
#!/usr/bin/env bash
# Build time against C pgvector (M6; spec §9: "build time against pgvector
# reported"). A report, not a gate: HNSW and IVFFlat builds of one table on a
# fresh C reference and a fresh subject, serially and with workers forced
# through the parallel_workers reloption (0 = serial).
# usage: run-buildtime.sh {pgrust|ref}
#   pgrust: the subject is pgrust.  ref: the subject is the seeded C build
#   (C against C, the self-test's run).
#   PGV_BUILDTIME_ROWS (default 100000) and PGV_BUILDTIME_DIM (default 128)
#   size the table; PGV_BUILDTIME_WORKERS (default "0 1 2 4") lists the
#   worker counts. One run per cell; the builds run one at a time.
#   Report: $PGV_WORK/buildtime/<mode>/report.md, rows in summary.tsv.
set -euo pipefail
. "$(dirname "$0")/common.sh"

usage() { die "usage: run-buildtime.sh {pgrust|ref}"; }
[ "$#" -eq 1 ] || usage
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) usage ;;
esac
rows="${PGV_BUILDTIME_ROWS:-100000}"
dim="${PGV_BUILDTIME_DIM:-128}"
workers_list="${PGV_BUILDTIME_WORKERS:-0 1 2 4}"
out="$PGV_WORK/buildtime/$mode"
rm -rf "$out"
mkdir -p "$out"

# The table: deterministic float values, the same on both servers.
setup_sql() {
  cat <<SQL
CREATE EXTENSION vector;
CREATE TABLE bt (id int, v vector($dim));
INSERT INTO bt SELECT i, ARRAY(SELECT sin(i * 12.9898 + j * 78.233) FROM generate_series(1, $dim) j)::vector($dim)
  FROM generate_series(1, $rows) i;
VACUUM ANALYZE bt;
SQL
}

# One build on the server at port $1: access method $2 with $3 workers
# forced. Prints "<ms>\t<workers launched>".
time_build() {
  local port="$1" am="$2" workers="$3" ddl log ms launched
  case "$am" in
    hnsw) ddl="CREATE INDEX bt_idx ON bt USING hnsw (v vector_l2_ops);" ;;
    ivfflat) ddl="CREATE INDEX bt_idx ON bt USING ivfflat (v vector_l2_ops) WITH (lists = 100);" ;;
    *) die "unknown access method '$am'" ;;
  esac
  log="$out/$am-$workers-$port.log"
  pgv_psql "$port" >"$log" 2>&1 <<SQL || die "the $am build with $workers workers failed on port $port; see $log"
SET max_parallel_maintenance_workers = $workers;
ALTER TABLE bt SET (parallel_workers = $workers);
SET maintenance_work_mem = '2GB';
SET client_min_messages = debug1;
\\timing on
$ddl
\\timing off
RESET client_min_messages;
DROP INDEX bt_idx;
ALTER TABLE bt RESET (parallel_workers);
SQL
  ms="$(sed -n 's/^Time: \([0-9.]*\) ms.*/\1/p' "$log" | head -1)"
  launched="$(sed -n 's/.*DEBUG:  using \([0-9]*\) parallel workers$/\1/p' "$log" | head -1)"
  [ -n "$ms" ] || die "no timing for the $am build with $workers workers on port $port; see $log"
  printf '%s\t%s\n' "$ms" "${launched:-0}"
}

trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  port="$(port_for "$side")"
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$port"
  setup_sql | pgv_psql "$port" >"$out/setup-$side.log" 2>&1 || die "loading the table failed on $side; see $out/setup-$side.log"
done

: >"$out/summary.tsv"
for am in hnsw ivfflat; do
  for w in $workers_list; do
    c="$(time_build "$(port_for ref)" "$am" "$w")"
    s="$(time_build "$(port_for "$subject")" "$am" "$w")"
    printf '%s\t%s\t%s\t%s\n' "$am" "$w" "$c" "$s" >>"$out/summary.tsv"
  done
done

{
  echo "# Build time: $subject against C pgvector 0.8.7"
  echo
  echo "$rows rows of vector($dim); HNSW m = 16, ef_construction = 64; IVFFlat lists = 100;" \
    "maintenance_work_mem = 2GB; $(uname -sm), $(ncpu) cores. One run per cell; workers forced" \
    "with the parallel_workers reloption (0 = serial)."
  echo
  echo "| AM | workers | C launched | C ms | $subject launched | $subject ms | $subject / C |"
  echo "|---|---|---|---|---|---|---|"
  awk -F'\t' '{ printf "| %s | %s | %s | %.1f | %s | %.1f | %.2f |\n", $1, $2, $4, $3, $6, $5, ($3 > 0 ? $5 / $3 : 0) }' "$out/summary.tsv"
} >"$out/report.md"
cat "$out/report.md"
```

Run: `/bin/bash -n scripts/pgvector/run-buildtime.sh`
Expected: no output.

- [ ] **Step 3: The self-test passes**

Run: `scripts/pgvector/tests/harness_test.sh test_buildtime_ref`
Expected: three `ok` lines, `all passed`.

- [ ] **Step 4: The report on pgrust**

Run: `scripts/pgvector/run-buildtime.sh pgrust`, then `mkdir -p ~/.cache/pgrust/m6-evidence && cp -R ~/.cache/pgrust/pgvector-work/buildtime/pgrust ~/.cache/pgrust/m6-evidence/buildtime`.
Expected: an 8-row table. The C and pgrust launched counts match the forced workers (0, 1, 2, 4; the C reference's `max_worker_processes` is 8, so 4 workers launch). The times go into the M6 report (Task 8) as measured; no threshold applies.

- [ ] **Step 5: Commit**

```bash
git add scripts/pgvector/run-buildtime.sh scripts/pgvector/tests/harness_test.sh
git commit -m "feat(pgvector): run-buildtime.sh, the build-time report against C

Co-Authored-By: <model trailer per Global Constraints>"
```

---

### Task 8: The full gate, the docs and the M6 report

**Files:**
- Modify: `CLAUDE.md` (the "Vector search (fork work)" section)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (§7.1, §8.2's Exact differential and Byte-identical rows, §8.3)
- Modify: `crates/contrib/pgvector_hnsw/src/lib.rs:5-6`, `crates/contrib/pgvector_ivfflat/src/lib.rs:8` (the "no parallel build" divergence notes)
- Create: `docs/superpowers/reports/pgvector-m6-report.md`, `docs/HANDOFF_<date>.md`

- [ ] **Step 1: Every gate**

Run, saving each log to `~/.cache/pgrust/m6-evidence/`:
- `scripts/pgvector/build-pgrust.sh`
- `scripts/pgvector/run-all.sh ref`. Expected: 138/138, or 139/139 if Task 5 promoted `spill`.
- `scripts/pgvector/run-all.sh pgrust`. Expected: the same count, all passing (TAP 48/48).
- `scripts/pgvector/tests/harness_test.sh`. Expected: `all passed`; record the `ok` count (195 before M6, plus the new checks).
- `scripts/pgvector/run-repair-check.sh`. Expected: 6/6.
- The 8 parallel TAP files three more times with `PGV_WORK=$HOME/.cache/pgrust/m6-work` (`003 012 020 024 028 032 035 045`). Expected: 8/8 each time.
- Unit tests: `cargo test -p planner --lib index_workers`, `-p types_relscan --lib`, `-p execindexing --lib`, `-p catalog_index --lib`, `-p pgvector_parallel --lib`, `-p pgvector_hnsw_graph --lib`, `-p pgvector_hnsw_build --lib`, `-p pgvector_ivfflat_build --lib`, `-p pgvector_hnsw --lib`, `-p pgvector_ivfflat --lib`, `-p types_hnsw --lib`. Expected: all pass; record the counts.
- Loom: `RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`. Expected: 5 pass.
- Lints: `cargo test -p seams_init --test lint_seam_installs`, `--test lint_determinism`, `--test lint_walker_arms`, `cargo test -p catalog_index --test lint_inplace_locks`. Expected: all pass; `lint_determinism` with 43 warnings.
- The kill switch: `PGRUST_VECTOR_INDEXBUILD_POOL=0 scripts/pgvector/sqldiff.sh pgrust scripts/pgvector/sql/parallel-build.sql`. Expected: DIFFERENT, and `grep -c 'parallel workers$' $PGV_WORK/sqldiff/pgrust/pgrust.out` counts only `with request for` lines (no `using N parallel workers`): every build ran serially.
- The pgvector-rust client check: `~/.cache/pgrust/pgvector-rust-check/run.sh` (usage in its header). Expected: 26/26 on pgrust, as before M6.
- The reference trees: `git status --porcelain --ignored crates/pgvector-0.8.7-reference crates/postgres-18.6-reference`. Expected: empty.

- [ ] **Step 2: The code comments that said "no parallel build"**

- `crates/contrib/pgvector_hnsw/src/lib.rs`: replace `DIVERGENCES (recorded): no parallel build` / `//! (C falls back to serial when no workers launch); iterative-scan` with `Parallel builds: pgvector_hnsw_build's parallel_build.rs. DIVERGENCES (recorded): iterative-scan`, rewrapping the comment.
- `crates/contrib/pgvector_ivfflat/src/lib.rs`: replace `DIVERGENCES (recorded): no parallel build (M6); an unknown` with `DIVERGENCES (recorded): a parallel build streams its rows to one leader sort (pgvector_ivfflat_build's parallel_build.rs); an unknown`, rewrapping.

Run: `cargo check -p pgvector_hnsw -p pgvector_ivfflat`
Expected: no errors.

- [ ] **Step 3: The spec, in place (as earlier milestones did)**

- §7.1, the **Workers** bullet: replace the morsel-pool sentences with: workers are launched backends from the `parallel` crate (`CreateParallelContext` … `DestroyParallelContext`), C's shape; ruling M6-1 records why (the pool's one-block minimum claim against its 2 ms target, its silent serial fallback under load against TAP's `using N parallel workers` lines, C's error and cancel paths, no raw spawns either way), and a pool channel remains possible later. Keep the kill switch sentence; add that the entry points register under `"postgres"` with pgrust names.
- §7.2: add that the graph is `pgvector_hnsw_graph`, the serial build uses it too, and its memory accounting is a model of C's allocator (Decision 3 of the M6 plan), with the 1436-tuple check.
- §7.3: add that the parallel context ends after the scan.
- §8.2, Exact differential: add `scripts/pgvector/sql/parallel-build.sql` (M6) to the list of sqldiff scripts, with its filter of per-participant lines.
- §8.2, Byte-identical index: if `spill` was promoted, move it out of the opt-in list ("Two cases are opt-in known divergences: `vector` cosine and `insert_grid`") and add it to the covered cases ("a 2,003-row build that spills past `maintenance_work_mem` at C's flush point"); otherwise update its reason to the one Task 5 measured.
- §8.3: add `run-buildtime.sh` (M6) and `sql/parallel-build.sql` (M6).

- [ ] **Step 4: `CLAUDE.md`**

In "Vector search (fork work)":
- add `pgvector_parallel` and `pgvector_hnsw_graph` to the crate list (one line each: what they hold);
- replace "Parallel index builds (M6) are **not ported**: both AMs build serially." with: parallel builds (M6) use launched workers (`pgvector_parallel`); the kill switch `PGRUST_VECTOR_INDEXBUILD_POOL=0`; HNSW's shared graph and its loom command (`RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`); IVFFlat streams rows to one leader sort;
- the harness block: add `scripts/pgvector/run-buildtime.sh {pgrust|ref}` with its one-line description, and note the `min_parallel_table_scan_size=8MB` pin beside the existing `max_parallel_workers_per_gather` mention;
- the baselines list: add `docs/superpowers/reports/pgvector-m6-report.md`.

- [ ] **Step 5: The M6 report**

Create `docs/superpowers/reports/pgvector-m6-report.md`, following the M5 report's structure (`## Summary`, `## pgrust results`, `## What M6 built`, `## Rulings`, `## Findings and known divergences`, `## Build time`, `## Coverage limits`, `## Still open`), with:
- the gate table from Step 1 (counts, timings, evidence paths), the three extra runs of the 8 parallel TAP files, the loom wall times and the red check from Task 4;
- rulings M6-1 (launched workers) and the plan's Decisions 2-14 as applied, each with what it would cost if wrong;
- the divergences: entry-point names; the graph's accounting model and when its arena is freed; IVFFlat's within-list order and early end of the parallel phase; `amcanbuildparallel` false for btree/GIN/BRIN; no `ii_ParallelWorkers` field; the kill switch;
- the bug-compatible finding: parallel builds drop `ii_BrokenHotChain` as C's do (Decision 10);
- `spill`'s outcome (promoted, or kept opt-in with the measured reason);
- the build-time table from Task 7, as measured;
- coverage limits: arm64 macOS only; the wasm build not run (serial by construction); the pool channel not built;
- still open: a morsel-pool channel; HNSW build phase names (handoff §5); the stale comment in `nbtsort/src/pool.rs`'s banner about `index_build` being serial-only (upstream-owned, left alone); anything the final review parked.

- [ ] **Step 6: The handoff**

Create `docs/HANDOFF_<today>.md` in the 2026-10-20 handoff's shape: §1 next step (M7: the approximate differential, then a final run of every tier through the wrapper and the CLAUDE.md update); §2 what M7 starts from (the gates now, the new tiers and rows); §3 open threads (the report's "Still open", plus the handoff-2026-10-20 §4 items still open); §4 how to resume (branch, ports, gates, timings, evidence and ledger paths).

- [ ] **Step 7: Commit**

```bash
git add CLAUDE.md docs crates/contrib/pgvector_hnsw/src/lib.rs crates/contrib/pgvector_ivfflat/src/lib.rs
git commit -m "docs(pgvector): M6 report, spec and CLAUDE.md for parallel builds; handoff to M7

Co-Authored-By: <model trailer per Global Constraints>"
```

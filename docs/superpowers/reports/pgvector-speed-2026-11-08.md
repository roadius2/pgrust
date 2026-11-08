# pgvector on pgrust: speed on the merged server (2026-11-08)

The first speed measurements on the v0.4-beta engine (fork `main` at `fe9b4a73cc`), all from `release` binaries on a quiet machine. Three questions from `docs/HANDOFF_2026-11-05.md`: how the fork compares with stock C pgvector 0.8.7 on the new engine; whether the engine change cost or bought speed against the fork's own v0.3.1 numbers (§4 there); and whether M7's `halfvec` build-time outlier survives on release. A report, not a gate.

**Summary**

- **Against C on `vector`:** pgrust is 1.1× to 1.3× C on HNSW searches, 1.5× to 1.6× on HNSW builds (serial and 2-worker), 1.5× on index-maintaining inserts, 1.45× on an exact (no-index) scan, 1.2× to 2.0× on the synthetic build-time grid. Recall is identical, and both servers read exactly 10 index tuples per `ORDER BY … LIMIT 10` query, so Gravity's windowed index scan over-reads nothing on an unfiltered walk.
- **Against the fork at v0.3.1:** the exact scan is 1.9× faster under Gravity (10.1 s → 5.4 s); searches are 3% to 7% faster; builds and inserts are unchanged.
- **Against upstream v0.4-beta's own port:** upstream refuses every search and insert (`gravity: refused`), as the handoff said, so only builds and the exact scan compare: the exact scan is equal, and upstream's serial HNSW build is 5.6% faster than the fork's (16.4 s vs 17.3 s) where the two were equal at v0.3.1.
- **`halfvec` is the outlier, confirmed on release:** HNSW builds 4.4× C serial and 3.9× parallel; IVFFlat 11.2× and 8.1×. On pgrust a `halfvec` HNSW build takes 2.35× its own `vector` build; on C it is faster than `vector`. The lead is the port's per-element software f16→f32 conversion (below).

## Method

- **Machine:** M5 Max, 18 cores, macOS 25.6, AC power, no sleep events, Time Machine finished before the first step (each step's `.log` in the evidence records power, load, backup state and sleep events before and after).
- **Binaries.** C: `~/.cache/pgrust/pgref-18.6/pgvec` (PostgreSQL 18.6 `--enable-debug`, no LLVM; pgvector 0.8.7 at `-O2 -ftree-vectorize -fassociative-math -ffp-contract=fast`, `_Float16` conversions). Fork: `target/release/postgres` at `fe9b4a73cc`, `version()` = `pgrust 0.4-beta, build fe9b4a7`, staged `vector` 0.8.7. Upstream: `~/.cache/pgrust/upstream-v04beta/target/release/postgres` at `38634b408d` (the `v0.4-beta` tag, a worktree), staged `vector` 0.8.5. Each server's command line and `lsof` text segment are in `bench/<label>-server-binary.txt`; its settings in `bench/<label>-settings.txt`.
- **Settings.** The harness's conventions: C at its defaults (`io_method = worker`, `max_worker_processes = 8`); pgrust with `PGRUST_SERVER_OPTS` (`io_method = sync`, `max_parallel_workers_per_gather = 2`, `min_parallel_table_scan_size = 8MB`; pgrust's own `max_worker_processes` default is 16). `shared_buffers` 128MB and `work_mem` 4MB on both; `maintenance_work_mem = 2GB` for builds. Nothing here binds on workers: `max_parallel_maintenance_workers` is 2 on both. The data (40 MB heap) sits in the page cache throughout.
- **Workload A, `bench.sh`** (the handoff's §4 workload, extended): the gate-size GloVe rows, 100,000 × `vector(100)` and 1,000 held-out queries; cosine HNSW `m = 16, ef_construction = 64`. Per server: the exact top-10 of all 1,000 queries by sequential scan (3 runs); 3 serial builds, each followed by 1,000 top-10 searches at `hnsw.ef_search` 40 and 100 (1 warm-up and 5 timed runs, median) with recall@10 against the exact answers, and 1,000 rows inserted under the index in one statement; then 3 builds at default settings with the same searches. Medians of the 3 builds' values. After the first serial build, `idx_tup_read` for one run of 1,000 searches (`pg_stat_force_next_flush()`).
- **Workload B, `run-buildtime.sh pgrust`:** 100,000 rows of synthetic `vector(128)`, HNSW and IVFFlat (`lists = 100`), workers forced to 0, 1, 2 and 4; one build per cell.
- **Workload C, `run-recall.sh pgrust`:** the four `halfvec` cases and the two serial `vector` cases at gate size (GloVe, 3 builds a side, medians); recall verdicts and build times.

## A. GloVe 100k × 100d, cosine HNSW (ms; medians of 3)

| metric | C 0.8.7 | fork v0.4 | upstream v0.4 | fork / C | upstream / C |
|---|---|---|---|---|---|
| exact scan, 1,000 queries | 3710.9 | 5386.1 | 5439.9 | 1.45 | 1.47 |
| build, serial | 11048.4 | 17278.3 | 16357.3 | 1.56 | 1.48 |
| 1,000 searches, ef_search 40 (serial-built) | 213.0 | 241.4 | refused | 1.13 | – |
| 1,000 searches, ef_search 100 (serial-built) | 431.7 | 476.7 | refused | 1.10 | – |
| 1,000 inserts under the index | 586.2 | 890.9 | refused | 1.52 | – |
| build, default settings | 3797.0 (2 workers) | 5802.5 (2 workers) | 16298.2 (serial) | 1.53 | 4.29 |
| 1,000 searches, ef_search 40 (default-built) | 189.9 | 244.5 | refused | 1.29 | – |
| 1,000 searches, ef_search 100 (default-built) | 400.5 | 485.8 | refused | 1.21 | – |
| recall@10, ef_search 40 / 100 | 0.9433 / 0.9796 | 0.9436 / 0.9783 | – | | |
| `idx_tup_read` per 1,000 searches, ef 40 / 100 | 10000 / 10000 | 10000 / 10000 | – | 1.00 | |

- **Spread.** C's serial builds 10.9 to 11.4 s, the fork's 16.8 to 17.3 s; C's searches at ef 40 after the default build 179 to 211 ms (the widest cell), the fork's 238 to 246 ms. Every other cell's three values are within 3% of each other. Per-run values: `bench/*.tsv`.
- **Upstream's refusals** are the two the handoff describes: `decompose: IndexScan … is not expressible: an ordinary scan of an index access method without one` for every search, and `lower: … an index whose access method is not btree / hash / … (index_maint)` for every insert. Its default-settings build is serial because upstream has no parallel vector build.
- **C's searches speed up after its parallel build** (190 vs 213 ms at ef 40) and the fork's don't (244 vs 241 ms). The index pages differ between a serial and a parallel build on both sides; whether C's parallel layout is more cache-friendly or the cell is noise (C's spread there is 15%) isn't settled.

**The same workload on 2026-11-05, release binaries at v0.3.1** (`~/.cache/pgrust/cmp-work/out/`; the machine's quietness then is unrecorded):

| metric | upstream v0.3.1 | fork v0.3.1 | fork v0.4 (today) | fork v0.4 / fork v0.3.1 |
|---|---|---|---|---|
| exact scan | 9586.3 | 10087.7 | 5386.1 | 0.53 |
| build, serial | 17112.1 | 17018.8 | 17278.3 | 1.02 |
| searches, ef 40 / 100 (serial-built) | 246.3 / 506.1 | 247.8 / 510.5 | 241.4 / 476.7 | 0.97 / 0.93 |
| 1,000 inserts (one run then) | 821.9 | 861.7 | 890.9 | 1.03 |
| build, default settings | 18292.7 (serial) | 5654.9 (2 workers) | 5802.5 (2 workers) | 1.03 |
| searches, ef 40 / 100 (default-built) | 253.0 / 507.0 | 248.7 / 510.5 | 244.5 / 485.8 | 0.98 / 0.95 |

Gravity bought the exact scan (the row executor's seq scan, sort and distance calls were 1.9× slower) and a few percent on searches; it changed nothing in builds and inserts, which never ran through the executor. Upstream's own serial build went from 17.1 s to 16.4 s between its releases while the fork's stayed at 17.0 to 17.3 s: whatever upstream sped up in its HNSW build didn't reach the fork's build crates. The handoff's open thread, the fork's `new_bump` contexts where upstream moved to `new_wholesale`, is a candidate.

## B. Build time, synthetic `vector(128)`, 100k rows (ms; one run per cell)

| AM | workers | C ms | pgrust ms | pgrust / C |
|---|---|---|---|---|
| hnsw | 0 | 4412.7 | 7236.9 | 1.64 |
| hnsw | 1 | 2701.1 | 3958.5 | 1.47 |
| hnsw | 2 | 1945.5 | 2907.2 | 1.49 |
| hnsw | 4 | 1463.5 | 1828.3 | 1.25 |
| ivfflat | 0 | 341.6 | 664.5 | 1.95 |
| ivfflat | 1 | 318.7 | 490.4 | 1.54 |
| ivfflat | 2 | 342.0 | 434.5 | 1.27 |
| ivfflat | 4 | 310.9 | 371.8 | 1.20 |

Both sides launched the requested workers in every cell. The ratios match M7's `fast-profile` table (1.58 / 1.46 / 1.35 for HNSW at 0 / 1 / 2 workers), so the profile wasn't hiding anything. pgrust scales better with workers than C because its serial build is the slow part; C's IVFFlat hardly scales at this size (the k-means dominates and is serial on both).

## C. The recall tier at gate size, release (build ms; medians of 3; all 14 rows `ok`)

| case | C | pgrust | pgrust / C | pgrust `halfvec` / pgrust `vector` | C `halfvec` / C `vector` |
|---|---|---|---|---|---|
| hnsw_vector_serial | 10268.4 | 17005.3 | 1.66 | | |
| hnsw_halfvec_serial | 9100.2 | 39929.0 | 4.39 | 2.35 | 0.89 |
| hnsw_halfvec_parallel | 3403.7 | 13202.6 | 3.88 | | |
| ivfflat_vector_serial | 784.1 | 1764.7 | 2.25 | | |
| ivfflat_halfvec_serial | 768.2 | 8583.0 | 11.17 | 4.86 | 0.98 |
| ivfflat_halfvec_parallel | 618.1 | 4984.2 | 8.06 | | |

Recall differences are within ±0.003 on every row and `exact_vector` / `exact_halfvec` match C to 4e-07. M7's release numbers were 4.13 / 3.79 / 9.85 / 8.44 for the four `halfvec` rows; these agree.

**The `halfvec` lead.** C's `half` is `_Float16` on this Mac (`halfvec.h:48`: `__FLT16_MAX__` is defined, so `FLT16_SUPPORT`), and `HalfToFloat4` is a hardware widening the compiler vectorizes inside the `*Default` kernels (`halfutils.h:67`, `halfutils.c:29-43`). The port's `half_to_float4` (`crates/contrib/pgvector/src/halfutils.rs:39-73`) is halfutils.h's *software* path, branches and a 10-iteration loop for subnormals, called once per element of both operands inside every kernel (`halfutils.rs:144-188`), and the kernels are what both index builds spend their time in. That explains why `halfvec` costs pgrust 2.4× to 4.9× its own `vector` time while costing C nothing. f16→f32 widening is exact, so any correct conversion gives the same bits: the fix changes no result and `pgvector_f16_parity` already checks every f16. Rust 1.96's `f16` primitive is unstable, but the conversion can be a branch-free routine LLVM vectorizes, or the hardware instruction per architecture: on AArch64 both `std::arch::aarch64::vcvt_f32_f16` (through `float16x4_t`) and an `asm!` `fcvtl` compile on the pinned stable toolchain (checked 2026-11-08); x86-64's F16C `_mm_cvtph_ps` is stable too but unchecked here. The kernels' summation order stays as it is (user decision 2026-10-06: no emulation of C's vectorized reductions), so some gap against C would remain after the fix; `vector`'s 1.5× to 1.7× is the floor to expect.

## What this measures and what it doesn't

- Single-client latency and single-statement build time on one machine, data in cache, release profile. No concurrency, no I/O-bound sizes, no x86-64.
- `halfvec`'s lead is from reading the source, not from a profile; the fix work should profile first to confirm the conversion is where the time goes.
- The 1.5× `vector` build gap and the 1.45× exact-scan gap have no attributed cause yet. Both need a profile (`fast-profile` or `profiling` builds exist for this). The exact scan is engine-level: upstream's port shows the same 1.47×.
- Nothing at scale was rerun: M7's `scale-2` (396k × 300d, 2.1× to 2.9× C on builds) stands as the only scale measurement.

## Evidence

`~/.cache/pgrust/speed-2026-11-08/`: `run.sh` (the driver; waits for Time Machine, then the five steps under `timed.sh`), `bench.sh`, `summarize.sh`, each step's `<step>.log` and `.out`, `bench/` (per-run TSVs, logs, plans, settings and server binaries), `work/buildtime/pgrust/` and `work/recall/pgrust/` (the tiers' own reports and TSVs), the two build logs and the commits measured (`fork-commit.txt`, `upstream-commit.txt`). The upstream worktree stays at `~/.cache/pgrust/upstream-v04beta` (1.1 GB target) for the next comparison; `git worktree remove` it when done.

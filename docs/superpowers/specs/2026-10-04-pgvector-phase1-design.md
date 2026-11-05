# pgvector Phase 1: Compatibility — Design

Date: 2026-10-04 · Branch: `vector/phase1` · Status: approved in brainstorming, awaiting spec review

## 1. Context

pgrust already contains a partial pgvector port, pinned to pgvector 0.8.5 (upstream commit `159b79a`):

- `crates/contrib/pgvector` contains the `vector` type, its functions, the `avg`/`sum` aggregates, array casts and the btree opclass.
- `crates/contrib/pgvector_hnsw` and `crates/contrib/pgvector_hnsw_build` contain the HNSW access method (build, insert, scan, vacuum, iterative scans). They use pgvector's page layout and GenericXLog.
- `crates/_support/types/types_hnsw` holds the HNSW constants and scan state.

The port's own header states its limits: `halfvec`, `sparsevec`, the `bit` opclasses and IVFFlat are not ported, and the extension script is trimmed to match. HNSW builds are serial only.

The survey `docs/Vector Search for pgrust Algorithm & Benchmark Survey.md` sets out a five-phase roadmap. This spec covers **Phase 1 (compatibility)** only. Phases 2–5 (an IVF + RaBitQ access method, planner-integrated filtering, columnar/parallel execution, DiskANN) each get their own spec. Per the survey's evidence, the compatibility layer is not where vector performance will come from, so Phase 1 aims for the cheapest correct path to compatibility and leaves algorithmic freedom to Phase 2.

## 2. Goals and decisions

| Decision | Choice | Rationale |
|---|---|---|
| Target version | **pgvector 0.8.7** (latest; the existing code moves up from 0.8.5) | The 0.8.5→0.8.7 changes are hardening and bug fixes, with no page-format change. The `pgvector/pgvector:0.8.7-pg18` image exists for amd64 and arm64. |
| Compatibility level | **SQL/wire drop-in + on-disk (forward)** | `CREATE EXTENSION vector` and every pgvector type, operator, function, cast, aggregate, opclass, reloption and GUC behave identically, with identical text and binary I/O. A PG18 + pgvector data directory, including its indexes, boots and works in pgrust. |
| Reverse on-disk direction | **Reported, not gated**, in Phase 1 | See §2.1. |
| Bit-exact distances | **Not required.** Distances are compared with a relative tolerance. | pgvector builds with `-fassociative-math`, so its own results vary with compiler vectorization. Regression outputs must still match exactly. |
| Performance | **Parallel index builds are in scope. No performance gate.** | Without parallel builds, index builds on a multi-core machine are many times slower than pgvector's. Build time is measured and reported. |
| Approach | **Faithful port + parallel builds on the morsel pool** (ruling M6-1 replaced the pool with launched workers; see §7.1) | Byte compatibility holds by construction, and the code follows repo conventions (C names, file:line citations, C line numbers in errors). Porting Postgres' DSM/bgworker parallel-build machinery and a clean-room redesign were both rejected. |
| Where work lives | Feature branches in the `roadius2/pgrust` fork | Upstream does not accept PRs. |

### 2.1 On-disk direction policy

- **Forward (pgvector → pgrust) is a gate.** It covers both a cleanly stopped data directory and a crash-stopped one. The crash case requires replaying pgvector's GenericXLog records, which a future pgrust standby of a C primary would also need.
- **Reverse (pgrust → C) is a non-gating report in Phase 1.** It becomes a gate the first time any of these is needed:
  1. a way back from pgrust without dump/restore;
  2. mixed physical replication with a pgrust primary and a C standby;
  3. C physical tooling (base backups, `pg_upgrade`) run on data pgrust wrote.
  
  The reverse direction also depends on pgrust core writing pages and catalogs that C can read. That is outside this spec, and the report will show whether it holds.
- **Design rule for all phases:** type and heap formats are frozen at pgvector's. New pgrust access methods may only add *index* formats. Vectors then stay readable by any pgvector server, and pgrust-only indexes are dropped or rebuilt.

## 3. Scope

**In scope**
- **Types:** `halfvec` and `sparsevec`, plus the `bit` functions (`hamming_distance`, `jaccard_distance`, `<~>`, `<%>`).
- **Everything in upstream `vector--0.8.7.sql`**, including every cast, the `avg`/`sum` aggregates for `halfvec`, and `binary_quantize`/`subvector`/`l2_normalize` for every type.
- **HNSW:** opclasses for all four types, and the 0.8.7 behavior changes.
- **IVFFlat:** the access method, its opclasses, the `lists` reloption, and the `ivfflat.probes`, `ivfflat.iterative_scan` and `ivfflat.max_probes` settings.
- **Parallel builds:** HNSW and IVFFlat, using `max_parallel_maintenance_workers`.
- **Extension files:** upgrade scripts 0.8.1 → … → 0.8.7. 0.8.1 is the first version supporting PG18. (M4 ships all 41 upstream upgrade scripts, as C's `DATA` installs them.)
- **Test infrastructure:** reference builds, the harness and fixtures (§8).

**Non-goals for Phase 1**
- SIMD distance kernels. Per the survey, system overhead rather than distance math dominates in-database cost; kernels belong in Phase 4.
- New access methods, quantization schemes and planner filtering (Phases 2–5).
- Kani proofs for new code. The existing `proofs/vector-io` must stay green.
- In-process fuzz targets against compiled pgvector C (a possible later addition).
- Any change to B-tree's parallel build pool or its kill switch.

## 4. Code layout

### 4.1 Reference source
- Vendor upstream pgvector 0.8.7 unmodified (`src/`, `sql/`, `test/`, `LICENSE`) at **`crates/pgvector-0.8.7-reference/`**. Like `crates/postgres-18.6-reference`, it is never compiled or edited, and it is not a workspace member.
- Port comments cite file:line in this tree, and its `test/` directory is the conformance suite.
- Delete the orphaned test files in `crates/contrib/pgvector/sql/`. They have no expected outputs and the vendored suite replaces them.

### 4.2 Types: extend the `pgvector` crate
- Add modules named after the C files: `halfvec.rs`, `halfutils.rs`, `sparsevec.rs`, `bitvec.rs`, `bitutils.rs`. The existing `vec.rs`/`funcs.rs` receive the 0.8.7 changes.
- They are modules rather than separate crates because every symbol resolves through the single `register_builtin_library("vector", lookup)` table. A pgvector data directory's catalog stores `probin = '$libdir/vector'` plus the C symbol name, so one library entry point is what makes on-disk boot work.
- No new external dependencies:
  - f16 conversion ports pgvector's software routine.
  - `bit` support builds on the existing `varbit` crate.
  - Float output uses the existing `ryu` crate.

### 4.3 HNSW: extend the existing crates
- `types_hnsw` gains `HnswTypeInfo`: max dimensions, normalize, value check, and the dimension-count function added in 0.8.7.
- `pgvector_hnsw` gains the 0.8.7 changes and type dispatch.
- `pgvector_hnsw_build` gains the opt-in build seed (§5.4) and a new `parallel_build.rs` module (§7).
- `crates/contrib/pgvector_hnsw_graph` (new in M6) holds the in-memory build graph, with C's lock protocol and a model of C's allocator; the serial build and every parallel participant insert through it (§7.2).

### 4.4 IVFFlat: three new crates, mirroring HNSW
- `crates/_support/types/types_ivfflat` holds the `ivfflat.h` vocabulary and the scan state. It lives here so `relscan`'s `IndexScanOpaque` can hold the scan state without a dependency cycle.
- `crates/contrib/pgvector_ivfflat` has modules for `ivfflat.c`, `ivfutils.c`, `ivfinsert.c`, `ivfscan.c` and `ivfvacuum.c`.
- `crates/contrib/pgvector_ivfflat_build` has `ivfbuild.c`, `ivfkmeans.c` and parallel builds.
- `crates/contrib/pgvector_parallel` (new in M6) holds what both AMs' parallel builds share: the launch and teardown, the worker prologue, the per-participant DEBUG1 lines and the kill switch (§7.1).

### 4.5 Core hook points for IVFFlat

These are the same places HNSW is wired in today. Core depends on access-method crates directly; the set of access methods is closed.

| Crate | Change |
|---|---|
| `relscan` (`crates/backend/access/index/relscan`) | `IndexAmKind::Ivfflat`; `IndexScanOpaque::Ivfflat` |
| `amapi` | `ivfflathandler` → kind; opclass validation |
| `indexam` | dispatch of insert, scan, bulk-delete and vacuum-cleanup |
| `catalog_index` | build and build-empty dispatch |
| `planner` `plancat.rs` | access-method capabilities (order-by-operator yes, bitmap no) |
| `planner` `selfuncs.rs` | `ivfflatcostestimate`, next to `hnswcostestimate` |
| `relcache_build` `index.rs` | support-proc count (5) |
| `reloptions` | `RELOPT_KIND_IVFFLAT`; `lists` (1–32,768, default 100) |
| `guc_tables` | `ivfflat.probes`, `ivfflat.iterative_scan`, `ivfflat.max_probes`; storage installed by `pgvector_ivfflat::init_seams` |
| `seams_init` | call `pgvector_ivfflat::init_seams()` |
| `types_error` `source_map_table.rs` | crate → C file entries for the new crates |

### 4.6 Extension SQL
- At the end of M4, replace the trimmed `vector--0.8.5.sql` with upstream's verbatim `vector--0.8.7.sql`, plus the upgrade scripts `vector--0.8.1--0.8.2.sql` … `vector--0.8.6--0.8.7.sql`, and set `default_version = '0.8.7'`. (Done in M4: extension/ is upstream's, byte for byte, which `shipped_scripts_are_upstream_verbatim` checks.)
- Until then, the trimmed script grows with each milestone. pgrust's `CREATE FUNCTION` checks the lookup table, so the verbatim script can only ship once every symbol exists.
- **Lookup-coverage unit test:** parse every shipped script and assert that every `MODULE_PATHNAME` symbol resolves in `lookup`. (In place since M2: `lookup_covers_every_module_pathname_symbol` in `crates/contrib/pgvector/src/lib.rs`; since M4 it reads every shipped file.)
- **Wasm:** the pgvector crates build for `wasm32-wasip1` today. New crates must too, and parallel builds fall back to serial there.

## 5. Types and the 0.8.7 catch-up

### 5.1 Bringing existing code to 0.8.7
Corrected on 2026-10-05 against `git diff v0.8.5 v0.8.7` (port base `159b79a` is the `v0.8.5` tag):
- **HNSW dimension count:** `HnswCheckDim` raises `expected %d dimensions, not %d` (`ERRCODE_DATA_EXCEPTION`, `hnswutils.c:1366-1374`).
  - Build checks against the column typmod (`buildstate->dimensions`, `hnswbuild.c:503-504`).
  - Insert (`hnswinsert.c:713-717`) and scan (`hnswscan.c:39-44`) check against the metapage (`HnswGetMetaPageInfo(..., &dimensions, ...)`).
  - The scan checks only a non-NULL query value, and checks it before returning early on an empty index, so a wrong-dimension query errors even on an empty index.
- **Level RNG:** a uniform draw of `0.0` returns the maximum level instead of computing `-log(0)` (`hnswutils.c:250-255`). The port already behaves this way because the `inf as i32` cast saturates; M1 makes it explicit. `HnswGetMaxLevel` caps at **63**, down from 255 (`hnsw.h:133`, 0.8.6).
- **Neighbour updates:** `UpdateNeighborOnDisk` always checks whether a connection already exists, and the `checkExisting` parameter is removed (upstream #1010).
- **`vector_combine`:** it branches on the partial states' dimensions rather than their counts, and calls `CheckDim` on every non-empty side (`vector.c:1211-1284`).
  - The 0.8.7 changelog's "error with `avg` aggregate when no matching rows" bug came in with 0.8.6 and never existed in 0.8.5 or in the port.
  - Only `avg` uses `vector_combine`. `sum` combines with `vector_add`, and `halfvec_combine` is an SQL alias of `vector_combine`.
- **Size arithmetic:** the only reachable site is `HnswLoadNeighborTids`' `mul_size(element->level - lc, m)` (`hnswutils.c:789`). A corrupt metapage entry level makes it raise instead of indexing out of bounds. The other `add_size`/`mul_size` sites have bounded inputs and stay plain arithmetic.
- **Version:** the extension stays at the trimmed `vector--0.8.5.sql` and `default_version = '0.8.5'` until M4 (§4.6). C also reports a library version of 0.8.7 through `PG_MODULE_MAGIC_EXT` (`vector.c:49`, visible in `pg_get_loaded_modules()`), while pgrust reports 18.6 for every built-in library. That gap is recorded as a `DIVERGENCE` and revisited in M4. (M4: `default_version = '0.8.7'`, and `pg_get_loaded_modules()` reports 0.8.7 through dfmgr's library-version registry.)

### 5.2 Type layouts (byte-identical to C)

| Type | Varlena payload | Limits | Text form | Binary send/recv |
|---|---|---|---|---|
| `vector` | `i16 dim, i16 unused, f32 x[dim]` | 16,000 dims | `[1,2,3]` | (existing) |
| `halfvec` | `i16 dim, i16 unused, f16 x[dim]` | 16,000 dims; values within ±65,504 | `[1,2,3]` | dim, unused, raw f16 bits (`pq_sendhalf`) |
| `sparsevec` | `i32 dim, i32 nnz, i32 unused, i32 indices[nnz], f32 values[nnz]` | dim ≤ 1e9, nnz ≤ 16,000 | `{1:1.5,3:2}/5` (1-based) | int32 dim/nnz/unused, int32 **0-based** indices, f32 values; zero values rejected on receive |
| `bit` | Postgres' built-in `bit`/`varbit` | n/a | n/a | n/a |

Every error message is copied verbatim with C's SQLSTATE, about 20 per type. Text input and binary receive have different validation rules (sorting, duplicates, zero values). Each is ported as written, not unified.

### 5.3 Numeric fidelity
- **Parsing** reuses the float parser the existing `vector_in` uses.
- **f32 ↔ f16** conversion ports pgvector's software round-to-nearest-even path. Because IEEE defines it exactly, it gives the same bits as C's F16C or `_Float16` hardware paths. Stored bytes are therefore identical to pgvector's, and the exhaustive unit test (§8.2) checks this.
- **Distances** use scalar loops in C's order, accumulating in `float` or `double` exactly where C does.
- **Output** uses `ryu` shortest representation as C does. `halfvec` prints the float value of the stored half.

### 5.4 HNSW across all four types
- **Support proc 3 (type info)** resolves by its C symbol (the `pg_proc.prosrc` of a `LANGUAGE c` function, `pgvector::support_proc_symbol`) to a static Rust table, and is never called through fmgr. This is in place since M3 as `hnsw_get_type_info` (`pgvector_hnsw/src/utils.rs`), with the `LANGUAGE c` check since M4. It knows the symbols below; any other raises 0A000, where C would call it. The symbols:
  - `vector`: no proc 3 (built-in default)
  - `hnsw_halfvec_support`
  - `hnsw_bit_support`
  - `hnsw_sparsevec_support`
- **Limits:**
  - max dimensions: vector 2,000, halfvec 4,000, bit 64,000;
  - sparsevec: at most 1,000 non-zeros (`HNSW_MAX_NNZ`).
- **New opclasses:**
  - `halfvec_{l2,ip,cosine,l1}_ops`
  - `bit_{hamming,jaccard}_ops`
  - `sparsevec_{l2,ip,cosine,l1}_ops`
- **Build seed (corrected 2026-10-05):** C seeds the level generator only when pgvector is compiled with `-DHNSW_MEMORY`. That is a debug flag that also prints `INFO: memory:` lines; the seed is `SeedRandom(42)` at `hnswbuild.c:1134-1136`.
  - Stock C builds, including the Docker image and the reference, draw levels from the backend's `InitProcessGlobals` seed, so they are not reproducible. Two stock builds of the same table differ.
  - pgrust matches stock C by default.
  - The pgrust-only placeholder option `pgrust.hnsw_build_seed` makes a build call `SeedRandom(<value>)` at the same point C does. It is unregistered, so it adds no `pg_settings` row; it is read through `guc_seams::get_config_option_missing_ok`, the way `guc_tables/src/gather_fair.rs` reads its option. With 42 it reproduces C's `-DHNSW_MEMORY` build, including that build's reseeding of the backend's `pg_global_prng_state`. The byte-identical and iterative-scan tiers (§8.2) use it.
  - IVFFlat is similar: `SeedRandom(42)` runs only under `#ifdef IVFFLAT_BENCH` (`ivfbuild.c:1075-1077`). Since M4, `pgvec-seeded` is also built with `-DIVFFLAT_BENCH`, and the placeholder `pgrust.ivfflat_build_seed` makes `ivfflatbuild` call `SeedRandom(<value>)` there.
- **Tie order:** C's search heaps are pairing heaps (`hnswutils.c:632-672`). On distance ties their pop order decides neighbour lists and the order of tied scan results, so the port uses the C-exact `pairingheap` crate rather than binary heaps.

## 6. IVFFlat (serial port)

- **Opclasses:** `vector_{l2,ip,cosine}_ops`, `halfvec_{l2,ip,cosine}_ops` and `bit_hamming_ops`. As in pgvector, there is no `sparsevec` and no L1.
- **Support procs:** 1 distance, 2 norm, 3 k-means distance, 4 k-means norm, 5 type info. Proc 5 resolves by its C symbol, as HNSW's proc 3 does (`ivfflat_get_type_info`, `pgvector_ivfflat/src/utils.rs`).
- **Build:**
  1. Sample rows with the block sampler plus reservoir: `lists × 50` samples, at least 10,000, capped at `blocks × MaxHeapTuplesPerPage`, and 1 for unlogged init forks.
  2. Run serial Elkan k-means.
  3. Write the metadata and list pages.
  4. Assign every row to its nearest center, sort by list, and write entry pages.
  
  Progress subphases and NOTICE texts match C.
- **Insert:** find the nearest center and append to that list's insert page using GenericXLog.
- **Scan:** probe the nearest `probes` lists. With iterative scan on, continue in batches up to `max(ivfflat.max_probes, probes)`. Candidates are sorted by distance with a scan-local tuplesort.
- **Vacuum:** bulk-delete and cleanup are ported as-is.
- **Cost:** `ivfflatcostestimate` in `selfuncs.rs`.

## 7. Parallel builds

### 7.1 Shared mechanics
- **Worker count:** port `plan_create_index_workers` (M6: `planner/src/index_workers.rs`, behind `planner_seams::plan_create_index_workers`; before M6, `catalog_index` built every index serially). It considers `max_parallel_maintenance_workers`, table size, `min_parallel_table_scan_size` and the `parallel_workers` reloption. Phase 1 applies it to vector access methods only.
- **Workers** (ruling M6-1, the M6 plan's Decision 1; it replaces the morsel pool that this section and §2's Approach row first chose):
  - Workers are launched backends from the thread-native `parallel` crate: `CreateParallelContext` → `InitializeParallelDSM` → `LaunchParallelWorkers` → `WaitForParallelWorkersToFinish` → `DestroyParallelContext`, C's shape. Each worker is a full backend: it restores the leader's snapshot, GUCs and lock group, opens its own relations, and its messages reach the client with `CONTEXT:  parallel worker`. Both AMs share the plumbing in `crates/contrib/pgvector_parallel`.
  - Why not the morsel pool: its smallest claim is one heap block against a 2 ms target (`runtime/src/sizing.rs`), and one block of HNSW inserts can take ten times that; it falls back to serial when no thread claims the work within 100 ms (`nbtsort/src/pool.rs`), so whether a build went parallel would depend on load, while 36 of the 37 TAP lines M6 had to pass check C's `using N parallel workers`; launched workers bring C's error, cancel and cleanup paths. Neither mechanism spawns raw threads. A pool channel remains possible later, in vacuumlazy's shape, without changing the per-row work.
  - The entry points register under `"postgres"` with pgrust names (`pgrust_hnsw_parallel_build_main`, `pgrust_ivfflat_parallel_build_main`): `LookupParallelWorkerFunction` accepts only `"postgres"`. DIVERGENCE: C's are `"vector"`'s `HnswParallelBuildMain` and `IvfflatParallelBuildMain`; neither name is visible to SQL.
  - The leader participates, as in C.
  - When no worker launches, the build continues serially, as C's does. wasm always builds serially (`IsUnderPostmaster` is false there).
  - The kill switch `PGRUST_VECTOR_INDEXBUILD_POOL=0` disables parallel vector builds. It is on by default and named after B-tree's opt-in `PGRUST_RUNTIME_INDEXBUILD_POOL`.
- **No raw thread spawns.** The determinism lint (`seams_init --test lint_determinism`) must stay within budget.

### 7.2 HNSW: shared in-memory graph
- **Graph storage:** C keeps the graph in DSM using `relptr` and an `LWLock` per element, plus `entryLock`, `entryWaitLock`, `allocatorLock` and `flushLock`. pgrust threads share an address space, so the graph becomes a shared arena with u32 element handles (the existing serial build already uses them to mirror C's `relptr`) and the same lock set, built on `pgsync`. `pgsync` is the only lock library allowed, and it supports `cfg(loom)`.
- **Same algorithm as C:**
  - Participants insert concurrently into the in-memory graph, with total graph memory bounded by `maintenance_work_mem`.
  - When the graph overflows, the `flushed` flag is set, the participant that found it full flushes the graph to disk (the leader flushes at the end if none did), and all participants continue inserting on disk under normal buffer locking.
  - NOTICE text is unchanged: `hnsw graph no longer fits into maintenance_work_mem after N tuples`.
- **The graph (M6)** is the leaf crate `crates/contrib/pgvector_hnsw_graph`, which depends only on `pgsync` and `pairingheap`, so its loom models (`tests/loom.rs`) compile the real lock protocol. The serial build inserts through it too, as C has one code path for both.
- **Memory accounting** (the M6 plan's Decision 3): the graph stores Rust values and charges C's allocation sizes to a model of C's allocator: a Generation context with 1 MB blocks in a serial build, MAXALIGNed bump allocations from `maintenance_work_mem` − 3 MB with a 1 MB margin in a parallel build. So a build flushes where C's does. At 1 MB the serial `spill` build flushes after 1,436 tuples, as C's seeded build does (unit test `serial_flush_point_matches_c_at_one_megabyte`; bytecmp's `spill`), and TAP `045`'s parallel build at `3073kB` flushes after 0 tuples. DIVERGENCE: the bytes actually held differ from C's, and the arena is freed with the build, not at the flush (C resets the serial graph's memory context there).
- **Determinism:** parallel builds are nondeterministic, as in C. Recall is the check.

### 7.3 IVFFlat
- The leader computes centers serially, as C does.
- Workers scan and assign rows to their nearest center, which costs O(rows × lists × dims) and is the expensive step. They stream `(list, tid, value)` to the leader, which feeds a single tuplesort and writes the pages.
- This differs from C, where each worker builds its own sorted run and the leader merges them. pgrust's `Tuplesort` is tied to the thread that created it (documented in `nbtsort/src/pool.rs`), and sorting is cheap next to assignment.
- The effect is limited to tuple order within a list, which C's parallel build doesn't fix either.
- The parallel context ends right after the scan (M6), since no shared tapes need it. Each worker streams its batches through its own bounded mailbox, which the leader drains into the sort (M6: `pgsync`'s mailbox has a single sender-wake slot, so one shared mailbox could lose a parked worker's wake).

## 8. Testing and verification

### 8.1 Reference builds
- **Local reference (dev loop):** PostgreSQL 18.6 built from `crates/postgres-18.6-reference`, plus pgvector 0.8.7 built from `crates/pgvector-0.8.7-reference` with the Docker image's compiler flags (`make OPTFLAGS=""`). `scripts/pgvector/build-reference.sh` builds two installs under `$PGREF` (default `~/.cache/pgrust/pgref-18.6`):
  - `pg`: PostgreSQL 18.6 only. pgrust reads its share directory and uses its `initdb`, `pg_ctl`, `psql` and `pg_regress`.
  - `pgvec`: a copy of `pg` plus C pgvector, used as the reference server. It's kept separate so pgrust never sees C pgvector's control or upgrade scripts.
  - `pgvec-seeded` (added in M1): the same, but pgvector is built with `-DHNSW_MEMORY`, the only C configuration that seeds HNSW builds (§5.4) and, since M4, `-DIVFFLAT_BENCH`, the only one that seeds IVFFlat builds. It is the oracle for the byte-identical and iterative-scan tiers, and nothing else uses it.
  
  (An earlier draft named the single prefix `/tmp/pgrust_pginstall`, which `crates/backend/commands/matview/tests/refresh_freeze_e2e.rs` probes. That test can be pointed at `$PGREF/pg` through `PGINSTALL`.) This build is needed for two reasons:
  - C `pg_ctl` and `initdb` require the `postgres` binary's version string to match theirs exactly, so the C tools must be exactly 18.6.
  - Using the same OS as pgrust avoids cross-OS problems when exchanging data directories.
  
  It supplies `pg_regress`, the TAP Perl modules (`src/test/perl`) and the reference server.
- **Docker `pgvector/pgvector:0.8.7-pg18`** serves the on-disk gate. It is the real-world artifact (Debian PGDG packages, PostgreSQL 18.6, gcc) and runs on Linux, either in a container or on a VM. Clusters are created with `initdb --no-locale --encoding UTF8` to avoid collation differences. Since M5 the on-disk tier also uses **`pgvector/pgvector:0.8.1-pg18`** (PostgreSQL 18.2) for its upgrade fixture. Both are pinned by digest in `scripts/pgvector/common.sh`.

### 8.2 Tiers

| Tier | Method | Pass rule | Gated |
|---|---|---|---|
| Unit | Per-crate tests. Exhaustive f16 parity (test crate `pgvector_f16_parity`, whose `build.rs` compiles the vendored `halfutils.h` routines and the compiler's `_Float16` path): all 2³² f32 and 2¹⁶ f16 inputs, plus every pair of halves through + − ×. Lookup-coverage test (§4.6). Loom tests for the HNSW graph lock protocol. | All pass | Yes |
| Regression | `pg_regress` in installcheck mode against a running pgrust server, as pgvector's `make installcheck` runs it (`--inputdir=test --load-extension=vector`), with inputs and expected outputs from `crates/pgvector-0.8.7-reference/test/` (14 files) | Byte for byte. Expected outputs are never edited. Any `-- pgrust:` annotation follows the `regress/overlay` contract and gets a written ruling. | Yes |
| TAP | `prove` on the 48 `test/t/*.pl` files using a hybrid setup: C `initdb` sits beside C `postgres` for bootstrap, while `pg_ctl` sits beside pgrust's `postgres`, so clusters are created by C and run by pgrust | Pass, or a written reason per exclusion (for example, the WAL tests need pgrust-to-pgrust streaming replication) | Yes |
| Exact differential | diffrunner's `--pgvector` suite (`crates/bin/fuzzgen/src/pgvector.rs`: a fixed SQL deck plus a seeded random arm), run by `scripts/pgvector/run-diff.sh`. Covers functions, casts, operators, aggregates, send output (bytea and binary COPY), error cases and exact (non-index) nearest-neighbour queries over integer data, with special values: zero vectors, denormals, −0, f16 range edges, dimension and nnz limits, NaN/inf rejection. Also compares the LOCATION of every SQL-reachable error in vector.c, halfvec.c, halfutils.h, sparsevec.c and bitvec.c (`scripts/pgvector/sql/error-locations.sql`, `sqldiff.sh --verbose`), every SQL-reachable HNSW error site for all four types (`scripts/pgvector/sql/hnsw-errors.sql`, M3), every SQL-reachable IVFFlat error site (`scripts/pgvector/sql/ivfflat-errors.sql`, M4), the core error sites pgvector SQL reaches (reloptions.c, guc.c, float.c, mcxt.c; `scripts/pgvector/sql/core-error-locations.sql`), where a session first loads vector (`scripts/pgvector/sql/library-load.sql`), and parallel IVFFlat and HNSW builds (`scripts/pgvector/sql/parallel-build.sql`, M6: index.c's request line, `using N parallel workers`, the builds that must stay serial, CREATE INDEX CONCURRENTLY, the overflow NOTICE, a row error, a statement timeout, duplicates and the built indexes' answers; `sqldiff.sh` drops each participant's `leader/worker processed N tuples` line and every `CONTEXT:  parallel worker` line, whose counts and order vary C against C); the binary-only recv sites are outside it (M2 report). | Identical. In distance and norm statements only, numerals that are non-integral on either side may differ within 1e-5 relative, floored at magnitude 1 (ruling `pgvector-float-rel`: pgvector's own f32 sums depend on compiler reassociation and FMA), and a zero of either sign counts as within tolerance (C's sign of a sum whose products all underflow follows its vectorized, fused reduction order; user-approved 2026-10-06). `l2_normalize`, the denormal cases the 1e-5 floor would hide (an exact deck section), row order, comparison results, send output and all other values compare exactly. Other accepted divergences go in `docs/fuzzing/rulings.toml`. | Yes |
| Approximate differential (M7) | `scripts/pgvector/run-recall.sh`, with its data from `recall-data.sh`: GloVe 6B word embeddings at 100 dimensions (`sparsevec` keeps 16 entries), split into 100,000 base rows and 1,000 held-out queries, loaded identically into stock C pgvector and the subject. The baseline is always stock C. Fifteen cases, each serial and parallel: HNSW for `vector`, `halfvec`, `sparsevec` and `bit`, and IVFFlat for `vector`, `halfvec` and `bit` (IVFFlat has no `sparsevec` opclass), plus an HNSW `vector` build that spills past `maintenance_work_mem`. Exact answers come from the baseline's sequential scan before any index exists, and the truth is tie-tolerant: a returned row counts when its distance is within 1e-5 of the true k-th distance. Each case builds three times on each side and compares the medians of recall@10 and @100. A case dies, leaving no `summary.tsv`, if a build fails, a parallel case builds serially, a plan is not an index scan, or a search returns a row twice. The `exact_*` rows (one per type) compare the subject's own sequential-scan answers for the first 100 query ids against C's, within 1e-5. A row also fails when the baseline's median falls outside the band 0.5 to 0.995, where a comparison cannot detect a difference. 34 rows: 15 cases at two depths, and four `exact_*` rows. | pgrust recall ≥ pgvector recall − 0.01 | Yes |
| Byte-identical index | Serial HNSW builds with seed 42, mostly on integer-valued data, where float math is exact or rounds once: `pgvec-seeded` against pgrust with `pgrust.hnsw_build_seed = 42`. Index files are compared after masking both with `crates/bin/pagemask generic`. Cases cover tie-free and tie-heavy data, `m = 4`, 2,000 dimensions, an unlogged init fork, inserts after the build, a 10,000-row build whose C graph spans several 1 MB blocks, and (since M6) a 2,003-row build that spills past `maintenance_work_mem` at C's flush point (`spill`: both servers flush after 1,436 tuples at 1 MB). Since M3 they also cover every `halfvec`, `sparsevec` and `bit` opclass family: integer grids, `bit(64)` from md5, a 1,000-non-zero `sparsevec` across pages, a `binary_quantize` expression index, and cosine and fractional `sparsevec` cases whose sums C and pgrust round alike. Two cases are opt-in known divergences: `vector` cosine (C's `-ffp-contract=fast`) and `insert_grid`. In that last case, on-disk inserts into a tie-heavy graph meet C's pointer tie-break, which is not reproducible even C against C. Since M4 the tier also covers IVFFlat, with pgrust setting `pgrust.ivfflat_build_seed = 42`. Its `vector` and `halfvec` cases use well-separated groups of integer points, so every k-means comparison has a clear winner. Its `bit` case uses `bit(64)` from md5, whose integer Hamming distances tie often, so it pins `bit`'s tie order. The eight cases cover reservoir sampling, `halfvec`, `bit`, inserts that append pages, an unlogged init fork, vacuum and 2,000 dimensions. | Every default case identical | HNSW and IVFFlat yes |
| Iterative-scan stop points (M1) | The same seeded HNSW indexes on both servers (tie-free `tst` and `big`; tie-heavy per-type tables `hv`, `sv` and `bv`). Iterative scans whose stop point depends on `hnsw.max_scan_tuples` or on the scan memory cap (`MemoryContextMemAllocated(tmpCtx) > work_mem × hnsw.scan_mem_multiplier`, `hnswscan.c:264`), including a rescan through `LATERAL`. Every case also compares the memory trace: the seeded build prints each search batch's `tmpCtx` size (KB) and tuple count (`ShowMemoryUsage`), and pgrust prints the same line under the opt-in placeholder `pgrust.hnsw_memory`. Since M3 the cases cover all four types, a 10,000-row table and `hnsw.ef_search` variants. | Same rows in the same order, the same `Rows Removed by Filter`, and the same memory lines | Yes |
| On-disk, forward | `scripts/pgvector/run-ondisk.sh` (M5). C pgvector writes three directories in Docker: **clean** (0.8.7 image, clean stop), **crash** (0.8.7 image; after a `CHECKPOINT`, on-disk inserts into every index, deletes and `VACUUM`, then a transaction left open, then `pg_ctl stop -m immediate`) and **v081** (0.8.1 image, clean stop). The fixture (`sql/ondisk-fixture.sql`) has all four types, every HNSW and IVFFlat opclass (25 vector indexes), a `binary_quantize` expression index, an unlogged table, a TOASTed 1,536-dimension table, deletes, `VACUUM` and `ANALYZE`, and every row value keeps the distances that index scans return exact (`sql/ondisk-rows.sql`); only IVFFlat's query-to-center distances round (see `docs/superpowers/reports/pgvector-m5-report.md`). C's answers come from the 0.8.7 image booting a copy of the same directory (for v081, after the same `ALTER EXTENSION vector UPDATE`). pgrust boots another copy, reruns the fixture queries (`sql/ondisk-queries.sql`), then runs inserts, deletes, `VACUUM` and `REINDEX` (`sql/ondisk-followup.sql`). A capture that did not exercise what this row claims is refused before use: the clean, crashed and 0.8.1 directories, all 42 relations and 25 vector indexes, the TOASTed column, the open transaction, only C's intended errors, and replay changing every vector index's pages (`scripts/pgvector/ondisk-capture.sh`, `capture_check`). | Query results **identical to C's on the same directory, including approximate-search results**: the same index pages give the same traversal. After the crash, the main and `_init` forks of every fixture relation also replay to the same pages as C's replay (after `pagemask`). After the follow-up, counts, index-scan row counts and exact (non-index) answers equal C's after the same follow-up; index answers are not compared row by row, since new elements draw random levels. Seven rows. | Yes |
| On-disk, reverse | pgrust writes the same fixture into a fresh cluster, once stopped cleanly and once crashed the same way; Docker pgvector boots each. On the cleanly stopped one it reruns the queries and the follow-up, runs `pg_amcheck` and lists pgrust-only catalog rows. On the crashed one it reruns the queries and compares its replay with pgrust's own | Reported (`reverse.md`; `run-all.sh` prints it) | No |

### 8.3 Harness location
- `scripts/pgvector/` (following the `scripts/` convention that repo comments assume), containing:
  - `build-reference.sh`
  - `run-regress.sh`
  - `run-tap.sh`
  - `run-ondisk.sh`
  - `run-bytecmp.sh` and `run-iterscan.sh` (M1)
  - `run-diff.sh` (M2) and `sqldiff.sh` (M2); `upstream-sql.sh` (M2) was removed in M4, once the install script was verbatim
  - `sql/hnsw-errors.sql` (M3)
  - `sql/ivfflat-errors.sql` (M4)
  - `sql/core-error-locations.sql` and `sql/library-load.sql` (M4 follow-ups)
  - `run-repair-check.sh` (M4 follow-ups)
  - `docker-lib.sh` and `sql/ondisk-*.sql` (M5; `run-ondisk.sh` is the tier), and `ondisk-capture.sh` (M5 follow-ups: the capture, its cache key and its self-checks)
  - `run-recall.sh` and `recall-data.sh` (M7: the approximate differential and its GloVe dataset)
  - `sql/parallel-build.sql` (M6; the diff tier's `parallel_build` row) and `run-buildtime.sh` (M6: build time against C, a report, not a gate)
  - fixture SQL
  - a wrapper that runs every gated tier and prints a pass/fail table
- The repo has no CI config, so the wrapper is the gate.

### 8.4 Prerequisites
- Building pgrust: `brew install re2 pkg-config`.
- Building the reference: ICU (or `--without-icu`), plus bison and flex if the vendored tree lacks generated files (check in M0).
- A running Docker daemon or a Linux VM with Docker.
- Disk space: currently 171 GB free.

## 9. Milestones

| # | Milestone | Exit criteria |
|---|---|---|
| M0 | Vendor the pgvector reference; build the local reference; write the `scripts/pgvector/` runners. **Spikes:** (1) the hybrid TAP setup running one pgvector TAP test end to end; (2) the regression suite against current pgrust. | One TAP test passes end to end; a baseline report of what passes today |
| M1 | 0.8.7 catch-up for `vector` and HNSW; the 016 page-chain fix; the opt-in build seed; C's pairing-heap tie order; exact iterative-scan memory accounting | Regression files `vector_type` and `hnsw_vector` pass; TAP `016`, `043` and `044` pass; the byte-identical HNSW tier and the iterative-scan tier pass for `vector` |
| M2 | `halfvec`, `sparsevec`, `bit` functions | Regression files `halfvec`, `sparsevec`, `bit`, `cast`, `btree` and `copy` pass (`btree` and `copy` also cover `halfvec` and `sparsevec`, so they move here from M1); exhaustive f16 test passes; exact differential is clean |
| M3 | HNSW type info and opclasses for all types | Regression files `hnsw_bit`, `hnsw_halfvec` and `hnsw_sparsevec` pass, plus all HNSW TAP tests (serial builds) |
| M4 | IVFFlat serial port; switch to the verbatim 0.8.7 script and upgrade scripts | Regression files `ivfflat_bit`, `ivfflat_halfvec` and `ivfflat_vector` pass, plus all IVFFlat TAP tests (serial builds); lookup-coverage test passes. All 14 regression files now pass. |
| M5 | On-disk tiers | Forward gate passes (clean, crash, 0.8.1 upgrade); reverse report produced |
| M6 | Port `plan_create_index_workers`; IVFFlat parallel build; HNSW shared graph. **Spike first:** the morsel pool hosting heavy per-row worker jobs (ruling M6-1 answered it from the code and replaced the pool with launched workers; see §7.1). | Loom tests pass; recall TAP tests pass with workers forced; build time against pgvector reported |
| M7 | Approximate differential; final run of every gated tier through the wrapper; update `CLAUDE.md` | Everything gated is green |

**Ordering notes**
- Correctness comes before parallelism, so the serial build is the reference for judging parallel builds.
- Some milestones can overlap: IVFFlat for `vector` can start alongside M2, and M5 can overlap with M6.
- Estimated scale: about 4.5k lines of C left to port, roughly 8–10k lines of Rust, and about 10 small core edits.

## 10. Risks

| Risk | Mitigation |
|---|---|
| The hybrid TAP setup doesn't work (the repo's old TAP runners are missing) | Spike in M0, before anything depends on it |
| Concurrency bugs in the shared HNSW graph | Loom tests on the lock protocol, serial fallback, kill switch, recall tests run with workers forced |
| The morsel pool's thread-bound relation handling may not suit heavy per-row worker jobs | Spike at the start of M6 (ruling M6-1 replaced the pool with launched workers; see §7.1) |
| pgvector's tests expose pgrust core gaps (pgrust-to-pgrust replication, `EXPLAIN` details, index-build progress views) | Fix in core only if the fix is small and needed for a gate; otherwise record a test exclusion with a reason and a note |
| Upstream drift (pgvector 0.8.8, pgrust `main`) | Keep core edits to the §4.5 hook points; merge each upstream release into `main` on its own branch under the merge policy (`docs/HANDOFF_2026-10-22.md` §3: rerun every gate, get the resolutions independently reviewed, then fast-forward `main`); watch whether upstream starts its own vector work |
| Tolerance-based comparison hides real bugs | Keep the tolerance tight; the byte-identical tier and the on-disk "identical approximate results" rule act as backstops |
| Build artifacts fill the disk | Watch `target/`; remove unused profile directories |

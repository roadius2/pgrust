# pgvector conformance after M2

Date: 2026-10-06 · pgrust commit: aa29a4ec90 (code: the server as of d8208e1a9a, plus the diffrunner comparator change in aa29a4ec90; the docs commit follows it) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY)

## Summary
- **C reference:** 94/94 (regress 14/14, TAP 48/48, bytecmp 10/10, iterscan 7/7, diff 15/15).
- **pgrust:** 56/94 (regress 8/14, TAP 16/48, bytecmp 10/10, iterscan 7/7, diff 15/15).
  - M1 was 33/79, before the `diff` tier existed.
  - On regress+TAP, pgrust went from 16/62 to 24/62.
- **Fixed in M2:**
  - regress `bit`, `btree`, `cast`, `copy`, `halfvec`, `sparsevec`;
  - TAP `033_comparison`, `034_distance_functions`;
  - the new exact-differential tier (`diff`, `scripts/pgvector/run-diff.sh`) is clean.
- **Short of the plan's 57/94.** TAP `018_aggregates` still fails. Its cause is pgrust's parallel worker count, which is outside M2's code. It is open and awaiting the user (see "Known divergences left open").
- **Gate also green:**
  - `harness_test.sh`: all passed (71 ok).
  - `UT_M2`: 27 passed (lookup 2, bitutils 2, bitvec 2, funcs 2, halfutils 3, halfvec 6, sparsevec 10; 0 in `main_main`).
  - `UT_PARITY`: 3 passed.
  - `UT_FUZZ pgvector:: rulings:: ruled::`: 21 passed (8 + 10 + 3).
  - `lint-determinism`: PASS (0 violations, 41 warnings).
  - `lint-seam-installs`: PASS (0 violations, 22 allowlisted).
  - `build-reference.sh --verify`: ok (11 checks).
  - Both reference trees print nothing from `git status --porcelain --ignored`.
- **How the run was produced.** The table below comes from a full `run-all.sh pgrust` on the final binaries, after the ±0 comparator change (aa29a4ec90).
  - An earlier full run on the same server, with the previous diffrunner, gave 55/94.
  - The only row that differed was `diff | seeded` (FAIL; 6 findings, all ±0, see "What M2 established").

## pgrust results
| Tier | Test | Result |
|---|---|---|
| regress | bit | ok |
| regress | btree | ok |
| regress | cast | ok |
| regress | copy | ok |
| regress | halfvec | ok |
| regress | hnsw_bit | FAIL |
| regress | hnsw_halfvec | FAIL |
| regress | hnsw_sparsevec | FAIL |
| regress | hnsw_vector | ok |
| regress | ivfflat_bit | FAIL |
| regress | ivfflat_halfvec | FAIL |
| regress | ivfflat_vector | FAIL |
| regress | sparsevec | ok |
| regress | vector_type | ok |
| tap | 001_ivfflat_wal | FAIL |
| tap | 002_ivfflat_vacuum | FAIL |
| tap | 003_ivfflat_vector_build_recall | FAIL |
| tap | 004_ivfflat_vector_insert_recall | FAIL |
| tap | 005_ivfflat_query_recall | FAIL |
| tap | 006_ivfflat_lists | FAIL |
| tap | 007_ivfflat_inserts | FAIL |
| tap | 008_ivfflat_centers | FAIL |
| tap | 009_ivfflat_filtering | FAIL |
| tap | 010_hnsw_wal | ok |
| tap | 011_hnsw_vacuum | ok |
| tap | 012_hnsw_vector_build_recall | FAIL |
| tap | 013_hnsw_vector_insert_recall | ok |
| tap | 014_hnsw_vector_vacuum_recall | ok |
| tap | 015_hnsw_vector_duplicates | ok |
| tap | 016_hnsw_inserts | ok |
| tap | 017_hnsw_filtering | ok |
| tap | 018_aggregates | FAIL |
| tap | 019_storage | ok |
| tap | 020_hnsw_bit_build_recall | FAIL |
| tap | 021_hnsw_bit_insert_recall | FAIL |
| tap | 022_hnsw_bit_vacuum_recall | FAIL |
| tap | 023_hnsw_bit_duplicates | FAIL |
| tap | 024_hnsw_halfvec_build_recall | FAIL |
| tap | 025_hnsw_halfvec_insert_recall | FAIL |
| tap | 026_hnsw_halfvec_vacuum_recall | FAIL |
| tap | 027_hnsw_halfvec_duplicates | FAIL |
| tap | 028_hnsw_sparsevec_build_recall | FAIL |
| tap | 029_hnsw_sparsevec_insert_recall | FAIL |
| tap | 030_hnsw_sparsevec_vacuum_recall | FAIL |
| tap | 031_hnsw_sparsevec_duplicates | FAIL |
| tap | 032_ivfflat_halfvec_build_recall | FAIL |
| tap | 033_comparison | ok |
| tap | 034_distance_functions | ok |
| tap | 035_ivfflat_bit_build_recall | FAIL |
| tap | 036_ivfflat_bit_centers | FAIL |
| tap | 037_inputs | ok |
| tap | 038_hnsw_sparsevec_vacuum_insert | FAIL |
| tap | 039_hnsw_cost | ok |
| tap | 040_ivfflat_cost | FAIL |
| tap | 041_ivfflat_iterative_scan | FAIL |
| tap | 042_ivfflat_iterative_scan_recall | FAIL |
| tap | 043_hnsw_iterative_scan | ok |
| tap | 044_hnsw_iterative_scan_recall | ok |
| tap | 045_hnsw_low_memory_build | FAIL |
| tap | 046_hnsw_vacuum_scan | ok |
| tap | 047_hnsw_vacuum_insert | ok |
| tap | 048_ivfflat_vacuum_insert | FAIL |
| bytecmp | l2_golomb | ok |
| bytecmp | l1_golomb | ok |
| bytecmp | m4_golomb | ok |
| bytecmp | wide_golomb | ok |
| bytecmp | unlogged_golomb | ok |
| bytecmp | insert_golomb | ok |
| bytecmp | l2_grid | ok |
| bytecmp | ip_grid | ok |
| bytecmp | dups | ok |
| bytecmp | insert_grid | ok |
| iterscan | relaxed_64k | ok |
| iterscan | relaxed_64k_x2 | ok |
| iterscan | strict_64k | ok |
| iterscan | relaxed_128k | ok |
| iterscan | relaxed_tuples | ok |
| iterscan | strict_tuples | ok |
| iterscan | lateral_64k | ok |
| diff | setup | ok |
| diff | io | ok |
| diff | typmod | ok |
| diff | cast | ok |
| diff | arith | ok |
| diff | distance | ok |
| diff | norm | ok |
| diff | agg | ok |
| diff | cmp | ok |
| diff | misc | ok |
| diff | bit | ok |
| diff | knn | ok |
| diff | limits | ok |
| diff | seeded | ok |
| diff | error_locations | ok |

**56/94 passed**

## Remaining failures
Every remaining failure is one of four things: HNSW on the new types (M3), IVFFlat (M4), parallel builds (M6), or TAP 018. I checked each against this run's logs.

**IVFFlat (M4):**
- regress `ivfflat_bit`, `ivfflat_halfvec` and `ivfflat_vector` diff only on `access method "ivfflat" does not exist`. `ivfflat_vector` also shows `unrecognized configuration parameter "ivfflat.probes"`.
- TAP `001`–`009`, `032`, `035`, `036`, `040`–`042` and `048` (16 tests) stop at `access method "ivfflat" does not exist`.

**HNSW opclasses on the new types (M3):**
- regress `hnsw_bit`, `hnsw_halfvec` and `hnsw_sparsevec` diff only on `operator class "<opclass>" does not exist for access method "hnsw"`.
- TAP `020`–`031` and `038` (13 tests) stop at the same error, for `bit_hamming_ops`, `halfvec_l2_ops` or `sparsevec_l2_ops`.

**Parallel builds (M6):** TAP `012` and `045` fail only on `using \d+ parallel workers`, plus 045's `after 0 tuples` message. This is the same as M1.

**TAP `018_aggregates`: open, awaiting the user.**
- Assertions 1–15 pass. Then `SELECT sum(v::halfvec) FROM tst;` fails with `ERROR:  value out of range: overflow`, and the script exits.
- The one assertion after it (`... WHERE r1 < 0`, expecting an empty result) passes when run by hand (Task 6).
- The cause is pgrust's parallel worker count, not M2's halfvec code. See "Known divergences left open".

## What M2 established
**f16 parity for every input, against both C paths.** `pgvector_f16_parity`'s `build.rs` compiles two things from the vendored `halfutils.h`:
- pgvector's software routines (the path C takes without F16C or `_Float16`);
- the compiler's `_Float16` conversions (the path the macOS arm64 reference build takes).

The port matches both for:
- all 2¹⁶ halves through half→f32;
- all 2³² f32 bit patterns through f32→half, NaNs included.

In Task 2, a one-line rounding mutation made 180,323,328 conversions and 2,483,235,116 arithmetic results fail, so the tests do catch real errors.

**halfvec arithmetic equals native `_Float16`.** For every pair of halves (2³² pairs) through `+`, `−` and `×`, widening to f32 and rounding once gives the reference's `_Float16` bits. The only exception is NaN payloads, and `CheckElement` rejects NaN before any arithmetic.

**The `diff` tier is clean on seeds 1 and 2.**
- **Deck.** The 184 deck statements in 12 sections match exactly at both seeds: 0 ruled, 0 findings.
- **Seeded arm, seed 1 × 2000:** 1949 match and 51 are ruled.
- **Seeded arm, seed 2 × 5000:** 4890 match and 110 are ruled.
- Every ruled record is `pgvector-float-rel`.
- **Where 6 and 2 of those ruled come from.** They are the pre-M2 `vector` ±0 statements that the user's 2026-10-06 decision rules (see below).
- **Counts before that decision:** 45 ruled + 6 findings at seed 1, and 108 ruled + 2 findings at seed 2.
  - The seed-2 figures are from Task 8's run on 1c0a2704fc.
  - This task's first full run reproduced the seed-1 figures on the final server.
  - Both seeds were re-run after the change (`run-diff.sh pgrust`, and `PGV_DIFF_SEED=2 PGV_DIFF_COUNT=5000 run-diff.sh pgrust`). Each newly ruled statement is one of the 8 former findings: seed 1 stmts 322, 441, 1080, 1197, 1729 and 2038; seed 2 stmts 541 and 3311.
- **Fixed during triage.** Task 8 found 5 sparsevec findings and fixed them by fusing the sparse inner product on aarch64.

**Error locations are identical at all 49 sites `scripts/pgvector/sql/error-locations.sql` covers.** `sqldiff.sh --verbose` shows 49 `LOCATION:` lines on each side.
- Examples: `CheckDims, sparsevec.c:50`; `CheckExpectedDim, sparsevec.c:62`; `Float4ToHalf, halfutils.h:257`; `CheckDims, bitvec.c:39`.
- The file deliberately leaves out the sites with known gaps: pre-M2 `vector` errors and the core `float.c:0` sites below. "Identical" holds only for the sites it covers.

**Huge declared sparse dimensions work.** Take `'{1000000000:1}/1000000000'`:
- Distance, norm, normalize and output return C's results, with no memory warnings in the server log.
- Casts to `vector` or `halfvec` error at the 16,000 limit.
- Evidence: the deck's `limits` section, and Task 6 Step 7.

## Known divergences left open
**TAP 018: parallel worker count. Open, awaiting the user.**
- **Planned workers.** pgrust plans 4 parallel workers for 018's 1M-row table, where C plans 2.
  - pgrust's `max_parallel_workers_per_gather` boot default is 4 (`guc_tables/src/tables.rs:965`; autotune `guc/src/autotune.rs:188`, `(cores/2).clamp(2, 8)`).
  - C's default is 2, and its log3 sizing still picks 2 even at a limit of 4.
- **Why the sum overflows.** Each partial `halfvec` sum saturates at `[8192,8192,16384]`. Five partials exceed 65504, so `halfvec_add` raises the float overflow error.
- **Evidence it is the worker count:**
  - With `SET max_parallel_workers_per_gather = 2`, pgrust returns C's `[24576,24576,49152]`.
  - C forced to 4 workers fails the same way.
- **This is a core planner/GUC divergence, not M2 code.** The harness's server options were not changed to hide it.
- **The user's choices:**
  - accept 018 as an exception;
  - pin the harness to 2 workers;
  - or align the core default.

  018 is re-run after that decision.

**Sign of an all-underflow zero (`pgvector-float-rel`, user-approved 2026-10-06).**
- **What differs.** When every product underflows, pre-M2 `vector` `inner_product`/`<#>` can return `0` where C returns `-0`, or the reverse.
- **Cause.** C sums in clang's arm64 vectorized, fused reduction order: 4×4 `fmla` lanes, a `faddp` tree, then a scalar `fmadd` tail. pgrust keeps source order (an M1 decision).
- **The amd64 C build, which does not fuse (`OPTFLAGS=""`), also returns `0` on seed-1 stmt 322**, where pgrust returns `-0` (Task 8). So the sign depends on the reduction order the compiler picks, not only on fusion.
- **The user chose option (b)** (commit aa29a4ec90): in distance and norm statements, a zero of either sign counts as within tolerance.
- **Option (a) was not taken.** It would port C's arm64 reduction tree into `vec.rs`: about 25 lines, which is over Task 8's ~20-line pre-M2 exception, and specific to the arch and compiler.
- **Still exact:** outside distance and norm statements, a signed-zero difference is still a finding. Non-zero integral numerals, NaN and Infinity also stay exact.

**Scope of `pgvector-float-rel`** (Task 7 ruling, `docs/fuzzing/rulings.toml`).
- **Where it applies.** It covers only distance and norm statements: deck sections `distance` and `norm`, plus the seeded distance-function, distance-operator, norm and sparsevec-metric kinds.
- **What it allows.** There, numerals that are non-integral on either side may differ within 1e-5 relative, floored at magnitude 1. Zeros of either sign count as equal.
- **Everything else compares exactly:**
  - row order: the suite escalates `tie-ordering`;
  - `*_cmp` magnitudes: it escalates `cmp-magnitude`;
  - float ulps: it escalates `b1-float-ulp`;
  - elementwise arithmetic, I/O, casts, aggregates, KNN, limits and errors.
- **Numerals are classed by spelling.** An exponent-form numeral such as `1e+06` counts as non-integral, so it would be ruled equal to `1000000` in a distance or norm statement.

**Pre-M2 `vector` error locations.**
- These errors report `pgvector.c:0`, a crate-derived location, because `types_error/src/source_map_table.rs` has no `pgvector` row. This is the open M1 item.
- `halfvec_to_vector` and `sparsevec_to_vector` inherit it through vector's `check_dim`/`check_expected_dim` (`CheckDim`). The crate header marks this `DIVERGENCE`.
- `fc_array_to_vector` (`funcs.rs`) duplicates M2's `cast_array_elems`/`cast_elem_f32`.
  - The plan required this (Task 3 ruling): routing vector through the shared helpers changes its LOCATIONs.
  - So the duplication closes together with this item.
- M1's malformed `"{hnsw.c"` entry (`source_map_table.rs:197`) is also still there.

**Core `float.c:0` locations.** Both are offered to the user as small core follow-ups.
- **The float overflow/underflow helpers.** `adt_float`'s `float_overflow_error`/`float_underflow_error` (`crates/backend/utils/adt/float/src/lib.rs:71`/`77`) set no location.
  - pgrust reports `float.c:0`, where C reports `float.c:90`/`98`.
  - halfvec `+ − ×`, halfvec and sparsevec `l2_normalize`, and `halfvec_accum` hit this, and so do `vector` and core float.
- **`numeric_float4`** (`crates/backend/utils/adt/numeric/src/ops.rs:836`).
  - pgrust reports `float.c:0`, where C reports `float4in_internal, float.c:288`.
  - Every `numeric[]` array cast hits it; for example, `'{1e39}'::numeric[]::halfvec` and `::sparsevec`.

**Sparse kernels.**
- **The inner product fuses multiply-adds on aarch64 only** (`inner_product_step`, `DIVERGENCE` in `sparsevec.rs`). That matches C's codegen per target: the arm64 reference fuses (`fmadd`), and the amd64 Docker build (`OPTFLAGS=""`) does not. An x86-64 C build with `-march=native` would differ in the last bit and in the sign of an all-underflow zero.
- **The L2 and L1 merges keep pgrust's source-order, unfused arithmetic.**
  - C's L2 merge fuses (`fmadd`, with a vectorized trailing loop; Task 8 disassembly).
  - C's L1 codegen was not inspected.
  - On non-integer data either merge can drift in the last bit, and the drift is ruled in distance statements.
- **Note for M3.** HNSW traversal will call these kernels, so a byte-compare case on non-integer sparse data may need the same per-target treatment.

**Other open items:**
- **Findings-file block (deferred by Task 7 ruling).** The `diffrunner.rs` raw-fs budget in `lint-determinism.allow` went from 7 to 8 because of the `--pgvector` findings-file write. That block is now duplicated across five suites: copybin, copytext, copyopts, dbddl and pgvector. Extracting a shared helper would shrink the budget again.
- **Error-message number masking (pre-existing, unchanged).** diffrunner's classifier (`canon::oid_pair`, `crates/bin/fuzzgen/src/canon.rs:313`) masks digit runs of 16384 or more at the same position in otherwise identical error messages. So the suite cannot tell two different large numbers apart there, such as the limit in `... cannot exceed 1000000000`. The deck's 16000/16001 dimension errors are below that threshold and compare exactly.
- **`pg_get_loaded_modules()` version (M4).** pgrust reports `vector` as 18.6, where C reports 0.8.7 (`PG_MODULE_MAGIC_EXT`, `vector.c:49`). This is marked `DIVERGENCE` in the crate header.
- **M1's HNSW divergences are unchanged:** the build-memory spill point, cosine byte-compare and the SelectNeighbors tie-break (`pgvector-m1-report.md`).

## Coverage limits
- **wasm.** `wasm/wasm-build.sh` was not run, because the toolchain `nightly-2026-07-17` is not installed. The parity crate is wasm-safe by construction: its `build.rs` returns early on wasm32, and its tests are compiled out there (`cfg(pgv_c_half)`).
- **x86.** The C reference's x86 F16C and AVX-512 dispatch paths are not exercised on this arm64 host. They are expected first on Linux, in M5. The x86 choice for the sparse inner product was checked once, against the amd64 Docker image under emulation (Task 8).
- **Binary-only error sites.** The recv functions are covered by unit tests (`recv_validates_like_c` for halfvec and for sparsevec), not by the LOCATION check.
- **Untested path.** The zero-dropping reallocation path of `sparsevec` `l2_normalize` (`sparsevec.rs`) has no test.
- **M1's two M3 prerequisites carry forward unchanged** (`pgvector-m1-report.md`, "Coverage limits"):
  - **Scan memory-charge emission order** is checked end to end at only one cap transition. Before M3 rewrites `scan.rs`, add either an event-stream diff or a larger iterscan tier.
  - **Review Focus 2** (a wrong-dimension query errors before the empty-index return; the insert and build `HnswCheckDim` sites) has no committed test. M3 should add a durable C-vs-pgrust SQL diff check for it.
- **M1's other iterscan limits stand:** one distinguishable memory-cap stop point, and no cosine case.

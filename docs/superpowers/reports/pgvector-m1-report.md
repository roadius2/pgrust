# pgvector conformance after M1

Date: 2026-10-05 · pgrust commit: 09e300eb80 (code; the docs commit follows it) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY)

## Summary
- C reference: 79/79 (regress 14/14, TAP 48/48, bytecmp 10/10, iterscan 7/7).
- pgrust: 33/79 (regress 2/14, TAP 14/48, bytecmp 10/10, iterscan 7/7). M0 was 12/62 on regress+TAP; it is now 16/62.
- Fixed in M1: regress `vector_type`; TAP `016_hnsw_inserts`, `043_hnsw_iterative_scan`, `044_hnsw_iterative_scan_recall`.
- Gate also green: `harness_test.sh` all passed; `UT_M1` 25 tests passed (8+8+3+6 across the four crates, 0 in `main_main`); `lint-determinism` PASS (0 violations, 41 warnings); `lint-seam-installs` PASS (0 violations, 22 allowlisted); `build-reference.sh --verify` ok; both reference trees print nothing from `git status --porcelain --ignored`.

## pgrust results
| Tier | Test | Result |
|---|---|---|
| regress | bit | FAIL |
| regress | btree | FAIL |
| regress | cast | FAIL |
| regress | copy | FAIL |
| regress | halfvec | FAIL |
| regress | hnsw_bit | FAIL |
| regress | hnsw_halfvec | FAIL |
| regress | hnsw_sparsevec | FAIL |
| regress | hnsw_vector | ok |
| regress | ivfflat_bit | FAIL |
| regress | ivfflat_halfvec | FAIL |
| regress | ivfflat_vector | FAIL |
| regress | sparsevec | FAIL |
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
| tap | 033_comparison | FAIL |
| tap | 034_distance_functions | FAIL |
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

**33/79 passed**

## Remaining failures
Every remaining failure keeps its M0 cause (`pgvector-m0-baseline.md`): types not yet ported (M2/M3), IVFFlat (M4), parallel builds (012, 045: M6). No test's cause changed. Checked against the run's logs: the regress diffs are still `type "halfvec"/"sparsevec" does not exist`, `access method "ivfflat" does not exist`, missing `hamming_distance`/`jaccard_distance` and `bit_hamming_ops`; 012 and 045 still fail only on `using \d+ parallel workers` (and 045's `after 0 tuples` message). The set of passing TAP tests is exactly the M0 eleven plus 016, 043, 044.

## What M1 established
- Stock C pgvector never seeds HNSW builds (`SeedRandom(42)` is under `#ifdef HNSW_MEMORY`); `pgrust.hnsw_build_seed` reproduces the seeded build on demand.
- With seed 42, pgrust's HNSW index pages are byte-identical to C's on tie-free and tie-heavy data, unlogged init forks and post-build inserts (bytecmp 10/10).
- Iterative scans stop at C's point at every tested stop point (memory cap and tuple limit), through rescans (iterscan 7/7, including `lateral_64k`).

## Known divergences left open
- Build memory accounting decides the spill point (`run-bytecmp.sh pgrust spill`: FAIL; C flushes after 1436 tuples, pgrust after 1840 at 1MB; pages differ in 526,983 masked bytes over 1,916,928); M6 reworks build memory.
- Cosine/non-integer data: C's `-ffp-contract=fast` fuses multiply-adds (`run-bytecmp.sh pgrust cosine`: FAIL; 221 masked bytes differ over 327,680, scattered across blocks).
- SelectNeighbors tie-break uses element ids where C uses addresses; they agree while C's graph fits one 1MB block (all tier cases). Unverified beyond that.
- `pg_get_loaded_modules()` version for `vector` (18.6 vs 0.8.7): M4.
- Error-location fields for pre-existing pgvector errors (crate-derived file names; `types_error/src/source_map_table.rs:197` has a malformed `"{hnsw.c"` entry): not addressed in M1.

## Coverage limits
- The iterscan tier distinguishes only one memory-cap stop point on its 1000-row index: C's scan memory goes 48 KB to 80 KB peak, the smallest reachable cap is 65,792 B, and `relaxed_64k_x2` and `relaxed_128k` have the same cap and both exhaust the index.
- It has no `vector_cosine_ops` case, so byte-level fidelity of the scan's memory charges rests on code review plus TAP 043/044.
- Follow-up: add an `hnsw.ef_search` column, a larger table with several caps, and a cosine case.
- Scan memory-charge emission order is checked end to end only at one cap transition; before M3 rewrites `scan.rs`, add either an event-stream diff (pgrust emits the B/R/A/F/C stream under a debug switch, diffed against a C trace of the same seed-42 query) or a larger tier (>=10k rows, caps 64k-512k, `hnsw.ef_search` variants, a lateral case whose later loops also hit the cap, a cosine case on data whose normalization is exact).
- Review Focus 2 (a wrong-dimension query errors before the empty-index return; the insert and build `HnswCheckDim` sites) has no committed test; it was a one-off C-vs-pgrust SQL diff in M1 Task 4. M3, which edits all three call sites, should add a durable C-vs-pgrust SQL diff check for it.

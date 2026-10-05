# pgvector conformance baseline (M0)

Date: 2026-10-04 · pgrust commit: 5dd12195e8 (upstream 79ad992ede + harness only) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (local build, `make OPTFLAGS=""`)

## Summary
- **C reference:** 62/62 passed (14 regress + 48 TAP; no `ref-flaky` tests), 203 s, in both runs.
- **pgrust today:** 12/62 passed (regress 1/14, TAP 11/48), with the same set passing in both runs. The second run came after the TAP clusters were moved to `--no-locale`; the first run had inherited `en_CA.UTF-8`.
- **Hybrid TAP setup works.** C `initdb` creates the cluster, and a copied `pg_ctl` starts pgrust through the shim. Every pgrust node log contains `starting pgrust`. Even `010_hnsw_wal` passes, which streams WAL from a pgrust primary to a pgrust standby.
- **Attention before M1:** `016_hnsw_inserts` fails because of a confirmed false positive in pgrust's own page-chain check, and pgrust's iterative scans stop early under selective filters (043, 044). Details below.

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
| regress | vector_type | FAIL |
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
| tap | 016_hnsw_inserts | FAIL |
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
| tap | 043_hnsw_iterative_scan | FAIL |
| tap | 044_hnsw_iterative_scan_recall | FAIL |
| tap | 045_hnsw_low_memory_build | FAIL |
| tap | 046_hnsw_vacuum_scan | ok |
| tap | 047_hnsw_vacuum_insert | ok |
| tap | 048_ivfflat_vacuum_insert | FAIL |

**12/62 passed**

## Failure causes
Each failing pgrust test appears in exactly one group. Regress causes come from `regress/pgrust/regression.diffs`, TAP causes from `tap/pgrust/log/<test>/regress_log_<test>` and `prove/<test>.log` (both under `~/.cache/pgrust/pgvector-work/`).

- **Not ported yet: `halfvec` / `sparsevec` / `bit` / IVFFlat (12 regress, 32 TAP).**
  - Regress:
    - `bit`: `ERROR:  function hamming_distance(unknown, unknown) does not exist`
    - `btree`, `cast`, `copy`, `halfvec`, `hnsw_halfvec`, `ivfflat_halfvec`: `ERROR:  type "halfvec" does not exist`
    - `sparsevec`, `hnsw_sparsevec`: `ERROR:  type "sparsevec" does not exist`
    - `hnsw_bit`: `ERROR:  operator class "bit_hamming_ops" does not exist for access method "hnsw"`
    - `ivfflat_bit`, `ivfflat_vector`: `ERROR:  access method "ivfflat" does not exist`
  - TAP, IVFFlat (`ERROR:  access method "ivfflat" does not exist`):
    - `001`–`009`, `036`, `040`, `041`, `042`, `048`
    - `035_ivfflat_bit_build_recall`: `ERROR:  operator does not exist: bit <~> bit`
  - TAP, `bit`:
    - `020_hnsw_bit_build_recall`: `ERROR:  operator does not exist: bit <~> bit`
    - `021`–`023`: `ERROR:  operator class "bit_hamming_ops" does not exist`
  - TAP, `halfvec` (`ERROR:  type "halfvec" does not exist`): `018_aggregates`, `024`–`027`, `032_ivfflat_halfvec_build_recall`, `033_comparison`, `034_distance_functions`
  - TAP, `sparsevec` (`ERROR:  type "sparsevec" does not exist`): `028`–`031`, `038_hnsw_sparsevec_vacuum_insert`
- **0.8.5 → 0.8.7 behavior change (1 regress).**
  - `vector_type`: `vector_combine` on 16,002-element state arrays (16,001 dimensions) returns a result, where 0.8.7 raises `ERROR:  vector cannot have more than 16000 dimensions`. This is the 0.8.7 `vector_combine` dimension check (spec §5.1).
- **Parallel HNSW build not ported (2 TAP, milestone M6).**
  - `012_hnsw_vector_build_recall`: expects `using \d+ parallel workers`. pgrust logs `DEBUG:  building index "idx" on table "tst" serially`. Recall subtests pass.
  - `045_hnsw_low_memory_build`: expects `using \d+ parallel workers` and `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`, the shared-memory accounting of a parallel build.
- **Iterative scans stop early under selective filters (2 TAP, already-ported HNSW code).**
  - `043_hnsw_iterative_scan`: with `relaxed_order`, `max_scan_tuples = 100000` and `scan_mem_multiplier = 2`, the first query finds 1–2 of the 10 matching rows across the two runs.
  - In the same test, the 20-query averages for `max_scan_tuples` = 50k/70k are 1.6 (run 1) and 2.35 (run 2). The test requires averages above 3 and 5 respectively (its expected count minus 2).
  - `044_hnsw_iterative_scan_recall` (tests only `<->` and `<=>`, 0.99 threshold): the 1-in-50 filter passes for both operators. At the 1-in-500 filter:
    - run 1: `<=>` 0.97, `<->` passed;
    - run 2: `<->` 0.935 and `<=>` 0.915, under both `strict_order` and `relaxed_order`.
  - The shortfall repeats across runs, so it isn't noise.
  - Likely cause, not confirmed: the port's header records that its iterative-scan memory cap "approximates C's MemoryContextMemAllocated with per-tuple estimates". If that cap trips early, the scan returns too few candidates.
- **Port bug, root cause confirmed (1 TAP).**
  - `016_hnsw_inserts`: `ERROR:  hnsw index "tst_v_idx" page chain does not terminate (cycle detected)`, 8 times in run 1 and 2 in run 2. The insert fails, but no data is corrupted.
  - **Cause:** this check exists only in pgrust (`crates/contrib/pgvector_hnsw/src/insert.rs:212-233`). It bounds the walk by the relation's block count, read **once before the walk**. Concurrent inserters legitimately append pages to the chain, so on a small index the walk overruns the stale bound.
  - **Evidence:** a standalone repro (`TRUNCATE` + 10 concurrent single-row inserts of 1,900-dimension vectors) fails on round 1. Parsing the index file at that moment shows a sound chain: 10 blocks, a 9-page chain from the head that terminates, every page reachable, and no page with two predecessors. Without the `TRUNCATE`s, `insertPage` sits near the end of a long relation and the repro runs clean.
  - **Fix:** when the bound is hit, re-read `RelationGetNumberOfBlocksInFork` and raise the error only if the walk still exceeds it. A real cycle is still caught. Re-reading is sound because `smgrnblocks` asks the file system outside recovery (`smgr/src/lib.rs:526-538`). TAP 016 is the failing test.
- **pgrust core gaps (not vector code):** none observed.
- **Harness-mode exclusions:** none needed. `010_hnsw_wal` passes, so pgrust-to-pgrust physical replication handles HNSW's GenericXLog records.

## Spec corrections made from this baseline
- **§9, M1's exit criteria:** `btree.sql` and `copy.sql` also cover `halfvec` and `sparsevec`, so they can't pass before M2. They moved to M2's exit criteria. M1 now requires `vector_type` and `hnsw_vector`; `hnsw_vector` already passes.
- **§8.1:** now describes the two-install layout under `$PGREF` (`pg` and `pgvec`).

## Other observations
- On first start, pgrust writes 3 pgrust-specific function rows (`pgrust_pin_database`, `pgrust_unpin_database`, `pgrust_seal_template`) and 3 description rows into each database of a cluster created by C `initdb`. This matters for the reverse on-disk tier (M5): a data directory that pgrust has run is no longer byte-for-byte what C created.
- **For M1 planning:** five HNSW TAP tests that use only `vector` fail, plus the `vector_type` regress test. Only `vector_type` (the 0.8.7 catch-up) is on M1's current list. Parallel builds (012, 045) are M6. Neither the 016 fix nor the iterative-scan divergence (043, 044) has a milestone. Both should go into M1, 016 first, since M3's exit criterion is "all HNSW TAP tests pass" and M1 is where pgrust's HNSW code is already being changed.

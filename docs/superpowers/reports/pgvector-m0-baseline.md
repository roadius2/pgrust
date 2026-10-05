# pgvector conformance baseline (M0)

Date: 2026-10-04 · pgrust commit: 5dd12195e8 (upstream 79ad992ede + harness only) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (local build, `make OPTFLAGS=""`)

## Summary
- **C reference:** 62/62 passed (14 regress + 48 TAP; no `ref-flaky` tests), 203 s.
- **pgrust today:** 12/62 passed (regress 1/14, TAP 11/48), 146 s.
- **Hybrid TAP setup works.** C `initdb` creates the cluster and a copied `pg_ctl` starts pgrust through the shim. Even `010_hnsw_wal` passes, which streams WAL from a pgrust primary to a pgrust standby.
- **Attention before M1:** `016_hnsw_inserts` raises pgrust's own "page chain does not terminate (cycle detected)" error under concurrent inserts. See "Port bug" below.

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
- **0.8.5 → 0.8.7 behavior changes (1 regress).**
  - `vector_type`: `vector_combine` on 16,002-dimension states returns a result where 0.8.7 raises `ERROR:  vector cannot have more than 16000 dimensions`. This is the 0.8.7 `vector_combine` dimension check (spec §5.1).
- **Parallel HNSW build not ported (2 TAP, milestone M6).**
  - `012_hnsw_vector_build_recall`: expects `using \d+ parallel workers`. pgrust logs `DEBUG:  building index "idx" on table "tst" serially`. Recall subtests pass.
  - `045_hnsw_low_memory_build`: expects `using \d+ parallel workers` and `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`, the shared-memory accounting of a parallel build.
- **Divergences in already-ported HNSW code (2 TAP).**
  - `043_hnsw_iterative_scan`: with `relaxed_order`, `max_scan_tuples = 100000` and `scan_mem_multiplier = 2`, it finds 2 matching rows where pgvector finds 10. The average for 50k/70k `max_scan_tuples` is 1.6, where pgvector's is above 3 and 5 respectively. Likely cause: the port's own header records that its iterative-scan memory cap "approximates C's MemoryContextMemAllocated with per-tuple estimates".
  - `044_hnsw_iterative_scan_recall`: `<=>` (cosine) recall falls below 0.97 at the 1-in-500 filter for both `strict_order` and `relaxed_order`. `<->` and `<#>` pass. Probably the same iterative-scan cause, but not confirmed.
- **Port bug, needs investigation (1 TAP).**
  - `016_hnsw_inserts`: `ERROR:  hnsw index "tst_v_idx" page chain does not terminate (cycle detected)`, 8 times. The test runs 20 rounds of 10 concurrent clients, each inserting one 1,900-dimension vector, so each element nearly fills a page and the chain grows on almost every insert.
  - This check exists only in pgrust (`crates/contrib/pgvector_hnsw/src/insert.rs:211-228`). It caps the walk at the relation's block count, read **once before the walk**, while concurrent inserters keep extending the chain.
  - It is either a false positive from that stale bound or a real page-chain corruption in the port's concurrent extend path. Decide which before M1 builds on this code.
- **pgrust core gaps (not vector code):** none observed.
- **Harness-mode exclusions:** none needed. `010_hnsw_wal` passes, so pgrust-to-pgrust physical replication handles HNSW's GenericXLog records.

## Other observations
- On first start, pgrust writes 3 pgrust-specific function rows (`pgrust_pin_database`, `pgrust_unpin_database`, `pgrust_seal_template`) into catalogs created by C `initdb`. This matters for the reverse on-disk tier (M5): a data directory that pgrust has run is no longer byte-for-byte what C created.
- Of the 6 failing vector-only HNSW tests, only one cause is planned for M1 (the 0.8.7 catch-up). Parallel builds are M6. The iterative-scan divergence and the 016 bug aren't on any milestone yet; M1's plan should take them first, since M3's exit criterion is "all HNSW TAP tests pass".

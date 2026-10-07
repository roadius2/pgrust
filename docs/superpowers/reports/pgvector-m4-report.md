# pgvector conformance after M4

Date: 2026-10-07 · pgrust commit: 38f384cb0e (code: `f57ea4bf3f`'s, Task 5's, the last Rust change; Task 6's `38f384cb0e` changed only the harness, and the M4 docs commit changes no code) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY -DIVFFLAT_BENCH)

## Summary
- **C reference:** 129/129 (regress 14/14, TAP 48/48, bytecmp 29/29, iterscan 19/19, diff 19/19).
- **pgrust:** 121/129 (regress 14/14, TAP 40/48, bytecmp 29/29, iterscan 19/19, diff 19/19).
  - M3 was 96/120. M4 added 9 tier rows: bytecmp 8 (the `ivf_*` cases) and diff 1 (`ivfflat_errors`). All 9 pass on pgrust.
  - On regress+TAP, pgrust went from 38/62 to 54/62.
  - The iterscan tier compared 1,874 memory-trace lines per side, all identical, as in M3: rebuilding the seeded oracle with `-DIVFFLAT_BENCH` moved no HNSW trace.
- **Fixed in M4:**
  - regress `ivfflat_bit`, `ivfflat_halfvec` and `ivfflat_vector`, so all 14 regression files now pass (spec §9's M4 exit criterion);
  - TAP `001`, `002`, `004`–`009`, `036`, `040`, `041`, `042` and `048`: 13 IVFFlat tests;
  - TAP `003`, `032` and `035` (the build-recall tests of the three types) now fail only on their parallel-build lines (M6). Every serial-build assertion, recall included, passes.
- **Gate also green:**
  - `harness_test.sh`: all passed (125 ok: Task 6's 127, less the two `upstream-sql` checks retired with the script).
  - `UT_M4`: 32 passed. `UT ivf_ block_sampler reservoir_ lookup_cover fmgr_stubs shipped_scripts` gave 24: `pgvector` 5 (`ivf_support_proc_symbol_requires_language_c`, `lookup_cover` 2, `fmgr_stubs`, `shipped_scripts`), `pgvector_ivfflat` 8, `pgvector_ivfflat_build` 5, `types_ivfflat` 3, `sampling` 3 (the moved sampler tests) and 0 in `main_main`. The `UT_CORE` half gave 8: `adt_amutils` 2, `types_relscan` 2, and `amapi`, `dfmgr`, `guc_tables` and `types_error` 1 each.
  - `UT_KEEP`: 63 passed (`pgvector` 39, `pgvector_hnsw` 13, `pgvector_hnsw_build` 3, `types_hnsw` 6). Its `type_info` and `parse_build_seed` filters also match one M4 test each (`pgvector_ivfflat` 1, `pgvector_ivfflat_build` 1), so 61 are M1–M3 tests.
  - f16 parity: 3 passed.
  - `lint-determinism`: PASS (0 violations, 41 warnings).
  - `lint-seam-installs`: PASS (0 violations, 22 allowlisted). Its 21 stale-allowlist warnings predate M4. The milestone adds no seam slot (its one new `init_seams`, `pgvector_ivfflat`'s, installs GUC storage) and leaves the allowlist alone.
  - `build-reference.sh --verify`: ok (13 checks; M3's 11 plus the two `IVFFLAT_BENCH` markers).
  - Both reference trees print nothing from `git status --porcelain --ignored`.
- **How the run was produced.** After `build-reference.sh --verify`, `build-pgrust.sh` found `target/fast-profile/postgres` current: cargo finished in 1.7 s without rebuilding, so the binary matches HEAD's Rust, unchanged since `f57ea4bf3f`. Then each command ran once: `run-all.sh ref` (the two lints ran alongside it), then `run-all.sh pgrust`, then `harness_test.sh`, `UT_M4`, `UT_KEEP`, the parity tests and the reference-tree check. Last, `sqldiff.sh pgrust` reran Task 5's extension and IVFFlat scripts (below). The table below is `run-all.sh pgrust`'s report.
- **No port fixes in Tasks 5–7.** IVFFlat's first end-to-end run (Task 5) passed the three regression files. The eight `ivf_*` cases were byte-identical on pgrust's first run (Task 6), and `ivfflat-errors.sql` matched with 18 errors and 4 NOTICEs. Task 7's IVFFlat TAP runs failed only the seven parallel-worker assertions, twice in a row, and `KEEP_TAP` stayed 27/27.

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
| tap | 003_ivfflat_vector_build_recall | FAIL |
| tap | 004_ivfflat_vector_insert_recall | ok |
| tap | 005_ivfflat_query_recall | ok |
| tap | 006_ivfflat_lists | ok |
| tap | 007_ivfflat_inserts | ok |
| tap | 008_ivfflat_centers | ok |
| tap | 009_ivfflat_filtering | ok |
| tap | 010_hnsw_wal | ok |
| tap | 011_hnsw_vacuum | ok |
| tap | 012_hnsw_vector_build_recall | FAIL |
| tap | 013_hnsw_vector_insert_recall | ok |
| tap | 014_hnsw_vector_vacuum_recall | ok |
| tap | 015_hnsw_vector_duplicates | ok |
| tap | 016_hnsw_inserts | ok |
| tap | 017_hnsw_filtering | ok |
| tap | 018_aggregates | ok |
| tap | 019_storage | ok |
| tap | 020_hnsw_bit_build_recall | FAIL |
| tap | 021_hnsw_bit_insert_recall | ok |
| tap | 022_hnsw_bit_vacuum_recall | ok |
| tap | 023_hnsw_bit_duplicates | ok |
| tap | 024_hnsw_halfvec_build_recall | FAIL |
| tap | 025_hnsw_halfvec_insert_recall | ok |
| tap | 026_hnsw_halfvec_vacuum_recall | ok |
| tap | 027_hnsw_halfvec_duplicates | ok |
| tap | 028_hnsw_sparsevec_build_recall | FAIL |
| tap | 029_hnsw_sparsevec_insert_recall | ok |
| tap | 030_hnsw_sparsevec_vacuum_recall | ok |
| tap | 031_hnsw_sparsevec_duplicates | ok |
| tap | 032_ivfflat_halfvec_build_recall | FAIL |
| tap | 033_comparison | ok |
| tap | 034_distance_functions | ok |
| tap | 035_ivfflat_bit_build_recall | FAIL |
| tap | 036_ivfflat_bit_centers | ok |
| tap | 037_inputs | ok |
| tap | 038_hnsw_sparsevec_vacuum_insert | ok |
| tap | 039_hnsw_cost | ok |
| tap | 040_ivfflat_cost | ok |
| tap | 041_ivfflat_iterative_scan | ok |
| tap | 042_ivfflat_iterative_scan_recall | ok |
| tap | 043_hnsw_iterative_scan | ok |
| tap | 044_hnsw_iterative_scan_recall | ok |
| tap | 045_hnsw_low_memory_build | FAIL |
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
| bytecmp | insert_grid | ok |
| bytecmp | big_golomb | ok |
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

**121/129 passed**

## Remaining failures
Every remaining failure is a parallel build (M6). I checked each against this run's logs.

**IVFFlat build recall (M6).** In M3 these failed outright (no `ivfflat` access method). Now TAP `003`, `032` and `035` fail only on `like($stderr, qr/using \d+ parallel workers/)`:
- `003`: 3 of 34 (line 127, once for each of the 3 opclasses);
- `032`: 3 of 34 (line 131, 3 opclasses);
- `035`: 1 of 14 (line 116, `bit_hamming_ops`).
- In each, pgrust logs `building index "idx" on table "tst" serially`. Every other assertion passes, including the `is($ret, 0, ...)` check on each "parallel" build and the recall checks after both builds (probes 1 and 10, which the tests skip for inner product, and probes equal to lists).

**HNSW build recall and low-memory build (M6), as in M3.**
- TAP `012`, `020`, `024` and `028` fail only on `like($stderr, qr/using \d+ parallel workers/)`:
  - `012`: 8 of 36 (lines 106 and 123, for each of the 4 opclasses);
  - `020`: 4 of 18 (lines 114 and 131, 2 opclasses);
  - `024`: 8 of 36 (lines 109 and 126);
  - `028`: 8 of 36 (lines 106 and 123).
  - In each, pgrust logs `building index "idx" on table "tst" serially`.
- TAP `045` fails 2 of 3: line 26 (`using \d+ parallel workers`) and line 27 (`hnsw graph no longer fits into maintenance_work_mem after 0 tuples`).
  - C's log shows `using 1 parallel workers` and then that NOTICE, so the message comes from C's parallel build.
  - pgrust logs `building index "tst_v_idx" on table "tst" serially`. This is the same as in M1–M3.

**A harness flake (for the user): `insert_grid`, C against C.**
- **What happens.** `run-bytecmp.sh ref` builds every case twice on the seeded C build. In Task 6 it intermittently failed the M1 HNSW case `insert_grid`: about 2 of ~37 runs, both inside full `harness_test.sh` runs. The first difference was the same three neighbor TIDs stored in a different order: `(37,39) (36,5) (38,25)` against `(38,25) (37,39) (36,5)`. All three are elements inserted after the build.
- **Likely cause** (consistent with the evidence, not proven). C's on-disk insert breaks equal distances by element address (`CompareCandidateDistances`, `hnswutils.c:1007-1011`). Addresses depend on the backend's allocation history, and the grid data has many equal distances. pgrust breaks the tie by element id, which matches C's usual order.
- **Not M4's.** The code path is under no `#ifdef`. In Task 6, 8 isolated runs of the HNSW cases on the new seeded build and 8 on a scratch `-DHNSW_MEMORY`-only build all passed. It never hit an IVFFlat case.
- **This run.** `run-all.sh ref` and `harness_test.sh`'s own C-against-C bytecmp run were both clean, so no rerun was needed.
- **Left unchanged.** Moving the case to opt-in, or redesigning it tie-free, changes an M1 case design. That is the user's call. Until then the `ref` gate can come back red on this case alone, and the remedy is a rerun.

## What M4 established
**IVFFlat, ported serially, for `vector`, `halfvec` and `bit`.**
- **Crates (spec §4.4):**
  - `types_ivfflat`: the `ivfflat.h` vocabulary, the page layout, `IvfflatTypeInfo` and the scan state;
  - `pgvector_ivfflat`: ivfutils.c, ivfinsert.c, ivfscan.c, ivfvacuum.c and the heap tuplesort that build and scan share;
  - `pgvector_ivfflat_build`: ivfbuild.c and ivfkmeans.c (Elkan);
  - `sampling`: sampling.c, moved out of `commands_analyze`, which re-exports it unchanged (Decision 6).
- **Core hook points** are spec §4.5's table, plus the plan's five small core edits.
- **The seven opclasses** are `vector_{l2,ip,cosine}_ops`, `halfvec_{l2,ip,cosine}_ops` and `bit_hamming_ops`, with the `lists` reloption and the `ivfflat.probes`, `ivfflat.iterative_scan` and `ivfflat.max_probes` settings.
- **Support proc 5 resolves by its C symbol.**
  - `ivfflat_get_type_info` (`pgvector_ivfflat/src/utils.rs`) maps it to one of ivfutils.c's three static tables. With no proc 5, the table is `vector`'s.
  - The symbol comes from the shared `pgvector::support_proc_symbol`: the `pg_proc.prosrc` of a `LANGUAGE c` function, the way GIN identifies its support procs. HNSW's proc 3 now resolves through the same helper, so it also gains the `prolang` check that M3 lacked.
  - Unit tests: `ivf_type_info_resolves_only_pgvector_symbols` and `ivf_support_proc_symbol_requires_language_c` (a C proc resolves; a `LANGUAGE sql` proc whose body names `ivfflat_halfvec_support` does not, and neither does a missing proc).
- **The plan's review focus**, each checked by a tier:
  - Fewer distinct values than lists: TAP `008` and `036` pass, and so does the unit test `ivf_kmeans_fills_every_center_from_duplicates`.
  - NULL, zero and wrong-dimension queries: `ivfflat-errors.sql`, below.
  - Lists spanning pages and full insert pages: bytecmp `ivf_insert`, `ivf_vacuum` and `ivf_wide`, plus TAP `002`, `007` and `048`.
  - `maintenance_work_mem` edges: the regression files' 1, 5, 6 and 29 MB builds pass, and the errors file raises C's exact `memory required is 8 MB, maintenance_work_mem is 1 MB`.
  - A support proc 5 that isn't pgvector's: the two unit tests above.

**Seeded IVFFlat builds byte-identical to C in all eight cases.**
- The seeded oracle is rebuilt with `-DIVFFLAT_BENCH`, the only C configuration that seeds IVFFlat builds (`SeedRandom(42)`, `ivfbuild.c:1075-1077`). On pgrust, the placeholder `pgrust.ivfflat_build_seed = 42` makes `ivfflatbuild` reseed at the same point (Decision 2).
- The data are well-separated groups of four integer points (`256 × grid + {0,1}²`), so every k-means and assignment comparison has a clear winner. The stored centers are element-wise sums divided once, which C and pgrust round alike.

| Case | Data | Blocks | Lists | What it exercises |
|---|---|---|---|---|
| `ivf_l2_groups` | 1,000 rows, 10 groups | 12 | 10 | k-means centers are exact half-integers, e.g. `(256.5, 0.5)` |
| `ivf_big` | 20,000 rows, 10 groups | 92 | 10 | 10,000 reservoir samples, with group means like `(256.48819, 0.511819)` |
| `ivf_halfvec_groups` | as `ivf_l2_groups`, `halfvec` | 12 | 10 | `1024.5` rounds to the half `1024` |
| `ivf_bit_hamming` | `bit(64)` from md5 | 12 | 10 | integer distances, thresholded centers |
| `ivf_insert` | 600 rows, then 400 inserts | 8 | 2 | inserts that append pages and move the insert pages |
| `ivf_unlogged` | as `ivf_l2_groups`, unlogged | 12 + 12 (init fork) | 10 | the init fork's random centers |
| `ivf_vacuum` | 3,000 rows; delete a third; `VACUUM` | 22 | 10 | each list's insert page goes back to its first page |
| `ivf_wide` | 50 rows of `vector(2000)` | 56 | 5 | 2,000 dimensions |

The block and list counts are this run's `bytecmp/pgrust/cases.tsv`: the oracle's file size and metapage, which the subject's files equal byte for byte. The `ref` run's are the same.

- Page layouts measured on C while writing the plan, now matched byte for byte:
  - `ivf_insert`'s two lists start on blocks 2 and 4. Its 400 inserts append blocks 6 and 7 and move both insert pages there.
  - `ivf_vacuum` deletes a third of 3,000 rows and runs `VACUUM`. Each list's insert page goes back to its first page with room.
  - `ivf_wide` stores one list item per page and one entry per page.
- With the knob off, pgrust's unseeded build differs from the oracle (harness self-test `test_bytecmp_ivf_knob_off`). The tier would notice a build that ignored the seed.

**Every SQL-reachable IVFFlat error site identical, LOCATION included.**
- `scripts/pgvector/sql/ivfflat-errors.sql`, the diff tier's `ivfflat_errors` row: 18 errors and 4 NOTICEs on each side, with identical transcripts. The sites:
  - `InitBuildState`: `ivfbuild.c:358` (1, `type not supported for ivfflat index` on a `varbit` column), `:364` (2), `:369` (4: 2,000, 4,000 and twice 64,000 dimensions) and `:384` (3, cosine and inner product on one dimension);
  - `IvfflatCheckMemoryUsage, ivfutils.c:133` (1);
  - `IvfflatCheckExpectedDim, ivfutils.c:289` (7), for all three types, at scans of empty and non-empty indexes;
  - the NOTICE `ivfflat index created with little data` with its DETAIL and HINT, at `ComputeCenters, ivfbuild.c:477` (4).
- Each NULL query is `(SELECT NULL::<type>)`, and an `EXPLAIN` pins it to `Index Scan using v_l2`, `h_l2` or `b_ham`. Each NULL-query count is 3 on both sides. The zero query on a cosine index returns `[1,2,3]` then `[1,1,1]` on both sides.
- The row covers pgvector's own error sites only. Reloption and GUC errors stay in the regression files, because pgrust core reports their LOCATION as `reloptions.c:0` and `guc.c:0` (Decision 5; Known divergences).

**Upstream's extension directory, verbatim.**
- `crates/contrib/pgvector/extension/` holds 43 files, each byte-identical to upstream 0.8.7: `vector.control` (`default_version = '0.8.7'`), `vector--0.8.7.sql` (upstream `sql/vector.sql`) and all 41 upgrade scripts, as C's `DATA` installs them (Decision 1).
  - `shipped_scripts_are_upstream_verbatim` checks the bytes. `lookup_covers_every_module_pathname_symbol` now reads every shipped file and expects 114 `MODULE_PATHNAME` symbols in the install script.
  - `main_main/build.rs` now prunes stale staged files, so the old `vector--0.8.5.sql` is gone from `target/<profile>/share/extension`.
- `scripts/pgvector/upstream-sql.sh` is retired with its self-test, since nothing grows the script now.
- `sqldiff.sh pgrust` on Task 5's extension script (`~/.cache/pgrust/pgvector-work/m4-ext.sql`, outside the repo), rerun in this task: identical. On both sides `extversion` is `0.8.7`, with one available version and 861 update paths (`0.8.1` reaches `0.8.7` through every 0.8.x script, `0.8.6` through `vector--0.8.6--0.8.7.sql`). After `UPDATE pg_extension SET extversion = '0.8.6'`, `ALTER EXTENSION vector UPDATE` returns to `0.8.7`. `pg_get_loaded_modules()` gives `vector | 0.8.7`. `SET ivfflat.nonexistent = 1` and `SET hnsw.nonexistent = 1` raise `invalid configuration parameter name ...` with `DETAIL: "ivfflat" is a reserved prefix.` (and `"hnsw"`). `CREATE EXTENSION vector VERSION '0.8.5'` and `'0.8.6'` fail as in C: `vector--0.8.7.sql` is the only install script, and no update path leads down to an older version.
- `sqldiff.sh pgrust` on Task 5's IVFFlat script (`~/.cache/pgrust/pgvector-work/m4-ivf.sql`, outside the repo), rerun in this task: identical. All seven opclasses build with `lists = 4`, the plan is `Index Scan using t_v_l2`, every opclass family's nearest-neighbor rows match, and the index-scan counts are 2000 (L2) and 1999 (cosine, without the zero vector) before a delete, `VACUUM` and insert, and 1834 and 1831 after.
- **Decision 4 changes HNSW too.** `SET hnsw.x = ...` after the library loads now errors, as in C. The `hnsw_*` regression files and `KEEP_TAP` stay green with it.

**What Tasks 5–7 had to fix:** nothing. No port fix was needed, so no fix test was added.

## Known divergences left open
**New in M4:**
- **Core LOCATIONs `reloptions.c:0` and `guc.c:0` (a new core follow-up).** C reports `parse_one_reloption, reloptions.c:1637` and `parse_and_validate_value, guc.c:3186`/`:3282` for the `lists` reloption and the `ivfflat.*`/`hnsw.*` settings. pgrust core reports line 0. The regression files run at default verbosity, so they pass; the durable `ivfflat_errors` row leaves these statements out. The fix is in core and needs the user's go-ahead.
- **`_PG_init` load timing (a core follow-up).**
  - C loads `vector`, running `_PG_init` and reserving the `hnsw` and `ivfflat` prefixes, on a session's first open of an hnsw or ivfflat index: `InitIndexAmRoutine` → `GetIndexAmRoutine` → `OidFunctionCall0(amhandler)` (`relcache.c:1432`, `amapi.c:38`).
  - pgrust resolves the handler by name (`resolve_extension_handler`, `amapi/src/lib.rs`) and never loads the library there.
  - So the reservation can come later than in C. For example, `SET hnsw.typo = 1`, then a query on an hnsw-indexed table, then `SET hnsw.typo2 = 1` gives C's WARNING and reserved-prefix ERROR but nothing on pgrust, and `pg_get_loaded_modules()` can omit `vector`.
  - This predates M4 (since M1); M4's `pg_init` makes it visible. The fix is in core `amapi` and needs the user's go-ahead.
- **`pg_index_has_property(<hnsw or ivfflat index>, 'bitmap_scan')` is `t` where C says `f` (a core follow-up).** `adt_amutils` answers `Prop::BitmapScan => Some(true)` for every access method, while C returns `amgetbitmap != NULL` (`amutils.c:368-369`), and neither pgvector AM has one. It predates M4 and is shared with HNSW. `AmFlags` has no `amgetbitmap` bit, and the fix is outside spec §4.5, so it needs the user's go-ahead.
- **The static `ivfflat.*` and `hnsw.*` GUCs exist before the library loads.** C defines them at `_PG_init` (`DefineCustom*Variable`), so before `vector` loads, `SHOW ivfflat.probes` is an unrecognized parameter and `SET` makes an unvalidated placeholder. pgrust registers them in core `guc_tables` at boot. HNSW's have done so since M1.
- **An unknown or non-C support proc 5 raises where C calls it.** pgrust knows only `ivfflat_halfvec_support` and `ivfflat_bit_support`, and raises 0A000 `ivfflat type support function "<symbol>" of index "<index>" is not supported` (`DIVERGENCE` in `utils.rs`). C calls any proc 5 and trusts the pointer it returns. HNSW's proc 3 behaves the same way, and since M4 a non-C proc 3 also raises.
- **The scan keeps a detoasted copy of the query** in its own memory context, where C uses the scan key's value in place (`GetScanValue`, `ivfscan.c:198-236`), as HNSW's scan does. Results are the same.
- **An insert into a list with no insert page raises where C asserts** (`ivfinsert.c:110`). Without the check, `InvalidBlockNumber` would reach `ReadBuffer` as `P_NEW` and extend the index from a corrupt list item. Only a corrupt index reaches it.
- **No parallel build (M6).** `plan_create_index_workers` is unported, so both access methods build serially. C also builds serially when no workers launch.
- **HNSW's build phase names are not reported** (pre-existing). `pg_indexam_progress_phasename` returns NULL for HNSW, where C names `initializing` and `loading tuples` (`hnswbuildphasename`, `hnsw.c:117-129`). M4 adds IVFFlat's four names only (`phasenames_match_c`).
- **ANALYZE seeds its reservoir from a u64 where C uses a u32.** C passes `(uint64) pg_prng_uint32(&pg_global_prng_state)` (`sampling.c:139`); pgrust's ANALYZE passes `next_u64()` (`analyze/src/lib.rs:1816`). That is unobservable, because nothing seeds ANALYZE. IVFFlat's build passes `next_u32() as u64`, as C does, and its sampling is byte-checked by `ivf_big`.
- **Type info is resolved on every call, not cached in `rd_amcache`** (Decision 3).
  - pgrust's `rd_amcache` is one typed field per access method in core `RelationData`, so caching would add core fields.
  - The cost is two `pg_proc` syscache probes and a memory context per insert into a `halfvec` or `bit` index (`support_proc_symbol`; `vector` has no proc 5), against an insert that already computes a distance to every list's center. C also calls the support proc on every insert, through `FunctionCall0Coll` on the cached `FmgrInfo` (`ivfutils.c:427`).
  - This is a performance item, and HNSW's non-`vector` inserts pay it too.
- **For M5: IVFFlat cosine and inner-product k-means.** These opclasses' k-means normalizes with norm sums that C vectorizes and pgrust sums in source order (M2's no-reduction-emulation ruling). So they are not in the byte-identical tier. The regression files, TAP and `m4-ivf.sql` cover them.

**Carried forward from M3, untouched by M4** (details in `pgvector-m3-report.md`):
- **The vectorized tails of 4+ b-only entries in the sparse L2/L1 kernels.** clang and gcc vectorize the trailing loop differently, so C's two builds disagree on long tails. pgrust fuses the scalar steps only (`DIVERGENCE` at `sparsevec.rs` `fmadd_step`), and equals both C builds when at most 3 b-only entries trail. `pgvector-float-rel` rules the last-bit differences in distance statements.
- **The same class for the dense `vector` and `halfvec` kernels**, unchanged from M2. C sums in its compiler's vectorized, fused order; pgrust keeps source order. That is why `vector` cosine stays an opt-in bytecmp case (`cosine`), and why the ±0 clause of `pgvector-float-rel` exists.
- **The fmgr stubs raise where C returns.** The access methods and their support procs are resolved by access method and by symbol, never called through fmgr. Bound by a superuser to a callable signature, C's support functions return a pointer to their static type info, and `hnswhandler`/`ivfflathandler` return a freshly built `IndexAmRoutine`. pgrust raises XX000 `<symbol> is resolved by ..., never called through fmgr` (`DIVERGENCE` in `pgvector/src/lib.rs`). `fmgr_stubs_raise_internal_error` now covers 7 symbols, M4's `ivfflathandler`, `ivfflat_halfvec_support` and `ivfflat_bit_support` included.
- **A hand-written opclass that gives `bit` a norm proc** makes C call a NULL normalize function, which crashes. pgrust raises, in HNSW (`hnsw_norm_value`) and now in IVFFlat (`ivfflat_norm_value`).
- **For M5.**
  - M5's identical-approximate-results gate compares pgrust with the Docker image's gcc build, whose tails differ from both clang's and pgrust's sequential order. M5 may need to measure how often that changes a traversal, or decide to emulate gcc's tail.
  - C also vectorizes the normalize functions' double-precision norm sums: sparsevec `l2_normalize` (`sparsevec.rs:904`), and halfvec's and vector's `l2_normalize`. Their order feeds the stored values of cosine indexes, HNSW and IVFFlat alike.

**Carried forward from M2, untouched by M3 and M4** (details in `pgvector-m2-report.md`):
- **TAP 018's worker pin.** The harness runs pgrust with C's `max_parallel_workers_per_gather = 2`. pgrust's own default (4) is unchanged, so stock pgrust still differs from C on parallel plan shape.
- **`float.c:0` locations (a core follow-up).**
  - `adt_float`'s `float_overflow_error`/`float_underflow_error` set no location; C reports `float.c:90`/`98`.
  - `numeric_float4` reports `float.c:0`, where C reports `float4in_internal, float.c:288`.
- **`pgvector.c:0` locations (a core follow-up).**
  - Pre-M2 `vector` errors have crate-derived locations, because `source_map_table.rs` has no `pgvector` row.
  - `halfvec_to_vector`, `sparsevec_to_vector` and `halfvec_combine` inherit this.
  - `fc_array_to_vector`'s duplication of the shared cast helpers closes with this item.
  - M1's malformed `"{hnsw.c"` entry (`source_map_table.rs:197`) is still there. M4's two rows below it (`pgvector_ivfflat` → `ivfflat.c`, `pgvector_ivfflat_build` → `ivfbuild.c`) are well-formed.
- **The findings-file block.** `diffrunner.rs`'s raw-fs budget is 8 because the block is duplicated across five suites (copybin, copytext, copyopts, dbddl, pgvector); a shared helper would shrink it.
- **M1's build divergences.** The build-memory spill point (opt-in `spill`, M6) and `vector` cosine pages (opt-in `cosine`) are unchanged.

**Closed in M4:** `pg_get_loaded_modules()` reports `vector | 0.8.7`, as C does, through `dfmgr`'s library-version registry (Decision 7).

## Deferred minors
Review minors deferred to a later cleanup, one line each. This section carries every M2 and M3 item that M4 did not absorb, so the M3 report's list is superseded.

**Absorbed by M4:**
- `fmgr_stubs` is in `UT_KEEP` (M3's final-wave test is part of the gate again).
- `hnsw_get_type_info` gains GIN's `prolang` check through `pgvector::support_proc_symbol` (Task 1), and the `utils.rs:116` comment no longer compares it with amapi's handler resolution.
- `pgvector/src/lib.rs` cites `hnsw.c:266-402` (Task 5).
- CLAUDE.md's `run-bytecmp.sh` line shows `[case ...]` (this task).
- Spec §5.4's split parenthetical and its "by function name" wording (this task).
- `strip_hnsw_memory`'s comment: the function is now `sqldiff.sh`'s `strip_seeded_info`, and its comment names `hnswvacuum.c:171` (Task 6).
- `upstream-sql.sh`'s `--only-am` typo handling, moot now that the script is retired (this task).
- The M3 report's three wording items, worded correctly where this report carries the same points (the M3 report itself is not rewritten): "C returns its static pointer" holds for the support functions, while PG18's `hnswhandler` builds its routine with `makeNode`; "C crashes on the same input" (an unknown support proc 3) was never probed, since C trusts the returned pointer, which is undefined behavior; and the stale "Note for the final review" lead-in.

**From M2: `pgvector` crate code and comments**
- `bitutils.rs:2-3`: the AVX/dispatch citation range is loose (AVX 75-96 and 133-160; dispatch 162-224).
- `bitvec.rs`'s `init_bit_vector` hand-rolls the little-endian varlena header, as `vec.rs` does.
- `halfvec.rs:2` cites `halfvec.h:62-68`; the struct is at `halfvec.h:67-73`.
- `sparsevec.rs:75` cites `sparsevec.h:44-48`; the lines are 43-47.
- `halfvec.rs:22-28` and `sparsevec.rs:18-19` split their `use crate::funcs`/`halfutils` lines.
- `funcs.rs:277`: `cast_elem_f32`'s doc comment starts mid-sentence (the brief's first line was dropped).
- `halfvec.rs:718-720`: `check_state_array` relabels every `StateArray::check` error, OOM and detoast included, as `halfvec.c:173`.
- `sparsevec.rs:279-287`: `underflowed_to_zero` allocates a `Vec` to skip space and sign, where a slice would do.
- sparsevec send's literal 0 for `unused`, and `from_payload`'s "corrupt sparsevec datum", are unmarked defensive differences (the same pattern as `VecView`/`HalfView`).
- "One module per C file" (the crate header, CLAUDE.md) is not literal: vector.c is split across `vec.rs` and `funcs.rs`.

**From M2: unit tests**
- bit: `check_dims` asserts only the message (no SQLSTATE or location), `init_bit_vector`'s test skips the varlena header word, and the kernels are not tested from non-zero starting counts.
- f16 parity: the tests report a mismatch count, not the first failing input. The `_Float16` skip is an `eprintln!` hidden by test capture. The 2³² tests are not `#[ignore]`d under the debug profile, so plain `cargo test` runs them unoptimized.
- `float4_to_half`, the halfvec parse-error tests (`parse_errors_match_halfvec_out`) and the sparsevec tests assert messages, not SQLSTATEs or locations.
- halfvec `recv_validates_like_c` lacks the dim > 16000 / `i16::MAX` case and the C check-order case. sparsevec recv lacks i32 extremes, and there is no case for an index position written as an i64 literal.
- No unit test covers halfvec `subvector` bounds at `i32::MIN`, `binary_quantize` bit order, or `mul` underflow (regress covers some).
- The sparsevec `cmp` test is narrow: its negative-value and index-mismatch branches are covered only by TAP 033.

**From M2: diffrunner's `--pgvector` suite**
- The seeded-arm test checks shape only (not sparsevec index validity, nor all 8 kinds). The compare tests lack NULL and oid-mismatch cases. The first fix round's test names only 2 of the review's 6 tolerance pairs.
- `normalize_user_oids` merges `vector`, `halfvec` and `sparsevec` (the deck's `pg_typeof` statements cover result types), and the suite flags silently exclude each other (a pre-existing pattern).
- Ruled ±0 records' detail text says "numerals within 1e-05 relative" and does not name the zero-sign clause (`pgvector.rs:169`).
- Numerals are classed as integral by spelling, so `1e+06` and `1000000` are ruled equal in a distance or norm statement (documented in the M2 report; seeded data does not produce it).
- `pgvector_deck.sql:8-10` says each denormal case has one non-zero term, but four statements have two (their sums are exact, so results are unaffected).
- diffrunner's classifier (`canon::oid_pair`, `canon.rs:313`) masks digit runs of 16384 or more at the same position in otherwise identical error messages (pre-existing).

**From M2: harness**
- `sqldiff.sh` calls `assert_identity` after `server_start`, which already does. Its `server_stop` also stops a `server.sh` server on the same port, and the usage does not say so.
- `common.sh`'s `PGRUST_SERVER_OPTS` carries `listen_addresses=`, and `server_start` appends another (the last wins).
- `harness_test.sh`'s `test_diff_detects_difference` asserts only the `normalize` FAIL, not that setup and io stay ok.
- Commit aa29a4ec90's message says amd64 C disagrees with arm64 C on stmt 322; both return 0. The M2 report is right, and history was not rewritten.

**From M3: `pgvector_hnsw` and `pgvector`**
- `utils.rs` `hnsw_norm_value` copies the fmgr result a second time in the same memory context.
- Non-vector inserts create a `MemoryContext` and look up `pg_proc` per row (`insert.rs:914`), where C uses a cached `FunctionCall0Coll`. Since M4 the lookup is `support_proc_symbol`'s two probes (see Decision 3 above).
- `charge_norm_value_matches_c` restates the function it checks, so the normalize-drop `tmpCtx` charge sequence rests on that test's reading of `sparsevec.c`; the iterscan trace is KB-granular. Also, `hnsw_get_type_info`'s error path has no unit test (M4's `ivf_support_proc_symbol_requires_language_c` covers only the shared symbol lookup).
- `sparsevec.rs:785-786`: the L1 tail's divergence has no `DIVERGENCE` marker at the site; `fmadd_step`'s comment covers only the L2 tail.
- `pgvector/src/lib.rs:13-15`: the header's "neither emulates C's vectorized L2/L1 tails" reads oddly.
- sparsevec's single-array norm loops are unfused scalar sums with no `DIVERGENCE` note, while C vectorizes them. These are the cosine norms (`sparsevec.rs:854`, `:858`) and `l2_norm`/`l2_normalize` (`:891`, `:904`), and `sparsevec_cosine_ops`' support proc 2 uses them. M2's no-reduction-emulation rule plausibly covers them.

**From M3: harness and report**
- `harness_test.sh:353`: `test_iterscan_memory_off` passes on any `relaxed_64k` FAIL. It should assert 0 subject memory lines and at least 1 oracle line.
- No stop-point self-test checks that `big_strict_128k` (2669) and `big_ef100_128k` (4727) stay below 9990.
- `harness_test.sh:355`: shellcheck SC1007 on `PGV_ITERSCAN_MEMORY= ` (use `''`), like the `PGV_BYTECMP_SEED=` lines (`:255`, `:268`).
- `run-iterscan.sh`'s `setup_sql` builds the 10,000-row `big` index on every run, even for single non-`big` cases.
- `sqldiff.sh`'s `strip_seeded_info` runs on both sides, so sqldiff alone would miss a default-on pgrust trace (regress and TAP would catch it).
- `hnsw-errors.sql`'s NULL-query statements for `bit` (`:49`), `sparsevec` (`:66`) and `vector` return the same count from a seq scan, so sqldiff cannot detect a fallback; an `EXPLAIN (COSTS OFF)` per NULL query would pin the plan. (`ivfflat-errors.sql` has those `EXPLAIN`s.)
- `wide_sparsevec` has only the generic 10-block self-test; a 150-block arm like `wide_golomb`'s would pin Review Focus 1.
- Five of the six M3 iterscan type cases stop in their first batch (no resume); only `sparsevec_cosine_drop_64k` spans several.
- `run-bytecmp.sh:23-26`: the header says the non-integer cases explain in their comments why C and pgrust round alike, but the opt-in `vector` `cosine` case is a known divergence, and `insert_sparsevec_cosine` leans on `sparsevec_cosine_grid`'s comment.
- `lint-seam-installs` warns of 21 stale allowlist rows, `pgaio_io_start_readv` among them (pre-existing, not M3 or M4).
- The M3 report's HNSW error-site claim should note that `HnswCheckDim`'s build and insert sites are unit-test-only (unreachable from SQL).

**From M4, Task 1 (`types_ivfflat`, ivfutils.c, `support_proc_symbol`)**
- `utils.rs` `index_getprocinfo`'s comment "Not reachable from SQL: CREATE OPERATOR CLASS checks the procs" is false: `ivfflathandler` has no `amadjustmembers` and `ivfflatvalidate` returns true. Behavior still matches C.
- `utils.rs` `ivfflat_norm_value` drops a `PgResult` (`let _ = mcx::vec_append_bytes(...)`; use `?`), and copies the normalized image twice.
- `ivfflat_norm_vectors` creates a bump `MemoryContext` per vector, where C resets one `tmpCtx`: up to about 1.6M samples at `lists = 32768`.
- `pub fn datum_image` and `buffer_page` return unbounded lifetimes from raw pointers behind safe signatures (the precedent is `pub(crate)`).
- `unwrap_or_default()` gives `type support function ""` for a non-C or missing proc; naming the OID or `proname` would help (IVFFlat and HNSW `utils.rs`).
- `support_proc_symbol` does two syscache lookups and creates a `MemoryContext` per call, so per inserted row; HNSW previously did one lookup. A performance item, tied to Decision 3.
- No unit test covers the relcache init-file codec's tag 12 or `sort.rs`'s `HeapSort`. The regression, TAP and bytecmp tiers exercise both end to end.
- `ivf_support_proc_symbol_requires_language_c` guards its seam installs with `is_installed()` and then `set`, not a `Once` (no other installer is in that test binary today).
- Task 1's report was imprecise on `UT_KEEP`'s per-crate counts and on the pre-existing lint warning counts (process only).

**From M4, Task 2 (ivfkmeans.c, ivfbuild.c, `sampling`)**
- `kmeans.rs`'s `random_double`/`random_int` cite `ivfflat.h:84-86`; the macros are at `ivfflat.h:87-88`.
- `ivf_kmeans_fills_every_center_from_duplicates` only asserts that the 3 distinct values appear, so it would pass if empty clusters kept duplicates or were zeroed. It should pin the two random centers in [0,1)² (or exactly, under its seed 7).
- `ivf_check_centers_rejects_short_arrays_and_nan` checks message text only, not SQLSTATE or location.
- The raw-pointer and `inner_err` indirection in `sample_rows`/`assign_tuples` is unnecessary (a direct `&mut bs` borrow works); it is copied from `pgvector_hnsw_build`.
- `ivfflat_kmeans` fetches `IVFFLAT_KMEANS_DISTANCE_PROC` before `IvfflatCheckMemoryUsage`, where C checks memory first (`ivfkmeans.c:290`, then `:297`). The error differs only for a custom opclass that lacks proc 3 and also overflows `maintenance_work_mem`, and it is not marked `DIVERGENCE`.
- `add_sample` and `add_tuple_to_sort` create a bump `MemoryContext` per row, where C resets one `tmpCtx` (speed only; the same as the HNSW build).
- The kmeans tests install their `CHECK_FOR_INTERRUPTS` stub as a side effect of `l2_procs()`; a direct call would be clearer.

**From M4, Task 3 (ivfinsert.c, ivfscan.c, ivfvacuum.c)**
- `insert.rs`'s `DIVERGENCE` error for a list with no insert page lacks `.with_location` (suggested: `ivfinsert.c:110`, `InsertTuple`).
- `vacuum.rs`'s `tid_cmp`/`tid_reaped` duplicate `pgvector_hnsw/src/vacuum.rs:23-36` verbatim.
- `scan.rs`'s iterative-batch logic (the `batch_probes` short-circuit, `list_index` past `listCount`) has no unit test; TAP `041` and `042`, which pass, are its coverage.

**From M4, Task 4 (core hook points)**
- The reloptions `lists` row uses the literals 100, 1 and 32768 rather than the `IVFFLAT_*` constants (as HNSW's rows do; it avoids a dependency).
- Pre-existing noise in every run: the 21 stale-allowlist warnings and the planner's `endpoint_datum_copy` warning (not M4's).
- The `bitmap_scan` property (see Known divergences).

**From M4, Task 5 (the verbatim script, `_PG_init`, the library version)**
- `dfmgr`'s `LIBRARY_VERSIONS` is a raw `std::sync::Mutex`, where CLAUDE.md names `pgsync` the only lock library. It matches `BUILTIN_LIBRARIES` beside it (plan-mandated, Decision 7).
- `main_main/build.rs` couples two separately written arrays by index (`sources[k]`), so reordering one list would prune every staged file. A single (subdirectory, directory, set) list removes the risk.
- `shipped_scripts_are_upstream_verbatim` maps a shipped `vector.sql` to `sql/vector.sql` through its `other` arm, so swapping one upgrade script for a `vector.sql` keeps 43 files and passes. Comparing names against upstream's listing would close it.
- `_PG_init`'s load timing (see Known divergences); at minimum, `pg_init` could carry a `DIVERGENCE` line.

**From M4, Task 6 (the seeded oracle, IVFFlat bytecmp, `ivfflat-errors.sql`)**
- `run-bytecmp.sh` has stale comments: lines 6-7 say "HNSW pages are written through GenericXLog" (IVFFlat's are too), line 17 says the empty seed "omits the SET" (it now omits two), and line 60's "Seeds pgrust (Task 5)" heads two lines. `harness_test.sh`'s `test_bytecmp_detects_difference` comment still names only `#ifdef HNSW_MEMORY`.
- `test_bytecmp_ivf_knob_off` drops both SETs, so it cannot catch IVFFlat reseeding from `pgrust.hnsw_build_seed` (no live bug: the build reads the right name).
- There is no negative control for the refusal on a missing IVFFlat oracle marker (`INFO:  k-means: `).
- `ivfflat-errors.sql`'s NULL-query checks all use `lists = 1`, so "a NULL query scores every list 0" is not discriminated. A 2-list index over two equal groups with `probes = 1` would be (N rows on C, 2N if every list were scanned).
- `build-reference.sh` keeps an existing `HNSW_MEMORY`-only `pgvec-seeded` (it rebuilds only when `vector.control` is missing). The verify step then fails with no remedy, `run-bytecmp.sh`'s refusal message is misleading for a stale seeded build, and the `rm -rf` remedy lives only in the plan.

## Coverage limits
- **wasm.** `wasm/wasm-build.sh` was not run, because the toolchain `nightly-2026-07-17` is not installed. So the new crates (`types_ivfflat`, `pgvector_ivfflat`, `pgvector_ivfflat_build`, `sampling`) are not checked against `wasm32-wasip1` (spec §4.6).
- **x86.** The harness ran only on macOS arm64. M3's per-target L2 test has amd64 values from the amd64 Docker image under emulation; the C reference's x86 F16C and AVX-512 dispatch paths are still unexercised (M5).
- **IVFFlat cosine and inner product** are covered by the regression files, TAP and `m4-ivf.sql`'s sqldiff, not by byte identity (Known divergences, "For M5").
- **Recall after a parallel build.** TAP `003`, `032` and `035` (and `012`, `020`, `024` and `028`) check recall after their "parallel" build, but pgrust built serially, so those assertions ran against serial builds. Recall margins are not visible either: the TAP output reports pass or fail only.
- **Tie order.** The byte-identical IVFFlat cases are designed so every k-means comparison has a clear winner, so they do not pin IVFFlat's distance-tie order. Ties are covered by TAP `008` and `036` (duplicate values, where build, insert and scan must all break ties toward the first list) and the unit test `ivf_kmeans_fills_every_center_from_duplicates`. HNSW's tie order rests on M1's pairing heap, as before.
- **M2's I/O limits stand.** No tier feeds C-produced binary into pgrust's recv; the recv functions and binary-only error sites are checked by unit tests only.

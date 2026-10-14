# pgvector conformance after the M4 follow-ups

Date: 2026-10-14 · pgrust commit: `06fd88bd88` (the last code commit, Task 7's fix round; this docs commit changes no code) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY -DIVFFLAT_BENCH) · Plan: `docs/superpowers/plans/2026-10-13-pgvector-m4-followups.md`

## Summary
- **C reference:** 130/130 (regress 14/14, TAP 48/48, bytecmp 28/28, iterscan 19/19, diff 21/21).
- **pgrust:** 122/130 (regress 14/14, TAP 40/48, bytecmp 28/28, iterscan 19/19, diff 21/21).
  - M4 was 121/129 (C 129/129). The tier rows changed by one net: bytecmp lost `insert_grid` (29 → 28, now opt-in), and diff gained `library_load` and `core_error_locations` (19 → 21). Both new diff rows pass on pgrust.
  - The 8 failures are the same parallel builds (M6) as in M4: TAP `003`, `012`, `020`, `024`, `028`, `032`, `035` and `045`. Each was checked against this run's logs (Remaining failures).
- **What changed** (one task each; details under "What was fixed"):
  - `insert_grid` is an opt-in known divergence. C breaks its ties by element address across two memory contexts, so C does not reproduce itself (Task 1).
  - `pg_index_has_property(.., 'bitmap_scan')` is `f` for hnsw and ivfflat indexes, as in C (Task 2).
  - A session loads `vector` where C calls the AM handler, and not where C does not (Task 3).
  - The reloptions.c, guc.c, float.c and mcxt.c errors that pgvector SQL reaches report C's `file:line, function`, not line 0 (Tasks 4 and 5).
  - The `{hnsw.c` source-map row is `hnsw.c` (Task 5).
  - Every SQL-reachable vector.c error reports its vector.c location, not `pgvector.c:0` (Task 6).
  - Databases made by pre-M4 builds can be repaired and then updated to 0.8.7 (Task 7).
- **Gate also green:**
  - `build-reference.sh --verify`: ok (13 checks).
  - `harness_test.sh`: all passed (124 ok: M4's 125, less `insert_grid`'s per-case shape check now that the case is opt-in; `test_diff_ref` expects 21 rows).
  - `run-repair-check.sh`: 6/6 passed (below).
  - `UT_KEEP`: 87 passed (`pgvector` 45, `pgvector_hnsw` 13, `pgvector_hnsw_build` 3, `pgvector_ivfflat` 8, `pgvector_ivfflat_build` 6, `sampling` 3, `types_hnsw` 6, `types_ivfflat` 3; 0 in `main_main`). This plan's `UT_KEEP` adds the IVFFlat packages and filters to M4's set: 83 through Task 5, then 87 with Task 6's four new `pgvector` tests.
  - The plan's unit tests (Task 8's filter list): 22 passed (`mcx` 5, `pgvector` 5, `guc` 3, and 2 each in `adt_amutils`, `adt_float`, `reloptions` and `types_error`, 1 in `amapi`, 0 in `main_main`). The tests this report cites that its filters miss were run too: 9 passed (catcache's `payload_alloc_failure_is_a_catchable_error`, `check_hook_failures_carry_c_locations`, `prefix_reservation_warnings_in_hash_order_with_context`, `custom_placeholder_variables`, `define_custom_string_variable_adopts_placeholder_values`, `local_reloption_locations`, `float_soft_input_errors_stay_soft`, `array_to_vector_unsupported_type_location` and `handler_dispatch_is_the_closed_set`).
  - `lint-determinism`: PASS (0 violations, 41 warnings). `lint-seam-installs`: PASS (0 violations, 22 allowlisted; the 21 stale-allowlist warnings predate this plan).
  - Both reference trees print nothing from `git status --porcelain --ignored`.
- **How the run was produced.** `build-reference.sh --verify`, then `build-pgrust.sh`, where cargo recompiled only `main_main` (4.1 s), so `target/fast-profile/postgres` is `06fd88bd88`'s Rust. Then each command ran once: `run-all.sh ref` (the two lints ran alongside it), `run-all.sh pgrust`, `harness_test.sh`, `run-repair-check.sh`, the unit tests and the reference-tree check. The table below is `run-all.sh pgrust`'s report.

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
| diff | library_load | ok |
| diff | core_error_locations | ok |

**122/130 passed**

## Remaining failures
Every remaining failure is a parallel build (M6), the same 8 tests and the same assertions as in M4. I checked each against this run's TAP output (`tap/pgrust/prove/<test>.log`). In every failure the captured stderr shows pgrust's `DEBUG:  building index "<index>" on table "tst" serially`, and every other assertion passes, including the `is($ret, 0, ...)` check on each "parallel" build and the recall checks after it.

| Test | Failed | Lines (one per opclass) |
|---|---|---|
| `003_ivfflat_vector_build_recall` | 3 of 34 | 127 ×3 |
| `012_hnsw_vector_build_recall` | 8 of 36 | 106 ×4, 123 ×4 |
| `020_hnsw_bit_build_recall` | 4 of 18 | 114 ×2, 131 ×2 |
| `024_hnsw_halfvec_build_recall` | 8 of 36 | 109 ×4, 126 ×4 |
| `028_hnsw_sparsevec_build_recall` | 8 of 36 | 106 ×4, 123 ×4 |
| `032_ivfflat_halfvec_build_recall` | 3 of 34 | 131 ×3 |
| `035_ivfflat_bit_build_recall` | 1 of 14 | 116 |
| `045_hnsw_low_memory_build` | 2 of 3 | 26, 27 |

- Every failing line except `045`'s line 27 is `like($stderr, qr/using \d+ parallel workers/)`.
- `045`'s line 27 expects `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`. C's log for this run shows `using 1 parallel workers` and then that NOTICE, so the message comes from C's parallel build.

## What was fixed

### Task 1: `insert_grid` becomes an opt-in known divergence
- **C's behavior.** On an on-disk insert, `HnswUpdateConnection` (called from `GetUpdateIndex`, `hnswinsert.c:441`) sorts candidates with `CompareCandidateDistances` and `base == NULL`, which breaks equal distances by raw element pointer (`hnswutils.c:1007-1011`).
  - The new element lives in `insertCtx`, an AllocSet (`hnswinsert.c:788`). The neighbors loaded from disk live in `updateCtx`, a Generation context (`hnswinsert.c:551`).
  - The two contexts' relative addresses vary from run to run, so on tie-heavy data C differs from itself. M4 saw it in about 2 of 37 seeded C-vs-C runs.
  - C's in-memory build has no such problem: every element comes from one Generation context (`HnswMemoryContextAlloc`, `hnswbuild.c:650`), so addresses rise with insertion order within a 1 MB block, and pgrust's element ids reproduce that order.
- **The change.**
  - `insert_grid` leaves `run-bytecmp.sh`'s default list (28 cases) and joins the opt-in list (`spill cosine insert_grid`). It still runs by name.
  - A `DIVERGENCE` note above `select_neighbors` (`pgvector_hnsw/src/utils.rs`) says pgrust ranks the new element (lowest id) first, C's usual order. A tie-order note above `select_neighbors_mem` (`pgvector_hnsw_build/src/lib.rs`) explains why the build matches.
  - Spec §8.2's byte-identical row names `insert_grid` as the third opt-in case.
- **The measurement.** `insert_sparsevec_cosine` is the other on-disk insert case on the same tie-heavy grid. 30 seeded C-vs-C runs gave 30 identical, so it stays default.
- **A correction (fix round).** The plan's comment called `big_golomb`'s data tie-free. Its p = 10007 coordinates exceed 2²⁴ and round in float4, and the reviewer measured 2,879 of 10,000 points with equal f32 distances to two neighbors (0 at p = 1009). The comment now says some distances tie and that whether a tie reaches the cross-block pointer tie-break is unmeasured. "Tie-free" now names only the p = 1009 cases.
- **Tests.** `run-bytecmp.sh --list` prints 28 cases; `run-bytecmp.sh pgrust insert_grid` runs by name (identical in Task 1's single run, which proves nothing about the divergence).

### Task 2: `bitmap_scan`
- **C's behavior.** `AMPROP_BITMAP_SCAN` returns `routine->amgetbitmap ? true : false` (`amutils.c:368-369`). Both pgvector handlers set `amgetbitmap = NULL` (`hnsw.c:315`, `ivfflat.c:232`).
- **The change.** `AmFlags` gains `has_amgetbitmap`: true for btree, hash, gist, gin, spgist, brin and bloom, false for hnsw and ivfflat. `Prop::BitmapScan` reads it.
- **Tests.** Unit `amgetbitmap_matches_c_handlers`. A one-off `sqldiff.sh` over the amutils properties of hnsw and ivfflat indexes and AMs was identical (`f` on both sides for `t_hnsw` and `t_ivf`).

### Task 3: where a session loads `vector`
- **C's behavior.** `OidFunctionCall0(amhandler)` runs `fmgr_info`, which loads a `LANGUAGE c` handler's library and runs its `_PG_init` once per session. That reserves the `hnsw` and `ivfflat` GUC prefixes and removes their placeholders with a WARNING.
  - C calls the handler at `CREATE INDEX` (`DefineIndex`, `indexcmds.c:864`), at a session's first open of an hnsw or ivfflat index (`relcache.c:1432` → `amapi.c:38`), and in amutils (`amutils.c:198`/`:458` → `GetIndexAmRoutineByAmId` → `amapi.c:106`).
  - C does not call it at `SELECT ... FROM pg_am`, or at `CREATE ACCESS METHOD` (`amcmds.c:78` checks the handler's return type without calling it), or per scan.
- **The change.**
  - `amapi`'s `GetIndexAmRoutine` calls `fmgr_info` for an hnsw, ivfflat or bloom handler. The new no-load `index_am_kind_of_handler` serves the `CREATE ACCESS METHOD` fence and the per-scan resolver.
  - `adt_amutils`' `extension_am_handler` calls `fmgr_info` and returns `PgResult`; `am_flags` returns `PgResult<Option<..>>`, so a load failure propagates while a lookup miss stays `None`.
  - `pgvector`'s `pg_init` loses its load-timing `DIVERGENCE` paragraph.
- **`scripts/pgvector/sql/library-load.sql`**, the diff row `library_load`. Each section starts a fresh session with `\c`, sets a placeholder, runs one statement, sets a second placeholder and lists the loaded modules. C prints:

  | Section | Statement | C |
  |---|---|---|
  | 1. Negative control | `SELECT amname FROM pg_am` | No WARNING. Both SETs succeed. No module row. |
  | 2. Negative control | `CREATE ACCESS METHOD hnsw2 TYPE INDEX HANDLER hnswhandler` | No WARNING. Both SETs succeed. No module row. |
  | 3. amutils | `pg_indexam_has_property(oid, 'can_order')` on hnsw | At the SELECT: `WARNING: invalid configuration parameter name "hnsw.typo", removing it` with `DETAIL: "hnsw" is now a reserved prefix.`, and the same for `ivfflat.typo`. Then `SET hnsw.typo2` fails: `ERROR: invalid configuration parameter name "hnsw.typo2"`, `DETAIL: "hnsw" is a reserved prefix.` The module row is `vector \| 0.8.7`. |
  | 4. First index open | `INSERT INTO t (id) VALUES (1)` (t has an hnsw index) | The `hnsw.typo` WARNING at the INSERT, then the `hnsw.typo2` ERROR and `vector \| 0.8.7`. |
  | 5. Failing `CREATE INDEX` | `CREATE INDEX ON t USING ivfflat (id)` | The `ivfflat.typo` WARNING, then `ERROR: data type integer has no default operator class for access method "ivfflat"` with its HINT. Then the `ivfflat.typo2` ERROR and `vector \| 0.8.7`. |
  | 6. `CREATE INDEX` | `CREATE INDEX t_v_ivf ON t USING ivfflat (v vector_l2_ops) WITH (lists = 1)` | The `ivfflat.typo` WARNING, then `NOTICE: ivfflat index created with little data` with its DETAIL and HINT. Then the `ivfflat.typo2` ERROR and `vector \| 0.8.7`. |

  - Section 5 fails after `DefineIndex`'s handler call and before anything else could load `vector`, so it isolates that load point. Section 6 matched C even before the change, because another vector function loaded the library on that path.
  - Before the change, pgrust printed no WARNING in sections 3 and 4, accepted the second SET and listed no module. All three placeholder WARNINGs came late, at `CREATE TABLE t (id int, v vector(3))` (the typmod input).
  - Two probe builds showed the row catches a regression. Without `GetIndexAmRoutine`'s load, sections 4 and 5 differ from C. With the `CREATE ACCESS METHOD` fence back on the loading path, section 2 differs from C.
- **Unit tests.** `extension_handlers_load_their_library` (amapi) counts `fmgr_info` calls per handler: `GetIndexAmRoutine` loads for the extension handlers, while `index_am_kind_of_handler` and the builtin handlers do not. `extension_am_properties_load_the_library` (adt_amutils) does the same for amutils. `handler_dispatch_is_the_closed_set` (amapi) still pins the handler dispatch and its errors.

### Tasks 4 and 5: C error locations in core
Every edit adds `.with_location(..)` and nothing else: messages, SQLSTATEs, DETAILs, HINTs and elevels are unchanged. Each line is the C call's closing `);`, checked against the vendored source. "Diff" means `scripts/pgvector/sql/core-error-locations.sql` (the diff row `core_error_locations`), run with `sqldiff.sh --verbose`, where C and pgrust print the same `LOCATION:` line; the count is how many times C prints it.

| C location | Error | Pinned by |
|---|---|---|
| `parseRelOptionsInternal, reloptions.c:1488` | `unrecognized parameter "<name>"` (heap, gist and local reloptions) | unit `reloption_enum_and_unrecognized_locations`, `local_reloption_locations`; diff ×2 |
| `parse_one_reloption, reloptions.c:1601` | `parameter "<name>" specified more than once` (heap and local) | unit `validation_error_texts_match_c`, `local_reloption_locations`; diff ×1 (`WITH (m = 8, m = 9)`) |
| `parse_one_reloption, reloptions.c:1617` | invalid value for boolean option | unit `validation_error_texts_match_c` |
| `parse_one_reloption, reloptions.c:1629` | invalid value for integer option | unit `validation_error_texts_match_c`; diff ×4 |
| `parse_one_reloption, reloptions.c:1637` | integer value out of bounds | unit `validation_error_texts_match_c`, `local_reloption_locations`; diff ×7 |
| `parse_one_reloption, reloptions.c:1649` | invalid value for floating point option | unit `validation_error_texts_match_c` |
| `parse_one_reloption, reloptions.c:1657` | floating point value out of bounds | unit `validation_error_texts_match_c` |
| `parse_one_reloption, reloptions.c:1681` | invalid value for enum option, with its DETAIL | unit `reloption_enum_and_unrecognized_locations` |
| `parse_and_validate_value, guc.c:3145` | parameter requires a Boolean value | unit `invalid_values_error_at_session_source`; diff ×1 |
| `parse_and_validate_value, guc.c:3166` | invalid value for an integer parameter | unit `invalid_values_error_at_session_source`; diff ×3 |
| `parse_and_validate_value, guc.c:3186` | integer value outside the valid range | unit `invalid_values_error_at_session_source`; diff ×6 |
| `parse_and_validate_value, guc.c:3207` | invalid value for a real parameter | unit `real_and_enum_errors_carry_c_locations`; diff ×1 |
| `parse_and_validate_value, guc.c:3227` | real value outside the valid range | unit `real_and_enum_errors_carry_c_locations`; diff ×2 |
| `parse_and_validate_value, guc.c:3282` | invalid enum value, with the `Available values` HINT | unit `real_and_enum_errors_carry_c_locations`; diff ×3 |
| `call_bool_check_hook, guc.c:6834`, `call_int_check_hook, :6868`, `call_real_check_hook, :6902`, `call_string_check_hook, :6945`, `call_enum_check_hook, :6987` | a check hook's rejection | unit `check_hook_failures_carry_c_locations` (no pgvector GUC has a check hook) |
| `assignable_custom_variable_name, guc.c:1139` | invalid configuration parameter name | unit `real_and_enum_errors_carry_c_locations` (`a..b`; `SET ivfflat..x` fails in the scanner first) |
| `assignable_custom_variable_name, guc.c:1156` | reserved-prefix ERROR | unit `define_custom_string_variable_adopts_placeholder_values`, `guc_array_add_reserved_prefix_is_invalid_name`; diff ×2 |
| `assignable_custom_variable_name, guc.c:1169` | unrecognized single-part name | unit `custom_placeholder_variables`; diff ×1 |
| `MarkGUCPrefixReserved, guc.c:5312` | the `removing it` WARNING | unit `prefix_reservation_warnings_in_hash_order_with_context`; diff ×2 |
| `float_overflow_error, float.c:90` | `value out of range: overflow` | unit `float_error_helpers_carry_c_locations`; diff (`'1e38'::float4 * 10::float4`, `1e300::float8 * 1e300::float8`, `'[3e38]'::vector + '[3e38]'`) |
| `float_underflow_error, float.c:98` | `value out of range: underflow` | unit `float_error_helpers_carry_c_locations`; diff (`'1e-38'::float4 * '1e-10'::float4`) |
| `float_zero_divide_error, float.c:106` | `division by zero` | unit `float_error_helpers_carry_c_locations`; diff (float8 and float4) |
| `float4in_internal, float.c:208` | empty input | unit `float_input_errors_carry_c_locations`; diff (`''::float4`) |
| `float4in_internal, float.c:288` | out of range for type real | unit `float_input_errors_carry_c_locations`; diff (`'1e40'::float4`, `1e39::numeric::float4`) |
| `float4in_internal, float.c:295` | invalid input syntax | unit `float_input_errors_carry_c_locations`, `float_soft_input_errors_stay_soft`; diff (`'abc'::float4`) |
| `float4in_internal, float.c:309` | trailing junk | unit `float_input_errors_carry_c_locations`; diff (`'1.5x'::float4`) |
| `float8in_internal, float.c:414` | empty input | unit `float_input_errors_carry_c_locations`; diff (`''::float8`) |
| `float8in_internal, float.c:490` | out of range for type double precision | unit `float_input_errors_carry_c_locations`; diff (`'1e400'::float8`) |
| `float8in_internal, float.c:497` | invalid input syntax | unit `float_input_errors_carry_c_locations`; diff (`'abc'::float8`) |
| `float8in_internal, float.c:511` | trailing junk | unit `float_input_errors_carry_c_locations`; diff (`'1.5x'::float8`) |
| `MemoryContextAllocationFailure, mcxt.c:1167` | `out of memory`, with the context's DETAIL | unit `oom_error_shape_matches_mcxt_c`, `ivf_kmeans_buffers_raise_out_of_memory` |
| `MemoryContextSizeFailure, mcxt.c:1180` | `invalid memory alloc request size <n>` | unit `alloc_size_gate_matches_palloc`, `allocator_rejects_over_ceiling_allocate`/`_grow`, catcache `payload_alloc_failure_is_a_catchable_error`; diff (`repeat('x', 500000000) \|\| repeat('y', 600000000)`, size 1100000004) |

- **Soft errors stay soft.** `pg_input_is_valid('abc', 'float4')` is `f` and `pg_input_error_info` gives `invalid input syntax for type real: "abc"` on both sides (diff), and `float_soft_input_errors_stay_soft` checks the saved error's SQLSTATE, message and location.
- **GUC elevels.** Only the location changed, so `ALTER SYSTEM` and config-file paths keep their elevels and messages.
- **The over-ceiling panic.** An allocation over `MaxAllocSize` without the huge flag unwinds with a bare `PgError` in `std` builds (the server's), so the statement boundary's `pg_error_from_panic` sends `MemoryContextSizeFailure, mcxt.c:1180`. The lmgr_proc and ipc containment catchers, which decode a bare `PgError`, keep its message. No-std builds keep the string panic.
- **The source-map row.** `("pgvector_hnsw", "{hnsw.c")` is `("pgvector_hnsw", "hnsw.c")`. Unit `rust_paths_map_to_c_basenames` checks the mapping, and `overrides_sorted_unique_and_non_default` now requires every reachable value to be a C basename (shown failing on the old row).
- One diff statement reaches a site outside these tables: `SET ivfflat..x = 1` reports `scanner_yyerror, scan.l:1240` on both sides. It is kept.

### Task 6: vector.c
All 24 SQL- or wire-reachable vector.c sites that lacked a location now carry `vector.c` with C's line and function. `sparsevec_to_vector`'s `index out of bounds` (`vector.c:1343`) already had one. "Diff" means a statement in `scripts/pgvector/sql/error-locations.sql`'s new vector.c section (the diff row `error_locations`).

| C location | Error | Pinned by |
|---|---|---|
| `CheckDims, vector.c:76` | different vector dimensions | diff (`'[1,2]'::vector + '[3]'`) |
| `CheckExpectedDim, vector.c:88` | expected N dimensions, not M | unit `parse_errors_carry_vector_c_locations`, `vector_combine_matches_0_8_7`; diff (typmod casts, `vector_combine`, `halfvec_combine`, `avg` over mixed dimensions, `halfvec` and `sparsevec` casts) |
| `CheckDim, vector.c:100` | vector must have at least 1 dimension | unit `vector_recv_errors_carry_vector_c_locations` (`dim = -1`); diff (`'{}'::real[]::vector`, `subvector(.., -2, 1)`) |
| `CheckDim, vector.c:105` | more than 16000 dimensions | unit `vector_combine_matches_0_8_7`; diff (concatenation, `vector_combine`, `sparsevec` cast) |
| `CheckElement, vector.c:117` | NaN not allowed | diff (literal, `real[]` cast, `vector_avg`) |
| `CheckElement, vector.c:122` | infinite value not allowed | diff (`'[Infinity,1]'::vector`) |
| `CheckStateArray, vector.c:169` | the aggregate state array check | unit `state_array_location_vector_and_halfvec`; diff (`vector_avg`, `vector_accum`, `vector_combine`) |
| `vector_in, vector.c:194`, `:204`, `:214`, `:223`, `:234`, `:240`, `:260`, `:271` | the eight input errors (missing `[`, `[]`, too many dimensions, truncated, bad number, out of range, bad separator, junk after `]`) | unit `parse_errors_carry_vector_c_locations`; diff (one literal each) |
| `vector_typmod_in, vector.c:356`, `:361`, `:366` | invalid type modifier; dimensions below 1; above 16000 | diff (`vector(3, 2)`, `vector(0)`, `vector(16001)`) |
| `vector_recv, vector.c:393` | expected unused to be 0 | unit `vector_recv_errors_carry_vector_c_locations` (binary only) |
| `array_to_vector, vector.c:459`, `:464` | array must be 1-D; must not contain nulls | diff (`'{{1}}'::real[]::vector`, `'{NULL}'::real[]::vector`) |
| `array_to_vector, vector.c:498` | unsupported array type | unit `array_to_vector_unsupported_type_location` (an `int2[]` image; no SQL cast reaches it) |
| `subvector, vector.c:998`, `:1015` | count below 1; start past the end | diff (`subvector(.., 1, 0)`, `subvector(.., 2147483647, 10)`) |

- **Signed helpers.** `check_dim` and `check_expected_dim` take `i32`, so a negative dimension from the wire's `i16` or from `subvector`'s span reports C's message at `CheckDim, vector.c:100`. `subvector` keeps C's order and its overflow-avoiding end arithmetic (`vector.c:985-1020`).
- **halfvec's override still wins.** `StateArray::check` now reports `vector.c:169`, and halfvec's `check_state_array` still relabels it as `halfvec.c:173` (both asserted by `state_array_location_vector_and_halfvec`).
- `fc_array_to_vector` now uses the shared `cast_array_elems`, which closes M2's duplication item, and `pgvector/src/lib.rs` loses the pre-M2 `DIVERGENCE` paragraph.
- **The diff section.** 36 statements: 35 print a vector.c location and 1 prints `float_overflow_error, float.c:90` (`'[3e38]'::vector * '[10]'`). `error-locations.sql` now prints 85 `LOCATION:` lines on each side, 49 before this section.

### Task 7: the pre-M4 repair
- **The problem (M4 report, "Upgrading from M1–M3 clusters").** pgrust builds before M4 installed a trimmed `vector` labelled 0.8.5. `ALTER EXTENSION vector UPDATE` then fails at `vector--0.8.6--0.8.7.sql`'s `COMMENT ON FUNCTION ivfflathandler(internal)` (line 104), because the function does not exist.
- **The repair.** `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` (1,561 lines), generated from upstream `sql/vector.sql` by the test-only `crates/contrib/pgvector/src/repair_gen.rs`.
  - It is one `DO` block with 213 guarded blocks in upstream order. Each creates a missing upstream object and adds it to the extension, so the script is atomic and idempotent.
  - It does nothing unless `vector` is at 0.8.5 (`vector is at 0.8.7; nothing to repair`), and raises if `vector` is not installed. It ends with `vector: created N missing objects; now run ALTER EXTENSION vector UPDATE`.
  - It runs the way `execute_extension_script` runs an extension script: `search_path` is the extension's schema then `pg_temp`, with `pg_catalog` searched implicitly first (`extension.c:1290-1312`), and `client_min_messages` is raised to `warning` (`extension.c:1271-1274`), then restored before the summary NOTICE.
  - `repair_script_is_generated_from_upstream` fails when the file is stale, and `PGVECTOR_REGEN_REPAIR=1` rewrites it. `repair_covers_every_upstream_object` counts 213 guards.
  - Usage is in CLAUDE.md ("Pre-M4 databases").
- **Two guards differ from the plan's table**, and both plan forms fail the same way on C PostgreSQL:
  - The full `CREATE TYPE` guard checks `pg_type` for a defined type of that name in the extension's schema. `to_regtype` returns NULL for a shell type (`parse_type.c:808-814`), so the plan's guard was NULL right after the shell block, the full type was never created, and generation `79ad992ede` failed at `CREATE AGGREGATE avg(halfvec)`.
  - The `CREATE OPERATOR` guard also requires `oprcode <> 0`. Upstream's `COMMUTATOR` and `NEGATOR` clauses leave shell operators (`pg_operator.c:370-380`), which `to_regoperator` finds, so the plan's guard skipped `>` and `>=` and generation `79ad992ede` failed at `CREATE OPERATOR CLASS halfvec_ops ... USING btree`.
- **`scripts/pgvector/run-repair-check.sh`.** A scratch pgrust server stages each generation's old script, makes a database at that 0.8.5 with 500 rows and an hnsw index, checks that the update fails, repairs twice, updates, and compares the catalog with a fresh `CREATE EXTENSION vector` (`sql/repair-verify.sql`; 484 rows). The data, the hnsw index scan and a new `ivfflat (lists = 4)` index survive. Task 7's runs:

  | Row | Before the repair | First repair | Second repair | After the update |
  |---|---|---|---|---|
  | `79ad992ede` (pgrust v0.3, 69 statements) | update fails at `ivfflathandler(internal)` | created 145 | created 0 | identical to a fresh install |
  | `88f29c4ccf` (M2, 190 statements) | same | created 24 | created 0 | identical |
  | `c1c5da11cc` (M3, 203 statements) | same | created 11 | created 0 | identical |
  | `79ad992ede/schema` (extension in schema `s`) | same | created 145 | created 0 | identical to a fresh install in `s` |
  | `79ad992ede/rollback` (repair fails at its last statement) | same | `ERROR: injected` | — | catalog unchanged (150 rows), still 0.8.5, update still fails |
  | `0.8.7/no-op` | — | `vector is at 0.8.7; nothing to repair` | — | — |

  This run: 6/6 passed with the same counts: 145, 24, 11 and 145 on the first repair and 0 on every second. Each first repair prints one NOTICE, the summary. Every catalog diff is empty, the rollback's dumps before and after are identical (150 rows), and the staged old script and the scratch server's directories are gone afterwards.
- **The dev cluster.** `~/.cache/pgrust/pgv-016`, database `postgres`: `vector` 0.8.5 with 73 members and a table `tst` (9 rows of `vector(1900)`) with an hnsw index.
  - The repair created 145 objects, and `ALTER EXTENSION vector UPDATE` took it to 0.8.7. Its catalog equals a fresh install's, `tst` is unchanged, and the plan still uses `Index Scan using tst_v_idx`. `template1` has no `vector`.
  - That repair ran before the fix round changed the script's `search_path` and `client_min_messages` handling. The created objects are the same, so it was not rerun. A rerun now prints `nothing to repair`.

## Statements dropped from diff files
One, under the plan's stop rule (a statement that does not error on C is fixed or dropped):
- `SELECT l2_normalize('[3e38,3e38]'::vector);` from `error-locations.sql`'s vector.c section. C and pgrust both return `[0.70710677,0.70710677]` (1 row). `l2_normalize` divides each element by `norm = sqrt(Σx²)`, which is at least `|x_i|`, so its overflow check is unreachable. `'[3e38]'::vector * '[10]'` already pins `float.c:90` from vector.c.

`core-error-locations.sql` and `library-load.sql`: none.

## Decisions made during execution
- **User decision (2026-10-14, Task 5):** allow a test-only edit in `crates/backend/utils/cache/catcache/src/tests.rs`, outside the plan's allowed crates, so `payload_alloc_failure_is_a_catchable_error` decodes the payload with `pg_error_from_panic`; and make mcx's over-ceiling panic payload a bare `PgError` rather than the plan's `Box<PgError>`, so the lmgr_proc and ipc containment catchers keep the message.
- Controller rulings:
  - The branch was cut from `main` at `cf8ef63d3d`, not `d4a642a645`: the one commit between is the docs-only handoff, and the older base would block a fast-forward.
  - In `harness_test.sh`'s `test_diff_ref`, every `19` on the line changes (the count and the `19 ok` text), and later every `20`.
  - Task 5's test additions in `types_error/src/source_map.rs` are within the allowed core edits; "the source-map row only" governs non-test code.
  - The plan's "9 commits" is a floor, since fix rounds add commits.
  - Task 1: `big_golomb`'s comment was rewritten to the measured truth, and "tie-free" narrowed to the p = 1009 cases.
  - Task 3: the amutils citation became `amutils.c:198 / amutils.c:458 -> GetIndexAmRoutineByAmId -> amapi.c:106`.
  - Task 3: `library-load.sql` gained the `CREATE ACCESS METHOD` and failing-`CREATE INDEX` sections, so both load points are discriminated.
  - Task 3: `install_amhandler_stub`'s comment moved back above its function.
  - Task 6: the `l2_normalize` statement was dropped (above).
  - Task 7: the repair's `search_path` is `<schema>, pg_temp`, C's form; the plan's `<schema>, pg_catalog` would let a writable schema shadow catalog objects in a superuser-run script.
  - Task 7: the fix round added the rollback row, the scratch-directory cleanup and the `client_min_messages` handling, so the check has six rows.
  - Task 7: the plan's mapping table stays as written; this report documents the shipped guards.
  - Task 8: the expected repair check is 6/6, not the plan's 5/5.

## Still open
- **lsyscache's name-matched AM arms** (`amop.rs`, plan Decision 2) do not load the library. They run only after relcache has opened an index of that AM, which already loaded it.
- **Other `unrecognized configuration parameter` constructions** that port other C functions stay unlocated (`unrecognized()` itself serves several C sites).
- **Hand-written `out of memory` errors in other crates**, each a different C site, stay unlocated. Those built through `mcx::oom_named` now report `mcxt.c:1167`.
- **pgvector_hnsw's 13 internal-guard errors**, unreachable from SQL, report `hnsw.c:0` (before, `{hnsw.c:0`).
- **The remaining source-map garbage keys.** Five path-shaped rows (`source_map_table.rs:94-106`, the pgclient row among them) can never match a lookup, so they are harmless. The value check skips them; delete them when `scripts/error-source-map.py` is restored.
- **The M3 report's `big_golomb` line** (`pgvector-m3-report.md:193`) says the multi-block tie-break is verified. After Task 1's measurement that is unproven. Reports are snapshots, so it is noted here.
- **The M4 report's deferred minors** stand, except those this plan closed (its "Resolved after M4" section).
- **This plan's deferred review minors:**
  - Task 1:
    - `run-bytecmp.sh:13-14` says `ref` proves "every case deterministic"; it should say every default case.
    - The `utils.rs` `DIVERGENCE` note covers on-disk inserts only; vacuum's repair (`RepairGraphElement` → `HnswUpdateNeighborsOnDisk`, `hnswvacuum.c:273`, `tmpCtx` at `:749`) reaches the same pointer tie-break.
  - Task 2:
    - The HNSW and IVFFlat `AmFlags` rows both cite `hnsw.c:315 / ivfflat.c:232`; one citation each would be more precise.
    - No unit test covers the `indexam_property` `BitmapScan` arm itself.
  - Task 3:
    - `GetIndexAmRoutine`'s `builtin_index_am(amhandler).is_none()` guard is redundant for extension kinds.
    - The phase-name path runs `fmgr_info` twice for hnsw and bloom, where C calls the handler once (harmless; fmgr caches).
    - `resolve_index_am_kind`'s no-load and the phase-name load path have no unit test.
    - `library-load.sql`'s header named the `core_error_locations` row before Task 4 created it (resolved by Task 4).
  - Task 4:
    - The guc `tests.rs:1270` comment says the stub hooks reject "only a sentinel"; `reject_bool_check` rejects every `true` for `default_with_oids`.
    - Pre-existing: the real-valued check-hook fallback (`registry.rs:741-747`) formats `{}` where C uses `%g` (`guc.c:6897`), so `1e-05` prints `0.00001`.
    - `guc.c:1139` has no SQL-level coverage (unit test only).
  - Task 5:
    - The mcxt_stats teardown guard reads string payloads only and logs "non-string panic payload" for any `PgError` panic (pre-existing); decoding with `pg_error_from_panic` in lmgr_proc, ipc and mcxt_stats is the durable fix.
    - The source-map value check exempts the five dead path-shaped rows (above).
  - Task 6:
    - The `loc` test helper is duplicated in `vec.rs` and `funcs.rs`.
    - `// CheckDim (vector.c:96-106)` starts at `{`, while its siblings start at the signature (94).
    - `parse_errors_carry_vector_c_locations` asserts locations only; message and DETAIL identity rest on the diff row.
    - The RED evidence covered the compile error only, not the location asserts of the other four tests.
  - Task 7:
    - `repair-verify.sql` compares members, comments, function-definition md5s, amop, amproc and opclass rows, not `pg_type`, `pg_operator`, `pg_cast` or `pg_aggregate` attributes.
    - The guards test existence, not extension membership: a planted non-member object with the same signature would be skipped, not added.
    - `run-repair-check.sh`'s unconditional verify and fresh-catalog steps abort under `set -e` with no summary line.
    - `repair_covers_every_upstream_object`'s comment says HEADER has two precondition `IF`s; it has three (the assertion is right).
    - The repair does not mirror C's other extension-script settings (`log_min_messages`, `check_function_bodies = off`); every function is `LANGUAGE C`, so there is no practical effect.
    - A user-made shell type named `vector`, `halfvec` or `sparsevec` makes the repair fail and roll back (safe, not self-healing).

## Coverage limits
- **macOS arm64 only.** Every tier ran on macOS arm64.
- **wasm not built.** `wasm/wasm-build.sh` was not run, so the changed crates are not checked against `wasm32-wasip1`.
- **GUC check hooks.** No pgvector GUC has a check hook, so the five `call_*_check_hook` locations are covered by unit tests only.
- **`insert_grid`** is outside the gate by design. Run by name, it can fail on C against C.
- **The repair's rollback** is checked by injecting a failure at the script's last statement only.

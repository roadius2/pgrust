# pgvector conformance after M3

Date: 2026-10-07 · pgrust commit: 1033c58acc (code: Task 6's, the last before the M3 docs commit, whose only code change is the `pgvector_hnsw` crate header comment) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY)

## Summary
- **C reference:** 120/120 (regress 14/14, TAP 48/48, bytecmp 21/21, iterscan 19/19, diff 18/18).
- **pgrust:** 96/120 (regress 11/14, TAP 27/48, bytecmp 21/21, iterscan 19/19, diff 18/18).
  - M2 was 59/96. M3 added 24 tier rows: bytecmp 11 (`big_golomb` and ten type cases), iterscan 12 (six `big_*` cases and six type cases) and diff 1 (`hnsw_errors`). All 24 pass on pgrust.
  - On regress+TAP, pgrust went from 25/62 to 38/62.
- **Fixed in M3:**
  - regress `hnsw_bit`, `hnsw_halfvec`, `hnsw_sparsevec`;
  - TAP `021`, `022`, `023` (bit), `025`, `026`, `027` (halfvec), `029`, `030`, `031` and `038` (sparsevec): 10 tests;
  - TAP `020`, `024` and `028` (the build-recall tests of the three types) now fail only on their parallel-build lines (M6); every serial-build assertion, recall included, passes.
- **Gate also green:**
  - `harness_test.sh`: all passed (115 ok).
  - `UT_M3`: 25 passed (`pgvector` 19: halfutils 4, sparsevec 13, lookup_cover 2; `pgvector_hnsw` 6: `parse_show_memory`, `charge_norm_value`, `hnsw_check_dim`, `hnsw_norm_value`, `type_info`, `sparsevec_check_value`; 0 in `main_main`).
  - `UT_KEEP`: 48 passed (`pgvector` 32, `pgvector_hnsw` 7, `pgvector_hnsw_build` 3, `types_hnsw` 6). Its `vec::` filter also matches `sparsevec::`, so the 13 sparsevec tests count in both sets.
  - f16 parity: 3 passed.
  - `lint-determinism`: PASS (0 violations, 41 warnings).
  - `lint-seam-installs`: PASS (0 violations, 22 allowlisted). Its 21 stale-allowlist warnings are not M3's: the branch adds no seam slot or install and leaves the allowlist alone.
  - `build-reference.sh --verify`: ok (11 checks).
  - Both reference trees print nothing from `git status --porcelain --ignored`.
- **How the run was produced.** After `build-reference.sh --verify`, `build-pgrust.sh` built `target/fast-profile/postgres` from 1033c58acc plus the docs commit's header-comment edit. Then each command ran once: `run-all.sh ref`, then `run-all.sh pgrust` (the two lints ran alongside it), then `harness_test.sh`, `UT_M3`, `UT_KEEP`, the parity tests and the reference-tree check. The table below is `run-all.sh pgrust`'s report.
- **After the final-review fix wave.** That wave made the fmgr stubs raise instead of panic, and gave `insert_sparsevec_cosine` one more inserted row. On a rebuilt binary:
  - `run-bytecmp.sh ref insert_sparsevec_cosine` and `run-bytecmp.sh pgrust insert_sparsevec_cosine` were both 1/1 identical;
  - `run-regress.sh pgrust hnsw_sparsevec hnsw_vector` passed 2/2;
  - `UT lookup_cover fmgr_stubs` passed 3/3.
  - The full table above predates the wave.

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
| tap | 036_ivfflat_bit_centers | FAIL |
| tap | 037_inputs | ok |
| tap | 038_hnsw_sparsevec_vacuum_insert | ok |
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

**96/120 passed**

## Remaining failures
Every remaining failure is IVFFlat (M4) or a parallel build (M6). I checked each against this run's logs.

**IVFFlat (M4):**
- regress `ivfflat_bit`, `ivfflat_halfvec` and `ivfflat_vector`.
  - Every added line of `regression.diffs` is one of these: `access method "ivfflat" does not exist` (27 times), `unrecognized configuration parameter "ivfflat.probes"` (once), or result rows of queries that ran without the index.
  - Every removed line is IVFFlat output: the "little data" NOTICE with its DETAIL and HINT, the `ivfflat` type, dimension, `lists` and GUC errors, the `ivfflat.*` settings' values, and the rows the index scans returned.
- TAP `001`–`009`, `032`, `035`, `036`, `040`–`042` and `048` (16 tests). The first `ERROR:` in each test's log is `access method "ivfflat" does not exist`.

**Parallel builds (M6):**
- TAP `012`, `020`, `024` and `028` fail only on `like($stderr, qr/using \d+ parallel workers/)`:
  - `012`: 8 of 36 (lines 106 and 123, for each of the 4 opclasses);
  - `020`: 4 of 18 (lines 114 and 131, 2 opclasses);
  - `024`: 8 of 36 (lines 109 and 126);
  - `028`: 8 of 36 (lines 106 and 123).
  - In each, pgrust logs `building index "idx" on table "tst" serially`.
- TAP `045` fails 2 of 3: line 26 (`using \d+ parallel workers`) and line 27 (`hnsw graph no longer fits into maintenance_work_mem after 0 tuples`).
  - C's log shows `using 1 parallel workers` and then that NOTICE, so the message comes from C's parallel build.
  - pgrust builds serially. This is the same as M1 and M2.

## What M3 established
**Type info resolved by symbol, for all four types.**
- `hnsw_get_type_info` (`pgvector_hnsw/src/utils.rs`) maps an opclass's support proc 3, by its C symbol (`pg_proc.prosrc`), to one of hnswutils.c's four static tables; with no proc 3 it is `vector`'s.
- The dimension count, the value check, normalization and the maximum dimensions all dispatch through it.
- The ten opclasses and the three `hnsw_*_support` functions come from upstream's script through `upstream-sql.sh --only-am hnsw`.
- Any other symbol raises 0A000 (Review Focus 4). The unit test `type_info_resolves_only_pgvector_symbols` checks only the symbol map; `hnsw_get_type_info`'s error branch has no test, and SQL cannot compare it with C, which crashes on the same input.
- **Support proc 3 is unreachable from SQL (Task 5 probe, controller ruling).** C rejects `SELECT hnsw_halfvec_support(NULL::internal)` at parse time: 42846, `cannot cast type unknown to internal`. pgrust ports the same check, so the `lookup` stubs are unreachable through their `internal` signatures. A superuser can still bind the symbols to a callable signature, so the final fix wave made the stubs (and M1's `hnswhandler`) raise XX000 instead of panicking.

**Byte-identical seeded builds for every opclass family (bytecmp 21/21).**
- **New type cases:**
  - `halfvec_l2_grid` and `halfvec_cosine_grid`;
  - `sparsevec_l2_grid`, `sparsevec_l2_frac`, `sparsevec_cosine_grid` and `insert_sparsevec_cosine`;
  - `bit_hamming` and `bit_jaccard`, on `bit(64)` from md5;
  - each of these builds a 39–42-block, level-2 index.
- **`wide_sparsevec`:** 1,000-non-zero values (8,016 bytes, TOASTed in the heap), 201 blocks, level 1. Each neighbor tuple goes on the next page, and inserts after the build append pages. They all match C's (Review Focus 1).
- **`insert_sparsevec_cosine`:** its last inserted row, `{1:3e38,3:1e-30}/3`, loses an element when normalized (1e-30/3e38 underflows in f32). Both sides store `{1:1}/3`, at the same offset of block 41, so Review Focus 2's stored shrink is pinned in page bytes.
- **`bq_expression`:** `binary_quantize(v)::bit(64)` with `bit_hamming_ops`, 40 blocks; the index attribute's typmod gives the dimensions (Review Focus 5).
- **`big_golomb`:** a 10,000-row `vector` build, 390 blocks, height 3. C's in-memory graph (6 MB) spans several 1 MB Generation blocks. So M1's SelectNeighbors tie-break question (pgrust breaks ties by element id, C by address) is verified at that size.

**Per-batch memory traces identical to C in all 19 iterscan cases (M1 prerequisite 1).**
- The seeded build prints `memory: N KB, tuples: T` for each search batch; pgrust prints it under `SET pgrust.hnsw_memory = on`. The tier diffs those lines with the rows and `Rows Removed by Filter`.
  - This run compared 1,874 trace lines per side, all identical.
- Stop points (`Rows Removed by Filter`, identical on both sides):
  - `big_*`: 751 (64 kB), 6545 (256 kB), 2669 (strict, 128 kB), 4727 (`ef_search` 100, 128 kB), 2739 (`LATERAL`, every loop hitting the 128 kB cap), 9990 (4 MB);
  - type cases at 64 kB: `halfvec` 139, `halfvec` cosine 132, `sparsevec` 139, `sparsevec` cosine 223 (its query normalizes to a dropped element), `bit` Hamming 663, `bit` Jaccard 608.
- That also closes M1's iterscan follow-up: an `hnsw.ef_search` column, a larger table with several caps, a `LATERAL` case whose later loops hit the cap, and cosine cases (on `halfvec` and `sparsevec`).

**Every SQL-reachable HNSW error site identical, LOCATION included (M1 prerequisite 2).**
- `scripts/pgvector/sql/hnsw-errors.sql`, the diff tier's `hnsw_errors` row: 22 errors on each side, with identical transcripts. The sites:
  - `HnswCheckDim, hnswutils.c:1373` (11), for all four types (7 of the 14 opclasses: `vector` and `halfvec` L2 and cosine, `bit` Hamming and Jaccard, `sparsevec` L2), even on an empty index;
  - `InitBuildState, hnswbuild.c:703` (1), `:709` (3), `:714` (3) and `:719` (1);
  - `SparsevecCheckValue, hnswutils.c:1392` (3), at build and at insert, before normalization.
- Before M3, pgrust reported `pgvector_hnsw_build.c:0` for 709, 714 and 719, and had neither the varbit check nor the non-zero check.
- **NULL queries (Task 2 review, controller ruling).** `ORDER BY val <-> NULL` is constant-folded, because the operator is STRICT, and never reaches the HNSW scan. So the file's NULL-query statements use upstream's `(SELECT NULL::<type>)` form (`test/sql/hnsw_vector.sql:12`), and EXPLAIN showed Index Scans on C and on pgrust.

**The sparse L2 merge is bit-exact per target.**
- `fmadd_step` (`sparsevec.rs`) fuses each scalar step on aarch64 (`fsub`, then `fmadd`, as both C builds emit) and keeps the unfused source form elsewhere.
- `l2_squared_distance_matches_c_per_arch` pins five dimension-6 cases. Each case separates fused from unfused:
  - the arm64 value is what the macOS clang reference and the Docker image's gcc arm64 build both return;
  - the amd64 value comes from the amd64 Docker image under emulation.
- While writing the plan, 12 of 12 random dimension-6 pairs equalled a fused computation on arm64.
- The L1 merge has no multiply to fuse, and pgrust's source order already matches C.
- **Effect on the diff tier.** With the seeded stream unchanged (diffrunner's pgvector suite did not change in M3), ruled records at seed 1 × 2000 went from 53 (M2) to 46. M3's other kernel change, slicing the half kernels' arrays once, keeps their arithmetic (`kernels_match_per_element_reference`).
  - The one ruled `sparsevec` `l2_distance` left (seed-1 stmt 1936) has 5 b-only entries after a's last index. That is the vectorized-tail class below.

## Known divergences left open
**The vectorized tails of 4+ b-only entries in the sparse L2/L1 kernels.**
- **What the compilers do.** The trailing loop over b-only entries is vectorized once it has 4 or more entries, and the two compilers do it differently (disassembly of the macOS clang arm64 reference and of the Docker image's gcc arm64 `vector.so`):
  - clang uses 16-wide blocks (4 accumulators, the first seeded with the running sum), then 4-wide blocks, then a scalar tail;
  - gcc sums 4-wide blocks in one lane vector, adds that to the running sum, then runs a scalar tail.
- **Example.** `{1:0.8976931}/9` against a vector with 8 trailing entries gives `1.4069117307662964` on C, while a sequential fused sum gives `1.4069116115570068`.
- **Decision.** C's two builds disagree with each other on long tails. So M3 fuses the scalar steps only and records the tails as a `DIVERGENCE` (`sparsevec.rs`, `fmadd_step`). That follows the user's M2 choice not to emulate C's vectorized reduction for `vector`.
- pgrust equals both C builds whenever at most 3 b-only entries trail. Otherwise it can differ from either in the last bit, which `pgvector-float-rel` rules in distance statements.

**The same class for the dense `vector` and `halfvec` kernels, unchanged from M2.** C sums in its compiler's vectorized, fused order (`-fassociative-math -ffp-contract=fast`); pgrust keeps source order. That is why `vector` cosine stays an opt-in bytecmp case (`cosine`), and why the ±0 clause of `pgvector-float-rel` exists.

**An unknown support proc 3 raises 0A000 where C calls it.** C trusts any proc 3's returned pointer. pgrust knows only pgvector's three support symbols and raises `hnsw type support function "<symbol>" of index "<index>" is not supported` (`DIVERGENCE` in `utils.rs`). Likewise, a hand-written opclass that gives `bit` a norm proc makes C call a NULL normalize function and crash; pgrust raises.

**The fmgr stubs raise where C returns.** `hnswhandler` and the three `hnsw_*_support` functions are resolved by access method and by symbol, never called through fmgr. Bound by a superuser to a callable signature (`CREATE FUNCTION f(int) RETURNS int AS '$libdir/vector', 'hnsw_bit_support' LANGUAGE C`), C returns its static pointer. pgrust raises XX000 `<symbol> is resolved by ..., never called through fmgr` (`DIVERGENCE` in `pgvector/src/lib.rs`; unit test `fmgr_stubs_raise_internal_error`).

**For M5.**
- M5's identical-approximate-results gate compares pgrust with the Docker image's gcc build. That build's tails differ from both clang's and pgrust's sequential order. So M5 may need to measure how often that changes a traversal, or decide to emulate gcc's tail.
- C also vectorizes the normalize functions' double-precision norm sums: sparsevec `l2_normalize` (`sparsevec.rs:904`), and halfvec's and vector's `l2_normalize`. Their order feeds the stored values of cosine indexes. pgrust sums in source order, which the user's M2 no-reduction-emulation ruling covers.

**Carried forward from M2, untouched by M3** (details in `pgvector-m2-report.md`):
- **TAP 018's worker pin.** The harness runs pgrust with C's `max_parallel_workers_per_gather = 2`. pgrust's own default (4) is unchanged, so stock pgrust still differs from C on parallel plan shape.
- **`float.c:0` locations.**
  - `adt_float`'s `float_overflow_error`/`float_underflow_error` set no location; C reports `float.c:90`/`98`.
  - `numeric_float4` reports `float.c:0`, where C reports `float4in_internal, float.c:288`.
- **`pgvector.c:0` locations.**
  - Pre-M2 `vector` errors have crate-derived locations, because `source_map_table.rs` has no `pgvector` row.
  - `halfvec_to_vector`, `sparsevec_to_vector` and `halfvec_combine` inherit this.
  - `fc_array_to_vector`'s duplication of the shared cast helpers closes with this item.
  - M1's malformed `"{hnsw.c"` entry (`source_map_table.rs:197`) is still there.
- **`pg_get_loaded_modules()`** reports `vector` as 18.6, where C reports 0.8.7 (M4).
- **The findings-file block.** `diffrunner.rs`'s raw-fs budget is 8 because the block is duplicated across five suites (copybin, copytext, copyopts, dbddl, pgvector); a shared helper would shrink it.
- **M1's build divergences.** The build-memory spill point (opt-in `spill`, M6) and `vector` cosine pages (opt-in `cosine`) are unchanged. The SelectNeighbors tie-break is now verified at 10,000 rows (above).

## Deferred minors
Review minors deferred to a later cleanup, one line each. This section carries every M2 ledger item M3 did not absorb, so `~/.cache/pgrust/m2-sdd-ledger.md` is no longer needed.

**Absorbed since the M2 ledger:**
- by M3: the half kernels' per-element `hx` bounds checks (slice once per kernel), the sparse kernel fusion (L2 fused per target; L1 has no multiply), `upstream-sql.sh`'s `grep -qxF` (its header now also documents file order), and a unit test of sparsevec `l2_normalize`'s zero-dropping path (`hnsw_norm_value_dispatches_by_type`);
- within M2, after the minor was logged: the `init_bit_vector` and halfvec `check_dims` dead-code warnings (gone once Task 4 used them), the crate header's stale bit note, and the compare test's reorder case;
- by M2's final fix wave: the halfutils module doc and citation, `fc_sparsevec_out`'s `wrapping_add`, `run-diff.sh`'s exit-code check, section regex, row count and header, the M2 report's wording items, and the exact `l2_normalize` comparison;
- M3's own: the harness self-tests' first run with `big_golomb` in the default cases (Task 6, and this run's `harness_test.sh`).

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
- `utils.rs` `hnsw_get_type_info` trusts `prosrc` without GIN's `prolang == C` check (`gin/src/util.rs:89-91`). So a non-C proc 3 whose body text is a pgvector symbol resolves to that type info (superuser-only, no panic).
- `utils.rs:116`'s comment says "as amapi resolves hnswhandler", but amapi matches `proname`, not `prosrc`.
- `utils.rs:184-192`: `hnsw_norm_value` copies the fmgr result a second time in the same memory context.
- Non-vector inserts create a `MemoryContext` and look up `pg_proc` per row (`insert.rs:914`), where C uses a cached `FunctionCall0Coll`.
- `charge_norm_value_matches_c` restates the function it checks, so the normalize-drop `tmpCtx` charge sequence rests on that test's reading of `sparsevec.c`; the iterscan trace is KB-granular. Also, `hnsw_get_type_info`'s error path has no unit test.
- `sparsevec.rs:785-786`: the L1 tail's divergence has no `DIVERGENCE` marker at the site; `fmadd_step`'s comment covers only the L2 tail.
- `pgvector/src/lib.rs:13-15`: the header's "neither emulates C's vectorized L2/L1 tails" reads oddly.
- Note for the final review: sparsevec's single-array norm loops are unfused scalar sums with no `DIVERGENCE` note, while C vectorizes them. These are the cosine norms (`sparsevec.rs:854`, `:858`) and `l2_norm`/`l2_normalize` (`:891`, `:904`), and `sparsevec_cosine_ops`' support proc 2 uses them. M2's no-reduction-emulation rule plausibly covers them.

**From M3: harness**
- `harness_test.sh:323`: `test_iterscan_memory_off` passes on any `relaxed_64k` FAIL. It should assert 0 subject memory lines and at least 1 oracle line.
- No stop-point self-test checks that `big_strict_128k` (2669) and `big_ef100_128k` (4727) stay below 9990.
- `harness_test.sh:325`: shellcheck SC1007 on `PGV_ITERSCAN_MEMORY= ` (use `''`), like the existing `PGV_BYTECMP_SEED=` line.
- `run-iterscan.sh`'s `setup_sql` builds the 10,000-row `big` index on every run, even for single non-`big` cases.
- `sqldiff.sh`'s `strip_hnsw_memory`:
  - its comment omits `hnswvacuum.c:171`, which it also strips (correctly);
  - it runs on both sides, so sqldiff alone would miss a default-on pgrust trace (regress and TAP would catch it).
- `hnsw-errors.sql`'s NULL-query statements for `bit` (`:49`), `sparsevec` (`:66`) and `vector` return the same count from a seq scan, so sqldiff cannot detect a fallback; an `EXPLAIN (COSTS OFF)` per NULL query would pin the plan.
- `upstream-sql.sh --only-am` with a typo (for example `hsnw`) exits 0 printing only headers, and `--only-am ''` silently means default mode.
- `wide_sparsevec` has only the generic 10-block self-test; a 150-block arm like `wide_golomb`'s would pin Review Focus 1.
- Five of the six new iterscan type cases stop in their first batch (no resume); only `sparsevec_cosine_drop_64k` spans several.
- `lint-seam-installs` warns of stale allowlist rows, `pgaio_io_start_readv` among them (pre-existing, not M3).
- Task 3's report lacks the diff tier's ruled count before the fusion. Its 47 findings-file lines after it are, as in this run, 46 ruled records plus the meta line. This report gives the M2 → M3 count instead (53 → 46).

## Coverage limits
- **wasm.** `wasm/wasm-build.sh` was not run, because the toolchain `nightly-2026-07-17` is not installed.
- **x86.** The per-target L2 test's amd64 values were measured on the amd64 Docker image under emulation. The harness itself ran only on macOS arm64. The C reference's x86 F16C and AVX-512 dispatch paths are still unexercised (M5).
- **Recall-test coverage.** The parallel-build assertions of 020, 024 and 028 wait for M6, while their serial recall assertions pass.
- **Tie order.** The iterscan tier's new type tables (`hv`, `sv`, `bv`) are tie-heavy, so their exact stop points rest on M1's pairing-heap tie order.
- **M2's I/O limits stand.** No tier feeds C-produced binary into pgrust's recv; the recv functions and binary-only error sites are checked by unit tests only.

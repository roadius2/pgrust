# pgvector conformance after M5

Date: 2026-10-15 · pgrust commit: `436bd1b16f` (the server's code is the M4 follow-ups': M5's only Rust edit, Task 1's `a74aad355b`, is the `#[cfg(test)]` repair generator and the repair script it writes, and this report's commit changes only `run-all.sh` and docs) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`, Apple clang 21.0.0), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY -DIVFFLAT_BENCH), Docker `pgvector/pgvector:0.8.7-pg18` (PostgreSQL 18.6, gcc) and `pgvector/pgvector:0.8.1-pg18` (PostgreSQL 18.2), both linux/arm64 and pinned by digest · Plan: `docs/superpowers/plans/2026-10-14-pgvector-m5-ondisk.md`

## Summary
- **C reference:** 137/137 (regress 14/14, TAP 48/48, bytecmp 28/28, iterscan 19/19, diff 21/21, ondisk 7/7).
- **pgrust:** 129/137 (regress 14/14, TAP 40/48, bytecmp 28/28, iterscan 19/19, diff 21/21, ondisk 7/7).
  - The M4 follow-ups were 122/130 (C 130/130). M5 adds the seven `ondisk` rows, and all seven pass on pgrust. No other tier's rows changed.
  - The 8 failures are the same parallel builds (M6) as before: TAP `003`, `012`, `020`, `024`, `028`, `032`, `035` and `045` (Remaining failures).
- **The on-disk forward gate passes.** All seven rows are `ok` in both modes: `clean_queries`, `clean_followup`, `crash_pages`, `crash_queries`, `crash_followup`, `v081_update` and `v081_followup`. pgrust booted three PostgreSQL 18 + pgvector directories written by C in Docker: one stopped cleanly, one crashed with a transaction still open, and one made by pgvector 0.8.1 and then updated. It gave C's answers on each, approximate HNSW and IVFFlat results and their tie order included, and after the crash it replayed all 42 fixture relations to C's pages.
- **Stability:** the forward gate passed 7/7 for `ref` and for `pgrust` on each of three fresh captures, each with new HNSW graphs and new k-means centers (six `7/7` lines; Stability).
- **The reverse report** (C booting what the subject wrote; reported, not gated): every row is `ok`, `identical` or `clean` in both modes. `clean_pgrust_rows` is `none` for `ref` and lists three pgrust-only `pg_proc` rows for pgrust. C replayed pgrust's crash WAL to the same pages as pgrust's own replay, and `pg_amcheck` reported nothing.
- **Spec §9's M5 exit criteria hold:** "Forward gate passes (clean, crash, 0.8.1 upgrade); reverse report produced." The forward rows are above. `reverse.md` is written in both modes, and `run-all.sh` now prints it under "Reported, not gated".
- **One parked M4 follow-up is closed:** the repair prologue's `DECLARE` types are qualified (Task 1; Follow-ups closed).
- **Gate also green:**
  - `harness_test.sh`: all passed (161 ok: the M4 follow-ups' 124 plus M5's 37; 372 s). Its own capture, a fourth fresh one, read 547,090 Generic records after the redo point (2,625 with full-page images) and 75,541 from the open transaction, and `test_ondisk_ref` passed 7/7 on it.
  - The plan runs unit tests and lints in Task 1 only, the only task that touches Rust. Task 1's run (`.superpowers/sdd/2026-10-14-pgvector-m5-ondisk/task-1-report.md`): `cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_ lookup_cover shipped_scripts` gave 6 passed, `repair_prologue_qualifies_its_types` among them; `run-repair-check.sh` gave 6/6; `lint-determinism` PASS (0 violations, 41 warnings); `lint-seam-installs` PASS (0 violations, 22 allowlisted).
  - Both reference trees print nothing from `git status --porcelain --ignored`.
- **Commits** (`5fcc2ba802..`, this report's last):

  | Commit | Task | Change |
  |---|---|---|
  | `236b0d85bd` | — | the M5 plan |
  | `a74aad355b` | 1 | the repair prologue qualifies its types |
  | `a5f93c67d1` | 2 | Docker helpers, the 0.8.1 pin, immediate stops |
  | `e73c30ba46` | 3 | the on-disk fixture SQL |
  | `d9d02983ef` | 4 | `run-ondisk.sh`: the capture and the forward gate |
  | `913342598d` | 4, fix round | the capture and `crash_pages` fail closed |
  | `8dcb1297b5` | 5 | the reverse report |
  | `436bd1b16f` | 5, fix round | reverse failures become rows and never abort the run |
  | this report's | 6 | `run-all.sh` gains the tier; the spec, CLAUDE.md and this report |

- **How the run was produced.** `build-pgrust.sh` found `target/fast-profile/postgres` current: cargo finished in 1.78 s without rebuilding, and the binary is Task 2's build of `a74aad355b`, unchanged since. Then each command ran once, in this order:
  1. the stability loop (Task 6 Step 2): three `run-ondisk.sh capture --force`, each followed by `run-ondisk.sh ref clean crash v081` and `run-ondisk.sh pgrust clean crash v081`;
  2. `run-all.sh ref`, then `run-all.sh pgrust`. Their `ondisk` tier reused the loop's third capture (key `eaf2dac728f0242d`, a cache hit), so its facts are the final capture's below;
  3. `harness_test.sh`, which makes its own captures under `pgvector-work/selftest`;
  4. the reference-tree check.

  Timings: `run-all.sh ref` 349 s and `run-all.sh pgrust` 485 s, each including the `ondisk` tier's reverse part; `harness_test.sh` 372 s. The table below is `run-all.sh pgrust`'s report.

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
| ondisk | clean_queries | ok |
| ondisk | clean_followup | ok |
| ondisk | crash_pages | ok |
| ondisk | crash_queries | ok |
| ondisk | crash_followup | ok |
| ondisk | v081_update | ok |
| ondisk | v081_followup | ok |

**129/137 passed**

## Remaining failures
Every remaining failure is a parallel build (M6): the same 8 tests and the same assertions as in M4 and the M4 follow-ups. I checked each against this run's TAP output (`tap/pgrust/prove/<test>.log`). The 8 tests fail 37 assertions. In every failure the captured stderr shows pgrust's `DEBUG:  building index "<index>" on table "tst" serially`, and every other assertion passes, including the recall checks after each "parallel" build.

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

- 36 of the 37 failing lines are `like($stderr, qr/using \d+ parallel workers/)`.
- `045`'s line 27 expects `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`. C's log for this run (`tap/ref/log/045_hnsw_low_memory_build/regress_log_045_hnsw_low_memory_build`) shows `using 1 parallel workers` and then that NOTICE, so the message comes from C's parallel build.

## What M5 built
**Files** (Tasks 1–6):
- `scripts/pgvector/common.sh`: the 0.8.1 image pin (`PGV_DOCKER_IMAGE_081`), and `server_stop <mode> [INT|QUIT]`, whose QUIT is an immediate stop that leaves a cluster needing recovery.
- `scripts/pgvector/docker-lib.sh` (new): Docker helpers for one container, `pgrust-pgvector-ondisk` on port 55493. `dk_start_new` runs an image's own initdb, `dk_boot_dir` boots a host directory through a read-only mount that the container copies on its first start (so the host copy is never written), and there are clean and immediate stops and a copy-out.
- `scripts/pgvector/run-ondisk.sh` (new): the capture, the forward gate (seven rows) and the reverse report (eight rows).
- `scripts/pgvector/sql/ondisk-rows.sql`: the row generator. Every value keeps index-scan distances exact (below).
- `scripts/pgvector/sql/ondisk-fixture.sql`: four tables (`vt`, `ct`, the unlogged `ut` and the TOASTed `wide`), 25 vector indexes, deletes and `VACUUM ANALYZE`.
- `scripts/pgvector/sql/ondisk-crash.sql`: the crash's committed writes after its `CHECKPOINT`: inserts into every index, deletes, `VACUUM`, more inserts.
- `scripts/pgvector/sql/ondisk-inflight.sql`: the transaction left open, whose rows (ids 4001–4200) reach every table and index before it sleeps.
- `scripts/pgvector/sql/ondisk-queries.sql`: per index an `EXPLAIN` proving the scan uses it, then nearest-neighbour queries at the default and at a narrower `hnsw.ef_search` or `ivfflat.probes`; iterative scans with a filter; the in-flight row count; and one wrong-dimension query per access method (`expected 8 dimensions, not 3`).
- `scripts/pgvector/sql/ondisk-followup.sql`: inserts, deletes and `VACUUM`, then counts, exact (no-index) answers, and index-scan row counts before and after `REINDEX`.
- `scripts/pgvector/sql/ondisk-update.sql`: the queries on the 0.8.1 catalog with the 0.8.7 library, `ALTER EXTENSION vector UPDATE`, `repair-verify.sql`'s catalog dump (version, members and comments, function definitions, amop, amproc, opclasses), then the queries again.
- `scripts/pgvector/sql/ondisk-relfiles.sql`: `name|rmgr|path` of every relation the fixture owns (its tables, their TOAST tables and the indexes of both), for the page comparison.
- `scripts/pgvector/tests/harness_test.sh`: seven new self-tests with 37 checks: `test_server_stop_immediate` (2), `test_docker_lib` (12), `test_ondisk_sql` (7), `test_ondisk_capture` (10), `test_ondisk_ref` (2), `test_ondisk_detects_difference` (1) and `test_ondisk_reverse_ref` (3).
- `scripts/pgvector/run-all.sh`: the `ondisk` tier, and a "Reported, not gated: on-disk reverse" section after the total, which the reverse rows never enter.
- `crates/contrib/pgvector/src/repair_gen.rs` and `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql`: Task 1 (Follow-ups closed).

**The capture** (`run-ondisk.sh capture`): C pgvector writes three directories in Docker.
- **clean:** the 0.8.7 image runs its own initdb (`--no-locale --encoding=UTF8`) with autovacuum off, loads the fixture, records the queries' transcript, and stops cleanly.
- **crash:** the same, with `checkpoint_timeout = 1d` and `max_wal_size = 10GB`. After the fixture comes a `CHECKPOINT`, then `ondisk-crash.sql`. Then `ondisk-inflight.sql` starts in the background, and the capture waits until its transaction sleeps with its rows written and records its xid. One more transaction (`CREATE TABLE crash_marker`) commits, which flushes WAL past every record of the open one. Then `pg_ctl stop -m immediate`. The copy must read `in production`.
- **v081:** the 0.8.1 image (PostgreSQL 18.2) loads the fixture and stops cleanly.
- The capture is cached under `$PGV_WORK/ondisk/capture`, keyed by a hash over the image pins, `sql/ondisk-*.sql`, `sql/repair-verify.sql`, `docker-lib.sh` and `run-ondisk.sh`; `--force` rebuilds it. A capture is 449 MB, and the forward rows need no Docker once it exists.

**The oracle is C booting the same bytes** (plan Decision 2).
- The 0.8.7 image boots a copy of each directory and records the transcripts of `ondisk-queries.sql` (`ondisk-update.sql` for v081) and of `ondisk-followup.sql`. For the crash it first replays the directory, lists the fixture's relations (`relfiles.txt`), stops cleanly, and keeps that replayed directory for the page comparison.
- For the crash, the open transaction's index entries may or may not reach disk before the stop, so only C's own replay is ground truth. For clean, the capture also checks that C's answers before the stop equal its answers after the reboot. For v081, the oracle is the 0.8.7 image after the same `ALTER EXTENSION vector UPDATE`, as an in-place upgrade does; the 0.8.1 library is not the oracle.
- The subject (`pgrust`, or `ref`: the local `pgvec` built with clang on macOS) boots its own copy of each directory and must reproduce C's transcripts byte for byte.

**The seven forward rows** (gated):

| Row | Directory | Compared with C |
|---|---|---|
| `clean_queries` | clean | the `ondisk-queries.sql` transcript |
| `clean_followup` | clean | the `ondisk-followup.sql` transcript: DML tags, counts, exact answers, index row counts |
| `crash_pages` | crash | after the subject's crash recovery and a clean stop, every relation in C's `relfiles.txt`, main and `_init` forks, against C's replay, after `pagemask` (`heap`, `btree` or `generic`) |
| `crash_queries` | crash | the queries, after the subject restarts |
| `crash_followup` | crash | the follow-up |
| `v081_update` | v081 | `ondisk-update.sql`: the queries on the 0.8.1 catalog, the update, the catalog dump, the queries again |
| `v081_followup` | v081 | the follow-up |

**The eight reverse rows** (reported in `reverse.tsv` and `reverse.md`, never gated):

| Row | What it shows |
|---|---|
| `clean_boot` | The subject loads the fixture into a fresh cluster (C `initdb`, autovacuum off), records its own query transcript and stops cleanly. The 0.8.7 image boots a copy: `ok`, or `failed: <step>`. |
| `clean_queries` | C's query transcript on that directory against the subject's: `identical` or `differs (<diff>)` |
| `clean_amcheck` | `pg_amcheck --install-missing --heapallindexed -d postgres` in the container: `clean` or `corruption reported (<file>)` |
| `clean_pgrust_rows` | the `pg_proc` rows named `pgrust%` that C sees, or `none` |
| `clean_followup` | C's follow-up on the subject-written directory against C's follow-up on its own clean capture |
| `crash_boot` | The subject loads the fixture, runs `CHECKPOINT`, the crash writes and the open transaction, and stops immediately (QUIT, leaving `in production`). It replays its own crash, lists the relations, stops, and restarts for its query transcript. The 0.8.7 image replays the same directory: `ok`, or `failed: <step>`. |
| `crash_pages` | the subject's replay against C's replay of the same WAL, every relation in the subject's list, after `pagemask` |
| `crash_queries` | C's queries after its replay against the subject's after its own |

If the reverse part aborts, an `aborted` row (`failed: exit <n> (see the run's output)`) follows the rows already written.

**Review fixes beyond the plan's text.** The controller ruled each one in; each is a fix to a gap in the plan's own code.
- **Task 4's fix round (`913342598d`): the capture and `crash_pages` fail closed.**
  - The capture's relation-list query stops on SQL errors (`ON_ERROR_STOP=1`) and dies on failure or on empty output. Before, a failing query left an empty `relfiles.txt`, and `crash_pages` compared nothing and passed.
  - `compare_pages` fails when a relation's main fork is missing from C's replay, whether or not the subject has it. Every relation has a main fork, even at zero blocks; only `_init` forks may be absent on both sides. A scratch run showed the old function passing a relation absent on both sides, and the new one failing it.
  - The capture dies if C's own follow-up transcript holds an `ERROR:` line. Spec §8.2's pass rule says "follow-up DML and maintenance succeed", and before, a C error that the subject mirrored passed the row.
- **Task 5's fix round (`436bd1b16f`): reverse failures become rows.**
  - The reverse part runs in its own errexit scope (`set +e; ( set -e; trap cleanup EXIT; reverse_clean; reverse_crash ); rc=$?; set -e`). Any failure, a `die` included, ends only that subshell, whose trap removes the container and the open transaction's psql. The parent then appends `aborted` = `failed: exit <rc> (see the run's output)`, stops the subject and removes the container.
  - `reverse.md` is always written, from whatever `reverse.tsv` holds. The script's exit status stays the forward rows' alone.
  - If the subject's relation list fails or comes back empty, `crash_pages` reads `failed: the subject's relation list (<path>)` and compares nothing; `crash_queries` still runs. This mirrors Task 4's capture-side guard.
  - A scratch run with an emptied relation list and an injected failure produced exactly those two rows, wrote `reverse.md`, exited 0, and left no container or server behind.

## The fixture and the float question
**Every fixture value makes index scans compute exact distances** (plan Decision 3, `sql/ondisk-rows.sql`):
- `v`, `h`, `s` (vector, halfvec, sparsevec): small integers, so L2, inner-product and L1 sums stay below 2²⁴ and every partial sum is an exactly representable integer.
- `vc`, `hc`, `sc`, `gc`, `ghc` (the cosine columns): 16 entries of ±1, so every norm is 4 and every normalized entry is ±0.25.
- `b`, `cb` (bit): Hamming and Jaccard are counts.
- `g`, `gh` (the IVFFlat L2 and inner-product columns): integer points around ten irregular centers, 50 × (0..31) apart per dimension, with offsets −1..2.
- `e` (`wide`): 1,536 md5-derived integers in 0..4.
- The query values follow the same rules (`ondisk-queries.sql`'s `q8`, `q8b`, `q16`, `s32`, `s64`, `b64` and `u4`, and the stored rows 13, 27 and 1230). Task 3's reviewer emulated the sums in Python: every `g` and `gh` L2 and inner-product sum for query rows 13 and 27 is below 2²⁴, the largest 6,936,505.

So the order and fusing of C's vectorized sums cannot change a result, and M2's no-emulation ruling stands. For this fixture, that answers the M4 report's "For M5" question (gcc's vectorized tails against pgrust's source order) by design, not by emulating gcc.

**Measured: the local clang build matched the gcc image everywhere.** In `ref` mode the subject is the local `pgvec`, built with Apple clang 21.0.0, reading directories that the gcc-built image wrote. It passed 7/7 on all three stability captures, in `run-all.sh ref`, and on the self-test suite's own capture (`test_ondisk_ref`). In the other direction, the gcc image's answers and replayed pages matched what the clang build wrote (`ref`'s reverse rows, all `identical`). There was no `ref` failure, so no float-rounding stop.

**IVFFlat center distances are the one rounded comparison.**
- Centers are k-means means, and the spherical k-means of the inner-product and cosine opclasses also normalizes them, so a query's distance to a center rounds. The subject computes that distance itself from C's stored centers, so a near-tie between two centers could send it to a different list than C. The row distances, the ones that are returned and ordered, stay exact. `ct_cb_ham`'s centers are bits, so its center distances are exact too.
- **L2 (`ct_g_l2`, `ct_gh_l2`):** the clusters are far apart (multiples of 50 per dimension) and every point lies within −1..2 of its cluster's base, so the probes are not near-ties (the plan's argument; Task 3's review found the L2 centers well separated).
- **Inner product (`ct_g_ip`, `ct_gh_ip`): the argument does not hold** (Task 3 review). These indexes cluster by angle (spherical k-means: support procs 3, `vector_spherical_distance`, and 4, `vector_norm`), and the `i % 10 = 0` cluster sits at the origin, where direction is arbitrary. So their probe choices (`ondisk-queries.sql:85`, `:91` and `:101`) have no designed margin.
- **The evidence for the inner-product probes, and for the cosine ones, is the measurement:** three fresh captures, each with new k-means centers, passed 7/7 on pgrust and on `ref`, at `probes = 1` and at `probes = 3`.

## What the capture exercised
The final capture is the stability loop's third. `run-all.sh` reused it, so it is the one behind the gate's results above. Its `facts.tsv` (`~/.cache/pgrust/m5-stability-facts-3.tsv`, the same bytes as `pgvector-work/ondisk/capture/facts.tsv`):

```
clean_state	shut down
v081_state	shut down
crash_state	in production
crash_generic_records	546200
crash_generic_fpi	2636
crash_heap_records	3257
crash_btree_records	2552
crash_inflight_generic	75698
crash_relations	42
vector_indexes	25
wide_toasted	t
v081_before	0.8.1
v081_after	0.8.7
```

Across the three stability captures (`~/.cache/pgrust/m5-stability-facts-{1,2,3}.tsv`):

| Fact | What it counts | Capture 1 | Capture 2 | Capture 3 |
|---|---|---|---|---|
| `crash_generic_records` | `pg_waldump --rmgr=Generic` records after the crash's redo point | 545,053 | 546,019 | 546,200 |
| `crash_generic_fpi` | of those, records with full-page images | 2,628 | 2,624 | 2,636 |
| `crash_heap_records` | Heap records after the redo point | 3,257 | 3,257 | 3,257 |
| `crash_btree_records` | Btree records after the redo point | 2,552 | 2,552 | 2,552 |
| `crash_inflight_generic` | Generic records of the open transaction's xid | 75,327 | 75,587 | 75,698 |
| `crash_relations` | fixture relations whose pages are compared | 42 | 42 | 42 |
| `vector_indexes` | distinct vector indexes the clean queries scan | 25 | 25 | 25 |

All three also read `shut down` for clean and v081, `in production` for crash, `wide_toasted t`, and v081 `0.8.1` → `0.8.7`.
- **Ranges:** Generic records 545,053–546,200, of which 2,624–2,636 carry full-page images; the open transaction's records 75,327–75,698; relations and indexes constant at 42 and 25. Only the Generic counts vary; Heap and Btree are the same in every capture. That fits each capture's new HNSW graph: how many neighbor pages an insert updates, and which pages get their first post-checkpoint image, depend on the random levels. (The capture records the open transaction's xid only once the transaction sleeps with all its rows written.)
- **The 42 relations** are 41 owned by the fixture (4 tables, 4 TOAST tables, 4 TOAST indexes, 4 primary keys and the 25 vector indexes) plus `crash_marker`.

## Stability
The brief's loop (Task 6 Step 2), with timestamps and a stop that would have copied a failing capture to `~/.cache/pgrust/m5-capture-fail-<n>` before the next iteration replaced it. It never fired, and no such directory exists.

```
run 1 ref: 7/7
run 1 pgrust: 7/7
run 2 ref: 7/7
run 2 pgrust: 7/7
run 3 ref: 7/7
run 3 pgrust: 7/7
```

- Each iteration took 3 minutes 19 s to 3 minutes 25 s: the capture 94–100 s, then `ref` and `pgrust` together 105 s. The whole loop took 10 minutes 4 seconds (01:19:17 to 01:29:21). Logs are in `~/.cache/pgrust/m5-stability-{1,2,3}.log`.
- **The captures differ, so the gate does not rest on one graph.** The final capture against the self-test capture that Task 5 left (`pgvector-work/selftest/ondisk/capture`, the same key; compared before this task's self-test run replaced it) differs in 145 lines of C's clean query transcript, 150 of the crash transcript and 286 of the v081 update transcript. The differences are tie order among equal distances, different rows at the same distance (for example L1 at the default `hnsw.ef_search`), and different rows at different distances (the IVFFlat inner-product queries, whose lists depend on the centers, and HNSW at `hnsw.ef_search = 10`). The three follow-up transcripts are identical, as their comparison rule expects. pgrust reproduced each capture's own answers exactly.

## Reverse
**`pgrust`** (`report-pgrust.md`, "Reported, not gated"; `ondisk/pgrust/reverse.md` has the same rows):

| Check | Result |
|---|---|
| clean_boot | ok |
| clean_queries | identical |
| clean_amcheck | clean |
| clean_pgrust_rows | pgrust_pin_database pgrust_seal_template pgrust_unpin_database |
| clean_followup | identical |
| crash_boot | ok |
| crash_pages | identical |
| crash_queries | identical |

**`ref`** (`report-ref.md`: the local clang build writes, the gcc image boots):

| Check | Result |
|---|---|
| clean_boot | ok |
| clean_queries | identical |
| clean_amcheck | clean |
| clean_pgrust_rows | none |
| clean_followup | identical |
| crash_boot | ok |
| crash_pages | identical |
| crash_queries | identical |

In both modes the subject's crash relation list has 42 lines with the same names and page types as C's capture (9 heap, 8 btree, 25 generic; TOAST names aside, which carry oids), and `pages.txt` (the page differences), `relfiles.log` (the list's errors) and `amcheck.txt` (`pg_amcheck`'s output) are empty.

- **The pgrust-only `pg_proc` rows.** pgrust's first client connection to a database inserts three builtins, `pgrust_pin_database`, `pgrust_unpin_database` and `pgrust_seal_template`, with their descriptions (`crates/backend/postmaster/janitor/src/bootstrap.rs`). They are `LANGUAGE internal` rows in `pg_catalog` with oids 9001, 9002 and 9005 from the reserved range, and pinned. C sees all three in a directory that pgrust wrote. The forward direction adds them too: a C-written database gains them on pgrust's first connection, which logs `backfilled 3 builtin function row(s)` (the plan's facts). `ref` lists `none`.
- **`pg_amcheck` is clean** on the pgrust-written clean directory, `--heapallindexed` included. It checks heap and btree relations only, not HNSW or IVFFlat indexes (Coverage limits).
- **C replays pgrust's crash WAL page-identically.** After C's replay of the directory pgrust crashed, every relation in pgrust's relation list equals pgrust's own replay after `pagemask` (`crash_pages identical`), and C's answers equal pgrust's (`crash_queries identical`). This compares two replays of the same WAL, so it cannot show WAL that pgrust failed to write (Coverage limits).
- C's follow-up on the pgrust-written clean directory is identical to C's follow-up on C's own clean capture (`clean_followup identical`).

## Findings and known divergences
- **No plan-defined stop was hit.** No forward row failed in any run, in either mode. `ref` never failed, so no float-rounding stop. No Rust, core or fixture-SQL edit was made, and `docs/fuzzing/rulings.toml` is untouched. The M5 ledger records no user decision; every ruling below is the controller's.
- **New known divergence: pgrust adds three `pg_proc` rows to every database it connects to** (Reverse). C boots, answers and passes `pg_amcheck` with them present. They make a C directory that pgrust has opened differ from one it has not, which matters for spec §2.1's reverse conditions (a way back without dump/restore, mixed replication, C physical tooling on pgrust's data).
- **The open transaction's WAL volume varies by capture** (75,327 to 75,698 Generic records), like the other Generic counts (What the capture exercised). Its rows stay invisible after replay on both sides (`inflight` count 0 in every crash transcript); the graph still holds them, and the pages match C's.
- **C does not reproduce its own approximate answers across separately built captures** (Stability), and Task 5 saw the same across separately built crash clusters. This is expected: unseeded HNSW levels and k-means. Every gated comparison is between boots of the same directory, so no verdict depends on it.

**Decisions made during execution** (controller rulings, from the M5 ledger):
- The branch was cut from `main` at `5fcc2ba802`, not the plan's `65a5820abc`: the one commit between is a docs-only handoff refresh, and the older base would block a fast-forward.
- Task 4: fix the capture's relation list and `compare_pages` (a row that can pass on zero comparisons breaks the gate's meaning), and make "C's follow-up succeeded" a capture check (spec §8.2's pass rule). Above, under "Review fixes beyond the plan's text".
- Task 4: harness scripts run under `#!/usr/bin/env bash`, which is Homebrew bash 5.3 here. The constraint requires only `/bin/bash -n`, which passes, so this was accepted.
- Task 5: run the reverse part in its own errexit scope with an `aborted` row (Decision 7 and `docker-lib.sh`'s contract make reverse a report whose rows record failures), and guard the subject's relation list.
- Every other review minor is deferred (Still open).

## Coverage limits
- **`wal_compression = lz4` or `zstd`.** pgrust's WAL reader refuses LZ4 and zstd full-page images (`restore_image_core`, `crates/backend/access/transam/xlogreader/src/lib.rs:2253-2258`: `NotSupportedByBuild`), while the Docker build has both (`pg_config --configure` lists `--with-lz4` and `--with-zstd`). A C primary or crash directory written with either setting cannot be replayed by pgrust. The fixture runs with the default (`off`).
- **x86 data directories are untested.** Both images ran as linux/arm64 (`docker image inspect`: `arm64`; `uname -m` in the container: `aarch64`), and the local reference is macOS arm64.
- **No relation reaches a `.1` segment.** The largest fixture file in the crash capture is 3,284,992 bytes, and no `*.1` file exists under its `base/`. `compare_pages` compares segment 0 only.
- **After DML, index answers are compared by count only.** The follow-up counts index-scan rows of 9 of the 25 indexes, before and after `REINDEX`, because new HNSW elements draw random levels and `REINDEX` reruns k-means. `ut_u_ip` (IVFFlat on the unlogged table) is not among the 9: after the crash it is scanned only while still empty from the reset, never after the follow-up's inserts. Before DML, all 25 indexes are scanned and compared row by row.
- **Exact data only.** The identical-results claim rests on values whose distances are exact. With real embeddings, pgrust's source-order sums can differ from gcc's vectorized ones in the last bit, which can change an HNSW traversal or an IVFFlat probe on a near-tie. The approximate differential (M7) measures recall there, not identity.
- **Parallel builds (M6) and the approximate differential (M7)** are not covered. C built every fixture index serially: every heap is below `min_parallel_table_scan_size` (8 MB; even after the crash writes the largest table, `vt`, is 1,368,064 bytes, and `wide`'s TOAST table 3,276,800), so `plan_create_index_workers` plans no workers. pgrust builds serially.
- **Reverse:**
  - `pg_amcheck` checks heap and btree only; nothing checks the pgrust-written HNSW and IVFFlat indexes structurally. They are covered by C's answers on them (`clean_queries`, `crash_queries`) and by C's follow-up (`clean_followup`).
  - The reverse crash rows compare pgrust's replay with C's replay of the same pgrust WAL. WAL that pgrust failed to write would read identical on both sides, and the rows never compare with C's own crash answers.
  - There is no reverse `crash_followup` row: C does not run the follow-up on the crashed pgrust directory.
  - The reverse part has no negative self-test (the perturbation hook is forward-only).
- **wasm is unchanged.** `wasm/wasm-build.sh` was not run. M5 changed no non-test Rust; `repair_gen` is `#[cfg(test)]`.
- **macOS arm64 only** for the subject and the local reference, as before.

## Follow-ups closed
- **Task 1: the repair prologue's types** (`a74aad355b`). The M4 follow-ups' final review parked it (`~/.cache/pgrust/m4f-sdd-ledger.md`: "Final: parked — repair prologue still resolves the type names `text`/`oid` (DECLARE) through the caller's search_path"; to qualify them "next time the generator is touched").
  - The `DECLARE` block of `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` now reads `pg_catalog.text` and `pg_catalog.oid` on its five lines that named them (`v_ver`, `v_nsp`, `v_nsp_oid`, `v_old_path`, `v_old_cmm`). `n int` is unchanged: `int` is a grammar keyword that resolves to `pg_catalog.int4`.
  - New unit test `repair_prologue_qualifies_its_types` (`repair_gen.rs`). RED: `unqualified type in v_ver text;`. GREEN after the change, with the regenerated script (`repair_script_is_generated_from_upstream`).
  - `run-repair-check.sh`: 6/6, with the same rows as the M4 follow-ups report.

## Still open
**The two deferred core items** (plan Decision 8; both are core edits outside M5's theme):
- lmgr_proc, ipc and mcxt_stats should decode panic payloads with `pg_error_from_panic`. Today the mcxt_stats teardown guard reads string payloads only and logs "non-string panic payload" for any `PgError` panic (M4 follow-ups, Task 5 minor).
- guc's real-valued check-hook fallback (`registry.rs:741-747`) formats `{}` where C uses `%g` (`guc.c:6897`), so `1e-05` prints `0.00001` (M4 follow-ups, Task 4 minor).

**The M4 follow-ups report's open items** stand (`pgvector-m4-followups-report.md`, "Still open"). M5 closed none of them. In short:
- lsyscache's name-matched AM arms do not load the library where C's do (invisible for pgvector; bloom differs);
- other `unrecognized configuration parameter` constructions, hand-written `out of memory` errors in other crates, and mcx's `add_size`/`mul_size` errors stay unlocated;
- pgvector_hnsw's 13 internal-guard errors report `hnsw.c:0`;
- the five dead path-shaped source-map rows;
- the M3 report's `big_golomb` lines;
- the M4 report's still-standing known divergences and deferred minors;
- the final review's declined-to-judge items (PL/pgSQL `EXCEPTION` re-raising panic payloads, `amapi`'s `proname` against amutils' `prosrc`, bloom's load timing, `run-repair-check.sh`'s port and SIGKILL residue, kind-before-load for foreign handlers, x86_64 and wasm untested);
- that plan's deferred review minors, Tasks 1–7, and Task 7's implementer's note on user-made shell types. The two Decision 8 items above are among those minors.
- The M4 follow-ups ledger's other parked item stays open too: the M4 report's "lists every load point" (`pgvector-m4-report.md:349`) omits handler calls that come after an earlier load in the same command (`index.c:299`, `execAmi.c:617`, `opclasscmds.c:885`/`:1168`/`:1252`/`:1360`). It is wording only.

**This plan's deferred review minors**, by task (from the M5 ledger; line numbers are at `436bd1b16f`):
- Task 1 (4):
  - `repair_gen.rs:263-264`: the test checks only each declaration's type token, so a future unqualified function in a `DECLARE` default would pass.
  - `repair_gen.rs:255-256`: "`int` is a keyword, not a catalog lookup" is imprecise; the grammar maps `INT_P` to `pg_catalog.int4` (`gram.y:14572-14574`, `:19133-19137`). Plan-mandated text.
  - `repair_gen.rs:260`: bare `unwrap()`s on the `DECLARE`/`BEGIN` splits; `.expect()` would name the cause.
  - Pre-existing noise in the reported output: an `unused_imports` warning in `crates/pl/plpgsql/src/exec.rs`, two stale-allowlist WARNs, and 41 determinism warnings.
- Task 2 (6):
  - `docker-lib.sh:106-109`: `dk_stop_immediate` swallows `pg_ctl`'s failure and then `docker wait`s without a bound, so it hangs if `DK_DATA` is empty or stale, and returns 0 on an already-stopped container. Every planned caller sets `DK_DATA` first, so it is latent. A bounded poll of `.State.Running` would fix it. Plan-mandated.
  - `harness_test.sh:416-419`: `cluster_state` (and the `v="$(…)"` and `ctl="$(pg_controldata …)"` lines and unguarded `dk_psql` INSERTs in `test_docker_lib`) abort the whole self-test under `set -euo pipefail` instead of recording a FAIL. Plan-mandated.
  - `docker-lib.sh:57-62` and `:77-86`: a failed `docker run` can leave a Created container; callers run `dk_rm`, but the function could do it itself.
  - `test_docker_lib`'s `dk_stop_fast` stderr is not redirected to the test log.
  - `test_docker_lib`'s "host directory untouched" compares `pg_controldata` only; checking that the host's `pg_hba.conf` lacks the trust line would test copy-not-mount directly.
  - `harness_test.sh:5-7`: the isolation comment says only Docker and port 55493 are shared, but `port_for`'s fixed ports (55491, 55492, 55494) are shared too. Plan-mandated text.
- Task 3 (4):
  - The inner-product IVFFlat indexes cluster by angle, so the well-separated argument holds for L2 only (The fixture and the float question). Plan-mandated premise; the stability run measured it.
  - `harness_test.sh:535`: "every index returns full rows" overstates: the follow-up counts 9 of 25 indexes, and `ut_u_ip` is never scanned after the follow-up's inserts (Coverage limits). Plan-mandated wording.
  - `harness_test.sh:540`: the update check counts 4 `ERROR`s but not their text.
  - `harness_test.sh:525-526`: the relation-list check counts 41 rows, not the heap/btree/generic split (8/8/25).
- Task 4 (8):
  - `run-ondisk.sh:108-111`: `close_transaction` `wait`s with no kill or timeout, so it could hang if Docker's proxy kept the client socket open; a `kill` before the `wait` is free.
  - `run-ondisk.sh:254-255`: a `pagemask` failure on a subject file dies, losing the remaining rows instead of recording `crash_pages` FAIL.
  - `run-ondisk.sh:288`: a boot-FAIL row names `$PGV_WORK/log/server-$subject.log`, which the next `server_start` truncates (`common.sh:142`, `:145`); copying the log into the row's directory after each boot would keep it.
  - `run-ondisk.sh:256-258`: files that differ only in length report "block 0, offset 0" (cmp says EOF); the row still fails.
  - `harness_test.sh:613-625`: the negative control changes only the heap, so the suite never shows `crash_pages` catching an HNSW or IVFFlat page difference (the Task 4 reviewer showed `pagemask` can).
  - Nits: `compare_pages`' local `rm` variable sits beside the `rm` command (`:235`, `:263`); shellcheck SC2034 on the unused loop variable `i` (`:91`, `:95`); `capture_key` (`:113-118`) omits `common.sh` and the `PG_TOOLS` psql (a spurious-FAIL risk only, never a false pass).
  - `run-ondisk.sh:130`: the follow-up guard matches `ERROR:` only, so an empty transcript or a `psql: error: connection` line would pass (unlikely after `dk_boot_dir`'s readiness wait).
  - `run-ondisk.sh:169`: `[ -s ]` accepts a `relfiles.txt` of blank lines only (implausible).
- Task 5 (10):
  - Failure paths write fewer than eight rows: `reverse_clean`'s early returns (`run-ondisk.sh:350`, `:356`) write only `clean_boot`; `reverse_crash`'s (`:386`, `:392`, `:398`, `:407`, `:414`) drop `crash_pages` and `crash_queries`, and `:430` drops `crash_queries`. Plan-mandated.
  - `reverse_clean` never checks that the subject's clean stop left `shut down` (the crash path checks at `:391`); C's crash recovery would still read `clean_boot ok`.
  - `clean_amcheck` (`:363-367`) labels any non-zero exit "corruption reported", but `pg_amcheck` exits 2 for corruption and 1 for an operational failure. It also covers heap and btree only (Coverage limits).
  - `clean_pgrust_rows` (`:368`) is empty, not `failed:`, when `dk_psql` fails.
  - The reverse crash rows compare two replays of the same pgrust WAL, with no comparison against C's own crash answers and no `crash_followup` row (Coverage limits). Plan-level.
  - The final echo (`:503`) names `reverse.md` even when reverse was not a part, and `rm -rf "$out"` has removed any earlier copy. Plan-mandated.
  - `harness_test.sh:645`: `tr … <"$r" 2>/dev/null` prints "No such file" when `reverse.tsv` is missing (the redirections apply in order). Plan-mandated.
  - No negative self-test for reverse (`PGV_ONDISK_PERTURB_SQL` is forward-only).
  - `run-ondisk.sh:450`: after an abort, the parent's bare `server_stop "$subject"` dies again if the abort was a truly hung server (server_stop's 30 s limit), exiting 1 without `reverse.md`; `( server_stop "$subject" ) || true` would harden it. The ruling prescribed the bare call.
  - An aborted reverse keeps about 976 MB of data copies under `$out/reverse/crash` until the next run.

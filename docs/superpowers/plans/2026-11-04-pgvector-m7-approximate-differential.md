# pgvector M7: the approximate differential — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meet M7's exit criteria (spec §9) so the user can say that a pgrust server (their name for it: pgj-rust) works natively with vectors: on real embeddings, pgrust's recall matches C pgvector's for every index type, serial and parallel, and every gated tier is green.

**Architecture:** A new tier, `scripts/pgvector/run-recall.sh`, in the shape of `run-buildtime.sh`: a baseline server (stock C) and a subject server (pgrust, or the seeded C build in `ref` mode) load the same GloVe rows. Exact answers come from the baseline's sequential scan, and every recall measurement is SQL on the server, so the harness gains no new language. `recall-data.sh` owns the download and the deterministic preparation.

**Decisions this plan makes (for review):**
1. **Dataset: GloVe 6B** (`glove.6B.zip`, public domain, 400,000 real word embeddings, 862,182,613 bytes, cached at `~/.cache/pgrust/recall/`). It is plain text, so the harness needs only `awk`; `python3` here has no numpy, and the HDF5 sets would add a new dependency. The default is 100 dimensions; Task 4's scale run uses 300.
2. **Size: 100,000 rows and 1,000 held-out queries**, measured below to fit about 19 minutes.
3. **Types:** `vector` and `halfvec` use the cosine opclass on the same embeddings; `sparsevec` keeps each embedding's 16 largest-magnitude entries; `bit` is `binary_quantize(v)` with Hamming distance. Other opclasses' distance functions are covered by the diff tier. That is 7 type and AM pairs (IVFFlat has no `sparsevec`).
4. **Serial and parallel builds are both in the tier,** plus one `hnsw_vector_spill` case: a parallel build at 64 MB that overflows, so participants finish with on-disk inserts. M6 left open that no run has a worker do the overflow flush; which participant flushes is not controllable, so the tier records it and the report says whether a worker did. 15 cases in all.
5. **The baseline is stock C, not the seeded build, with the median of 3 builds on each side.** Stock C is what the claim is measured against. Seeding would not remove parallel nondeterminism, and C's own serial and parallel builds differed by up to 0.0074 in single builds, which is too close to the 0.01 margin. In `ref` mode the subject is the seeded build, as in every other tier, so the rule is tested C against C.
6. **Ground truth is tie-tolerant.** A returned row counts when its distance is at most the true k-th distance plus `1e-5`. `bit` needs this because Hamming distances tie heavily. The epsilon covers pgrust's rounding: measured over all 1,000 queries, exact k-th distances differ from C's by at most 6.3e-07, and by 0 for `bit`. The same rule applies to both sides.
7. **Rows:** each case gives `<case>@10` and `<case>@100` (30 rows), plus `exact_vector`, `exact_halfvec`, `exact_sparsevec` and `exact_bit` (subject's exact scan against C's truth on 100 queries). 34 rows, so `run-all.sh` goes from 139 to 173.
8. **Search parameters:** HNSW `m = 16`, `ef_construction = 64`, `hnsw.ef_search = 100`; IVFFlat `lists = 316`, `ivfflat.probes = 20`. `ef_search` must be at least 100 or `LIMIT 100` returns too few rows. These keep C's recall off the ceiling, where a difference could not show.
9. **A baseline outside 0.5 to 0.995 fails the row,** because such a comparison cannot detect a difference.
10. **`run-buildtime.sh` is rerun** on AC power with the machine awake (Task 4), since M6's tables were measured through Maintenance Sleep. It stays a report.
11. **Not in M7:** merging upstream v0.4-beta (next, after M7, on `merge/upstream-v0.4-beta`); capture-input changes (they would force a recapture); halfvec build speed (see the facts below; reported only).

**Tech Stack:** bash 3.2, `awk`, `psql`, PostgreSQL 18.6 + pgvector 0.8.7 C reference (`pgvec`, `pgvec-seeded`). No Rust changes are expected.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, §8.2's Approximate differential row and §9's M7 row. Inputs: `docs/HANDOFF_2026-10-22.md` §1 and §3.

**Facts verified while writing this plan** (2026-11-04, macOS arm64, AC power, `main` at `6a41ec7fcc`, one throwaway spike, one build per cell, 100,000 rows, 100 dimensions):
- Download: `https://downloads.cs.stanford.edu/nlp/data/glove.6B.zip`, sha256 `617afb2fe6cbd085c235baf7a465b96f4112bd7f7ccb2b2cbd649fed9cbcf2fb`. `nlp.stanford.edu` did not respond, and Hugging Face's copy has different bytes.
- Preparation takes 35 s. Truth on C: 6, 11, 19 and 12 s for `vector`, `halfvec`, `sparsevec` and `bit`. Each case's 1,000 queries take 1-2 s.

| Case (C / pgrust) | serial build, s | 2 workers, s | recall@10 at `ef_search` 100 or `probes` 20, serial | recall@100, serial |
|---|---|---|---|---|
| HNSW `vector` | 10 / 18 | 4 / 6 | 0.9811 / 0.9815 | 0.9257 / 0.9269 |
| HNSW `halfvec` | 9 / 55 | 4 / 17 | 0.9779 / 0.9799 | 0.9244 / 0.9246 |
| HNSW `sparsevec` | 32 / 44 | 12 / 14 | 0.9487 / 0.9496 | 0.8372 / 0.8388 |
| HNSW `bit` | 10 / 14 | 3 / 5 | 0.9788 / 0.9736 | 0.9287 / 0.9270 |
| IVFFlat `vector` | 1 / 2 | 1 / 2 | 0.9369 / 0.9341 | 0.8851 / 0.8831 |
| IVFFlat `halfvec` | 0 / 12 | 1 / 7 | 0.9419 / 0.9419 | 0.8864 / 0.8913 |
| IVFFlat `bit` | 0 / 1 | 0 / 1 | 0.8755 / 0.8741 | 0.7839 / 0.7872 |

- Spill at `maintenance_work_mem = 64MB` with 2 workers: C flushes after 58,127 tuples in 15 s (0.9772, 0.9233); pgrust after 58,139 in 17 s (0.9812, 0.9247).
- **Estimated tier time:** about 19 minutes in `pgrust` mode (3 builds per side, 15 cases), less in `ref` mode. `run-all.sh pgrust` goes from about 6 to about 25 minutes.
- **`halfvec` builds are about 6× C's on pgrust** (HNSW 55 s against 9 s). A performance finding for the M7 report, not a gate.
- `capture_key` (`ondisk-capture.sh:82-88`) hashes the image pins' values, `psql --version`, `sql/ondisk-*.sql`, `sql/repair-verify.sql`, `docker-lib.sh` and `ondisk-capture.sh`. This plan changes none of them, so nothing here forces a recapture.
- At the toy size (2,000 rows, 16 dimensions) both servers launch 2 workers for HNSW and IVFFlat at `3073kB`, `8MB` and `2GB`, and HNSW at `3073kB` flushes after 0 tuples, so `recall_toy`'s settings exercise the same checks.

## Global Constraints

- Pass rule, verbatim from spec §8.2: `pgrust recall ≥ pgvector recall − 0.01`, for recall@10 and @100, HNSW and IVFFlat, all types.
- Never edit `crates/postgres-18.6-reference/`, `crates/pgvector-0.8.7-reference/` or any on-disk capture input.
- Never widen a rule or `docs/fuzzing/rulings.toml` without asking the user.
- All downloads and state stay outside the repo, under `~/.cache/pgrust/`.
- Harness scripts must run on bash 3.2 (no `case` inside `$( )`).
- `summary.tsv` has exactly two tab-separated columns, `name` and `ok` or `FAIL`; `run-all.sh` reads nothing else.
- Commit trailers name the implementing model.
- Push only when the user asks.
- **Plan-defined stops, to the user through AskUserQuestion:** `ref` mode fails any row at full size, or pgrust fails a row after root-cause work finds no pgrust bug. The rule and margin stay as they are until the user rules.

## Review Focus

1. **Recall that silently isn't approximate:** if the planner picks a sequential scan, recall reads 1.0 and proves nothing. Every measurement must first show `Index Scan using rec_idx` (Task 2).
2. **A parallel case that silently built serially,** for example under `PGRUST_VECTOR_INDEXBUILD_POOL=0`: the case must die rather than pass (Task 2).
3. **A broken subject that still looks fine:** a subject degraded by a lower `probes` or `ef_search` must produce FAIL rows, and a too-short result must lower recall, not shrink the denominator (Task 2).
4. **A corrupt or partial download:** a truncated zip or an interrupted preparation must never be used, and must not leave a directory a later run trusts (Task 1).
5. **Distances the subject gets wrong in its own favour:** recall trusts the subject's reported distance, so the subject's exact answers are checked against C's in the `exact_*` rows (Task 2).

---

### Task 1: The dataset: `recall-data.sh` and `recall-prep.awk`

**Files:**
- Create: `scripts/pgvector/recall-data.sh`, `scripts/pgvector/recall-prep.awk`
- Test: `scripts/pgvector/tests/harness_test.sh` (add `test_recall_prep`, `test_recall_download_check`)

**Interfaces:**
- Produces: `recall-data.sh <rows> <queries> <dim> <nnz>` prints one line, the absolute path of a directory holding `base.tsv` and `query.tsv`, and exits 0. Each line of both files is `<line number>\t[<dense values>]\t{<i:x,...>}/<dim>`, values copied verbatim from the source text, `i` 1-based.
- Env: `PGV_RECALL_CACHE` (default `$HOME/.cache/pgrust/recall`); `PGV_RECALL_TEXT` (self-test only: a GloVe-format text file, used instead of the zip).

- [ ] **Step 1: Write the failing tests.** `test_recall_prep` writes a 30-line, 4-dimension text fixture and runs `recall-data.sh 20 10 4 2` with `PGV_RECALL_TEXT` and a temporary `PGV_RECALL_CACHE`. It asserts:
  - `base.tsv` has 20 lines and `query.tsv` has 10, with query ids 3, 6, ... 30 (`stride = rows / queries + 1`, a line is a query when its number divides by the stride);
  - a fixture line `w 0.5 -0.9 0.1 0.5` gives `[0.5,-0.9,0.1,0.5]` and `{1:0.5,2:-0.9}/4` (largest magnitudes, ties to the lower index);
  - a second run prints the same path;
  - `recall-data.sh 21 10 4 2` (rows not a multiple of queries) and a fixture line with the wrong field count both die, and leave no prepared directory;
  - a fixture shorter than `rows + queries` dies and leaves no prepared directory.

  `test_recall_download_check` puts a garbage `glove.6B.zip` in a temporary cache, runs `recall-data.sh 20 10 4 2` without `PGV_RECALL_TEXT`, and asserts it dies with `checksum` in its message and leaves no prepared directory. Neither test touches the network.

- [ ] **Step 2: Run them to verify they fail.**
Run: `scripts/pgvector/tests/harness_test.sh test_recall_prep test_recall_download_check`
Expected: FAIL (no `recall-data.sh`).

- [ ] **Step 3: Implement.**
  - Source: `glove.6B.zip` member `glove.6B.<dim>d.txt` through `unzip -p`, or `PGV_RECALL_TEXT`. The script reads lines 1 to `rows + queries`.
  - Download with `curl -fsSL` to `glove.6B.zip.part`, then rename. Check the sha256 (Facts) before every preparation; a mismatch dies with `checksum`.
  - Prepare into `prep-<key>.part`, verify the two line counts, then rename to `prep-<key>`, so a partial result is never trusted. `<key>` is the first 16 hex digits of a sha256 over `recall-prep.awk`, the four arguments and the source's identity (the pinned sha, or the fixture's sha).
  - Check before reading: `rows % queries == 0`. The awk script exits non-zero on a line without `dim + 1` fields, which covers a truncated stream as well as the line-count check.

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `scripts/pgvector/tests/harness_test.sh test_recall_prep test_recall_download_check`
Expected: all passed.

- [ ] **Step 5: Prepare the real data.**
Run: `scripts/pgvector/recall-data.sh 100000 1000 100 16`
Expected: prints a `prep-*` path; `base.tsv` has 100000 lines and `query.tsv` 1000. (The zip is already cached.)

- [ ] **Step 6: Commit.**

### Task 2: The tier: `run-recall.sh`

**Files:**
- Create: `scripts/pgvector/run-recall.sh`
- Test: `scripts/pgvector/tests/harness_test.sh` (add `recall_toy`, `test_recall_ref`, `test_recall_detects_difference`, `test_recall_guards`, `test_recall_pgrust`; extend `test_stale_results`)

**Interfaces:**
- Consumes: `recall-data.sh <rows> <queries> <dim> <nnz>`.
- Produces: `run-recall.sh {pgrust|ref} [case ...]` and `run-recall.sh --list`. Output in `$PGV_WORK/recall/<mode>/`: `summary.tsv` (name, result); `recall.tsv` (case, side, build number, build ms, recall@10, recall@100, rows returned, flush tuples or empty, flushing participant `leader`, `worker` or empty); `report.md`.
- Env, with defaults: `PGV_RECALL_ROWS` 100000, `PGV_RECALL_QUERIES` 1000, `PGV_RECALL_DIM` 100, `PGV_RECALL_NNZ` 16, `PGV_RECALL_BUILDS` 3 (odd), `PGV_RECALL_LISTS` 316, `PGV_RECALL_EF_SEARCH` 100, `PGV_RECALL_PROBES` 20, `PGV_RECALL_SPILL_MEM` 64MB, `PGV_RECALL_BAND` `0.5 0.995`. Self-test only: `PGV_RECALL_PERTURB_SUBJECT`, a `name=value` setting applied last on the subject, to its builds and its searches.

**The cases** (`--list` prints these, one per line):

| Case | Column, operator, opclass | Build |
|---|---|---|
| `hnsw_vector_serial`, `hnsw_vector_parallel`, `hnsw_vector_spill` | `v`, `<=>`, `vector_cosine_ops` | see below |
| `hnsw_halfvec_serial`, `hnsw_halfvec_parallel` | `h`, `<=>`, `halfvec_cosine_ops` | |
| `hnsw_sparsevec_serial`, `hnsw_sparsevec_parallel` | `s`, `<=>`, `sparsevec_cosine_ops` | |
| `hnsw_bit_serial`, `hnsw_bit_parallel` | `b`, `<~>`, `bit_hamming_ops` | |
| `ivfflat_vector_serial`, `ivfflat_vector_parallel` | as `hnsw_vector` | |
| `ivfflat_halfvec_serial`, `ivfflat_halfvec_parallel` | as `hnsw_halfvec` | |
| `ivfflat_bit_serial`, `ivfflat_bit_parallel` | as `hnsw_bit` | |

Builds: `serial` sets `max_parallel_maintenance_workers = 0` and `parallel_workers = 0`; `parallel` and `spill` set both to 2. `maintenance_work_mem` is `2GB`, or `PGV_RECALL_SPILL_MEM` for `spill`.

- [ ] **Step 1: Write the failing tests.** `recall_toy` is a helper that writes a 2,020-line, 16-dimension fixture with `awk` (deterministic, `sin`-based) and runs `run-recall.sh` with `PGV_RECALL_TEXT`, `ROWS=2000 QUERIES=20 DIM=16 NNZ=4 BUILDS=1 LISTS=10 PROBES=3 SPILL_MEM=3073kB BAND="0 1.01"` and a temporary `PGV_RECALL_CACHE`.
  - `test_recall_ref`: `recall_toy ref` exits 0; `summary.tsv` has 34 rows, all `ok`; `recall.tsv` has 30 rows (15 cases, 2 sides, 1 build); every parallel-spill row has a flush count.
  - `test_recall_detects_difference`: `PGV_RECALL_PERTURB_SUBJECT=ivfflat.probes=1 recall_toy ref ivfflat_vector_serial` exits non-zero with `ivfflat_vector_serial@10` FAIL; `hnsw.ef_search=10` with `hnsw_vector_serial` gives `hnsw_vector_serial@100` FAIL, and its `recall.tsv` subject rows show fewer rows returned than the baseline's.
  - `test_recall_guards`: `enable_indexscan=off` on `ivfflat_vector_serial` dies with `not an index scan`; `max_parallel_maintenance_workers=0` on `hnsw_vector_parallel` dies with `expected 2 parallel workers`; `PGV_RECALL_BAND="0.9999 1"` gives a FAIL row whose `report.md` line says `cannot detect a difference` even though the subject equals the baseline in value; `PGV_RECALL_BUILDS=2` dies with `odd`; an unknown case name dies with `unknown case`.
  - `test_recall_pgrust`: `recall_toy pgrust` passes; `PGRUST_VECTOR_INDEXBUILD_POOL=0 recall_toy pgrust hnsw_vector_parallel` dies with `expected 2 parallel workers`.
  - `test_stale_results`: add `stale_results_check recall PG_VEC_SEEDED`.

- [ ] **Step 2: Run them to verify they fail.**
Run: `scripts/pgvector/tests/harness_test.sh test_recall_ref`
Expected: FAIL (no `run-recall.sh`).

- [ ] **Step 3: Implement `run-recall.sh`** on `run-buildtime.sh`'s shape: `baseline=ref` always; `subject` is `pgrust` or, in `ref` mode, `seeded`. Validate arguments first, then `rm -rf` the output directory, then check the subject's binary (`missing <path>`, for `stale_results_check`), so a usage error leaves old results and a later failure leaves none.

  **Load, on each side:** `rec_base` and `rec_query` with `id int, v vector(d), h halfvec(d), s sparsevec(d), b bit(d)`. COPY `id, v, s` from the prepared files into a staging table, then derive `h` as `v::halfvec(d)` and `b` as `binary_quantize(v)::bit(d)`; `VACUUM ANALYZE rec_base`. `rec_truth (space text, qid int, d10 float8, d100 float8)`.

  **Truth, before any index exists:** on the baseline, for each space the selected cases need, one exact query per space, then `\copy` the table out and into the subject:

  ```sql
  INSERT INTO rec_truth
  SELECT '<space>', q.id, x.d[10], x.d[100]
  FROM rec_query q, LATERAL (
    SELECT array_agg(d ORDER BY d) d
    FROM (SELECT b.<col> <op> q.<col> AS d FROM rec_base b ORDER BY 1 LIMIT 100) s) x;
  ```

  **`exact_<space>` rows:** the same query on the subject for the 100 lowest query ids; `ok` when the largest `abs` difference from `rec_truth` in `d10` and `d100` is at most `1e-5`.

  **Each case:** for each side, for each build: set the build's settings, then `CREATE INDEX rec_idx ...` under `client_min_messages = debug1` and `\timing`, keeping the log. Die naming the case and side if:
  - the build fails;
  - the log's `using N parallel workers` line does not say 2 for `parallel` or `spill` (`expected 2 parallel workers`), or exists for `serial`;
  - a `spill` log has no `no longer fits into maintenance_work_mem` line.

  Record the flush count, and record `worker` when that NOTICE's next line is `CONTEXT:  parallel worker`, otherwise `leader`. Then with `enable_seqscan = off`, `hnsw.ef_search` and `ivfflat.probes` set, run `EXPLAIN (COSTS OFF)` on the search below and die with `not an index scan` unless it shows `Index Scan using rec_idx`. Then measure, `DROP INDEX`, and clear the reloption:

  ```sql
  CREATE TEMP TABLE got AS
  SELECT q.id AS qid, n.d FROM rec_query q, LATERAL (
    SELECT b.<col> <op> q.<col> AS d FROM rec_base b ORDER BY b.<col> <op> q.<col> LIMIT 100) n;
  CREATE TEMP TABLE ranked AS
  SELECT qid, d, row_number() OVER (PARTITION BY qid ORDER BY d) AS r FROM got;
  SELECT count(*) FILTER (WHERE r <= 10 AND d <= t.d10 + 1e-5) / (10.0 * <queries>),
         count(*) FILTER (WHERE r <= 100 AND d <= t.d100 + 1e-5) / (100.0 * <queries>),
         count(*)
  FROM ranked JOIN rec_truth t ON t.qid = ranked.qid AND t.space = '<space>';
  ```

  The denominators are fixed, so short results lower recall. Read results from stdout only: the seeded build prints `INFO:  memory:` lines to stderr.

  **Verdict:** for each case and each k, take the median of the builds on each side. FAIL if the subject's median is below the baseline's minus 0.01, or the baseline's median is outside `PGV_RECALL_BAND` (`report.md` says `cannot detect a difference`). Exit non-zero if any row is FAIL.

  **`report.md`:** parameters, `uname -sm`, one table of every case and k (baseline median, subject median, difference, verdict), and one table of median build times and subject / baseline. Remove the clusters unless `PGV_KEEP_DATA=1`; stop both servers in an EXIT trap.

- [ ] **Step 4: Run the tests to verify they pass.**
Run: `scripts/pgvector/tests/harness_test.sh test_recall_ref test_recall_detects_difference test_recall_guards test_recall_pgrust test_stale_results`
Expected: all passed.

- [ ] **Step 5: Run the tier at full size in both modes**, on AC power with the machine awake.
Run: `scripts/pgvector/run-recall.sh ref && scripts/pgvector/run-recall.sh pgrust`
Expected: 34/34 `ok` in each, each within 25 minutes. Keep both `report.md` files and `recall.tsv` files in `~/.cache/pgrust/m7-evidence/task2/`. A failing row goes through superpowers:systematic-debugging and then Global Constraints' stops.

- [ ] **Step 6: Commit.**

### Task 3: Wire the tier into `run-all.sh`, and update the spec

**Files:**
- Modify: `scripts/pgvector/run-all.sh:19` (`tiers` gains `recall`, last)
- Modify: `scripts/pgvector/tests/harness_test.sh` (`test_run_all_exit_status`)
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (§8.2 Approximate differential row, §8.3 list, §10's upstream line)

**Interfaces:**
- Consumes: `run-recall.sh {pgrust|ref}`, `summary.tsv`.

- [ ] **Step 1: Update the failing test.** In `test_run_all_exit_status`, add `stub_tier "$w/bin" recall 0 "r:ok"`. The expected totals become `**5/9 passed**` and, with `recall` added to the all-ok loop, `**7/7 passed**`. The interrupted-run check also requires that `$w/recall/ref/summary.tsv` is gone.

- [ ] **Step 2: Run it to verify it fails.**
Run: `scripts/pgvector/tests/harness_test.sh test_run_all_exit_status`
Expected: FAIL on the totals.

- [ ] **Step 3: Add `recall` to `tiers` in `run-all.sh`.**

- [ ] **Step 4: Run it to verify it passes.**
Run: `scripts/pgvector/tests/harness_test.sh test_run_all_exit_status`
Expected: all passed.

- [ ] **Step 5: Update the spec.** Keep §8.2's pass rule word for word. Add to the row's method: GloVe 6B, 100,000 rows and 1,000 held-out queries; the 15 cases; the median of 3 builds; the tie-tolerant truth with its `1e-5`; the `exact_*` rows and the baseline band; 34 rows. Add `run-recall.sh` and `recall-data.sh` to §8.3. Replace §10's "periodically rebase" with a pointer to the merge policy, since that line predates the pushed fork.

- [ ] **Step 6: Commit.**

### Task 4: Final gates, the scale run, the report

**Files:**
- Create: `docs/superpowers/reports/pgvector-m7-report.md`
- Modify: `CLAUDE.md` ("Vector search (fork work)"), `docs/HANDOFF_2026-10-22.md` or a new dated handoff

**Interfaces:**
- Consumes: everything above. Evidence goes in `~/.cache/pgrust/m7-evidence/`.

Run every step on AC power with the machine awake, and record `pmset -g batt` and `pmset -g log | grep -E 'DarkWake|Sleep'` around each timed command.

- [ ] **Step 1: Run every gated tier through the wrapper.**
Run: `scripts/pgvector/build-pgrust.sh && scripts/pgvector/run-all.sh ref && scripts/pgvector/run-all.sh pgrust`
Expected: `**173/173 passed**` in both.

- [ ] **Step 2: Run the checks that were not rerun after M6's final fixes.**
Run: `scripts/pgvector/tests/harness_test.sh`; `scripts/pgvector/run-repair-check.sh`; the client suite (`~/.cache/pgrust/pgvector-rust-check/run.sh 55491`, set up as its header says).
Expected: all passed, with a count of 201 plus this plan's new checks; 6/6; 26/26.

- [ ] **Step 3: Run the unit tests, loom models and lints.**
Run the commands in `docs/HANDOFF_2026-10-22.md` §2 and CLAUDE.md: the pgvector crates' tests, `RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`, and the four lints.
Expected: all pass; `lint_determinism` reports the same 43 warnings. Both reference trees are clean (`git status` on `crates/postgres-18.6-reference` and `crates/pgvector-0.8.7-reference`), and the capture key is still `8342dca63fe99622`.

- [ ] **Step 4: Run the claim on the shipped binary, at scale.** Build `cargo build --release --locked --bin postgres`, then:
Run: `PGRUST_PROFILE=release PGV_RECALL_ROWS=396000 PGV_RECALL_DIM=300 PGV_RECALL_NNZ=48 PGV_RECALL_LISTS=630 PGV_RECALL_BUILDS=1 scripts/pgvector/run-recall.sh pgrust hnsw_vector_serial hnsw_vector_parallel ivfflat_vector_serial`
Expected: every row `ok`; `report.md` copied to `~/.cache/pgrust/m7-evidence/scale/`. This is a report, not a gate, so a single build's noise is expected; a row below the rule is rerun with `PGV_RECALL_BUILDS=3` before it is treated as a finding.

- [ ] **Step 5: Rerun the build-time report.**
Run: `scripts/pgvector/run-buildtime.sh pgrust`
Expected: a table with no sleep events inside it, every launched count as forced.

- [ ] **Step 6: Write the M7 report and update CLAUDE.md.** The report leads with the claim and its evidence, then: gate counts, the recall table, the scale run, build times including the `halfvec` finding, the spill rows (how many flushes a worker did, which closes or narrows M6's coverage gap), and what stays open. CLAUDE.md gains `run-recall.sh` and `recall-data.sh` in the command list, the M7 report in the baselines, the dataset's cache path, and the capture-key list's missing items (`sql/repair-verify.sql`, the image pins, psql's version). Refresh the handoff: next step is the v0.4-beta merge (§3), with Phase 1 closed.

- [ ] **Step 7: Commit, then run the final whole-branch review** (superpowers:requesting-code-review), fix what it confirms, and rerun the tiers those fixes touch.

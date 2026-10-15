# pgvector M5: the on-disk tiers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Meet M5's exit criteria (spec §9): the on-disk forward gate passes (clean, crash, 0.8.1 upgrade), and the reverse report is produced. Also close one parked M4 follow-up: the repair prologue's unqualified types.

**Architecture:**
- **A new tier, `scripts/pgvector/run-ondisk.sh`**, built on new Docker helpers (`scripts/pgvector/docker-lib.sh`) and eight fixture SQL files (`scripts/pgvector/sql/ondisk-*.sql`).
- **Capture.** C pgvector writes three data directories in Docker, and C's answers come from the 0.8.7 image booting a copy of each:
  - **clean:** the 0.8.7 image, the fixture, a clean stop;
  - **crash:** the 0.8.7 image, the fixture, `CHECKPOINT`, more writes, a transaction left open, then `pg_ctl stop -m immediate`;
  - **v081:** the 0.8.1 image, the fixture, a clean stop. Its oracle runs `ALTER EXTENSION vector UPDATE` first.
- **Forward (gated, 7 rows).** The subject (pgrust, or the local C reference in `ref` mode) boots another copy and must reproduce C's transcripts exactly. After the crash, every fixture relation must replay to the same pages as C's replay. Then the subject runs DML and maintenance, and the results that do not depend on random choices must equal C's.
- **Reverse (reported, never gated).** The subject writes the same fixture into a fresh cluster, once stopped cleanly and once crashed. The 0.8.7 image boots it, and the report compares answers and replayed pages and runs `pg_amcheck`.
- **No Rust change.** Spikes and a full draft of this plan's harness both passed against the current `main`. The only Rust edit is Task 1's test-only repair generator.

**Decisions this plan makes (for review):**
1. **M5 is a harness milestone.** pgrust already boots, replays and upgrades C-written directories (Facts, below). Any core or Rust edit beyond Task 1 is a plan-defined stop.
2. **The oracle is C booting the same bytes,** not answers captured before the stop.
   - For the crash, the open transaction's index entries may or may not reach disk before the crash, so pre-crash answers and C's post-recovery answers can legitimately differ. C's own replay is the ground truth.
   - For clean, the capture also checks that C's answers before the stop equal its answers after a reboot.
   - For v081, the oracle is the 0.8.7 image after the same `ALTER EXTENSION vector UPDATE`, which is what an in-place upgrade does. The 0.8.1 library is not the oracle.
3. **The float question is answered by design, not by emulation.**
   - Every fixture value makes index scans compute exact distances:
     - small integers for L2, inner product and L1;
     - ±1 entries in 16 dimensions for cosine (every norm is 4, so normalized entries are ±0.25);
     - bits for Hamming and Jaccard.
   - The order of C's vectorized, fused sums then cannot change a result, so M2's no-emulation ruling stands.
   - IVFFlat center distances are still rounded, because centers are k-means means. Its data are ten irregular, well-separated clusters, so no probe is a near-tie.
   - Measured: the local clang build matched the gcc image on every query, and Task 6 repeats the gate on 3 fresh captures.
4. **Replayed pages are compared, beyond the spec's query rule.** After the crash, every relation the fixture owns must equal C's replay after `pagemask`: tables, TOAST, btree, HNSW and IVFFlat, main and init forks. The spike found them identical, even before masking, except heap hint bits.
5. **The follow-up's comparison rule.**
   - Compared exactly: DML tags, counts, exact (no-index) answers, and index-scan row counts before and after `REINDEX`.
   - Not compared row by row: index answers after DML, because new HNSW elements draw random levels and `REINDEX` reruns k-means.
6. **Captures are cached** under `$PGV_WORK/ondisk/capture`.
   - The cache key is a hash over the SQL, `docker-lib.sh`, `run-ondisk.sh` and the image pins. `capture --force` rebuilds it.
   - The forward phase needs no Docker once captured. The reverse phase does.
7. **Reverse rows** are `clean_boot`, `clean_queries`, `clean_amcheck`, `clean_pgrust_rows`, `clean_followup`, `crash_boot`, `crash_pages` and `crash_queries`.
   - They go to `reverse.tsv` and `reverse.md`, and `run-all.sh` prints them under "Reported, not gated".
   - Known finding: on its first connection pgrust writes three pgrust-only `pg_proc` rows into a C-created database. C sees them.
8. **One parked follow-up is folded in:** the repair prologue's `text`/`oid` types (Task 1).
   - Two others stay deferred, because they are core edits outside M5's theme: lmgr_proc/ipc/mcxt_stats decoding panics with `pg_error_from_panic`, and guc's `%g` check-hook fallback.
9. **Not in M5:** x86 data directories, `wal_compression = lz4|zstd` full-page images (pgrust's reader refuses them), parallel builds (M6), and the approximate differential (M7). Each is recorded in the M5 report.

**Tech Stack:** bash 3.2 harness scripts, Docker (`pgvector/pgvector:0.8.7-pg18` and `0.8.1-pg18`, linux/arm64), the PostgreSQL 18.6 + pgvector 0.8.7 C reference (`pgvec`), `pg_controldata`/`pg_waldump`/`pg_amcheck`, `crates/bin/pagemask`; Rust for Task 1 only.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`. Binding sections:
- §2.1, the on-disk direction policy;
- §8.1, the Docker reference;
- §8.2, the "On-disk, forward" and "On-disk, reverse" rows;
- §8.3, the harness;
- §9, M5.

Inputs:
- `docs/HANDOFF_2026-10-14.md` §1;
- `docs/superpowers/reports/pgvector-m4-report.md`, its "For M5" items;
- `docs/superpowers/reports/pgvector-m4-followups-report.md`, "Still open";
- `~/.cache/pgrust/m4f-sdd-ledger.md`, the parked items.

**Facts verified while writing this plan** (2026-10-14, macOS arm64, Docker 29.1.3 linux/aarch64, `main` = `65a5820abc`):
- **The images.**
  - `pgvector/pgvector:0.8.7-pg18@sha256:2358fcba…` (already pinned) is PostgreSQL 18.6 from Debian PGDG, built with gcc 12.2 and with `--with-lz4 --with-zstd`.
  - `pgvector/pgvector:0.8.1-pg18@sha256:508c5290cda481d4f5f846446a26e9c1b804766828a394a5861de1b348a18b4c` is PostgreSQL 18.2 with pgvector 0.8.1, built 2026-02-13.
  - Both keep `PGDATA=/var/lib/postgresql/18/docker`, ship `gosu`, and run postgres as uid 999 with `STOPSIGNAL SIGINT`. Data checksums are on, as PG18's initdb defaults.
- **Spike on a small fixture.** pgrust booted a Docker clean directory, returned the same HNSW ids as C, and ran insert, delete, `VACUUM` and `REINDEX`.
  - **Crash:** pgrust replayed 25,751 Generic records after the redo point (105 of them with full-page images). The answers matched C's, and every user relation's pages matched C's replay after pagemask (raw too, except the heap's hint bits).
  - **0.8.1:** after `ALTER EXTENSION vector UPDATE`, pgrust gave the same ids and the same 237 extension members as C 0.8.7 doing the same.
  - **Reverse:** C booted pgrust-written directories. pgrust's `pg_ctl stop -m immediate` leaves `in production`, C replayed pgrust's WAL, and the rows of the transaction left open were invisible.
- **pgrust's first connection** to a C-created database logs `backfilled 3 builtin function row(s)`. The rows are `pgrust_pin_database`, `pgrust_seal_template` and `pgrust_unpin_database`.
- **Recovery code survey.** All 22 built-in rmgrs have redo functions. `generic_redo` applies C's two-region delta format. Checksums are verified on read. LZ4 and zstd full-page images are refused (`xlogreader` `restore_image_core`).
- **A full draft of Tasks 2–5's harness** (the code in this plan) ran against the current binary:
  - **Capture:** 3 min 52 s. Facts:
    - 545,540 Generic records after the crash's redo point, 2,631 of them with full-page images;
    - 3,257 Heap records and 2,552 Btree records;
    - 75,607 Generic records from the transaction left open;
    - 42 fixture relations, 25 vector indexes scanned, and `wide` TOASTed;
    - v081 went from 0.8.1 to 0.8.7.
  - **`ref`:** 7/7 forward and every reverse row identical, in 3 min 31 s.
  - **`pgrust`:** 7/7 forward, in 5 min 34 s. Every reverse row was identical, `pg_amcheck` was clean, and `clean_pgrust_rows` listed the three rows.
  - **Negative control:** deleting the rows nearest the first query failed all 5 selected rows.
  - **Self-tests:** every self-test in this plan passed against the draft:
    - `test_server_stop_immediate`, `test_docker_lib` and `test_ondisk_sql`: 2, 12 and 7 checks;
    - `test_ondisk_capture`, `test_ondisk_ref`, `test_ondisk_detects_difference` and `test_ondisk_reverse_ref`: 10, 2, 1 and 3 checks.
  - **A second fresh capture** (the self-test's) also passed `ref` 7/7, with 546,355 Generic records, 2,623 of them full-page images, and 75,443 from the open transaction.
- **Timings on the local C reference:** the fixture takes 13 s, the crash writes 23 s and the follow-up 35 s. The fixture takes 21 s on the 0.8.1 image.
- **Gotchas found while drafting:**
  - `bigint[]` has no cast to `vector`, so the generated elements are cast to `int`.
  - bash 3.2 cannot parse `case` inside `$( )`.
  - `docker exec … pg_ctl stop -m immediate` dies with its PID 1, so `dk_stop_immediate` judges by the copied directory's state.
  - A host initdb's `pg_hba.conf` rejects Docker's gateway, so `dk_boot_dir` appends a trust line to its own copy.
  - With `i` a multiple of 10, `(i*j) % 5` is constant, so `wide`'s values are md5-derived.

## Global Constraints

- **Branch:** work on `vector/m5`, cut from `main` (`65a5820abc`). Merge locally with a fast-forward when done; push only when the user asks. Work in the main checkout.
- **Commit trailer:** every commit ends with a `Co-Authored-By:` trailer naming the model that wrote it: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. `git status --porcelain --ignored` on both must stay empty.
- **Rust:** only Task 1's test-only `crates/contrib/pgvector/src/repair_gen.rs` and the file it generates. Any other Rust or core edit is a plan-defined stop.
- **Never widen `docs/fuzzing/rulings.toml`.** Never edit upstream expected outputs. No `-- pgrust:` annotations.
- **Port conventions** (CLAUDE.md) apply to Task 1: keep C names, cite C `file.c:line`, mark intentional differences `DIVERGENCE`.
- **Harness scripts:**
  - Use `#!/usr/bin/env bash` and `set -euo pipefail` (a sourced library has `# shellcheck shell=bash` instead).
  - Stay bash 3.2 compatible: no associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, or `case` inside `$( )`.
  - Use `${arr[@]+"${arr[@]}"}` for a possibly empty array.
  - Check each changed script with `/bin/bash -n` (macOS's 3.2).
- **Docker:**
  - Use only the two digest-pinned images.
  - Touch only the container `pgrust-pgvector-ondisk` (and, in its existing self-test, `docker-ref.sh`'s `pgrust-pgvector-ref`). Remove containers with `docker rm -f -v`.
  - Never `docker system prune`, never remove images, and never pull other tags.
  - Port 55493 is shared with `docker-ref.sh`; never run both at once.
- **Ports:** pgrust 55491, C reference 55492, Docker 55493.
- **Verdicts:** read each tier's from `~/.cache/pgrust/pgvector-work/<tier>/<mode>/summary.tsv` before a later run overwrites it. Leave no servers or containers running.
- **Server build:** Task 2 runs `scripts/pgvector/build-pgrust.sh` once, so `target/fast-profile/{postgres,pagemask}` match the branch. Nothing after Task 1 changes Rust.
- **Unit tests and lints:** Task 1 only (it is the only task touching Rust); each command is in its steps. Never run a test command with an empty filter.
- **Disk:** a capture is about 450 MB, and the self-tests keep a second one under `pgvector-work/selftest`. `PGV_KEEP_DATA=1` copies are deleted after inspection. Disk was 521 GiB free at planning time.

## Review Focus

The five input classes most likely to bite a pgvector user moving a data directory to pgrust that no earlier tier exercises. Each one's test is in the owning task.

1. **A crash with a transaction still open.** Its rows reach the heap and every index: the draft's WAL held 75,607 Generic records for that xid. They must stay invisible after replay, while the graph still holds them.
   - Test: `ondisk-inflight.sql` and the `inflight` count in `ondisk-queries.sql` (Task 3), compared in `crash_queries`; the `crash_inflight_generic` fact (Task 4's `test_ondisk_capture`).
2. **Unlogged tables after a crash.** They are reset from the init fork, so they are empty, and their HNSW and IVFFlat indexes must accept inserts.
   - Test: table `ut` (Task 3), through `crash_queries`, `crash_followup` and `crash_pages`'s `_init` forks (Task 4).
3. **Embedding-sized vectors stored out of line.** A 1,536-dimension `vector` column in a C-written TOAST table must be read through both the index and the heap.
   - Test: table `wide` (Task 3); the `wide_toasted` fact (Task 4).
4. **A database still on the 0.8.1 catalog with the 0.8.7 library,** before and after `ALTER EXTENSION vector UPDATE`. Answers must equal C's both times. The updated catalog (members, comments, function definitions, amop, amproc, opclasses) must equal C's.
   - Test: `ondisk-update.sql` (Task 3), through the `v081_update` row (Task 4).
5. **A query of the wrong dimension against a C-written metapage** must give C's error text (`expected 8 dimensions, not 3`) from both access methods.
   - Test: the last two statements of `ondisk-queries.sql` (Task 3), compared in every queries row (Task 4).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/contrib/pgvector/src/repair_gen.rs`, `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` | qualified prologue types; their test | 1 |
| `scripts/pgvector/common.sh` | the 0.8.1 image pin; `server_stop`'s signal | 2 |
| `scripts/pgvector/docker-lib.sh` | Docker helpers: start, boot a host directory, stop clean or immediate, copy out | 2 |
| `scripts/pgvector/sql/ondisk-rows.sql` | the row generator (exact-arithmetic values) | 3 |
| `scripts/pgvector/sql/ondisk-fixture.sql` | tables, 25 vector indexes, deletes, `VACUUM ANALYZE` | 3 |
| `scripts/pgvector/sql/ondisk-crash.sql`, `ondisk-inflight.sql` | the crash's writes and its open transaction | 3 |
| `scripts/pgvector/sql/ondisk-queries.sql` | index scans compared with C | 3 |
| `scripts/pgvector/sql/ondisk-followup.sql` | DML and maintenance after the boot | 3 |
| `scripts/pgvector/sql/ondisk-update.sql` | the 0.8.1 update transcript | 3 |
| `scripts/pgvector/sql/ondisk-relfiles.sql` | the relations whose replayed pages are compared | 3 |
| `scripts/pgvector/run-ondisk.sh` | capture, the forward gate (Task 4), the reverse report (Task 5) | 4, 5 |
| `scripts/pgvector/tests/harness_test.sh` | self-tests | 2–5 |
| `scripts/pgvector/run-all.sh` | the `ondisk` tier and the reverse section | 6 |
| spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m5-report.md` | docs and the report | 6 |

## Execution notes

- **Task order:** 1 to 6. Task 1 is independent. Tasks 2 → 3 → 4 → 5 → 6 build on each other.
- **Suggested implementers:**
  - Sonnet: Tasks 1, 2 and 3 (the code is spelled out).
  - Opus: Tasks 4, 5 and 6 (integration, the gate runs, the measurement and the report).
  - Opus does every review and the final review.
- **Time:**
  - A capture takes about 4 minutes, and editing `run-ondisk.sh` or any `sql/ondisk-*.sql` file triggers a new one.
  - `run-ondisk.sh ref` takes about 3.5 minutes and `run-ondisk.sh pgrust` about 5.5.
  - The new self-tests take about 15 minutes together, and Task 6's stability loop about 30.
- **Plan-defined stops**, in addition to subagent-driven-development's four:
  - Any Rust or core edit outside Task 1.
  - **A forward row that fails for pgrust while `ref` passes on the same capture.** This is a real pgrust divergence.
    - Keep the capture (`cp -R ~/.cache/pgrust/pgvector-work/ondisk/capture ~/.cache/pgrust/m5-capture-fail-<n>`) and the subject's diff.
    - Report the first differing statement to the user (AskUserQuestion). Offer a small core fix (spec §10) or a recorded exclusion.
  - **A `ref` failure traced to float rounding** (clang against gcc). The fixture's exactness claim is wrong there. Report it; do not reshape data to make it pass without the user's ruling.
  - **Fixture changes after Task 3** (data, queries, settings) need a recorded reason in the ledger and a re-review of Task 3.
  - Never widen `rulings.toml`.
- **Expected totals after Task 6:**
  - `run-all.sh ref`: 137/137 (regress 14, TAP 48, bytecmp 28, iterscan 19, diff 21, ondisk 7). The reverse section shows every row `ok`, `identical`, `clean` or `none`.
  - `run-all.sh pgrust`: 129/137. The 8 TAP parallel-build failures (M6) remain. The reverse section is as in the draft, unless a run says otherwise.

---

### Task 1: The repair prologue qualifies its types

The parked item from the M4 follow-ups' final review (ledger, "Final: parked"). The repair's `DECLARE` block resolves `text` and `oid` through the caller's `search_path`. Qualify them.

**Files:**
- Modify: `crates/contrib/pgvector/src/repair_gen.rs` (`HEADER`'s `DECLARE` block and its comment; one new test)
- Regenerate: `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql`

**Interfaces:** none.

- [ ] **Step 1: Branch and commit the plan**

```bash
git switch -c vector/m5 main
git add docs/superpowers/plans/2026-10-14-pgvector-m5-ondisk.md
git commit -q -m "docs(pgvector): M5 plan (on-disk tiers)" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```
(The controller may already have done this. Skip it if `git log -1 --format=%s` shows the plan commit.)

- [ ] **Step 2: Write the failing test**

Append to `crates/contrib/pgvector/src/repair_gen.rs`, after `repair_covers_every_upstream_object`:

```rust
// The DECLARE block runs under the caller's search_path, so every type it
// names is schema-qualified; `int` is a keyword, not a catalog lookup.
#[cfg(test)]
#[test]
fn repair_prologue_qualifies_its_types() {
    let decl = HEADER.split("\nDECLARE\n").nth(1).unwrap().split("\nBEGIN\n").next().unwrap();
    let mut n = 0;
    for line in decl.lines() {
        let ty = line.split_whitespace().nth(1).unwrap_or("").trim_end_matches(';');
        assert!(ty == "int" || ty.starts_with("pg_catalog."), "unqualified type in `{}`", line.trim());
        n += 1;
    }
    assert_eq!(n, 6, "the DECLARE block declares six variables");
}
```

- [ ] **Step 3: Run it to verify it fails**

Run: `cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_prologue_qualifies_its_types`
Expected: FAIL with ``unqualified type in `v_ver text;` ``.

- [ ] **Step 4: Qualify the types**

In `HEADER`, replace the `DECLARE` block's first five lines and the comment that opens `BEGIN`:

```text
DECLARE
  v_ver pg_catalog.text;
  v_nsp pg_catalog.text;
  v_nsp_oid pg_catalog.oid;
  v_old_path pg_catalog.text := pg_catalog.current_setting('search_path');
  v_old_cmm pg_catalog.text := pg_catalog.current_setting('client_min_messages');
  n int := 0;
BEGIN
  -- The caller's search_path is in force in the DECLARE block, until the
  -- search_path set_config below, and again once the end restores it, so
  -- every type, function, relation and operator used in those stretches is
  -- schema-qualified.
```
(`n int` stays: `int` is grammar, not a lookup.)

- [ ] **Step 5: Regenerate the script and run the repair tests**

```bash
PGVECTOR_REGEN_REPAIR=1 cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_script_is_generated_from_upstream
cargo test --profile fast-profile --locked -p main_main -p pgvector --lib -- repair_ lookup_cover shipped_scripts
git diff --stat
```
Expected:
- all tests pass;
- `git diff --stat` lists `repair_gen.rs` and `pgrust-trimmed-0.8.5.sql` only;
- the `.sql` diff touches the five `DECLARE` lines and the comment only.

- [ ] **Step 6: The repair check still passes 6/6**

Run: `scripts/pgvector/run-repair-check.sh`. It uses `target/fast-profile/postgres`. Task 1 changes no server code, so `main`'s build serves; if it is missing, run `scripts/pgvector/build-pgrust.sh` first.
Expected: `6/6 passed`. The rollback injection still finds its footer line, which is unchanged.

- [ ] **Step 7: Lints**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
```
Expected: both exit 0. The determinism lint counts no new site, because the test is `#[cfg(test)]`.

- [ ] **Step 8: Commit**

```bash
git add crates/contrib/pgvector/src/repair_gen.rs crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql
git commit -q -m "pgvector: the repair prologue qualifies its types" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Docker helpers, the 0.8.1 pin, and immediate stops

**Files:**
- Create: `scripts/pgvector/docker-lib.sh`
- Modify: `scripts/pgvector/common.sh` (`PGV_DOCKER_IMAGE_081`; `server_stop`'s optional signal)
- Modify: `scripts/pgvector/tests/harness_test.sh` (source `docker-lib.sh`; `cluster_state`; `test_server_stop_immediate`, `test_docker_lib`)

**Interfaces:**
- Produces, used by Tasks 4 and 5:
  - `PGV_DOCKER_IMAGE_081` (common.sh).
  - `server_stop <mode> [INT|QUIT]` (common.sh). `QUIT` is an immediate shutdown.
  - From `docker-lib.sh`:
    - `DK_NAME`, `DK_DATA`;
    - `dk_require`, `dk_rm` and `dk_psql [psql args]`;
    - `dk_start_new <image> [postgres -c opts]` and `dk_boot_dir <image> <host datadir> [postgres -c opts]`;
    - `dk_start`, `dk_stop_fast` and `dk_stop_immediate`;
    - `dk_copy_out <dest>` and `dk_exec <cmd...>`.
  - Every `dk_*` function except `dk_require` returns non-zero on failure instead of exiting.
- Produces in `harness_test.sh`: `cluster_state <datadir>`.

- [ ] **Step 1: Build the server and confirm both images**

```bash
scripts/pgvector/build-pgrust.sh
docker pull -q "pgvector/pgvector:0.8.1-pg18@sha256:508c5290cda481d4f5f846446a26e9c1b804766828a394a5861de1b348a18b4c"
docker image inspect -f '{{.Architecture}}' "pgvector/pgvector:0.8.7-pg18@sha256:2358fcba361ed2233a5ed81b5fe4ca779ccb304120ce531a3bf51c0ed7e2bc11"
```
Expected: the build succeeds and `docker image inspect` prints `arm64`. The 0.8.1 pull is a no-op if the image is cached, or about 400 MB if not.

- [ ] **Step 2: Write the failing self-tests**

In `scripts/pgvector/tests/harness_test.sh`:

(a) Under `. "$here/../common.sh"`, add the Docker helpers, and fix the isolation comment above it to say that the Docker containers and their port are shared:

```bash
# Self-tests run in their own work dir so they never replace real results or
# stop real harness servers (only the Docker containers and port 55493 are
# shared).
PGV_REAL_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
export PGV_WORK="$PGV_REAL_WORK/selftest"
. "$here/../common.sh"
. "$here/../docker-lib.sh"
```

(b) Above the `# --- runner ---` marker, add:

```bash
# Database cluster state of data directory $1, from pg_controldata.
cluster_state() {
  "$PG_TOOLS/bin/pg_controldata" "$1" 2>/dev/null | sed -n 's/^Database cluster state: *//p'
}

# server_stop's QUIT: an immediate stop leaves a cluster that needs recovery.
test_server_stop_immediate() {
  local st
  if ! "$here/../server.sh" fresh ref >/dev/null 2>&1; then
    fail "immediate stop: the ref server starts"
    return
  fi
  server_stop ref QUIT
  st="$(cluster_state "$PGV_WORK/data/ref")"
  if [ "$st" = "in production" ]; then pass "immediate stop leaves 'in production'"; else fail "immediate stop left '$st'"; fi
  if [ ! -f "$PGV_WORK/ref.pid" ]; then pass "immediate stop removes the pidfile"; else fail "immediate stop kept the pidfile"; fi
}

# docker-lib.sh: both images start; a clean and an immediate stop copy out a
# shut-down and an in-production cluster; the 0.8.7 image boots a host copy
# without writing it, keeps its own copy across a restart, and replays a
# crashed copy; nothing is left behind.
test_docker_lib() {
  local w="$PGV_WORK/docker-lib" before after v ctl
  before="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  rm -rf "$w"
  mkdir -p "$w"
  if ! dk_start_new "$PGV_DOCKER_IMAGE_081" -c autovacuum=off >"$w.log" 2>&1; then
    fail "docker-lib: the 0.8.1 image starts (see $w.log)"
    dk_rm
    return
  fi
  pass "docker-lib: the 0.8.1 image starts"
  v="$(dk_psql -Atq -c "CREATE EXTENSION vector" -c "CREATE TABLE t AS SELECT '[1,2]'::vector AS v" \
    -c "SELECT extversion FROM pg_extension WHERE extname = 'vector'" 2>&1 || true)"
  if [ "$v" = 0.8.1 ]; then pass "docker-lib: vector 0.8.1"; else fail "docker-lib: extversion '$v'"; fi
  if dk_stop_fast && dk_copy_out "$w/clean" >>"$w.log" 2>&1; then
    pass "docker-lib: clean stop and copy out"
  else
    fail "docker-lib: clean stop and copy out (see $w.log)"
  fi
  v="$(cluster_state "$w/clean")"
  if [ "$v" = "shut down" ]; then pass "docker-lib: the copy is shut down"; else fail "docker-lib: the copy is '$v'"; fi
  ctl="$("$PG_TOOLS/bin/pg_controldata" "$w/clean")"
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/clean" >>"$w.log" 2>&1; then
    fail "docker-lib: the 0.8.7 image boots the copy (see $w.log)"
    dk_rm
    return
  fi
  pass "docker-lib: the 0.8.7 image boots the copy"
  dk_psql -qc "INSERT INTO t VALUES ('[3,4]')" >/dev/null
  if dk_stop_fast && dk_start >>"$w.log" 2>&1 && [ "$(dk_psql -Atc 'SELECT count(*) FROM t' 2>&1)" = 2 ]; then
    pass "docker-lib: a restart keeps the container's copy"
  else
    fail "docker-lib: a restart keeps the container's copy (see $w.log)"
  fi
  dk_psql -qc "INSERT INTO t VALUES ('[5,6]')" >/dev/null
  if dk_stop_immediate && dk_copy_out "$w/crash" >>"$w.log" 2>&1; then
    pass "docker-lib: immediate stop and copy out"
  else
    fail "docker-lib: immediate stop and copy out (see $w.log)"
  fi
  v="$(cluster_state "$w/crash")"
  if [ "$v" = "in production" ]; then pass "docker-lib: the crashed copy needs recovery"; else fail "docker-lib: the crashed copy is '$v'"; fi
  if [ "$("$PG_TOOLS/bin/pg_controldata" "$w/clean")" = "$ctl" ]; then
    pass "docker-lib: the host directory is untouched"
  else
    fail "docker-lib: the host directory changed"
  fi
  if dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/crash" >>"$w.log" 2>&1 && [ "$(dk_psql -Atc 'SELECT count(*) FROM t' 2>&1)" = 3 ]; then
    pass "docker-lib: the crashed copy replays"
  else
    fail "docker-lib: the crashed copy replays (see $w.log)"
  fi
  dk_rm
  if [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then pass "docker-lib: container removed"; else fail "docker-lib: $DK_NAME left behind"; fi
  after="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if [ "$after" -le "$before" ]; then pass "docker-lib: no leaked volume"; else fail "docker-lib: leaked $((after - before)) anonymous volume(s)"; fi
  rm -rf "$w"
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/pgvector/tests/harness_test.sh test_server_stop_immediate test_docker_lib`
Expected: the script aborts, because `docker-lib.sh: No such file or directory`.

- [ ] **Step 4: Edit `common.sh`**

(a) After the `PGV_DOCKER_IMAGE=` line, add:

```bash
# The on-disk tier's 0.8.1 fixture (PostgreSQL 18.2): ALTER EXTENSION vector
# UPDATE from the first version that supports PG18 (spec §3).
PGV_DOCKER_IMAGE_081="pgvector/pgvector:0.8.1-pg18@sha256:508c5290cda481d4f5f846446a26e9c1b804766828a394a5861de1b348a18b4c"
```

(b) Give `server_stop` an optional signal. Replace its first line and its `kill -INT`:

```bash
# $2: the signal, INT (fast shutdown, the default) or QUIT (immediate
# shutdown: no checkpoint, so the next start replays WAL).
server_stop() {
  local mode="$1" sig="${2:-INT}" pidfile="$PGV_WORK/$1.pid" pid i
```
and
```bash
    kill -"$sig" "$pid"
```
Everything else in `server_stop` stays: the postgres-binary check, the wait loop and the pidfile removal.

- [ ] **Step 5: Create `scripts/pgvector/docker-lib.sh`**

```bash
# shellcheck shell=bash
# Docker helpers for the on-disk tier (run-ondisk.sh). Sourced after
# common.sh; bash 3.2 compatible. Every dk_* function except dk_require
# returns non-zero on failure (with a message on stderr) instead of exiting,
# so callers choose: the capture dies, the reverse report records it.
#
# One container at a time, named $DK_NAME and published on
# 127.0.0.1:$PGV_PORT_DOCKER. docker-ref.sh uses the same port, so never run
# the two at once. The pgvector/pgvector images (postgres:18 base) keep
# PGDATA at /var/lib/postgresql/18/docker.
#   dk_start_new  runs the image's own entrypoint: initdb (C collation, UTF8,
#                 trust), then postgres.
#   dk_boot_dir   boots a host data directory instead. The directory is
#                 mounted read-only at /snap and copied to /pgdata on the
#                 container's first start only, so the host copy is never
#                 written and a restarted container keeps its own copy. The
#                 copy's pg_hba.conf gains "host all all all trust": the
#                 host's connections arrive from Docker's gateway, which a
#                 host initdb's 127.0.0.1-only rules reject.

DK_NAME=pgrust-pgvector-ondisk
# The container's data directory (set by dk_start_new and dk_boot_dir).
DK_DATA=

dk_require() {
  docker info >/dev/null 2>&1 || die "Docker daemon not reachable; start Docker Desktop"
}

dk_rm() {
  docker rm -f -v "$DK_NAME" >/dev/null 2>&1 || true
}

dk_psql() {
  "$PG_TOOLS/bin/psql" -X -h 127.0.0.1 -p "$PGV_PORT_DOCKER" -U postgres -d postgres "$@"
}

dk_wait_ready() {
  local i
  for i in $(seq 1 240); do
    if "$PG_TOOLS/bin/pg_isready" -q -h 127.0.0.1 -p "$PGV_PORT_DOCKER" &&
      dk_psql -Atc 'SELECT 1' >/dev/null 2>&1; then
      return 0
    fi
    [ "$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)" = true ] || break
    sleep 0.5
  done
  echo "container $DK_NAME did not become ready; its last log lines:" >&2
  docker logs "$DK_NAME" 2>&1 | tail -20 >&2
  return 1
}

# $1 image; the rest are postgres options (-c name=value ...).
dk_start_new() {
  local image="$1"
  shift
  dk_rm
  if ! docker run -d --name "$DK_NAME" -p "127.0.0.1:$PGV_PORT_DOCKER:5432" \
    -e POSTGRES_HOST_AUTH_METHOD=trust -e POSTGRES_INITDB_ARGS="--no-locale --encoding=UTF8" \
    "$image" postgres "$@" >/dev/null; then
    echo "docker run $image failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    return 1
  fi
  DK_DATA=/var/lib/postgresql/18/docker
  dk_wait_ready
}

# $1 image, $2 host data directory; the rest are postgres options.
dk_boot_dir() {
  local image="$1" dir="$2"
  shift 2
  if [ ! -f "$dir/PG_VERSION" ]; then
    echo "not a data directory: $dir" >&2
    return 1
  fi
  dk_rm
  # shellcheck disable=SC2016 # expanded by the container's bash
  if ! docker run -d --name "$DK_NAME" -p "127.0.0.1:$PGV_PORT_DOCKER:5432" -v "$dir:/snap:ro" \
    --entrypoint bash "$image" -c '
      if [ ! -d /pgdata ]; then
        cp -a /snap /pgdata && echo "host all all all trust" >>/pgdata/pg_hba.conf &&
          chown -R postgres:postgres /pgdata && chmod 700 /pgdata || exit 1
      fi
      exec gosu postgres postgres -D /pgdata -c listen_addresses="*" "$@"' bash "$@" >/dev/null; then
    echo "docker run $image on $dir failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    return 1
  fi
  DK_DATA=/pgdata
  dk_wait_ready
}

# Start the stopped container again; it keeps its data directory.
dk_start() {
  docker start "$DK_NAME" >/dev/null && dk_wait_ready
}

# Fast shutdown (the images' STOPSIGNAL is SIGINT).
dk_stop_fast() {
  docker stop -t 120 "$DK_NAME" >/dev/null
}

# pg_ctl stop -m immediate: no shutdown checkpoint, so the next start
# replays WAL from the last checkpoint. The postmaster is the container's
# PID 1, so the container ends the moment it exits and takes this exec with
# it: pg_ctl's own exit status means nothing, and the caller checks the
# copied directory's state instead.
dk_stop_immediate() {
  docker exec -u postgres "$DK_NAME" pg_ctl stop -m immediate -D "$DK_DATA" >/dev/null 2>&1 || true
  docker wait "$DK_NAME" >/dev/null
}

# Copy the stopped container's data directory to $1 (replacing it).
dk_copy_out() {
  rm -rf "$1"
  docker cp "$DK_NAME:$DK_DATA" "$1" >/dev/null || return 1
  if [ -e "$1/postmaster.pid" ]; then
    echo "$1 holds postmaster.pid: the server did not stop" >&2
    return 1
  fi
}

dk_exec() {
  docker exec -u postgres "$DK_NAME" "$@"
}
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
/bin/bash -n scripts/pgvector/common.sh && /bin/bash -n scripts/pgvector/docker-lib.sh && /bin/bash -n scripts/pgvector/tests/harness_test.sh
scripts/pgvector/tests/harness_test.sh test_server_stop_immediate test_docker_lib test_docker test_servers
```
Expected: `all passed`. That is 2 checks, then 12, then `test_docker`'s and `test_servers`' existing checks. `docker ps -a` shows no `pgrust-pgvector-*` container.

- [ ] **Step 7: Commit**

```bash
git add scripts/pgvector/common.sh scripts/pgvector/docker-lib.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "pgvector harness: Docker helpers, the 0.8.1 pin, immediate stops" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The on-disk fixture SQL

**Files:** create the eight files below; modify `scripts/pgvector/tests/harness_test.sh` (`test_ondisk_sql`).

**Interfaces:**
- Produces, used by Tasks 4 and 5:
  - **Tables:** `vt`, `ct`, `ut` (unlogged) and `wide`. The 25 vector indexes are named as in `ondisk-fixture.sql`.
  - **Row ranges, by file:**

    | File | Rows |
    |---|---|
    | `ondisk-fixture.sql` | 1–3000 |
    | `ondisk-crash.sql` | 3001–3800 |
    | `ondisk-inflight.sql` | 4001–4200, never committed |
    | `ondisk-followup.sql` | 10001–10300 |

  - **`ondisk-relfiles.sql`** prints `name|rmgr|path` (rmgr is `heap`, `btree` or `generic`).
  - **`ondisk-update.sql`** includes `ondisk-queries.sql` twice and `repair-verify.sql` once.
- Every file is run from `scripts/pgvector/sql`, because the `\ir` includes resolve there.

- [ ] **Step 1: Write the failing self-test**

Above the `# --- runner ---` marker in `harness_test.sh`, add:

```bash
# The on-disk SQL on a fresh local C reference, without Docker: the fixture
# loads; the queries scan all 25 vector indexes and fail only where they
# mean to (the two wrong-dimension queries); the relation list has 41 rows;
# the crash writes load; the follow-up runs without errors and its index
# row counts are full; the update script reaches repair-verify.sql's ext row.
test_ondisk_sql() {
  local w="$PGV_WORK/ondisk-sql" sql="$here/../sql" port n
  port="$(port_for ref)"
  rm -rf "$w"
  mkdir -p "$w"
  if ! "$here/../server.sh" fresh ref >"$w/server.log" 2>&1; then
    fail "ondisk sql: the ref server starts (see $w/server.log)"
    return
  fi
  if (cd "$sql" && pgv_psql "$port" -f ondisk-fixture.sql) >"$w/fixture.log" 2>&1; then
    pass "ondisk sql: the fixture loads"
  else
    fail "ondisk sql: the fixture loads (see $w/fixture.log)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-queries.sql) >"$w/queries.out" 2>&1 || true
  n="$(sed -n -E 's/^ *->  Index Scan using ([a-z0-9_]+) on (vt|ct|ut|wide)$/\1/p' "$w/queries.out" | sort -u | wc -l | tr -d ' ')"
  if [ "$n" = 25 ]; then pass "ondisk sql: 25 vector indexes scanned"; else fail "ondisk sql: $n vector indexes scanned, want 25"; fi
  n="$(grep -c 'ERROR:' "$w/queries.out" || true)"
  if [ "$n" = 2 ] && [ "$(grep -c 'ERROR:  expected 8 dimensions, not 3' "$w/queries.out" || true)" = 2 ]; then
    pass "ondisk sql: only the wrong-dimension queries fail"
  else
    fail "ondisk sql: $n errors in $w/queries.out, want the 2 wrong-dimension ones"
  fi
  n="$( (cd "$sql" && pgv_psql "$port" -f ondisk-relfiles.sql) | grep -c '|' || true)"
  if [ "$n" = 41 ]; then pass "ondisk sql: 41 fixture relations"; else fail "ondisk sql: $n fixture relations, want 41"; fi
  if pgv_psql "$port" -c CHECKPOINT >/dev/null && (cd "$sql" && pgv_psql "$port" -f ondisk-crash.sql) >"$w/crash.log" 2>&1; then
    pass "ondisk sql: the crash writes load"
  else
    fail "ondisk sql: the crash writes load (see $w/crash.log)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-followup.sql) >"$w/followup.out" 2>&1 || true
  if ! grep -q 'ERROR:' "$w/followup.out" &&
    [ "$(grep -cE '^ +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +5$' "$w/followup.out" || true)" = 2 ]; then
    pass "ondisk sql: the follow-up runs, every index returns full rows"
  else
    fail "ondisk sql: the follow-up (see $w/followup.out)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-update.sql) >"$w/update.out" 2>&1 || true
  if [ "$(grep -c 'ERROR:' "$w/update.out" || true)" = 4 ] && grep -q '^ext|0.8.7|true$' "$w/update.out"; then
    pass "ondisk sql: the update script runs"
  else
    fail "ondisk sql: the update script (see $w/update.out)"
  fi
  "$here/../server.sh" stop ref >/dev/null 2>&1 || true
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_sql`
Expected: `FAIL - ondisk sql: the fixture loads`, and the later checks fail too, because the files do not exist.

- [ ] **Step 3: Create `scripts/pgvector/sql/ondisk-rows.sql`**

```sql
-- Rows :lo..:hi of every on-disk fixture table (run-ondisk.sh). Included by
-- ondisk-fixture.sql, ondisk-crash.sql, ondisk-inflight.sql and
-- ondisk-followup.sql with \set lo / \set hi. Every value makes index scans
-- compute exact distances, whatever the summation order or FMA:
--   v, h, s: small integers (L2, inner product and L1 sums stay below 2^24);
--   vc, hc, sc, gc, ghc: 16 entries of +-1, so every norm is 4 and every
--     normalized entry is +-0.25 (cosine opclasses);
--   b, cb: bits (Hamming and Jaccard counts);
--   g, gh: integer points around ten irregular centers 50 * (0..31) apart
--     per dimension (IVFFlat), offsets -1..2;
--   e: 1536 md5-derived integers in 0..4 (6,152 bytes, stored out of line in
--     TOAST).
INSERT INTO vt
SELECT i,
  ARRAY[i % 7, i % 11, i % 13, i % 17, i % 19, i % 23, i % 29, i % 31]::vector,
  ARRAY[(i * 3) % 17, (i * 5) % 19, (i * 7) % 23, (i * 11) % 29, (i * 13) % 31, (i * 2) % 13, (i * 4) % 11, (i * 6) % 7]::halfvec,
  format('{%s:%s,%s:%s,%s:%s,%s:%s}/32', 1 + i % 8, 1 + i % 5, 9 + (i / 8) % 8, 1 + i % 7,
         17 + (i / 64) % 8, 1 + i % 3, 25 + (i * 7) % 8, 1 + i % 11)::sparsevec,
  ('x' || substr(md5(i::text), 1, 16))::bit(64),
  (SELECT array_agg((1 - 2 * ((i::bigint * 2654435761 >> (j + 16)) & 1))::int ORDER BY j) FROM generate_series(0, 15) j)::vector,
  (SELECT array_agg((1 - 2 * ((i::bigint * 2246822519 >> (j + 16)) & 1))::int ORDER BY j) FROM generate_series(0, 15) j)::halfvec,
  (SELECT '{' || string_agg(format('%s:%s', 4 * j + ((i::bigint * 2654435761 >> (2 * j + 12)) & 3) + 1,
                                   1 - 2 * ((i::bigint * 2246822519 >> (j + 20)) & 1)), ',' ORDER BY j) || '}/64'
     FROM generate_series(0, 15) j)::sparsevec
FROM generate_series(:lo, :hi) i;

INSERT INTO ct
SELECT i,
  (SELECT array_agg((50 * (((i % 10)::bigint * 2654435761 >> (3 * d + 8)) & 31) + ((i::bigint * 2246822519 >> (2 * d + 12)) & 3) - 1)::int ORDER BY d)
     FROM generate_series(0, 7) d)::vector,
  (SELECT array_agg((50 * (((i % 10)::bigint * 2654435761 >> (3 * d + 8)) & 31) + ((i::bigint * 2246822519 >> (2 * d + 12)) & 3) - 1)::int ORDER BY d)
     FROM generate_series(0, 7) d)::halfvec,
  (SELECT array_agg((1 - 2 * ((((i % 10)::bigint * 2654435761 >> (d + 16)) & 1) # (((i::bigint * 2246822519 >> (d + 20)) & 15) = 0)::int))::int ORDER BY d)
     FROM generate_series(0, 15) d)::vector,
  (SELECT array_agg((1 - 2 * ((((i % 10)::bigint * 2654435761 >> (d + 16)) & 1) # (((i::bigint * 2246822519 >> (d + 20)) & 15) = 0)::int))::int ORDER BY d)
     FROM generate_series(0, 15) d)::halfvec,
  ('x' || substr(md5((i % 10)::text), 1, 16))::bit(64)
    # (('x' || substr(md5(i::text), 1, 16))::bit(64) & ('x' || substr(md5((i * 7)::text), 1, 16))::bit(64)
       & ('x' || substr(md5((i * 13)::text), 1, 16))::bit(64))
FROM generate_series(:lo, :hi) i;

INSERT INTO ut
SELECT i, ARRAY[i % 5, i % 7, i % 9, i % 11]::vector
FROM generate_series(:lo, :hi) i WHERE i % 6 = 0;

INSERT INTO wide
SELECT i, (SELECT array_agg(get_byte(decode(md5(i || '/' || j / 16), 'hex'), j % 16) % 5 ORDER BY j) FROM generate_series(1, 1536) j)::vector
FROM generate_series(:lo, :hi) i WHERE i % 10 = 0;
```

- [ ] **Step 4: Create `scripts/pgvector/sql/ondisk-fixture.sql`**

```sql
-- The on-disk tier's fixture (spec §8.2 "On-disk"; run-ondisk.sh): every
-- HNSW and IVFFlat opclass, a binary_quantize expression index, an unlogged
-- table, a TOASTed 1536-dimension table, deletes, VACUUM and ANALYZE. Runs
-- unchanged on C pgvector 0.8.1 and 0.8.7 and on pgrust. Row values:
-- ondisk-rows.sql.
CREATE EXTENSION vector;
CREATE TABLE vt (id int PRIMARY KEY, v vector(8), h halfvec(8), s sparsevec(32), b bit(64),
  vc vector(16), hc halfvec(16), sc sparsevec(64));
CREATE TABLE ct (id int PRIMARY KEY, g vector(8), gh halfvec(8), gc vector(16), ghc halfvec(16), cb bit(64));
CREATE UNLOGGED TABLE ut (id int PRIMARY KEY, u vector(4));
CREATE TABLE wide (id int PRIMARY KEY, e vector(1536));
\set lo 1
\set hi 3000
\ir ondisk-rows.sql
CREATE INDEX vt_v_l2 ON vt USING hnsw (v vector_l2_ops) WITH (m = 8, ef_construction = 32);
CREATE INDEX vt_v_ip ON vt USING hnsw (v vector_ip_ops);
CREATE INDEX vt_v_l1 ON vt USING hnsw (v vector_l1_ops) WITH (m = 24);
CREATE INDEX vt_vc_cos ON vt USING hnsw (vc vector_cosine_ops);
CREATE INDEX vt_h_l2 ON vt USING hnsw (h halfvec_l2_ops);
CREATE INDEX vt_h_ip ON vt USING hnsw (h halfvec_ip_ops);
CREATE INDEX vt_h_l1 ON vt USING hnsw (h halfvec_l1_ops);
CREATE INDEX vt_hc_cos ON vt USING hnsw (hc halfvec_cosine_ops);
CREATE INDEX vt_s_l2 ON vt USING hnsw (s sparsevec_l2_ops);
CREATE INDEX vt_s_ip ON vt USING hnsw (s sparsevec_ip_ops);
CREATE INDEX vt_s_l1 ON vt USING hnsw (s sparsevec_l1_ops);
CREATE INDEX vt_sc_cos ON vt USING hnsw (sc sparsevec_cosine_ops);
CREATE INDEX vt_b_ham ON vt USING hnsw (b bit_hamming_ops);
CREATE INDEX vt_b_jac ON vt USING hnsw (b bit_jaccard_ops);
CREATE INDEX vt_bq ON vt USING hnsw ((binary_quantize(vc)::bit(16)) bit_hamming_ops);
CREATE INDEX ct_g_l2 ON ct USING ivfflat (g vector_l2_ops) WITH (lists = 10);
CREATE INDEX ct_g_ip ON ct USING ivfflat (g vector_ip_ops) WITH (lists = 10);
CREATE INDEX ct_gc_cos ON ct USING ivfflat (gc vector_cosine_ops) WITH (lists = 10);
CREATE INDEX ct_gh_l2 ON ct USING ivfflat (gh halfvec_l2_ops) WITH (lists = 10);
CREATE INDEX ct_gh_ip ON ct USING ivfflat (gh halfvec_ip_ops) WITH (lists = 10);
CREATE INDEX ct_ghc_cos ON ct USING ivfflat (ghc halfvec_cosine_ops) WITH (lists = 10);
CREATE INDEX ct_cb_ham ON ct USING ivfflat (cb bit_hamming_ops) WITH (lists = 10);
CREATE INDEX ut_u_l2 ON ut USING hnsw (u vector_l2_ops);
CREATE INDEX ut_u_ip ON ut USING ivfflat (u vector_ip_ops) WITH (lists = 4);
CREATE INDEX wide_e_l2 ON wide USING hnsw (e vector_l2_ops);
DELETE FROM vt WHERE id % 10 = 0;
DELETE FROM ct WHERE id % 10 = 0;
DELETE FROM ut WHERE id % 60 = 0;
VACUUM ANALYZE vt;
VACUUM ANALYZE ct;
VACUUM ANALYZE ut;
VACUUM ANALYZE wide;
```

- [ ] **Step 5: Create `scripts/pgvector/sql/ondisk-crash.sql` and `ondisk-inflight.sql`**

```sql
-- Writes after the crash fixture's CHECKPOINT (run-ondisk.sh): on-disk
-- inserts into every index, deletes, and VACUUM's index cleanup, all WAL that
-- crash recovery must replay. Every transaction here commits.
\set lo 3001
\set hi 3600
\ir ondisk-rows.sql
DELETE FROM vt WHERE id % 10 = 5;
DELETE FROM ct WHERE id % 10 = 5;
DELETE FROM ut WHERE id % 60 = 30;
VACUUM vt;
VACUUM ct;
VACUUM ut;
\set lo 3601
\set hi 3800
\ir ondisk-rows.sql
```

```sql
-- The transaction still open when the crash fixture stops immediately
-- (run-ondisk.sh): its rows reach the heap and every index, and recovery
-- must leave them invisible.
BEGIN;
\set lo 4001
\set hi 4200
\ir ondisk-rows.sql
SELECT pg_sleep(600);
```

- [ ] **Step 6: Create `scripts/pgvector/sql/ondisk-queries.sql`**

```sql
-- Index scans over the on-disk fixture (run-ondisk.sh). The subject's
-- transcript must equal C's on the same data directory: the same index
-- pages give the same traversal (spec §8.2). Per index: an EXPLAIN proving
-- the scan uses it, then nearest-neighbour queries at the default and at a
-- narrower hnsw.ef_search or ivfflat.probes. Query values follow
-- ondisk-rows.sql's exact-arithmetic rules.
SET enable_seqscan = off;
\set q8 '[3,1,4,1,5,9,2,6]'
\set q8b '[10,0,5,12,3,8,20,7]'
\set q16 '[1,-1,1,1,-1,-1,1,-1,1,1,1,-1,-1,1,-1,1]'
\set s32 '{2:3,10:5,18:1,27:4}/32'
\set s64 '{1:1,6:-1,11:1,16:1,17:-1,22:1,27:-1,32:1,33:1,38:-1,43:1,48:-1,49:1,54:1,59:-1,64:1}/64'
\set b64 '1100100100001111110110101010001000100001011010001100001000110100'
\set u4 '[2,3,4,5]'
SELECT count(*) AS vt FROM vt;
SELECT count(*) AS ct FROM ct;
SELECT count(*) AS ut FROM ut;
SELECT count(*) AS wide FROM wide;
SELECT count(*) AS inflight FROM vt WHERE id BETWEEN 4001 AND 4200;
SELECT pg_relation_size(reltoastrelid) > 0 AS wide_toasted FROM pg_class WHERE relname = 'wide';
-- HNSW: vector
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <-> :'q8' LIMIT 10;
SELECT id, v <-> :'q8' AS d FROM vt ORDER BY v <-> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <#> :'q8' LIMIT 10;
SELECT id, v <#> :'q8' AS d FROM vt ORDER BY v <#> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <+> :'q8' LIMIT 10;
SELECT id, v <+> :'q8' AS d FROM vt ORDER BY v <+> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, v <-> :'q8b' AS d FROM vt ORDER BY v <-> :'q8b' LIMIT 10;
SELECT id, v <#> :'q8b' AS d FROM vt ORDER BY v <#> :'q8b' LIMIT 10;
SELECT id, v <+> :'q8b' AS d FROM vt ORDER BY v <+> :'q8b' LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: halfvec
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <-> :'q8' LIMIT 10;
SELECT id, h <-> :'q8' AS d FROM vt ORDER BY h <-> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <#> :'q8' LIMIT 10;
SELECT id, h <#> :'q8' AS d FROM vt ORDER BY h <#> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <+> :'q8' LIMIT 10;
SELECT id, h <+> :'q8' AS d FROM vt ORDER BY h <+> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
SELECT id, hc <=> :'q16' AS d FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, h <-> :'q8b' AS d FROM vt ORDER BY h <-> :'q8b' LIMIT 10;
SELECT id, h <#> :'q8b' AS d FROM vt ORDER BY h <#> :'q8b' LIMIT 10;
SELECT id, h <+> :'q8b' AS d FROM vt ORDER BY h <+> :'q8b' LIMIT 10;
SELECT id, hc <=> :'q16' AS d FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: sparsevec
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <-> :'s32' LIMIT 10;
SELECT id, s <-> :'s32' AS d FROM vt ORDER BY s <-> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <#> :'s32' LIMIT 10;
SELECT id, s <#> :'s32' AS d FROM vt ORDER BY s <#> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <+> :'s32' LIMIT 10;
SELECT id, s <+> :'s32' AS d FROM vt ORDER BY s <+> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
SELECT id, sc <=> :'s64' AS d FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, s <-> :'s32' AS d FROM vt ORDER BY s <-> :'s32' LIMIT 10;
SELECT id, sc <=> :'s64' AS d FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: bit, and binary_quantize
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY b <~> :'b64' LIMIT 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY b <~> :'b64' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY b <%> :'b64' LIMIT 10;
SELECT id, b <%> :'b64' AS d FROM vt ORDER BY b <%> :'b64' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) LIMIT 10;
SELECT id, binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) AS d FROM vt
  ORDER BY binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY b <~> :'b64' LIMIT 10;
SELECT id, b <%> :'b64' AS d FROM vt ORDER BY b <%> :'b64' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: the unlogged and the TOASTed tables
EXPLAIN (COSTS OFF) SELECT id FROM ut ORDER BY u <-> :'u4' LIMIT 10;
SELECT id, u <-> :'u4' AS d FROM ut ORDER BY u <-> :'u4' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5;
SELECT id, e <-> (SELECT e FROM wide WHERE id = 1230) AS d FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5;
-- IVFFlat: vector and halfvec around ct's clusters (the query is row 13's point)
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY g <#> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
SELECT id, g <#> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY g <#> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gc <=> (SELECT gc FROM ct WHERE id = 13) AS d FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gh <-> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gh <-> (SELECT gh FROM ct WHERE id = 13) AS d FROM ct ORDER BY gh <-> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gh <#> (SELECT gh FROM ct WHERE id = 13) AS d FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY ghc <=> (SELECT ghc FROM ct WHERE id = 13) LIMIT 10;
SELECT id, ghc <=> (SELECT ghc FROM ct WHERE id = 13) AS d FROM ct ORDER BY ghc <=> (SELECT ghc FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 13) LIMIT 10;
SELECT id, cb <~> (SELECT cb FROM ct WHERE id = 13) AS d FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ut ORDER BY u <#> :'u4' LIMIT 10;
SELECT id, u <#> :'u4' AS d FROM ut ORDER BY u <#> :'u4' LIMIT 10;
SET ivfflat.probes = 3;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 27) AS d FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 27) LIMIT 10;
SELECT id, gc <=> (SELECT gc FROM ct WHERE id = 27) AS d FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 27) LIMIT 10;
SELECT id, gh <#> (SELECT gh FROM ct WHERE id = 27) AS d FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 27) LIMIT 10;
SELECT id, cb <~> (SELECT cb FROM ct WHERE id = 27) AS d FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 27) LIMIT 10;
RESET ivfflat.probes;
-- Iterative scans with a filter
SET hnsw.iterative_scan = strict_order;
SELECT id FROM vt WHERE id % 9 = 4 ORDER BY v <-> :'q8' LIMIT 20;
SET hnsw.iterative_scan = relaxed_order;
SELECT id FROM vt WHERE id % 9 = 4 ORDER BY h <-> :'q8' LIMIT 20;
RESET hnsw.iterative_scan;
SET ivfflat.iterative_scan = relaxed_order;
SELECT id FROM ct WHERE id % 7 = 2 ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 20;
RESET ivfflat.iterative_scan;
-- A query of the wrong dimension against each access method's metapage
SELECT id FROM vt ORDER BY v <-> '[1,2,3]' LIMIT 1;
SELECT id FROM ct ORDER BY g <-> '[1,2,3]' LIMIT 1;
```

- [ ] **Step 7: Create `scripts/pgvector/sql/ondisk-followup.sql`**

```sql
-- DML and maintenance after the subject boots a fixture (run-ondisk.sh):
-- inserts into every index, deletes, VACUUM and REINDEX. New HNSW elements
-- draw random levels and REINDEX reruns k-means, so after this point index
-- answers are compared by row count, and exact answers (no index) row by
-- row.
\set lo 10001
\set hi 10300
\ir ondisk-rows.sql
DELETE FROM vt WHERE id % 7 = 3;
DELETE FROM ct WHERE id % 7 = 3;
DELETE FROM ut WHERE id % 7 = 3;
VACUUM vt;
VACUUM ct;
VACUUM ut;
VACUUM wide;
SELECT count(*) AS vt FROM vt;
SELECT count(*) AS ct FROM ct;
SELECT count(*) AS ut FROM ut;
SELECT count(*) AS wide FROM wide;
\set q8 '[3,1,4,1,5,9,2,6]'
\set q16 '[1,-1,1,1,-1,-1,1,-1,1,1,1,-1,-1,1,-1,1]'
\set s32 '{2:3,10:5,18:1,27:4}/32'
\set b64 '1100100100001111110110101010001000100001011010001100001000110100'
\set u4 '[2,3,4,5]'
-- Exact answers, no index
SET enable_indexscan = off;
SET enable_bitmapscan = off;
SELECT id, v <-> :'q8' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, h <#> :'q8' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, s <+> :'s32' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY d, id LIMIT 10;
SELECT id, u <-> :'u4' AS d FROM ut ORDER BY d, id LIMIT 10;
SELECT id, e <-> (SELECT e FROM wide WHERE id = 1230) AS d FROM wide ORDER BY d, id LIMIT 5;
RESET enable_indexscan;
RESET enable_bitmapscan;
-- Index answers: row counts, before and after REINDEX
SET enable_seqscan = off;
SET ivfflat.probes = 10;
SELECT (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY v <-> :'q8' LIMIT 10) s) AS v_l2,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY vc <=> :'q16' LIMIT 10) s) AS vc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY h <+> :'q8' LIMIT 10) s) AS h_l1,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY s <#> :'s32' LIMIT 10) s) AS s_ip,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY b <%> :'b64' LIMIT 10) s) AS b_jac,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY gc <=> :'q16' LIMIT 10) s) AS gc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY cb <~> :'b64' LIMIT 10) s) AS cb_ham,
       (SELECT count(*) FROM (SELECT 1 FROM ut ORDER BY u <-> :'u4' LIMIT 10) s) AS u_l2,
       (SELECT count(*) FROM (SELECT 1 FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5) s) AS e_l2;
REINDEX TABLE vt;
REINDEX TABLE ct;
REINDEX TABLE ut;
REINDEX TABLE wide;
SELECT (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY v <-> :'q8' LIMIT 10) s) AS v_l2,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY vc <=> :'q16' LIMIT 10) s) AS vc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY h <+> :'q8' LIMIT 10) s) AS h_l1,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY s <#> :'s32' LIMIT 10) s) AS s_ip,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY b <%> :'b64' LIMIT 10) s) AS b_jac,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY gc <=> :'q16' LIMIT 10) s) AS gc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY cb <~> :'b64' LIMIT 10) s) AS cb_ham,
       (SELECT count(*) FROM (SELECT 1 FROM ut ORDER BY u <-> :'u4' LIMIT 10) s) AS u_l2,
       (SELECT count(*) FROM (SELECT 1 FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5) s) AS e_l2;
```

- [ ] **Step 8: Create `scripts/pgvector/sql/ondisk-update.sql` and `ondisk-relfiles.sql`**

```sql
-- The 0.8.1 fixture after a binary upgrade (run-ondisk.sh): the index
-- queries on the 0.8.1 catalog, ALTER EXTENSION vector UPDATE, the catalog
-- it leaves (repair-verify.sql: version, members and comments, function
-- definitions, amop, amproc, opclasses), then the queries again.
SELECT extversion FROM pg_extension WHERE extname = 'vector';
\ir ondisk-queries.sql
ALTER EXTENSION vector UPDATE;
\ir repair-verify.sql
\pset format aligned
\pset tuples_only off
\ir ondisk-queries.sql
```

```sql
-- name|pagemask rmgr|path of every relation the fixture owns: its tables,
-- their TOAST tables, and the indexes of both (run-ondisk.sh compares their
-- pages after crash recovery).
\pset format unaligned
\pset tuples_only on
WITH t AS (SELECT oid, reltoastrelid FROM pg_class WHERE relnamespace = 'public'::regnamespace AND relkind = 'r'),
r AS (SELECT oid FROM t
      UNION SELECT reltoastrelid FROM t WHERE reltoastrelid <> 0
      UNION SELECT indexrelid FROM pg_index WHERE indrelid IN (SELECT oid FROM t UNION SELECT reltoastrelid FROM t))
SELECT c.relname || '|' || CASE WHEN c.relkind IN ('r', 't') THEN 'heap' WHEN a.amname = 'btree' THEN 'btree' ELSE 'generic' END
       || '|' || pg_relation_filepath(c.oid)
FROM pg_class c JOIN r ON r.oid = c.oid LEFT JOIN pg_am a ON a.oid = c.relam
ORDER BY 1;
```

- [ ] **Step 9: Run the test to verify it passes**

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_sql`
Expected: 7 `ok` lines and `all passed`, in about 1.5 minutes. If an `EXPLAIN` names a different index, or the counts differ, the SQL differs from this plan's: fix the file, never the test.

- [ ] **Step 10: Commit**

```bash
git add scripts/pgvector/sql/ondisk-*.sql scripts/pgvector/tests/harness_test.sh
git commit -q -m "pgvector harness: the on-disk fixture SQL" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `run-ondisk.sh`: the capture and the forward gate

**Files:**
- Create: `scripts/pgvector/run-ondisk.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (`ondisk_fact`; `test_ondisk_capture`, `test_ondisk_ref`, `test_ondisk_detects_difference`)

**Interfaces:**
- Consumes: Task 2's helpers and Task 3's SQL.
- Produces:
  - `run-ondisk.sh capture [--force]` and `run-ondisk.sh {pgrust|ref} [clean|crash|v081 ...]`.
  - Under `$PGV_WORK/ondisk/capture`: `key`, `facts.tsv`, and three fixture directories:
    - `clean/` holds `data`, `prestop-queries.out`, `expected-queries.out` and `expected-followup.out`;
    - `crash/` holds `data`, `replayed`, `relfiles.txt`, `inflight-xid`, `expected-queries.out` and `expected-followup.out`;
    - `v081/` holds `data`, `expected-update.out` and `expected-followup.out`.
  - `$PGV_WORK/ondisk/<mode>/summary.tsv`, seven rows: `clean_queries`, `clean_followup`, `crash_pages`, `crash_queries`, `crash_followup`, `v081_update`, `v081_followup`.
  - Functions Task 5 reuses: `load`, `transcript`, `state`, `open_transaction`, `close_transaction`, `compare_pages`, `subject_start`, `result`, plus the `DK_CONN` and `SUBJ_CONN` arrays.

- [ ] **Step 1: Write the failing self-tests**

Above the `# --- runner ---` marker in `harness_test.sh`, add:

```bash
# A value from the self-test capture's facts.tsv.
ondisk_fact() {
  awk -F'\t' -v k="$1" '$1 == k { print $2 }' "$PGV_WORK/ondisk/capture/facts.tsv" 2>/dev/null
}

# The capture (capture --force, in the self-test work dir) exercises what M5
# claims: clean and crashed directories; past the crash's redo point a WAL
# full of GenericXLog deltas, Heap and Btree records, and the open
# transaction's index records; 42 fixture relations and 25 scanned vector
# indexes; a TOASTed column; the 0.8.1 -> 0.8.7 update. A second capture is
# a cache hit.
test_ondisk_capture() {
  local log="$PGV_WORK/ondisk-capture.log" g f h b i
  if ! "$here/../run-ondisk.sh" capture --force >"$log" 2>&1; then
    fail "ondisk capture: exit 0 (see $log)"
    return
  fi
  pass "ondisk capture: exit 0"
  [ "$(ondisk_fact clean_state)/$(ondisk_fact v081_state)" = "shut down/shut down" ] &&
    pass "ondisk capture: clean and v081 shut down" ||
    fail "ondisk capture: clean '$(ondisk_fact clean_state)', v081 '$(ondisk_fact v081_state)'"
  [ "$(ondisk_fact crash_state)" = "in production" ] &&
    pass "ondisk capture: the crash copy needs recovery" ||
    fail "ondisk capture: crash state '$(ondisk_fact crash_state)'"
  g="$(ondisk_fact crash_generic_records)"
  f="$(ondisk_fact crash_generic_fpi)"
  [ "${g:-0}" -ge 100000 ] && [ "${f:-0}" -ge 1000 ] && [ $((${g:-0} - ${f:-0})) -ge 100000 ] &&
    pass "ondisk capture: $g Generic records after the redo point, $f with full-page images" ||
    fail "ondisk capture: $g Generic records, $f with full-page images (want >= 100000 and >= 1000)"
  h="$(ondisk_fact crash_heap_records)"
  b="$(ondisk_fact crash_btree_records)"
  [ "${h:-0}" -ge 1000 ] && [ "${b:-0}" -ge 1000 ] &&
    pass "ondisk capture: $h Heap and $b Btree records to replay" ||
    fail "ondisk capture: $h Heap and $b Btree records (want >= 1000 each)"
  i="$(ondisk_fact crash_inflight_generic)"
  [ "${i:-0}" -ge 10000 ] &&
    pass "ondisk capture: the open transaction wrote $i Generic records" ||
    fail "ondisk capture: the open transaction wrote $i Generic records (want >= 10000)"
  [ "$(ondisk_fact crash_relations)/$(ondisk_fact vector_indexes)" = 42/25 ] &&
    pass "ondisk capture: 42 relations, 25 vector indexes scanned" ||
    fail "ondisk capture: $(ondisk_fact crash_relations) relations, $(ondisk_fact vector_indexes) vector indexes (want 42, 25)"
  [ "$(ondisk_fact wide_toasted)" = t ] && pass "ondisk capture: wide is TOASTed" || fail "ondisk capture: wide is not TOASTed"
  [ "$(ondisk_fact v081_before) $(ondisk_fact v081_after)" = "0.8.1 0.8.7" ] &&
    pass "ondisk capture: v081 goes from 0.8.1 to 0.8.7" ||
    fail "ondisk capture: v081 '$(ondisk_fact v081_before)' -> '$(ondisk_fact v081_after)'"
  if "$here/../run-ondisk.sh" capture >"$log.2" 2>&1 && ! grep -q '^capture: clean' "$log.2"; then
    pass "ondisk capture: a second capture is a cache hit"
  else
    fail "ondisk capture: a second capture rebuilt or failed (see $log.2)"
  fi
}

# Forward gate, C against C: the local clang build reproduces the gcc image's
# answers, replayed pages and follow-up on all three directories.
test_ondisk_ref() {
  local n
  if "$here/../run-ondisk.sh" ref clean crash v081 >"$PGV_WORK/ondisk-ref.log" 2>&1; then
    pass "ondisk ref: exit 0"
  else
    fail "ondisk ref: exit 0 (see $PGV_WORK/ondisk-ref.log)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/ondisk/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 7 ]; then pass "ondisk ref: 7 ok"; else fail "ondisk ref: '$n' ok, want 7"; fi
}

# Negative control: deleting the rows nearest the first query right after
# the subject boots must fail the queries row and the replayed pages.
test_ondisk_detects_difference() {
  local s="$PGV_WORK/ondisk/ref/summary.tsv"
  if PGV_ONDISK_PERTURB_SQL="DELETE FROM vt WHERE v <-> '[3,1,4,1,5,9,2,6]' < 4" \
    "$here/../run-ondisk.sh" ref clean crash >"$PGV_WORK/ondisk-neg.log" 2>&1; then
    fail "ondisk flags a changed table"
  elif grep -q $'^clean_queries\tFAIL' "$s" && grep -q $'^crash_pages\tFAIL' "$s"; then
    pass "ondisk flags a changed table"
  else
    fail "ondisk flags a changed table: wrong failure (see $PGV_WORK/ondisk-neg.log)"
  fi
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_capture`
Expected: `FAIL - ondisk capture: exit 0`, because `run-ondisk.sh` does not exist.

- [ ] **Step 3: Create `scripts/pgvector/run-ondisk.sh`** (and `chmod +x` it)

```bash
#!/usr/bin/env bash
# On-disk tier, forward (spec §8.2 "On-disk, forward").
#
# Forward (gated). C pgvector writes three data directories in Docker:
#   clean  the 0.8.7 image: the fixture, then a clean stop;
#   crash  the 0.8.7 image: the fixture, CHECKPOINT, more writes
#          (sql/ondisk-crash.sql), a transaction left open
#          (sql/ondisk-inflight.sql), then pg_ctl stop -m immediate;
#   v081   the 0.8.1 image: the fixture, then a clean stop.
# C's answers come from the 0.8.7 image booting a copy of the same directory
# (for v081, through the same ALTER EXTENSION vector UPDATE). The subject
# boots another copy and must reproduce them exactly: the index queries
# (the same pages give the same traversal), then, after its own DML and
# maintenance (sql/ondisk-followup.sql), the counts, row counts and exact
# answers. After the crash, every relation the fixture owns must also replay
# to the same pages as C's replay, compared after pagemask. Rows:
#   clean_queries clean_followup crash_pages crash_queries crash_followup
#   v081_update v081_followup
#
# Every fixture value makes index scans compute exact distances
# (sql/ondisk-rows.sql), so C's answers do not depend on its compiler's
# vectorized, fused float sums (M2's no-emulation ruling stands).
#
# usage: run-ondisk.sh capture [--force]
#        run-ondisk.sh {pgrust|ref} [clean|crash|v081 ...]
#   capture: build the three directories and C's answers under
#     $PGV_WORK/ondisk/capture. Cached by a key over sql/ondisk-*.sql,
#     sql/repair-verify.sql, docker-lib.sh, this script and the image pins;
#     --force rebuilds. {pgrust|ref} capture first when the key changed.
#   pgrust: the subject is pgrust. ref: the subject is the local C reference
#     ($PG_VEC; clang, macOS), which must always pass.
#   With no parts, all three run. PGV_KEEP_DATA=1 keeps the subject's copies.
#   PGV_ONDISK_PERTURB_SQL runs on the subject right after its first boot of
#   each forward directory (harness self-test only).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"
. "$here/docker-lib.sh"

sql="$here/sql"
cap="$PGV_WORK/ondisk/capture"
DK_CONN=(-h 127.0.0.1 -p "$PGV_PORT_DOCKER")
INFLIGHT_PID=

usage() {
  die "usage: run-ondisk.sh capture [--force] | run-ondisk.sh {pgrust|ref} [clean|crash|v081 ...]"
}

cleanup() {
  if [ -n "$INFLIGHT_PID" ]; then kill "$INFLIGHT_PID" 2>/dev/null || true; fi
  dk_rm
}

# Run one sql/ file quietly, stopping at its first error. $1 file name; the
# rest are psql connection options.
load() {
  local f="$1"
  shift
  (cd "$sql" && "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 "$@" -U postgres -d postgres -f "$f")
}

# The echoed transcript of one sql/ file, made the same way on every side:
# from $sql (for its \ir includes), errors kept in the stream. $1 file name;
# the rest are psql connection options.
transcript() {
  local f="$1"
  shift
  (cd "$sql" && "$PG_TOOLS/bin/psql" -X -a -v ON_ERROR_STOP=0 "$@" -U postgres -d postgres -f "$f") 2>&1 || true
}

state() {
  "$PG_TOOLS/bin/pg_controldata" "$1" | sed -n 's/^Database cluster state: *//p'
}

# pg_waldump's records after $1's redo point; the rest are pg_waldump options.
waldump() {
  local d="$1" redo
  shift
  redo="$("$PG_TOOLS/bin/pg_controldata" "$d" | sed -n "s/^Latest checkpoint's REDO location: *//p")"
  "$PG_TOOLS/bin/pg_waldump" -p "$d/pg_wal" --start "$redo" "$@" 2>/dev/null || true
}

# Start sql/ondisk-inflight.sql in the background and wait until its
# transaction sleeps with its rows written; write its xid to $1. Then commit
# one more transaction: its commit flushes WAL past every record the open one
# wrote. The rest are psql connection options.
open_transaction() {
  local xidfile="$1" i xid=
  shift
  (cd "$sql" && exec "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 "$@" -U postgres -d postgres -f ondisk-inflight.sql) >/dev/null 2>&1 &
  INFLIGHT_PID=$!
  for i in $(seq 1 600); do
    xid="$("$PG_TOOLS/bin/psql" -X -At "$@" -U postgres -d postgres \
      -c "SELECT backend_xid FROM pg_stat_activity WHERE wait_event = 'PgSleep' AND backend_xid IS NOT NULL" 2>/dev/null || true)"
    [ -z "$xid" ] || break
    kill -0 "$INFLIGHT_PID" 2>/dev/null || return 1
    sleep 0.5
  done
  [ -n "$xid" ] || return 1
  echo "$xid" >"$xidfile"
  "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 "$@" -U postgres -d postgres -c "CREATE TABLE crash_marker AS SELECT 1 AS x"
}

# Reap the open transaction's psql once its server is gone.
close_transaction() {
  wait "$INFLIGHT_PID" 2>/dev/null || true
  INFLIGHT_PID=
}

capture_key() {
  {
    echo "$PGV_DOCKER_IMAGE $PGV_DOCKER_IMAGE_081"
    cat "$sql"/ondisk-*.sql "$sql/repair-verify.sql" "$here/docker-lib.sh" "$here/run-ondisk.sh"
  } | shasum -a 256 | cut -c1-16
}

# C's answers on a copy of $1/data: the 0.8.7 image boots it, then the
# transcripts of $2 (ondisk-queries.sql, or ondisk-update.sql for v081) and
# of the follow-up.
oracle() {
  local d="$1" q="$2" name
  name="${q#ondisk-}"
  name="${name%.sql}"
  dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" || die "the 0.8.7 image did not boot $d/data"
  transcript "$q" "${DK_CONN[@]}" >"$d/expected-$name.out"
  transcript ondisk-followup.sql "${DK_CONN[@]}" >"$d/expected-followup.out"
  dk_rm
}

capture() {
  local c="$cap/clean" k="$cap/crash" v="$cap/v081"
  dk_require
  rm -rf "$cap"
  mkdir -p "$c" "$k" "$v"

  echo "capture: clean (0.8.7)"
  dk_start_new "$PGV_DOCKER_IMAGE" -c autovacuum=off || die "the 0.8.7 image did not start"
  load ondisk-fixture.sql "${DK_CONN[@]}" >"$c/fixture.log" 2>&1 || die "the fixture failed; see $c/fixture.log"
  transcript ondisk-queries.sql "${DK_CONN[@]}" >"$c/prestop-queries.out"
  dk_stop_fast || die "docker stop failed"
  dk_copy_out "$c/data" || die "docker cp failed"
  echo "autovacuum = off" >>"$c/data/postgresql.conf"
  oracle "$c" ondisk-queries.sql
  cmp -s "$c/prestop-queries.out" "$c/expected-queries.out" ||
    die "C's answers changed across a clean restart: $c/prestop-queries.out vs $c/expected-queries.out"

  echo "capture: crash (0.8.7)"
  dk_start_new "$PGV_DOCKER_IMAGE" -c autovacuum=off -c checkpoint_timeout=1d -c max_wal_size=10GB ||
    die "the 0.8.7 image did not start"
  load ondisk-fixture.sql "${DK_CONN[@]}" >"$k/fixture.log" 2>&1 || die "the fixture failed; see $k/fixture.log"
  dk_psql -q -c CHECKPOINT
  load ondisk-crash.sql "${DK_CONN[@]}" >"$k/crash.log" 2>&1 || die "the crash writes failed; see $k/crash.log"
  open_transaction "$k/inflight-xid" "${DK_CONN[@]}" || die "the open transaction did not start"
  dk_stop_immediate || die "the immediate stop failed"
  close_transaction
  dk_copy_out "$k/data" || die "docker cp failed"
  [ "$(state "$k/data")" = "in production" ] || die "the immediate stop left '$(state "$k/data")', not a crash"
  echo "autovacuum = off" >>"$k/data/postgresql.conf"
  # C's own replay of the crash, kept for crash_pages.
  dk_boot_dir "$PGV_DOCKER_IMAGE" "$k/data" || die "the 0.8.7 image did not replay $k/data"
  dk_psql -q -f "$sql/ondisk-relfiles.sql" >"$k/relfiles.txt"
  dk_stop_fast || die "docker stop failed"
  dk_copy_out "$k/replayed" || die "docker cp failed"
  oracle "$k" ondisk-queries.sql

  echo "capture: v081 (0.8.1)"
  dk_start_new "$PGV_DOCKER_IMAGE_081" -c autovacuum=off || die "the 0.8.1 image did not start"
  load ondisk-fixture.sql "${DK_CONN[@]}" >"$v/fixture.log" 2>&1 || die "the fixture failed; see $v/fixture.log"
  dk_stop_fast || die "docker stop failed"
  dk_copy_out "$v/data" || die "docker cp failed"
  echo "autovacuum = off" >>"$v/data/postgresql.conf"
  oracle "$v" ondisk-update.sql

  facts
  capture_key >"$cap/key"
}

# What the capture exercised, for the harness self-test and the report.
facts() {
  local k="$cap/crash/data" xid
  xid="$(cat "$cap/crash/inflight-xid")"
  {
    printf 'clean_state\t%s\n' "$(state "$cap/clean/data")"
    printf 'v081_state\t%s\n' "$(state "$cap/v081/data")"
    printf 'crash_state\t%s\n' "$(state "$k")"
    printf 'crash_generic_records\t%s\n' "$(waldump "$k" --rmgr=Generic | wc -l | tr -d ' ')"
    printf 'crash_generic_fpi\t%s\n' "$(waldump "$k" --rmgr=Generic --fullpage | wc -l | tr -d ' ')"
    printf 'crash_heap_records\t%s\n' "$(waldump "$k" --rmgr=Heap | wc -l | tr -d ' ')"
    printf 'crash_btree_records\t%s\n' "$(waldump "$k" --rmgr=Btree | wc -l | tr -d ' ')"
    printf 'crash_inflight_generic\t%s\n' "$(waldump "$k" --rmgr=Generic --xid="$xid" | wc -l | tr -d ' ')"
    printf 'crash_relations\t%s\n' "$(grep -c . "$cap/crash/relfiles.txt")"
    printf 'vector_indexes\t%s\n' "$(sed -n -E 's/^ *->  Index Scan using ([a-z0-9_]+) on (vt|ct|ut|wide)$/\1/p' \
      "$cap/clean/expected-queries.out" | sort -u | wc -l | tr -d ' ')"
    printf 'wide_toasted\t%s\n' "$(awk '/AS wide_toasted/ { getline; getline; getline; print $1; exit }' "$cap/clean/expected-queries.out")"
    printf 'v081_before\t%s\n' "$(awk '/^SELECT extversion FROM pg_extension/ { getline; getline; getline; print $1; exit }' "$cap/v081/expected-update.out")"
    printf 'v081_after\t%s\n' "$(sed -n 's/^ext|\([^|]*\)|.*/\1/p' "$cap/v081/expected-update.out")"
  } >"$cap/facts.tsv"
}

ensure_capture() {
  if [ "$1" = 1 ] || [ ! -f "$cap/key" ] || [ "$(cat "$cap/key")" != "$(capture_key)" ]; then
    capture
  fi
}

result() {
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/summary.tsv"
}

# Row $1: C's transcript $2 against the subject's $3.
compare() {
  if diff -u "$2" "$3" >"$out/$1.diff"; then
    rm -f "$out/$1.diff"
    result "$1" ok
  else
    result "$1" "FAIL ($out/$1.diff)"
  fi
}

# Masked pages of every relation in $3 (name|rmgr|path, main and init forks):
# the subject's replay under $1 against C's under $2. Prints each file that
# differs or exists on one side only. (Every fixture relation is under 1 GB,
# so there are no .1 segments.)
compare_pages() {
  local a="$1" b="$2" name rm path fork off rc=0
  while IFS='|' read -r name rm path; do
    [ -n "$path" ] || continue
    for fork in "" _init; do
      if [ ! -e "$b/$path$fork" ]; then
        if [ -e "$a/$path$fork" ]; then
          echo "$name$fork: only in the subject's replay"
          rc=1
        fi
        continue
      fi
      if [ ! -e "$a/$path$fork" ]; then
        echo "$name$fork: missing from the subject's replay"
        rc=1
        continue
      fi
      "$PAGEMASK_BIN" "$rm" "$a/$path$fork" "$out/subject.masked" || die "pagemask failed on $a/$path$fork"
      "$PAGEMASK_BIN" "$rm" "$b/$path$fork" "$out/c.masked" || die "pagemask failed on $b/$path$fork"
      if ! cmp -s "$out/subject.masked" "$out/c.masked"; then
        off="$({ cmp "$out/subject.masked" "$out/c.masked" 2>&1 || true; } | sed -n 's/.* char \([0-9]*\).*/\1/p')"
        echo "$name$fork ($rm): differs from block $(((${off:-1} - 1) / 8192)), offset $(((${off:-1} - 1) % 8192))"
        rc=1
      fi
    done
  done <"$3"
  rm -f "$out/subject.masked" "$out/c.masked"
  return "$rc"
}

# Boot the subject on data directory $1, logging to $1.boot.log.
subject_start() {
  if (server_start "$subject" "$1" "$port") >>"$1.boot.log" 2>&1; then
    return 0
  fi
  server_stop "$subject"
  return 1
}

forward() {
  local f="$1" d="$out/$1" c="$cap/$1" q=ondisk-queries.sql name=queries rows r
  if [ "$f" = v081 ]; then
    q=ondisk-update.sql
    name=update
  fi
  rm -rf "$d"
  mkdir -p "$d"
  cp -R "$c/data" "$d/data"
  if ! subject_start "$d/data"; then
    if [ "$f" = crash ]; then rows="pages queries followup"; else rows="$name followup"; fi
    for r in $rows; do
      result "${f}_$r" "FAIL (boot; $d/data.boot.log, $PGV_WORK/log/server-$subject.log)"
    done
    return
  fi
  if [ -n "${PGV_ONDISK_PERTURB_SQL:-}" ]; then
    pgv_psql "$port" -c "$PGV_ONDISK_PERTURB_SQL" >/dev/null
  fi
  if [ "$f" = crash ]; then
    server_stop "$subject"
    if compare_pages "$d/data" "$c/replayed" "$c/relfiles.txt" >"$d/pages.txt"; then
      result crash_pages ok
    else
      result crash_pages "FAIL ($d/pages.txt)"
    fi
    if ! subject_start "$d/data"; then
      result crash_queries "FAIL (restart; $d/data.boot.log)"
      result crash_followup "FAIL (restart; $d/data.boot.log)"
      return
    fi
  fi
  transcript "$q" "${SUBJ_CONN[@]}" >"$d/$name.out"
  compare "${f}_$name" "$c/expected-$name.out" "$d/$name.out"
  transcript ondisk-followup.sql "${SUBJ_CONN[@]}" >"$d/followup.out"
  compare "${f}_followup" "$c/expected-followup.out" "$d/followup.out"
  server_stop "$subject"
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data"; fi
}

# --- main ---
[ "$#" -ge 1 ] || usage
mode="$1"
shift
case "$mode" in
  capture)
    trap cleanup EXIT
    case "$#:${1:-}" in
      0:) ensure_capture 0 ;;
      1:--force) ensure_capture 1 ;;
      *) usage ;;
    esac
    echo "capture: $cap"
    exit 0
    ;;
  pgrust | ref) subject="$mode" ;;
  *) usage ;;
esac
port="$(port_for "$subject")"
SUBJ_CONN=(-h "$PGV_WORK/sock" -p "$port")
if [ "$#" -gt 0 ]; then parts=("$@"); else parts=(clean crash v081); fi
for p in "${parts[@]}"; do
  case "$p" in clean | crash | v081) ;; *) usage ;; esac
done
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/ondisk/$mode"
trap 'cleanup; server_stop "$subject"' EXIT
ensure_capture 0
rm -rf "$out"
mkdir -p "$out"
: >"$out/summary.tsv"
server_stop "$subject"
for p in "${parts[@]}"; do
  forward "$p"
done
total=$(wc -l <"$out/summary.tsv" | tr -d ' ')
passed=$(grep -c $'\tok$' "$out/summary.tsv" || true)
echo "ondisk ($mode, subject $subject): $passed/$total checks passed; diffs in $out"
[ "$passed" -eq "$total" ]
```

- [ ] **Step 4: Run the self-tests to verify they pass**

```bash
/bin/bash -n scripts/pgvector/run-ondisk.sh
scripts/pgvector/tests/harness_test.sh test_ondisk_capture test_ondisk_ref test_ondisk_detects_difference
```
Expected: `all passed`, in about 9 minutes. Copy the self-test capture's `facts.tsv` (`~/.cache/pgrust/pgvector-work/selftest/ondisk/capture/facts.tsv`) into the task report.

- [ ] **Step 5: Run the gate on pgrust**

```bash
scripts/pgvector/run-ondisk.sh pgrust
cat ~/.cache/pgrust/pgvector-work/ondisk/pgrust/summary.tsv
```
Expected: the first run captures (about 4 minutes) under the real work dir, then `ondisk (pgrust, subject pgrust): 7/7 checks passed`. Any row that fails while `ref` passed on the same capture is a plan-defined stop (Execution notes).

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/run-ondisk.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "pgvector harness: the on-disk tier (capture and forward gate)" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: The reverse report

**Files:**
- Modify: `scripts/pgvector/run-ondisk.sh` (header, usage, the reverse section, the default parts)
- Modify: `scripts/pgvector/tests/harness_test.sh` (`test_ondisk_reverse_ref`)

**Interfaces:**
- Consumes: Task 4's functions.
- Produces `$PGV_WORK/ondisk/<mode>/reverse.tsv`, eight rows, each with `ok`, `identical`, `clean`, `none`, a list of rows, `differs (<diff>)` or `failed: <step>`. It also produces `reverse.md`, which Task 6's `run-all.sh` prints.

- [ ] **Step 1: Write the failing self-test**

Above the `# --- runner ---` marker in `harness_test.sh`, add:

```bash
# Reverse, C against C: the gcc image boots what the local clang build wrote,
# stopped cleanly and crashed, with the same answers and replayed pages;
# pg_amcheck is clean and there are no pgrust-only rows.
test_ondisk_reverse_ref() {
  local r="$PGV_WORK/ondisk/ref/reverse.tsv" want
  if "$here/../run-ondisk.sh" ref reverse >"$PGV_WORK/ondisk-reverse.log" 2>&1; then
    pass "ondisk reverse ref: exit 0"
  else
    fail "ondisk reverse ref: exit 0 (see $PGV_WORK/ondisk-reverse.log)"
  fi
  want="clean_boot ok
clean_queries identical
clean_amcheck clean
clean_pgrust_rows none
clean_followup identical
crash_boot ok
crash_pages identical
crash_queries identical"
  if [ "$(tr '\t' ' ' <"$r" 2>/dev/null)" = "$want" ]; then
    pass "ondisk reverse ref: all eight checks identical"
  else
    fail "ondisk reverse ref: $r is not all identical"
  fi
  if [ -s "$PGV_WORK/ondisk/ref/reverse.md" ]; then
    pass "ondisk reverse ref: reverse.md written"
  else
    fail "ondisk reverse ref: no reverse.md"
  fi
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_reverse_ref`
Expected: `FAIL - ondisk reverse ref: exit 0`, because `reverse` is not a part yet and usage dies.

- [ ] **Step 3: Extend `run-ondisk.sh`**

(a) Header. Line 2, `# On-disk tier, forward (spec §8.2 "On-disk, forward").`, becomes:

```bash
# On-disk tier (spec §8.2 "On-disk, forward" and "On-disk, reverse").
```
The two lines `#   v081_update v081_followup` and the `#` after them become:
```bash
#   v081_update v081_followup
# Reverse (reported, never gated): the subject writes the fixture into a
# fresh cluster, once stopped cleanly and once crashed the same way, and the
# 0.8.7 image boots it ($PGV_WORK/ondisk/<mode>/reverse.tsv and reverse.md).
#
```

(b) Usage, in the header and in `usage()`: `[clean|crash|v081 ...]` becomes `[clean|crash|v081|reverse ...]`, and `With no parts, all three run.` becomes `With no parts, all four run.`

(c) Insert this section immediately above `# --- main ---`:

```bash
# --- reverse: reported, never gated ---

rrow() {
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/reverse.tsv"
}

# Row $1: "identical" when files $2 and $3 match, else the diff's path.
rcompare() {
  if diff -u "$2" "$3" >"$4"; then
    rm -f "$4"
    rrow "$1" identical
  else
    rrow "$1" "differs ($4)"
  fi
}

# A fresh C-initdb cluster at $1; the rest are postgresql.conf lines.
fresh_cluster() {
  local data="$1"
  shift
  "$PG_TOOLS/bin/initdb" -D "$data" --no-locale --encoding UTF8 -U postgres -A trust >"$data.initdb.log" 2>&1 ||
    die "initdb failed; see $data.initdb.log"
  printf '%s\n' "$@" >>"$data/postgresql.conf"
}

reverse_clean() {
  local d="$out/reverse/clean"
  mkdir -p "$d"
  fresh_cluster "$d/data" "autovacuum = off"
  if ! subject_start "$d/data" || ! load ondisk-fixture.sql "${SUBJ_CONN[@]}" >"$d/fixture.log" 2>&1; then
    server_stop "$subject"
    rrow clean_boot "failed: the subject's fixture ($d)"
    return
  fi
  transcript ondisk-queries.sql "${SUBJ_CONN[@]}" >"$d/subject-queries.out"
  server_stop "$subject"
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" >"$d/c-boot.log" 2>&1; then
    rrow clean_boot "failed: C did not start ($d/c-boot.log)"
    dk_rm
    return
  fi
  rrow clean_boot ok
  transcript ondisk-queries.sql "${DK_CONN[@]}" >"$d/c-queries.out"
  rcompare clean_queries "$d/subject-queries.out" "$d/c-queries.out" "$d/queries.diff"
  if dk_exec pg_amcheck --install-missing --heapallindexed -d postgres >"$d/amcheck.txt" 2>&1; then
    rrow clean_amcheck clean
  else
    rrow clean_amcheck "corruption reported ($d/amcheck.txt)"
  fi
  rrow clean_pgrust_rows "$(dk_psql -At -c "SELECT coalesce(string_agg(proname, ' ' ORDER BY proname), 'none') FROM pg_proc WHERE proname LIKE 'pgrust%'")"
  transcript ondisk-followup.sql "${DK_CONN[@]}" >"$d/c-followup.out"
  rcompare clean_followup "$cap/clean/expected-followup.out" "$d/c-followup.out" "$d/followup.diff"
  dk_rm
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data"; fi
}

reverse_crash() {
  local d="$out/reverse/crash"
  mkdir -p "$d"
  fresh_cluster "$d/data" "autovacuum = off" "checkpoint_timeout = 1d" "max_wal_size = 10GB"
  if ! subject_start "$d/data" ||
    ! load ondisk-fixture.sql "${SUBJ_CONN[@]}" >"$d/fixture.log" 2>&1 ||
    ! pgv_psql "$port" -c CHECKPOINT >/dev/null 2>&1 ||
    ! load ondisk-crash.sql "${SUBJ_CONN[@]}" >"$d/crash.log" 2>&1 ||
    ! open_transaction "$d/inflight-xid" "${SUBJ_CONN[@]}" >/dev/null 2>&1; then
    server_stop "$subject"
    close_transaction
    rrow crash_boot "failed: the subject's fixture ($d)"
    return
  fi
  server_stop "$subject" QUIT
  close_transaction
  if [ "$(state "$d/data")" != "in production" ]; then
    rrow crash_boot "failed: the subject's immediate stop left '$(state "$d/data")'"
    return
  fi
  # The subject's own replay: pages after a clean stop, then the queries.
  cp -R "$d/data" "$d/subject"
  if ! subject_start "$d/subject"; then
    rrow crash_boot "failed: the subject did not replay its own crash ($d/subject.boot.log)"
    return
  fi
  pgv_psql "$port" -f "$sql/ondisk-relfiles.sql" >"$d/relfiles.txt"
  server_stop "$subject"
  cp -R "$d/subject" "$d/subject-replayed"
  if ! subject_start "$d/subject"; then
    rrow crash_boot "failed: the subject did not restart ($d/subject.boot.log)"
    return
  fi
  transcript ondisk-queries.sql "${SUBJ_CONN[@]}" >"$d/subject-queries.out"
  server_stop "$subject"
  # C's replay of the same directory.
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" >"$d/c-boot.log" 2>&1; then
    rrow crash_boot "failed: C did not replay it ($d/c-boot.log)"
    dk_rm
    return
  fi
  rrow crash_boot ok
  if dk_stop_fast && dk_copy_out "$d/c-replayed" && dk_start; then
    if compare_pages "$d/subject-replayed" "$d/c-replayed" "$d/relfiles.txt" >"$d/pages.txt"; then
      rrow crash_pages identical
    else
      rrow crash_pages "differs ($d/pages.txt)"
    fi
    transcript ondisk-queries.sql "${DK_CONN[@]}" >"$d/c-queries.out"
    rcompare crash_queries "$d/subject-queries.out" "$d/c-queries.out" "$d/queries.diff"
  else
    rrow crash_pages "failed: C's stop, copy or restart"
  fi
  dk_rm
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data" "$d/subject" "$d/subject-replayed" "$d/c-replayed"; fi
}

reverse() {
  rm -rf "$out/reverse"
  : >"$out/reverse.tsv"
  reverse_clean
  reverse_crash
  {
    printf '# On-disk, reverse (%s): reported, not gated\n\n' "$mode"
    printf 'The subject (%s) wrote the fixture into a fresh cluster; the pgvector 0.8.7 image booted it.\n\n' "$subject"
    printf '| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name r; do
      printf '| %s | %s |\n' "$name" "$r"
    done <"$out/reverse.tsv"
  } >"$out/reverse.md"
}
```

(d) In the main section, replace the parts default and check, the loop, and the final `echo`:

```bash
if [ "$#" -gt 0 ]; then parts=("$@"); else parts=(clean crash v081 reverse); fi
for p in "${parts[@]}"; do
  case "$p" in clean | crash | v081 | reverse) ;; *) usage ;; esac
done
```
```bash
for p in "${parts[@]}"; do
  case "$p" in
    reverse) reverse ;;
    *) forward "$p" ;;
  esac
done
```
```bash
echo "ondisk ($mode, subject $subject): $passed/$total forward checks passed; reverse: $out/reverse.md"
```

- [ ] **Step 4: Run the self-test to verify it passes**

```bash
/bin/bash -n scripts/pgvector/run-ondisk.sh
scripts/pgvector/tests/harness_test.sh test_ondisk_reverse_ref
```
Expected: `all passed`. The edit changed the capture key, so the self-test work dir captures again first, which takes about 4 more minutes.

- [ ] **Step 5: Run the whole tier on pgrust**

```bash
scripts/pgvector/run-ondisk.sh pgrust
cat ~/.cache/pgrust/pgvector-work/ondisk/pgrust/summary.tsv ~/.cache/pgrust/pgvector-work/ondisk/pgrust/reverse.tsv
```
Expected: 7/7 forward. Reverse, as in the draft run: every row `ok`, `identical` or `clean`, and `clean_pgrust_rows` lists `pgrust_pin_database pgrust_seal_template pgrust_unpin_database`. Reverse is not gated. Record what it says, and a `differs` row is a finding for the report, not a stop.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/run-ondisk.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "pgvector harness: the on-disk reverse report" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `run-all.sh`, the stability measurement, the docs and the M5 report

**Files:**
- Modify: `scripts/pgvector/run-all.sh`
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (§8.1, §8.2's two on-disk rows, §8.3)
- Modify: `CLAUDE.md` ("Vector search (fork work)")
- Create: `docs/superpowers/reports/pgvector-m5-report.md`

**Interfaces:** consumes everything above.

- [ ] **Step 1: Add the tier to `run-all.sh`**

(a) After `"$here/run-diff.sh" "$mode" || true`, add:

```bash
"$here/run-ondisk.sh" "$mode" || true
```

(b) In the report loop, `for tier in regress tap bytecmp iterscan diff; do` becomes `for tier in regress tap bytecmp iterscan diff ondisk; do`.

(c) After `printf '\n**%d/%d passed**\n' "$passed" "$total"` and inside the same `{ … } >"$report"` block, add:

```bash
  # On-disk, reverse (spec §8.2): reported, never counted.
  if [ -s "$PGV_WORK/ondisk/$mode/reverse.tsv" ]; then
    printf '\n## Reported, not gated: on-disk reverse\n\n| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name result; do
      printf '| %s | %s |\n' "$name" "$result"
    done <"$PGV_WORK/ondisk/$mode/reverse.tsv"
  fi
```

(d) Run: `/bin/bash -n scripts/pgvector/run-all.sh`

- [ ] **Step 2: The stability measurement (three fresh captures)**

Each capture builds new random HNSW graphs and new k-means centers, so passing on three of them shows that the gate does not rest on one lucky graph.

```bash
for i in 1 2 3; do
  scripts/pgvector/run-ondisk.sh capture --force >"$HOME/.cache/pgrust/m5-stability-$i.log" 2>&1 || echo "capture $i failed"
  cp ~/.cache/pgrust/pgvector-work/ondisk/capture/facts.tsv "$HOME/.cache/pgrust/m5-stability-facts-$i.tsv"
  scripts/pgvector/run-ondisk.sh ref clean crash v081 >>"$HOME/.cache/pgrust/m5-stability-$i.log" 2>&1
  echo "run $i ref: $(grep -c $'\tok$' ~/.cache/pgrust/pgvector-work/ondisk/ref/summary.tsv)/7"
  scripts/pgvector/run-ondisk.sh pgrust clean crash v081 >>"$HOME/.cache/pgrust/m5-stability-$i.log" 2>&1
  echo "run $i pgrust: $(grep -c $'\tok$' ~/.cache/pgrust/pgvector-work/ondisk/pgrust/summary.tsv)/7"
done
```
Expected: `7/7` six times, in about 30 minutes. On any failure, follow the plan-defined stop: copy that capture aside before the next iteration overwrites it. The report quotes the three facts files.

- [ ] **Step 3: The full gates**

```bash
scripts/pgvector/run-all.sh ref
scripts/pgvector/run-all.sh pgrust
```
Expected:
- `ref`: `**137/137 passed**`; its reverse section shows `ok`, `identical`, `clean` or `none` on every row.
- `pgrust`: `**129/137 passed**`. The only failures are the 8 TAP parallel-build rows (`003`, `012`, `020`, `024`, `028`, `032`, `035`, `045`).

Copy both reports (`~/.cache/pgrust/pgvector-work/report-{ref,pgrust}.md`) into the M5 report's evidence before anything reruns.

- [ ] **Step 4: The full self-test suite**

Run: `scripts/pgvector/tests/harness_test.sh`
Expected: `all passed`. It includes the existing tiers' self-tests, so it is long.

- [ ] **Step 5: Update the spec**

(a) §8.1, the Docker bullet becomes:

```markdown
- **Docker `pgvector/pgvector:0.8.7-pg18`** serves the on-disk gate. It is the real-world artifact (Debian PGDG packages, PostgreSQL 18.6, gcc) and runs on Linux, either in a container or on a VM. Clusters are created with `initdb --no-locale --encoding UTF8` to avoid collation differences. Since M5 the on-disk tier also uses **`pgvector/pgvector:0.8.1-pg18`** (PostgreSQL 18.2) for its upgrade fixture. Both are pinned by digest in `scripts/pgvector/common.sh`.
```

(b) §8.2, the two on-disk rows become:

```markdown
| On-disk, forward | `scripts/pgvector/run-ondisk.sh` (M5). C pgvector writes three directories in Docker: **clean** (0.8.7 image, clean stop), **crash** (0.8.7 image; after a `CHECKPOINT`, on-disk inserts into every index, deletes and `VACUUM`, then a transaction left open, then `pg_ctl stop -m immediate`) and **v081** (0.8.1 image, clean stop). The fixture (`sql/ondisk-fixture.sql`) has all four types, every HNSW and IVFFlat opclass (25 vector indexes), a `binary_quantize` expression index, an unlogged table, a TOASTed 1,536-dimension table, deletes, `VACUUM` and `ANALYZE`, and every value makes index scans compute exact distances (`sql/ondisk-rows.sql`). C's answers come from the 0.8.7 image booting a copy of the same directory (for v081, after the same `ALTER EXTENSION vector UPDATE`). pgrust boots another copy, reruns the fixture queries (`sql/ondisk-queries.sql`), then runs inserts, deletes, `VACUUM` and `REINDEX` (`sql/ondisk-followup.sql`). | Query results **identical to C's on the same directory, including approximate-search results**: the same index pages give the same traversal. After the crash, every fixture relation also replays to the same pages as C's replay (after `pagemask`). After the follow-up, counts, index-scan row counts and exact (non-index) answers equal C's after the same follow-up; index answers are not compared row by row, since new elements draw random levels. Seven rows. | Yes |
| On-disk, reverse | pgrust writes the same fixture into a fresh cluster, once stopped cleanly and once crashed the same way; Docker pgvector boots it, reruns the queries and the follow-up, runs `pg_amcheck`, lists pgrust-only catalog rows, and compares its replay of the crash with pgrust's own | Reported (`reverse.md`; `run-all.sh` prints it) | No |
```

(c) §8.3, after the `run-repair-check.sh` bullet, add:

```markdown
  - `docker-lib.sh` and `sql/ondisk-*.sql` (M5; `run-ondisk.sh` is the tier)
```

- [ ] **Step 6: Update `CLAUDE.md`**

(a) In the harness code block, after the `run-repair-check.sh` line, add:

```bash
scripts/pgvector/run-ondisk.sh capture [--force]                            # Docker: C pgvector writes the clean, crash and 0.8.1 directories and C's answers (cached)
scripts/pgvector/run-ondisk.sh {pgrust|ref} [clean|crash|v081|reverse ...]  # on-disk forward gate (7 rows) and the reverse report
```

(b) After the "Parallel index builds (M6) are **not ported**" line, add:

```markdown
**On-disk (M5):** a PG18 + pgvector data directory written by C in Docker (stopped cleanly, crashed mid-transaction, or made at 0.8.1 and then updated) boots in pgrust with C's answers and C's replayed pages (`run-ondisk.sh`, which needs Docker). C booting a pgrust-written directory is reported, not gated.
```

(c) Add `docs/superpowers/reports/pgvector-m5-report.md` to the Baselines list.

- [ ] **Step 7: Write `docs/superpowers/reports/pgvector-m5-report.md`**

Follow the M4 report's shape. Required content:
- **Summary and gates:**
  - the `run-all.sh` totals for both modes (Step 3) and the on-disk rows;
  - the commit range;
  - the exit criteria met.
- **What M5 built:**
  - the files, with one line each;
  - the capture/oracle design;
  - the seven rows and the eight reverse rows.
- **The fixture and the float question:**
  - Decision 3's exactness rules;
  - that the local clang build matched the gcc image everywhere;
  - that IVFFlat center distances are the one rounded comparison, and why it does not bite.
- **What the capture exercised:** the final capture's `facts.tsv`, and the range across the three stability captures (Generic records and full-page images after the redo point, the open transaction's records, relations, indexes).
- **Stability:** the six `7/7` lines from Step 2.
- **Reverse:**
  - both modes' tables;
  - the pgrust-only `pg_proc` rows (C sees them; pgrust backfills them on first connection);
  - that `pg_amcheck` is clean;
  - that C replays pgrust's crash WAL page-identically, if it does.
- **Findings and known divergences:** anything the runs turned up, with the plan-defined-stop decisions the user made.
- **Coverage limits:**
  - `wal_compression = lz4|zstd` full-page images are refused by pgrust's reader, and the Docker build supports both;
  - x86 data directories are untested (the images ran as arm64);
  - no relation reaches a `.1` segment;
  - after DML, index answers are compared by count only;
  - parallel builds (M6) and the approximate differential (M7) are not covered;
  - wasm is unchanged.
- **Follow-ups closed:** Task 1, with the ledger's parked item.
- **Still open:** the two deferred core items (Decision 8) and every deferred review minor, by task.

- [ ] **Step 8: Commit**

```bash
git add scripts/pgvector/run-all.sh docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m5-report.md
git commit -q -m "docs(pgvector): M5 report; spec, CLAUDE.md; run-all.sh gains the on-disk tier" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 9: Leave nothing running**

```bash
docker ps -a --format '{{.Names}}' | grep pgrust-pgvector || echo "no containers"
pgrep -fl "postgres -D" || echo "no servers"
git status --porcelain --ignored -- crates/pgvector-0.8.7-reference crates/postgres-18.6-reference
```
Expected: no containers, no servers, and empty reference-tree status.

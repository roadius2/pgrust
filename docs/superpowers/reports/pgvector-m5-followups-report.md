# pgvector conformance after the M5 follow-ups

Date: 2026-10-19 · pgrust commit: `436bd1b16f` (the server's code is unchanged since M5: no file under `crates/`, `Cargo.toml` or `Cargo.lock` changed from there to this branch, which changes only `scripts/pgvector/` and docs) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY -DIVFFLAT_BENCH), Docker `pgvector/pgvector:0.8.7-pg18` and `pgvector/pgvector:0.8.1-pg18`, both linux/arm64 and pinned by digest · Plan: `docs/superpowers/plans/2026-10-15-pgvector-m5-followups.md`

## Summary
- **C reference:** 137/137 (regress 14/14, TAP 48/48, bytecmp 28/28, iterscan 19/19, diff 21/21, ondisk 7/7).
- **pgrust:** 129/137 (regress 14/14, TAP 40/48, bytecmp 28/28, iterscan 19/19, diff 21/21, ondisk 7/7).
  - Both are M5's totals: no tier gained or lost a row (plan Decision 8). `report-ref.md` and `report-pgrust.md` are byte-identical to M5's fix-round reports (`~/.cache/pgrust/m5-evidence/fr1/`; `diff` prints nothing), reverse sections included.
  - Neither report has a `(run-X.sh exited N)` or `(no results; run-X.sh exited N)` row, so every tier's exit status agreed with its rows.
  - The 8 failures are the same parallel builds (M6) as in M5: TAP `003`, `012`, `020`, `024`, `028`, `032`, `035` and `045` (Remaining failures).
- **Stability:** three fresh captures, each with new HNSW graphs and new k-means centers, each passing the new `capture_check`, and each followed by 7/7 for `ref` and for `pgrust` (Stability):

  ```
  capture 1: exit 0
  run 1 ref: exit 0, 7/7
  run 1 pgrust: exit 0, 7/7
  capture 2: exit 0
  run 2 ref: exit 0, 7/7
  run 2 pgrust: exit 0, 7/7
  capture 3: exit 0
  run 3 ref: exit 0, 7/7
  run 3 pgrust: exit 0, 7/7
  ```
- **`harness_test.sh`:** all passed, 192 ok (M5's 161 plus this plan's 31: Task 1's 10, Task 2's 4, Task 3's 12 and Task 4's 5), in 363 s. Its own capture, a fourth fresh one made by the final code, exited 0 (so it passed `capture_check`) with 546,859 Generic records after the redo point (2,621 with full-page images) and 75,770 from the open transaction, and `test_ondisk_ref` passed 7/7 on it.
- **What changed** (one task each; details under "What changed"):
  - `run-all.sh` counts every tier's exit status and always prints the reverse section; `run-bytecmp.sh` and `run-diff.sh` clear their results before their binary checks (Task 1).
  - `docker-lib.sh`'s `dk_stop_immediate` is bounded, and a failed `docker run` leaves no container (Task 2).
  - The capture moves into `scripts/pgvector/ondisk-capture.sh`, and its key covers that file, `docker-lib.sh`, the on-disk SQL, the image pins and the psql version, so a comment in `run-ondisk.sh` no longer forces a recapture; `run-ondisk.sh` clears its results before anything that can fail (Task 3).
  - The capture checks its own coverage before it writes its key (Task 4).
- **Spec §9's M5 exit criteria still hold:** "Forward gate passes (clean, crash, 0.8.1 upgrade); reverse report produced." All seven `ondisk` rows are `ok` in both modes, and `reverse.md` is written in both and printed under "Reported, not gated" (Reverse).
- **The M5 report's three errata** are fixed in place (plan Decision 7; its "Still open" otherwise stays a snapshot): Task 1's run now cites `~/.cache/pgrust/m5-evidence/sdd-reports/task-1-report.md` (line 17); the Evidence bullets name `stale-results-demo.txt` and `build-pgrust.txt` (lines 45–46); and the exit-status follow-up no longer says `run-iterscan.sh` and `run-diff.sh` can die after writing rows (line 482).
- **Both reference trees** print nothing from `git status --porcelain --ignored`.
- **Commits** (`7514d75fe6..00df07113f`, the plan through Task 4, then this report's commit):

  | Commit | Task | Change |
  |---|---|---|
  | `456f26b6e1` | — | the plan |
  | `af738d15bc` | 1 | `run-all.sh` counts every tier's exit status; tiers clear old results before they can fail |
  | `e0752ed3ac` | 2 | `docker-lib.sh`: `dk_stop_immediate` is bounded; a failed `docker run` leaves no container |
  | `8e29932776` | 3 | the capture moves into `ondisk-capture.sh` with a key over its own inputs; `run-ondisk.sh` clears its results first |
  | `00df07113f` | 4 | the capture checks its own coverage before it writes its key |
  | this report's commit | 5 | this report; the spec, CLAUDE.md and the M5 report's errata |

- **How the run was produced.** No Rust changed, so the server binary is Task 1's `build-pgrust.sh` output: cargo finished in 4.31 s (Task 1's report), and `target/fast-profile/postgres` kept that build's mtime, 10:09:06 on 2026-10-19, through every run below. Then each command ran once, in this order, each one's verdict written down before the next began:
  1. the stability loop (Step 1), from 10:50:57: three `run-ondisk.sh capture --force`, each followed by `run-ondisk.sh ref clean crash v081` and `run-ondisk.sh pgrust clean crash v081`. The loop is the plan's, with three evidence-only additions: it copies each capture's key and each run's `summary.tsv` aside, and it would have copied the capture aside and stopped had pgrust failed where `ref` passed. It never fired;
  2. `run-all.sh ref` (343 s), then `run-all.sh pgrust` (497 s). Their `ondisk` tier reused the loop's third capture (a cache hit: neither log has a `capture:` line, and the key stayed `8342dca63fe99622`), so its facts are the final capture's below;
  3. `harness_test.sh`, which made its own fresh capture under `pgvector-work/selftest`;
  4. the reference-tree check.

## Remaining failures
Every remaining failure is a parallel build (M6): the same 8 tests and the same 37 assertions as in M5 (`pgvector-m5-report.md`, "Remaining failures"). I checked each against this run's TAP output (`tap/pgrust/prove/<test>.log`, copied to `~/.cache/pgrust/m5f-evidence/pgrust/tap-prove/`). Every one of the 8 logs shows pgrust's `building index "<index>" on table "tst" serially`.

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
- `045`'s line 27 expects `hnsw graph no longer fits into maintenance_work_mem after 0 tuples`. C's log for this run (`~/.cache/pgrust/m5f-evidence/ref/regress_log_045_hnsw_low_memory_build`) prints `using 1 parallel workers` and then that NOTICE, so the message comes from C's parallel build.

## Reverse
Reported, not gated (`report-{ref,pgrust}.md`, "Reported, not gated: on-disk reverse"; `ondisk/<mode>/reverse.md` has the same rows):

| Check | `ref` | `pgrust` |
|---|---|---|
| clean_boot | ok | ok |
| clean_queries | identical | identical |
| clean_amcheck | clean | clean |
| clean_pgrust_rows | none | pgrust_pin_database pgrust_seal_template pgrust_unpin_database |
| clean_followup | identical | identical |
| crash_boot | ok | ok |
| crash_pages | identical | identical |
| crash_queries | identical | identical |

Both are M5's rows. The three `pg_proc` rows are M5's known divergence (`pgvector-m5-report.md`, "Findings and known divergences").

## What changed
### Task 1: `run-all.sh` counts every tier's exit status (`af738d15bc`)
- **Closes** the M5 final review's "Generalise `run-all.sh`'s exit-status guard to every tier" and M5 Task 6's third minor (`run-all.sh` silently omitted the reverse section when `reverse.tsv` was missing). It also fixes the path that the corrected M5 line names instead (`pgvector-m5-report.md:482`): `run-bytecmp.sh:416` and `run-diff.sh:28` died on a missing binary before they removed `$out`, leaving the previous results.
- **The change.**
  - Every tier runs from one loop. `run-all.sh` removes the tier's old `summary.tsv` first (and `ondisk`'s `reverse.tsv`), then keeps its exit status.
  - A tier that wrote no rows reads `(no results; run-X.sh exited N) | FAIL`, whatever its exit status. A tier whose rows are all `ok` but which exited non-zero gets `(run-X.sh exited N) | FAIL`. A tier with a FAIL row gets no extra row.
  - The reverse section always prints its header, then the table or `(no results: run-ondisk.sh wrote no reverse.tsv; see its output)`.
  - `run-bytecmp.sh` and `run-diff.sh` check `PAGEMASK_BIN` and `DIFFRUNNER_BIN` after `rm -rf "$out"`.
  - `PGV_TIER_DIR`, a self-test knob (plan Decision 5), runs the `run-<tier>.sh` scripts from another directory.
- **Shown by** `test_stale_results` (2 checks: each tier, its binary pointed at nothing, leaves no `summary.tsv`) and `test_run_all_exit_status` (8 checks on stub tiers, among them a tier that exits 0 with no rows, Review Focus 4). Red first: 3 failures (both tiers left their old `summary.tsv`; no `PGV_TIER_DIR` knob). Green: 10 ok.

### Task 2: `docker-lib.sh` fails instead of hanging or leaking (`e0752ed3ac`)
- **Closes** M5 Task 2's `dk_stop_immediate` minor (an unbounded `docker wait` after a swallowed `pg_ctl` failure) and its Created-container minor (a failed `docker run` left the container behind).
- **The change.** `dk_stop_immediate` returns 1 at once when `DK_DATA` is empty, then polls `docker inspect` every 0.5 s for up to 60 s: 0 once the container has stopped, 1 if there is no container or it is still running. `dk_start_new` and `dk_boot_dir` call `dk_rm` when `docker run` fails. `dk_wait_ready` loses its unused loop variable (SC2034).
- **Shown by** `test_docker_lib_failures` (4 checks; the port is held so `docker run` exits 125 and leaves a Created container). Red first: 3 failures (both functions left `pgrust-pgvector-ondisk` behind; `dk_stop_immediate` hung with `DK_DATA` empty) and 1 pass (`docker wait` already fails on a missing container). Green: 4 ok, and `test_docker_lib`'s 12 still ok.

### Task 3: the capture moves into `ondisk-capture.sh`; the key narrows (`8e29932776`)
- **Closes** the M5 final review's "Narrow the capture key to the capture's actual inputs", M5 Task 6's first two minors, and two of M5 Task 4's nits:
  - the header wording of `run-ondisk.sh` and `sql/ondisk-rows.sql`, which said every value makes index scans compute exact distances. Both now say only IVFFlat's query-to-center distances round, and point at the M5 report. The M5 report's note that "the wording waits" (`pgvector-m5-report.md:471`) is done;
  - `$out` cleared only after `ensure_capture` and the `pagemask` check. `run-ondisk.sh` now runs `rm -rf "$out"` right after its argument checks, so a usage error (a misspelt part) still leaves the old results alone (plan Decision 4, Review Focus 5);
  - the unused loop variable `i` in `open_transaction` (SC2034);
  - `capture_key`'s missing psql version. The key now hashes `$PG_TOOLS/bin/psql --version` too; `common.sh` stays out of it by design (plan Decision 2).
- **The change.** `ondisk-capture.sh` (new, sourced after `common.sh` and `docker-lib.sh`) holds `load`, `transcript`, `state`, `waldump`, `open_transaction`, `close_transaction`, `capture_key`, `oracle`, `capture`, `facts` and `ensure_capture`. The key hashes the image pins, the psql version, `sql/ondisk-*.sql`, `sql/repair-verify.sql`, `docker-lib.sh` and `ondisk-capture.sh`. `run-ondisk.sh` keeps the forward gate, the reverse report and main, all outside the key.
- **The move is the old code.** The plan's Step 5 diff of M5's `run-ondisk.sh` lines 57–212 against `ondisk-capture.sh` from `# Run one sql/ file` on shows exactly three hunks, the loop variable (twice) and the key:

  ```
  35c35
  <   local xidfile="$1" i xid=
  ---
  >   local xidfile="$1" xid=
  39c39
  <   for i in $(seq 1 600); do
  ---
  >   for _ in $(seq 1 600); do
  60c60,61
  <     cat "$sql"/ondisk-*.sql "$sql/repair-verify.sql" "$here/docker-lib.sh" "$here/run-ondisk.sh"
  ---
  >     "$PG_TOOLS/bin/psql" --version
  >     cat "$sql"/ondisk-*.sql "$sql/repair-verify.sql" "$ondisk_lib/docker-lib.sh" "$ondisk_lib/ondisk-capture.sh"
  ```

  No function is defined in both files (`comm -12` is empty).
- **A comment in `run-ondisk.sh` no longer forces a recapture.** `test_ondisk_capture_key` (10 checks) shows it on a copy of the scripts: a comment in `run-ondisk.sh` keeps the key; a comment in `ondisk-capture.sh`, `docker-lib.sh`, `sql/ondisk-rows.sql` or `sql/repair-verify.sql` changes it, and so does a fake `psql (PostgreSQL) 18.7` (Review Focus 1); a capture with no key or a stale key is rebuilt, and one with the current key is reused (Review Focus 2).
- **Also shown by** `test_stale_results`' two new checks: `run-ondisk.sh` clears its old results before it dies, and a usage error (`nosuchpart`) leaves them alone. Red first: 2 failures. Green: 14 ok. On a fresh capture (key `b85df49d89537f72`), `run-ondisk.sh ref clean crash v081` passed 7/7 in 137 s (`~/.cache/pgrust/m5f-evidence/task3-ref.log`).

### Task 4: the capture checks itself (`00df07113f`)
- **Closes** the M5 final review's "The capture never checks its own coverage" and M5 Task 4's `close_transaction` and follow-up guard minors.
- **The change.** `capture` runs `capture_check` after `facts` and writes its key only after it, so a capture that fails a check has no key and is rebuilt next time. `capture_check` dies unless:
  - the facts hold: clean and v081 `shut down`, crash `in production`, 42 relations, 25 vector indexes, `wide` TOASTed, v081 0.8.1 → 0.8.7, and the open transaction's Generic records above 0;
  - C made only its intended errors: 2 in each queries transcript and 4 in the update transcript, every one `ERROR:  expected 8 dimensions, not 3`;
  - replay changed every vector index: each of the 25 `generic` main forks in `crash/relfiles.txt` differs between `crash/data` and `crash/replayed` after `pagemask`.

  `capture` also checks for `pagemask` before it deletes the cached capture (Review Focus 3). `close_transaction` kills the open transaction's psql before it `wait`s. `oracle`'s follow-up guard also dies on an empty transcript, on `FATAL:` and on `^psql: error:`, not only on `ERROR:`.
- **Shown by** `test_ondisk_capture_check` (5 checks): it passes on a capture, and dies on a copy with a clean crash, one with an unintended error and one with an unreplayed crash; a capture without `pagemask` keeps the cached one. Red first: 1 failure (`capture_check: command not found`). Green: the 5, with `test_ondisk_capture`'s 10 and `test_ondisk_ref`'s 2, on a fresh self-test capture (key `8342dca63fe99622`) that passed `capture_check`.
- **On the gate's own captures:** all three of the stability loop's fresh captures passed `capture_check` (each exited 0 and wrote its key, which `capture` does only after the check). The final capture, the one `run-all.sh` used, has key `8342dca63fe99622`, the same as the self-test capture's and as the planning draft's, so the capture inputs are byte-identical to the draft the plan verified.

## Stability
The plan's loop (Step 1), with its timestamps. A failing capture would have been copied to `~/.cache/pgrust/m5f-capture-fail-<n>` before the next iteration replaced it; no such directory exists.

```
10:50:57
capture 1: exit 0
run 1 ref: exit 0, 7/7
run 1 pgrust: exit 0, 7/7
10:54:27
capture 2: exit 0
run 2 ref: exit 0, 7/7
run 2 pgrust: exit 0, 7/7
10:57:51
capture 3: exit 0
run 3 ref: exit 0, 7/7
run 3 pgrust: exit 0, 7/7
11:01:21
```

- **Time.** Each iteration took 3 min 24 s to 3 min 30 s: the capture, `capture_check` included, 96–100 s, then `ref` 41–46 s and `pgrust` 63–68 s (the evidence files' times). The loop took 10 min 24 s.
- **Keys.** All three captures have key `8342dca63fe99622`: the inputs did not change between them.
- **Facts** (`stability/facts-{1,2,3}.tsv`; the third is the final capture's `facts.tsv`, byte for byte):

  | Fact | What it counts | Capture 1 | Capture 2 | Capture 3 |
  |---|---|---|---|---|
  | `crash_generic_records` | `pg_waldump --rmgr=Generic` records after the crash's redo point | 545,139 | 546,705 | 546,351 |
  | `crash_generic_fpi` | of those, records with full-page images | 2,619 | 2,623 | 2,626 |
  | `crash_inflight_generic` | Generic records of the open transaction's xid | 75,727 | 75,738 | 75,583 |
  | `crash_heap_records` | Heap records after the redo point | 3,257 | 3,257 | 3,257 |
  | `crash_btree_records` | Btree records after the redo point | 2,552 | 2,552 | 2,552 |
  | `crash_relations` | fixture relations whose pages are compared | 42 | 42 | 42 |
  | `vector_indexes` | distinct vector indexes the clean queries scan | 25 | 25 | 25 |

  All three also read `shut down` for clean and v081, `in production` for crash, `wide_toasted t`, and v081 `0.8.1` → `0.8.7`. As in M5, only the Generic counts vary, which fits each capture's new HNSW graphs; M5's three captures read 545,053–546,200 Generic records, 2,624–2,636 with full-page images, and 75,327–75,698 from the open transaction.
- **Replay margins.** `capture_check` requires that replay change every vector index's main fork. To see how far each capture was from that edge, each one was measured read-only while its `ref` and `pgrust` runs went on (`stability/margins.sh`, `margins-{1,2,3}.tsv`): per vector index, the 8 KB blocks that differ after `pagemask` between `crash/data` and `crash/replayed`.
  - All 25 differ in every capture.
  - The smallest counts are the unlogged table's. Replay resets `ut_u_l2` from 23 blocks to its 2-block init fork, so both remaining blocks differ, in every capture (the "2 pages" Task 4's review saw). `ut_u_ip` has 6 blocks after replay, all 6 different.
  - Among the logged indexes, the smallest are `ct_cb_ham` and `ct_gh_l2`, 12 of 22 blocks in every capture.

## Decisions made during execution
- **No plan-defined stop occurred.** No capture failed (`capture_check` or otherwise), no forward row failed in any run in either mode, and this task edited no capture input, script, Rust, core or fixture file. `docs/fuzzing/rulings.toml` is untouched. No user decision was needed.
- **The controller's one code ruling** (Task 3): `ondisk_ensure` in `harness_test.sh` gained `# shellcheck disable=SC2034 # ensure_capture reads it` above its `cap="$2"`. At Task 3's commit shellcheck reported 20 findings in the harness, not the plan's 19, because it could not see the sourced `ondisk-capture.sh` read `cap`. Task 4's `. "$here/../ondisk-capture.sh"` makes the directive redundant (the end state lints at 19 without it), but it keeps "19 findings" true at every commit. It is a test-file comment, not a capture input.

## Still open
**Closed here, from the M5 report's "Still open"** (by task above):
- the final review's three follow-ups: the capture's self-checks (Task 4), the narrower key (Task 3) and the exit-status guard for every tier (Task 1);
- M5 Task 6's three minors: the header wording (Task 3), `$out` cleared first (Task 3) and the silently omitted reverse section (Task 1);
- M5 Task 2's `dk_stop_immediate` and Created-container minors (Task 2);
- M5 Task 4's `close_transaction` and follow-up guard items (Task 4), and its SC2034 and `capture_key` nits (Task 3).
- One more M5 Task 4 minor is closed in effect, though no task targeted it: `[ -s ]` accepted a `relfiles.txt` of blank lines only (`run-ondisk.sh:169` at M5). That check is unchanged in `capture`, but `capture_check` now requires `crash_relations` (`grep -c .`, non-blank lines) to be 42 and 25 `generic` rows, so such a capture fails.

**Still open from the M5 report** (details, and line numbers at `436bd1b16f`, there):
- the two deferred core items (plan Decision 8 of M5): panic-payload decoding in lmgr_proc, ipc and mcxt_stats, and guc's `%g` check-hook fallback;
- M5 Task 1's four minors (the repair generator's test and its comments, and pre-existing output noise);
- M5 Task 2's other four: `cluster_state` and other unguarded lines that abort `test_docker_lib` instead of recording a FAIL, `dk_stop_fast`'s unredirected stderr, the "host directory untouched" check comparing `pg_controldata` only, and the isolation comment that omits `port_for`'s fixed ports;
- M5 Task 3's four: the inner-product IVFFlat premise (measured by the stability runs), "every index returns full rows", and the update and relation-list checks in `test_ondisk_sql`, which still count without checking text or the heap/btree/generic split. (`capture_check` now checks both on C's capture: the errors' text and the 25 `generic` rows.)
- M5 Task 4's other five: a `pagemask` failure on a subject file dies instead of recording `crash_pages` FAIL; a boot-FAIL row names a server log the next start truncates; length-only differences report "block 0, offset 0"; the negative control changes only the heap; and `compare_pages`' local `rm` variable;
- M5 Task 5's ten reverse-side minors;
- the M4 follow-ups ledger's parked wording item (the M4 report's "lists every load point").

**The M4 follow-ups report's open items** stand (`pgvector-m4-followups-report.md`, "Still open"); this plan closed none of them.

**This plan's deferred review minors**, by task (from the run's ledger, `~/.cache/pgrust/m5f-sdd-ledger.md`):
- Task 1:
  - `stale_results_check` does not assert that the tier died at its binary check (`missing /nonexistent` in the log), so a later reorder could start real servers and still pass.
  - `test_run_all_exit_status` never plants or writes a `reverse.tsv`, so `run-all.sh`'s `reverse.tsv` removal and its table path are untested (correct by reading).
  - `test_run_all_exit_status` leaves `$PGV_WORK/run-all/` and its logs in `selftest/` (the existing log convention).
- Task 2:
  - The absent-container check inherits `DK_DATA`; run alone after a failed start, it can pass through the empty-`DK_DATA` branch instead of the `docker inspect` branch it names.
  - `dk_stop_immediate`'s stale-`DK_DATA` timeout path (60 s) is never exercised, and the bound has no override.
  - Its "60 s" message understates the real wait (120 × (inspect + 0.5 s), about 66–80 s).
  - It sends `pg_ctl`'s stderr to `/dev/null`, so a timeout does not say why; `docker kill -s QUIT` (postgres is PID 1) would drop the `DK_DATA` dependency.
  - The harness's `within` kills only its direct child (in red, an orphaned `docker wait` outlived it until `dk_rm`).
- Task 3:
  - The `nosuchpart` check runs with the real `PAGEMASK_BIN` and does not grep for `usage:`, so a regressed part check would start a real Docker capture from a non-Docker test (plan-mandated).
  - `INFLIGHT_PID`, which the library owns, is reaped only by `run-ondisk.sh`'s cleanup trap; another script that sources the library and calls `capture` must copy that trap.
- Task 4:
  - The no-`pagemask` case does not check why the run failed; with Docker down it passes through `dk_require` without reaching the `pagemask` check (plan-mandated).
  - A failing negative case's "see `$1.out`" names a file the next case overwrites and the cleanup deletes (plan-mandated).
  - No negative case covers the relation count, the open-transaction check or the `pagemask`-failure branch; the facts loop is tested only through `crash_state`.
  - `$nopm.log` stays in `$PGV_WORK` on a pass, and `check-*.masked` stays in `$cap` on every die path (the next capture's `rm -rf` clears them).
- Several of these checks do not assert why the run failed (Task 1's first, Task 2's first, Task 3's first, Task 4's first).
- Fixing those that sit in a capture input means a recapture and a stability rerun: Task 2's `dk_stop_immediate` items (`docker-lib.sh`), Task 3's library cleanup and Task 4's `check-*.masked` leftovers (`ondisk-capture.sh`). The rest are in `harness_test.sh`.

## Evidence
Under `~/.cache/pgrust/m5f-evidence/`:
- `stability/`: the loop's script (`loop.sh`) and output (`loop-output.txt`), each iteration's log (`run-{1,2,3}.log`), each capture's `facts-`, `key-` and `margins-` files, each run's `summary-<i>-<mode>.tsv`, and `margins.sh`;
- `gates.sh` and `gates-output.txt` (Step 2's commands and verdicts), `run-all-{ref,pgrust}.log` and `report-{ref,pgrust}.md`;
- `ref/` and `pgrust/`: every tier's `summary.tsv` (as `summary-<tier>.tsv`), `reverse.tsv` and `reverse.md`, the TAP prove logs, and C's TAP `045` log;
- `harness_test.log`;
- `task3-ref.log`: Task 3's Step 8 run on its fresh capture;
- `sdd-reports/`: the run's task reports, with their red and green evidence.

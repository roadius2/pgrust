# pgvector M5 follow-ups: a self-checking capture, a narrower capture key and exit-status-aware gates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the M5 report's capture-key follow-ups in one change, at the cost of one recapture and one stability rerun:
- the capture checks its own coverage;
- its cache key covers its own inputs and nothing else;
- no run that dies leaves an earlier `summary.tsv` behind.

Also make `run-all.sh` count every tier's exit status, fold in the open minors that sit in the capture's inputs, and fix the M5 report's three errata.

**Architecture:**
- **The capture moves into its own sourced file, `scripts/pgvector/ondisk-capture.sh`,** with everything it runs:
  - the helpers: `load`, `transcript`, `state`, `waldump`, `open_transaction` and `close_transaction`;
  - the capture itself: `oracle`, `capture`, `facts`, `capture_key` and `ensure_capture`.

  The key hashes that file, `docker-lib.sh`, `sql/ondisk-*.sql`, `sql/repair-verify.sql`, the two image pins and `$PG_TOOLS`'s psql version. `run-ondisk.sh` keeps the forward gate, the reverse report and its main, all outside the key, so editing them no longer forces a recapture.
- **The capture checks itself** (`capture_check`, Task 4) before it writes its key. It dies unless:
  - **the facts hold:** clean and v081 `shut down`, crash `in production`, 42 relations, 25 vector indexes, the open transaction's Generic records above 0, `wide` TOASTed, and 0.8.1 → 0.8.7;
  - **C made only its intended errors:** 2 in each queries transcript and 4 in the update, every one `expected 8 dimensions, not 3`;
  - **replay changed every vector index:** every `generic` main fork differs between `crash/data` and `crash/replayed` after `pagemask`.

  A capture that fails a check has no key, so the next run rebuilds it.
- **A tier clears its old results before anything that can fail** after its argument checks: `run-ondisk.sh` (pagemask, the capture), `run-bytecmp.sh` (pagemask) and `run-diff.sh` (diffrunner).
- **`run-all.sh` counts every tier's exit status.**
  - It removes each tier's old `summary.tsv` before running that tier.
  - A tier that exits non-zero after writing only `ok` rows gets a FAIL row of its own, and so does a tier that writes no rows.
  - A missing `reverse.tsv` is reported, not skipped.
- **No Rust, core or fixture change.** The only SQL edit is a header comment in `sql/ondisk-rows.sql`.

**Decisions this plan makes (for review):**
1. **Scope: the capture-key bundle plus every open minor in a capture input.** Any edit to a capture input forces a recapture and a stability rerun, so this plan makes all of them now:
   - **the final review's three follow-ups:** the self-checks, the narrower key and the exit-status guard for every tier;
   - **M5 Task 6's three minors:** the header wording, `$out` cleared first, and the reverse section that `run-all.sh` drops silently;
   - **M5 Task 2's two `docker-lib.sh` minors:** `dk_stop_immediate`'s unbounded `docker wait`, and the Created container a failed `docker run` leaves;
   - **M5 Task 4's capture-side minors:**
     - `close_transaction`'s `wait` with no kill;
     - the follow-up guard, which passes an empty transcript or a `psql: error:` line;
     - the unused loop variables (SC2034);
     - the key's missing psql version.
   - **Out of scope:** the other minors (the forward and reverse code in `run-ondisk.sh`, self-test wording, M5 Task 1's Rust test, the two core items). After Task 3 none of them is in a capture input, so fixing them later costs no recapture. They stay in the M5 report's "Still open".
2. **The key leaves out `common.sh`**, against the M5 Task 4 nit that suggested it.
   - Every tier shares `common.sh`. The capture uses only `die`, paths, ports and the image pins from it, and the key already hashes the pins by value. Hashing the whole file would force recaptures for unrelated edits.
   - The key adds `psql --version` instead: both sides' transcripts come from `$PG_TOOLS`'s psql, so a different version must not reuse a capture.
3. **The self-checks are coverage invariants, not volume thresholds.**
   - Checked: the exact counts (42 relations, 25 indexes, errors 2/2/4, 25 changed forks), and the open transaction's records above 0.
   - Not checked: the volume thresholds (≥100,000 Generic records and so on). They stay in `test_ondisk_capture`.
4. **Argument checks stay before the clear.** A usage error, such as a typo in a part name, touches nothing. Missing binaries and failed captures come after the clear.
5. **`run-all.sh` gets a self-test knob, `PGV_TIER_DIR`.** It runs `run-<tier>.sh` from another directory, so the gate logic is tested on stub tiers in seconds instead of 14 minutes of real tiers. The precedent is `PGV_ONDISK_PERTURB_SQL`, an environment variable only the self-tests set.
6. **One stability rerun, after the last capture-input edit.** Task 5 runs three fresh captures, each followed by `ref` and `pgrust`, and checks every exit status. Then come the full gates. Editing a capture input after the loop starts is a plan-defined stop.
7. **The record.**
   - A new short report, `docs/superpowers/reports/pgvector-m5-followups-report.md`, follows the M4 follow-ups' convention.
   - The M5 report's three errata are fixed in place.
   - Its "Still open" stays a snapshot; the new report says which of its items closed.
8. **The expected gate totals are unchanged:** C 137/137, pgrust 129/137. No tier gains or loses rows when every tier exits as it should.

**Tech Stack:** bash 3.2 harness scripts, shellcheck 0.11.0, perl (already a TAP dependency; one test uses it to hold a port), Docker (the two digest-pinned pgvector images), the PostgreSQL 18.6 + pgvector 0.8.7 C reference (`pgvec`), `crates/bin/pagemask`.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`. Binding sections:
- §8.2, the "On-disk, forward" row and its pass rule;
- §8.3, the harness;
- §9, M5's exit criteria, which must still hold.

Inputs:
- `docs/HANDOFF_2026-10-15.md` §2;
- `docs/superpowers/reports/pgvector-m5-report.md`, "Still open";
- `~/.cache/pgrust/m5-sdd-ledger.md`.

**Facts verified while writing this plan** (2026-10-15, `main` = `7514d75fe6`, macOS arm64, Docker 29.1.3 linux/aarch64):
- **The current capture** (key `eaf2dac728f0242d`, from M5's stability loop):
  - All 25 `generic` main forks differ between `crash/data` and `crash/replayed` after `pagemask`.
  - C's transcripts hold exactly the intended errors, every one `ERROR:  expected 8 dimensions, not 3`: 2 in each of `clean/` and `crash/expected-queries.out`, and 4 in `v081/expected-update.out`. The three follow-ups have none.
  - `crash/relfiles.txt` lists 9 heap, 8 btree and 25 generic relations.
- **A full draft of this plan's code** ran in a scratch copy of `scripts/pgvector`. The code blocks below are that draft.
  - **Lint:** `/bin/bash -n` (3.2) and `shellcheck -x` are clean on every changed script. `harness_test.sh` keeps its 19 existing findings and gains none.
  - **The new self-tests:** 31 checks, all green on the draft, in 7 s.
  - **Red first:** on the current scripts they fail on the defects they target (counts per task in each task's red step).
  - **`capture_check`** passes on the current capture, in 1.8 s.
  - **A fresh capture made with the draft** took 95 s and passed `capture_check`. Its key is `8342dca63fe99622`, and its facts are:
    - 545,975 Generic records after the redo point, 2,621 of them with full-page images;
    - 75,405 Generic records from the open transaction;
    - 42 relations and 25 vector indexes.
  - **On that capture:**
    - `run-ondisk.sh ref`: 7/7 forward, and every reverse row `ok`, `identical`, `clean` or `none` (101 s);
    - `run-ondisk.sh pgrust`: 7/7 forward, and every reverse row `ok`, `identical` or `clean`, with `clean_pgrust_rows` listing the three known `pg_proc` rows (142 s).
- **Docker behaviour the tests rely on:**
  - With 127.0.0.1:55493 held by another process, `docker run -p 127.0.0.1:55493:5432` exits 125 and leaves the container in Created.
  - `docker inspect` of a missing container exits 1.
  - After a container that served connections is removed, those connections sit in TIME_WAIT on 127.0.0.1:55493 for about 30 s. A plain bind fails during that time, but one with `SO_REUSEADDR` succeeds, and Docker still refuses a port held that way.
- **`run-repair-check.sh` sources only `common.sh` and reads `sql/repair-verify.sql`.** This plan changes neither, so it is not rerun.

## Global Constraints

- **Branch:** work on `vector/m5-followups`, cut from `main` (`7514d75fe6`). Merge locally with a fast-forward when done; push only when the user asks. Work in the main checkout.
- **Commit trailer:** every commit ends with a `Co-Authored-By:` trailer naming the model that wrote it: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` or `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. `git status --porcelain --ignored` on both must stay empty.
- **No Rust, core or fixture edit.** Fixture means the data, queries and settings in `sql/ondisk-*.sql`; the only SQL edit allowed is Task 3's header comment in `sql/ondisk-rows.sql`. Anything else is a plan-defined stop.
- **Never widen `docs/fuzzing/rulings.toml`.** Never edit upstream expected outputs.
- **Harness scripts:**
  - Use `#!/usr/bin/env bash` and `set -euo pipefail`; a sourced library has `# shellcheck shell=bash` instead.
  - Stay bash 3.2 compatible: no associative arrays, `${x,,}`, `mapfile`, `sed -i`, GNU-only flags, or `case` inside `$( )`.
  - Check each changed script with `/bin/bash -n` (macOS's 3.2).
  - Run `shellcheck -x` from the script's own directory (it resolves `. "$here/…"` from there). Every changed script must be clean, except `tests/harness_test.sh`, whose findings must stay its existing 19 (3 SC1007, 13 SC2015, 2 SC2016, 1 SC2046).
- **Docker:**
  - Use only the two digest-pinned images.
  - Touch only the container `pgrust-pgvector-ondisk`, and remove containers with `docker rm -f -v`.
  - Never `docker system prune`, never remove images, and never pull other tags.
  - Port 55493 is shared with `docker-ref.sh`; never run both at once.
- **Never run two `run-ondisk.sh` invocations at once,** and never run a self-test that calls it while one is running. Every invocation's exit trap removes the shared container.
- **Ports:** pgrust 55491, C reference 55492, Docker 55493.
- **Verdicts:** read each tier's from `~/.cache/pgrust/pgvector-work/<tier>/<mode>/summary.tsv` before a later run overwrites it. Leave no servers or containers running.
- **Capture inputs** are `ondisk-capture.sh`, `docker-lib.sh`, `sql/ondisk-*.sql` and `sql/repair-verify.sql`. Once Task 5's stability loop starts, editing any of them is a plan-defined stop.
- **Server build:** Task 1 runs `scripts/pgvector/build-pgrust.sh` once. It should rebuild nothing, since the branch changes no Rust. Every task uses `target/fast-profile/{postgres,pagemask,diffrunner}`.
- **Disk:** a capture is about 450 MB; the self-tests keep a second one under `pgvector-work/selftest`. Disk was 510 GiB free at planning time.

## Review Focus

The five failure modes most likely to bite someone running the gate that no task's main tests exercise. Each one's test is in the owning task.

1. **A rebuilt `$PG_TOOLS` with another psql version, under a cached capture.** The capture's transcripts and the subject's must come from the same psql, so the key must change and the capture be rebuilt.
   - Test: `test_ondisk_capture_key`'s fake `psql (PostgreSQL) 18.7` (Task 3).
2. **A capture that was interrupted or failed a check.** It has no key (or a stale one), so the next run must rebuild it, never reuse it. A matching key must still be reused, or every run would recapture.
   - Test: `test_ondisk_capture_key`'s three `ensure_capture` cases (Task 3).
3. **A capture that cannot check itself (no `pagemask`).** It must fail before deleting the cached capture, which a later run can still use.
   - Test: `test_ondisk_capture_check`'s last check (Task 4).
4. **A tier that exits 0 but writes no rows,** for example because its output parser stopped matching. It must fail the report, not vanish from it.
   - Test: `test_run_all_exit_status`'s `iterscan` stub (Task 1).
5. **A usage error,** such as a typo in a part name. It must leave the previous results alone; only failures after the argument checks clear them.
   - Test: `test_stale_results`'s `nosuchpart` check (Task 3).

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `scripts/pgvector/run-all.sh` | every tier's exit status; old results removed first; the reverse note; `PGV_TIER_DIR` | 1 |
| `scripts/pgvector/run-bytecmp.sh`, `run-diff.sh` | clear `$out` before the binary check | 1 |
| `scripts/pgvector/docker-lib.sh` | `dk_stop_immediate` bounded; no Created leftovers; SC2034 | 2 |
| `scripts/pgvector/ondisk-capture.sh` (new) | the capture, its helpers, its key (Task 3) and its self-checks (Task 4) | 3, 4 |
| `scripts/pgvector/run-ondisk.sh` | the forward gate, the reverse report, main; sources `ondisk-capture.sh`; clears `$out` first | 3 |
| `scripts/pgvector/sql/ondisk-rows.sql` | header wording only | 3 |
| `scripts/pgvector/tests/harness_test.sh` | the new self-tests, appended above `# --- runner ---` in task order | 1–4 |
| spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m5-report.md` (errata), `docs/superpowers/reports/pgvector-m5-followups-report.md` (new) | docs and the record | 5 |

## Execution notes

- **Task order:** 1 to 5.
  - Tasks 1 and 2 are independent.
  - Task 3 extends Task 1's `test_stale_results`.
  - Task 4 extends Task 3's `ondisk-capture.sh`.
  - Task 5 measures everything.
- **Suggested implementers:**
  - Sonnet: Tasks 1–4 (the code is spelled out).
  - Opus: Task 5 (the measurement, the docs and the report).
  - Opus does every review and the final review.
- **Time:**
  - The new self-tests take about 7 s together.
  - A capture takes about 95 s. Every task from 2 on changes a capture input, so the next on-disk run recaptures.
  - `run-ondisk.sh ref clean crash v081` takes about 70 s after its capture.
  - Task 5:
    - the stability loop: about 10 minutes;
    - `run-all.sh ref`: about 350 s;
    - `run-all.sh pgrust`: about 490 s;
    - the full self-test suite: about 7 minutes.
- **Plan-defined stops**, in addition to subagent-driven-development's four:
  - Any Rust, core or fixture edit.
  - **A forward row that fails for pgrust while `ref` passes on the same capture.** This is a real pgrust divergence.
    - Keep the capture: `cp -R ~/.cache/pgrust/pgvector-work/ondisk/capture ~/.cache/pgrust/m5f-capture-fail-<n>`.
    - Report the first differing statement to the user (AskUserQuestion).
  - **`capture_check` failing on a fresh capture.** The fixture no longer covers what the gate claims.
    - Keep the capture as above and report the message.
    - Never loosen a check without the user's ruling.
  - **Editing a capture input after Task 5's stability loop starts.**
  - Never widen `rulings.toml`.
- **Expected totals after Task 5:**
  - `run-all.sh ref`: 137/137 (regress 14, TAP 48, bytecmp 28, iterscan 19, diff 21, ondisk 7). Every reverse row is `ok`, `identical`, `clean` or `none`.
  - `run-all.sh pgrust`: 129/137. The 8 TAP parallel-build failures (M6) remain, and the reverse section is as in M5.
  - `harness_test.sh`: all passed, 192 ok (M5's 161 plus this plan's 31).

---

### Task 1: `run-all.sh` counts every tier's exit status, and tiers clear their results before they can fail

The final review's "generalise `run-all.sh`'s exit-status guard to every tier", plus M5 Task 6's silent reverse section.
- `run-all.sh:12-16` runs every tier but `ondisk` with `|| true`. A tier that dies after writing `ok` rows, such as `run-bytecmp.sh:430` or a `pg_regress` that stops partway, counts as passing on the rows it wrote.
- `run-bytecmp.sh:416` and `run-diff.sh:28` check for their binary before they remove `$out`. A direct run without it leaves the previous `summary.tsv` in place.

**Files:**
- Modify: `scripts/pgvector/run-all.sh` (whole file)
- Modify: `scripts/pgvector/run-bytecmp.sh:416-419`, `scripts/pgvector/run-diff.sh:28-31`
- Test: `scripts/pgvector/tests/harness_test.sh` (insert above `# --- runner ---`)

**Interfaces:**
- Produces:
  - `run-all.sh`'s `PGV_TIER_DIR`: a directory to run `run-<tier>.sh` from (self-test only);
  - `stale_results_check <tier> <binary variable>`;
  - `test_stale_results`, which Task 3 extends;
  - `stub_tier <dir> <tier> <exit status> "<name:result ...>"`.

- [ ] **Step 1: Branch, commit the plan, check the build**

```bash
git switch -c vector/m5-followups main
git add docs/superpowers/plans/2026-10-15-pgvector-m5-followups.md
git commit -q -m "docs(pgvector): M5 follow-ups plan (capture key, self-checks, exit statuses)" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
scripts/pgvector/build-pgrust.sh
```
(The controller may already have done the first three commands. Skip them if `git log -1 --format=%s` shows the plan commit.)

Expected: `build-pgrust.sh` ends with `built …/target/fast-profile/postgres (postgres (PostgreSQL) 18.6; staged vector extension 0.8.7)` and recompiles nothing.

- [ ] **Step 2: Write the failing tests**

Insert above the line `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# Every tier clears its old results before anything that can fail after
# its argument checks. With its binary missing, a direct run must exit
# non-zero and leave no summary.tsv behind: a stale all-ok file reads as a
# pass to anything that reads it without the exit status. $1 tier; $2 the
# binary's variable, pointed at nothing.
stale_results_check() {
  local w="$PGV_WORK/stale"
  mkdir -p "$w/$1/ref"
  printf 'old\tok\n' >"$w/$1/ref/summary.tsv"
  if env PGV_WORK="$w" "$2=/nonexistent" "$here/../run-$1.sh" ref >"$w/$1.log" 2>&1; then
    fail "stale results: run-$1.sh exits 0 without $2"
  elif [ -e "$w/$1/ref/summary.tsv" ]; then
    fail "stale results: run-$1.sh left its old summary.tsv (see $w/$1.log)"
  else
    pass "stale results: run-$1.sh clears its old results before it dies"
  fi
}

test_stale_results() {
  local w="$PGV_WORK/stale"
  rm -rf "$w"
  stale_results_check bytecmp PAGEMASK_BIN
  stale_results_check diff DIFFRUNNER_BIN
  rm -rf "$w"
}

# A stub tier for test_run_all_exit_status: $1/run-$2.sh writes the rows $4
# (space-separated name:result pairs; "" writes no summary.tsv) and exits $3.
stub_tier() {
  {
    echo '#!/usr/bin/env bash'
    echo 'set -eu'
    if [ -n "$4" ]; then
      echo "mkdir -p \"\$PGV_WORK/$2/\$1\""
      printf '%s\n' "for r in $4; do printf '%s\\t%s\\n' \"\${r%%:*}\" \"\${r#*:}\"; done >\"\$PGV_WORK/$2/\$1/summary.tsv\""
    fi
    echo "exit $3"
  } >"$1/run-$2.sh"
  chmod +x "$1/run-$2.sh"
}

# run-all.sh counts every tier's exit status: a tier that died after writing
# only ok rows fails, a tier that wrote nothing reports "(no results)" even
# over an old summary.tsv or with exit 0, a tier with a FAIL row gets no
# extra row, and a missing reverse.tsv is reported. Stub tiers
# (PGV_TIER_DIR): no server runs.
test_run_all_exit_status() {
  local w="$PGV_WORK/run-all" r t
  r="$w/report-ref.md"
  # Without the knob, run-all.sh would run the real tiers.
  if ! grep -q PGV_TIER_DIR "$here/../run-all.sh"; then
    fail "run-all: no PGV_TIER_DIR knob"
    return
  fi
  rm -rf "$w"
  mkdir -p "$w/bin" "$w/bytecmp/ref"
  stub_tier "$w/bin" regress 1 "a:ok b:ok"
  stub_tier "$w/bin" tap 1 "t:FAIL"
  stub_tier "$w/bin" bytecmp 1 ""
  printf 'old\tok\n' >"$w/bytecmp/ref/summary.tsv"
  stub_tier "$w/bin" iterscan 0 ""
  stub_tier "$w/bin" diff 0 "d:ok"
  stub_tier "$w/bin" ondisk 0 "o:ok"
  if PGV_WORK="$w" PGV_TIER_DIR="$w/bin" "$here/../run-all.sh" ref >"$w.log" 2>&1; then
    fail "run-all: exits 0 with failing tiers"
  else
    pass "run-all: exits non-zero with failing tiers"
  fi
  if grep -qxF '| regress | (run-regress.sh exited 1) | FAIL |' "$r"; then
    pass "run-all: a tier that died after ok rows fails"
  else
    fail "run-all: no exit-status row for regress (see $r)"
  fi
  if grep -qxF '| bytecmp | (no results; run-bytecmp.sh exited 1) | FAIL |' "$r" && ! grep -qF '| bytecmp | old |' "$r"; then
    pass "run-all: a tier with no results fails, its old rows dropped"
  else
    fail "run-all: bytecmp's rows (see $r)"
  fi
  if ! grep -qF '| tap | (run-tap.sh' "$r"; then
    pass "run-all: a tier with a FAIL row gets no extra row"
  else
    fail "run-all: an extra row for tap (see $r)"
  fi
  if grep -qxF '| iterscan | (no results; run-iterscan.sh exited 0) | FAIL |' "$r"; then
    pass "run-all: a tier that exits 0 with no rows fails"
  else
    fail "run-all: iterscan's empty run (see $r)"
  fi
  if grep -qxF '**4/8 passed**' "$r"; then pass "run-all: 4/8 passed"; else fail "run-all: the totals (see $r)"; fi
  if grep -qF '(no results: run-ondisk.sh wrote no reverse.tsv' "$r"; then
    pass "run-all: a missing reverse.tsv is reported"
  else
    fail "run-all: the reverse section (see $r)"
  fi
  for t in regress tap bytecmp iterscan diff ondisk; do stub_tier "$w/bin" "$t" 0 "x:ok"; done
  if PGV_WORK="$w" PGV_TIER_DIR="$w/bin" "$here/../run-all.sh" ref >"$w.log.2" 2>&1 && grep -qxF '**6/6 passed**' "$r"; then
    pass "run-all: all-ok tiers pass"
  else
    fail "run-all: all-ok tiers (see $w.log.2)"
  fi
}
```

- [ ] **Step 3: Run them to verify they fail**

Run: `scripts/pgvector/tests/harness_test.sh test_stale_results test_run_all_exit_status`
Expected: `3 failure(s)`:
```
FAIL - stale results: run-bytecmp.sh left its old summary.tsv (see …)
FAIL - stale results: run-diff.sh left its old summary.tsv (see …)
FAIL - run-all: no PGV_TIER_DIR knob
```
(The knob guard stops `test_run_all_exit_status` before it could run the real tiers.)

- [ ] **Step 4: Replace `scripts/pgvector/run-all.sh`**

```bash
#!/usr/bin/env bash
# Run every pgvector conformance tier built so far and print a pass/fail report.
# usage: run-all.sh {pgrust|ref}
# Each tier's old summary.tsv is removed before it runs, so a tier that dies
# before writing one reports "(no results)". A tier that exits non-zero
# after writing only ok rows (it died partway) gets a FAIL row of its own.
# PGV_TIER_DIR (harness self-test only) runs the run-<tier>.sh scripts from
# another directory.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

[ "$#" -eq 1 ] || die "usage: run-all.sh {pgrust|ref}"
mode="$1"
port_for "$mode" >/dev/null

tiers=(regress tap bytecmp iterscan diff ondisk)
rcs=()
for tier in "${tiers[@]}"; do
  rm -f "$PGV_WORK/$tier/$mode/summary.tsv"
  if [ "$tier" = ondisk ]; then rm -f "$PGV_WORK/ondisk/$mode/reverse.tsv"; fi
  rc=0
  "${PGV_TIER_DIR:-$here}/run-$tier.sh" "$mode" || rc=$?
  rcs+=("$rc")
done

report="$PGV_WORK/report-$mode.md"
total=0
passed=0
{
  printf '# pgvector conformance: %s\n\n' "$mode"
  printf '| Tier | Test | Result |\n|---|---|---|\n'
  i=0
  for tier in "${tiers[@]}"; do
    rc="${rcs[$i]}"
    i=$((i + 1))
    f="$PGV_WORK/$tier/$mode/summary.tsv"
    rows=0
    ok=0
    if [ -s "$f" ]; then
      while IFS=$'\t' read -r name result; do
        rows=$((rows + 1))
        [ "$result" = ok ] && ok=$((ok + 1))
        printf '| %s | %s | %s |\n' "$tier" "$name" "$result"
      done <"$f"
    fi
    total=$((total + rows))
    passed=$((passed + ok))
    if [ "$rows" -eq 0 ]; then
      printf '| %s | (no results; run-%s.sh exited %d) | FAIL |\n' "$tier" "$tier" "$rc"
      total=$((total + 1))
    elif [ "$rc" -ne 0 ] && [ "$ok" -eq "$rows" ]; then
      printf '| %s | (run-%s.sh exited %d) | FAIL |\n' "$tier" "$tier" "$rc"
      total=$((total + 1))
    fi
  done
  printf '\n**%d/%d passed**\n' "$passed" "$total"
  # On-disk, reverse (spec §8.2): reported, never counted.
  printf '\n## Reported, not gated: on-disk reverse\n\n'
  if [ -s "$PGV_WORK/ondisk/$mode/reverse.tsv" ]; then
    printf '| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name result; do
      printf '| %s | %s |\n' "$name" "$result"
    done <"$PGV_WORK/ondisk/$mode/reverse.tsv"
  else
    printf '(no results: run-ondisk.sh wrote no reverse.tsv; see its output)\n'
  fi
} >"$report"
cat "$report"
[ "$passed" -eq "$total" ]
```

The report's bytes are unchanged whenever every tier exits as it should. The reverse section prints the same header, then either the table or one note.

- [ ] **Step 5: Clear results before the binary checks**

In `scripts/pgvector/run-bytecmp.sh`, replace:

```bash
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/bytecmp/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
```
with:
```bash
out="$PGV_WORK/bytecmp/$mode"
rm -rf "$out"
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
mkdir -p "$out/diff"
```

In `scripts/pgvector/run-diff.sh`, replace:

```bash
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/diff/$mode"
rm -rf "$out"
mkdir -p "$out"
```
with:
```bash
out="$PGV_WORK/diff/$mode"
rm -rf "$out"
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN; run scripts/pgvector/build-pgrust.sh"
mkdir -p "$out"
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `scripts/pgvector/tests/harness_test.sh test_stale_results test_run_all_exit_status`
Expected: 10 `ok` lines and `all passed`, in a few seconds; no server starts.

- [ ] **Step 7: Lint**

```bash
for f in run-all.sh run-bytecmp.sh run-diff.sh tests/harness_test.sh; do /bin/bash -n "scripts/pgvector/$f" || echo "SYNTAX: $f"; done
(cd scripts/pgvector && shellcheck -x run-all.sh run-bytecmp.sh run-diff.sh)
(cd scripts/pgvector/tests && shellcheck -x -f gcc harness_test.sh | wc -l)
```
Expected: no `SYNTAX` lines, no shellcheck output for the three tier scripts, and `19` for the harness.

- [ ] **Step 8: Commit**

```bash
git add scripts/pgvector/run-all.sh scripts/pgvector/run-bytecmp.sh scripts/pgvector/run-diff.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "fix(pgvector): run-all.sh counts every tier's exit status; tiers clear old results before they can fail" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: `docker-lib.sh` fails instead of hanging or leaking

M5 Task 2's two `docker-lib.sh` minors, and the SC2034 nit in the same file.
- `dk_stop_immediate` swallows `pg_ctl`'s failure and then runs `docker wait` with no bound. With `DK_DATA` empty or stale, the container keeps running and the wait never returns.
- A `docker run` that fails after creating its container, for example because the port is held, leaves the container in Created. The callers remove it, but the function should do that itself.

`docker-lib.sh` is a capture input, so from this task on the next on-disk run recaptures.

**Files:**
- Modify: `scripts/pgvector/docker-lib.sh` (`dk_wait_ready`, `dk_start_new`, `dk_boot_dir`, `dk_stop_immediate`)
- Test: `scripts/pgvector/tests/harness_test.sh` (insert above `# --- runner ---`)

**Interfaces:**
- Produces:
  - `within <seconds> <command...>`: exits 124 if it had to kill the command;
  - `dk_stop_immediate`: returns 1 on an empty `DK_DATA`, on a missing container, or when the container is still running 60 s after the stop.

- [ ] **Step 1: Write the failing test**

Insert above the line `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# Run "$@" in the background and kill it after $1 seconds; exit 124 then.
within() {
  local secs="$1" pid rc=0
  shift
  "$@" &
  pid=$!
  for _ in $(seq 1 $((secs * 2))); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    return 124
  fi
  wait "$pid" || rc=$?
  return "$rc"
}

# docker-lib.sh fails instead of hanging or leaking: a docker run that fails
# (its port held) leaves no Created container behind, from dk_start_new and
# dk_boot_dir alike; dk_stop_immediate refuses at once when DK_DATA is empty
# (it used to wait forever on the running container) and fails fast when
# there is no container.
test_docker_lib_failures() {
  local w="$PGV_WORK/docker-lib-failures" holder rc
  rm -rf "$w"
  mkdir -p "$w/fake"
  : >"$w.log"
  echo 18 >"$w/fake/PG_VERSION"
  # Hold the port, so docker run creates the container and then fails to
  # start it. ReuseAddr: an earlier test's connections through the port may
  # still be in TIME_WAIT, which blocks a plain bind (not Docker's, which the
  # held port still refuses). The holder touches $w/held once it listens.
  perl -MIO::Socket::INET -e '
    my $s = IO::Socket::INET->new(LocalAddr => "127.0.0.1", LocalPort => $ARGV[0], Listen => 1, ReuseAddr => 1)
      or die "bind: $!\n";
    open(my $f, ">", $ARGV[1]) or die "$ARGV[1]: $!\n";
    close $f;
    sleep 120' "$PGV_PORT_DOCKER" "$w/held" 2>>"$w.log" &
  holder=$!
  for _ in $(seq 1 30); do
    [ ! -e "$w/held" ] || break
    sleep 0.5
  done
  if [ ! -e "$w/held" ]; then
    fail "docker-lib: could not hold port $PGV_PORT_DOCKER (see $w.log)"
    kill "$holder" 2>/dev/null || true
    return
  fi
  if dk_start_new "$PGV_DOCKER_IMAGE" >>"$w.log" 2>&1; then
    fail "docker-lib: dk_start_new succeeds on a held port"
  elif [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then
    pass "docker-lib: a failed dk_start_new leaves no container"
  else
    fail "docker-lib: a failed dk_start_new left $DK_NAME behind"
  fi
  if dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/fake" >>"$w.log" 2>&1; then
    fail "docker-lib: dk_boot_dir succeeds on a held port"
  elif [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then
    pass "docker-lib: a failed dk_boot_dir leaves no container"
  else
    fail "docker-lib: a failed dk_boot_dir left $DK_NAME behind"
  fi
  kill "$holder" 2>/dev/null || true
  wait "$holder" 2>/dev/null || true
  dk_rm
  if dk_start_new "$PGV_DOCKER_IMAGE" >>"$w.log" 2>&1; then
    rc=0
    DK_DATA='' within 10 dk_stop_immediate >>"$w.log" 2>&1 || rc=$?
    if [ "$rc" = 124 ]; then
      fail "docker-lib: dk_stop_immediate hangs with DK_DATA empty"
    elif [ "$rc" = 0 ]; then
      fail "docker-lib: dk_stop_immediate returns 0 with DK_DATA empty"
    elif [ "$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)" = true ]; then
      pass "docker-lib: dk_stop_immediate refuses at once with DK_DATA empty"
    else
      fail "docker-lib: dk_stop_immediate with DK_DATA empty stopped the container"
    fi
  else
    fail "docker-lib: the 0.8.7 image starts (see $w.log)"
  fi
  dk_rm
  rc=0
  DK_NAME=pgrust-pgvector-ondisk-absent within 10 dk_stop_immediate >>"$w.log" 2>&1 || rc=$?
  if [ "$rc" = 124 ] || [ "$rc" = 0 ]; then
    fail "docker-lib: dk_stop_immediate without a container (exit $rc)"
  else
    pass "docker-lib: dk_stop_immediate fails fast without a container"
  fi
  rm -rf "$w"
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `scripts/pgvector/tests/harness_test.sh test_docker_lib_failures`
Expected: `3 failure(s)`, in about 15 s (the watchdog kills the hung stop after 10 s):
```
FAIL - docker-lib: a failed dk_start_new left pgrust-pgvector-ondisk behind
FAIL - docker-lib: a failed dk_boot_dir left pgrust-pgvector-ondisk behind
FAIL - docker-lib: dk_stop_immediate hangs with DK_DATA empty
ok   - docker-lib: dk_stop_immediate fails fast without a container
```
The last check already passes, because `docker wait` fails on a missing container. It pins that behaviour.

- [ ] **Step 3: Implement**

(a) `dk_wait_ready`: drop the unused loop variable (SC2034). It becomes:

```bash
dk_wait_ready() {
  for _ in $(seq 1 240); do
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
```

(b) In `dk_start_new`, the failure branch becomes:

```bash
    echo "docker run $image failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    dk_rm
    return 1
```

(c) In `dk_boot_dir`, the failure branch becomes:

```bash
    echo "docker run $image on $dir failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    dk_rm
    return 1
```

(d) Replace `dk_stop_immediate` and its comment:

```bash
# pg_ctl stop -m immediate: no shutdown checkpoint, so the next start
# replays WAL from the last checkpoint. The postmaster is the container's
# PID 1, so the container ends the moment it exits and takes this exec with
# it: pg_ctl's own exit status means nothing, and the caller checks the
# copied directory's state instead. Fails, rather than waits forever, when
# no container was started, there is none, or it is still running 60 s on.
dk_stop_immediate() {
  local running
  if [ -z "$DK_DATA" ]; then
    echo "dk_stop_immediate: no container was started (DK_DATA is empty)" >&2
    return 1
  fi
  docker exec -u postgres "$DK_NAME" pg_ctl stop -m immediate -D "$DK_DATA" >/dev/null 2>&1 || true
  for _ in $(seq 1 120); do
    if ! running="$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)"; then
      echo "dk_stop_immediate: no container $DK_NAME" >&2
      return 1
    fi
    [ "$running" = true ] || return 0
    sleep 0.5
  done
  echo "dk_stop_immediate: $DK_NAME still running 60 s after pg_ctl stop -m immediate" >&2
  return 1
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `scripts/pgvector/tests/harness_test.sh test_docker_lib_failures test_docker_lib`
Expected: `all passed`, with 4 and 12 `ok` lines. `test_docker_lib` is M5's helper test, rerun because this task changes the helpers it covers.

- [ ] **Step 5: Lint**

```bash
/bin/bash -n scripts/pgvector/docker-lib.sh && /bin/bash -n scripts/pgvector/tests/harness_test.sh
(cd scripts/pgvector && shellcheck -x docker-lib.sh)
(cd scripts/pgvector/tests && shellcheck -x -f gcc harness_test.sh | wc -l)
```
Expected: no shellcheck output for `docker-lib.sh` (its SC2034 is gone), and `19` for the harness.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/docker-lib.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "fix(pgvector): docker-lib.sh: dk_stop_immediate is bounded; a failed docker run leaves no container" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: The capture moves into `ondisk-capture.sh`; the key narrows; `run-ondisk.sh` clears its results first

This task covers:
- the final review's "narrow the capture key";
- M5 Task 6's first two minors: the header wording, and `$out` cleared only after `ensure_capture` and the pagemask check (the highest-priority item);
- M5 Task 4's SC2034 and psql-version nits.

The moved code is unchanged apart from the key and one loop variable. Step 5 proves it with a diff.

**Files:**
- Create: `scripts/pgvector/ondisk-capture.sh`
- Modify: `scripts/pgvector/run-ondisk.sh` (header `:23-32`; remove `:43-46` and `:57-213`; source the library; main `:487-491`)
- Modify: `scripts/pgvector/sql/ondisk-rows.sql:3-4` (header comment only)
- Test: `scripts/pgvector/tests/harness_test.sh` (insert above `# --- runner ---`; two checks added to `test_stale_results`)

**Interfaces:**
- Consumes:
  - `stale_results_check` and `test_stale_results` (Task 1);
  - `dk_*` (Task 2).
- Produces: `scripts/pgvector/ondisk-capture.sh`, sourced after `common.sh` and `docker-lib.sh`. It defines:
  - the variables `ondisk_lib`, `sql`, `cap`, `DK_CONN` and `INFLIGHT_PID`;
  - the functions `load`, `transcript`, `state`, `waldump`, `open_transaction`, `close_transaction`, `capture_key`, `oracle`, `capture`, `facts` and `ensure_capture`.

  Task 4 adds `cap_fact` and `capture_check` to it.

- [ ] **Step 1: Write the failing tests**

(a) Insert above the line `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# capture_key of the scripts under $1 (scripts/pgvector or a copy).
ondisk_key() {
  (
    # shellcheck source=/dev/null
    . "$1/common.sh"
    # shellcheck source=/dev/null
    . "$1/docker-lib.sh"
    # shellcheck source=/dev/null
    . "$1/ondisk-capture.sh"
    capture_key
  )
}

# ensure_capture 0 of the scripts under $1 on the capture at $2, with capture
# itself replaced: prints "rebuilt" or "reused".
ondisk_ensure() {
  (
    # shellcheck source=/dev/null
    . "$1/common.sh"
    # shellcheck source=/dev/null
    . "$1/docker-lib.sh"
    # shellcheck source=/dev/null
    . "$1/ondisk-capture.sh"
    cap="$2"
    # shellcheck disable=SC2329 # ensure_capture calls it
    capture() { echo rebuilt; }
    r="$(ensure_capture 0)"
    echo "${r:-reused}"
  )
}

# The capture key covers the capture's inputs and nothing else: a comment in
# ondisk-capture.sh, docker-lib.sh or an on-disk SQL file changes it, and so
# does another psql version; a comment in run-ondisk.sh does not.
test_ondisk_capture_key() {
  local w="$PGV_WORK/ondisk-key" src k f c
  src="$(cd "$here/.." && pwd)"
  rm -rf "$w"
  mkdir -p "$w"
  cp -R "$src" "$w/pgvector"
  k="$(ondisk_key "$src" 2>"$w.log" || true)"
  if [ -n "$k" ] && [ "$(ondisk_key "$w/pgvector" 2>>"$w.log" || true)" = "$k" ]; then
    pass "ondisk key: a copy of the scripts has the same key"
  else
    fail "ondisk key: no key, or a copy's differs (see $w.log)"
    return
  fi
  echo '# a comment' >>"$w/pgvector/run-ondisk.sh"
  if [ "$(ondisk_key "$w/pgvector")" = "$k" ]; then
    pass "ondisk key: a comment in run-ondisk.sh keeps the key"
  else
    fail "ondisk key: a comment in run-ondisk.sh changes the key"
  fi
  for f in ondisk-capture.sh docker-lib.sh sql/ondisk-rows.sql sql/repair-verify.sql; do
    case "$f" in *.sql) c='-- a comment' ;; *) c='# a comment' ;; esac
    echo "$c" >>"$w/pgvector/$f"
    if [ "$(ondisk_key "$w/pgvector")" != "$k" ]; then
      pass "ondisk key: a comment in $f changes the key"
    else
      fail "ondisk key: a comment in $f keeps the key"
    fi
    cp "$src/$f" "$w/pgvector/$f"
  done
  # Both sides' transcripts come from $PG_TOOLS's psql, so another psql
  # version must not reuse a capture.
  mkdir -p "$w/pgref/pg/bin"
  printf '#!/bin/sh\necho "psql (PostgreSQL) 18.7"\n' >"$w/pgref/pg/bin/psql"
  chmod +x "$w/pgref/pg/bin/psql"
  if [ "$(PGREF="$w/pgref" ondisk_key "$w/pgvector")" != "$k" ]; then
    pass "ondisk key: another psql version changes the key"
  else
    fail "ondisk key: another psql version keeps the key"
  fi
  # ensure_capture rebuilds when the key file is missing (an interrupted or
  # failed capture writes none) or stale, and reuses a matching one.
  mkdir -p "$w/capture"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = rebuilt ]; then
    pass "ondisk key: a capture without a key is rebuilt"
  else
    fail "ondisk key: a capture without a key is reused"
  fi
  echo 0000000000000000 >"$w/capture/key"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = rebuilt ]; then
    pass "ondisk key: a capture with a stale key is rebuilt"
  else
    fail "ondisk key: a capture with a stale key is reused"
  fi
  echo "$k" >"$w/capture/key"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = reused ]; then
    pass "ondisk key: a capture with the current key is reused"
  else
    fail "ondisk key: a capture with the current key is rebuilt"
  fi
  rm -rf "$w"
}
```

(b) Replace `test_stale_results` (Task 1's version) with:

```bash
test_stale_results() {
  local w="$PGV_WORK/stale"
  rm -rf "$w"
  stale_results_check bytecmp PAGEMASK_BIN
  stale_results_check diff DIFFRUNNER_BIN
  stale_results_check ondisk PAGEMASK_BIN
  # An argument error comes before the clear: a typo touches nothing.
  mkdir -p "$w/ondisk/ref"
  printf 'old\tok\n' >"$w/ondisk/ref/summary.tsv"
  if PGV_WORK="$w" "$here/../run-ondisk.sh" ref nosuchpart >"$w/usage.log" 2>&1; then
    fail "stale results: run-ondisk.sh accepts a bad part"
  elif [ -s "$w/ondisk/ref/summary.tsv" ]; then
    pass "stale results: a usage error leaves the old results alone"
  else
    fail "stale results: a usage error removed the old results"
  fi
  rm -rf "$w"
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `scripts/pgvector/tests/harness_test.sh test_stale_results test_ondisk_capture_key`
Expected: `2 failure(s)`:
```
FAIL - stale results: run-ondisk.sh left its old summary.tsv (see …)
FAIL - ondisk key: no key, or a copy's differs (see …)
```
The `nosuchpart` check already passes: today's usage error also comes first. It pins that order.

- [ ] **Step 3: Create `scripts/pgvector/ondisk-capture.sh`**

Its mode is `-rw-r--r--`, like `docker-lib.sh`.

```bash
# shellcheck shell=bash
# The on-disk tier's capture: C pgvector writes the clean, crash and v081
# directories in Docker, and C's answers come from the 0.8.7 image booting
# a copy of each (run-ondisk.sh's header describes them). Sourced by
# run-ondisk.sh after common.sh and docker-lib.sh; bash 3.2 compatible.
#
# The capture is cached under $cap by a key over everything it runs
# (capture_key): this file, docker-lib.sh, sql/ondisk-*.sql,
# sql/repair-verify.sql, the image pins and $PG_TOOLS's psql version. Any
# edit to those, even a comment, forces a new capture, which invalidates
# measured evidence. run-ondisk.sh's forward and reverse code is outside the
# key. The load, transcript, state and transaction helpers live here
# because the capture's outputs depend on them; the forward gate and the
# reverse report use the same ones.

ondisk_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sql="$ondisk_lib/sql"
cap="$PGV_WORK/ondisk/capture"
DK_CONN=(-h 127.0.0.1 -p "$PGV_PORT_DOCKER")
INFLIGHT_PID=

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
  local xidfile="$1" xid=
  shift
  (cd "$sql" && exec "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 "$@" -U postgres -d postgres -f ondisk-inflight.sql) >/dev/null 2>&1 &
  INFLIGHT_PID=$!
  for _ in $(seq 1 600); do
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
    "$PG_TOOLS/bin/psql" --version
    cat "$sql"/ondisk-*.sql "$sql/repair-verify.sql" "$ondisk_lib/docker-lib.sh" "$ondisk_lib/ondisk-capture.sh"
  } | shasum -a 256 | cut -c1-16
}

# C's answers on a copy of $1/data: the 0.8.7 image boots it, then the
# transcripts of $2 (ondisk-queries.sql, or ondisk-update.sql for v081) and
# of the follow-up, whose DML and maintenance must succeed on C.
oracle() {
  local d="$1" q="$2" name
  name="${q#ondisk-}"
  name="${name%.sql}"
  dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" || die "the 0.8.7 image did not boot $d/data"
  transcript "$q" "${DK_CONN[@]}" >"$d/expected-$name.out"
  transcript ondisk-followup.sql "${DK_CONN[@]}" >"$d/expected-followup.out"
  if grep -q 'ERROR:' "$d/expected-followup.out"; then
    die "C's follow-up failed; see $d/expected-followup.out"
  fi
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
  dk_psql -q -v ON_ERROR_STOP=1 -f "$sql/ondisk-relfiles.sql" >"$k/relfiles.txt" ||
    die "ondisk-relfiles.sql failed on C's replay of $k/data"
  [ -s "$k/relfiles.txt" ] || die "the relation list is empty: $k/relfiles.txt"
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
```

- [ ] **Step 4: Edit `scripts/pgvector/run-ondisk.sh`**

(a) **The header's float paragraph and usage.** Replace lines 23–32, from `# Every fixture value makes index scans compute exact distances` through `#     --force rebuilds. {pgrust|ref} capture first when the key changed.`, with:

```bash
# Every fixture value keeps the distances that index scans return exact
# (sql/ondisk-rows.sql), so they do not depend on C's vectorized, fused
# float sums (M2's no-emulation ruling stands). Only IVFFlat's distances
# from a query to its k-means centers round; for the lists a probe picks,
# the evidence is the stability measurement (docs/superpowers/reports/
# pgvector-m5-report.md, "The fixture and the float question").
#
# usage: run-ondisk.sh capture [--force]
#        run-ondisk.sh {pgrust|ref} [clean|crash|v081|reverse ...]
#   capture: build the three directories and C's answers under
#     $PGV_WORK/ondisk/capture (ondisk-capture.sh, which also holds the
#     cache key); --force rebuilds. {pgrust|ref} capture first when the
#     key changed.
```

(b) **Source the library.** After `. "$here/docker-lib.sh"`, add:

```bash
. "$here/ondisk-capture.sh"
```

(c) **Remove what moved.** Delete:
- the block from `sql="$here/sql"` through `INFLIGHT_PID=`, and the blank line after it;
- every line from `# Run one sql/ file quietly, stopping at its first error.` through `ensure_capture`'s closing `}`, and the blank line after it.

`result()` then follows `cleanup()`.

(d) **Main.** Replace:

```bash
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/ondisk/$mode"
trap 'cleanup; server_stop "$subject"' EXIT
ensure_capture 0
rm -rf "$out"
mkdir -p "$out"
```
with:
```bash
out="$PGV_WORK/ondisk/$mode"
# Clear the old results before anything that can fail (pagemask, the
# capture), so a run that dies never leaves a previous summary.tsv behind.
rm -rf "$out"
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
trap 'cleanup; server_stop "$subject"' EXIT
ensure_capture 0
mkdir -p "$out"
```

- [ ] **Step 5: Check the move**

```bash
cd scripts/pgvector
diff <(git show main:scripts/pgvector/run-ondisk.sh | sed -n 57,212p) <(sed -n '/^# Run one sql\/ file/,$p' ondisk-capture.sh)
comm -12 <(sed -n 's/^\([a-z_]*\)() {$/\1/p' run-ondisk.sh | sort) <(sed -n 's/^\([a-z_]*\)() {$/\1/p' ondisk-capture.sh | sort)
sed -n 's/^\([a-z_]*\)() {$/\1/p' run-ondisk.sh | tr '\n' ' '; echo
cd ../..
```
Expected:
- **The diff** shows exactly three changes: the loop variable in `open_transaction` (twice) and the key.
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
- **`comm`** prints nothing: no function is defined in both files.
- **The function list** is `usage cleanup result compare compare_pages subject_start forward rrow rcompare fresh_cluster reverse_clean reverse_crash reverse`.

- [ ] **Step 6: The header wording in `sql/ondisk-rows.sql`**

Replace lines 3–4:

```sql
-- ondisk-followup.sql with \set lo / \set hi. Every value makes index scans
-- compute exact distances, whatever the summation order or FMA:
```
with:
```sql
-- ondisk-followup.sql with \set lo / \set hi. Every value keeps the
-- distances that index scans return exact, whatever the summation order or
-- FMA (only IVFFlat's distances from a query to its k-means centers round;
-- docs/superpowers/reports/pgvector-m5-report.md, "The fixture and the
-- float question"):
```

Nothing else in the file changes. `git diff --stat -- scripts/pgvector/sql` must show only this file, with 2 lines removed and 5 added.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `scripts/pgvector/tests/harness_test.sh test_stale_results test_ondisk_capture_key`
Expected: 14 `ok` lines and `all passed`.

- [ ] **Step 8: The tier still works on a fresh capture**

The key changed, so this recaptures first.

```bash
scripts/pgvector/run-ondisk.sh ref clean crash v081 >"$HOME/.cache/pgrust/m5f-task3-ref.log" 2>&1; echo "exit $?"
cat ~/.cache/pgrust/pgvector-work/ondisk/ref/summary.tsv
```
Expected: `exit 0` and seven `ok` rows, in about 3 minutes; the log starts with `capture: clean (0.8.7)`. A failure here is a defect in the move, not in pgrust: compare with the Step 5 diff.

- [ ] **Step 9: Lint**

```bash
for f in ondisk-capture.sh run-ondisk.sh tests/harness_test.sh; do /bin/bash -n "scripts/pgvector/$f" || echo "SYNTAX: $f"; done
(cd scripts/pgvector && shellcheck -x ondisk-capture.sh run-ondisk.sh)
(cd scripts/pgvector/tests && shellcheck -x -f gcc harness_test.sh | wc -l)
```
Expected: no `SYNTAX` lines, no shellcheck output (`run-ondisk.sh`'s SC2034 moved with `open_transaction` and is fixed), and `19`.

- [ ] **Step 10: Commit**

```bash
git add scripts/pgvector/ondisk-capture.sh scripts/pgvector/run-ondisk.sh scripts/pgvector/sql/ondisk-rows.sql scripts/pgvector/tests/harness_test.sh
git commit -q -m "fix(pgvector): the on-disk capture moves into ondisk-capture.sh with a key over its own inputs; run-ondisk.sh clears its results first" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The capture checks itself

The final review's "the capture never checks its own coverage", plus M5 Task 4's two capture-side minors:
- `close_transaction` `wait`s with no kill;
- `oracle`'s follow-up guard matches `ERROR:` only, so an empty transcript or a `psql: error:` line passes.

`capture_check` reads what `capture` already wrote: `facts.tsv`, C's transcripts, `crash/relfiles.txt` and the crash's two page sets. It runs before `capture_key` writes the key. A capture without `pagemask` now fails before it deletes the cached one.

**Files:**
- Modify: `scripts/pgvector/ondisk-capture.sh` (`close_transaction`, `oracle`, `capture`; new `cap_fact` and `capture_check`)
- Test: `scripts/pgvector/tests/harness_test.sh` (insert above `# --- runner ---`)

**Interfaces:**
- Consumes: `ondisk-capture.sh` (Task 3).
- Produces:
  - `cap_fact <name>`: a value from `$cap/facts.tsv`;
  - `capture_check`: dies with a `capture check: …` message;
  - `ondisk_capture_copy <dir>` and `ondisk_check_fails <dir> <case> <message>` (self-test helpers).

- [ ] **Step 1: Write the failing test**

Insert above the line `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:

```bash
# A copy of the self-test capture at $1: its files copied (so a test can
# edit them), its data directories linked.
ondisk_capture_copy() {
  local src="$PGV_WORK/ondisk/capture" f
  rm -rf "$1"
  mkdir -p "$1/clean" "$1/crash" "$1/v081"
  cp "$src/facts.tsv" "$1/"
  for f in "$src"/clean/* "$src"/crash/* "$src"/v081/*; do
    if [ -d "$f" ]; then ln -s "$f" "$1/${f#"$src"/}"; else cp "$f" "$1/${f#"$src"/}"; fi
  done
}

# capture_check on the capture at $1, which must die with message $3; $2
# names the case.
ondisk_check_fails() {
  if (
    . "$here/../ondisk-capture.sh"
    cap="$1"
    capture_check
  ) >"$1.out" 2>&1; then
    fail "ondisk capture check: passes with $2"
  elif grep -qF "$3" "$1.out"; then
    pass "ondisk capture check: dies with $2"
  else
    fail "ondisk capture check: dies with $2, but not for that (see $1.out)"
  fi
}

# The capture checks its own coverage (capture_check): it passes on the
# self-test capture (test_ondisk_capture's) and dies on copies with a fact
# wrong, an unintended error in C's answers, or a crash whose replay changed
# nothing. A capture that cannot check itself (no pagemask) fails before it
# deletes the cached one.
test_ondisk_capture_check() {
  local w="$PGV_WORK/ondisk-check" nopm="$PGV_WORK/ondisk-check-nopm"
  if [ ! -f "$PGV_WORK/ondisk/capture/key" ]; then
    fail "ondisk capture check: no self-test capture (run test_ondisk_capture first)"
    return
  fi
  ondisk_capture_copy "$w"
  if (
    . "$here/../ondisk-capture.sh"
    cap="$w"
    capture_check
  ) >"$w.out" 2>&1; then
    pass "ondisk capture check: passes on the capture"
  else
    # No capture_check (yet): stop here, before a capture --force below
    # could start a real capture.
    fail "ondisk capture check: fails on the capture (see $w.out)"
    return
  fi
  awk -F'\t' -v OFS='\t' '$1 == "crash_state" { $2 = "shut down" } { print }' "$w/facts.tsv" >"$w/facts.new"
  mv "$w/facts.new" "$w/facts.tsv"
  ondisk_check_fails "$w" "a clean crash" "capture check: crash_state is 'shut down', want 'in production'"
  ondisk_capture_copy "$w"
  echo 'psql:ondisk-queries.sql:1: ERROR:  relation "nope" does not exist' >>"$w/crash/expected-queries.out"
  ondisk_check_fails "$w" "an unintended error" "has 3 errors, 2 of them wrong-dimension"
  ondisk_capture_copy "$w"
  rm "$w/crash/replayed"
  ln -s "$PGV_WORK/ondisk/capture/crash/data" "$w/crash/replayed"
  ondisk_check_fails "$w" "an unreplayed crash" "unchanged, so crash_pages would not test its replay"
  rm -rf "$w" "$w.out" "$nopm"
  mkdir -p "$nopm/ondisk/capture"
  echo old >"$nopm/ondisk/capture/key"
  if PGV_WORK="$nopm" PAGEMASK_BIN=/nonexistent "$here/../run-ondisk.sh" capture --force >"$nopm.log" 2>&1; then
    fail "ondisk capture check: capture --force succeeds without pagemask"
  elif [ "$(cat "$nopm/ondisk/capture/key" 2>/dev/null)" = old ]; then
    pass "ondisk capture check: a capture without pagemask keeps the cached one"
  else
    fail "ondisk capture check: a capture without pagemask deleted the cached one (see $nopm.log)"
  fi
  rm -rf "$nopm"
}
```

- [ ] **Step 2: Run it to verify it fails**

The test needs the self-test capture. M5 left one under `~/.cache/pgrust/pgvector-work/selftest/ondisk/capture`; if `key` is missing there, run `scripts/pgvector/tests/harness_test.sh test_ondisk_capture` first (about 2 minutes).

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_capture_check`
Expected: `1 failure(s)`. `ondisk-check.out` says `capture_check: command not found`.
```
FAIL - ondisk capture check: fails on the capture (see …/ondisk-check.out)
```
The test stops at its first check, so the `capture --force` at its end never starts a real capture on the old code.

- [ ] **Step 3: Implement, in `scripts/pgvector/ondisk-capture.sh`**

(a) Replace `close_transaction` and its comment:

```bash
# Reap the open transaction's psql once its server is gone. Kill it first:
# a client whose socket Docker's proxy keeps open would never exit.
close_transaction() {
  if [ -n "$INFLIGHT_PID" ]; then
    kill "$INFLIGHT_PID" 2>/dev/null || true
    wait "$INFLIGHT_PID" 2>/dev/null || true
  fi
  INFLIGHT_PID=
}
```

(b) Replace `oracle` and its comment:

```bash
# C's answers on a copy of $1/data: the 0.8.7 image boots it, then the
# transcripts of $2 (ondisk-queries.sql, or ondisk-update.sql for v081) and
# of the follow-up, whose DML and maintenance must succeed on C: no error,
# and a transcript at all (psql reports a failed connection as
# "psql: error:").
oracle() {
  local d="$1" q="$2" name
  name="${q#ondisk-}"
  name="${name%.sql}"
  dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" || die "the 0.8.7 image did not boot $d/data"
  transcript "$q" "${DK_CONN[@]}" >"$d/expected-$name.out"
  transcript ondisk-followup.sql "${DK_CONN[@]}" >"$d/expected-followup.out"
  if [ ! -s "$d/expected-followup.out" ] || grep -qE 'ERROR:|FATAL:|^psql: error:' "$d/expected-followup.out"; then
    die "C's follow-up failed; see $d/expected-followup.out"
  fi
  dk_rm
}
```

(c) In `capture`, check for `pagemask` before anything is deleted. Replace:

```bash
  dk_require
  rm -rf "$cap"
```
with:
```bash
  dk_require
  [ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
  rm -rf "$cap"
```

and check the capture before its key is written. Replace:

```bash
  facts
  capture_key >"$cap/key"
```
with:
```bash
  facts
  capture_check
  capture_key >"$cap/key"
```

(d) Insert before `ensure_capture() {`:

```bash
# A value from the capture's facts.tsv.
cap_fact() {
  awk -F'\t' -v k="$1" '$1 == k { print $2 }' "$cap/facts.tsv"
}

# Die unless the capture exercised what the gate claims. Without this, a
# capture that missed the crash, an index, the TOAST table or the update
# would still let every forward row pass. capture writes the key only after
# it, so a capture that fails here is rebuilt next time.
capture_check() {
  local f got n name kind path
  # The facts: the directories' states, every relation and vector index,
  # the TOASTed column, the open transaction and the update.
  for f in "clean_state=shut down" "v081_state=shut down" "crash_state=in production" \
    crash_relations=42 vector_indexes=25 wide_toasted=t v081_before=0.8.1 v081_after=0.8.7; do
    got="$(cap_fact "${f%%=*}")"
    [ "$got" = "${f#*=}" ] || die "capture check: ${f%%=*} is '$got', want '${f#*=}' ($cap/facts.tsv)"
  done
  got="$(cap_fact crash_inflight_generic)"
  [ "${got:-0}" -gt 0 ] || die "capture check: the open transaction wrote no Generic records ($cap/facts.tsv)"
  # C's intended errors, every one a wrong-dimension query: two in the
  # queries, four in the update (which runs the queries twice).
  for f in clean/expected-queries.out=2 crash/expected-queries.out=2 v081/expected-update.out=4; do
    got="$(grep -c 'ERROR:' "$cap/${f%%=*}" || true)"
    n="$(grep -c 'ERROR:  expected 8 dimensions, not 3$' "$cap/${f%%=*}" || true)"
    [ "$got/$n" = "${f#*=}/${f#*=}" ] ||
      die "capture check: $cap/${f%%=*} has $got errors, $n of them wrong-dimension; want ${f#*=}, all wrong-dimension"
  done
  # crash_pages means something only where replay changed the pages: every
  # vector index's main fork must differ, after pagemask, between the
  # crashed directory and C's replay of it.
  n=0
  while IFS='|' read -r name kind path; do
    [ "$kind" = generic ] || continue
    n=$((n + 1))
    if ! "$PAGEMASK_BIN" generic "$cap/crash/data/$path" "$cap/check-data.masked" ||
      ! "$PAGEMASK_BIN" generic "$cap/crash/replayed/$path" "$cap/check-replayed.masked"; then
      die "capture check: pagemask failed on $name ($path)"
    fi
    if cmp -s "$cap/check-data.masked" "$cap/check-replayed.masked"; then
      die "capture check: replay left $name unchanged, so crash_pages would not test its replay"
    fi
  done <"$cap/crash/relfiles.txt"
  rm -f "$cap/check-data.masked" "$cap/check-replayed.masked"
  [ "$n" -eq 25 ] || die "capture check: $n vector indexes in $cap/crash/relfiles.txt, want 25"
}
```

- [ ] **Step 4: Run the tests to verify they pass, on a fresh self-test capture**

Run: `scripts/pgvector/tests/harness_test.sh test_ondisk_capture test_ondisk_capture_check test_ondisk_ref`
Expected, in about 5 minutes: `all passed`, with 10, 5 and 2 `ok` lines.
- `test_ondisk_capture` makes a fresh capture, which now runs `capture_check`.
- `test_ondisk_capture_check` reads that capture.
- `test_ondisk_ref` passes 7/7 on it.

A `capture check:` failure on the fresh capture is a plan-defined stop.

- [ ] **Step 5: Lint**

```bash
/bin/bash -n scripts/pgvector/ondisk-capture.sh && /bin/bash -n scripts/pgvector/tests/harness_test.sh
(cd scripts/pgvector && shellcheck -x ondisk-capture.sh run-ondisk.sh)
(cd scripts/pgvector/tests && shellcheck -x -f gcc harness_test.sh | wc -l)
```
Expected: no shellcheck output, and `19`.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/ondisk-capture.sh scripts/pgvector/tests/harness_test.sh
git commit -q -m "fix(pgvector): the on-disk capture checks its own coverage before it writes its key" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The stability rerun, the gates, the docs and the report

No capture input changes in this task (Global Constraints). Write each command's verdict down before the next command runs.

**Files:**
- Modify: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md` (§8.2's "On-disk, forward" row, §8.3)
- Modify: `CLAUDE.md` ("Vector search (fork work)")
- Modify: `docs/superpowers/reports/pgvector-m5-report.md` (three errata)
- Create: `docs/superpowers/reports/pgvector-m5-followups-report.md`

**Interfaces:** consumes everything above.

- [ ] **Step 1: The stability loop (three fresh captures, every exit status checked)**

Each capture builds new HNSW graphs and new k-means centers, and now runs `capture_check`. This is the one rerun that M5's report asked for.

```bash
mkdir -p ~/.cache/pgrust/m5f-evidence/stability
for i in 1 2 3; do
  log="$HOME/.cache/pgrust/m5f-evidence/stability/run-$i.log"
  date +%T
  rc=0
  scripts/pgvector/run-ondisk.sh capture --force >"$log" 2>&1 || rc=$?
  echo "capture $i: exit $rc"
  if [ "$rc" -ne 0 ]; then
    cp -R ~/.cache/pgrust/pgvector-work/ondisk/capture "$HOME/.cache/pgrust/m5f-capture-fail-$i"
    break
  fi
  cp ~/.cache/pgrust/pgvector-work/ondisk/capture/facts.tsv "$HOME/.cache/pgrust/m5f-evidence/stability/facts-$i.tsv"
  for m in ref pgrust; do
    rc=0
    scripts/pgvector/run-ondisk.sh "$m" clean crash v081 >>"$log" 2>&1 || rc=$?
    n="$(grep -c $'\tok$' ~/.cache/pgrust/pgvector-work/ondisk/$m/summary.tsv 2>/dev/null || true)"
    echo "run $i $m: exit $rc, ${n:-0}/7"
  done
done
date +%T
```
Expected, in about 10 minutes: three `capture i: exit 0` lines and six `run i m: exit 0, 7/7` lines.
- A capture that fails is a plan-defined stop. The loop has already copied it aside.
- So is a pgrust row that fails while `ref` passes on the same capture. Copy that capture aside before going on.

- [ ] **Step 2: The full gates**

```bash
scripts/pgvector/run-all.sh ref >"$HOME/.cache/pgrust/m5f-evidence/run-all-ref.log" 2>&1; echo "ref exit $?"
cp ~/.cache/pgrust/pgvector-work/report-ref.md ~/.cache/pgrust/m5f-evidence/
scripts/pgvector/run-all.sh pgrust >"$HOME/.cache/pgrust/m5f-evidence/run-all-pgrust.log" 2>&1; echo "pgrust exit $?"
cp ~/.cache/pgrust/pgvector-work/report-pgrust.md ~/.cache/pgrust/m5f-evidence/
grep -h '^\*\*' ~/.cache/pgrust/m5f-evidence/report-{ref,pgrust}.md
```
Expected:
- `ref exit 0`, `**137/137 passed**`, and a reverse section in which every row is `ok`, `identical`, `clean` or `none`;
- `pgrust exit 1`, `**129/137 passed**`. The only failures are the 8 TAP parallel-build rows (`003`, `012`, `020`, `024`, `028`, `032`, `035`, `045`), with no `(run-…sh exited …)` or `(no results…)` row.

The `ondisk` tier reuses the loop's third capture (a cache hit).

- [ ] **Step 3: The full self-test suite**

```bash
scripts/pgvector/tests/harness_test.sh >"$HOME/.cache/pgrust/m5f-evidence/harness_test.log" 2>&1; echo "exit $?"
grep -c '^ok' ~/.cache/pgrust/m5f-evidence/harness_test.log; tail -1 ~/.cache/pgrust/m5f-evidence/harness_test.log
```
Expected: `exit 0`, `192` and `all passed`, in about 7 minutes.

- [ ] **Step 4: The reference trees**

Run: `git status --porcelain --ignored -- crates/pgvector-0.8.7-reference crates/postgres-18.6-reference`
Expected: no output.

- [ ] **Step 5: Update the spec**

(a) §8.2, in the "On-disk, forward" row's middle cell, after `…runs inserts, deletes, `VACUUM` and `REINDEX` (`sql/ondisk-followup.sql`).`, add:

```markdown
 A capture that did not exercise what this row claims is refused before use: the clean, crashed and 0.8.1 directories, all 42 relations and 25 vector indexes, the TOASTed column, the open transaction, only C's intended errors, and replay changing every vector index's pages (`scripts/pgvector/ondisk-capture.sh`, `capture_check`).
```

(b) §8.3, the bullet `` - `docker-lib.sh` and `sql/ondisk-*.sql` (M5; `run-ondisk.sh` is the tier) `` becomes:

```markdown
  - `docker-lib.sh` and `sql/ondisk-*.sql` (M5; `run-ondisk.sh` is the tier), and `ondisk-capture.sh` (M5 follow-ups: the capture, its cache key and its self-checks)
```

- [ ] **Step 6: Update `CLAUDE.md`**

(a) The `run-ondisk.sh capture` line in the harness code block becomes:

```bash
scripts/pgvector/run-ondisk.sh capture [--force]                            # Docker: C pgvector writes the clean, crash and 0.8.1 directories and C's answers (cached by a key over ondisk-capture.sh, docker-lib.sh and the on-disk SQL; editing any of them recaptures)
```

(b) Add `docs/superpowers/reports/pgvector-m5-followups-report.md` to the end of the Baselines list.

- [ ] **Step 7: The M5 report's errata** (`docs/superpowers/reports/pgvector-m5-report.md`)

(a) Line 17: `` Task 1's run (`.superpowers/sdd/2026-10-14-pgvector-m5-ondisk/task-1-report.md`) `` becomes `` Task 1's run (`~/.cache/pgrust/m5-evidence/sdd-reports/task-1-report.md`; the in-repo copy was deleted at cleanup) ``.

(b) The Evidence bullets (lines 44–47) name every file cited inline. Replace the `fr1/` and `task6-run1/` lines:

```markdown
  - `fr1/`: the fix-round gate runs, with each run's log, `report-{ref,pgrust}.md`, every tier's `summary.tsv`, `reverse.tsv` and `reverse.md`, pgrust's TAP prove logs and C's TAP `045` log;
  - `task6-run1/`: the first gate runs (the same files), the stability loop's script, output, logs and facts, and the self-test log;
```
with:
```markdown
  - `fr1/`: the fix-round gate runs, with each run's log, `report-{ref,pgrust}.md`, every tier's `summary.tsv`, `reverse.tsv` and `reverse.md`, pgrust's TAP prove logs and C's TAP `045` log, and `stale-results-demo.txt` (Review fixes beyond the plan's text);
  - `task6-run1/`: the first gate runs (the same files), the stability loop's script, output, logs and facts, the self-test log, and `build-pgrust.txt` (How the run was produced);
```

(c) Line 482 says `run-iterscan.sh` and `run-diff.sh` share `run-bytecmp.sh`'s die-after-rows path. They don't. Replace the whole line:

```markdown
- **Generalise `run-all.sh`'s exit-status guard to every tier** (pre-existing). `run-bytecmp.sh:430` can die after earlier cases have written `ok` rows, and the review found the same die-after-rows paths in `run-iterscan.sh` and `run-diff.sh`. `run-all.sh:12-16`'s `|| true` then counts the partial rows.
```
with:
```markdown
- **Generalise `run-all.sh`'s exit-status guard to every tier** (pre-existing). `run-bytecmp.sh:430` can die after earlier cases have written `ok` rows, and `run-all.sh:12-16`'s `|| true` then counts the partial rows. (`run-iterscan.sh` and `run-diff.sh` have no such path: every `die` in them comes before they truncate `summary.tsv`. `run-bytecmp.sh:416` and `run-diff.sh:28` can instead die before they remove `$out`, leaving the previous results.)
```

- [ ] **Step 8: Write `docs/superpowers/reports/pgvector-m5-followups-report.md`**

Follow the M4 follow-ups report's shape, shorter. Required content:
- **Header:** the date, the pgrust commit (the server's code is unchanged since M5's `436bd1b16f`), the references and the plan.
- **Summary:**
  - both modes' `run-all.sh` totals (Step 2), unchanged from M5;
  - the stability loop's nine lines (Step 1);
  - `harness_test.sh`'s count (Step 3);
  - the commit range;
  - that M5's exit criteria still hold.
- **What changed,** one subsection per task, each saying which M5 "Still open" item it closes and how it was shown. Include:
  - Task 3's move diff (Step 5);
  - the final capture's `capture_check` passing and its key;
  - that a comment in `run-ondisk.sh` no longer forces a recapture.
- **Stability:** the nine lines, the three captures' facts (Generic records, full-page images, the open transaction's records), and the time per iteration.
- **Decisions made during execution:** any plan-defined stop and the user's ruling. Write "none" if there were none.
- **Still open:**
  - the M5 report's "Still open", minus the items closed here, listed by name: the three final-review follow-ups; M5 Task 6's three minors; M5 Task 2's `dk_stop_immediate` and Created-container minors; and M5 Task 4's `close_transaction`, follow-up guard, SC2034 and `capture_key` items;
  - the M4 follow-ups' open items, which still stand;
  - this plan's own deferred review minors, by task.
- **Evidence:** `~/.cache/pgrust/m5f-evidence/`.

- [ ] **Step 9: Commit**

```bash
git add docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md CLAUDE.md docs/superpowers/reports/pgvector-m5-report.md docs/superpowers/reports/pgvector-m5-followups-report.md
git commit -q -m "docs(pgvector): M5 follow-ups report; spec, CLAUDE.md; M5 report errata" -m "Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>"
```

- [ ] **Step 10: Leave nothing running**

```bash
docker ps -a --format '{{.Names}}' | grep pgrust-pgvector || echo "no containers"
pgrep -fl "postgres -D" || echo "no servers"
git status --porcelain
```
Expected: `no containers`, `no servers`, and an empty `git status`.

Expected end state: 6 commits on `vector/m5-followups` (the plan, then Tasks 1–5). Do not merge or push; the controller does that after the final review and the user's go-ahead.

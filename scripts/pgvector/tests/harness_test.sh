#!/usr/bin/env bash
# Self-tests for the pgvector harness (they check the harness, not pgrust).
# usage: harness_test.sh [test_fn ...]   (default: every test_* function)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# Self-tests run in their own work dir so they never replace real results or
# stop real harness servers (only the Docker container name is shared).
PGV_REAL_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
export PGV_WORK="$PGV_REAL_WORK/selftest"
. "$here/../common.sh"
# Tests write logs beside their work dirs ("$PGV_WORK/<name>.log").
mkdir -p "$PGV_WORK"

fails=0
pass() { printf 'ok   - %s\n' "$1"; }
fail() { printf 'FAIL - %s\n' "$1"; fails=$((fails + 1)); }

# --- tests: each task inserts its functions above the runner marker ---

test_reference() {
  if "$here/../build-reference.sh" --verify; then
    pass "reference installs verified"
  else
    fail "reference installs verified"
  fi
}

test_pgrust_binary() {
  # pg_ctl (hybrid TAP) accepts only this exact -V output.
  local v
  v="$("$PGRUST_BIN" -V 2>&1 || true)"
  if [ "$v" = "postgres (PostgreSQL) 18.6" ]; then pass "pgrust -V"; else fail "pgrust -V: '$v'"; fi
  if [ -n "$(pgrust_ext_version)" ]; then
    pass "pgrust staged vector.control"
  else
    fail "pgrust staged vector.control missing under target/$PGRUST_PROFILE/share/extension"
  fi
}

test_servers() {
  local mode port v ext want d
  for mode in ref pgrust; do
    port="$(port_for "$mode")"
    if ! "$here/../server.sh" fresh "$mode" >/dev/null 2>&1; then
      fail "$mode: server starts (see $PGV_WORK/log/server-$mode.log)"
      continue
    fi
    pass "$mode: server starts"
    v="$(pgv_psql "$port" -Atc 'select version()' 2>&1 || true)"
    case "$mode:$v" in
      "pgrust:PostgreSQL 18.6"*"(pgrust "*) pass "$mode: version" ;;
      "ref:PostgreSQL 18.6"*"(pgrust "*) fail "$mode: version is pgrust: $v" ;;
      "ref:PostgreSQL 18.6"*) pass "$mode: version" ;;
      *) fail "$mode: version: $v" ;;
    esac
    # Review focus 2: pgrust must load its own staged extension files.
    ext="$(pgv_psql "$port" -Atc 'create extension vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
    if [ "$mode" = ref ]; then want=0.8.7; else want="$(pgrust_ext_version)"; fi
    if [ "$ext" = "$want" ]; then pass "$mode: extension version $want"; else fail "$mode: extension version: got '$ext', want '$want'"; fi
    d="$(pgv_psql "$port" -Atc "select '[1,2,3]'::vector <-> '[4,5,6]'" 2>&1 || true)"
    if [ "$d" = "5.196152422706632" ]; then pass "$mode: l2 distance"; else fail "$mode: l2 distance: '$d'"; fi
    # Review focus 3: a second start on a busy port must refuse.
    if "$here/../server.sh" start "$mode" >/dev/null 2>&1; then
      fail "$mode: second start refused"
    else
      pass "$mode: second start refused"
    fi
    if "$here/../server.sh" stop "$mode" >/dev/null 2>&1; then pass "$mode: server stops"; else fail "$mode: server stops"; fi
  done
}

# Review focus 1: suites must never write into the read-only reference trees.
# --ignored matters: pgvector's .gitignore hides results/ and regression.*.
assert_reference_clean() {
  [ -z "$(git -C "$PGV_REPO" status --porcelain --ignored -- crates/pgvector-0.8.7-reference crates/postgres-18.6-reference)" ]
}

test_regress_ref() {
  local n
  if "$here/../run-regress.sh" ref >/dev/null 2>&1; then
    pass "regress ref: exit 0"
  else
    fail "regress ref: exit 0 (see $PGV_WORK/regress/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/regress/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 14 ]; then pass "regress ref: 14 ok"; else fail "regress ref: '$n' ok, want 14"; fi
  if assert_reference_clean; then pass "regress: reference trees untouched"; else fail "regress: reference trees modified"; fi
}

# Review focus 4: pgrust mode must refuse a server that isn't pgrust.
test_regress_identity() {
  local log="$PGV_WORK/identity-test.log" sentinel="$PGV_WORK/regress/pgrust/.identity-sentinel"
  mkdir -p "$(dirname "$sentinel")"
  : >"$sentinel"
  # Isolated work dir: run-regress.sh wipes its output and data dirs first.
  if PGV_WORK="$PGV_WORK/identity" PGRUST_BIN="$PG_VEC/bin/postgres" \
    "$here/../run-regress.sh" pgrust bit >"$log" 2>&1; then
    fail "pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$log"; then
    pass "pgrust mode refuses a C server"
  else
    fail "pgrust mode refuses a C server: wrong error (see $log)"
  fi
  PGV_WORK="$PGV_WORK/identity" "$here/../server.sh" stop pgrust >/dev/null 2>&1 || true
  # Self-tests must not destroy real results from the last pgrust run.
  if [ -e "$sentinel" ]; then pass "identity test leaves pgrust results alone"; else fail "identity test wiped $PGV_WORK/regress/pgrust"; fi
  rm -f "$sentinel"
}

test_tap_ref() {
  if "$here/../run-tap.sh" ref 015_hnsw_vector_duplicates.pl >/dev/null 2>&1; then
    pass "tap ref: 015_hnsw_vector_duplicates"
  else
    fail "tap ref: 015_hnsw_vector_duplicates (see $PGV_WORK/tap/ref/prove)"
  fi
  if assert_reference_clean; then pass "tap: reference trees untouched"; else fail "tap: reference trees modified"; fi
  # Global constraint: clusters use --no-locale (Cluster.pm otherwise inherits the shell's locale).
  if grep -q 'initialized with locale "C"' \
    "$PGV_WORK/tap/ref/log/015_hnsw_vector_duplicates/regress_log_015_hnsw_vector_duplicates" 2>/dev/null; then
    pass "tap: clusters use --no-locale"
  else
    fail "tap: clusters not initialized with locale C"
  fi
}

# Review focus 4 for TAP: pgrust mode must not count a C server's passes.
test_tap_identity() {
  local w="$PGV_WORK/tap-identity"
  if PGV_WORK="$w" PGRUST_BIN="$PG_VEC/bin/postgres" \
    "$here/../run-tap.sh" pgrust 015_hnsw_vector_duplicates.pl >"$w.log" 2>&1; then
    fail "tap pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$w.log"; then
    pass "tap pgrust mode refuses a C server"
  else
    fail "tap pgrust mode refuses a C server: wrong error (see $w.log)"
  fi
}

# Self-tests must never replace the results of a real run.
test_selftest_isolation() {
  local sentinel="$PGV_REAL_WORK/regress/ref/.selftest-sentinel"
  mkdir -p "$(dirname "$sentinel")"
  : >"$sentinel"
  "$here/../run-regress.sh" ref bit >/dev/null 2>&1 || true
  if [ -e "$sentinel" ]; then pass "self-tests leave real results alone"; else fail "self-tests wiped $PGV_REAL_WORK/regress/ref"; fi
  rm -f "$sentinel"
}

# A stale pidfile (crash, reboot, pid reuse) must never get an unrelated process signalled.
test_server_stop_stale_pidfile() {
  local pid
  # perl restores default SIGINT handling (bash ignores SIGINT in background jobs).
  perl -e '$SIG{INT} = "DEFAULT"; sleep 300' &
  pid=$!
  mkdir -p "$PGV_WORK"
  echo "$pid" >"$PGV_WORK/ref.pid"
  "$here/../server.sh" stop ref >/dev/null 2>&1 || true
  sleep 0.5
  if kill -0 "$pid" 2>/dev/null; then
    pass "stale pidfile: unrelated process untouched"
  else
    fail "stale pidfile: unrelated process was signalled"
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -f "$PGV_WORK/ref.pid"
}

test_docker() {
  local ext coll before after
  before="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if ! "$here/../docker-ref.sh" up >/dev/null 2>&1; then
    fail "docker: up"
    return
  fi
  pass "docker: up"
  ext="$("$here/../docker-ref.sh" psql -Atc 'create extension if not exists vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
  if [ "$ext" = 0.8.7 ]; then pass "docker: pgvector 0.8.7"; else fail "docker: extversion '$ext'"; fi
  coll="$("$here/../docker-ref.sh" psql -Atc "select datcollate from pg_database where datname = 'postgres'" 2>&1 || true)"
  if [ "$coll" = C ]; then pass "docker: C collation"; else fail "docker: datcollate '$coll'"; fi
  if "$here/../docker-ref.sh" down >/dev/null 2>&1; then pass "docker: down"; else fail "docker: down"; fi
  # The image declares VOLUME; removing the container must not leave it dangling.
  after="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if [ "$after" -le "$before" ]; then pass "docker: no leaked volume"; else fail "docker: leaked $((after - before)) anonymous volume(s)"; fi
}

# Byte-identical tier, C vs C: two fresh seeded clusters must produce
# identical index files for every default case.
test_bytecmp_ref() {
  local n want
  want="$(set -- $("$here/../run-bytecmp.sh" --list) && echo $#)"
  if "$here/../run-bytecmp.sh" ref >/dev/null 2>&1; then
    pass "bytecmp ref: exit 0"
  else
    fail "bytecmp ref: exit 0 (see $PGV_WORK/bytecmp/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/bytecmp/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = "$want" ]; then pass "bytecmp ref: $want ok"; else fail "bytecmp ref: '$n' ok, want $want"; fi
  if assert_reference_clean; then pass "bytecmp: reference trees untouched"; else fail "bytecmp: reference trees modified"; fi
}

# The cases must exercise what they claim (reads the C-vs-C run above):
# multi-level graphs, a deep m=4 graph, page-split wide tuples, an init fork.
test_bytecmp_cases() {
  local f="$PGV_WORK/bytecmp/ref/cases.tsv" c label blocks level
  if [ ! -s "$f" ]; then
    fail "bytecmp cases: no $f (run test_bytecmp_ref first)"
    return
  fi
  while IFS=$'\t' read -r c label blocks level; do
    case "$c:$label" in
      *:idx_init) [ "$level" = -1 ] && pass "bytecmp $c: empty init fork" || fail "bytecmp $c: init fork entry level $level" ;;
      m4_golomb:idx) [ "$level" -ge 3 ] && pass "bytecmp $c: graph height $level" || fail "bytecmp $c: graph height $level, want >= 3" ;;
      wide_golomb:idx) [ "$blocks" -ge 150 ] && pass "bytecmp $c: $blocks blocks" || fail "bytecmp $c: $blocks blocks, want >= 150" ;;
      *) [ "$level" -ge 1 ] && [ "$blocks" -ge 10 ] && pass "bytecmp $c: $blocks blocks, height $level" ||
        fail "bytecmp $c: $blocks blocks, height $level (want multi-page, multi-level)" ;;
    esac
  done <"$f"
}

# Negative control: stock C pgvector never seeds (SeedRandom(42) is under
# #ifdef HNSW_MEMORY), so an unseeded subject must be reported as different.
test_bytecmp_detects_difference() {
  local w="$PGV_WORK/bytecmp-neg"
  if PGV_WORK="$w" PGV_BYTECMP_SUBJECT=ref "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp flags an unseeded C build"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/ref/summary.tsv" 2>/dev/null; then
    pass "bytecmp flags an unseeded C build"
  else
    fail "bytecmp flags an unseeded C build: wrong failure (see $w.log)"
  fi
}

# Review focus 1: the seed is opt-in. Without SET pgrust.hnsw_build_seed,
# pgrust builds like stock C and so differs from the seeded oracle.
test_bytecmp_knob_off() {
  local w="$PGV_WORK/bytecmp-noseed"
  if PGV_WORK="$w" PGV_BYTECMP_SEED= "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp: unseeded pgrust differs from the seeded oracle"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/pgrust/summary.tsv" 2>/dev/null; then
    pass "bytecmp: unseeded pgrust differs from the seeded oracle"
  else
    fail "bytecmp: unseeded pgrust: wrong failure (see $w.log)"
  fi
}

# The oracle must be the seeded build, and pgrust mode must refuse a C server.
test_bytecmp_identity() {
  local w="$PGV_WORK/bytecmp-ident"
  if PGV_WORK="$w" PG_VEC_SEEDED="$PG_VEC" "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp refuses an unseeded oracle"
  elif grep -q 'not built by the seeded C pgvector' "$w.log"; then
    pass "bytecmp refuses an unseeded oracle"
  else
    fail "bytecmp refuses an unseeded oracle: wrong error (see $w.log)"
  fi
  if PGV_WORK="$w" PGRUST_BIN="$PG_VEC/bin/postgres" "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$w.log"; then
    pass "bytecmp pgrust mode refuses a C server"
  else
    fail "bytecmp pgrust mode refuses a C server: wrong error (see $w.log)"
  fi
}

# --- runner ---
if [ "$#" -eq 0 ]; then
  # Declaration order (declare -F would sort): a test may read the results of
  # one declared above it, e.g. test_bytecmp_cases reads test_bytecmp_ref's.
  # shellcheck disable=SC2046 # intentional splitting: function names have no spaces
  set -- $(sed -n 's/^\(test_[A-Za-z0-9_]*\)() {$/\1/p' "${BASH_SOURCE[0]}")
fi
for t in "$@"; do
  echo "# $t"
  "$t"
done
if [ "$fails" -ne 0 ]; then
  echo "$fails failure(s)"
  exit 1
fi
echo "all passed"

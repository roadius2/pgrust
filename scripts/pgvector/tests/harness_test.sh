#!/usr/bin/env bash
# Self-tests for the pgvector harness (they check the harness, not pgrust).
# usage: harness_test.sh [test_fn ...]   (default: every test_* function)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/../common.sh"

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
  local log="$PGV_WORK/identity-test.log"
  if PGRUST_BIN="$PG_VEC/bin/postgres" "$here/../run-regress.sh" pgrust bit >"$log" 2>&1; then
    fail "pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$log"; then
    pass "pgrust mode refuses a C server"
  else
    fail "pgrust mode refuses a C server: wrong error (see $log)"
  fi
  "$here/../server.sh" stop pgrust >/dev/null 2>&1 || true
}

test_tap_ref() {
  if "$here/../run-tap.sh" ref 015_hnsw_vector_duplicates.pl >/dev/null 2>&1; then
    pass "tap ref: 015_hnsw_vector_duplicates"
  else
    fail "tap ref: 015_hnsw_vector_duplicates (see $PGV_WORK/tap/ref/prove)"
  fi
  if assert_reference_clean; then pass "tap: reference trees untouched"; else fail "tap: reference trees modified"; fi
}

test_docker() {
  local ext coll
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
}

# --- runner ---
if [ "$#" -eq 0 ]; then
  # shellcheck disable=SC2046 # intentional splitting: function names have no spaces
  set -- $(declare -F | awk '{print $3}' | grep '^test_')
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

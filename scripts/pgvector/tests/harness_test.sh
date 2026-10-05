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

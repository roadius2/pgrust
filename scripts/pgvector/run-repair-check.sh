#!/usr/bin/env bash
# Pre-M4 repair check (M4 follow-ups, Task 7). pgrust builds before M4
# installed a trimmed vector 0.8.5; three generations shipped on main
# (pgrust v0.3 79ad992ede, M2 88f29c4ccf, M3 c1c5da11cc). For each: make a
# database at that 0.8.5 with a table and an hnsw index, prove ALTER
# EXTENSION vector UPDATE fails, run the repair twice (the second run
# creates nothing), update, and compare the catalog with a fresh CREATE
# EXTENSION vector (sql/repair-verify.sql); the data and the index survive
# and an ivfflat index builds. Once more with the extension in schema s, and
# once on a fresh 0.8.7 install (nothing to repair).
# pgrust only: a scratch server under $PGV_WORK/repair-check; the staged old
# script is removed on exit.
# usage: run-repair-check.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

repair="$PGV_REPO/crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql"
verify="$here/sql/repair-verify.sql"
staged="$(dirname "$PGRUST_BIN")/share/extension"
PGV_WORK="$PGV_WORK/repair-check"
export PGV_WORK
out="$PGV_WORK/out"
rm -rf "$out"
mkdir -p "$out"

cleanup() {
  rm -f "$staged/vector--0.8.5.sql"
  "$here/server.sh" stop pgrust >/dev/null 2>&1 || true
}
trap cleanup EXIT

q() { "$here/server.sh" psql pgrust "$@"; }

passed=0
total=0
result() {
  total=$((total + 1))
  if [ "$2" = ok ]; then passed=$((passed + 1)); fi
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/summary.tsv"
}

# A fresh 0.8.7 install's catalog. $1 database, $2 schema ("" = public).
fresh_catalog() {
  q -c "CREATE DATABASE $1" >/dev/null
  if [ -n "$2" ]; then q -d "$1" -c "CREATE SCHEMA $2" >/dev/null; fi
  q -d "$1" -c "CREATE EXTENSION vector${2:+ SCHEMA $2}" >/dev/null
  q -d "$1" -f "$verify" >"$out/$1.txt"
}

# $1 label, $2 revision, $3 database, $4 schema ("" = public), $5 reference catalog.
check() {
  local label="$1" rev="$2" db="$3" schema="$4" ref="$5"
  local log="$out/$3.log" path="public${4:+, $4}"
  git -C "$PGV_REPO" show "$rev:crates/contrib/pgvector/extension/vector--0.8.5.sql" >"$staged/vector--0.8.5.sql"
  if ! {
    q -c "CREATE DATABASE $db" &&
      { [ -z "$schema" ] || q -d "$db" -c "CREATE SCHEMA $schema"; } &&
      q -d "$db" -c "CREATE EXTENSION vector VERSION '0.8.5'${schema:+ SCHEMA $schema}" &&
      q -d "$db" -c "SET search_path = $path; CREATE TABLE public.t (id int, v vector(3)); INSERT INTO public.t SELECT i, ARRAY[i % 7, i % 11, i % 13] FROM generate_series(1, 500) i; CREATE INDEX t_hnsw ON public.t USING hnsw (v vector_l2_ops);"
  } >"$log" 2>&1; then
    result "$label" "FAIL (setup; $log)"
    return
  fi
  if q -d "$db" -c "ALTER EXTENSION vector UPDATE" >>"$log" 2>&1; then
    result "$label" "FAIL (UPDATE worked before the repair; $log)"
    return
  fi
  if ! q -d "$db" -f "$repair" >"$out/$db.repair1" 2>&1 ||
    ! grep -q 'created [1-9][0-9]* missing objects' "$out/$db.repair1"; then
    result "$label" "FAIL (repair; $out/$db.repair1)"
    return
  fi
  if ! q -d "$db" -f "$repair" >"$out/$db.repair2" 2>&1 ||
    ! grep -q 'created 0 missing objects' "$out/$db.repair2"; then
    result "$label" "FAIL (second repair not a no-op; $out/$db.repair2)"
    return
  fi
  if ! q -d "$db" -c "ALTER EXTENSION vector UPDATE" >>"$log" 2>&1; then
    result "$label" "FAIL (UPDATE after the repair; $log)"
    return
  fi
  q -d "$db" -f "$verify" >"$out/$db.txt"
  if ! diff "$ref" "$out/$db.txt" >"$out/$db.diff"; then
    result "$label" "FAIL (catalog differs from a fresh install; $out/$db.diff)"
    return
  fi
  if ! {
    [ "$(q -d "$db" -At -c "SELECT count(*) FROM public.t")" = 500 ] &&
      q -d "$db" -At -c "SET search_path = $path; SET enable_seqscan = off; EXPLAIN (COSTS OFF) SELECT id FROM public.t ORDER BY v <-> '[1,1,1]' LIMIT 3" | grep -q 'Index Scan using t_hnsw' &&
      [ "$(q -d "$db" -At -c "SET search_path = $path; SELECT count(*) FROM (SELECT id FROM public.t ORDER BY v <-> '[1,1,1]' LIMIT 3) s")" = 3 ] &&
      q -d "$db" -c "SET search_path = $path; CREATE INDEX t_ivf ON public.t USING ivfflat (v vector_l2_ops) WITH (lists = 4)"
  } >>"$log" 2>&1; then
    result "$label" "FAIL (data or indexes after the update; $log)"
    return
  fi
  result "$label" ok
}

"$here/server.sh" fresh pgrust >"$out/server.log" 2>&1
fresh_catalog fresh ""
fresh_catalog fresh_s s
for rev in 79ad992ede 88f29c4ccf c1c5da11cc; do
  check "$rev" "$rev" "g_$rev" "" "$out/fresh.txt"
done
check "79ad992ede/schema" 79ad992ede g_schema s "$out/fresh_s.txt"
if q -d fresh -f "$repair" 2>&1 | grep -q 'nothing to repair'; then
  result "0.8.7/no-op" ok
else
  result "0.8.7/no-op" "FAIL (repair on a 0.8.7 install did not say nothing to repair)"
fi
echo "$passed/$total passed"
[ "$passed" = "$total" ]

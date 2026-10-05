#!/usr/bin/env bash
# Iterative-scan stop-point tier (spec §8.2). Builds the same seeded, tie-free
# HNSW index on the seeded C reference (the oracle) and on the subject, then
# runs iterative index scans that stop on hnsw.max_scan_tuples or on the scan
# memory cap, MemoryContextMemAllocated(so->tmpCtx) > work_mem *
# hnsw.scan_mem_multiplier (hnswscan.c:264). The filter matches 10 of 1000
# rows and each scan asks for 11, so every scan runs until a limit stops it;
# the rows it returns and "Rows Removed by Filter" show where it stopped.
#
# usage: run-iterscan.sh {pgrust|ref}
#        run-iterscan.sh --list
#   pgrust: the subject is pgrust (SET pgrust.hnsw_build_seed = 42).
#   ref:    the subject is a second, fresh seeded C cluster (must always pass).
#   PGV_KEEP_DATA=1 keeps each side's cluster under $PGV_WORK/iterscan/<mode>/data
#           (default: deleted after a successful side, to save disk).
set -euo pipefail
. "$(dirname "$0")/common.sh"

# name:hnsw.iterative_scan:work_mem:hnsw.scan_mem_multiplier:hnsw.max_scan_tuples:query
CASES="relaxed_64k:relaxed_order:64kB:1:20000:knn
relaxed_64k_x2:relaxed_order:64kB:2:20000:knn
strict_64k:strict_order:64kB:1:20000:knn
relaxed_128k:relaxed_order:128kB:1:20000:knn
relaxed_tuples:relaxed_order:4MB:1:300:knn
strict_tuples:strict_order:4MB:1:300:knn
lateral_64k:relaxed_order:64kB:1:20000:lateral"

# Tie-free Golomb-ruler data (see run-bytecmp.sh l2_golomb): the graph and
# every scan order follow from the seed-42 level stream alone.
setup_sql() {
  cat <<'SQL'
SET max_parallel_maintenance_workers = 0;
SET maintenance_work_mem = '64MB';
SET pgrust.hnsw_build_seed = 42;
CREATE EXTENSION vector;
CREATE TABLE tst (i int4, v vector(3));
INSERT INTO tst SELECT i, ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector FROM (SELECT i, (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX tst_v_idx ON tst USING hnsw (v vector_l2_ops);
SQL
}

query_sql() {
  case "$1" in
    knn) echo "SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> '[1000000,0,0]' LIMIT 11" ;;
    # Three rescans of one index scan (review focus 4: each must start from a
    # reset tmpCtx and fresh visited/discarded state).
    lateral) echo "SELECT q.x, t.i FROM (VALUES (1000000), (250000), (1750000)) q(x) CROSS JOIN LATERAL (SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    *) die "unknown query '$1'" ;;
  esac
}

case_sql() { # case_sql ITERATIVE WORK_MEM MULTIPLIER MAX_TUPLES QUERY
  cat <<SQL
SET enable_seqscan = off;
SET hnsw.iterative_scan = $1;
SET work_mem = '$2';
SET hnsw.scan_mem_multiplier = $3;
SET hnsw.max_scan_tuples = $4;
EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF, BUFFERS OFF) $(query_sql "$5");
$(query_sql "$5");
SQL
}

# run_side LABEL KIND: fresh cluster, build the index, run every case, stop.
# Keeps the "Rows Removed by Filter" lines and the result rows.
run_side() {
  local label="$1" kind="$2" port name iter wm mult maxt query
  port="$(port_for "$kind")"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  setup_sql | pgv_psql "$port" -f - >"$out/$label.setup.log" 2>&1 ||
    die "$label ($kind): setup failed; see $out/$label.setup.log"
  if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$out/$label.setup.log"; then
    die "$label: index was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
  fi
  while IFS=: read -r name iter wm mult maxt query; do
    case_sql "$iter" "$wm" "$mult" "$maxt" "$query" >"$out/$label.$name.sql"
    pgv_psql "$port" -At -f "$out/$label.$name.sql" 2>"$out/$label.$name.err" |
      sed -n -e 's/^ *\(Rows Removed by Filter: [0-9]*\)$/\1/p' -e '/^[0-9][0-9|]*$/p' \
        >"$out/$label.$name.out" ||
      die "$label ($kind): case $name failed; see $out/$label.$name.err"
  done <<EOF
$CASES
EOF
  server_stop "$kind"
  # Nothing reads the cluster after this; PGV_KEEP_DATA=1 keeps it.
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$label"
}

if [ "${1:-}" = --list ]; then
  echo "$CASES" | cut -d: -f1
  exit 0
fi
[ "$#" -eq 1 ] || die "usage: run-iterscan.sh {pgrust|ref}"
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
out="$PGV_WORK/iterscan/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
total=0
passed=0
while IFS=: read -r name _; do
  total=$((total + 1))
  if [ -s "$out/oracle.$name.out" ] && cmp -s "$out/oracle.$name.out" "$out/subject.$name.out"; then
    result=ok
    passed=$((passed + 1))
  else
    result=FAIL
    diff "$out/oracle.$name.out" "$out/subject.$name.out" >"$out/diff/$name.txt" || true
  fi
  printf '%s\t%s\n' "$name" "$result" | tee -a "$out/summary.tsv"
done <<EOF
$CASES
EOF
echo "iterscan ($mode, subject $subject): $passed/$total identical; diffs in $out/diff"
[ "$passed" -eq "$total" ]

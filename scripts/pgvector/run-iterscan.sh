#!/usr/bin/env bash
# Iterative-scan tier (spec §8.2). Builds the same seeded HNSW indexes on the
# seeded C reference (the oracle) and on the subject, then runs iterative
# index scans that stop on hnsw.max_scan_tuples or on the scan memory cap,
# MemoryContextMemAllocated(so->tmpCtx) > work_mem *
# hnsw.scan_mem_multiplier (hnswscan.c:264). Each filter matches 10 rows and
# each scan asks for 11, so every scan runs until a limit stops it; the rows
# it returns and "Rows Removed by Filter" show where it stopped.
# Each case also compares the scan's memory trace. The seeded C build
# (-DHNSW_MEMORY) prints "memory: <KB> KB, tuples: <n>" after every search
# batch (ShowMemoryUsage, hnswscan.c:126-129, called at :243 and :290), and
# pgrust prints the same line with SET pgrust.hnsw_memory = on. So the
# modeled tmpCtx is compared after every batch, not only where a cap stops.
#
# usage: run-iterscan.sh {pgrust|ref} [case ...]   (default: every case)
#        run-iterscan.sh --list
#   pgrust: the subject is pgrust (SET pgrust.hnsw_build_seed = 42).
#   ref:    the subject is a second, fresh seeded C cluster (must always pass).
#   PGV_ITERSCAN_MEMORY= (empty) omits SET pgrust.hnsw_memory (self-test of
#           the opt-in trace; default on).
#   PGV_KEEP_DATA=1 keeps each side's cluster under $PGV_WORK/iterscan/<mode>/data
#           (default: deleted after a successful side, to save disk).
set -euo pipefail
. "$(dirname "$0")/common.sh"

# name:hnsw.iterative_scan:work_mem:hnsw.scan_mem_multiplier:hnsw.max_scan_tuples:hnsw.ef_search:query
CASES="relaxed_64k:relaxed_order:64kB:1:20000:40:knn
relaxed_64k_x2:relaxed_order:64kB:2:20000:40:knn
strict_64k:strict_order:64kB:1:20000:40:knn
relaxed_128k:relaxed_order:128kB:1:20000:40:knn
relaxed_tuples:relaxed_order:4MB:1:300:40:knn
strict_tuples:strict_order:4MB:1:300:40:knn
lateral_64k:relaxed_order:64kB:1:20000:40:lateral
big_relaxed_64k:relaxed_order:64kB:1:20000:40:bigknn
big_relaxed_256k:relaxed_order:256kB:1:20000:40:bigknn
big_strict_128k:strict_order:128kB:1:20000:40:bigknn
big_ef100_128k:relaxed_order:128kB:1:20000:100:bigknn
big_lateral_128k:relaxed_order:128kB:1:20000:40:biglateral
big_exhaust_4MB:relaxed_order:4MB:1:20000:40:bigknn
halfvec_64k:relaxed_order:64kB:1:20000:40:halfvec_l2
halfvec_cosine_64k:relaxed_order:64kB:1:20000:40:halfvec_cosine
sparsevec_64k:relaxed_order:64kB:1:20000:40:sparsevec_l2
sparsevec_cosine_drop_64k:relaxed_order:64kB:1:20000:40:sparsevec_cosine_drop
bit_hamming_64k:relaxed_order:64kB:1:20000:40:bit_hamming
bit_jaccard_64k:relaxed_order:64kB:1:20000:40:bit_jaccard"
MEMORY="${PGV_ITERSCAN_MEMORY-on}"

# tst: tie-free Golomb-ruler data (see run-bytecmp.sh l2_golomb), so the
# graph and every scan order follow from the seed-42 level stream alone.
# big: the same construction at 10,000 rows (p = 10007). Its values pass
# 2^24, so f32 rounds them, identically on both sides. Its index is
# byte-identical to C's (run-bytecmp.sh big_golomb), and C's in-memory graph
# spans several 1 MB blocks.
# hv, sv, bv: one table per type, each with two indexes (the ORDER BY
# operator picks one). Their data has ties, which pop in C's pairing-heap
# order (M1), and every build reseeds with 42.
setup_sql() {
  cat <<'SQL'
SET max_parallel_maintenance_workers = 0;
SET maintenance_work_mem = '64MB';
SET pgrust.hnsw_build_seed = 42;
CREATE EXTENSION vector;
CREATE TABLE tst (i int4, v vector(3));
INSERT INTO tst SELECT i, ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector FROM (SELECT i, (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX tst_v_idx ON tst USING hnsw (v vector_l2_ops);
CREATE TABLE big (i int4, v vector(3));
INSERT INTO big SELECT i, ARRAY[2*10007*k + (k*k) % 10007, 0, 0]::vector FROM (SELECT i, (i * 37) % 10007 AS k FROM generate_series(0, 9999) i) s;
CREATE INDEX big_v_idx ON big USING hnsw (v vector_l2_ops);
CREATE TABLE hv (i int4, v halfvec(3));
INSERT INTO hv SELECT i, ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31] FROM generate_series(0, 999) i;
CREATE INDEX hv_l2_idx ON hv USING hnsw (v halfvec_l2_ops);
CREATE INDEX hv_cosine_idx ON hv USING hnsw (v halfvec_cosine_ops);
CREATE TABLE sv (i int4, v sparsevec(3));
INSERT INTO sv SELECT i, ARRAY[i % 17, 1 + (i * 7) % 23, (i * 13) % 31]::vector::sparsevec FROM generate_series(0, 999) i;
CREATE INDEX sv_l2_idx ON sv USING hnsw (v sparsevec_l2_ops);
CREATE INDEX sv_cosine_idx ON sv USING hnsw (v sparsevec_cosine_ops);
CREATE TABLE bv (i int4, v bit(64));
INSERT INTO bv SELECT i, ('x' || substr(md5(i::text), 1, 16))::bit(64) FROM generate_series(0, 999) i;
CREATE INDEX bv_hamming_idx ON bv USING hnsw (v bit_hamming_ops);
CREATE INDEX bv_jaccard_idx ON bv USING hnsw (v bit_jaccard_ops);
SQL
}

query_sql() {
  case "$1" in
    knn) echo "SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> '[1000000,0,0]' LIMIT 11" ;;
    # Three rescans of one index scan (M1 review focus 4: each must start from
    # a reset tmpCtx and fresh visited/discarded state).
    lateral) echo "SELECT q.x, t.i FROM (VALUES (1000000), (250000), (1750000)) q(x) CROSS JOIN LATERAL (SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    bigknn) echo "SELECT i FROM big WHERE i % 1000 = 0 ORDER BY v <-> '[100000000,0,0]' LIMIT 11" ;;
    # Every one of the three loops hits the 128kB cap.
    biglateral) echo "SELECT q.x, t.i FROM (VALUES (100000000), (25000000), (175000000)) q(x) CROSS JOIN LATERAL (SELECT i FROM big WHERE i % 1000 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    halfvec_l2) echo "SELECT i FROM hv WHERE i % 100 = 0 ORDER BY v <-> '[1000,1000,1000]' LIMIT 11" ;;
    halfvec_cosine) echo "SELECT i FROM hv WHERE i % 100 = 0 ORDER BY v <=> '[1,2,3]' LIMIT 11" ;;
    sparsevec_l2) echo "SELECT i FROM sv WHERE i % 100 = 0 ORDER BY v <-> '{1:1000,2:1000,3:1000}/3' LIMIT 11" ;;
    # 1e-30 normalizes to zero: sparsevec_l2_normalize's zero-dropping
    # path, charged to tmpCtx as two vectors and a pfree (Review Focus 2).
    sparsevec_cosine_drop) echo "SELECT i FROM sv WHERE i % 100 = 0 ORDER BY v <=> '{1:3e38,3:1e-30}/3' LIMIT 11" ;;
    bit_hamming) echo "SELECT i FROM bv WHERE i % 100 = 0 ORDER BY v <~> ('x' || substr(md5('q'), 1, 16))::bit(64) LIMIT 11" ;;
    bit_jaccard) echo "SELECT i FROM bv WHERE i % 100 = 0 ORDER BY v <%> ('x' || substr(md5('q'), 1, 16))::bit(64) LIMIT 11" ;;
    *) die "unknown query '$1'" ;;
  esac
}

case_sql() { # case_sql ITERATIVE WORK_MEM MULTIPLIER MAX_TUPLES EF_SEARCH QUERY
  # C keeps pgrust.hnsw_memory as an unused placeholder.
  [ -z "$MEMORY" ] || echo "SET pgrust.hnsw_memory = $MEMORY;"
  cat <<SQL
SET enable_seqscan = off;
SET hnsw.iterative_scan = $1;
SET work_mem = '$2';
SET hnsw.scan_mem_multiplier = $3;
SET hnsw.max_scan_tuples = $4;
SET hnsw.ef_search = $5;
EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF, BUFFERS OFF) $(query_sql "$6");
$(query_sql "$6");
SQL
}

# The CASES line of case NAME.
case_line() {
  local line
  line="$(printf '%s\n' "$CASES" | grep "^$1:" || true)"
  [ -n "$line" ] || die "unknown case '$1' (see run-iterscan.sh --list)"
  echo "$line"
}

# run_side LABEL KIND: fresh cluster, build the indexes, run every case, stop.
# Keeps the "Rows Removed by Filter" lines and the result rows, then appends
# the memory trace in emission order (psql prints INFO on stderr).
run_side() {
  local label="$1" kind="$2" port c name iter wm mult maxt ef query
  port="$(port_for "$kind")"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  setup_sql | pgv_psql "$port" -f - >"$out/$label.setup.log" 2>&1 ||
    die "$label ($kind): setup failed; see $out/$label.setup.log"
  if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$out/$label.setup.log"; then
    die "$label: index was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
  fi
  for c in "${cases[@]}"; do
    IFS=: read -r name iter wm mult maxt ef query <<EOF
$(case_line "$c")
EOF
    case_sql "$iter" "$wm" "$mult" "$maxt" "$ef" "$query" >"$out/$label.$name.sql"
    pgv_psql "$port" -At -f "$out/$label.$name.sql" 2>"$out/$label.$name.err" |
      sed -n -e 's/^ *\(Rows Removed by Filter: [0-9]*\)$/\1/p' -e '/^[0-9][0-9|]*$/p' \
        >"$out/$label.$name.out" ||
      die "$label ($kind): case $name failed; see $out/$label.$name.err"
    sed -n 's/^.*INFO:  \(memory: [0-9]* KB, tuples: [0-9]*\)$/\1/p' \
      "$out/$label.$name.err" >>"$out/$label.$name.out"
    if [ "$kind" = seeded ] && ! grep -q '^memory: ' "$out/$label.$name.out"; then
      die "$label: case $name printed no 'memory:' lines (is $PG_VEC_SEEDED the -DHNSW_MEMORY build?)"
    fi
  done
  server_stop "$kind"
  # Nothing reads the cluster after this; PGV_KEEP_DATA=1 keeps it.
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$label"
}

if [ "${1:-}" = --list ]; then
  printf '%s\n' "$CASES" | cut -d: -f1
  exit 0
fi
[ "$#" -ge 1 ] || die "usage: run-iterscan.sh {pgrust|ref} [case ...]"
mode="$1"
shift
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
if [ "$#" -gt 0 ]; then
  cases=("$@")
else
  # shellcheck disable=SC2207 # intentional splitting: case names have no spaces
  cases=($(printf '%s\n' "$CASES" | cut -d: -f1))
fi
for c in "${cases[@]}"; do case_line "$c" >/dev/null; done
out="$PGV_WORK/iterscan/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
total=0
passed=0
for name in "${cases[@]}"; do
  total=$((total + 1))
  if [ -s "$out/oracle.$name.out" ] && cmp -s "$out/oracle.$name.out" "$out/subject.$name.out"; then
    result=ok
    passed=$((passed + 1))
  else
    result=FAIL
    diff "$out/oracle.$name.out" "$out/subject.$name.out" >"$out/diff/$name.txt" || true
  fi
  printf '%s\t%s\n' "$name" "$result" | tee -a "$out/summary.tsv"
done
echo "iterscan ($mode, subject $subject): $passed/$total identical; diffs in $out/diff"
[ "$passed" -eq "$total" ]

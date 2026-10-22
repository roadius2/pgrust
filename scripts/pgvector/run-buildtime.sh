#!/usr/bin/env bash
# Build time against C pgvector (M6; spec §9: "build time against pgvector
# reported"). A report, not a gate: HNSW and IVFFlat builds of one table on a
# fresh C reference and a fresh subject, serially and with workers forced
# through the parallel_workers reloption (0 = serial).
# usage: run-buildtime.sh {pgrust|ref}
#   pgrust: the subject is pgrust.  ref: the subject is the seeded C build
#   (C against C, the self-test's run).
#   PGV_BUILDTIME_ROWS (default 100000) and PGV_BUILDTIME_DIM (default 128)
#   size the table; PGV_BUILDTIME_WORKERS (default "0 1 2 4") lists the
#   worker counts. One run per cell; the builds run one at a time.
#   Report: $PGV_WORK/buildtime/<mode>/report.md, rows in summary.tsv.
set -euo pipefail
. "$(dirname "$0")/common.sh"

usage() { die "usage: run-buildtime.sh {pgrust|ref}"; }
[ "$#" -eq 1 ] || usage
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) usage ;;
esac
rows="${PGV_BUILDTIME_ROWS:-100000}"
dim="${PGV_BUILDTIME_DIM:-128}"
workers_list="${PGV_BUILDTIME_WORKERS:-0 1 2 4}"
out="$PGV_WORK/buildtime/$mode"
rm -rf "$out"
mkdir -p "$out"

# The table: deterministic float values, the same on both servers.
setup_sql() {
  cat <<SQL
CREATE EXTENSION vector;
CREATE TABLE bt (id int, v vector($dim));
INSERT INTO bt SELECT i, ARRAY(SELECT sin(i * 12.9898 + j * 78.233) FROM generate_series(1, $dim) j)::vector($dim)
  FROM generate_series(1, $rows) i;
VACUUM ANALYZE bt;
SQL
}

# One build on the server at port $1: access method $2 with $3 workers
# forced. Prints "<ms>\t<workers launched>".
time_build() {
  local port="$1" am="$2" workers="$3" ddl log ms launched
  case "$am" in
    hnsw) ddl="CREATE INDEX bt_idx ON bt USING hnsw (v vector_l2_ops);" ;;
    ivfflat) ddl="CREATE INDEX bt_idx ON bt USING ivfflat (v vector_l2_ops) WITH (lists = 100);" ;;
    *) die "unknown access method '$am'" ;;
  esac
  log="$out/$am-$workers-$port.log"
  pgv_psql "$port" >"$log" 2>&1 <<SQL || die "the $am build with $workers workers failed on port $port; see $log"
SET max_parallel_maintenance_workers = $workers;
ALTER TABLE bt SET (parallel_workers = $workers);
SET maintenance_work_mem = '2GB';
SET client_min_messages = debug1;
\\timing on
$ddl
\\timing off
RESET client_min_messages;
DROP INDEX bt_idx;
ALTER TABLE bt RESET (parallel_workers);
SQL
  ms="$(sed -n 's/^Time: \([0-9.]*\) ms.*/\1/p' "$log" | head -1)"
  launched="$(sed -n 's/.*DEBUG:  using \([0-9]*\) parallel workers$/\1/p' "$log" | head -1)"
  [ -n "$ms" ] || die "no timing for the $am build with $workers workers on port $port; see $log"
  printf '%s\t%s\n' "$ms" "${launched:-0}"
}

trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  port="$(port_for "$side")"
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$port"
  setup_sql | pgv_psql "$port" >"$out/setup-$side.log" 2>&1 || die "loading the table failed on $side; see $out/setup-$side.log"
done

: >"$out/summary.tsv"
for am in hnsw ivfflat; do
  for w in $workers_list; do
    c="$(time_build "$(port_for ref)" "$am" "$w")"
    s="$(time_build "$(port_for "$subject")" "$am" "$w")"
    printf '%s\t%s\t%s\t%s\n' "$am" "$w" "$c" "$s" >>"$out/summary.tsv"
  done
done

{
  echo "# Build time: $subject against C pgvector 0.8.7"
  echo
  echo "$rows rows of vector($dim); HNSW m = 16, ef_construction = 64; IVFFlat lists = 100;" \
    "maintenance_work_mem = 2GB; $(uname -sm), $(ncpu) cores. One run per cell; workers forced" \
    "with the parallel_workers reloption (0 = serial)."
  echo
  echo "| AM | workers | C launched | C ms | $subject launched | $subject ms | $subject / C |"
  echo "|---|---|---|---|---|---|---|"
  awk -F'\t' '{ printf "| %s | %s | %s | %.1f | %s | %.1f | %.2f |\n", $1, $2, $4, $3, $6, $5, ($3 > 0 ? $5 / $3 : 0) }' "$out/summary.tsv"
} >"$out/report.md"
cat "$out/report.md"

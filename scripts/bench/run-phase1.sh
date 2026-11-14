#!/usr/bin/env bash
# Phase 1 of the borg benchmark for one contestant: every measured step, in
# the plan's order, from the host. Runs on borg2, shipped there by sync.sh:
#   ssh -n borg2 'bash ~/bench/scripts/run-phase1.sh CONTESTANT RUN_ID [options]'
# Long runs go under systemd-run on the host (see the README).
#
#   run-phase1.sh CONTESTANT RUN_ID [--subset N] [--skip-soak]
#     CONTESTANT   pg-c (pgrust-db-c) or pgrust (pgrust-db-rust)
#     --subset N   the dry run: load.py --subset N, --queries 200, pgbench -T 5,
#                  builds once, 2,000 inserts per client count, a 60 s soak
#     --skip-soak  no soak
#   PHASE1_SOAK_ONLY=1 in the environment runs only the start, the soak and
#   the facts (a test of the soak path, with --subset a 60 s one)
#
# Results go to /home/jody/bench/results/RUN_ID/: CONTESTANT.tsv (one row per
# measurement, header below), CONTESTANT.log (this script's output),
# -version.txt, -settings.txt, -explain-*.txt, -build-*.log, -pgbench-*.log,
# -soak.log, -soak-stats.txt, -soak-rss.tsv, -soak-txlogs.tar.gz and
# facts/CONTESTANT/ (borg.sh facts, the scripts' REVISION, the options).
#
# Sequence (per dataset D: glove-100-angular, then sift-128-euclidean):
#   start the contestant alone; version and settings
#   D: load x3 (fresh database each), VACUUM ANALYZE; exact scans x3 + 8
#      clients; HNSW builds x3 serial, x3 with 4 workers (the last kept),
#      search sweep (ef_search 40 64 100 200 400 at 1, 8, 16 clients),
#      inserts at 1 and 8 clients; on glove the same for items_h (halfvec);
#      IVFFlat builds and sweep (probes 10 20 40 80) on items
#   plain SQL: pgbench -i -s 100, -S and TPC-B-like, 8 clients
#   soak: 1 h of 9:1 search/insert at 8 clients over a fresh glove HNSW index
#   facts, stop the contestant
#
# Every measured step is bracketed by metrics_reset/metrics_read on the
# database container's cgroup (lib.sh): peak_bytes is the peak since the
# reset, cpu_usec the container's CPU time used during the step. Both are
# taken in this shell, which the memory.peak helper requires. settle runs
# before each load, each index build, the first point of each sweep (the exact
# passes count as one), each insert step, the plain-SQL step and the soak.
# A pgbench step that exits non-zero or prints "aborted"/"error", and a
# recall, load or build that fails, is a row MEASUREMENT_error whose value is
# the exit status; the run goes on.
#
# TSV columns: dataset type index measurement setting clients rep value unit
# recall p50_ms p95_ms p99_ms peak_bytes cpu_usec wall_s started_at
#   load          value rows/s of items (load.py), wall_s the whole load
#   exact         clients 1: recall.py --exact (qps); clients 8: pgbench tps
#   build         setting serial|parallel4, value ms (psql \timing)
#   index_size    bytes after that build (pg_relation_size)
#   search        setting = ef_search or probes; clients 1: recall.py (qps,
#                 recall, latencies); clients 8/16: pgbench tps with p50/p99
#                 from its transaction log
#   insert        clients 1|8, value rows/s (pgbench tps, one row each)
#   vacuum        wall_s of the DELETE + VACUUM after an insert step
#   pgbench_init  value s; pgbench_select, pgbench_tpcb value tps
#   soak          setting window-N, clients = clients logging in the window,
#                 value tps; soak_errors value = failed transactions (scaled)
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib.sh
. "$here/lib.sh"

HEADER="dataset type index measurement setting clients rep value unit recall p50_ms p95_ms p99_ms peak_bytes cpu_usec wall_s started_at"
CLIENT="pgrust-client"
RESULTS="/home/jody/bench/results"

usage() {
  sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2
  exit 2
}

# --- arguments and run parameters --------------------------------------------

[ $# -ge 2 ] || usage
CONTESTANT="$1"
RUN_ID="$2"
shift 2
SUBSET=""
SKIP_SOAK=0
while [ $# -gt 0 ]; do
  case "$1" in
    --subset) [ $# -ge 2 ] || usage; SUBSET="$2"; shift 2 ;;
    --skip-soak) SKIP_SOAK=1; shift ;;
    *) usage ;;
  esac
done
case "$CONTESTANT" in
  pg-c) DB="pgrust-db-c" ;;
  pgrust) DB="pgrust-db-rust" ;;
  *) usage ;;
esac
case "$RUN_ID" in
  "" | */* | .*) die "bad RUN_ID '$RUN_ID'" ;;
esac

REPS=3                       # loads, exact passes
if [ -n "$SUBSET" ]; then
  EXACT_QUERIES=200 QUERIES=200 T=5 PLAIN_T=5 BUILD_REPS=1
  INSERT_1=2000 INSERT_8=250
  SOAK_T=60 SOAK_WINDOW=20 SOAK_VACUUM=20
else
  EXACT_QUERIES=1000 QUERIES=10000 T=30 PLAIN_T=60 BUILD_REPS=3
  INSERT_1=100000 INSERT_8=12500
  SOAK_T=3600 SOAK_WINDOW=300 SOAK_VACUUM=600
fi
HNSW_EF="40 64 100 200 400"
IVF_PROBES="10 20 40 80"

OUT="$RESULTS/$RUN_ID"
P="$OUT/$CONTESTANT"         # prefix of every evidence file
TSV="$P.tsv"
TMP="$OUT/.tmp-$CONTESTANT"
mkdir -p "$OUT"
[ ! -e "$TSV" ] || die "$TSV exists: use another RUN_ID"
exec > >(tee -a "$P.log") 2>&1
echo "$HEADER" | tr ' ' '\t' >"$TSV"
trap 'rc=$?; echo "[$(now)] run-phase1.sh $CONTESTANT $RUN_ID: exit status $rc"' EXIT

# --- helpers -------------------------------------------------------------------

say() { echo "[$(now)] $*"; }

# Seconds since $1 (date +%s.%N).
since() { awk -v a="$1" -v b="$(date +%s.%N)" 'BEGIN { printf "%.3f", b - a }'; }

# psql as bench in the bench database, from the client: identical for both.
csql() { X "$CLIENT" psql "$DSN" -X -v ON_ERROR_STOP=1 -At "$@"; }

# psql as the superuser postgres inside the database container.
admin() {
  case "$CONTESTANT" in
    pg-c) X "$DB" sudo -u postgres psql -X -v ON_ERROR_STOP=1 -At "$@" ;;
    pgrust) X "$DB" runuser -u pgrust -- psql -X -h /tmp -U postgres -v ON_ERROR_STOP=1 -At "$@" ;;
  esac
}

# The measurement bracket. measure_begin opens a memory.peak window and notes
# the CPU time; measure_end sets M_PEAK, M_CPU (the step's usec), M_WALL.
measure_begin() {
  local m
  M_START="$(now)"
  metrics_reset "$DB"
  m="$(metrics_read "$DB")" || die "metrics_read $DB failed"
  M_CPU0="${m##*cpu_usec=}"
  M_T0="$(date +%s.%N)"
}

measure_end() {
  local m
  M_WALL="$(since "$M_T0")"
  m="$(metrics_read "$DB")" || die "metrics_read $DB failed"
  M_PEAK="${m#peak=}"
  M_PEAK="${M_PEAK%% *}"
  M_CPU=$(( ${m##*cpu_usec=} - M_CPU0 ))
}

# measure CMD...: run CMD inside the bracket; M_RC is its exit status.
measure() {
  measure_begin
  M_RC=0
  "$@" || M_RC=$?
  measure_end
}

# A row with no metrics (index_size): only the time it was taken.
unmeasured() { M_PEAK="" M_CPU="" M_WALL="" M_START="$(now)"; }

# emit MEASUREMENT SETTING CLIENTS REP VALUE UNIT [RECALL P50 P95 P99]: one TSV
# row for the current DS/TY/IX with the last bracket's metrics.
emit() {
  row "$TSV" "$DS" "$TY" "$IX" "$1" "$2" "$3" "$4" "$5" "$6" "${7:-}" "${8:-}" "${9:-}" \
    "${10:-}" "$M_PEAK" "$M_CPU" "$M_WALL" "$M_START"
}

# emit_error MEASUREMENT SETTING CLIENTS REP STATUS
emit_error() {
  say "ERROR-ROW ${1}_error setting=$2 clients=$3 rep=$4 status=$5"
  emit "${1}_error" "$2" "$3" "$4" "$5" status
}

# to FILE CMD...: CMD's stdout to FILE; to_all also its stderr.
to() { local f="$1"; shift; "$@" >"$f"; }
to_all() { local f="$1"; shift; "$@" >"$f" 2>&1; }

# --- the server ------------------------------------------------------------------

start_contestant() {
  local ip _
  bash "$here/borg.sh" stop-others "$DB"
  ip="$(bash "$here/borg.sh" start "$DB" | awk '{ print $2 }')"
  [ -n "$ip" ] || die "no address for $DB"
  DSN="postgresql://bench:bench@$ip/bench?sslmode=disable"
  say "$CONTESTANT: $DB at $ip"
  for _ in $(seq 1 120); do
    X "$CLIENT" pg_isready -q -h "$ip" -p 5432 -U bench -d bench && return 0
    sleep 1
  done
  die "$DB is not accepting connections after 120 s"
}

record_server() {
  {
    csql -c "SELECT version()" -c "SELECT 'vector ' || extversion FROM pg_extension WHERE extname = 'vector'"
    # shellcheck disable=SC2016 # dpkg-query's format, not the shell's
    case "$CONTESTANT" in
      pg-c) X "$DB" dpkg-query -W -f='${Package} ${Version}\n' postgresql-18 postgresql-18-pgvector ;;
      pgrust) X "$DB" cat /opt/pgrust/BUILD.txt ;;
    esac
  } >"$P-version.txt"
  {
    csql -F $'\t' -c "SELECT name, setting, unit, source FROM pg_settings WHERE source <> 'default' ORDER BY name"
    # The vector GUCs exist on C only once the library is loaded, so they are
    # checked through the startup packet (as every sweep sets them).
    echo "## PGOPTIONS='-c hnsw.ef_search=40 -c ivfflat.probes=10': SHOW hnsw.ef_search, ivfflat.probes"
    X "$CLIENT" env PGOPTIONS="-c hnsw.ef_search=40 -c ivfflat.probes=10" \
      psql "$DSN" -X -v ON_ERROR_STOP=1 -At -c "SHOW hnsw.ef_search" -c "SHOW ivfflat.probes"
  } >"$P-settings.txt"
  cat "$P-version.txt"
}

# DROP DATABASE needs every session to it gone; the client steps are all
# synchronous, so this waits only for backends still exiting.
fresh_db() {
  local n _
  for _ in $(seq 1 60); do
    n="$(admin -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'bench'")"
    [ "$n" = 0 ] && break
    sleep 1
  done
  admin -c "DROP DATABASE IF EXISTS bench"
  admin -c "CREATE DATABASE bench OWNER bench"
  admin -d bench -c "CREATE EXTENSION vector"
}

# --- client steps (run inside the measurement bracket) -----------------------------

# client_pgbench LOG PGOPTIONS TXLOG ARGS...: pgbench ARGS DSN in the client,
# in a scratch directory; with TXLOG=1 it writes -l logs and soak_stats.py
# prints their latency percentiles after a "== latency" line.
client_pgbench() {
  local log="$1" opts="$2" txlog="$3" cmd
  shift 3
  cmd="PGOPTIONS=$(printf %q "$opts") $(printf '%q ' pgbench "$@")"
  if [ "$txlog" = 1 ]; then
    cmd="$cmd -l $(printf %q "$DSN"); rc=\$?; echo '== latency'; /opt/bench/soak_stats.py --window 1000000 pgbench_log.*"
  else
    cmd="$cmd $(printf %q "$DSN"); rc=\$?"
  fi
  XB "$CLIENT" "d=\$(mktemp -d) && cd \"\$d\" || exit 99; $cmd; cd / && rm -rf \"\$d\"; exit \$rc" >"$log" 2>&1
}

# pgbench_point MEASUREMENT SETTING CLIENTS UNIT TAG PGOPTIONS TXLOG ARGS...:
# one measured pgbench step and its row: value = pgbench's tps (UNIT tps or
# rows/s), or with UNIT s the "done in" seconds of pgbench -i.
pgbench_point() {
  local meas="$1" setting="$2" clients="$3" unit="$4" tag="$5" opts="$6" txlog="$7" log value lat
  shift 7
  log="$P-pgbench-$tag.log"
  say "pgbench $tag: $*${opts:+ (PGOPTIONS=$opts)}"
  measure client_pgbench "$log" "$opts" "$txlog" "$@"
  if [ "$M_RC" != 0 ] || grep -qE 'aborted|error' "$log"; then
    grep -E 'aborted|error' "$log" | head -5 || true # context for the run log only
    emit_error "$meas" "$setting" "$clients" 1 "$M_RC"
    return 0
  fi
  if [ "$unit" = s ]; then
    value="$(sed -n 's/^done in \([0-9.]*\) s.*/\1/p' "$log")"
  else
    value="$(awk '/^tps = / { print $3; exit }' "$log")"
  fi
  if [ -z "$value" ]; then
    say "pgbench $tag: no result in $log"
    emit_error "$meas" "$setting" "$clients" 1 "$M_RC"
    return 0
  fi
  # soak_stats.py's line: window_start tps p50_ms p99_ms errors clients
  lat="$(awk '/^== latency/ { getline; print $3, $4; exit }' "$log")"
  emit "$meas" "$setting" "$clients" 1 "$value" "$unit" "" "${lat% *}" "" "${lat#* }"
  say "  $meas $setting c$clients: $value $unit"
}

# recall_point MEASUREMENT SETTING REP ARGS...: recall.py ARGS, one row with
# value qps, recall and latencies.
recall_point() {
  local meas="$1" setting="$2" rep="$3" qps recall p50 p95 p99
  shift 3
  say "recall.py $*"
  measure to "$TMP" X "$CLIENT" /opt/bench/recall.py "$DSN" "$@"
  if [ "$M_RC" != 0 ]; then
    emit_error "$meas" "$setting" 1 "$rep" "$M_RC"
    return 0
  fi
  IFS=$'\t' read -r qps recall p50 p95 p99 < <(jq -r '[.qps, .recall, .p50_ms, .p95_ms, .p99_ms] | @tsv' "$TMP")
  emit "$meas" "$setting" 1 "$rep" "$qps" qps "$recall" "$p50" "$p95" "$p99"
  say "  $meas $setting c1 rep $rep: $qps qps, recall $recall"
}

# --- steps -----------------------------------------------------------------------

# step_load REP: a fresh database and one timed load.py of $DS. Returns 1 if
# the load failed (its row is load_error).
step_load() {
  local rep="$1" value
  local args=("$DSN" "/data/$DS.hdf5" --dim "$DIM" --metric "$METRIC")
  [ "$HALFVEC" = 1 ] && args+=(--halfvec)
  [ -n "$SUBSET" ] && args+=(--subset "$SUBSET")
  TY=vector IX=none
  fresh_db
  settle
  say "load $DS rep $rep"
  measure to "$TMP" X "$CLIENT" /opt/bench/load.py "${args[@]}"
  if [ "$M_RC" != 0 ]; then
    emit_error load "" "" "$rep" "$M_RC"
    return 1
  fi
  value="$(jq -r .rows_per_s "$TMP")"
  emit load "" "" "$rep" "$value" rows/s
  say "  load rep $rep: $value rows/s ($(jq -r .rows "$TMP") rows)"
}

# step_exact TABLE QCOL: exact-scan passes before any index exists.
step_exact() {
  local tbl="$1" qcol="$2" rep
  IX=none
  settle
  for rep in $(seq 1 "$REPS"); do
    recall_point exact "" "$rep" --table "$tbl" --op "$OP" --exact --queries "$EXACT_QUERIES"
  done
  pgbench_point exact "" 8 tps "$DS-$TY-exact-c8" "" 1 -n -c 8 -j 8 -T "$T" \
    -f /opt/bench/search.pgbench -D "tbl=$tbl" -D "qcol=$qcol" -D "op=$OP"
}

# build_once TABLE AM OPCLASS WITH SETTING WORKERS REP: one timed CREATE INDEX
# ${TABLE}_v_idx and its rows. Returns 1 if it failed.
build_once() {
  local tbl="$1" am="$2" opclass="$3" with="$4" setting="$5" workers="$6" rep="$7"
  local idx="${1}_v_idx" log ms size
  log="$P-build-$DS-$TY-$am-$setting-$rep.log"
  settle
  say "build $idx ($am, $setting) rep $rep"
  measure to_all "$log" X "$CLIENT" psql "$DSN" -X -v ON_ERROR_STOP=1 \
    -c "SET client_min_messages = debug1" \
    -c "SET max_parallel_maintenance_workers = $workers" \
    -c "SET maintenance_work_mem = '8GB'" \
    -c '\timing on' \
    -c "CREATE INDEX $idx ON $tbl USING $am (v $opclass) WITH ($with)"
  ms="$(sed -n 's/^Time: \([0-9.]*\) ms.*/\1/p' "$log" | tail -1)"
  grep -E 'DEBUG|NOTICE|ERROR|parallel' "$log" | sed 's/^/  /' || true # echo into the run log
  if [ "$M_RC" != 0 ] || [ -z "$ms" ]; then
    emit_error build "$setting" "" "$rep" "$M_RC"
    return 1
  fi
  emit build "$setting" "" "$rep" "$ms" ms
  unmeasured
  size="$(csql -c "SELECT pg_relation_size('$idx')")"
  emit index_size "$setting" "" "$rep" "$size" bytes
  say "  build $setting rep $rep: $ms ms, $size bytes"
}

# step_builds TABLE AM OPCLASS WITH: BUILD_REPS serial then BUILD_REPS with 4
# workers, dropping each index but the last. Returns 1 if the last one failed.
step_builds() {
  local tbl="$1" am="$2" opclass="$3" with="$4" setting workers rep last=0
  IX="$am"
  for setting in serial parallel4; do
    workers=0
    [ "$setting" = parallel4 ] && workers=4
    for rep in $(seq 1 "$BUILD_REPS"); do
      last=0
      build_once "$tbl" "$am" "$opclass" "$with" "$setting" "$workers" "$rep" || last=1
      if [ "$setting" != parallel4 ] || [ "$rep" != "$BUILD_REPS" ]; then
        csql -c "DROP INDEX IF EXISTS ${tbl}_v_idx"
      fi
    done
  done
  return "$last"
}

# step_explain TABLE QCOL GUC VALUE: the sweep's query shape under EXPLAIN,
# saved and checked for the index's name.
step_explain() {
  local tbl="$1" qcol="$2" guc="$3" value="$4" qv f="$P-explain-$DS-$IX"
  [ "$TY" = vector ] || f="$f-$TY"
  f="$f.txt"
  qv="$(csql -c "SELECT $qcol::text FROM queries WHERE id = 0")"
  X "$CLIENT" env PGOPTIONS="-c $guc=$value" psql "$DSN" -X -v ON_ERROR_STOP=1 \
    -c "SHOW $guc" -c "EXPLAIN (COSTS OFF) SELECT id FROM $tbl ORDER BY v $OP '$qv' LIMIT 10" \
    >"$f" 2>&1 || true # the check below reads the file either way
  sed 's/^/  /' "$f"
  if ! grep -q "${tbl}_v_idx" "$f"; then
    say "PLAN-NOT-INDEX: $f does not name ${tbl}_v_idx"
    unmeasured
    emit plan_not_index "" "" 1 1 flag
  fi
}

# step_sweep TABLE QCOL GUC VALUES...: per value, a recall pass (the warm-up
# for the pgbench points), then 8 and 16 clients for $T s.
step_sweep() {
  local tbl="$1" qcol="$2" guc="$3" v c
  shift 3
  step_explain "$tbl" "$qcol" "$guc" "$1"
  settle
  for v in "$@"; do
    recall_point search "$v" 1 --table "$tbl" --op "$OP" --queries "$QUERIES" --guc "$guc=$v"
    for c in 8 16; do
      pgbench_point search "$v" "$c" tps "$DS-$TY-$IX-$v-c$c" "-c $guc=$v" 1 -n -c "$c" -j "$c" -T "$T" \
        -f /opt/bench/search.pgbench -D "tbl=$tbl" -D "qcol=$qcol" -D "op=$OP"
    done
  done
}

# step_inserts TABLE: 1 then 8 clients of insert.pgbench, each followed by the
# removal of the inserted rows (they duplicate indexed vectors).
step_inserts() {
  local tbl="$1" c n t0 rc
  for c in 1 8; do
    n="$INSERT_1"
    [ "$c" = 8 ] && n="$INSERT_8"
    settle
    # One insert transaction is one row, so tps is rows/s.
    pgbench_point insert "" "$c" rows/s "$DS-$TY-$IX-insert-c$c" "" 0 -n -c "$c" -j "$c" -t "$n" \
      -f /opt/bench/insert.pgbench -D "tbl=$tbl"
    unmeasured
    t0="$(date +%s.%N)"
    rc=0
    csql -c "DELETE FROM $tbl WHERE id >= 100000000" -c "VACUUM $tbl" || rc=$?
    M_WALL="$(since "$t0")"
    if [ "$rc" != 0 ]; then
      emit_error vacuum "" "$c" 1 "$rc"
    else
      emit vacuum "" "$c" 1 "$M_WALL" s
    fi
  done
}

# run_hnsw TABLE QCOL OPCLASS: builds, sweep and inserts of one HNSW index.
run_hnsw() {
  local tbl="$1" qcol="$2" opclass="$3"
  say "== $DS $TY hnsw"
  if ! step_builds "$tbl" hnsw "$opclass" "m = 16, ef_construction = 64"; then
    say "SKIP: no ${tbl}_v_idx after the builds; no hnsw sweep or inserts for $DS $TY"
    return 0
  fi
  # shellcheck disable=SC2086 # the list splits into the sweep's values
  step_sweep "$tbl" "$qcol" hnsw.ef_search $HNSW_EF
  step_inserts "$tbl"
}

# run_ivfflat: IVFFlat on items, lists = rows / 1000 (1183 on glove, 1000 on
# sift, 100 on a 100,000-row subset).
run_ivfflat() {
  local rows lists
  say "== $DS $TY ivfflat"
  csql -c "DROP INDEX IF EXISTS items_v_idx"
  rows="$(csql -c "SELECT count(*) FROM items")"
  lists=$(( (rows + 500) / 1000 ))
  [ "$lists" -ge 1 ] || lists=1
  say "ivfflat lists = $lists ($rows rows)"
  if ! step_builds items ivfflat "$OPCLASS" "lists = $lists"; then
    say "SKIP: no items_v_idx after the ivfflat builds; no sweep for $DS"
    return 0
  fi
  # shellcheck disable=SC2086 # the list splits into the sweep's values
  step_sweep items v ivfflat.probes $IVF_PROBES
}

set_dataset() {
  DS="$1"
  case "$DS" in
    glove-100-angular) DIM=100 METRIC=cosine OP='<=>' OPCLASS=vector_cosine_ops HALFVEC=1 ;;
    sift-128-euclidean) DIM=128 METRIC=l2 OP='<->' OPCLASS=vector_l2_ops HALFVEC=0 ;;
  esac
}

run_dataset() {
  local rep
  set_dataset "$1"
  say "=== dataset $DS"
  for rep in $(seq 1 "$REPS"); do
    if ! step_load "$rep"; then
      say "SKIP: dataset $DS (the load failed)"
      return 0
    fi
  done
  say "VACUUM ANALYZE"
  csql -c "VACUUM ANALYZE"
  TY=vector
  step_exact items v
  if [ "$HALFVEC" = 1 ]; then
    TY=halfvec
    step_exact items_h vh
  fi
  TY=vector
  run_hnsw items v "$OPCLASS"
  if [ "$HALFVEC" = 1 ]; then
    TY=halfvec
    run_hnsw items_h vh halfvec_cosine_ops
  fi
  TY=vector
  run_ivfflat
}

step_plain() {
  DS=pgbench TY="" IX=""
  say "=== plain SQL"
  fresh_db
  settle
  pgbench_point pgbench_init "scale=100" "" s pgbench-init "" 0 -i -s 100
  pgbench_point pgbench_select "scale=100" 8 tps pgbench-select "" 0 -S -c 8 -j 8 -T "$PLAIN_T"
  pgbench_point pgbench_tpcb "scale=100" 8 tps pgbench-tpcb "" 0 -c 8 -j 8 -T "$PLAIN_T"
}

# --- the soak --------------------------------------------------------------------

SOAK_DIR="/root/soak"
SOAK_UNIT="bench-soak"

soak_vacuum() {
  local t0 rc=0
  t0="$(date +%s.%N)"
  csql -c "VACUUM items" || rc=$?
  say "soak: VACUUM items exit $rc in $(since "$t0") s"
}

# Starts pgbench in the client as a transient unit; it writes pgbench.out and,
# when done, its exit status to rc.
soak_start() {
  local cmd
  cmd="$(printf '%q ' pgbench -n -c 8 -j 8 -T "$SOAK_T" -P "$SOAK_WINDOW" -l --sampling-rate 0.1 \
    -f /opt/bench/search.pgbench@9 -f /opt/bench/insert.pgbench@1 \
    -D tbl=items -D qcol=v -D 'op=<=>' "$DSN")"
  X "$CLIENT" rm -rf "$SOAK_DIR"
  X "$CLIENT" mkdir -p "$SOAK_DIR"
  # A unit left failed by an earlier run would block the name; none is normal.
  X "$CLIENT" systemctl reset-failed "$SOAK_UNIT" 2>/dev/null || true
  X "$CLIENT" systemd-run --unit="$SOAK_UNIT" --collect -p WorkingDirectory="$SOAK_DIR" \
    --setenv=PGOPTIONS="-c hnsw.ef_search=100" \
    /bin/bash -c "$cmd >pgbench.out 2>&1; echo \$? >rc"
}

step_soak() {
  local args cg rss start next_vac rc st w tps p50 p99 e clients rest n=0 errs=0 timed_out=0
  local -a vac=()
  set_dataset glove-100-angular
  TY=vector IX=hnsw
  say "=== soak"
  args=("$DSN" "/data/$DS.hdf5" --dim "$DIM" --metric "$METRIC")
  [ -n "$SUBSET" ] && args+=(--subset "$SUBSET")
  fresh_db
  if ! X "$CLIENT" /opt/bench/load.py "${args[@]}"; then
    unmeasured
    emit_error soak "" 8 1 load
    return 0
  fi
  csql -c "VACUUM ANALYZE"
  if ! X "$CLIENT" psql "$DSN" -X -v ON_ERROR_STOP=1 -c "SET max_parallel_maintenance_workers = 4" \
    -c "SET maintenance_work_mem = '8GB'" -c '\timing on' \
    -c "CREATE INDEX items_v_idx ON items USING hnsw (v $OPCLASS) WITH (m = 16, ef_construction = 64)" \
    >"$P-build-soak.log" 2>&1; then
    unmeasured
    emit_error soak "" 8 1 build
    return 0
  fi
  settle
  cg="$(cgroup_dir "$DB")"
  rss="$P-soak-rss.tsv"
  printf 'time\tmemory_current_bytes\n' >"$rss"
  measure_begin
  if ! soak_start; then
    measure_end
    emit_error soak "" 8 1 start
    return 0
  fi
  start="$SECONDS"
  next_vac=$((start + SOAK_VACUUM))
  until X "$CLIENT" test -f "$SOAK_DIR/rc"; do
    printf '%s\t%s\n' "$(now)" "$(cat "$cg/memory.current")" >>"$rss"
    if [ "$SECONDS" -ge "$next_vac" ]; then
      soak_vacuum &
      vac+=("$!")
      next_vac=$((next_vac + SOAK_VACUUM))
    fi
    if [ $((SECONDS - start)) -gt $((SOAK_T + 900)) ]; then
      say "soak: pgbench still running $((SOAK_T + 900)) s after its start; stopping it"
      X "$CLIENT" systemctl stop "$SOAK_UNIT"
      timed_out=1
      break
    fi
    sleep 10
  done
  [ "${#vac[@]}" -eq 0 ] || wait "${vac[@]}"
  measure_end
  X "$CLIENT" cat "$SOAK_DIR/pgbench.out" >"$P-soak.log"
  rc="$(X "$CLIENT" cat "$SOAK_DIR/rc" 2>/dev/null)" || rc=timeout
  [ "$timed_out" = 0 ] || rc=timeout
  tail -n 25 "$P-soak.log"
  if [ "$rc" != 0 ] || grep -qE 'aborted|error' "$P-soak.log"; then
    emit_error soak "" 8 1 "$rc"
  fi
  st=0
  XB "$CLIENT" "cd $SOAK_DIR && /opt/bench/soak_stats.py --sampling-rate 0.1 --window $SOAK_WINDOW pgbench_log.*" \
    >"$P-soak-stats.txt" 2>&1 || st=$?
  cat "$P-soak-stats.txt"
  # window_start tps p50_ms p99_ms errors clients. The soak's bracket goes on
  # its soak_errors row, not on every window's.
  local peak="$M_PEAK" cpu="$M_CPU" wall="$M_WALL" started="$M_START"
  M_PEAK="" M_CPU="" M_WALL=""
  while read -r w tps p50 p99 e clients rest; do
    [[ "$w" =~ ^[0-9]+$ ]] && [ -n "$clients" ] && [ -z "$rest" ] || continue
    emit soak "window-$n" "$clients" 1 "$tps" tps "" "$p50" "" "$p99"
    errs="$(awk -v a="$errs" -v b="$e" 'BEGIN { print a + b }')"
    n=$((n + 1))
  done <"$P-soak-stats.txt"
  M_PEAK="$peak" M_CPU="$cpu" M_WALL="$wall" M_START="$started"
  emit soak_errors "" 8 1 "$errs" transactions
  if [ "$st" != 0 ]; then
    emit_error soak_aborted "" 8 1 "$st"
  fi
  X "$CLIENT" tar czf - -C "$SOAK_DIR" . >"$P-soak-txlogs.tar.gz"
}

# --- main ------------------------------------------------------------------------

say "run-phase1.sh $CONTESTANT $RUN_ID${SUBSET:+ --subset $SUBSET}$([ "$SKIP_SOAK" = 1 ] && echo ' --skip-soak')"
DS="" TY="" IX=""
start_contestant
record_server
settle
if [ "${PHASE1_SOAK_ONLY:-0}" != 1 ]; then
  run_dataset glove-100-angular
  run_dataset sift-128-euclidean
  step_plain
fi
if [ "$SKIP_SOAK" = 0 ]; then
  step_soak
fi
say "=== facts"
mkdir -p "$OUT/facts/$CONTESTANT"
bash "$here/borg.sh" facts "$OUT/facts/$CONTESTANT"
cp "$here/REVISION" "$OUT/facts/$CONTESTANT/REVISION" 2>/dev/null ||
  echo "no REVISION (shipped without sync.sh?)" >"$OUT/facts/$CONTESTANT/REVISION"
echo "run-phase1.sh $CONTESTANT $RUN_ID${SUBSET:+ --subset $SUBSET} skip_soak=$SKIP_SOAK" \
  >"$OUT/facts/$CONTESTANT/options.txt"
bash "$here/borg.sh" stop "$DB"
rm -f "$TMP"
say "done: $(($(wc -l <"$TSV") - 1)) rows in $TSV"

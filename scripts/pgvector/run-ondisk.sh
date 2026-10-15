#!/usr/bin/env bash
# On-disk tier, forward (spec §8.2 "On-disk, forward").
#
# Forward (gated). C pgvector writes three data directories in Docker:
#   clean  the 0.8.7 image: the fixture, then a clean stop;
#   crash  the 0.8.7 image: the fixture, CHECKPOINT, more writes
#          (sql/ondisk-crash.sql), a transaction left open
#          (sql/ondisk-inflight.sql), then pg_ctl stop -m immediate;
#   v081   the 0.8.1 image: the fixture, then a clean stop.
# C's answers come from the 0.8.7 image booting a copy of the same directory
# (for v081, through the same ALTER EXTENSION vector UPDATE). The subject
# boots another copy and must reproduce them exactly: the index queries
# (the same pages give the same traversal), then, after its own DML and
# maintenance (sql/ondisk-followup.sql), the counts, row counts and exact
# answers. After the crash, every relation the fixture owns must also replay
# to the same pages as C's replay, compared after pagemask. Rows:
#   clean_queries clean_followup crash_pages crash_queries crash_followup
#   v081_update v081_followup
#
# Every fixture value makes index scans compute exact distances
# (sql/ondisk-rows.sql), so C's answers do not depend on its compiler's
# vectorized, fused float sums (M2's no-emulation ruling stands).
#
# usage: run-ondisk.sh capture [--force]
#        run-ondisk.sh {pgrust|ref} [clean|crash|v081 ...]
#   capture: build the three directories and C's answers under
#     $PGV_WORK/ondisk/capture. Cached by a key over sql/ondisk-*.sql,
#     sql/repair-verify.sql, docker-lib.sh, this script and the image pins;
#     --force rebuilds. {pgrust|ref} capture first when the key changed.
#   pgrust: the subject is pgrust. ref: the subject is the local C reference
#     ($PG_VEC; clang, macOS), which must always pass.
#   With no parts, all three run. PGV_KEEP_DATA=1 keeps the subject's copies.
#   PGV_ONDISK_PERTURB_SQL runs on the subject right after its first boot of
#   each forward directory (harness self-test only).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"
. "$here/docker-lib.sh"

sql="$here/sql"
cap="$PGV_WORK/ondisk/capture"
DK_CONN=(-h 127.0.0.1 -p "$PGV_PORT_DOCKER")
INFLIGHT_PID=

usage() {
  die "usage: run-ondisk.sh capture [--force] | run-ondisk.sh {pgrust|ref} [clean|crash|v081 ...]"
}

cleanup() {
  if [ -n "$INFLIGHT_PID" ]; then kill "$INFLIGHT_PID" 2>/dev/null || true; fi
  dk_rm
}

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
  local xidfile="$1" i xid=
  shift
  (cd "$sql" && exec "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 "$@" -U postgres -d postgres -f ondisk-inflight.sql) >/dev/null 2>&1 &
  INFLIGHT_PID=$!
  for i in $(seq 1 600); do
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
    cat "$sql"/ondisk-*.sql "$sql/repair-verify.sql" "$here/docker-lib.sh" "$here/run-ondisk.sh"
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

result() {
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/summary.tsv"
}

# Row $1: C's transcript $2 against the subject's $3.
compare() {
  if diff -u "$2" "$3" >"$out/$1.diff"; then
    rm -f "$out/$1.diff"
    result "$1" ok
  else
    result "$1" "FAIL ($out/$1.diff)"
  fi
}

# Masked pages of every relation in $3 (name|rmgr|path, main and init forks):
# the subject's replay under $1 against C's under $2. Prints each file that
# differs or exists on one side only. Every relation has a main fork, even at
# zero blocks, so one missing from C's replay fails too; only an init fork
# may be absent on both sides. (Every fixture relation is under 1 GB, so
# there are no .1 segments.)
compare_pages() {
  local a="$1" b="$2" name rm path fork off rc=0
  while IFS='|' read -r name rm path; do
    [ -n "$path" ] || continue
    for fork in "" _init; do
      if [ ! -e "$b/$path$fork" ]; then
        if [ -z "$fork" ]; then
          echo "$name: missing from C's replay"
          rc=1
        elif [ -e "$a/$path$fork" ]; then
          echo "$name$fork: only in the subject's replay"
          rc=1
        fi
        continue
      fi
      if [ ! -e "$a/$path$fork" ]; then
        echo "$name$fork: missing from the subject's replay"
        rc=1
        continue
      fi
      "$PAGEMASK_BIN" "$rm" "$a/$path$fork" "$out/subject.masked" || die "pagemask failed on $a/$path$fork"
      "$PAGEMASK_BIN" "$rm" "$b/$path$fork" "$out/c.masked" || die "pagemask failed on $b/$path$fork"
      if ! cmp -s "$out/subject.masked" "$out/c.masked"; then
        off="$({ cmp "$out/subject.masked" "$out/c.masked" 2>&1 || true; } | sed -n 's/.* char \([0-9]*\).*/\1/p')"
        echo "$name$fork ($rm): differs from block $(((${off:-1} - 1) / 8192)), offset $(((${off:-1} - 1) % 8192))"
        rc=1
      fi
    done
  done <"$3"
  rm -f "$out/subject.masked" "$out/c.masked"
  return "$rc"
}

# Boot the subject on data directory $1, logging to $1.boot.log.
subject_start() {
  if (server_start "$subject" "$1" "$port") >>"$1.boot.log" 2>&1; then
    return 0
  fi
  server_stop "$subject"
  return 1
}

forward() {
  local f="$1" d="$out/$1" c="$cap/$1" q=ondisk-queries.sql name=queries rows r
  if [ "$f" = v081 ]; then
    q=ondisk-update.sql
    name=update
  fi
  rm -rf "$d"
  mkdir -p "$d"
  cp -R "$c/data" "$d/data"
  if ! subject_start "$d/data"; then
    if [ "$f" = crash ]; then rows="pages queries followup"; else rows="$name followup"; fi
    for r in $rows; do
      result "${f}_$r" "FAIL (boot; $d/data.boot.log, $PGV_WORK/log/server-$subject.log)"
    done
    return
  fi
  if [ -n "${PGV_ONDISK_PERTURB_SQL:-}" ]; then
    pgv_psql "$port" -c "$PGV_ONDISK_PERTURB_SQL" >/dev/null
  fi
  if [ "$f" = crash ]; then
    server_stop "$subject"
    if compare_pages "$d/data" "$c/replayed" "$c/relfiles.txt" >"$d/pages.txt"; then
      result crash_pages ok
    else
      result crash_pages "FAIL ($d/pages.txt)"
    fi
    if ! subject_start "$d/data"; then
      result crash_queries "FAIL (restart; $d/data.boot.log)"
      result crash_followup "FAIL (restart; $d/data.boot.log)"
      return
    fi
  fi
  transcript "$q" "${SUBJ_CONN[@]}" >"$d/$name.out"
  compare "${f}_$name" "$c/expected-$name.out" "$d/$name.out"
  transcript ondisk-followup.sql "${SUBJ_CONN[@]}" >"$d/followup.out"
  compare "${f}_followup" "$c/expected-followup.out" "$d/followup.out"
  server_stop "$subject"
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data"; fi
}

# --- main ---
[ "$#" -ge 1 ] || usage
mode="$1"
shift
case "$mode" in
  capture)
    trap cleanup EXIT
    case "$#:${1:-}" in
      0:) ensure_capture 0 ;;
      1:--force) ensure_capture 1 ;;
      *) usage ;;
    esac
    echo "capture: $cap"
    exit 0
    ;;
  pgrust | ref) subject="$mode" ;;
  *) usage ;;
esac
port="$(port_for "$subject")"
SUBJ_CONN=(-h "$PGV_WORK/sock" -p "$port")
if [ "$#" -gt 0 ]; then parts=("$@"); else parts=(clean crash v081); fi
for p in "${parts[@]}"; do
  case "$p" in clean | crash | v081) ;; *) usage ;; esac
done
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/ondisk/$mode"
trap 'cleanup; server_stop "$subject"' EXIT
ensure_capture 0
rm -rf "$out"
mkdir -p "$out"
: >"$out/summary.tsv"
server_stop "$subject"
for p in "${parts[@]}"; do
  forward "$p"
done
total=$(wc -l <"$out/summary.tsv" | tr -d ' ')
passed=$(grep -c $'\tok$' "$out/summary.tsv" || true)
echo "ondisk ($mode, subject $subject): $passed/$total checks passed; diffs in $out"
[ "$passed" -eq "$total" ]

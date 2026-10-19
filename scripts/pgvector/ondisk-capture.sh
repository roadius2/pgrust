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

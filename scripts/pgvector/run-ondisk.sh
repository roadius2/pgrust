#!/usr/bin/env bash
# On-disk tier (spec §8.2 "On-disk, forward" and "On-disk, reverse").
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
# Reverse (reported, never gated): the subject writes the fixture into a
# fresh cluster, once stopped cleanly and once crashed the same way, and the
# 0.8.7 image boots it ($PGV_WORK/ondisk/<mode>/reverse.tsv and reverse.md).
#
# Every fixture value keeps the distances that index scans return exact
# (sql/ondisk-rows.sql), so they do not depend on C's vectorized, fused
# float sums (M2's no-emulation ruling stands). Only IVFFlat's distances
# from a query to its k-means centers round; for the lists a probe picks,
# the evidence is the stability measurement (docs/superpowers/reports/
# pgvector-m5-report.md, "The fixture and the float question").
#
# usage: run-ondisk.sh capture [--force]
#        run-ondisk.sh {pgrust|ref} [clean|crash|v081|reverse ...]
#   capture: build the three directories and C's answers under
#     $PGV_WORK/ondisk/capture (ondisk-capture.sh, which also holds the
#     cache key); --force rebuilds. {pgrust|ref} capture first when the
#     key changed.
#   pgrust: the subject is pgrust. ref: the subject is the local C reference
#     ($PG_VEC; clang, macOS), which must always pass.
#   With no parts, all four run. PGV_KEEP_DATA=1 keeps the subject's copies.
#   PGV_ONDISK_PERTURB_SQL runs on the subject right after its first boot of
#   each forward directory (harness self-test only).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"
. "$here/docker-lib.sh"
. "$here/ondisk-capture.sh"

usage() {
  die "usage: run-ondisk.sh capture [--force] | run-ondisk.sh {pgrust|ref} [clean|crash|v081|reverse ...]"
}

cleanup() {
  if [ -n "$INFLIGHT_PID" ]; then kill "$INFLIGHT_PID" 2>/dev/null || true; fi
  dk_rm
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

# --- reverse: reported, never gated ---
# A reverse failure, even one that would end the script, becomes an
# "aborted" row; it never changes the exit status, which is the forward
# checks' alone.

rrow() {
  printf '%s\t%s\n' "$1" "$2" | tee -a "$out/reverse.tsv"
}

# Row $1: "identical" when files $2 and $3 match, else the diff's path.
rcompare() {
  if diff -u "$2" "$3" >"$4"; then
    rm -f "$4"
    rrow "$1" identical
  else
    rrow "$1" "differs ($4)"
  fi
}

# A fresh C-initdb cluster at $1; the rest are postgresql.conf lines.
fresh_cluster() {
  local data="$1"
  shift
  "$PG_TOOLS/bin/initdb" -D "$data" --no-locale --encoding UTF8 -U postgres -A trust >"$data.initdb.log" 2>&1 ||
    die "initdb failed; see $data.initdb.log"
  printf '%s\n' "$@" >>"$data/postgresql.conf"
}

reverse_clean() {
  local d="$out/reverse/clean"
  mkdir -p "$d"
  fresh_cluster "$d/data" "autovacuum = off"
  if ! subject_start "$d/data" || ! load ondisk-fixture.sql "${SUBJ_CONN[@]}" >"$d/fixture.log" 2>&1; then
    server_stop "$subject"
    rrow clean_boot "failed: the subject's fixture ($d)"
    return
  fi
  transcript ondisk-queries.sql "${SUBJ_CONN[@]}" >"$d/subject-queries.out"
  server_stop "$subject"
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" >"$d/c-boot.log" 2>&1; then
    rrow clean_boot "failed: C did not start ($d/c-boot.log)"
    dk_rm
    return
  fi
  rrow clean_boot ok
  transcript ondisk-queries.sql "${DK_CONN[@]}" >"$d/c-queries.out"
  rcompare clean_queries "$d/subject-queries.out" "$d/c-queries.out" "$d/queries.diff"
  if dk_exec pg_amcheck --install-missing --heapallindexed -d postgres >"$d/amcheck.txt" 2>&1; then
    rrow clean_amcheck clean
  else
    rrow clean_amcheck "corruption reported ($d/amcheck.txt)"
  fi
  rrow clean_pgrust_rows "$(dk_psql -At -c "SELECT coalesce(string_agg(proname, ' ' ORDER BY proname), 'none') FROM pg_proc WHERE proname LIKE 'pgrust%'")"
  transcript ondisk-followup.sql "${DK_CONN[@]}" >"$d/c-followup.out"
  rcompare clean_followup "$cap/clean/expected-followup.out" "$d/c-followup.out" "$d/followup.diff"
  dk_rm
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data"; fi
}

reverse_crash() {
  local d="$out/reverse/crash"
  mkdir -p "$d"
  fresh_cluster "$d/data" "autovacuum = off" "checkpoint_timeout = 1d" "max_wal_size = 10GB"
  if ! subject_start "$d/data" ||
    ! load ondisk-fixture.sql "${SUBJ_CONN[@]}" >"$d/fixture.log" 2>&1 ||
    ! pgv_psql "$port" -c CHECKPOINT >/dev/null 2>&1 ||
    ! load ondisk-crash.sql "${SUBJ_CONN[@]}" >"$d/crash.log" 2>&1 ||
    ! open_transaction "$d/inflight-xid" "${SUBJ_CONN[@]}" >/dev/null 2>&1; then
    server_stop "$subject"
    close_transaction
    rrow crash_boot "failed: the subject's fixture ($d)"
    return
  fi
  server_stop "$subject" QUIT
  close_transaction
  if [ "$(state "$d/data")" != "in production" ]; then
    rrow crash_boot "failed: the subject's immediate stop left '$(state "$d/data")'"
    return
  fi
  # The subject's own replay: pages after a clean stop, then the queries.
  cp -R "$d/data" "$d/subject"
  if ! subject_start "$d/subject"; then
    rrow crash_boot "failed: the subject did not replay its own crash ($d/subject.boot.log)"
    return
  fi
  # A failed or empty relation list fails crash_pages (it would compare
  # nothing); psql's errors go to relfiles.log beside it.
  pgv_psql "$port" -f "$sql/ondisk-relfiles.sql" >"$d/relfiles.txt" 2>"$d/relfiles.log" || : >"$d/relfiles.txt"
  server_stop "$subject"
  cp -R "$d/subject" "$d/subject-replayed"
  if ! subject_start "$d/subject"; then
    rrow crash_boot "failed: the subject did not restart ($d/subject.boot.log)"
    return
  fi
  transcript ondisk-queries.sql "${SUBJ_CONN[@]}" >"$d/subject-queries.out"
  server_stop "$subject"
  # C's replay of the same directory.
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$d/data" >"$d/c-boot.log" 2>&1; then
    rrow crash_boot "failed: C did not replay it ($d/c-boot.log)"
    dk_rm
    return
  fi
  rrow crash_boot ok
  if dk_stop_fast && dk_copy_out "$d/c-replayed" && dk_start; then
    if [ ! -s "$d/relfiles.txt" ]; then
      rrow crash_pages "failed: the subject's relation list ($d/relfiles.txt)"
    elif compare_pages "$d/subject-replayed" "$d/c-replayed" "$d/relfiles.txt" >"$d/pages.txt"; then
      rrow crash_pages identical
    else
      rrow crash_pages "differs ($d/pages.txt)"
    fi
    transcript ondisk-queries.sql "${DK_CONN[@]}" >"$d/c-queries.out"
    rcompare crash_queries "$d/subject-queries.out" "$d/c-queries.out" "$d/queries.diff"
  else
    rrow crash_pages "failed: C's stop, copy or restart"
  fi
  dk_rm
  if [ "${PGV_KEEP_DATA:-}" != 1 ]; then rm -rf "$d/data" "$d/subject" "$d/subject-replayed" "$d/c-replayed"; fi
}

reverse() {
  local rc
  rm -rf "$out/reverse"
  : >"$out/reverse.tsv"
  # Its own errexit scope: a die or failed command ends only the subshell,
  # whose trap removes the container and the open transaction's psql (whose
  # pid only the subshell knows). Not inside an if, || or &&, which would
  # turn errexit off in it.
  set +e
  ( set -e; trap cleanup EXIT; reverse_clean; reverse_crash )
  rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    rrow aborted "failed: exit $rc (see the run's output)"
    server_stop "$subject"
    dk_rm
  fi
  {
    printf '# On-disk, reverse (%s): reported, not gated\n\n' "$mode"
    printf 'The subject (%s) wrote the fixture into a fresh cluster; the pgvector 0.8.7 image booted it.\n\n' "$subject"
    printf '| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name r; do
      printf '| %s | %s |\n' "$name" "$r"
    done <"$out/reverse.tsv"
  } >"$out/reverse.md"
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
if [ "$#" -gt 0 ]; then parts=("$@"); else parts=(clean crash v081 reverse); fi
for p in "${parts[@]}"; do
  case "$p" in clean | crash | v081 | reverse) ;; *) usage ;; esac
done
out="$PGV_WORK/ondisk/$mode"
# Clear the old results before anything that can fail (pagemask, the
# capture), so a run that dies never leaves a previous summary.tsv behind.
rm -rf "$out"
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
trap 'cleanup; server_stop "$subject"' EXIT
ensure_capture 0
mkdir -p "$out"
: >"$out/summary.tsv"
server_stop "$subject"
for p in "${parts[@]}"; do
  case "$p" in
    reverse) reverse ;;
    *) forward "$p" ;;
  esac
done
total=$(wc -l <"$out/summary.tsv" | tr -d ' ')
passed=$(grep -c $'\tok$' "$out/summary.tsv" || true)
echo "ondisk ($mode, subject $subject): $passed/$total forward checks passed; reverse: $out/reverse.md"
[ "$passed" -eq "$total" ]

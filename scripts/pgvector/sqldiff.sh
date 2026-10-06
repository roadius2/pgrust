#!/usr/bin/env bash
# Run one SQL script on a fresh C reference and a fresh subject server and
# compare the two echoed psql sessions: M2's per-task check of each port
# (the regress, TAP and diff tiers are the gates).
# usage: sqldiff.sh {pgrust|ref} [--verbose] <file.sql | ->
#   pgrust: the subject is pgrust.  ref: the subject is the seeded C build.
#   --verbose: \set VERBOSITY verbose first, so each error's LOCATION line
#   (C function, file and line) is compared too.
#   Each side: fresh cluster, CREATE EXTENSION vector, then the script via
#   psql -a with ON_ERROR_STOP off, run from a directory holding results/
#   (the regress files' \copy target). Exit 0 when identical; otherwise the
#   unified diff is in $PGV_WORK/sqldiff/<mode>/diff.txt.
set -euo pipefail
. "$(dirname "$0")/common.sh"

usage() { die "usage: sqldiff.sh {pgrust|ref} [--verbose] <file.sql | ->"; }
[ "$#" -ge 2 ] || usage
mode="$1"
shift
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) usage ;;
esac
verbose=0
if [ "$1" = --verbose ]; then
  verbose=1
  shift
fi
[ "$#" -eq 1 ] || usage
out="$PGV_WORK/sqldiff/$mode"
rm -rf "$out"
mkdir -p "$out/results"
{
  if [ "$verbose" = 1 ]; then echo '\set VERBOSITY verbose'; fi
  if [ "$1" = - ]; then cat; else cat "$1"; fi
} >"$out/input.sql"

trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  port="$(port_for "$side")"
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$port"
  assert_identity "$side" "$port"
  pgv_psql "$port" -c 'CREATE EXTENSION vector' >/dev/null
  (cd "$out" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f input.sql) >"$out/$side.out" 2>&1 || true
  server_stop "$side"
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$side"
done
if diff -u "$out/ref.out" "$out/$subject.out" >"$out/diff.txt"; then
  echo "sqldiff ($mode): identical; transcripts in $out"
else
  echo "sqldiff ($mode): DIFFERENT; see $out/diff.txt" >&2
  head -60 "$out/diff.txt" >&2
  exit 1
fi

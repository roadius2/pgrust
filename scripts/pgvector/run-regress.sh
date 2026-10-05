#!/usr/bin/env bash
# Run pgvector's SQL regression suite against a fresh server, the same way
# pgvector's `make installcheck` does (--inputdir=test --load-extension=vector).
# usage: run-regress.sh {pgrust|ref} [test ...]   (default: all of test/sql)
set -euo pipefail
. "$(dirname "$0")/common.sh"

[ "$#" -ge 1 ] || die "usage: run-regress.sh {pgrust|ref} [test ...]"
mode="$1"
shift
port="$(port_for "$mode")"
out="$PGV_WORK/regress/$mode"

if [ "$#" -gt 0 ]; then
  tests=("$@")
else
  tests=()
  for f in "$PGV_SRC"/test/sql/*.sql; do
    tests+=("$(basename "$f" .sql)")
  done
fi

rm -rf "$out"
mkdir -p "$out"
server_stop "$mode"
rm -rf "$PGV_WORK/data/$mode"
# Trap first: if server_start dies after launching (e.g. the identity check),
# the server must still be stopped.
trap 'server_stop "$mode"' EXIT
server_start "$mode" "$PGV_WORK/data/$mode" "$port"

set +e
# Run from $out: copy.sql does `\copy ... 'results/vector.bin'`, a path relative
# to psql's cwd (make installcheck runs where results/ is created).
(
  cd "$out" &&
    "$(pg_regress_bin)" --bindir="$PG_TOOLS/bin" --host="$PGV_WORK/sock" --port="$port" --user=postgres \
      --inputdir="$PGV_SRC/test" --outputdir="$out" \
      --dbname=contrib_regression --load-extension=vector \
      "${tests[@]}"
) >"$out/pg_regress.log" 2>&1
rc=$?
set -e

# pg_regress (PG 16+) prints "ok N - name ..." / "not ok N - name ...".
sed -n -E 's/^(not ok|ok)[[:space:]]+[0-9]+[[:space:]]+[-+][[:space:]]+([^[:space:]]+).*$/\2 \1/p' "$out/pg_regress.log" |
  while read -r name result; do
    if [ "$result" = ok ]; then printf '%s\tok\n' "$name"; else printf '%s\tFAIL\n' "$name"; fi
  done >"$out/summary.tsv"

passed="$(grep -c $'\tok$' "$out/summary.tsv" || true)"
echo "regress ($mode): $passed/${#tests[@]} passed; log $out/pg_regress.log; diffs $out/regression.diffs"
exit "$rc"

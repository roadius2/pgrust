#!/usr/bin/env bash
# Exact-differential tier (spec §8.2): diffrunner's --pgvector suite against
# the C reference (A) and a subject (B), plus the error-location check.
# Every vector, halfvec, sparsevec and bit function, cast, operator and
# aggregate, their errors, and exact (non-index) nearest-neighbour queries
# must match; numerals non-integral on either side may differ within 1e-5
# relative (ruling pgvector-float-rel, docs/fuzzing/rulings.toml).
#
# usage: run-diff.sh {pgrust|ref}
#   pgrust: B is pgrust.  ref: B is the seeded C build (must always pass).
#   PGV_DIFF_SEED (default 1) and PGV_DIFF_COUNT (default 2000) seed and
#   size the random arm. PGV_DIFF_PERTURB_B adds one diffrunner --perturb-b
#   statement (harness self-test only).
set -euo pipefail
. "$(dirname "$0")/common.sh"
here="$(cd "$(dirname "$0")" && pwd)"

[ "$#" -eq 1 ] || die "usage: run-diff.sh {pgrust|ref}"
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/diff/$mode"
rm -rf "$out"
mkdir -p "$out"

PGV_LISTEN=127.0.0.1
trap 'server_stop ref; server_stop "$subject"' EXIT
for side in ref "$subject"; do
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$(port_for "$side")"
  assert_identity "$side" "$(port_for "$side")"
done

perturb=()
if [ -n "${PGV_DIFF_PERTURB_B:-}" ]; then perturb=(--perturb-b "$PGV_DIFF_PERTURB_B"); fi
set +e
"$DIFFRUNNER_BIN" --pgvector \
  --a "127.0.0.1:$(port_for ref)" --b "127.0.0.1:$(port_for "$subject")" \
  --db postgres --user postgres \
  --seed "${PGV_DIFF_SEED:-1}" --count "${PGV_DIFF_COUNT:-2000}" \
  ${perturb[@]+"${perturb[@]}"} \
  --findings "$out/findings.jsonl" 2>"$out/diffrunner.log"
rc=$?
set -e
server_stop ref
server_stop "$subject"
[ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data"
[ "$rc" -le 2 ] || die "diffrunner failed (exit $rc); see $out/diffrunner.log"

sed -n 's/^diffrunner: pgvector section=\([a-z_]*\) .* findings=\([0-9]*\)$/\1 \2/p' \
  "$out/diffrunner.log" >"$out/sections.txt"
[ -s "$out/sections.txt" ] || die "no section results in $out/diffrunner.log"
: >"$out/summary.tsv"
while read -r name findings; do
  if [ "$findings" = 0 ]; then r=ok; else r=FAIL; fi
  printf '%s\t%s\n' "$name" "$r" | tee -a "$out/summary.tsv"
done <"$out/sections.txt"

# The error F/L/R fields, which diffrunner does not compare.
PGV_LISTEN=
if "$here/sqldiff.sh" "$mode" --verbose "$here/sql/error-locations.sql" >"$out/error-locations.log" 2>&1; then
  r=ok
else
  r=FAIL
fi
printf 'error_locations\t%s\n' "$r" | tee -a "$out/summary.tsv"

total=$(wc -l <"$out/summary.tsv" | tr -d ' ')
passed=$(grep -c $'\tok$' "$out/summary.tsv" || true)
echo "diff ($mode, subject $subject): $passed/$total clean; findings in $out/findings.jsonl"
[ "$passed" -eq "$total" ]

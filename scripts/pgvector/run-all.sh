#!/usr/bin/env bash
# Run every pgvector conformance tier built so far and print a pass/fail report.
# usage: run-all.sh {pgrust|ref}
# Each tier's old summary.tsv is removed before it runs, so a tier that dies
# before writing one reports "(no results)". A tier that exits non-zero
# after writing only ok rows (it died partway) gets a FAIL row of its own.
# PGV_TIER_DIR (harness self-test only) runs the run-<tier>.sh scripts from
# another directory.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

[ "$#" -eq 1 ] || die "usage: run-all.sh {pgrust|ref}"
mode="$1"
port_for "$mode" >/dev/null

tiers=(regress tap bytecmp iterscan diff ondisk)
rcs=()
for tier in "${tiers[@]}"; do
  rm -f "$PGV_WORK/$tier/$mode/summary.tsv"
  if [ "$tier" = ondisk ]; then rm -f "$PGV_WORK/ondisk/$mode/reverse.tsv"; fi
  rc=0
  "${PGV_TIER_DIR:-$here}/run-$tier.sh" "$mode" || rc=$?
  rcs+=("$rc")
done

report="$PGV_WORK/report-$mode.md"
total=0
passed=0
{
  printf '# pgvector conformance: %s\n\n' "$mode"
  printf '| Tier | Test | Result |\n|---|---|---|\n'
  i=0
  for tier in "${tiers[@]}"; do
    rc="${rcs[$i]}"
    i=$((i + 1))
    f="$PGV_WORK/$tier/$mode/summary.tsv"
    rows=0
    ok=0
    if [ -s "$f" ]; then
      while IFS=$'\t' read -r name result; do
        rows=$((rows + 1))
        [ "$result" = ok ] && ok=$((ok + 1))
        printf '| %s | %s | %s |\n' "$tier" "$name" "$result"
      done <"$f"
    fi
    total=$((total + rows))
    passed=$((passed + ok))
    if [ "$rows" -eq 0 ]; then
      printf '| %s | (no results; run-%s.sh exited %d) | FAIL |\n' "$tier" "$tier" "$rc"
      total=$((total + 1))
    elif [ "$rc" -ne 0 ] && [ "$ok" -eq "$rows" ]; then
      printf '| %s | (run-%s.sh exited %d) | FAIL |\n' "$tier" "$tier" "$rc"
      total=$((total + 1))
    fi
  done
  printf '\n**%d/%d passed**\n' "$passed" "$total"
  # On-disk, reverse (spec §8.2): reported, never counted.
  printf '\n## Reported, not gated: on-disk reverse\n\n'
  if [ -s "$PGV_WORK/ondisk/$mode/reverse.tsv" ]; then
    printf '| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name result; do
      printf '| %s | %s |\n' "$name" "$result"
    done <"$PGV_WORK/ondisk/$mode/reverse.tsv"
  else
    printf '(no results: run-ondisk.sh wrote no reverse.tsv; see its output)\n'
  fi
} >"$report"
cat "$report"
[ "$passed" -eq "$total" ]

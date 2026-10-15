#!/usr/bin/env bash
# Run every pgvector conformance tier built so far and print a pass/fail report.
# usage: run-all.sh {pgrust|ref}
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

[ "$#" -eq 1 ] || die "usage: run-all.sh {pgrust|ref}"
mode="$1"
port_for "$mode" >/dev/null

"$here/run-regress.sh" "$mode" || true
"$here/run-tap.sh" "$mode" || true
"$here/run-bytecmp.sh" "$mode" || true
"$here/run-iterscan.sh" "$mode" || true
"$here/run-diff.sh" "$mode" || true
# run-ondisk.sh can die before it clears its old results, so drop them first and keep its status.
rm -f "$PGV_WORK/ondisk/$mode/summary.tsv" "$PGV_WORK/ondisk/$mode/reverse.tsv"
ondisk_rc=0
"$here/run-ondisk.sh" "$mode" || ondisk_rc=$?

report="$PGV_WORK/report-$mode.md"
total=0
passed=0
{
  printf '# pgvector conformance: %s\n\n' "$mode"
  printf '| Tier | Test | Result |\n|---|---|---|\n'
  for tier in regress tap bytecmp iterscan diff ondisk; do
    f="$PGV_WORK/$tier/$mode/summary.tsv"
    if [ ! -s "$f" ]; then
      printf '| %s | (no results) | FAIL |\n' "$tier"
      total=$((total + 1))
      continue
    fi
    while IFS=$'\t' read -r name result; do
      total=$((total + 1))
      [ "$result" = ok ] && passed=$((passed + 1))
      printf '| %s | %s | %s |\n' "$tier" "$name" "$result"
    done <"$f"
    # A run that died after writing only passing rows must still fail.
    if [ "$tier" = ondisk ] && [ "$ondisk_rc" -ne 0 ] && ! grep -q $'\tFAIL' "$f"; then
      printf '| %s | (run-ondisk.sh exited %d) | FAIL |\n' "$tier" "$ondisk_rc"
      total=$((total + 1))
    fi
  done
  printf '\n**%d/%d passed**\n' "$passed" "$total"
  # On-disk, reverse (spec §8.2): reported, never counted.
  if [ -s "$PGV_WORK/ondisk/$mode/reverse.tsv" ]; then
    printf '\n## Reported, not gated: on-disk reverse\n\n| Check | Result |\n|---|---|\n'
    while IFS=$'\t' read -r name result; do
      printf '| %s | %s |\n' "$name" "$result"
    done <"$PGV_WORK/ondisk/$mode/reverse.tsv"
  fi
} >"$report"
cat "$report"
[ "$passed" -eq "$total" ]

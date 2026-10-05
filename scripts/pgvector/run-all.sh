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

report="$PGV_WORK/report-$mode.md"
total=0
passed=0
{
  printf '# pgvector conformance: %s\n\n' "$mode"
  printf '| Tier | Test | Result |\n|---|---|---|\n'
  for tier in regress tap bytecmp; do
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
  done
  printf '\n**%d/%d passed**\n' "$passed" "$total"
} >"$report"
cat "$report"
[ "$passed" -eq "$total" ]

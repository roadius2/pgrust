#!/usr/bin/env bash
# Tests report.py's headline rule (plan, Review Focus 5) on tests/fixture.tsv:
# the QPS at recall >= 0.95 is the 8-client QPS of the first ef_search whose
# single-client recall reaches 0.95 (ef 64, not ef 40's higher QPS), and the
# one at >= 0.99 is ef 100's; nothing is interpolated. Runs anywhere with
# python3 (the Mac or borg2):
#   scripts/bench/tests/test_report.sh
#
# fixture.tsv is run-phase1.sh's TSV with a leading contestant column, so one
# file holds both contestants; the test splits it into pg-c.tsv and
# pgrust.tsv, as a run directory holds them.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT

awk -F'\t' -v OFS='\t' -v dir="$dir" '
  NR == 1 { $1 = ""; sub(/^\t/, ""); header = $0; next }
  {
    f = dir "/" $1 ".tsv"
    if (!(f in seen)) { print header >f; seen[f] = 1 }
    $1 = ""; sub(/^\t/, ""); print >f
  }' "$here/fixture.tsv"

out="$(python3 "$here/../report.py" "$dir")"
echo "$out"
echo "----"

fail=0
expect() {
  if grep -qxF -- "$1" <<<"$out"; then
    echo "ok: $1"
  else
    echo "FAIL: no line $1"
    fail=1
  fi
}
reject() {
  if grep -qF -- "$1" <<<"$out"; then
    echo "FAIL: unexpected $1"
    fail=1
  else
    echo "ok: no $1"
  fi
}

expect "| metric | pg-c | pgrust | pgrust ÷ pg-c |"
expect "| HNSW QPS at recall ≥ 0.95, 8 clients | 4000 (ef_search 64) | 3200 (ef_search 64) | 0.80× |"
expect "| HNSW QPS at recall ≥ 0.99, 8 clients | 3000 (ef_search 100) | 2700 (ef_search 100) | 0.90× |"
# The fixture has no 16-client points: the setting is named, the value absent.
expect "| HNSW QPS at recall ≥ 0.95, 16 clients | no point (ef_search 64) | no point (ef_search 64) | – |"
# ef 40 (recall 0.93) has the highest QPS and must not be a headline.
reject "5000 (ef_search"
reject "4400 (ef_search"
# Every point is in the search table, the 1-client recall included.
expect "| glove-100-angular | vector | hnsw | 64 | 8 | 1 | 4000 | 3200 | 0.80× | – | – | 1.90 | 1.90 | – | – | 3.10 | 3.10 |"
expect "| glove-100-angular | vector | hnsw | 40 | 1 | 1 | 900 | 800 | 0.89× | 0.9300 | 0.9310 | 1.10 | 1.10 | 1.50 | 1.50 | 2.00 | 2.00 |"
if [ "$(grep -c '^| glove-100-angular | vector | hnsw |' <<<"$out")" = 6 ]; then
  echo "ok: 6 search points"
else
  echo "FAIL: not 6 search points"
  fail=1
fi

if [ "$fail" != 0 ]; then
  echo "FAILED"
  exit 1
fi
echo "PASS"

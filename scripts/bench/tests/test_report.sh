#!/usr/bin/env bash
# Tests report.py's headline rule (plan, Review Focus 5) on tests/fixture.tsv:
# the QPS at recall >= 0.95 is the 8-client QPS of the first ef_search whose
# single-client recall reaches 0.95 (ef 64, not ef 40's higher QPS), the one
# at >= 0.99 is ef 100's and the one at >= 0.90 is ef 40's; nothing is
# interpolated. It checks the matched-setting rows (ef_search 100, probes 20),
# the "not reached:" fallback to the highest setting, the missing ratio
# between a reached and an unreached cell, and the † on cells whose setting
# or the one before it is within 0.005 of the target. It also checks the
# serial build's container peak and its approximate increment (rep 1's peak
# minus the preceding row's, here the exact pass's; none for reps 2 and 3,
# which follow another build directly). Runs anywhere with
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
# ef 100's recalls (0.99, 0.991) are within 0.005 of 0.99: marked †.
expect "| HNSW QPS at recall ≥ 0.99, 8 clients | 3000 (ef_search 100)† | 2700 (ef_search 100)† | 0.90× |"
# The fixture has no 16-client points: the setting is named, the value absent.
expect "| HNSW QPS at recall ≥ 0.95, 16 clients | no point (ef_search 64) | no point (ef_search 64) | – |"
# ef 40 (recall 0.93) has the highest QPS: the 0.90 headline, never the 0.95 one.
expect "| HNSW QPS at recall ≥ 0.90, 8 clients | 5000 (ef_search 40) | 4400 (ef_search 40) | 0.88× |"
reject "| HNSW QPS at recall ≥ 0.95, 8 clients | 5000"
# Matched-setting rows: the same setting on both, each engine's recall shown.
expect "| HNSW QPS at ef_search 100, 8 clients (matched setting) | 3000 (recall 0.9900) | 2700 (recall 0.9910) | 0.90× |"
expect "| IVFFlat QPS at probes 20, 8 clients (matched setting) | 727 (recall 0.9501) | 550 (recall 0.9491) | 0.76× |"
# The threshold effect: pgrust's probes 10 (0.899) misses 0.90 by 0.001, so
# its cell is probes 20's; both cells are marked.
expect "| IVFFlat QPS at recall ≥ 0.90, 8 clients | 1380 (probes 10)† | 550 (probes 20)† | 0.40× |"
# Not reached: the highest setting's QPS and recall, and no ratio against a
# cell that did reach the target; † when the highest recall is near it.
expect "| IVFFlat QPS at recall ≥ 0.95, 8 clients | 727 (probes 20)† | not reached: 550 (probes 20, recall 0.9491)† | – |"
# Neither reached: both are the highest setting, so the ratio stands; no †.
expect "| IVFFlat QPS at recall ≥ 0.99, 8 clients | not reached: 727 (probes 20, recall 0.9501) | not reached: 550 (probes 20, recall 0.9491) | 0.76× |"
reject "not reached |"
# Every point is in the search table, the 1-client recall included.
expect "| glove-100-angular | vector | hnsw | 64 | 8 | 1 | 4000 | 3200 | 0.80× | – | – | 1.90 | 1.90 | – | – | 3.10 | 3.10 |"
expect "| glove-100-angular | vector | hnsw | 40 | 1 | 1 | 900 | 800 | 0.89× | 0.9300 | 0.9310 | 1.10 | 1.10 | 1.50 | 1.50 | 2.00 | 2.00 |"
# Memory: pg-c 1.50 GiB peak after a 1.00 GiB row (+0.50); pgrust's three
# serial reps peak at 0.90 (after a 0.75 row: +0.15), 0.89 and 0.91. The
# increment is rep 1's alone: per-rep increments (0.15, -0.01, 0.02) would
# have a median of 0.02.
expect "| Container peak, serial HNSW build (median), GiB¹ | 1.50 | 0.90 | 0.60× |"
expect "| Build increment, rep 1 (approx.), serial HNSW build, GiB¹ | 0.50 | 0.15 | 0.30× |"
reject "| 0.50 | 0.02 |"
expect "| glove-100-angular | vector | hnsw | serial |  | 1 | 30000 | 54000 | 1.80× | 1.50 | 0.90 | 0.50 | 0.15 |"
expect "| glove-100-angular | vector | hnsw | serial |  | 2 | – | 56000 | – | – | 0.89 | – | – |"
expect "| glove-100-angular | vector | hnsw | serial |  | 3 | – | 52000 | – | – | 0.91 | – | – |"
if [ "$(grep -c '^| glove-100-angular | vector | hnsw | [0-9]' <<<"$out")" = 6 ]; then
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

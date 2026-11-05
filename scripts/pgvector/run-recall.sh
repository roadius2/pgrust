#!/usr/bin/env bash
# The approximate differential (M7; spec §8.2): recall on real embeddings
# against stock C pgvector. The baseline is always stock C; the subject builds
# and searches the same rows, and its recall must meet
#   pgrust recall >= pgvector recall - 0.01
# for recall@10 and @100. Exact answers come from the baseline's sequential
# scan, before any index exists.
# usage: run-recall.sh {pgrust|ref} [case ...]
#        run-recall.sh --list
#   pgrust: the subject is pgrust.  ref: the subject is the seeded C build (C
#   against C, the self-test's run).  No cases means all of them.
#   Rows of summary.tsv, the only file run-all.sh reads: <case>@10 and
#   <case>@100, the median of the builds on each side, and exact_<space>, the
#   subject's exact answers to the 100 lowest query ids against C's. A row also
#   fails when the baseline's median is outside PGV_RECALL_BAND, where a
#   comparison cannot detect a difference. Exit 1 if any row fails; a guard
#   (a build that failed, a parallel case built serially, a plan that is not an
#   index scan, a search that returned a row twice) dies, leaving no summary.tsv.
#   Output in $PGV_WORK/recall/<mode>/: summary.tsv; recall.tsv (case, side,
#   build number, build ms, recall@10, recall@100, rows returned, flush tuples
#   or empty, flushing participant or empty); report.md; the logs. The clusters
#   are removed at exit unless PGV_KEEP_DATA=1.
#   Environment, with defaults: PGV_RECALL_ROWS 100000, PGV_RECALL_QUERIES
#   1000, PGV_RECALL_DIM 100, PGV_RECALL_NNZ 16, PGV_RECALL_BUILDS 3 (odd, so
#   a median is one build's value), PGV_RECALL_LISTS 316, PGV_RECALL_EF_SEARCH
#   100, PGV_RECALL_PROBES 20, PGV_RECALL_SPILL_MEM 64MB, PGV_RECALL_BAND
#   "0.5 0.995". PGV_RECALL_TEXT and PGV_RECALL_CACHE reach recall-data.sh.
#   PGV_RECALL_PERTURB_SUBJECT (self-test only) is a name=value setting applied
#   last on the subject, to its builds and its searches.
set -euo pipefail
. "$(dirname "$0")/common.sh"

cases=(
  hnsw_vector_serial hnsw_vector_parallel hnsw_vector_spill
  hnsw_halfvec_serial hnsw_halfvec_parallel
  hnsw_sparsevec_serial hnsw_sparsevec_parallel
  hnsw_bit_serial hnsw_bit_parallel
  ivfflat_vector_serial ivfflat_vector_parallel
  ivfflat_halfvec_serial ivfflat_halfvec_parallel
  ivfflat_bit_serial ivfflat_bit_parallel
)

usage() { die "usage: run-recall.sh {pgrust|ref} [case ...] | --list"; }
[ "$#" -ge 1 ] || usage
if [ "$1" = --list ]; then
  [ "$#" -eq 1 ] || usage
  printf '%s\n' "${cases[@]}"
  exit 0
fi
mode="$1"
shift
case "$mode" in
  pgrust)
    subject=pgrust
    subject_bin="$PGRUST_BIN"
    ;;
  ref)
    subject=seeded
    subject_bin="$PG_VEC_SEEDED/bin/postgres"
    ;;
  *) usage ;;
esac

named=""
for c in "$@"; do
  known=""
  for k in "${cases[@]}"; do
    if [ "$c" = "$k" ]; then known=1; fi
  done
  [ -n "$known" ] || die "unknown case '$c'; run-recall.sh --list prints them"
  # A case run twice would put an even number of builds under one median.
  case " $named " in
    *" $c "*) die "case '$c' is named twice" ;;
  esac
  named="$named $c"
done
if [ "$#" -eq 0 ]; then
  selected=("${cases[@]}")
else
  selected=("$@")
fi

rows="${PGV_RECALL_ROWS:-100000}"
queries="${PGV_RECALL_QUERIES:-1000}"
dim="${PGV_RECALL_DIM:-100}"
nnz="${PGV_RECALL_NNZ:-16}"
builds="${PGV_RECALL_BUILDS:-3}"
lists="${PGV_RECALL_LISTS:-316}"
ef_search="${PGV_RECALL_EF_SEARCH:-100}"
probes="${PGV_RECALL_PROBES:-20}"
spill_mem="${PGV_RECALL_SPILL_MEM:-64MB}"
band="${PGV_RECALL_BAND:-0.5 0.995}"
perturb="${PGV_RECALL_PERTURB_SUBJECT:-}"

case "$builds" in
  "" | 0* | *[!0-9]*) die "PGV_RECALL_BUILDS must be a positive odd number, got '$builds'" ;;
esac
[ $((builds % 2)) -eq 1 ] || die "PGV_RECALL_BUILDS must be odd, got $builds"
read -r band_lo band_hi band_rest <<<"$band"
case "$band_lo:$band_hi:$band_rest" in
  *[!0-9.:]* | :* | *::* | *:*:?*) die "PGV_RECALL_BAND must be two numbers, got '$band'" ;;
esac
perturb_sql=""
if [ -n "$perturb" ]; then
  case "$perturb" in
    [a-z]*=*) perturb_sql="SET ${perturb%%=*} = '${perturb#*=}';" ;;
    *) die "PGV_RECALL_PERTURB_SUBJECT must be name=value, got '$perturb'" ;;
  esac
fi

out="$PGV_WORK/recall/$mode"
rm -rf "$out"
mkdir -p "$out"
[ -x "$subject_bin" ] || die "missing $subject_bin"
[ -x "$PG_VEC/bin/postgres" ] || die "missing $PG_VEC/bin/postgres; run scripts/pgvector/build-reference.sh"

data="$("$(dirname "$0")/recall-data.sh" "$rows" "$queries" "$dim" "$nnz")" || die "preparing the dataset failed"

# Sets col, op and opclass for the space (the column type) $1.
space_ops() {
  case "$1" in
    vector) col=v op='<=>' opclass=vector_cosine_ops ;;
    halfvec) col=h op='<=>' opclass=halfvec_cosine_ops ;;
    sparsevec) col=s op='<=>' opclass=sparsevec_cosine_ops ;;
    bit) col=b op='<~>' opclass=bit_hamming_ops ;;
    *) die "unknown space '$1'" ;;
  esac
}

# The spaces the selected cases need, in a fixed order.
spaces=""
for space in vector halfvec sparsevec bit; do
  for c in "${selected[@]}"; do
    case "$c" in
      *_"$space"_*)
        spaces="$spaces $space"
        break
        ;;
    esac
  done
done

# The same rows on both servers. COPY reads the prepared text once per table;
# halfvec and bit are derived on the server, as a user would derive them.
load_sql() {
  local table
  cat <<SQL
CREATE EXTENSION vector;
CREATE TABLE rec_base (id int, v vector($dim), h halfvec($dim), s sparsevec($dim), b bit($dim));
CREATE TABLE rec_query (LIKE rec_base);
CREATE TABLE rec_truth (space text, qid int, d10 float8, d100 float8);
CREATE TEMP TABLE rec_stage (id int, v vector($dim), s sparsevec($dim));
SQL
  for table in base query; do
    cat <<SQL
\\copy rec_stage FROM '$data/$table.tsv'
INSERT INTO rec_$table SELECT id, v, v::halfvec($dim), s, binary_quantize(v)::bit($dim) FROM rec_stage;
TRUNCATE rec_stage;
SQL
  done
  echo 'VACUUM ANALYZE rec_base;'
}

# The exact 10th and 100th distances of space $1 for the queries in $2 (a table
# or a subquery), with no index to use.
truth_select() {
  space_ops "$1"
  cat <<SQL
SELECT '$1', q.id, x.d[10], x.d[100]
FROM $2 q, LATERAL (
  SELECT array_agg(d ORDER BY d) d
  FROM (SELECT b.$col $op q.$col AS d FROM rec_base b ORDER BY 1 LIMIT 100) s) x
SQL
}

# The exact_<space> row: the subject's own exact answers against C's, since
# recall below trusts the distance the subject reports.
exact_row() {
  local space="$1" port res log want
  port="$(port_for "$subject")"
  res="$out/exact-$space.out"
  log="$out/exact-$space.log"
  want=$((queries < 100 ? queries : 100))
  pgv_psql "$port" -At -F "$tab" >"$res" 2>"$log" <<SQL || die "the exact query of $space failed on $subject; see $log"
CREATE TEMP TABLE rec_exact (space text, qid int, d10 float8, d100 float8);
INSERT INTO rec_exact $(truth_select "$space" "(SELECT * FROM rec_query ORDER BY id LIMIT 100)");
SELECT count(*) FILTER (WHERE abs(e.d10 - t.d10) <= 1e-5 AND abs(e.d100 - t.d100) <= 1e-5),
       max(greatest(abs(e.d10 - t.d10), abs(e.d100 - t.d100)))
FROM rec_exact e JOIN rec_truth t ON t.space = e.space AND t.qid = e.qid;
SQL
  local matched largest result=FAIL
  IFS="$tab" read -r matched largest <"$res" || true
  if [ "$matched" = "$want" ]; then result=ok; fi
  printf 'exact_%s\t%s\t%s\t%s\n' "$space" "$result" "$matched" "$largest" >>"$out/exact.tsv"
}

# Prints "<tuples>\t<participant>" for the build log $1, or a lone tab if the
# graph never overflowed. The NOTICE's block is its DETAIL, HINT and CONTEXT
# lines, up to the next message; a line of CONTEXT may follow other context.
flush_of() {
  awk '
    /^[A-Z]+:  / && !/^(DETAIL|HINT|CONTEXT):  / { block = 0 }
    /^NOTICE:  hnsw graph no longer fits into maintenance_work_mem after [0-9]+ tuples$/ {
      tuples = $(NF - 1)
      who = "leader"
      block = 1
    }
    block && /^(CONTEXT:  )?parallel worker$/ { who = "worker" }
    END { printf "%s\t%s", tuples, who }
  ' "$1"
}

# One build and one measurement of case $1 on side $2, build number $3; appends
# its recall.tsv row.
measure() {
  local c="$1" side="$2" n="$3" port rest am space build workers=2 mem=2GB ddl
  local log plan res err set_perturb="" search search_set ms launched flush r10 r100 got repeats
  port="$(port_for "$side")"
  am="${c%%_*}"
  rest="${c#*_}"
  space="${rest%%_*}"
  build="${rest#*_}"
  space_ops "$space"
  case "$build" in
    serial) workers=0 ;;
    parallel) ;;
    spill) mem="$spill_mem" ;;
    *) die "unknown build '$build' in case $c" ;;
  esac
  case "$am" in
    hnsw) ddl="CREATE INDEX rec_idx ON rec_base USING hnsw ($col $opclass) WITH (m = 16, ef_construction = 64);" ;;
    ivfflat) ddl="CREATE INDEX rec_idx ON rec_base USING ivfflat ($col $opclass) WITH (lists = $lists);" ;;
    *) die "unknown access method '$am' in case $c" ;;
  esac
  if [ "$side" = "$subject" ]; then set_perturb="$perturb_sql"; fi
  log="$out/$c-$side-$n.log"
  plan="$out/$c-$side-$n.plan"
  res="$out/$c-$side-$n.out"
  err="$out/$c-$side-$n.err"

  # SHOW_CONTEXT: psql prints a NOTICE's CONTEXT, which is what names a worker,
  # only when told to.
  pgv_psql "$port" >"$log" 2>&1 <<SQL || die "$c on $side: the build failed; see $log"
\\set SHOW_CONTEXT always
SET max_parallel_maintenance_workers = $workers;
SET maintenance_work_mem = '$mem';
ALTER TABLE rec_base SET (parallel_workers = $workers);
SET client_min_messages = debug1;
$set_perturb
\\timing on
$ddl
\\timing off
SQL
  ms="$(sed -n 's/^Time: \([0-9.]*\) ms.*/\1/p' "$log" | head -1)"
  [ -n "$ms" ] || die "$c on $side: no timing for the build; see $log"
  launched="$(sed -n 's/.*DEBUG:  using \([0-9]*\) parallel workers$/\1/p' "$log")"
  if [ "$build" = serial ]; then
    [ -z "$launched" ] || die "$c on $side: expected a serial build, got $launched parallel workers; see $log"
  else
    [ "$launched" = 2 ] || die "$c on $side: expected 2 parallel workers, got '${launched:-none}'; see $log"
  fi
  flush="$(flush_of "$log")"
  if [ "$build" = spill ]; then
    grep -q 'no longer fits into maintenance_work_mem' "$log" ||
      die "$c on $side: the graph did not overflow maintenance_work_mem ($mem); see $log"
  fi

  search_set="SET enable_seqscan = off;
SET hnsw.ef_search = $ef_search;
SET ivfflat.probes = $probes;
$set_perturb"
  search="SELECT q.id AS qid, n.id, n.d FROM rec_query q, LATERAL (
  SELECT b.id, b.$col $op q.$col AS d FROM rec_base b ORDER BY b.$col $op q.$col LIMIT 100) n"
  # A sequential scan would read recall 1.0 and prove nothing.
  pgv_psql "$port" -At >"$plan" 2>"$err" <<SQL || die "$c on $side: EXPLAIN failed; see $err"
$search_set
EXPLAIN (COSTS OFF) $search;
SQL
  grep -q 'Index Scan using rec_idx' "$plan" || die "$c on $side: not an index scan; see $plan"

  # The denominators are fixed, so a short result lowers recall. A repeated row
  # would count as a hit, so an index holding every row twice could read 1.0:
  # the last column is the rows returned twice. Results are stdout only: the
  # seeded build prints INFO lines to stderr.
  pgv_psql "$port" -At -F "$tab" >"$res" 2>"$err" <<SQL || die "$c on $side: the measurement failed; see $err"
$search_set
CREATE TEMP TABLE got AS
$search;
CREATE TEMP TABLE ranked AS
SELECT qid, id, d, row_number() OVER (PARTITION BY qid ORDER BY d) AS r FROM got;
SELECT count(*) FILTER (WHERE r <= 10 AND d <= t.d10 + 1e-5) / (10.0 * $queries),
       count(*) FILTER (WHERE r <= 100 AND d <= t.d100 + 1e-5) / (100.0 * $queries),
       count(*),
       count(*) - count(DISTINCT (ranked.qid, ranked.id))
FROM ranked JOIN rec_truth t ON t.qid = ranked.qid AND t.space = '$space';
DROP INDEX rec_idx;
ALTER TABLE rec_base RESET (parallel_workers);
SQL
  IFS="$tab" read -r r10 r100 got repeats <"$res" || true
  [ -n "$repeats" ] || die "$c on $side: no measurement; see $res"
  [ "$repeats" = 0 ] || die "$c on $side: returned a row twice ($repeats repeats); see $res"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$c" "$side" "$n" "$ms" "$r10" "$r100" "$got" "$flush" >>"$out/recall.tsv"
}

# The median of column $3 of case $1 on side $2. builds is odd, so it is one
# build's value.
median() {
  awk -F'\t' -v c="$1" -v s="$2" -v col="$3" '$1 == c && $2 == s { print $col }' "$out/recall.tsv" |
    LC_ALL=C sort -n | sed -n "$(((builds + 1) / 2))p"
}

# Prints "<baseline>\t<subject>\t<difference>\t<verdict>" for the medians $2
# (baseline) and $3 (subject) of recall@$1. The margin is compared in whole
# hits, of k * queries, because 0.01 and most recalls have no exact binary
# form: a subject exactly 0.01 below the baseline meets the rule.
verdict() {
  awk -v k="$1" -v b="$2" -v s="$3" -v q="$queries" -v lo="$band_lo" -v hi="$band_hi" 'BEGIN {
    total = k * q
    hits_b = int(b * total + 0.5)
    hits_s = int(s * total + 0.5)
    v = ""
    if (100 * hits_s < 100 * hits_b - total) v = "below the margin"
    if (b + 0 < lo + 0 || b + 0 > hi + 0)
      v = (v == "" ? "" : v "; ") "baseline outside the band, cannot detect a difference"
    printf "%.4f\t%.4f\t%+.4f\t%s\n", b, s, s - b, (v == "" ? "ok" : "FAIL: " v)
  }'
}

cleanup() {
  server_stop ref
  server_stop "$subject"
  # Nothing reads the clusters afterwards (they run to gigabytes at the
  # defaults). The stops above mean they are not in use.
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data"
}
trap cleanup EXIT

tab="$(printf '\t')"
for side in ref "$subject"; do
  port="$(port_for "$side")"
  server_stop "$side"
  server_start "$side" "$out/data/$side" "$port"
  load_sql | pgv_psql "$port" >"$out/load-$side.log" 2>&1 || die "loading the rows failed on $side; see $out/load-$side.log"
done

# Truth, before any index exists: one exact query per space on the baseline,
# copied to the subject so both sides are scored against the same answers.
ref_port="$(port_for ref)"
subject_port="$(port_for "$subject")"
{
  for space in $spaces; do
    echo "INSERT INTO rec_truth $(truth_select "$space" rec_query);"
  done
  echo "\\copy rec_truth TO '$out/truth.tsv'"
} | pgv_psql "$ref_port" >"$out/truth.log" 2>&1 || die "the exact queries failed on ref; see $out/truth.log"
echo "\\copy rec_truth FROM '$out/truth.tsv'" | pgv_psql "$subject_port" >>"$out/truth.log" 2>&1 ||
  die "copying the truth to $subject failed; see $out/truth.log"
space_count=0
for space in $spaces; do space_count=$((space_count + 1)); done
complete="$(pgv_psql "$subject_port" -Atc 'SELECT count(*) FROM rec_truth WHERE d10 IS NOT NULL AND d100 IS NOT NULL')"
[ "$complete" = $((space_count * queries)) ] ||
  die "expected $((space_count * queries)) complete rows of truth, got $complete; are there fewer than 100 rows?"

: >"$out/exact.tsv"
for space in $spaces; do
  exact_row "$space"
done

: >"$out/recall.tsv"
for c in "${selected[@]}"; do
  for side in ref "$subject"; do
    n=1
    while [ "$n" -le "$builds" ]; do
      echo "$c: $side build $n of $builds" >&2
      measure "$c" "$side" "$n"
      n=$((n + 1))
    done
  done
done

cleanup

# Every row at once, at the end: a run that died above leaves no summary.tsv.
: >"$out/rows.tsv"
for c in "${selected[@]}"; do
  for k in 10 100; do
    col=5
    if [ "$k" = 100 ]; then col=6; fi
    printf '%s@%s\t%s\n' "$c" "$k" \
      "$(verdict "$k" "$(median "$c" ref "$col")" "$(median "$c" "$subject" "$col")")" >>"$out/rows.tsv"
  done
done
{
  cut -f1,2 "$out/exact.tsv"
  awk -F'\t' '{ print $1 "\t" ($5 == "ok" ? "ok" : "FAIL") }' "$out/rows.tsv"
} >"$out/summary.tsv"

{
  echo "# Recall: $subject against C pgvector 0.8.7"
  echo
  echo "$rows rows and $queries held-out queries of GloVe 6B at $dim dimensions (sparsevec keeps $nnz" \
    "entries); $builds builds on each side, medians shown; HNSW m = 16, ef_construction = 64," \
    "ef_search = $ef_search; IVFFlat lists = $lists, probes = $probes; maintenance_work_mem = 2GB," \
    "or $spill_mem for spill; $(uname -sm). A row passes when the subject is at least the baseline" \
    "minus 0.01 and the baseline is within $band_lo to $band_hi."
  if [ -n "$perturb" ]; then
    echo
    echo "**Self-test run: the subject was perturbed with $perturb.**"
  fi
  echo
  echo "| Case | baseline | $subject | difference | verdict |"
  echo "|---|---|---|---|---|"
  awk -F'\t' '{ printf "| %s | %s | %s | %s | %s |\n", $1, $2, $3, $4, $5 }' "$out/rows.tsv"
  echo
  echo "Exact answers, $subject against C (at most 1e-5 apart in both distances):"
  echo
  echo "| Space | verdict | queries matched | largest difference |"
  echo "|---|---|---|---|"
  awk -F'\t' '{ printf "| %s | %s | %s | %s |\n", $1, $2, $3, $4 }' "$out/exact.tsv"
  echo
  echo "| Case | baseline build ms | $subject build ms | $subject / baseline |"
  echo "|---|---|---|---|"
  for c in "${selected[@]}"; do
    awk -v c="$c" -v b="$(median "$c" ref 4)" -v s="$(median "$c" "$subject" 4)" \
      'BEGIN { printf "| %s | %.1f | %.1f | %.2f |\n", c, b, s, (b > 0 ? s / b : 0) }'
  done
  if awk -F'\t' '$8 != "" { found = 1 } END { exit !found }' "$out/recall.tsv"; then
    echo
    echo "Graph overflows (a worker flushed in" \
      "$(awk -F'\t' '$9 == "worker" { w++ } $8 != "" { n++ } END { printf "%d of %d", w, n }' "$out/recall.tsv") builds):"
    echo
    echo "| Case | side | build | tuples | flushed by |"
    echo "|---|---|---|---|---|"
    awk -F'\t' '$8 != "" { printf "| %s | %s | %s | %s | %s |\n", $1, $2, $3, $8, $9 }' "$out/recall.tsv"
  fi
} >"$out/report.md"
cat "$out/report.md"

if awk -F'\t' '$2 != "ok" { bad = 1 } END { exit !bad }' "$out/summary.tsv"; then
  exit 1
fi

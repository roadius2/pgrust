#!/usr/bin/env bash
# Self-tests for the pgvector harness (they check the harness, not pgrust).
# usage: harness_test.sh [test_fn ...]   (default: every test_* function)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# Self-tests run in their own work dir so they never replace real results or
# stop real harness servers (only the Docker containers and port 55493 are
# shared).
PGV_REAL_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
export PGV_WORK="$PGV_REAL_WORK/selftest"
. "$here/../common.sh"
. "$here/../docker-lib.sh"
# Tests write logs beside their work dirs ("$PGV_WORK/<name>.log").
mkdir -p "$PGV_WORK"

fails=0
pass() { printf 'ok   - %s\n' "$1"; }
fail() { printf 'FAIL - %s\n' "$1"; fails=$((fails + 1)); }

# --- tests: each task inserts its functions above the runner marker ---

test_reference() {
  if "$here/../build-reference.sh" --verify; then
    pass "reference installs verified"
  else
    fail "reference installs verified"
  fi
}

test_pgrust_binary() {
  # pg_ctl (hybrid TAP) accepts only this exact -V output.
  local v
  v="$("$PGRUST_BIN" -V 2>&1 || true)"
  if [ "$v" = "postgres (PostgreSQL) 18.6" ]; then pass "pgrust -V"; else fail "pgrust -V: '$v'"; fi
  if [ -n "$(pgrust_ext_version)" ]; then
    pass "pgrust staged vector.control"
  else
    fail "pgrust staged vector.control missing under target/$PGRUST_PROFILE/share/extension"
  fi
}

test_servers() {
  local mode port v ext want d
  for mode in ref pgrust; do
    port="$(port_for "$mode")"
    if ! "$here/../server.sh" fresh "$mode" >/dev/null 2>&1; then
      fail "$mode: server starts (see $PGV_WORK/log/server-$mode.log)"
      continue
    fi
    pass "$mode: server starts"
    v="$(pgv_psql "$port" -Atc 'select version()' 2>&1 || true)"
    case "$mode:$v" in
      "pgrust:PostgreSQL 18.6"*"(pgrust "*) pass "$mode: version" ;;
      "ref:PostgreSQL 18.6"*"(pgrust "*) fail "$mode: version is pgrust: $v" ;;
      "ref:PostgreSQL 18.6"*) pass "$mode: version" ;;
      *) fail "$mode: version: $v" ;;
    esac
    # Review focus 2: pgrust must load its own staged extension files.
    ext="$(pgv_psql "$port" -Atc 'create extension vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
    if [ "$mode" = ref ]; then want=0.8.7; else want="$(pgrust_ext_version)"; fi
    if [ "$ext" = "$want" ]; then pass "$mode: extension version $want"; else fail "$mode: extension version: got '$ext', want '$want'"; fi
    d="$(pgv_psql "$port" -Atc "select '[1,2,3]'::vector <-> '[4,5,6]'" 2>&1 || true)"
    if [ "$d" = "5.196152422706632" ]; then pass "$mode: l2 distance"; else fail "$mode: l2 distance: '$d'"; fi
    # Review focus 3: a second start on a busy port must refuse.
    if "$here/../server.sh" start "$mode" >/dev/null 2>&1; then
      fail "$mode: second start refused"
    else
      pass "$mode: second start refused"
    fi
    if "$here/../server.sh" stop "$mode" >/dev/null 2>&1; then pass "$mode: server stops"; else fail "$mode: server stops"; fi
  done
}

# Review focus 1: suites must never write into the read-only reference trees.
# --ignored matters: pgvector's .gitignore hides results/ and regression.*.
assert_reference_clean() {
  [ -z "$(git -C "$PGV_REPO" status --porcelain --ignored -- crates/pgvector-0.8.7-reference crates/postgres-18.6-reference)" ]
}

test_regress_ref() {
  local n
  if "$here/../run-regress.sh" ref >/dev/null 2>&1; then
    pass "regress ref: exit 0"
  else
    fail "regress ref: exit 0 (see $PGV_WORK/regress/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/regress/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 14 ]; then pass "regress ref: 14 ok"; else fail "regress ref: '$n' ok, want 14"; fi
  if assert_reference_clean; then pass "regress: reference trees untouched"; else fail "regress: reference trees modified"; fi
}

# Review focus 4: pgrust mode must refuse a server that isn't pgrust.
test_regress_identity() {
  local log="$PGV_WORK/identity-test.log" sentinel="$PGV_WORK/regress/pgrust/.identity-sentinel"
  mkdir -p "$(dirname "$sentinel")"
  : >"$sentinel"
  # Isolated work dir: run-regress.sh wipes its output and data dirs first.
  if PGV_WORK="$PGV_WORK/identity" PGRUST_BIN="$PG_VEC/bin/postgres" \
    "$here/../run-regress.sh" pgrust bit >"$log" 2>&1; then
    fail "pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$log"; then
    pass "pgrust mode refuses a C server"
  else
    fail "pgrust mode refuses a C server: wrong error (see $log)"
  fi
  PGV_WORK="$PGV_WORK/identity" "$here/../server.sh" stop pgrust >/dev/null 2>&1 || true
  # Self-tests must not destroy real results from the last pgrust run.
  if [ -e "$sentinel" ]; then pass "identity test leaves pgrust results alone"; else fail "identity test wiped $PGV_WORK/regress/pgrust"; fi
  rm -f "$sentinel"
}

test_tap_ref() {
  if "$here/../run-tap.sh" ref 015_hnsw_vector_duplicates.pl >/dev/null 2>&1; then
    pass "tap ref: 015_hnsw_vector_duplicates"
  else
    fail "tap ref: 015_hnsw_vector_duplicates (see $PGV_WORK/tap/ref/prove)"
  fi
  if assert_reference_clean; then pass "tap: reference trees untouched"; else fail "tap: reference trees modified"; fi
  # Global constraint: clusters use --no-locale (Cluster.pm otherwise inherits the shell's locale).
  if grep -q 'initialized with locale "C"' \
    "$PGV_WORK/tap/ref/log/015_hnsw_vector_duplicates/regress_log_015_hnsw_vector_duplicates" 2>/dev/null; then
    pass "tap: clusters use --no-locale"
  else
    fail "tap: clusters not initialized with locale C"
  fi
}

# Review focus 4 for TAP: pgrust mode must not count a C server's passes.
test_tap_identity() {
  local w="$PGV_WORK/tap-identity"
  if PGV_WORK="$w" PGRUST_BIN="$PG_VEC/bin/postgres" \
    "$here/../run-tap.sh" pgrust 015_hnsw_vector_duplicates.pl >"$w.log" 2>&1; then
    fail "tap pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$w.log"; then
    pass "tap pgrust mode refuses a C server"
  else
    fail "tap pgrust mode refuses a C server: wrong error (see $w.log)"
  fi
}

# Self-tests must never replace the results of a real run.
test_selftest_isolation() {
  local sentinel="$PGV_REAL_WORK/regress/ref/.selftest-sentinel"
  mkdir -p "$(dirname "$sentinel")"
  : >"$sentinel"
  "$here/../run-regress.sh" ref bit >/dev/null 2>&1 || true
  if [ -e "$sentinel" ]; then pass "self-tests leave real results alone"; else fail "self-tests wiped $PGV_REAL_WORK/regress/ref"; fi
  rm -f "$sentinel"
}

# A stale pidfile (crash, reboot, pid reuse) must never get an unrelated process signalled.
test_server_stop_stale_pidfile() {
  local pid
  # perl restores default SIGINT handling (bash ignores SIGINT in background jobs).
  perl -e '$SIG{INT} = "DEFAULT"; sleep 300' &
  pid=$!
  mkdir -p "$PGV_WORK"
  echo "$pid" >"$PGV_WORK/ref.pid"
  "$here/../server.sh" stop ref >/dev/null 2>&1 || true
  sleep 0.5
  if kill -0 "$pid" 2>/dev/null; then
    pass "stale pidfile: unrelated process untouched"
  else
    fail "stale pidfile: unrelated process was signalled"
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -f "$PGV_WORK/ref.pid"
}

test_docker() {
  local ext coll before after
  before="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if ! "$here/../docker-ref.sh" up >/dev/null 2>&1; then
    fail "docker: up"
    return
  fi
  pass "docker: up"
  ext="$("$here/../docker-ref.sh" psql -Atc 'create extension if not exists vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
  if [ "$ext" = 0.8.7 ]; then pass "docker: pgvector 0.8.7"; else fail "docker: extversion '$ext'"; fi
  coll="$("$here/../docker-ref.sh" psql -Atc "select datcollate from pg_database where datname = 'postgres'" 2>&1 || true)"
  if [ "$coll" = C ]; then pass "docker: C collation"; else fail "docker: datcollate '$coll'"; fi
  if "$here/../docker-ref.sh" down >/dev/null 2>&1; then pass "docker: down"; else fail "docker: down"; fi
  # The image declares VOLUME; removing the container must not leave it dangling.
  after="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if [ "$after" -le "$before" ]; then pass "docker: no leaked volume"; else fail "docker: leaked $((after - before)) anonymous volume(s)"; fi
}

# Byte-identical tier, C vs C: two fresh seeded clusters must produce
# identical index files for every default case.
test_bytecmp_ref() {
  local n want
  want="$(set -- $("$here/../run-bytecmp.sh" --list) && echo $#)"
  if "$here/../run-bytecmp.sh" ref >/dev/null 2>&1; then
    pass "bytecmp ref: exit 0"
  else
    fail "bytecmp ref: exit 0 (see $PGV_WORK/bytecmp/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/bytecmp/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = "$want" ]; then pass "bytecmp ref: $want ok"; else fail "bytecmp ref: '$n' ok, want $want"; fi
  if assert_reference_clean; then pass "bytecmp: reference trees untouched"; else fail "bytecmp: reference trees modified"; fi
}

# Blocks and lists of each IVFFlat case's relation on the seeded C build
# (M4 plan facts): lists start on blocks 2 and 4 and inserts append two
# pages (ivf_insert), one list item and one entry per page (ivf_wide).
ivf_want() {
  case "$1" in
    ivf_l2_groups:idx | ivf_halfvec_groups:idx | ivf_bit_hamming:idx | ivf_unlogged:idx | ivf_unlogged:idx_init) echo "12 10" ;;
    ivf_big:idx) echo "92 10" ;;
    ivf_insert:idx) echo "8 2" ;;
    ivf_vacuum:idx) echo "22 10" ;;
    ivf_wide:idx) echo "56 5" ;;
    *) echo "unknown case" ;;
  esac
}

# The cases must exercise what they claim (reads the C-vs-C run above):
# multi-level graphs, a deep m=4 graph, page-split wide tuples, an init fork,
# IVFFlat block and list counts.
test_bytecmp_cases() {
  local f="$PGV_WORK/bytecmp/ref/cases.tsv" c label blocks level
  if [ ! -s "$f" ]; then
    fail "bytecmp cases: no $f (run test_bytecmp_ref first)"
    return
  fi
  while IFS=$'\t' read -r c label blocks level; do
    case "$c:$label" in
      ivf_*) [ "$blocks $level" = "$(ivf_want "$c:$label")" ] && pass "bytecmp $c $label: $blocks blocks, $level lists" ||
        fail "bytecmp $c $label: $blocks blocks, $level lists, want $(ivf_want "$c:$label")" ;;
      *:idx_init) [ "$level" = -1 ] && pass "bytecmp $c: empty init fork" || fail "bytecmp $c: init fork entry level $level" ;;
      m4_golomb:idx) [ "$level" -ge 3 ] && pass "bytecmp $c: graph height $level" || fail "bytecmp $c: graph height $level, want >= 3" ;;
      wide_golomb:idx) [ "$blocks" -ge 150 ] && pass "bytecmp $c: $blocks blocks" || fail "bytecmp $c: $blocks blocks, want >= 150" ;;
      *) [ "$level" -ge 1 ] && [ "$blocks" -ge 10 ] && pass "bytecmp $c: $blocks blocks, height $level" ||
        fail "bytecmp $c: $blocks blocks, height $level (want multi-page, multi-level)" ;;
    esac
  done <"$f"
}

# Negative control: stock C pgvector never seeds (SeedRandom(42) is under
# #ifdef HNSW_MEMORY), so an unseeded subject must be reported as different.
test_bytecmp_detects_difference() {
  local w="$PGV_WORK/bytecmp-neg"
  if PGV_WORK="$w" PGV_BYTECMP_SUBJECT=ref "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp flags an unseeded C build"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/ref/summary.tsv" 2>/dev/null; then
    pass "bytecmp flags an unseeded C build"
  else
    fail "bytecmp flags an unseeded C build: wrong failure (see $w.log)"
  fi
}

# Review focus 1: the seed is opt-in. Without SET pgrust.hnsw_build_seed,
# pgrust builds like stock C and so differs from the seeded oracle.
test_bytecmp_knob_off() {
  local w="$PGV_WORK/bytecmp-noseed"
  if PGV_WORK="$w" PGV_BYTECMP_SEED= "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp: unseeded pgrust differs from the seeded oracle"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/pgrust/summary.tsv" 2>/dev/null; then
    pass "bytecmp: unseeded pgrust differs from the seeded oracle"
  else
    fail "bytecmp: unseeded pgrust: wrong failure (see $w.log)"
  fi
}

# The IVFFlat seed is opt-in too: without SET pgrust.ivfflat_build_seed the
# k-means++ picks, and so the list order, differ from the seeded oracle.
test_bytecmp_ivf_knob_off() {
  local w="$PGV_WORK/bytecmp-ivf-noseed"
  if PGV_WORK="$w" PGV_BYTECMP_SEED= "$here/../run-bytecmp.sh" pgrust ivf_l2_groups >"$w.log" 2>&1; then
    fail "bytecmp: unseeded pgrust ivfflat differs from the seeded oracle"
  elif grep -q $'^ivf_l2_groups\tFAIL$' "$w/bytecmp/pgrust/summary.tsv" 2>/dev/null; then
    pass "bytecmp: unseeded pgrust ivfflat differs from the seeded oracle"
  else
    fail "bytecmp: unseeded pgrust ivfflat: wrong failure (see $w.log)"
  fi
}

# The oracle must be the seeded build, and pgrust mode must refuse a C server.
test_bytecmp_identity() {
  local w="$PGV_WORK/bytecmp-ident"
  if PGV_WORK="$w" PG_VEC_SEEDED="$PG_VEC" "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp refuses an unseeded oracle"
  elif grep -q 'not built by the seeded C pgvector' "$w.log"; then
    pass "bytecmp refuses an unseeded oracle"
  else
    fail "bytecmp refuses an unseeded oracle: wrong error (see $w.log)"
  fi
  if PGV_WORK="$w" PGRUST_BIN="$PG_VEC/bin/postgres" "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$w.log"; then
    pass "bytecmp pgrust mode refuses a C server"
  else
    fail "bytecmp pgrust mode refuses a C server: wrong error (see $w.log)"
  fi
}

# Iterative-scan tier, C vs C: two fresh seeded clusters stop identically.
test_iterscan_ref() {
  local want n
  want="$("$here/../run-iterscan.sh" --list | wc -l | tr -d ' ')"
  if "$here/../run-iterscan.sh" ref >/dev/null 2>&1; then
    pass "iterscan ref: exit 0"
  else
    fail "iterscan ref: exit 0 (see $PGV_WORK/iterscan/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/iterscan/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = "$want" ]; then pass "iterscan ref: $want ok"; else fail "iterscan ref: '$n' ok, want $want"; fi
}

# The cases must stop where they claim (reads the C-vs-C run above): the 64kB
# case on the memory cap, the 300-tuple case on hnsw.max_scan_tuples, and a
# doubled multiplier must scan further. 990 rows fail the filter in total.
test_iterscan_cases() {
  local a b t l x f n
  removed() { sed -n 's/^Rows Removed by Filter: //p' "$PGV_WORK/iterscan/ref/oracle.$1.out" | head -1; }
  a="$(removed relaxed_64k)"
  b="$(removed relaxed_64k_x2)"
  t="$(removed relaxed_tuples)"
  if [ -n "$a" ] && [ "$a" -lt 990 ]; then pass "iterscan relaxed_64k stops early ($a removed)"; else fail "iterscan relaxed_64k: removed '$a', want < 990"; fi
  if [ -n "$b" ] && [ -n "$a" ] && [ "$b" -gt "$a" ]; then pass "iterscan: multiplier x2 scans further ($a -> $b)"; else fail "iterscan: multiplier x2 removed '$b', want > '$a'"; fi
  if [ -n "$t" ] && [ "$t" -lt 990 ]; then pass "iterscan relaxed_tuples stops early ($t removed)"; else fail "iterscan relaxed_tuples: removed '$t', want < 990"; fi
  # lateral_64k must hit the cap in its first loop (x = 1000000): a loop the
  # cap does not stop returns all 10 matching rows. Otherwise the
  # rescan-after-cap coverage would silently vanish.
  l="$(grep -c '^1000000|' "$PGV_WORK/iterscan/ref/oracle.lateral_64k.out" 2>/dev/null || true)"
  if [ -n "$l" ] && [ "$l" -lt 10 ]; then pass "iterscan lateral_64k first loop hits the cap ($l of 10 rows)"; else fail "iterscan lateral_64k: first loop returned '$l' rows, want < 10 (cap not hit)"; fi
  # big: the 64kB and 256kB caps stop early, at increasing depths; 4MB scans
  # the whole index (9,990 rows fail its filter); every LATERAL loop hits the
  # 128kB cap.
  a="$(removed big_relaxed_64k)"
  b="$(removed big_relaxed_256k)"
  if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -lt "$b" ] && [ "$b" -lt 9990 ]; then pass "iterscan big caps stop early ($a, $b removed)"; else fail "iterscan big caps: removed '$a', '$b', want increasing and < 9990"; fi
  a="$(removed big_exhaust_4MB)"
  if [ "$a" = 9990 ]; then pass "iterscan big_exhaust_4MB scans the whole index"; else fail "iterscan big_exhaust_4MB: removed '$a', want 9990"; fi
  for x in 25000000 100000000 175000000; do
    l="$(grep -c "^$x|" "$PGV_WORK/iterscan/ref/oracle.big_lateral_128k.out" 2>/dev/null || true)"
    if [ -n "$l" ] && [ "$l" -lt 10 ]; then pass "iterscan big_lateral_128k loop $x hits the cap ($l of 10 rows)"; else fail "iterscan big_lateral_128k loop $x: '$l' rows, want < 10"; fi
  done
  # The type cases stop on the 64kB cap (on the seeded build when the M3 plan
  # was written: 139, 132, 139, 223, 663 and 608 of 990 rows removed).
  for x in halfvec_64k halfvec_cosine_64k sparsevec_64k sparsevec_cosine_drop_64k bit_hamming_64k bit_jaccard_64k; do
    a="$(removed "$x")"
    if [ -n "$a" ] && [ "$a" -lt 990 ]; then pass "iterscan $x stops early ($a removed)"; else fail "iterscan $x: removed '$a', want < 990"; fi
  done
  # Every case compares a memory trace.
  for f in "$PGV_WORK"/iterscan/ref/oracle.*.out; do
    n="$(grep -c '^memory: ' "$f" || true)"
    if [ "$n" -ge 1 ]; then pass "iterscan $(basename "$f" .out): $n memory lines"; else fail "iterscan $(basename "$f" .out): no memory lines"; fi
  done
}

# The memory trace is opt-in: without SET pgrust.hnsw_memory, pgrust prints
# no memory lines, so the comparison must fail.
test_iterscan_memory_off() {
  local w="$PGV_WORK/iterscan-nomem"
  if PGV_WORK="$w" PGV_ITERSCAN_MEMORY= "$here/../run-iterscan.sh" pgrust relaxed_64k >"$w.log" 2>&1; then
    fail "iterscan: pgrust without pgrust.hnsw_memory differs"
  elif grep -q $'^relaxed_64k\tFAIL$' "$w/iterscan/pgrust/summary.tsv" 2>/dev/null; then
    pass "iterscan: pgrust without pgrust.hnsw_memory differs"
  else
    fail "iterscan: pgrust without pgrust.hnsw_memory: wrong failure (see $w.log)"
  fi
}

# sqldiff, C vs C: one script gives identical sessions on the reference and
# the seeded build, error lines included.
test_sqldiff_ref() {
  if printf '%s\n' "SELECT '[1,2,3]'::vector;" "SELECT '[1,2'::vector;" |
    "$here/../sqldiff.sh" ref - >"$PGV_WORK/sqldiff-ref.log" 2>&1; then
    pass "sqldiff ref: identical"
  else
    fail "sqldiff ref: identical (see $PGV_WORK/sqldiff-ref.log)"
  fi
}

# Negative control: version() differs between C and pgrust.
test_sqldiff_detects_difference() {
  if echo 'SELECT version();' | "$here/../sqldiff.sh" pgrust - >"$PGV_WORK/sqldiff-neg.log" 2>&1; then
    fail "sqldiff flags a difference"
  elif grep -q 'DIFFERENT' "$PGV_WORK/sqldiff-neg.log"; then
    pass "sqldiff flags a difference"
  else
    fail "sqldiff flags a difference: wrong failure (see $PGV_WORK/sqldiff-neg.log)"
  fi
}

# Exact differential, C vs C: the reference and the seeded build agree on
# every section, error locations included.
test_diff_ref() {
  local n
  if "$here/../run-diff.sh" ref >"$PGV_WORK/diff-ref.log" 2>&1; then
    pass "diff ref: exit 0"
  else
    fail "diff ref: exit 0 (see $PGV_WORK/diff-ref.log)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/diff/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 21 ]; then pass "diff ref: 21 ok"; else fail "diff ref: '$n' ok, want 21"; fi
}

# Negative control: a B side without l2_normalize(halfvec) must be reported
# in the deck section that calls it (normalize).
test_diff_detects_difference() {
  local w="$PGV_WORK/diff-neg"
  if PGV_WORK="$w" PGV_DIFF_COUNT=50 \
    PGV_DIFF_PERTURB_B="CREATE EXTENSION vector; ALTER EXTENSION vector DROP FUNCTION l2_normalize(halfvec); DROP FUNCTION l2_normalize(halfvec)" \
    "$here/../run-diff.sh" ref >"$w.log" 2>&1; then
    fail "diff flags a missing function"
  elif grep -q $'^normalize\tFAIL$' "$w/diff/ref/summary.tsv" 2>/dev/null; then
    pass "diff flags a missing function"
  else
    fail "diff flags a missing function: wrong failure (see $w.log)"
  fi
}

# Database cluster state of data directory $1, from pg_controldata.
cluster_state() {
  "$PG_TOOLS/bin/pg_controldata" "$1" 2>/dev/null | sed -n 's/^Database cluster state: *//p'
}

# server_stop's QUIT: an immediate stop leaves a cluster that needs recovery.
test_server_stop_immediate() {
  local st
  if ! "$here/../server.sh" fresh ref >/dev/null 2>&1; then
    fail "immediate stop: the ref server starts"
    return
  fi
  server_stop ref QUIT
  st="$(cluster_state "$PGV_WORK/data/ref")"
  if [ "$st" = "in production" ]; then pass "immediate stop leaves 'in production'"; else fail "immediate stop left '$st'"; fi
  if [ ! -f "$PGV_WORK/ref.pid" ]; then pass "immediate stop removes the pidfile"; else fail "immediate stop kept the pidfile"; fi
}

# docker-lib.sh: both images start; a clean and an immediate stop copy out a
# shut-down and an in-production cluster; the 0.8.7 image boots a host copy
# without writing it, keeps its own copy across a restart, and replays a
# crashed copy; nothing is left behind.
test_docker_lib() {
  local w="$PGV_WORK/docker-lib" before after v ctl
  before="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  rm -rf "$w"
  mkdir -p "$w"
  if ! dk_start_new "$PGV_DOCKER_IMAGE_081" -c autovacuum=off >"$w.log" 2>&1; then
    fail "docker-lib: the 0.8.1 image starts (see $w.log)"
    dk_rm
    return
  fi
  pass "docker-lib: the 0.8.1 image starts"
  v="$(dk_psql -Atq -c "CREATE EXTENSION vector" -c "CREATE TABLE t AS SELECT '[1,2]'::vector AS v" \
    -c "SELECT extversion FROM pg_extension WHERE extname = 'vector'" 2>&1 || true)"
  if [ "$v" = 0.8.1 ]; then pass "docker-lib: vector 0.8.1"; else fail "docker-lib: extversion '$v'"; fi
  if dk_stop_fast && dk_copy_out "$w/clean" >>"$w.log" 2>&1; then
    pass "docker-lib: clean stop and copy out"
  else
    fail "docker-lib: clean stop and copy out (see $w.log)"
  fi
  v="$(cluster_state "$w/clean")"
  if [ "$v" = "shut down" ]; then pass "docker-lib: the copy is shut down"; else fail "docker-lib: the copy is '$v'"; fi
  ctl="$("$PG_TOOLS/bin/pg_controldata" "$w/clean")"
  if ! dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/clean" >>"$w.log" 2>&1; then
    fail "docker-lib: the 0.8.7 image boots the copy (see $w.log)"
    dk_rm
    return
  fi
  pass "docker-lib: the 0.8.7 image boots the copy"
  dk_psql -qc "INSERT INTO t VALUES ('[3,4]')" >/dev/null
  if dk_stop_fast && dk_start >>"$w.log" 2>&1 && [ "$(dk_psql -Atc 'SELECT count(*) FROM t' 2>&1)" = 2 ]; then
    pass "docker-lib: a restart keeps the container's copy"
  else
    fail "docker-lib: a restart keeps the container's copy (see $w.log)"
  fi
  dk_psql -qc "INSERT INTO t VALUES ('[5,6]')" >/dev/null
  if dk_stop_immediate && dk_copy_out "$w/crash" >>"$w.log" 2>&1; then
    pass "docker-lib: immediate stop and copy out"
  else
    fail "docker-lib: immediate stop and copy out (see $w.log)"
  fi
  v="$(cluster_state "$w/crash")"
  if [ "$v" = "in production" ]; then pass "docker-lib: the crashed copy needs recovery"; else fail "docker-lib: the crashed copy is '$v'"; fi
  if [ "$("$PG_TOOLS/bin/pg_controldata" "$w/clean")" = "$ctl" ]; then
    pass "docker-lib: the host directory is untouched"
  else
    fail "docker-lib: the host directory changed"
  fi
  if dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/crash" >>"$w.log" 2>&1 && [ "$(dk_psql -Atc 'SELECT count(*) FROM t' 2>&1)" = 3 ]; then
    pass "docker-lib: the crashed copy replays"
  else
    fail "docker-lib: the crashed copy replays (see $w.log)"
  fi
  dk_rm
  if [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then pass "docker-lib: container removed"; else fail "docker-lib: $DK_NAME left behind"; fi
  after="$(docker volume ls -q -f dangling=true | wc -l | tr -d ' ')"
  if [ "$after" -le "$before" ]; then pass "docker-lib: no leaked volume"; else fail "docker-lib: leaked $((after - before)) anonymous volume(s)"; fi
  rm -rf "$w"
}

# The on-disk SQL on a fresh local C reference, without Docker: the fixture
# loads; the queries scan all 25 vector indexes and fail only where they
# mean to (the two wrong-dimension queries); the relation list has 41 rows;
# the crash writes load; the follow-up runs without errors and its index
# row counts are full; the update script reaches repair-verify.sql's ext row.
test_ondisk_sql() {
  local w="$PGV_WORK/ondisk-sql" sql="$here/../sql" port n
  port="$(port_for ref)"
  rm -rf "$w"
  mkdir -p "$w"
  if ! "$here/../server.sh" fresh ref >"$w/server.log" 2>&1; then
    fail "ondisk sql: the ref server starts (see $w/server.log)"
    return
  fi
  if (cd "$sql" && pgv_psql "$port" -f ondisk-fixture.sql) >"$w/fixture.log" 2>&1; then
    pass "ondisk sql: the fixture loads"
  else
    fail "ondisk sql: the fixture loads (see $w/fixture.log)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-queries.sql) >"$w/queries.out" 2>&1 || true
  n="$(sed -n -E 's/^ *->  Index Scan using ([a-z0-9_]+) on (vt|ct|ut|wide)$/\1/p' "$w/queries.out" | sort -u | wc -l | tr -d ' ')"
  if [ "$n" = 25 ]; then pass "ondisk sql: 25 vector indexes scanned"; else fail "ondisk sql: $n vector indexes scanned, want 25"; fi
  n="$(grep -c 'ERROR:' "$w/queries.out" || true)"
  if [ "$n" = 2 ] && [ "$(grep -c 'ERROR:  expected 8 dimensions, not 3' "$w/queries.out" || true)" = 2 ]; then
    pass "ondisk sql: only the wrong-dimension queries fail"
  else
    fail "ondisk sql: $n errors in $w/queries.out, want the 2 wrong-dimension ones"
  fi
  n="$( (cd "$sql" && pgv_psql "$port" -f ondisk-relfiles.sql) | grep -c '|' || true)"
  if [ "$n" = 41 ]; then pass "ondisk sql: 41 fixture relations"; else fail "ondisk sql: $n fixture relations, want 41"; fi
  if pgv_psql "$port" -c CHECKPOINT >/dev/null && (cd "$sql" && pgv_psql "$port" -f ondisk-crash.sql) >"$w/crash.log" 2>&1; then
    pass "ondisk sql: the crash writes load"
  else
    fail "ondisk sql: the crash writes load (see $w/crash.log)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-followup.sql) >"$w/followup.out" 2>&1 || true
  if ! grep -q 'ERROR:' "$w/followup.out" &&
    [ "$(grep -cE '^ +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +10 \| +5$' "$w/followup.out" || true)" = 2 ]; then
    pass "ondisk sql: the follow-up runs, every index returns full rows"
  else
    fail "ondisk sql: the follow-up (see $w/followup.out)"
  fi
  (cd "$sql" && pgv_psql "$port" -a -v ON_ERROR_STOP=0 -f ondisk-update.sql) >"$w/update.out" 2>&1 || true
  if [ "$(grep -c 'ERROR:' "$w/update.out" || true)" = 4 ] && grep -q '^ext|0.8.7|true$' "$w/update.out"; then
    pass "ondisk sql: the update script runs"
  else
    fail "ondisk sql: the update script (see $w/update.out)"
  fi
  "$here/../server.sh" stop ref >/dev/null 2>&1 || true
}

# A value from the self-test capture's facts.tsv.
ondisk_fact() {
  awk -F'\t' -v k="$1" '$1 == k { print $2 }' "$PGV_WORK/ondisk/capture/facts.tsv" 2>/dev/null
}

# The capture (capture --force, in the self-test work dir) exercises what M5
# claims: clean and crashed directories; past the crash's redo point a WAL
# full of GenericXLog deltas, Heap and Btree records, and the open
# transaction's index records; 42 fixture relations and 25 scanned vector
# indexes; a TOASTed column; the 0.8.1 -> 0.8.7 update. A second capture is
# a cache hit.
test_ondisk_capture() {
  local log="$PGV_WORK/ondisk-capture.log" g f h b i
  if ! "$here/../run-ondisk.sh" capture --force >"$log" 2>&1; then
    fail "ondisk capture: exit 0 (see $log)"
    return
  fi
  pass "ondisk capture: exit 0"
  [ "$(ondisk_fact clean_state)/$(ondisk_fact v081_state)" = "shut down/shut down" ] &&
    pass "ondisk capture: clean and v081 shut down" ||
    fail "ondisk capture: clean '$(ondisk_fact clean_state)', v081 '$(ondisk_fact v081_state)'"
  [ "$(ondisk_fact crash_state)" = "in production" ] &&
    pass "ondisk capture: the crash copy needs recovery" ||
    fail "ondisk capture: crash state '$(ondisk_fact crash_state)'"
  g="$(ondisk_fact crash_generic_records)"
  f="$(ondisk_fact crash_generic_fpi)"
  [ "${g:-0}" -ge 100000 ] && [ "${f:-0}" -ge 1000 ] && [ $((${g:-0} - ${f:-0})) -ge 100000 ] &&
    pass "ondisk capture: $g Generic records after the redo point, $f with full-page images" ||
    fail "ondisk capture: $g Generic records, $f with full-page images (want >= 100000 and >= 1000)"
  h="$(ondisk_fact crash_heap_records)"
  b="$(ondisk_fact crash_btree_records)"
  [ "${h:-0}" -ge 1000 ] && [ "${b:-0}" -ge 1000 ] &&
    pass "ondisk capture: $h Heap and $b Btree records to replay" ||
    fail "ondisk capture: $h Heap and $b Btree records (want >= 1000 each)"
  i="$(ondisk_fact crash_inflight_generic)"
  [ "${i:-0}" -ge 10000 ] &&
    pass "ondisk capture: the open transaction wrote $i Generic records" ||
    fail "ondisk capture: the open transaction wrote $i Generic records (want >= 10000)"
  [ "$(ondisk_fact crash_relations)/$(ondisk_fact vector_indexes)" = 42/25 ] &&
    pass "ondisk capture: 42 relations, 25 vector indexes scanned" ||
    fail "ondisk capture: $(ondisk_fact crash_relations) relations, $(ondisk_fact vector_indexes) vector indexes (want 42, 25)"
  [ "$(ondisk_fact wide_toasted)" = t ] && pass "ondisk capture: wide is TOASTed" || fail "ondisk capture: wide is not TOASTed"
  [ "$(ondisk_fact v081_before) $(ondisk_fact v081_after)" = "0.8.1 0.8.7" ] &&
    pass "ondisk capture: v081 goes from 0.8.1 to 0.8.7" ||
    fail "ondisk capture: v081 '$(ondisk_fact v081_before)' -> '$(ondisk_fact v081_after)'"
  if "$here/../run-ondisk.sh" capture >"$log.2" 2>&1 && ! grep -q '^capture: clean' "$log.2"; then
    pass "ondisk capture: a second capture is a cache hit"
  else
    fail "ondisk capture: a second capture rebuilt or failed (see $log.2)"
  fi
}

# Forward gate, C against C: the local clang build reproduces the gcc image's
# answers, replayed pages and follow-up on all three directories.
test_ondisk_ref() {
  local n
  if "$here/../run-ondisk.sh" ref clean crash v081 >"$PGV_WORK/ondisk-ref.log" 2>&1; then
    pass "ondisk ref: exit 0"
  else
    fail "ondisk ref: exit 0 (see $PGV_WORK/ondisk-ref.log)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/ondisk/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 7 ]; then pass "ondisk ref: 7 ok"; else fail "ondisk ref: '$n' ok, want 7"; fi
}

# Negative control: deleting the rows nearest the first query right after
# the subject boots must fail the queries row and the replayed pages.
test_ondisk_detects_difference() {
  local s="$PGV_WORK/ondisk/ref/summary.tsv"
  if PGV_ONDISK_PERTURB_SQL="DELETE FROM vt WHERE v <-> '[3,1,4,1,5,9,2,6]' < 4" \
    "$here/../run-ondisk.sh" ref clean crash >"$PGV_WORK/ondisk-neg.log" 2>&1; then
    fail "ondisk flags a changed table"
  elif grep -q $'^clean_queries\tFAIL' "$s" && grep -q $'^crash_pages\tFAIL' "$s"; then
    pass "ondisk flags a changed table"
  else
    fail "ondisk flags a changed table: wrong failure (see $PGV_WORK/ondisk-neg.log)"
  fi
}

# Reverse, C against C: the gcc image boots what the local clang build wrote,
# stopped cleanly and crashed, with the same answers and replayed pages;
# pg_amcheck is clean and there are no pgrust-only rows.
test_ondisk_reverse_ref() {
  local r="$PGV_WORK/ondisk/ref/reverse.tsv" want
  if "$here/../run-ondisk.sh" ref reverse >"$PGV_WORK/ondisk-reverse.log" 2>&1; then
    pass "ondisk reverse ref: exit 0"
  else
    fail "ondisk reverse ref: exit 0 (see $PGV_WORK/ondisk-reverse.log)"
  fi
  want="clean_boot ok
clean_queries identical
clean_amcheck clean
clean_pgrust_rows none
clean_followup identical
crash_boot ok
crash_pages identical
crash_queries identical"
  if [ "$(tr '\t' ' ' <"$r" 2>/dev/null)" = "$want" ]; then
    pass "ondisk reverse ref: all eight checks identical"
  else
    fail "ondisk reverse ref: $r is not all identical"
  fi
  if [ -s "$PGV_WORK/ondisk/ref/reverse.md" ]; then
    pass "ondisk reverse ref: reverse.md written"
  else
    fail "ondisk reverse ref: no reverse.md"
  fi
}

# Every tier clears its old results before anything that can fail after
# its argument checks. With its binary missing, a direct run must exit
# non-zero and leave no summary.tsv behind: a stale all-ok file reads as a
# pass to anything that reads it without the exit status. $1 tier; $2 the
# binary's variable, pointed at nothing.
stale_results_check() {
  local w="$PGV_WORK/stale"
  mkdir -p "$w/$1/ref"
  printf 'old\tok\n' >"$w/$1/ref/summary.tsv"
  if env PGV_WORK="$w" "$2=/nonexistent" "$here/../run-$1.sh" ref >"$w/$1.log" 2>&1; then
    fail "stale results: run-$1.sh exits 0 without $2"
  elif [ -e "$w/$1/ref/summary.tsv" ]; then
    fail "stale results: run-$1.sh left its old summary.tsv (see $w/$1.log)"
  else
    pass "stale results: run-$1.sh clears its old results before it dies"
  fi
}

test_stale_results() {
  local w="$PGV_WORK/stale"
  rm -rf "$w"
  stale_results_check bytecmp PAGEMASK_BIN
  stale_results_check diff DIFFRUNNER_BIN
  stale_results_check ondisk PAGEMASK_BIN
  # An argument error comes before the clear: a typo touches nothing.
  mkdir -p "$w/ondisk/ref"
  printf 'old\tok\n' >"$w/ondisk/ref/summary.tsv"
  if PGV_WORK="$w" "$here/../run-ondisk.sh" ref nosuchpart >"$w/usage.log" 2>&1; then
    fail "stale results: run-ondisk.sh accepts a bad part"
  elif [ -s "$w/ondisk/ref/summary.tsv" ]; then
    pass "stale results: a usage error leaves the old results alone"
  else
    fail "stale results: a usage error removed the old results"
  fi
  rm -rf "$w"
}

# A stub tier for test_run_all_exit_status: $1/run-$2.sh writes the rows $4
# (space-separated name:result pairs; "" writes no summary.tsv) and exits $3.
stub_tier() {
  {
    echo '#!/usr/bin/env bash'
    echo 'set -eu'
    if [ -n "$4" ]; then
      echo "mkdir -p \"\$PGV_WORK/$2/\$1\""
      printf '%s\n' "for r in $4; do printf '%s\\t%s\\n' \"\${r%%:*}\" \"\${r#*:}\"; done >\"\$PGV_WORK/$2/\$1/summary.tsv\""
    fi
    echo "exit $3"
  } >"$1/run-$2.sh"
  chmod +x "$1/run-$2.sh"
}

# run-all.sh counts every tier's exit status: a tier that died after writing
# only ok rows fails, a tier that wrote nothing reports "(no results)" even
# over an old summary.tsv or with exit 0, a tier with a FAIL row gets no
# extra row, and a missing reverse.tsv is reported. Stub tiers
# (PGV_TIER_DIR): no server runs.
test_run_all_exit_status() {
  local w="$PGV_WORK/run-all" r t
  r="$w/report-ref.md"
  # Without the knob, run-all.sh would run the real tiers.
  if ! grep -q PGV_TIER_DIR "$here/../run-all.sh"; then
    fail "run-all: no PGV_TIER_DIR knob"
    return
  fi
  rm -rf "$w"
  mkdir -p "$w/bin" "$w/bytecmp/ref"
  stub_tier "$w/bin" regress 1 "a:ok b:ok"
  stub_tier "$w/bin" tap 1 "t:FAIL"
  stub_tier "$w/bin" bytecmp 1 ""
  printf 'old\tok\n' >"$w/bytecmp/ref/summary.tsv"
  stub_tier "$w/bin" iterscan 0 ""
  stub_tier "$w/bin" diff 0 "d:ok"
  stub_tier "$w/bin" ondisk 0 "o:ok"
  if PGV_WORK="$w" PGV_TIER_DIR="$w/bin" "$here/../run-all.sh" ref >"$w.log" 2>&1; then
    fail "run-all: exits 0 with failing tiers"
  else
    pass "run-all: exits non-zero with failing tiers"
  fi
  if grep -qxF '| regress | (run-regress.sh exited 1) | FAIL |' "$r"; then
    pass "run-all: a tier that died after ok rows fails"
  else
    fail "run-all: no exit-status row for regress (see $r)"
  fi
  if grep -qxF '| bytecmp | (no results; run-bytecmp.sh exited 1) | FAIL |' "$r" && ! grep -qF '| bytecmp | old |' "$r"; then
    pass "run-all: a tier with no results fails, its old rows dropped"
  else
    fail "run-all: bytecmp's rows (see $r)"
  fi
  if ! grep -qF '| tap | (run-tap.sh' "$r"; then
    pass "run-all: a tier with a FAIL row gets no extra row"
  else
    fail "run-all: an extra row for tap (see $r)"
  fi
  if grep -qxF '| iterscan | (no results; run-iterscan.sh exited 0) | FAIL |' "$r"; then
    pass "run-all: a tier that exits 0 with no rows fails"
  else
    fail "run-all: iterscan's empty run (see $r)"
  fi
  if grep -qxF '**4/8 passed**' "$r"; then pass "run-all: 4/8 passed"; else fail "run-all: the totals (see $r)"; fi
  if grep -qF '(no results: run-ondisk.sh wrote no reverse.tsv' "$r"; then
    pass "run-all: a missing reverse.tsv is reported"
  else
    fail "run-all: the reverse section (see $r)"
  fi
  for t in regress tap bytecmp iterscan diff ondisk; do stub_tier "$w/bin" "$t" 0 "x:ok"; done
  if PGV_WORK="$w" PGV_TIER_DIR="$w/bin" "$here/../run-all.sh" ref >"$w.log.2" 2>&1 && grep -qxF '**6/6 passed**' "$r"; then
    pass "run-all: all-ok tiers pass"
  else
    fail "run-all: all-ok tiers (see $w.log.2)"
  fi
}

# Run "$@" in the background and kill it after $1 seconds; exit 124 then.
within() {
  local secs="$1" pid rc=0
  shift
  "$@" &
  pid=$!
  for _ in $(seq 1 $((secs * 2))); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    return 124
  fi
  wait "$pid" || rc=$?
  return "$rc"
}

# docker-lib.sh fails instead of hanging or leaking: a docker run that fails
# (its port held) leaves no Created container behind, from dk_start_new and
# dk_boot_dir alike; dk_stop_immediate refuses at once when DK_DATA is empty
# (it used to wait forever on the running container) and fails fast when
# there is no container.
test_docker_lib_failures() {
  local w="$PGV_WORK/docker-lib-failures" holder rc
  rm -rf "$w"
  mkdir -p "$w/fake"
  : >"$w.log"
  echo 18 >"$w/fake/PG_VERSION"
  # Hold the port, so docker run creates the container and then fails to
  # start it. ReuseAddr: an earlier test's connections through the port may
  # still be in TIME_WAIT, which blocks a plain bind (not Docker's, which the
  # held port still refuses). The holder touches $w/held once it listens.
  perl -MIO::Socket::INET -e '
    my $s = IO::Socket::INET->new(LocalAddr => "127.0.0.1", LocalPort => $ARGV[0], Listen => 1, ReuseAddr => 1)
      or die "bind: $!\n";
    open(my $f, ">", $ARGV[1]) or die "$ARGV[1]: $!\n";
    close $f;
    sleep 120' "$PGV_PORT_DOCKER" "$w/held" 2>>"$w.log" &
  holder=$!
  for _ in $(seq 1 30); do
    [ ! -e "$w/held" ] || break
    sleep 0.5
  done
  if [ ! -e "$w/held" ]; then
    fail "docker-lib: could not hold port $PGV_PORT_DOCKER (see $w.log)"
    kill "$holder" 2>/dev/null || true
    return
  fi
  if dk_start_new "$PGV_DOCKER_IMAGE" >>"$w.log" 2>&1; then
    fail "docker-lib: dk_start_new succeeds on a held port"
  elif [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then
    pass "docker-lib: a failed dk_start_new leaves no container"
  else
    fail "docker-lib: a failed dk_start_new left $DK_NAME behind"
  fi
  if dk_boot_dir "$PGV_DOCKER_IMAGE" "$w/fake" >>"$w.log" 2>&1; then
    fail "docker-lib: dk_boot_dir succeeds on a held port"
  elif [ -z "$(docker ps -aq -f "name=^${DK_NAME}\$")" ]; then
    pass "docker-lib: a failed dk_boot_dir leaves no container"
  else
    fail "docker-lib: a failed dk_boot_dir left $DK_NAME behind"
  fi
  kill "$holder" 2>/dev/null || true
  wait "$holder" 2>/dev/null || true
  dk_rm
  if dk_start_new "$PGV_DOCKER_IMAGE" >>"$w.log" 2>&1; then
    rc=0
    DK_DATA='' within 10 dk_stop_immediate >>"$w.log" 2>&1 || rc=$?
    if [ "$rc" = 124 ]; then
      fail "docker-lib: dk_stop_immediate hangs with DK_DATA empty"
    elif [ "$rc" = 0 ]; then
      fail "docker-lib: dk_stop_immediate returns 0 with DK_DATA empty"
    elif [ "$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)" = true ]; then
      pass "docker-lib: dk_stop_immediate refuses at once with DK_DATA empty"
    else
      fail "docker-lib: dk_stop_immediate with DK_DATA empty stopped the container"
    fi
  else
    fail "docker-lib: the 0.8.7 image starts (see $w.log)"
  fi
  dk_rm
  rc=0
  DK_NAME=pgrust-pgvector-ondisk-absent within 10 dk_stop_immediate >>"$w.log" 2>&1 || rc=$?
  if [ "$rc" = 124 ] || [ "$rc" = 0 ]; then
    fail "docker-lib: dk_stop_immediate without a container (exit $rc)"
  else
    pass "docker-lib: dk_stop_immediate fails fast without a container"
  fi
  rm -rf "$w"
}

# capture_key of the scripts under $1 (scripts/pgvector or a copy).
ondisk_key() {
  (
    # shellcheck source=/dev/null
    . "$1/common.sh"
    # shellcheck source=/dev/null
    . "$1/docker-lib.sh"
    # shellcheck source=/dev/null
    . "$1/ondisk-capture.sh"
    capture_key
  )
}

# ensure_capture 0 of the scripts under $1 on the capture at $2, with capture
# itself replaced: prints "rebuilt" or "reused".
ondisk_ensure() {
  (
    # shellcheck source=/dev/null
    . "$1/common.sh"
    # shellcheck source=/dev/null
    . "$1/docker-lib.sh"
    # shellcheck source=/dev/null
    . "$1/ondisk-capture.sh"
    # shellcheck disable=SC2034 # ensure_capture reads it
    cap="$2"
    # shellcheck disable=SC2329 # ensure_capture calls it
    capture() { echo rebuilt; }
    r="$(ensure_capture 0)"
    echo "${r:-reused}"
  )
}

# The capture key covers the capture's inputs and nothing else: a comment in
# ondisk-capture.sh, docker-lib.sh or an on-disk SQL file changes it, and so
# does another psql version; a comment in run-ondisk.sh does not.
test_ondisk_capture_key() {
  local w="$PGV_WORK/ondisk-key" src k f c
  src="$(cd "$here/.." && pwd)"
  rm -rf "$w"
  mkdir -p "$w"
  cp -R "$src" "$w/pgvector"
  k="$(ondisk_key "$src" 2>"$w.log" || true)"
  if [ -n "$k" ] && [ "$(ondisk_key "$w/pgvector" 2>>"$w.log" || true)" = "$k" ]; then
    pass "ondisk key: a copy of the scripts has the same key"
  else
    fail "ondisk key: no key, or a copy's differs (see $w.log)"
    return
  fi
  echo '# a comment' >>"$w/pgvector/run-ondisk.sh"
  if [ "$(ondisk_key "$w/pgvector")" = "$k" ]; then
    pass "ondisk key: a comment in run-ondisk.sh keeps the key"
  else
    fail "ondisk key: a comment in run-ondisk.sh changes the key"
  fi
  for f in ondisk-capture.sh docker-lib.sh sql/ondisk-rows.sql sql/repair-verify.sql; do
    case "$f" in *.sql) c='-- a comment' ;; *) c='# a comment' ;; esac
    echo "$c" >>"$w/pgvector/$f"
    if [ "$(ondisk_key "$w/pgvector")" != "$k" ]; then
      pass "ondisk key: a comment in $f changes the key"
    else
      fail "ondisk key: a comment in $f keeps the key"
    fi
    cp "$src/$f" "$w/pgvector/$f"
  done
  # Both sides' transcripts come from $PG_TOOLS's psql, so another psql
  # version must not reuse a capture.
  mkdir -p "$w/pgref/pg/bin"
  printf '#!/bin/sh\necho "psql (PostgreSQL) 18.7"\n' >"$w/pgref/pg/bin/psql"
  chmod +x "$w/pgref/pg/bin/psql"
  if [ "$(PGREF="$w/pgref" ondisk_key "$w/pgvector")" != "$k" ]; then
    pass "ondisk key: another psql version changes the key"
  else
    fail "ondisk key: another psql version keeps the key"
  fi
  # ensure_capture rebuilds when the key file is missing (an interrupted or
  # failed capture writes none) or stale, and reuses a matching one.
  mkdir -p "$w/capture"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = rebuilt ]; then
    pass "ondisk key: a capture without a key is rebuilt"
  else
    fail "ondisk key: a capture without a key is reused"
  fi
  echo 0000000000000000 >"$w/capture/key"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = rebuilt ]; then
    pass "ondisk key: a capture with a stale key is rebuilt"
  else
    fail "ondisk key: a capture with a stale key is reused"
  fi
  echo "$k" >"$w/capture/key"
  if [ "$(ondisk_ensure "$w/pgvector" "$w/capture")" = reused ]; then
    pass "ondisk key: a capture with the current key is reused"
  else
    fail "ondisk key: a capture with the current key is rebuilt"
  fi
  rm -rf "$w"
}

# --- runner ---
if [ "$#" -eq 0 ]; then
  # Declaration order (declare -F would sort): a test may read the results of
  # one declared above it, e.g. test_bytecmp_cases reads test_bytecmp_ref's.
  # shellcheck disable=SC2046 # intentional splitting: function names have no spaces
  set -- $(sed -n 's/^\(test_[A-Za-z0-9_]*\)() {$/\1/p' "${BASH_SOURCE[0]}")
fi
for t in "$@"; do
  echo "# $t"
  "$t"
done
if [ "$fails" -ne 0 ]; then
  echo "$fails failure(s)"
  exit 1
fi
echo "all passed"

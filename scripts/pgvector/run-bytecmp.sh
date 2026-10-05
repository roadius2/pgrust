#!/usr/bin/env bash
# Byte-identical HNSW tier (spec §8.2). Build the same HNSW indexes on the
# seeded C reference (the oracle) and on the subject server, stop both cleanly,
# then compare every index relation file page by page after masking both sides
# with crates/bin/pagemask's `generic` rmgr: pd_lsn, pd_checksum and the
# pd_lower..pd_upper gap (generic_mask, generic_xlog.c:539; HNSW pages are
# written through GenericXLog). Everything else on every page must match.
#
# usage: run-bytecmp.sh {pgrust|ref} [case ...]   (default: the cases in CASES)
#        run-bytecmp.sh --list
#   pgrust: the subject is pgrust ($PGRUST_BIN), seeded with
#           SET pgrust.hnsw_build_seed = $PGV_BYTECMP_SEED (default 42).
#   ref:    the subject is a second, fresh seeded C cluster: C-vs-C proof that
#           the oracle and every case are deterministic (must always pass).
#   PGV_BYTECMP_SUBJECT={pgrust|ref|seeded} overrides the subject (self-tests);
#   PGV_BYTECMP_SEED= (empty) omits the SET (self-test of the opt-in seed).
#   PGV_KEEP_DATA=1 keeps each side's cluster under $PGV_WORK/bytecmp/<mode>/data
#           (default: deleted after a successful side, to save disk).
#
# The oracle is $PG_VEC_SEEDED (see common.sh): stock C pgvector does not seed
# its level RNG at all (SeedRandom(42) sits under #ifdef HNSW_MEMORY), so only
# that build is deterministic. Data are integer-valued so float math is exact
# or rounds once (identical under FMA and reassociation). Every case runs in its
# own session; insert_* cases build and then insert in ONE session, because
# on-disk inserts draw levels from the stream the build left behind.
set -euo pipefail
. "$(dirname "$0")/common.sh"

# Default cases. *_golomb data are tie-free: x = 2*p*k + (k*k % p) is an
# Erdos-Turan Golomb ruler, so all pairwise distances differ. *_grid and dups
# have many equal distances, which exercises C's pairingheap tie order.
CASES="l2_golomb l1_golomb m4_golomb wide_golomb unlogged_golomb insert_golomb l2_grid ip_grid dups insert_grid"
# Opt-in only (known divergences, run by name): spill cosine

GOLOMB1009='ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector'
GRID='ARRAY[i % 17, (i * 7) % 23, (i * 13) % 31]::vector'
SEED="${PGV_BYTECMP_SEED-42}"

# Print one case's SQL. Each case ends by selecting "rel|<label>|<relfile>"
# rows for the files to compare (paths relative to the data directory).
case_sql() {
  echo "SET max_parallel_maintenance_workers = 0;"
  echo "SET maintenance_work_mem = '64MB';"
  # Seeds pgrust (Task 5); C keeps it as an unused placeholder.
  [ -z "$SEED" ] || echo "SET pgrust.hnsw_build_seed = $SEED;"
  case "$1" in
    l2_golomb | l1_golomb)
      local ops=vector_l2_ops
      [ "$1" = l1_golomb ] && ops=vector_l1_ops
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    m4_golomb)
      # m = 4: about one element in four gets level >= 1 (levels reach 5).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, ARRAY[2*2003*k + (k*k) % 2003, 0, 0]::vector FROM (SELECT (i * 37) % 2003 AS k FROM generate_series(0, 1999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops) WITH (m = 4, ef_construction = 16);
SQL
      ;;
    wide_golomb)
      # 2000 dimensions: element + neighbor tuple exceed HNSW_MAX_SIZE, so the
      # neighbor tuple goes on the next page (CreateGraphPages).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(2000));
INSERT INTO $1 SELECT k, (ARRAY[2*1009*k + (k*k) % 1009] || array_fill(0, ARRAY[1999]))::vector FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 99) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    unlogged_golomb)
      # Also compares the init fork written by hnswbuildempty.
      cat <<SQL
CREATE UNLOGGED TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SELECT 'rel|idx_init|' || pg_relation_filepath('$1_idx') || '_init';
SQL
      ;;
    insert_golomb)
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 799) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(800, 999) i) s;
SQL
      ;;
    l2_grid | ip_grid)
      local ops=vector_l2_ops
      [ "$1" = ip_grid ] && ops=vector_ip_ops
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    dups)
      # Every value three times: heaptids arrays (FindDuplicateInMemory).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GOLOMB1009 FROM (SELECT i, ((i / 3) * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    insert_grid)
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 800) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
INSERT INTO $1 SELECT i, $GRID FROM generate_series(801, 1000) i;
SQL
      ;;
    spill)
      # Known divergence: C flushes when its Generation context has allocated
      # maintenance_work_mem (1436 tuples here); pgrust estimates (1840).
      cat <<SQL
SET maintenance_work_mem = '1MB';
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 6000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    cosine)
      # Known divergence: normalized values are not integers, and C's
      # -ffp-contract=fast fuses multiply-adds that pgrust rounds twice.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31]::vector FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_cosine_ops);
SQL
      ;;
    *) die "unknown case '$1' (default cases: $CASES; opt-in: spill cosine)" ;;
  esac
  echo "SELECT 'rel|idx|' || pg_relation_filepath('$1_idx');"
}

# run_side LABEL KIND: fresh cluster of server KIND, run every case, stop
# cleanly (the shutdown checkpoint writes every buffer, unlogged ones too),
# then copy and mask the files into $out/LABEL.
run_side() {
  local label="$1" kind="$2" port dir c line rel path
  port="$(port_for "$kind")"
  dir="$out/$label"
  mkdir -p "$dir"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  pgv_psql "$port" -c 'CREATE EXTENSION vector'
  for c in "${cases[@]}"; do
    case_sql "$c" >"$dir/$c.sql"
    pgv_psql "$port" -At -f "$dir/$c.sql" >"$dir/$c.out" 2>"$dir/$c.err" ||
      die "$label ($kind): case $c failed; see $dir/$c.err"
    # The seeded build reports its graph memory (HNSW_MEMORY's FlushPages
    # elog), which proves the seeded library built the index.
    if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$dir/$c.err"; then
      die "$label: case $c was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
    fi
  done
  server_stop "$kind"
  cp "$PGV_WORK/log/server-$kind.log" "$dir/server.log"
  for c in "${cases[@]}"; do
    while IFS= read -r line; do
      case "$line" in
        rel\|*) ;;
        *) continue ;;
      esac
      rel="${line#rel|}"
      path="${rel#*|}"
      rel="${rel%%|*}"
      cp "$out/data/$label/$path" "$dir/$c.$rel"
      "$PAGEMASK_BIN" generic "$dir/$c.$rel" "$dir/$c.$rel.masked" ||
        die "pagemask failed on $dir/$c.$rel"
    done <"$dir/$c.out"
  done
  # Nothing reads the cluster after the copies; PGV_KEEP_DATA=1 keeps it.
  [ "${PGV_KEEP_DATA:-}" = 1 ] || rm -rf "$out/data/$label"
}

# HnswMetaPageData.entryLevel (int16 at page offset 24 + 22): the graph height.
entry_level() {
  od -An -td2 -j46 -N2 "$1" | tr -d ' '
}

# Per-block summary of the masked bytes that differ.
describe_diff() {
  local a="$1" b="$2"
  echo "oracle $(wc -c <"$a" | tr -d ' ') bytes, subject $(wc -c <"$b" | tr -d ' ') bytes"
  { cmp -l "$a" "$b" 2>&1 || true; } | awk -v bs=8192 '
    /EOF/ { print; next }
    { o = $1 - 1; blk = int(o / bs); n[blk]++; if (!(blk in first)) first[blk] = o % bs; total++ }
    END {
      printf "%d byte(s) differ after masking\n", total
      for (blk in n) printf "block %d: %d byte(s), first at page offset %d\n", blk, n[blk], first[blk]
    }' | sort -n -k2 | head -40
}

if [ "${1:-}" = --list ]; then
  echo "$CASES"
  exit 0
fi
[ "$#" -ge 1 ] || die "usage: run-bytecmp.sh {pgrust|ref} [case ...]"
mode="$1"
shift
case "$mode" in
  pgrust) subject="${PGV_BYTECMP_SUBJECT:-pgrust}" ;;
  ref) subject="${PGV_BYTECMP_SUBJECT:-seeded}" ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
if [ "$#" -gt 0 ]; then
  cases=("$@")
else
  # shellcheck disable=SC2206 # intentional splitting: case names have no spaces
  cases=($CASES)
fi
for c in "${cases[@]}"; do case_sql "$c" >/dev/null; done

[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/bytecmp/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
: >"$out/cases.tsv"
for c in "${cases[@]}"; do
  result=ok
  for f in "$out/oracle/$c".*.masked; do
    [ -e "$f" ] || die "case $c produced no relation files"
    name="$(basename "$f" .masked)"
    g="$out/subject/$name.masked"
    printf '%s\t%s\t%s\t%s\n' "$c" "${name#"$c".}" "$(($(wc -c <"$f") / 8192))" \
      "$(entry_level "$f")" >>"$out/cases.tsv"
    if [ ! -e "$g" ] || ! cmp -s "$f" "$g"; then
      result=FAIL
      if [ -e "$g" ]; then describe_diff "$f" "$g"; else echo "subject has no $name"; fi >"$out/diff/$name.txt"
    fi
  done
  printf '%s\t%s\n' "$c" "$result" | tee -a "$out/summary.tsv"
done

passed="$(grep -c $'\tok$' "$out/summary.tsv" || true)"
echo "bytecmp ($mode, subject $subject): $passed/${#cases[@]} identical; diffs in $out/diff"
[ "$passed" -eq "${#cases[@]}" ]

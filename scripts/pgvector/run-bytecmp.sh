#!/usr/bin/env bash
# Byte-identical HNSW and IVFFlat tier (spec §8.2). Build the same indexes on the
# seeded C reference (the oracle) and on the subject server, stop both cleanly,
# then compare every index relation file page by page after masking both sides
# with crates/bin/pagemask's `generic` rmgr: pd_lsn, pd_checksum and the
# pd_lower..pd_upper gap (generic_mask, generic_xlog.c:539; HNSW pages are
# written through GenericXLog). Everything else on every page must match.
#
# usage: run-bytecmp.sh {pgrust|ref} [case ...]   (default: the cases in CASES)
#        run-bytecmp.sh --list
#   pgrust: the subject is pgrust ($PGRUST_BIN), seeded with
#           SET pgrust.hnsw_build_seed = $PGV_BYTECMP_SEED (default 42)
#           and SET pgrust.ivfflat_build_seed.
#   ref:    the subject is a second, fresh seeded C cluster: C-vs-C proof that
#           the oracle and every default case are deterministic (must always
#           pass; the opt-in insert_grid, run by name, is not).
#   PGV_BYTECMP_SUBJECT={pgrust|ref|seeded} overrides the subject (self-tests);
#   PGV_BYTECMP_SEED= (empty) omits the SET (self-test of the opt-in seed).
#   PGV_KEEP_DATA=1 keeps each side's cluster under $PGV_WORK/bytecmp/<mode>/data
#           (default: deleted after a successful side, to save disk).
#
# The oracle is $PG_VEC_SEEDED (see common.sh): stock C pgvector seeds neither
# access method (SeedRandom(42) sits under #ifdef HNSW_MEMORY and #ifdef
# IVFFLAT_BENCH), so only that build is deterministic. HNSW data are mostly
# integer-valued so float math is exact or rounds once (identical under FMA
# and reassociation); the non-integer cases (sparsevec_l2_frac and the cosine
# cases) say in their comments why C and pgrust round alike. The vector and
# halfvec IVFFlat cases (ivf_*) use well-separated groups of four integer
# points: every k-means and assignment comparison has a clear winner, and the
# stored centers are element-wise sums divided once, which C and pgrust round
# alike. ivf_bit_hamming uses md5 bit(64) data, whose integer Hamming
# distances tie often, so it pins bit's tie order. Every case
# runs in its own session; insert_* cases build and then insert in ONE
# session, because on-disk inserts draw levels from the stream the build
# left behind.
set -euo pipefail
. "$(dirname "$0")/common.sh"

# Default cases. Golomb data with p = 1009 (l2_, l1_, wide_, unlogged_ and
# insert_golomb) are tie-free: x = 2*p*k + (k*k % p) is an Erdos-Turan Golomb
# ruler, so all pairwise distances differ. *_grid and dups have many equal
# distances, which exercises C's pairingheap tie order.
# insert_grid, the tie-heavy on-disk insert case, is opt-in (see its arm).
CASES="l2_golomb l1_golomb m4_golomb wide_golomb unlogged_golomb insert_golomb l2_grid ip_grid dups big_golomb halfvec_l2_grid halfvec_cosine_grid sparsevec_l2_grid sparsevec_l2_frac sparsevec_cosine_grid insert_sparsevec_cosine wide_sparsevec bit_hamming bit_jaccard bq_expression ivf_l2_groups ivf_big ivf_halfvec_groups ivf_bit_hamming ivf_insert ivf_unlogged ivf_vacuum ivf_wide"
# Opt-in only (known divergences, run by name): spill cosine insert_grid

GOLOMB1009='ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector'
GRID='ARRAY[i % 17, (i * 7) % 23, (i * 13) % 31]::vector'
# The same grid as typed arrays; _POS has no zero coordinate (cosine needs
# a non-zero norm). MD5BITS: 64 pseudo-random bits per row.
GRID3='ARRAY[i % 17, (i * 7) % 23, (i * 13) % 31]'
GRID3_POS='ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31]'
MD5BITS="('x' || substr(md5(i::text), 1, 16))::bit(64)"
# IVFFlat groups: ten groups of four points, 256 apart; a group's mean is
# its corner + (0.5, 0.5).
IVF_GROUPS='ARRAY[256 * ((i % 10) % 5) + (i / 10) % 2, 256 * ((i % 10) / 5) + ((i / 10) / 2) % 2, 0]'
IVF_GROUPS2='ARRAY[256 * (i % 2) + (i / 2) % 2, ((i / 2) / 2) % 2, 0]'
SEED="${PGV_BYTECMP_SEED-42}"

# Print one case's SQL. Each case ends by selecting "rel|<label>|<relfile>"
# rows for the files to compare (paths relative to the data directory).
case_sql() {
  echo "SET max_parallel_maintenance_workers = 0;"
  echo "SET maintenance_work_mem = '64MB';"
  # Seeds pgrust (Task 5); C keeps it as an unused placeholder.
  [ -z "$SEED" ] || echo "SET pgrust.hnsw_build_seed = $SEED;"
  [ -z "$SEED" ] || echo "SET pgrust.ivfflat_build_seed = $SEED;"
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
      # Known divergence (opt-in): on-disk inserts into a tie-heavy graph. C's
      # HnswUpdateConnection breaks equal distances by raw element pointer
      # (CompareCandidateDistances, hnswutils.c:1007-1011, base == NULL); the
      # new element lives in insertCtx (AllocSet, hnswinsert.c:788) and the
      # loaded neighbors in updateCtx (Generation, hnswinsert.c:551), whose
      # relative addresses vary per run, so C differs from itself (~2 in 37
      # seeded C-vs-C runs). pgrust ranks the new element first, C's usual order.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 800) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
INSERT INTO $1 SELECT i, $GRID FROM generate_series(801, 1000) i;
SQL
      ;;
    big_golomb)
      # 10,000 rows (p = 10007): C's in-memory graph (about 6 MB) spans
      # several 1 MB graph blocks. Its float4 values exceed 2^24 and round, so
      # some distances tie (often a point's two nearest neighbors); whether a
      # tie reaches the cross-block pointer tie-break is unmeasured.
      # run-iterscan.sh's `big` table builds this same index.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, ARRAY[2*10007*k + (k*k) % 10007, 0, 0]::vector FROM (SELECT (i * 37) % 10007 AS k FROM generate_series(0, 9999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    halfvec_l2_grid | halfvec_cosine_grid)
      # Small integers are exact halves. Cosine: products of normalized
      # halves are exact in f32, so C's fused sums equal pgrust's.
      local ops=halfvec_l2_ops grid="$GRID3"
      if [ "$1" = halfvec_cosine_grid ]; then ops=halfvec_cosine_ops; grid="$GRID3_POS"; fi
      cat <<SQL
CREATE TABLE $1 (id int, v halfvec(3));
INSERT INTO $1 SELECT i, $grid::halfvec FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    sparsevec_l2_grid | sparsevec_cosine_grid)
      # 1 to 3 non-zeros per row. Cosine: the normalized values' inner
      # products fuse on arm64 in C and in pgrust alike.
      local ops=sparsevec_l2_ops grid="$GRID3"
      if [ "$1" = sparsevec_cosine_grid ]; then ops=sparsevec_cosine_ops; grid="$GRID3_POS"; fi
      cat <<SQL
CREATE TABLE $1 (id int, v sparsevec(3));
INSERT INTO $1 SELECT i, $grid::vector::sparsevec FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    sparsevec_l2_frac)
      # Non-integer values, so the L2 sums round. With at most 3 non-zeros
      # no L2 tail reaches 4 entries, so C and pgrust round every step alike
      # (fused on arm64: sparsevec.rs fmadd_step).
      cat <<SQL
CREATE TABLE $1 (id int, v sparsevec(3));
INSERT INTO $1 SELECT i, ARRAY[(i % 17)::real * 0.37::real, ((i * 7) % 23)::real * 0.53::real, ((i * 13) % 31)::real * 0.71::real]::vector::sparsevec FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_l2_ops);
SQL
      ;;
    insert_sparsevec_cosine)
      # Inserts after the build normalize on the on-disk path. The last row
      # loses an element when normalized (Review Focus 2): 1e-30/3e38
      # underflows in f32, so both sides store {1:1}/3.
      cat <<SQL
CREATE TABLE $1 (id int, v sparsevec(3));
INSERT INTO $1 SELECT i, $GRID3_POS::vector::sparsevec FROM generate_series(1, 800) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_cosine_ops);
INSERT INTO $1 SELECT i, $GRID3_POS::vector::sparsevec FROM generate_series(801, 1000) i;
INSERT INTO $1 VALUES (1001, '{1:3e38,3:1e-30}/3');
SQL
      ;;
    wide_sparsevec)
      # Review Focus 1: 1,000 non-zeros in 1e9 dimensions, 8,016-byte values
      # TOASTed in the heap. Each neighbor tuple goes on the next page, and
      # the 20 inserts after the build append pages. Values are distinct.
      local sv="('{' || (SELECT string_agg((j * 999983)::text || ':' || CASE WHEN j = 1 THEN i ELSE (i * j) % 7 + 1 END, ',') FROM generate_series(1, 1000) j) || '}/1000000000')::sparsevec"
      cat <<SQL
CREATE TABLE $1 (id int, v sparsevec(1000000000));
INSERT INTO $1 SELECT i, $sv FROM generate_series(1, 80) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v sparsevec_l2_ops);
INSERT INTO $1 SELECT i, $sv FROM generate_series(81, 100) i;
SQL
      ;;
    bit_hamming | bit_jaccard)
      # Hamming distances are small integers: many ties, popped in C's
      # pairing-heap order.
      cat <<SQL
CREATE TABLE $1 (id int, v bit(64));
INSERT INTO $1 SELECT i, $MD5BITS FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v $1_ops);
SQL
      ;;
    bq_expression)
      # Review Focus 5: pgvector's binary-quantization pattern. The index
      # attribute is the cast's bit(64), not the vector column.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(64));
INSERT INTO $1 SELECT i, (SELECT array_agg(CASE WHEN substr(md5(i::text || ':' || j::text), 1, 1) < '8' THEN 1 ELSE -1 END) FROM generate_series(1, 64) j)::vector(64) FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw ((binary_quantize(v)::bit(64)) bit_hamming_ops);
SQL
      ;;
    ivf_l2_groups)
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 999) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
SQL
      ;;
    ivf_big)
      # 20,000 rows and 10,000 samples: the reservoir replaces samples
      # (reservoir_get_next_S, sampler_random_fract), so the group means are
      # inexact, but every comparison still has a clear winner.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 19999) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
SQL
      ;;
    ivf_halfvec_groups)
      # 1024.5 is not a half: the centers 1024.5 round to 1024 on both sides.
      cat <<SQL
CREATE TABLE $1 (id int, v halfvec(3));
INSERT INTO $1 SELECT i, $IVF_GROUPS::halfvec FROM generate_series(0, 999) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v halfvec_l2_ops) WITH (lists = 10);
SQL
      ;;
    ivf_bit_hamming)
      # Hamming distances are integers; bit centers threshold their means.
      # Nearest-center ties are common, so this case pins the tie order.
      cat <<SQL
CREATE TABLE $1 (id int, v bit(64));
INSERT INTO $1 SELECT i, $MD5BITS FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v bit_hamming_ops) WITH (lists = 10);
SQL
      ;;
    ivf_insert)
      # Two lists of two pages each; the inserts fill them, append a page to
      # each (under the extension lock) and move the insert pages there.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $IVF_GROUPS2::vector FROM generate_series(0, 599) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 2);
INSERT INTO $1 SELECT i, $IVF_GROUPS2::vector FROM generate_series(600, 999) i;
SQL
      ;;
    ivf_unlogged)
      # Also compares the init fork ivfflatbuildempty writes: no heap, so
      # random centers from the stream the main-fork build left behind.
      cat <<SQL
CREATE UNLOGGED TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 999) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
SELECT 'rel|idx_init|' || pg_relation_filepath('$1_idx') || '_init';
SQL
      ;;
    ivf_vacuum)
      # ivfflatbulkdelete compacts every entry page and moves each list's
      # insert page back to its first page with deletions.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3)) WITH (autovacuum_enabled = false);
INSERT INTO $1 SELECT i, $IVF_GROUPS::vector FROM generate_series(0, 2999) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 10);
DELETE FROM $1 WHERE id % 3 = 0;
VACUUM $1;
SQL
      ;;
    ivf_wide)
      # 2,000 dimensions: one list item per page (IvfflatAppendPage in
      # CreateListPages) and one entry per page.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(2000));
INSERT INTO $1 SELECT i, (ARRAY[256 * (i % 5) + (i / 5) % 2, ((i / 5) / 2) % 2] || array_fill(0, ARRAY[1998]))::vector FROM generate_series(0, 49) i;
CREATE INDEX $1_idx ON $1 USING ivfflat (v vector_l2_ops) WITH (lists = 5);
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
    *) die "unknown case '$1' (default cases: $CASES; opt-in: spill cosine insert_grid)" ;;
  esac
  echo "SELECT 'rel|idx|' || pg_relation_filepath('$1_idx');"
}

# run_side LABEL KIND: fresh cluster of server KIND, run every case, stop
# cleanly (the shutdown checkpoint writes every buffer, unlogged ones too),
# then copy and mask the files into $out/LABEL.
run_side() {
  local label="$1" kind="$2" port dir c line rel path marker
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
    # The seeded build reports its HNSW graph memory (HNSW_MEMORY's
    # FlushPages elog) or its IVFFlat k-means time (IVFFLAT_BENCH's
    # IvfflatBench), which proves the seeded library built the index.
    marker='INFO:  memory: '
    case "$c" in ivf_*) marker='INFO:  k-means: ' ;; esac
    if [ "$kind" = seeded ] && ! grep -q "$marker" "$dir/$c.err"; then
      die "$label: case $c was not built by the seeded C pgvector (no '${marker% }' line)"
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

# IvfflatMetaPageData.lists (uint16 at page offset 24 + 10).
meta_lists() {
  od -An -tu2 -j34 -N2 "$1" | tr -d ' '
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
    # Column 4: the HNSW graph height, or an IVFFlat index's lists.
    case "$c" in ivf_*) col4="$(meta_lists "$f")" ;; *) col4="$(entry_level "$f")" ;; esac
    printf '%s\t%s\t%s\t%s\n' "$c" "${name#"$c".}" "$(($(wc -c <"$f") / 8192))" "$col4" >>"$out/cases.tsv"
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

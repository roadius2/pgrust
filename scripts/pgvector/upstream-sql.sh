#!/usr/bin/env bash
# Print named sections of the vendored upstream extension script
# (crates/pgvector-0.8.7-reference/sql/vector.sql) for the trimmed
# extension/vector--0.8.5.sql. Dropped: COMMENT ON statements (0.8.6+; they
# arrive with the verbatim script in M4, spec §4.6) and every block of an
# access method not yet ported (USING ivfflat/hnsw opclasses and the
# *_support(internal) functions). Blocks are blank-line separated; a section
# runs from its "-- <name>" header to the next header. Output follows the
# upstream file's order.
# With --only-am AM, print instead only that access method's blocks of the
# sections (its `USING AM` opclasses and `AM_*_support(internal)` functions),
# for sections whose other blocks the trimmed script already has.
# usage: upstream-sql.sh [--only-am AM] 'SECTION' ...
#   e.g. upstream-sql.sh 'bit functions' 'bit operators'
#        upstream-sql.sh --only-am hnsw 'access method private functions' 'bit opclasses'
set -euo pipefail
. "$(dirname "$0")/common.sh"
am=""
if [ "${1:-}" = --only-am ]; then
  [ "$#" -ge 2 ] || die "usage: upstream-sql.sh [--only-am AM] 'SECTION' ..."
  am="$2"
  shift 2
fi
[ "$#" -ge 1 ] || die "usage: upstream-sql.sh [--only-am AM] 'SECTION' ..."
want=""
for s in "$@"; do
  grep -qxF -- "-- $s" "$PGV_SRC/sql/vector.sql" || die "no section '-- $s' in $PGV_SRC/sql/vector.sql"
  want="$want|-- $s|"
done
awk -v want="$want" -v am="$am" '
  BEGIN { RS = ""; ORS = "\n\n" }
  /^-- / && !/\n/ { keep = index(want, "|" $0 "|") > 0; if (keep) print; next }
  !keep { next }
  am == "" && (/USING (ivfflat|hnsw)/ || /_support\(internal\)/) { next }
  am != "" && $0 !~ ("USING " am " AS") && $0 !~ ("^CREATE FUNCTION " am "_[a-z]+_support[(]internal[)]") { next }
  {
    n = split($0, line, "\n"); out = ""
    for (i = 1; i <= n; i++) if (line[i] !~ /^COMMENT ON /) out = out (out == "" ? "" : "\n") line[i]
    if (out != "") print out
  }
' "$PGV_SRC/sql/vector.sql"

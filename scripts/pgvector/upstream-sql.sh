#!/usr/bin/env bash
# Print named sections of the vendored upstream extension script
# (crates/pgvector-0.8.7-reference/sql/vector.sql) for the trimmed
# extension/vector--0.8.5.sql. Dropped: COMMENT ON statements (0.8.6+; they
# arrive with the verbatim script in M4, spec §4.6) and every block of an
# access method not yet ported (USING ivfflat/hnsw opclasses and the
# *_support(internal) functions). Blocks are blank-line separated; a section
# runs from its "-- <name>" header to the next header.
# usage: upstream-sql.sh 'SECTION' ...   e.g. upstream-sql.sh 'bit functions' 'bit operators'
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -ge 1 ] || die "usage: upstream-sql.sh 'SECTION' ..."
want=""
for s in "$@"; do
  grep -qx -- "-- $s" "$PGV_SRC/sql/vector.sql" || die "no section '-- $s' in $PGV_SRC/sql/vector.sql"
  want="$want|-- $s|"
done
awk -v want="$want" '
  BEGIN { RS = ""; ORS = "\n\n" }
  /^-- / && !/\n/ { keep = index(want, "|" $0 "|") > 0; if (keep) print; next }
  keep && !/USING (ivfflat|hnsw)/ && !/_support\(internal\)/ {
    n = split($0, line, "\n"); out = ""
    for (i = 1; i <= n; i++) if (line[i] !~ /^COMMENT ON /) out = out (out == "" ? "" : "\n") line[i]
    if (out != "") print out
  }
' "$PGV_SRC/sql/vector.sql"

#!/usr/bin/env bash
# Build the pgrust server used by the harness: target/$PGRUST_PROFILE/postgres.
set -euo pipefail
. "$(dirname "$0")/common.sh"

pkg-config --exists re2 || die "RE2 not found (release-family profiles require it): brew install re2 pkg-config"
(cd "$PGV_REPO" && cargo build --profile "$PGRUST_PROFILE" --locked --bin postgres --bin pagemask --bin diffrunner)
v="$("$PGRUST_BIN" -V)"
[ "$v" = "postgres (PostgreSQL) 18.6" ] || die "unexpected pgrust -V output: '$v'"
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN after the build"
[ -x "$DIFFRUNNER_BIN" ] || die "missing $DIFFRUNNER_BIN after the build"
echo "built $PGRUST_BIN ($v; staged vector extension $(pgrust_ext_version))"

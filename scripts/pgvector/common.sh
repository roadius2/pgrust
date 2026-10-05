# shellcheck shell=bash
# Shared environment for the pgvector conformance harness. Sourced by the
# scripts in this directory; not executable on its own. bash 3.2 compatible.
#
# Two C installs live under $PGREF, both built outside the repo:
#   $PG_TOOLS  PostgreSQL 18.6 only. pgrust reads its share/ (timezone data)
#              and uses its initdb, pg_ctl, psql and pg_regress.
#   $PG_VEC    Copy of $PG_TOOLS plus C pgvector 0.8.7: the reference server.
#              Kept separate so pgrust never sees C pgvector's control file
#              or upgrade scripts.

PGV_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PG_SRC="$PGV_REPO/crates/postgres-18.6-reference"
PGV_SRC="$PGV_REPO/crates/pgvector-0.8.7-reference"

PGREF="${PGREF:-$HOME/.cache/pgrust/pgref-18.6}"
PGV_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
PG_TOOLS="$PGREF/pg"
PG_VEC="$PGREF/pgvec"
PGV_PERL5LIB="$PGREF/perl5/lib/perl5"

PGV_PORT_PGRUST=55491
PGV_PORT_REF=55492
PGV_PORT_DOCKER=55493
PGV_DOCKER_IMAGE="pgvector/pgvector:0.8.7-pg18@sha256:2358fcba361ed2233a5ed81b5fe4ca779ccb304120ce531a3bf51c0ed7e2bc11"

die() {
  echo "error: $*" >&2
  exit 1
}

ncpu() {
  sysctl -n hw.ncpu 2>/dev/null || nproc
}

pg_regress_bin() {
  echo "$("$PG_TOOLS/bin/pg_config" --pkglibdir)/pgxs/src/test/regress/pg_regress"
}

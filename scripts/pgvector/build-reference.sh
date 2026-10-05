#!/usr/bin/env bash
# Build the C reference installs under $PGREF (see common.sh), then verify.
# usage: build-reference.sh [--verify]
set -euo pipefail
. "$(dirname "$0")/common.sh"

verify_reference() {
  local fails=0 tools_share vec_share
  expect() { # expect DESCRIPTION ACTUAL WANTED
    if [ "$2" = "$3" ]; then
      echo "ok   - $1"
    else
      echo "FAIL - $1: got '$2', want '$3'"
      fails=$((fails + 1))
    fi
  }
  tools_share="$("$PG_TOOLS/bin/pg_config" --sharedir 2>/dev/null || true)"
  vec_share="$("$PG_VEC/bin/pg_config" --sharedir 2>/dev/null || true)"
  # pg_ctl and initdb accept only a postgres whose -V output matches exactly.
  expect "tools postgres -V" "$("$PG_TOOLS/bin/postgres" -V 2>/dev/null)" "postgres (PostgreSQL) 18.6"
  expect "tools pg_ctl -V" "$("$PG_TOOLS/bin/pg_ctl" -V 2>/dev/null)" "pg_ctl (PostgreSQL) 18.6"
  expect "reference postgres -V" "$("$PG_VEC/bin/postgres" -V 2>/dev/null)" "postgres (PostgreSQL) 18.6"
  expect "pg_regress installed" "$([ -x "$(pg_regress_bin 2>/dev/null)" ] && echo yes)" "yes"
  expect "reference has pgvector" \
    "$(sed -n "s/^default_version = '\(.*\)'$/\1/p" "$vec_share/extension/vector.control" 2>/dev/null)" "0.8.7"
  expect "tools share has no pgvector" \
    "$([ -e "$tools_share/extension/vector.control" ] && echo present || echo absent)" "absent"
  expect "IPC::Run loadable" "$(PERL5LIB="$PGV_PERL5LIB" perl -MIPC::Run -e 'print "yes"' 2>/dev/null)" "yes"
  [ "$fails" -eq 0 ] || die "$fails reference check(s) failed"
}

if [ "${1:-}" = "--verify" ]; then
  verify_reference
  exit 0
fi

# 1. IPC::Run, needed by `configure --enable-tap-tests` and by the TAP suite.
if ! PERL5LIB="$PGV_PERL5LIB" perl -MIPC::Run -e1 2>/dev/null; then
  command -v cpanm >/dev/null || die "cpanm not found: brew install cpanminus"
  cpanm --notest -l "$PGREF/perl5" IPC::Run
fi
export PERL5LIB="$PGV_PERL5LIB${PERL5LIB:+:$PERL5LIB}"

# 2. PostgreSQL 18.6, VPATH build: nothing is written into the vendored tree.
if [ ! -x "$PG_TOOLS/bin/postgres" ]; then
  build="$PGREF/build/pg"
  rm -rf "$build"
  mkdir -p "$build"
  echo "building PostgreSQL 18.6 in $build (a few minutes)"
  # Explicit && chain: set -e is suspended inside a subshell that is the left
  # side of ||, so a failed configure would otherwise run on into make.
  (
    cd "$build" &&
      "$PG_SRC/configure" --prefix="$PG_TOOLS" --without-icu --without-readline \
        --enable-tap-tests --enable-debug >configure.log 2>&1 &&
      make -j"$(ncpu)" >make.log 2>&1 &&
      make install >install.log 2>&1
  ) || die "PostgreSQL build failed; logs in $build"
fi

# 3. Reference server install: a copy of the tools install (PostgreSQL installs
#    are relocatable) plus C pgvector, built from a copy of the vendored tree.
if [ ! -d "$PG_VEC" ]; then
  cp -R "$PG_TOOLS" "$PG_VEC"
fi
vec_share="$("$PG_VEC/bin/pg_config" --sharedir)"
if [ ! -f "$vec_share/extension/vector.control" ]; then
  build="$PGREF/build/pgvector"
  rm -rf "$build"
  mkdir -p "$build"
  cp -R "$PGV_SRC/." "$build/"
  echo "building pgvector 0.8.7 in $build"
  (
    cd "$build" &&
      make OPTFLAGS="" PG_CONFIG="$PG_VEC/bin/pg_config" >make.log 2>&1 &&
      make install PG_CONFIG="$PG_VEC/bin/pg_config" >install.log 2>&1
  ) || die "pgvector build failed; logs in $build"
fi

verify_reference

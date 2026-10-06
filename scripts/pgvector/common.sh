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
#   $PG_VEC_SEEDED  Same as $PG_VEC, but pgvector is built with -DHNSW_MEMORY,
#              the only configuration in which C seeds the HNSW level RNG
#              (SeedRandom(42), hnswbuild.c:1134-1136). Oracle of the
#              byte-identical and iterative-scan tiers; nothing else uses it.

PGV_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PG_SRC="$PGV_REPO/crates/postgres-18.6-reference"
PGV_SRC="$PGV_REPO/crates/pgvector-0.8.7-reference"

PGREF="${PGREF:-$HOME/.cache/pgrust/pgref-18.6}"
PGV_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
PG_TOOLS="$PGREF/pg"
PG_VEC="$PGREF/pgvec"
PG_VEC_SEEDED="${PG_VEC_SEEDED:-$PGREF/pgvec-seeded}"
PGV_PERL5LIB="$PGREF/perl5/lib/perl5"

PGV_PORT_PGRUST=55491
PGV_PORT_REF=55492
PGV_PORT_DOCKER=55493
PGV_PORT_SEEDED=55494
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

# --- pgrust binary and servers ---

PGRUST_PROFILE="${PGRUST_PROFILE:-fast-profile}"
PGRUST_BIN="${PGRUST_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/postgres}"
PAGEMASK_BIN="${PAGEMASK_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/pagemask}"
DIFFRUNNER_BIN="${DIFFRUNNER_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/diffrunner}"
# Extra listen address for harness servers (default: Unix socket only).
# run-diff.sh sets 127.0.0.1 because diffrunner speaks TCP only.
PGV_LISTEN="${PGV_LISTEN:-}"
PGRUST_SERVER_OPTS=(-c listen_addresses= -c io_method=sync -c max_stack_depth=60000)

port_for() {
  case "$1" in
    pgrust) echo "$PGV_PORT_PGRUST" ;;
    ref) echo "$PGV_PORT_REF" ;;
    seeded) echo "$PGV_PORT_SEEDED" ;;
    *) die "unknown mode '$1' (expected pgrust or ref)" ;;
  esac
}

# Runtime environment from the README quickstart. pgrust reads timezone data
# from a PostgreSQL share dir; extensions come from its own staged
# target/<profile>/share/extension first.
pgrust_env() {
  local share
  share="$("$PG_TOOLS/bin/pg_config" --sharedir)"
  export PGRUST_PGSHAREDIR="$share"
  export PGRUST_TZDIR="$share/timezone"
  export RUST_MIN_STACK=33554432
}

pgrust_ext_version() {
  sed -n "s/^default_version = '\(.*\)'$/\1/p" \
    "$(dirname "$PGRUST_BIN")/share/extension/vector.control" 2>/dev/null || true
}

pgv_psql() {
  local port="$1"
  shift
  "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 -h "$PGV_WORK/sock" -p "$port" -U postgres -d postgres "$@"
}

wait_ready() {
  local port="$1" i
  for i in $(seq 1 120); do
    if "$PG_TOOLS/bin/pg_isready" -q -h "$PGV_WORK/sock" -p "$port"; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

assert_identity() {
  local mode="$1" port="$2" v
  v="$(pgv_psql "$port" -Atc 'select version()')" || die "cannot query the server on port $port"
  case "$mode:$v" in
    "pgrust:"*"(pgrust "*) ;;
    "pgrust:"*) die "port $port is not a pgrust server: $v" ;;
    "ref:"*"(pgrust "*) die "port $port is a pgrust server, expected the C reference: $v" ;;
    "ref:PostgreSQL 18.6"*) ;;
    "seeded:"*"(pgrust "*) die "port $port is a pgrust server, expected the seeded C reference: $v" ;;
    "seeded:PostgreSQL 18.6"*) ;;
    *) die "port $port runs an unexpected server: $v" ;;
  esac
}

server_start() {
  local mode="$1" data="$2" port="$3" log
  mkdir -p "$PGV_WORK/sock" "$PGV_WORK/log"
  # Unix-domain socket paths are limited to 103 bytes on macOS.
  [ "${#PGV_WORK}" -le 80 ] || die "PGV_WORK is too long for a socket path (${#PGV_WORK} > 80 bytes): $PGV_WORK"
  if "$PG_TOOLS/bin/pg_isready" -q -h "$PGV_WORK/sock" -p "$port"; then
    die "a server is already running on port $port; stop it first (scripts/pgvector/server.sh stop $mode)"
  fi
  if [ ! -d "$data" ]; then
    "$PG_TOOLS/bin/initdb" -D "$data" --no-locale --encoding UTF8 -U postgres -A trust \
      >"$PGV_WORK/log/initdb-$mode.log" 2>&1 || die "initdb failed; see $PGV_WORK/log/initdb-$mode.log"
  fi
  log="$PGV_WORK/log/server-$mode.log"
  case "$mode" in
    pgrust)
      [ -x "$PGRUST_BIN" ] || die "missing $PGRUST_BIN; run scripts/pgvector/build-pgrust.sh"
      (
        pgrust_env
        ulimit -s 65520
        exec "$PGRUST_BIN" -D "$data" -k "$PGV_WORK/sock" -p "$port" "${PGRUST_SERVER_OPTS[@]}" -c "listen_addresses=$PGV_LISTEN"
      ) >"$log" 2>&1 &
      ;;
    ref)
      (exec "$PG_VEC/bin/postgres" -D "$data" -k "$PGV_WORK/sock" -p "$port" -c "listen_addresses=$PGV_LISTEN") >"$log" 2>&1 &
      ;;
    seeded)
      [ -x "$PG_VEC_SEEDED/bin/postgres" ] || die "missing $PG_VEC_SEEDED; run scripts/pgvector/build-reference.sh"
      (exec "$PG_VEC_SEEDED/bin/postgres" -D "$data" -k "$PGV_WORK/sock" -p "$port" -c "listen_addresses=$PGV_LISTEN") >"$log" 2>&1 &
      ;;
    *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
  esac
  echo $! >"$PGV_WORK/$mode.pid"
  wait_ready "$port" || die "server ($mode) did not become ready; see $log"
  assert_identity "$mode" "$port"
}

server_stop() {
  local mode="$1" pidfile="$PGV_WORK/$1.pid" pid i
  [ -f "$pidfile" ] || return 0
  pid="$(cat "$pidfile")"
  # Signal only a postgres binary: after a crash or reboot the pid may be reused.
  case "$(ps -p "$pid" -o command= 2>/dev/null || true)" in
    */postgres | */postgres\ *) ;;
    *)
      rm -f "$pidfile"
      return 0
      ;;
  esac
  if kill -0 "$pid" 2>/dev/null; then
    kill -INT "$pid"
    for i in $(seq 1 60); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
    if kill -0 "$pid" 2>/dev/null; then
      die "server ($mode, pid $pid) did not stop"
    fi
  fi
  rm -f "$pidfile"
}

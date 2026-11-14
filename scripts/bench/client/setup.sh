#!/usr/bin/env bash
# Prepares pgrust-client, the benchmark's load generator: PGDG psql and
# pgbench 18.6, a Python venv at /opt/bench/venv with h5py, numpy and psycopg,
# and this directory's scripts at /opt/bench/. Runs on the host, shipped there
# by sync.sh: ssh -n borg2 bash ~/bench/scripts/client/setup.sh
# It copies client/ into the container (borg.sh push-dir, to /root/client) and
# runs the "inside" part of this same file there. Idempotent: a second run
# installs nothing new, replaces the scripts in /opt/bench/ with the shipped
# ones (the venv stays) and prints the same report. Run it after every edit
# of client/ (and sync.sh before it).
#
#   setup.sh           set up the client and report
#   setup.sh inside    (in the container) the setup itself
#
# Debian packages pgbench with the server, so pgbench comes from postgresql-18,
# not postgresql-client-18. Both are pinned to 18.6-1.pgdg24.04+2 (the pg-c
# server's build) and held; as in install-pg-c.sh, postgresql-common is
# installed first with create_main_cluster = false, so the server package
# creates and starts no cluster here: the client runs no PostgreSQL server.
#
# Result: /opt/bench/{load.py,recall.py,soak_stats.py} (run them directly; their
# shebang is the venv's python), search.pgbench and insert.pgbench (the soak runs
# -f search.pgbench@9 -f insert.pgbench@1), and pgbench 18.6.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

CONTAINER="pgrust-client"
PG_VERSION="18.6-1.pgdg24.04+2"
KEY="/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc"
LIST="/etc/apt/sources.list.d/pgdg.list"
OPT="/opt/bench"
VENV="$OPT/venv"
STAGE="/root/client"

install_repo() {
  [ -f "$KEY" ] || {
    apt-get update -qq
    apt-get install -y -qq curl ca-certificates
    install -d "$(dirname "$KEY")"
    curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc -o "$KEY"
  }
  echo "deb [signed-by=$KEY] https://apt.postgresql.org/pub/repos/apt noble-pgdg main" >"$LIST"
  apt-get update -qq
}

install_packages() {
  dpkg -s postgresql-common >/dev/null 2>&1 ||
    apt-get install -y -qq -o Dpkg::Options::=--force-confold postgresql-common
  sed -i -E 's/^#?[[:space:]]*create_main_cluster[[:space:]]*=.*/create_main_cluster = false/' \
    /etc/postgresql-common/createcluster.conf
  grep -q '^create_main_cluster = false$' /etc/postgresql-common/createcluster.conf || {
    echo "error: createcluster.conf has no create_main_cluster line" >&2
    exit 1
  }
  apt-get install -y -qq --no-install-recommends -o Dpkg::Options::=--force-confold \
    "postgresql-18=$PG_VERSION" "postgresql-client-18=$PG_VERSION" python3-venv
  apt-mark hold postgresql-18 postgresql-client-18 >/dev/null
  if [ -n "$(pg_lsclusters -h 2>/dev/null)" ]; then
    echo "error: a PostgreSQL cluster exists in the client: $(pg_lsclusters -h)" >&2
    exit 1
  fi
}

install_scripts() {
  install -d "$OPT"
  find "$OPT" -mindepth 1 -maxdepth 1 -type f -delete
  cp "$STAGE"/* "$OPT"/
  find "$OPT" -maxdepth 1 -type f \( -name '*.py' -o -name '*.sh' \) -exec chmod 0755 {} +
  rm -rf "$STAGE"
}

install_venv() {
  [ -x "$VENV/bin/python" ] || python3 -m venv "$VENV"
  "$VENV/bin/pip" install -q --disable-pip-version-check h5py numpy 'psycopg[binary]'
}

report() {
  dpkg-query -W -f='${Package} ${Version}\n' postgresql-18 postgresql-client-18 libpq5 python3-venv
  psql --version
  pgbench --version
  "$VENV/bin/python" -c 'import h5py, numpy, psycopg' || exit 1
  "$VENV/bin/python" -c 'import sys, h5py, numpy, psycopg
print("python", sys.version.split()[0], "h5py", h5py.__version__, "numpy", numpy.__version__,
      "psycopg", psycopg.__version__, "libpq", psycopg.pq.version())'
  ls "$OPT"
}

inside() {
  export DEBIAN_FRONTEND=noninteractive
  install_repo
  install_packages
  install_scripts
  install_venv
  report
}

if [ "${1:-}" = inside ]; then
  inside
else
  # shellcheck source=../lib.sh
  . "$here/../lib.sh"
  X "$CONTAINER" rm -rf "$STAGE"
  bash "$here/../borg.sh" push-dir "$CONTAINER" "$here" /root
  XB "$CONTAINER" "bash $STAGE/setup.sh inside"
fi

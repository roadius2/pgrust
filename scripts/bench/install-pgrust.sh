#!/usr/bin/env bash
# Installs the pgrust contestant in pgrust-db-rust: pgrust built inside the
# container from the public fork's main (or $PGRUST_REF: a branch, a tag or a
# full 40-character sha), served by pgrust.service and configured with
# common-settings.conf, like the C side.
# Runs on the host, shipped there by sync.sh:
#   ssh -n borg2 bash ~/bench/scripts/install-pgrust.sh
# It copies scripts/ into the container (borg.sh push-dir) and runs the
# "inside" parts of this same file there. Idempotent: when the fetched sha is
# the one in /opt/pgrust/BUILD.txt nothing is rebuilt, the server restarts
# only when its configuration or unit changed, and the same report prints.
#
#   install-pgrust.sh                  install and report
#   install-pgrust.sh prepare          (in the container) packages, rustup, source
#   install-pgrust.sh needs-build      (in the container) exit 0 if a build is
#                                      due, 10 if /opt/pgrust is the fetched sha
#   install-pgrust.sh build            (in the container) cargo build, /opt/pgrust
#   install-pgrust.sh serve            (in the container) user, initdb, settings,
#                                      unit, role, database, extension, report
#
# The build gets all 16 host CPUs (limits.cpu 0-15) and the server is stopped
# while it runs; limits.cpu returns to 0-5, checked from the host and from
# inside, before the server starts again. The build runs as the transient unit
# pgrust-build (journalctl -u pgrust-build) so it outlives no shell's I/O.
# /opt/pgrust/BUILD.txt records the sha, rustc, the cargo command and the wall
# time. PostgreSQL refuses to run as root, so the user pgrust owns /srv/pgrust.
# psql and pg_isready come from PGDG's postgresql-client-18, pinned and held.
#
# Result: pgrust on *:5432 (pgrust.service, data /srv/pgrust/data), database
# bench, role bench (superuser, password bench, scram), extension vector.
set -euo pipefail

# shellcheck source=lib.sh
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
. "$here/lib.sh"

CONTAINER="pgrust-db-rust"
BUILD_CPUS="0-15"
SERVE_CPUS="0-5"
REF="${PGRUST_REF:-main}"
REPO="https://github.com/roadius2/pgrust"
SRC="/root/pgrust"
OPT="/opt/pgrust"
CARGO="/root/.cargo/bin/cargo"
CARGO_CMD="cargo build --release --locked -p main_main -p initdb"
RUST_VERSION="1.96.0"
PG_VERSION="18.6-1.pgdg24.04+2"
KEY="/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc"
LIST="/etc/apt/sources.list.d/pgdg.list"
DATA="/srv/pgrust/data"
CONF="$DATA/postgresql.conf"
HBA="$DATA/pg_hba.conf"
HBA_LINE="host all bench 192.168.0.0/24 scram-sha-256"
SETTINGS="/root/scripts/common-settings.conf"
UNIT_SRC="/root/scripts/pgrust.service"
UNIT="/etc/systemd/system/pgrust.service"

psql_postgres() {
  psql -X -v ON_ERROR_STOP=1 -tA -h /tmp -p 5432 -U postgres "$@"
}

# --- inside: prepare --------------------------------------------------------

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
  apt-get install -y -o Dpkg::Options::=--force-confold \
    build-essential pkg-config libre2-dev git curl ca-certificates \
    "postgresql-client-18=$PG_VERSION"
  apt-mark hold postgresql-client-18
}

install_rust() {
  [ -x /root/.cargo/bin/rustup ] ||
    curl -sSf https://sh.rustup.rs |
    sh -s -- -y -q --default-toolchain "$RUST_VERSION" --profile minimal
}

# One path for a branch, a tag or a full 40-character sha (GitHub serves a
# full sha to a shallow fetch; an abbreviated one cannot be fetched).
fetch_source() {
  if [ ! -d "$SRC/.git" ]; then
    git init -q "$SRC"
    git -C "$SRC" remote add origin "$REPO"
  fi
  git -C "$SRC" fetch --depth 1 origin "$REF"
  git -C "$SRC" checkout -q --detach --force FETCH_HEAD
  echo "source: $REF at $(git -C "$SRC" rev-parse HEAD)"
}

prepare() {
  export DEBIAN_FRONTEND=noninteractive
  install_repo
  install_packages
  install_rust
  fetch_source
}

# --- inside: needs-build, build ---------------------------------------------

needs_build() {
  local head built=""
  head="$(git -C "$SRC" rev-parse HEAD)"
  [ ! -f "$OPT/BUILD.txt" ] || built="$(sed -n 's/^sha //p' "$OPT/BUILD.txt")"
  if [ "$head" = "$built" ] && [ -x "$OPT/postgres" ] && [ -x "$OPT/initdb" ]; then
    echo "build: $OPT is already $head"
    exit 10
  fi
  echo "build: due ($REF is $head, $OPT has ${built:-nothing})"
}

# Builds under systemd-run, then swaps a complete /opt/pgrust.new into place,
# so a failed build or copy leaves the previous installation intact.
build() {
  local start end wall t="$SRC/target/release" new="$OPT.new"
  start="$(date +%s)"
  systemd-run --unit=pgrust-build --wait --collect \
    -p WorkingDirectory="$SRC" --setenv=HOME=/root \
    --setenv=PATH=/root/.cargo/bin:/usr/local/bin:/usr/bin:/bin \
    "$CARGO" build --release --locked -p main_main -p initdb ||
    {
      journalctl -u pgrust-build -n 40 --no-pager -o cat >&2
      die "cargo build failed (journalctl -u pgrust-build)"
    }
  end="$(date +%s)"
  wall="$((end - start))"
  echo "build: wall time $((wall / 60))m$((wall % 60))s ($wall s)"
  rm -rf "$new"
  install -d "$new"
  cp "$t/postgres" "$t/initdb" "$new/"
  cp -a "$t/share" "$new/share"
  {
    echo "sha $(git -C "$SRC" rev-parse HEAD)"
    echo "ref $REF ($REPO)"
    echo "rustc $(cd "$SRC" && /root/.cargo/bin/rustc --version)"
    echo "command $CARGO_CMD"
    echo "wall $((wall / 60))m$((wall % 60))s ($wall s, $(nproc) cpus)"
    echo "built $(now)"
  } >"$new/BUILD.txt"
  chmod -R a+rX "$new"
  rm -rf "$OPT.old"
  [ ! -e "$OPT" ] || mv "$OPT" "$OPT.old"
  mv "$new" "$OPT"
  rm -rf "$OPT.old"
}

# --- inside: serve ----------------------------------------------------------

create_cluster() {
  id pgrust >/dev/null 2>&1 ||
    useradd --system --user-group --home-dir /srv/pgrust --shell /usr/sbin/nologin pgrust
  install -d -o pgrust -g pgrust -m 0700 /srv/pgrust
  [ -f "$DATA/PG_VERSION" ] ||
    (cd /srv/pgrust && runuser -u pgrust -- "$OPT/initdb" -D "$DATA" --no-locale --encoding UTF8 -U postgres)
}

# The settings block between the "# bench begin/end" markers is replaced on
# every run, so a changed common-settings.conf reaches the server. Sets
# CHANGED=1 when the configuration was modified.
configure() {
  local new="$CONF.new"
  {
    sed -e '/^# bench begin$/,/^# bench end$/d' "$CONF"
    echo '# bench begin'
    cat "$SETTINGS"
    echo '# bench end'
  } >"$new"
  if ! cmp -s "$new" "$CONF"; then
    cat "$new" >"$CONF"
    CHANGED=1
  fi
  rm "$new"
  if ! grep -qxF "$HBA_LINE" "$HBA"; then
    { echo "$HBA_LINE"; cat "$HBA"; } >"$HBA.new"
    cat "$HBA.new" >"$HBA"
    rm "$HBA.new"
    CHANGED=1
  fi
}

install_unit() {
  if ! cmp -s "$UNIT_SRC" "$UNIT"; then
    install -m 0644 "$UNIT_SRC" "$UNIT"
    systemctl daemon-reload
    CHANGED=1
  fi
  systemctl enable --quiet pgrust.service
}

# Waits at most 120 s; a server that exits or never answers is a failure, and
# its log is the evidence.
wait_ready() {
  local _
  for _ in $(seq 1 120); do
    pg_isready -q -h /tmp -p 5432 -U postgres && return 0
    systemctl is-active --quiet pgrust.service || break
    sleep 1
  done
  journalctl -u pgrust.service -n 50 --no-pager -o cat >&2
  die "pgrust.service is not accepting connections"
}

create_objects() {
  [ -n "$(psql_postgres -c "select 1 from pg_roles where rolname = 'bench'")" ] ||
    psql_postgres -c "create role bench superuser login password 'bench'"
  [ -n "$(psql_postgres -c "select 1 from pg_database where datname = 'bench'")" ] ||
    psql_postgres -c "create database bench owner bench"
  psql_postgres -d bench -c "create extension if not exists vector"
}

report() {
  cat "$OPT/BUILD.txt"
  dpkg-query -W -f='${Package} ${Version}\n' postgresql-client-18
  psql_postgres -d bench -c "select version()"
  psql_postgres -d bench -c "show io_method" -c "show ssl"
  psql_postgres -d bench -c "select 'vector ' || extversion from pg_extension where extname = 'vector'"
  psql_postgres -d bench -F ' | ' \
    -c "select name, setting from pg_settings where source <> 'default' order by 1"
}

serve() {
  local CHANGED=0
  create_cluster
  configure
  install_unit
  if [ "$CHANGED" = 1 ] || ! systemctl is-active --quiet pgrust.service; then
    systemctl restart pgrust.service
  fi
  wait_ready
  create_objects
  report
}

# --- host -------------------------------------------------------------------

inside() {
  XB "$CONTAINER" "PGRUST_REF=$(printf %q "$REF") bash /root/scripts/install-pgrust.sh $1"
}

set_cpus() {
  incus config set "$CONTAINER" limits.cpu "$1" </dev/null
}

# The server may start only on its own CPUs: the setting and the CPUs a fresh
# process in the container may use must both read $SERVE_CPUS.
check_cpus() {
  local set allowed
  set="$(incus config get "$CONTAINER" limits.cpu </dev/null)"
  allowed="$(X "$CONTAINER" grep Cpus_allowed_list /proc/self/status | awk '{ print $2 }')"
  echo "limits.cpu: $set; Cpus_allowed_list inside: $allowed"
  if [ "$set" != "$SERVE_CPUS" ] || [ "$allowed" != "$SERVE_CPUS" ]; then
    die "$CONTAINER is not on cpus $SERVE_CPUS"
  fi
}

host() {
  local due=0
  bash "$here/borg.sh" push-dir "$CONTAINER" "$here" /root
  inside prepare
  inside needs-build || due=$?
  case "$due" in
    0 | 10) ;;
    *) die "needs-build failed (exit $due)" ;;
  esac
  if [ "$due" = 0 ]; then
    XB "$CONTAINER" "! systemctl is-active --quiet pgrust.service || systemctl stop pgrust.service"
    trap 'set_cpus "$SERVE_CPUS"' EXIT
    set_cpus "$BUILD_CPUS"
    inside build
    set_cpus "$SERVE_CPUS"
    trap - EXIT
  fi
  check_cpus
  inside serve
}

case "${1:-}" in
  "") host ;;
  prepare) prepare ;;
  needs-build) needs_build ;;
  build) build ;;
  serve) serve ;;
  *) die "usage: install-pgrust.sh [prepare|needs-build|build|serve]" ;;
esac

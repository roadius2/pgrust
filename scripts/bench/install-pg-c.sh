#!/usr/bin/env bash
# Installs the C contestant in pgrust-db-c: PostgreSQL 18.6 from PGDG plus
# pgvector 0.8.7, configured with common-settings.conf. Runs on the host,
# shipped there by sync.sh: ssh -n borg2 bash ~/bench/scripts/install-pg-c.sh
# It copies scripts/ into the container (borg.sh push-dir) and runs the
# "inside" part of this same file there. Idempotent: a second run changes
# nothing (the server restarts only when the configuration changed) and prints
# the same report.
#
#   install-pg-c.sh           install and report
#   install-pg-c.sh inside    (in the container) the installation itself
#
# Pinned packages: postgresql-18 and postgresql-client-18 at 18.6-1.pgdg24.04+2,
# postgresql-18-pgvector at 0.8.7-1.pgdg24.04+1 (the +2 build depends on
# postgresql-18 (>> 18.7), so it cannot be installed with 18.6).
#
# The postgresql-18 package must not create and start its default "main"
# cluster in its postinst, so create_main_cluster is turned off first and the
# cluster is created, unstarted, with pg_createcluster. The containers need
# security.nesting=true (borg.sh sets it), else AppArmor denies the signals
# PostgreSQL relies on.
#
# Result: PostgreSQL on *:5432 (service postgresql@18-main), database bench,
# role bench (superuser, password bench, scram), extension vector.
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

CONTAINER="pgrust-db-c"
PG_VERSION="18.6-1.pgdg24.04+2"
VECTOR_VERSION="0.8.7-1.pgdg24.04+1"
KEY="/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc"
LIST="/etc/apt/sources.list.d/pgdg.list"
CONF="/etc/postgresql/18/main/postgresql.conf"
HBA="/etc/postgresql/18/main/pg_hba.conf"
HBA_LINE="host all bench 192.168.0.0/24 scram-sha-256"
SETTINGS="/root/scripts/common-settings.conf"

psql_postgres() {
  sudo -u postgres psql -X -v ON_ERROR_STOP=1 -tA "$@"
}

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
  apt-get install -y postgresql-common
  sed -i -E 's/^#?[[:space:]]*create_main_cluster[[:space:]]*=.*/create_main_cluster = false/' \
    /etc/postgresql-common/createcluster.conf
  grep -q '^create_main_cluster = false$' /etc/postgresql-common/createcluster.conf ||
    die "createcluster.conf has no create_main_cluster line"
  apt-get install -y \
    "postgresql-18=$PG_VERSION" "postgresql-client-18=$PG_VERSION" "postgresql-18-pgvector=$VECTOR_VERSION"
  apt-mark hold postgresql-18 postgresql-client-18 postgresql-18-pgvector
}

create_cluster() {
  [ -d /etc/postgresql/18/main ] || pg_createcluster 18 main
}

# The settings block between the "# bench begin/end" markers is replaced on
# every run, so a changed common-settings.conf reaches the server. (An earlier
# version appended a bare "# bench" block at the end of the file; that is
# removed too.) Sets CHANGED=1 when the configuration was modified.
configure() {
  local new="$CONF.new"
  {
    sed -e '/^# bench begin$/,/^# bench end$/d' -e '/^# bench$/,$d' "$CONF"
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

create_objects() {
  [ -n "$(psql_postgres -c "select 1 from pg_roles where rolname = 'bench'")" ] ||
    psql_postgres -c "create role bench superuser login password 'bench'"
  [ -n "$(psql_postgres -c "select 1 from pg_database where datname = 'bench'")" ] ||
    psql_postgres -c "create database bench owner bench"
  psql_postgres -d bench -c "create extension if not exists vector"
}

report() {
  dpkg-query -W -f='${Package} ${Version}\n' postgresql-18 postgresql-client-18 postgresql-18-pgvector
  psql_postgres -d bench -c "select version()"
  psql_postgres -d bench -c "select 'vector ' || extversion from pg_extension where extname = 'vector'"
  psql_postgres -d bench -F ' | ' \
    -c "select name, setting from pg_settings where source <> 'default' order by 1"
}

inside() {
  local CHANGED=0
  export DEBIAN_FRONTEND=noninteractive
  install_repo
  install_packages
  create_cluster
  configure
  if [ "$CHANGED" = 1 ] || ! systemctl is-active --quiet postgresql@18-main; then
    systemctl restart postgresql@18-main
  fi
  until pg_isready -q; do sleep 1; done
  create_objects
  report
}

if [ "${1:-}" = inside ]; then
  inside
else
  bash "$(dirname "${BASH_SOURCE[0]}")/borg.sh" push-dir "$CONTAINER" "$(dirname "${BASH_SOURCE[0]}")" /root
  XB "$CONTAINER" "bash /root/scripts/install-pg-c.sh inside"
fi

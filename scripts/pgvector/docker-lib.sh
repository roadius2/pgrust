# shellcheck shell=bash
# Docker helpers for the on-disk tier (run-ondisk.sh). Sourced after
# common.sh; bash 3.2 compatible. Every dk_* function except dk_require
# returns non-zero on failure (with a message on stderr) instead of exiting,
# so callers choose: the capture dies, the reverse report records it.
#
# One container at a time, named $DK_NAME and published on
# 127.0.0.1:$PGV_PORT_DOCKER. docker-ref.sh uses the same port, so never run
# the two at once. The pgvector/pgvector images (postgres:18 base) keep
# PGDATA at /var/lib/postgresql/18/docker.
#   dk_start_new  runs the image's own entrypoint: initdb (C collation, UTF8,
#                 trust), then postgres.
#   dk_boot_dir   boots a host data directory instead. The directory is
#                 mounted read-only at /snap and copied to /pgdata on the
#                 container's first start only, so the host copy is never
#                 written and a restarted container keeps its own copy. The
#                 copy's pg_hba.conf gains "host all all all trust": the
#                 host's connections arrive from Docker's gateway, which a
#                 host initdb's 127.0.0.1-only rules reject.

DK_NAME=pgrust-pgvector-ondisk
# The container's data directory (set by dk_start_new and dk_boot_dir).
DK_DATA=

dk_require() {
  docker info >/dev/null 2>&1 || die "Docker daemon not reachable; start Docker Desktop"
}

dk_rm() {
  docker rm -f -v "$DK_NAME" >/dev/null 2>&1 || true
}

dk_psql() {
  "$PG_TOOLS/bin/psql" -X -h 127.0.0.1 -p "$PGV_PORT_DOCKER" -U postgres -d postgres "$@"
}

dk_wait_ready() {
  for _ in $(seq 1 240); do
    if "$PG_TOOLS/bin/pg_isready" -q -h 127.0.0.1 -p "$PGV_PORT_DOCKER" &&
      dk_psql -Atc 'SELECT 1' >/dev/null 2>&1; then
      return 0
    fi
    [ "$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)" = true ] || break
    sleep 0.5
  done
  echo "container $DK_NAME did not become ready; its last log lines:" >&2
  docker logs "$DK_NAME" 2>&1 | tail -20 >&2
  return 1
}

# $1 image; the rest are postgres options (-c name=value ...).
dk_start_new() {
  local image="$1"
  shift
  dk_rm
  if ! docker run -d --name "$DK_NAME" -p "127.0.0.1:$PGV_PORT_DOCKER:5432" \
    -e POSTGRES_HOST_AUTH_METHOD=trust -e POSTGRES_INITDB_ARGS="--no-locale --encoding=UTF8" \
    "$image" postgres "$@" >/dev/null; then
    echo "docker run $image failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    dk_rm
    return 1
  fi
  DK_DATA=/var/lib/postgresql/18/docker
  dk_wait_ready
}

# $1 image, $2 host data directory; the rest are postgres options.
dk_boot_dir() {
  local image="$1" dir="$2"
  shift 2
  if [ ! -f "$dir/PG_VERSION" ]; then
    echo "not a data directory: $dir" >&2
    return 1
  fi
  dk_rm
  # shellcheck disable=SC2016 # expanded by the container's bash
  if ! docker run -d --name "$DK_NAME" -p "127.0.0.1:$PGV_PORT_DOCKER:5432" -v "$dir:/snap:ro" \
    --entrypoint bash "$image" -c '
      if [ ! -d /pgdata ]; then
        cp -a /snap /pgdata && echo "host all all all trust" >>/pgdata/pg_hba.conf &&
          chown -R postgres:postgres /pgdata && chmod 700 /pgdata || exit 1
      fi
      exec gosu postgres postgres -D /pgdata -c listen_addresses="*" "$@"' bash "$@" >/dev/null; then
    echo "docker run $image on $dir failed (is port $PGV_PORT_DOCKER free? scripts/pgvector/docker-ref.sh down)" >&2
    dk_rm
    return 1
  fi
  DK_DATA=/pgdata
  dk_wait_ready
}

# Start the stopped container again; it keeps its data directory.
dk_start() {
  docker start "$DK_NAME" >/dev/null && dk_wait_ready
}

# Fast shutdown (the images' STOPSIGNAL is SIGINT).
dk_stop_fast() {
  docker stop -t 120 "$DK_NAME" >/dev/null
}

# pg_ctl stop -m immediate: no shutdown checkpoint, so the next start
# replays WAL from the last checkpoint. The postmaster is the container's
# PID 1, so the container ends the moment it exits and takes this exec with
# it: pg_ctl's own exit status means nothing, and the caller checks the
# copied directory's state instead. Fails, rather than waits forever, when
# no container was started, there is none, or it is still running 60 s on.
dk_stop_immediate() {
  local running
  if [ -z "$DK_DATA" ]; then
    echo "dk_stop_immediate: no container was started (DK_DATA is empty)" >&2
    return 1
  fi
  docker exec -u postgres "$DK_NAME" pg_ctl stop -m immediate -D "$DK_DATA" >/dev/null 2>&1 || true
  for _ in $(seq 1 120); do
    if ! running="$(docker inspect -f '{{.State.Running}}' "$DK_NAME" 2>/dev/null)"; then
      echo "dk_stop_immediate: no container $DK_NAME" >&2
      return 1
    fi
    [ "$running" = true ] || return 0
    sleep 0.5
  done
  echo "dk_stop_immediate: $DK_NAME still running 60 s after pg_ctl stop -m immediate" >&2
  return 1
}

# Copy the stopped container's data directory to $1 (replacing it).
dk_copy_out() {
  rm -rf "$1"
  docker cp "$DK_NAME:$DK_DATA" "$1" >/dev/null || return 1
  if [ -e "$1/postmaster.pid" ]; then
    echo "$1 holds postmaster.pid: the server did not stop" >&2
    return 1
  fi
}

dk_exec() {
  docker exec -u postgres "$DK_NAME" "$@"
}

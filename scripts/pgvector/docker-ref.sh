#!/usr/bin/env bash
# Docker pgvector reference: the real-world artifact (Debian PGDG build) used
# by the on-disk tier. Image pinned by digest in common.sh.
# usage: docker-ref.sh {up|down|psql} [psql args...]
set -euo pipefail
. "$(dirname "$0")/common.sh"

name=pgrust-pgvector-ref
cmd="${1:-}"
[ -n "$cmd" ] || die "usage: docker-ref.sh {up|down|psql} [psql args...]"
shift

case "$cmd" in
  up)
    docker info >/dev/null 2>&1 || die "Docker daemon not reachable; start Docker Desktop"
    docker rm -f -v "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" -p "127.0.0.1:$PGV_PORT_DOCKER:5432" \
      -e POSTGRES_HOST_AUTH_METHOD=trust \
      -e POSTGRES_INITDB_ARGS="--no-locale --encoding=UTF8" \
      "$PGV_DOCKER_IMAGE" >/dev/null
    for i in $(seq 1 120); do
      if "$PG_TOOLS/bin/pg_isready" -q -h 127.0.0.1 -p "$PGV_PORT_DOCKER"; then
        echo "docker reference up on 127.0.0.1:$PGV_PORT_DOCKER"
        exit 0
      fi
      sleep 1
    done
    die "docker reference did not become ready; see: docker logs $name"
    ;;
  down) docker rm -f -v "$name" >/dev/null ;;
  psql) "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$PGV_PORT_DOCKER" -U postgres -d postgres "$@" ;;
  *) die "unknown command '$cmd'" ;;
esac

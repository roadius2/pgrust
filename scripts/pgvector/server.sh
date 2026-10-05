#!/usr/bin/env bash
# Manage the harness servers.
# usage: server.sh {start|fresh|stop|psql} {pgrust|ref} [psql args...]
#   fresh = stop, delete the data directory, initdb, start
set -euo pipefail
. "$(dirname "$0")/common.sh"

[ "$#" -ge 2 ] || die "usage: server.sh {start|fresh|stop|psql} {pgrust|ref} [psql args...]"
cmd="$1"
mode="$2"
shift 2
port="$(port_for "$mode")"
data="$PGV_WORK/data/$mode"
case "$cmd" in
  start) server_start "$mode" "$data" "$port" ;;
  fresh)
    server_stop "$mode"
    rm -rf "$data"
    server_start "$mode" "$data" "$port"
    ;;
  stop) server_stop "$mode" ;;
  psql) pgv_psql "$port" "$@" ;;
  *) die "unknown command '$cmd'" ;;
esac

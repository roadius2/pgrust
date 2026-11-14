#!/usr/bin/env bash
# Manages the borg benchmark's Incus containers on borg2. Runs on the host,
# shipped there by sync.sh: ssh -n borg2 bash ~/bench/scripts/borg.sh CMD ...
# It only ever touches instances named pgrust-*.
#
#   create-db NAME          a database container, pinned to cpus 0-5, 24 GiB
#   create-client           pgrust-client, cpus 6,7,14,15, 4 GiB, with the
#                           host's /home/jody/bench/data read-only at /data
#   start NAME | stop NAME  start (and wait for its address) or stop NAME
#   stop-others NAME        stop every running pgrust-db-* except NAME
#   disable-upgrades NAME   disable unattended-upgrades and the apt timers
#   wait-net NAME           wait for NAME's LAN IPv4 address and print it
#   push-dir NAME SRC DST   copy host directory SRC into NAME as DST/<basename>
#   facts OUTDIR            write host.txt, containers.txt and zfs.txt
#   delete NAME             delete NAME, stopping it first
set -euo pipefail

# shellcheck source=lib.sh
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib.sh"

IMAGE="images:ubuntu/noble/cloud"
CLIENT="pgrust-client"
DATA_DIR="/home/jody/bench/data"

usage() {
  sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2
  exit 2
}

need_ours() {
  case "$1" in
    pgrust-*) ;;
    *) die "refusing to touch '$1': not a pgrust-* instance" ;;
  esac
}

# Status (RUNNING, STOPPED, ...) of instance $1, or nothing if it does not exist.
status_of() {
  incus list pgrust- -f csv -c ns </dev/null | awk -F, -v n="$1" '$1 == n { print $2 }'
}

launch() {
  local name="$1" cpu="$2" mem="$3"
  need_ours "$name"
  [ -z "$(status_of "$name")" ] || die "$name already exists"
  # security.nesting: without it AppArmor denies signals between systemd's and
  # incus exec's label sets, so PostgreSQL cannot be stopped or woken.
  incus launch "$IMAGE" "$name" -p trusted-lan --target borg2 \
    -c limits.cpu="$cpu" -c limits.memory="$mem" -c boot.autostart=false \
    -c security.nesting=true </dev/null
  cmd_wait_net "$name"
  X "$name" cloud-init status --wait
  cmd_disable_upgrades "$name"
}

cmd_create_db() {
  launch "$1" 0-5 24GiB
}

cmd_create_client() {
  mkdir -p "$DATA_DIR"
  launch "$CLIENT" 6,7,14,15 4GiB
  incus config device add "$CLIENT" data disk source="$DATA_DIR" path=/data \
    readonly=true shift=true </dev/null
}

cmd_start() {
  need_ours "$1"
  [ "$(status_of "$1")" = RUNNING ] || incus start "$1" </dev/null
  cmd_wait_net "$1"
}

# The database server is stopped first, from inside, and checked gone: the
# containers' rsyslogd AppArmor profile refuses systemd's TERM and KILL, so a
# plain incus stop can hang in the shutdown and time out. With no postgres
# process left, a stop that does not finish in 120 s is forced.
cmd_stop() {
  need_ours "$1"
  [ "$(status_of "$1")" != STOPPED ] || return 0
  # shellcheck disable=SC2016 # expanded inside the container
  XB "$1" 'for u in pgrust.service postgresql@18-main.service; do
      if systemctl is-active --quiet "$u"; then systemctl stop "$u"; fi
    done
    ! pgrep -x postgres >/dev/null' || die "$1: a postgres process survived the service stop"
  if ! incus stop --timeout 120 "$1" </dev/null; then
    echo "$1: no clean shutdown in 120 s (database already stopped); forcing"
    incus stop --force "$1" </dev/null
  fi
}

cmd_stop_others() {
  local keep="$1" name status
  need_ours "$keep"
  incus list pgrust-db- -f csv -c ns </dev/null | while IFS=, read -r name status; do
    case "$name" in pgrust-db-*) ;; *) continue ;; esac
    if [ "$name" != "$keep" ] && [ "$status" = RUNNING ]; then
      echo "stopping $name"
      cmd_stop "$name"
    fi
  done
}

cmd_disable_upgrades() {
  local units="unattended-upgrades.service apt-daily.timer apt-daily-upgrade.timer"
  need_ours "$1"
  XB "$1" "set -e; systemctl disable --now $units; systemctl is-enabled $units || true"
}

cmd_wait_net() {
  local name="$1" ip="" _
  need_ours "$name"
  for _ in $(seq 1 60); do
    ip="$(X "$name" ip -4 -o addr show scope global 2>/dev/null |
      awk '{ sub("/.*", "", $4); print $4; exit }')" || true
    [ -n "$ip" ] && break
    sleep 2
  done
  [ -n "$ip" ] || die "$name has no IPv4 address after 120 s"
  echo "$name $ip"
}

cmd_push_dir() {
  need_ours "$1"
  [ -d "$2" ] || die "no directory $2"
  incus file push -r -p "$2" "$1/${3#/}" </dev/null
}

cmd_facts() {
  local out="$1" name names
  mkdir -p "$out"
  names="$(incus list pgrust- -f csv -c n </dev/null)"
  {
    echo "## uname -a"; uname -a
    echo "## lscpu"; lscpu
    echo "## incus version"; incus version </dev/null
    echo "## /proc/loadavg"; cat /proc/loadavg
  } >"$out/host.txt"
  : >"$out/containers.txt"
  : >"$out/zfs.txt"
  for name in $names; do
    { echo "## $name"; incus config show "$name" </dev/null; } >>"$out/containers.txt"
    # shellcheck disable=SC2024 # the redirect is meant to run as the caller
    sudo -n zfs get compression,recordsize,used "local/containers/$name" >>"$out/zfs.txt"
  done
  echo "facts: $out ($(echo "$names" | tr '\n' ' '))"
}

cmd_delete() {
  need_ours "$1"
  incus delete --force "$1" </dev/null
}

[ $# -ge 1 ] || usage
cmd="$1"
shift
case "$cmd:$#" in
  create-db:1) cmd_create_db "$@" ;;
  create-client:0) cmd_create_client ;;
  start:1) cmd_start "$@" ;;
  stop:1) cmd_stop "$@" ;;
  stop-others:1) cmd_stop_others "$@" ;;
  disable-upgrades:1) cmd_disable_upgrades "$@" ;;
  wait-net:1) cmd_wait_net "$@" ;;
  push-dir:3) cmd_push_dir "$@" ;;
  facts:1) cmd_facts "$@" ;;
  delete:1) cmd_delete "$@" ;;
  *) usage ;;
esac

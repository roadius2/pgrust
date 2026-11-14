# shellcheck shell=bash
# Shared helpers for the borg benchmark campaign's host-side scripts. Sourced
# by every script in this directory that runs on borg2; not executable on its
# own. Every incus command gets </dev/null: incus launch and incus exec read
# stdin when it is not a terminal and would swallow the caller's input.
#
#   die MSG                 print "error: MSG" to stderr and exit 1
#   X CONTAINER CMD...      run CMD in CONTAINER
#   XB CONTAINER 'STRING'   run the bash -c STRING in CONTAINER
#   settle                  block until the host's 1-minute load average is
#                           below 1.0 on three readings in a row, 10 s apart,
#                           or for at most $SETTLE_MAX_S (600) s; prints them
#   metrics_reset C         open a memory.peak window on container C's cgroup
#   metrics_read C          print "peak=<bytes> current=<bytes> cpu_usec=<usec>":
#                           the peak since the last metrics_reset, the rest now
#   now                     the UTC time, e.g. 2026-11-14T19:42:00Z
#   row FILE FIELDS...      append FIELDS to FILE as one tab-separated line

die() {
  echo "error: $*" >&2
  exit 1
}

X() {
  local c="$1"
  shift
  incus exec "$c" -- "$@" </dev/null
}

XB() {
  [ $# -eq 2 ] || die "usage: XB CONTAINER 'bash -c string'"
  incus exec "$1" -- bash -c "$2" </dev/null
}

SETTLE_MAX_S="${SETTLE_MAX_S:-600}"

# Other people's instances share the host, so settle gives up waiting after
# $SETTLE_MAX_S seconds rather than park an unattended run; the printed
# readings are the record of the load it ran under.
settle() {
  local load ok=0 start="$SECONDS"
  while :; do
    load="$(cut -d' ' -f1 /proc/loadavg)"
    if awk -v l="$load" 'BEGIN { exit !(l < 1.0) }'; then
      ok=$((ok + 1))
    else
      ok=0
    fi
    echo "settle: load1=$load ($ok/3)"
    [ "$ok" -lt 3 ] || return 0
    if [ $((SECONDS - start)) -ge "$SETTLE_MAX_S" ]; then
      echo "settle: proceeding at load $load after $SETTLE_MAX_S s"
      return 0
    fi
    sleep 10
  done
}

# The host-side cgroup of an Incus container (cgroup v2, readable by anyone).
cgroup_dir() {
  local d="/sys/fs/cgroup/lxc.payload.$1"
  [ -d "$d" ] || die "no cgroup $d (is $1 running?)"
  echo "$d"
}

# Since Linux 6.12 a write to memory.peak resets the peak only for reads
# through the same file descriptor; every other open still sees the peak since
# the cgroup was created. So metrics_reset starts this helper as root: it opens
# memory.peak, writes to it and answers "reset"; then, for each metrics_read
# request, it answers with the peak read back through that descriptor. It
# exits when a newer metrics_reset removes its request FIFO or when the shell
# that started it ($4) is gone.
METRICS_HOLDER='
import os, select, sys
peak, req, reply, owner = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
def answer(msg):
    try:
        w = os.open(reply, os.O_WRONLY | os.O_NONBLOCK)
    except OSError:
        return  # the reader gave up
    os.write(w, (msg + "\n").encode())
    os.close(w)
fd = os.open(peak, os.O_RDWR)
os.write(fd, b"1")
r = os.open(req, os.O_RDWR)
answer("reset")
while True:
    if select.select([r], [], [], 1)[0]:
        for _ in range(os.read(r, 4096).count(b"\n")):
            os.lseek(fd, 0, os.SEEK_SET)
            answer(os.read(fd, 64).decode().strip())
        continue
    if os.fstat(r).st_nlink == 0:
        sys.exit(0)
    try:
        os.kill(owner, 0)
    except ProcessLookupError:
        sys.exit(0)
'
METRICS_RUN="${METRICS_RUN:-$HOME/bench/run}"

metrics_reset() {
  local c="$1" d f fd line=""
  d="$(cgroup_dir "$c")" || return 1
  f="$METRICS_RUN/metrics.$c"
  mkdir -p "$METRICS_RUN"
  rm -f "$f.req" "$f.reply"
  mkfifo "$f.req" "$f.reply"
  exec {fd}<>"$f.reply"
  # The double fork re-parents the helper to init, so the caller's bare
  # "wait" does not wait for it; $$ in the subshell is still the caller's PID.
  (setsid sudo -n python3 -c "$METRICS_HOLDER" "$d/memory.peak" "$f.req" "$f.reply" "$$" \
    </dev/null >/dev/null 2>"$f.err" &)
  read -r -t 10 -u "$fd" line || true
  exec {fd}<&-
  [ "$line" = reset ] || die "metrics_reset $c: the helper did not start: $(cat "$f.err")"
}

metrics_read() {
  local c="$1" d f fd w peak=""
  d="$(cgroup_dir "$c")" || return 1
  f="$METRICS_RUN/metrics.$c"
  [ -p "$f.req" ] || die "metrics_read $c: no metrics_reset pending"
  # Read-write opens of a FIFO never block, so a dead helper is a timeout.
  exec {fd}<>"$f.reply" {w}<>"$f.req"
  echo read >&"$w"
  read -r -t 10 -u "$fd" peak || true
  exec {fd}<&- {w}>&-
  [ -n "$peak" ] ||
    die "metrics_read $c: no answer from the helper (metrics_reset runs in the same shell) $(cat "$f.err")"
  echo "peak=$peak current=$(cat "$d/memory.current")" \
    "cpu_usec=$(awk '$1 == "usage_usec" { print $2 }' "$d/cpu.stat")"
}

now() {
  date -u +%FT%TZ
}

row() {
  local file="$1"
  shift
  (IFS=$'\t'; printf '%s\n' "$*") >>"$file"
}

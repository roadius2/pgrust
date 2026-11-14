#!/usr/bin/env bash
# Ships scripts/bench/ from this checkout to ~/bench/scripts/ on borg2, which
# has no rsync, through a tar pipe. Runs on the Mac. The host copy is replaced
# whole, so nothing of the campaign lives only on the cluster.
#
#   scripts/bench/sync.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# COPYFILE_DISABLE and --no-xattrs keep macOS metadata out of the archive;
# the v flag lists what shipped on stderr.
COPYFILE_DISABLE=1 tar czvf - --no-xattrs -C "$here" . |
  ssh borg2 'rm -rf ~/bench/scripts && mkdir -p ~/bench/scripts && tar xzf - -C ~/bench/scripts'
# REVISION names the commit shipped (and "dirty" if scripts/bench differs from
# it); run-phase1.sh copies it into each run's facts.
rev="$(git -C "$here" rev-parse HEAD)"
[ -z "$(git -C "$here" status --porcelain -- .)" ] || rev="$rev dirty"
ssh -n borg2 "echo '$rev' >~/bench/scripts/REVISION"
echo "shipped $here to borg2:~/bench/scripts/ ($rev)"

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
echo "shipped $here to borg2:~/bench/scripts/"

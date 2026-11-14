#!/usr/bin/env bash
# Fetches ann-benchmarks datasets into the host's /home/jody/bench/data, which
# pgrust-client sees read-only at /data. Runs on the host (the client cannot
# write there), shipped by sync.sh:
#   ssh -n borg2 bash ~/bench/scripts/client/datasets.sh NAME...
# e.g. NAME = glove-100-angular, sift-128-euclidean.
#
# Each NAME is http://ann-benchmarks.com/NAME.hdf5. The files are unversioned
# URLs, so every file is checked against SHA256SUMS (beside this script) on
# every run, downloaded or already present: a mismatch is an error and the
# file is not used (a fresh download stays as NAME.hdf5.bad). A NAME with no
# entry yet is pinned: its hash is appended to SHA256SUMS and the script
# prints "PINNED NAME". sync.sh replaces the host's copy of this directory, so
# after a pin copy the file back and commit it:
#   scp borg2:~/bench/scripts/client/SHA256SUMS scripts/bench/client/
# Prints "OK NAME" for a file that matched its pin.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../lib.sh
. "$here/../lib.sh"

DATA_DIR="/home/jody/bench/data"
SUMS="$here/SHA256SUMS"
BASE_URL="http://ann-benchmarks.com"

[ $# -ge 1 ] || die "usage: datasets.sh NAME..."
[ -d "$DATA_DIR" ] || die "no directory $DATA_DIR"
touch "$SUMS"

# The pinned sha256 of file NAME.hdf5, or nothing.
pinned() {
  awk -v f="$1.hdf5" '$2 == f { print $1 }' "$SUMS"
}

fetch() {
  local name="$1" file="$DATA_DIR/$1.hdf5" want got
  case "$name" in
    *[!A-Za-z0-9._-]* | "") die "bad dataset name '$name'" ;;
  esac
  want="$(pinned "$name")"
  if [ ! -f "$file" ]; then
    echo "downloading $BASE_URL/$name.hdf5"
    curl -fL --retry 3 -o "$file.part" "$BASE_URL/$name.hdf5" ||
      die "download of $name failed"
    got="$(sha256sum "$file.part" | cut -d' ' -f1)"
    if [ -n "$want" ] && [ "$got" != "$want" ]; then
      mv "$file.part" "$file.bad"
      die "$name.hdf5: sha256 $got, pinned $want (kept as $file.bad)"
    fi
    mv "$file.part" "$file"
  else
    got="$(sha256sum "$file" | cut -d' ' -f1)"
  fi
  if [ -z "$want" ]; then
    echo "$got  $name.hdf5" >>"$SUMS"
    echo "PINNED $name $got"
  elif [ "$got" = "$want" ]; then
    echo "OK $name $got"
  else
    die "$file: sha256 $got, pinned $want"
  fi
}

for name in "$@"; do
  fetch "$name"
done

#!/usr/bin/env bash
# The recall tier's dataset (M7): GloVe 6B word embeddings, split into base
# rows and held-out queries by recall-prep.awk.
# usage: recall-data.sh <rows> <queries> <dim> <nnz>
#   Prints one line, the absolute path of a directory holding base.tsv and
#   query.tsv; recall-prep.awk gives their format. rows must be a multiple of
#   queries, and dim one of the zip's sizes (50, 100, 200 or 300).
#   PGV_RECALL_CACHE (default ~/.cache/pgrust/recall) holds the zip and the
#   prepared directories, which are kept and reused. PGV_RECALL_TEXT (self-test
#   only) names a GloVe-format text file to use instead of the zip.
set -euo pipefail
. "$(dirname "$0")/common.sh"

GLOVE_URL="https://downloads.cs.stanford.edu/nlp/data/glove.6B.zip"
GLOVE_SHA256="617afb2fe6cbd085c235baf7a465b96f4112bd7f7ccb2b2cbd649fed9cbcf2fb"

usage() { die "usage: recall-data.sh <rows> <queries> <dim> <nnz> (positive integers)"; }
[ "$#" -eq 4 ] || usage
for n in "$@"; do
  case "$n" in
    "" | 0* | *[!0-9]*) usage ;;
  esac
done
rows="$1"
queries="$2"
dim="$3"
nnz="$4"
[ $((rows % queries)) -eq 0 ] || die "rows ($rows) must be a multiple of queries ($queries)"
[ "$nnz" -le "$dim" ] || die "nnz ($nnz) must not exceed dim ($dim)"

prep_awk="$(dirname "$0")/recall-prep.awk"
text="${PGV_RECALL_TEXT:-}"
cache="${PGV_RECALL_CACHE:-$HOME/.cache/pgrust/recall}"
mkdir -p "$cache"
cache="$(cd "$cache" && pwd)"
zip="$cache/glove.6B.zip"

sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }

if [ -n "$text" ]; then
  [ -f "$text" ] || die "PGV_RECALL_TEXT: no such file: $text"
  identity="$(sha256 "$text")"
else
  identity="$GLOVE_SHA256"
fi
key="$({
  cat "$prep_awk"
  printf '%s\n' "$rows" "$queries" "$dim" "$nnz" "$identity"
} | shasum -a 256 | cut -c1-16)"
dir="$cache/prep-$key"
# Only the rename below makes this name, so it is always complete.
if [ -d "$dir" ]; then
  echo "$dir"
  exit 0
fi

if [ -z "$text" ]; then
  if [ ! -e "$zip" ]; then
    echo "downloading $GLOVE_URL" >&2
    curl -fsSL -o "$zip.part" "$GLOVE_URL" || die "download failed: $GLOVE_URL"
    mv "$zip.part" "$zip"
  fi
  got="$(sha256 "$zip")"
  [ "$got" = "$GLOVE_SHA256" ] ||
    die "checksum mismatch on $zip: got $got, expected $GLOVE_SHA256; delete it to download it again"
fi

# unzip -p and cat both run to the end of the stream (recall-prep.awk reads all
# of it), so any failure of theirs is a real one and pipefail reports it.
stream() {
  if [ -n "$text" ]; then
    cat "$text"
  else
    unzip -p "$zip" "glove.6B.${dim}d.txt"
  fi
}

check_lines() {
  local got
  got="$(wc -l <"$part/$1" | tr -d ' ')"
  [ "$got" = "$2" ] || die "$1 has $got lines, expected $2: the source is too short"
}

part="$dir.part"
rm -rf "$part"
trap 'rm -rf "$part"' EXIT
mkdir "$part"
stream | LC_ALL=C awk -v rows="$rows" -v queries="$queries" -v dim="$dim" -v nnz="$nnz" \
  -v base="$part/base.tsv" -v query="$part/query.tsv" -f "$prep_awk" ||
  die "preparation failed; see the message above"
check_lines base.tsv "$rows"
check_lines query.tsv "$queries"
mv "$part" "$dir"
echo "$dir"

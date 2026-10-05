#!/usr/bin/env bash
# Self-tests for the pgvector harness (they check the harness, not pgrust).
# usage: harness_test.sh [test_fn ...]   (default: every test_* function)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/../common.sh"

fails=0
pass() { printf 'ok   - %s\n' "$1"; }
fail() { printf 'FAIL - %s\n' "$1"; fails=$((fails + 1)); }

# --- tests: each task inserts its functions above the runner marker ---

test_reference() {
  if "$here/../build-reference.sh" --verify; then
    pass "reference installs verified"
  else
    fail "reference installs verified"
  fi
}

# --- runner ---
if [ "$#" -eq 0 ]; then
  # shellcheck disable=SC2046 # intentional splitting: function names have no spaces
  set -- $(declare -F | awk '{print $3}' | grep '^test_')
fi
for t in "$@"; do
  echo "# $t"
  "$t"
done
if [ "$fails" -ne 0 ]; then
  echo "$fails failure(s)"
  exit 1
fi
echo "all passed"

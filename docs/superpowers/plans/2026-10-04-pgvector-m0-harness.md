# pgvector Phase 1 · M0: Conformance Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the harness that tests pgrust's pgvector support against real pgvector, prove it works against the C reference, run it once against today's pgrust to get a baseline, and confirm that the hybrid TAP setup works.

**Architecture:** Bash scripts in `scripts/pgvector/` share one `common.sh` (paths, ports, server lifecycle). There are two C installs, both built out of tree under `$PGREF`:
- `pg`: PostgreSQL 18.6 tools. pgrust reads its share directory and uses its `initdb`/`pg_ctl`/`psql`/`pg_regress`.
- `pgvec`: a copy of `pg` with C pgvector 0.8.7 installed. This is the reference server.

The suites come from the vendored upstream sources. pgvector's regression suite runs through `pg_regress` against a running server. Its TAP suite runs through `prove`; in pgrust mode, a hybrid bin directory lets C `initdb` create clusters that pgrust then runs.

**Tech Stack:** bash (3.2-compatible), PostgreSQL 18.6 C build (autoconf, VPATH), pgvector 0.8.7 (PGXS), Perl `prove` + IPC::Run, Docker, cargo (`fast-profile`).

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, sections §8 (testing) and §9 (M0 row). This plan covers **M0 only**. M1–M7 each get their own plan once the previous milestone lands, and M1's plan starts from the baseline this plan produces.

## Global Constraints

- PostgreSQL reference: **18.6**, tag `REL_18_6`, commit `724edf9bde9d356724ad384a2e196edc3c9f80f7`, at `crates/postgres-18.6-reference`.
- pgvector reference: **0.8.7**, tag `v0.8.7`, commit `f37c13f68b57d2c3472b2214fbcff699d6d34876`, vendored at `crates/pgvector-0.8.7-reference`.
- Reference trees are read-only: never edit them and never build inside them. Use VPATH or copy builds only. `git status --porcelain --ignored` on both trees must stay empty.
- C pgvector is built with `make OPTFLAGS=""`, matching the Docker image's build.
- Docker reference image: `pgvector/pgvector:0.8.7-pg18@sha256:2358fcba361ed2233a5ed81b5fe4ca779ccb304120ce531a3bf51c0ed7e2bc11`. M5 will also use `pgvector/pgvector:0.8.1-pg18@sha256:508c5290cda481d4f5f846446a26e9c1b804766828a394a5861de1b348a18b4c`.
- Every cluster is created with `initdb --no-locale --encoding UTF8 -U postgres`.
- pgrust runtime settings (README quickstart):
  - `ulimit -s 65520`
  - `RUST_MIN_STACK=33554432`
  - `PGRUST_PGSHAREDIR=<PG 18.6 tools sharedir>`
  - `PGRUST_TZDIR=<sharedir>/timezone`
  - server options `-c listen_addresses= -c io_method=sync -c max_stack_depth=60000`
- pgrust binary: `cargo build --profile fast-profile --locked --bin postgres`, producing `target/fast-profile/postgres`. Requires RE2.
- Harness code lives in `scripts/pgvector/`. All generated state lives outside the repo under `$PGREF` (default `~/.cache/pgrust/pgref-18.6`) and `$PGV_WORK` (default `~/.cache/pgrust/pgvector-work`).
- Ports: pgrust `55491`, C reference `55492`, Docker reference `55493`.
- Scripts are `#!/usr/bin/env bash` with `set -euo pipefail` and must be bash 3.2 compatible: no associative arrays, no `${x,,}`, no `mapfile`, no `sed -i`, no GNU-only flags.
- Upstream expected outputs are never edited.
- Commit to branch `vector/phase1`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **Stray writes into reference trees.** pg_regress writes `results/` and `regression.*`, TAP writes `tmp_check/` and `log/`, and pgvector's own `.gitignore` hides exactly those. Expected: every run leaves both reference trees clean, ignored files included. Test: `assert_reference_clean` in Task 4 and Task 5.
2. **pgrust silently loading C pgvector's extension files** from a shared share directory. Expected: pgrust's `CREATE EXTENSION vector` reports pgrust's own staged `default_version`, never C's `0.8.7` script. Test: extension-version check in Task 3.
3. **A harness port already taken** by a leftover or foreign server. Expected: start refuses with a clear message instead of running tests against the wrong server. Test: second-start check in Task 3.
4. **Running "pgrust" tests against a non-pgrust server**, for example a misbuilt binary or a stray C postgres. Expected: the runner refuses with "not a pgrust server". Test: `test_regress_identity` in Task 4.
5. **Version-string mismatch breaking the hybrid TAP setup.** `pg_ctl`/`initdb` require `postgres -V` to print exactly `postgres (PostgreSQL) 18.6`. Expected: the build scripts check exact strings for both C tools and pgrust. Tests: `test_reference` in Task 2, `test_pgrust_binary` in Task 3.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/pgvector-0.8.7-reference/**` | Vendored upstream pgvector, read-only (Task 1) |
| `crates/pgvector-0.8.7-reference/README-WHY-THIS-IS-HERE.md` | Provenance and do-not-edit rules (Task 1) |
| `scripts/pgvector/common.sh` | Shared paths, ports, helpers, server lifecycle (Tasks 2–4) |
| `scripts/pgvector/build-reference.sh` | Build and verify the C installs `pg` and `pgvec` (Task 2) |
| `scripts/pgvector/build-pgrust.sh` | Build and verify the pgrust server binary (Task 3) |
| `scripts/pgvector/server.sh` | Manual server control: `start`, `fresh`, `stop`, `psql` (Task 3) |
| `scripts/pgvector/run-regress.sh` | pgvector SQL regression suite → `summary.tsv` (Task 4) |
| `scripts/pgvector/run-tap.sh` | pgvector TAP suite, including the hybrid bin directory → `summary.tsv` (Task 5) |
| `scripts/pgvector/docker-ref.sh` | Docker pgvector reference: `up`, `down`, `psql` (Task 6) |
| `scripts/pgvector/run-all.sh` | Run all M0 tiers and write the pass/fail report (Task 7) |
| `scripts/pgvector/tests/harness_test.sh` | Self-tests of the harness; each task adds functions (Tasks 2–6) |
| `docs/superpowers/reports/pgvector-m0-baseline.md` | Baseline results for today's pgrust (Task 7) |
| `CLAUDE.md` | Add harness usage to the vector section (Task 7) |

---

### Task 1: Vendor pgvector 0.8.7 as a read-only reference

**Files:**
- Create: `crates/pgvector-0.8.7-reference/**` (upstream `git archive` of `v0.8.7`)
- Create: `crates/pgvector-0.8.7-reference/README-WHY-THIS-IS-HERE.md`

**Interfaces:**
- Produces: the paths `crates/pgvector-0.8.7-reference/test/sql/*.sql` (14 files), `test/expected/*.out` (14), `test/t/*.pl` (48), `test/perl/`, `sql/vector.sql`, and `src/*.c` (19). Every later task reads them.

- [ ] **Step 1: Write the failing check**

Run from the repo root:
```bash
cd /Users/jody/dev_projects/pgrust
ls crates/pgvector-0.8.7-reference/test/sql/*.sql | wc -l
```
Expected: an error (`No such file or directory`) and a count of `0`.

- [ ] **Step 2: Export the tagged tree**

```bash
cd /Users/jody/dev_projects/pgrust
tmp="$(mktemp -d)"
git clone --quiet --depth 1 --branch v0.8.7 https://github.com/pgvector/pgvector.git "$tmp/pgvector"
test "$(git -C "$tmp/pgvector" rev-parse HEAD)" = f37c13f68b57d2c3472b2214fbcff699d6d34876 && echo "commit ok"
mkdir crates/pgvector-0.8.7-reference
git -C "$tmp/pgvector" archive HEAD | tar -x -C crates/pgvector-0.8.7-reference
git -C "$tmp/pgvector" archive HEAD | tar -t | grep -v '/$' | sort >"$tmp/archive.txt"
```
Expected: `commit ok`.

- [ ] **Step 3: Write the provenance README**

Create `crates/pgvector-0.8.7-reference/README-WHY-THIS-IS-HERE.md`:
```markdown
# pgvector 0.8.7 source — reference copy for comparison only

This folder contains the pristine pgvector 0.8.7 source tree, extracted with
`git archive` from the upstream `v0.8.7` tag (commit
`f37c13f68b57d2c3472b2214fbcff699d6d34876`, "Version bump to 0.8.7"). It is
the behavior reference for pgrust's pgvector port (`crates/contrib/pgvector*`)
and its `test/` directory is the conformance suite run by `scripts/pgvector/`.

- **Do not edit anything under this folder.** It is a read-only reference.
- **Do not build from it here.** `scripts/pgvector/build-reference.sh` builds a
  copy under `$PGREF`; pg_regress/TAP output goes to `$PGV_WORK`.
- Not a Cargo workspace member (no `Cargo.toml`), not compiled, not read by any
  `build.rs`.
- The upstream `.gitignore` travels with the archive. It ignores
  `/sql/vector--?.?.?.sql` (generated by `make` from `sql/vector.sql`) and test
  output (`/results/`, `regression.*`, `/tmp_check/`, `/log/`), none of which
  the archive contains.
- Licensed under the PostgreSQL License (`LICENSE`).
```

- [ ] **Step 4: Stage and confirm that nothing was skipped**

```bash
cd /Users/jody/dev_projects/pgrust
git add -f crates/pgvector-0.8.7-reference
git ls-files crates/pgvector-0.8.7-reference | sed 's|^crates/pgvector-0.8.7-reference/||' \
  | grep -v '^README-WHY-THIS-IS-HERE.md$' | sort >"$tmp/staged.txt"
diff "$tmp/archive.txt" "$tmp/staged.txt" && echo "no files skipped"
ls crates/pgvector-0.8.7-reference/test/sql/*.sql | wc -l
ls crates/pgvector-0.8.7-reference/test/expected/*.out | wc -l
ls crates/pgvector-0.8.7-reference/test/t/*.pl | wc -l
ls crates/pgvector-0.8.7-reference/src/*.c | wc -l
cargo metadata --no-deps --format-version 1 >/dev/null && echo "workspace unaffected"
rm -rf "$tmp"
```
Expected: `no files skipped`, then `14`, `14`, `48`, `19`, then `workspace unaffected`.

- [ ] **Step 5: Commit**

```bash
git commit -q -F - <<'EOF'
Vendor pgvector 0.8.7 as a read-only reference tree

git archive of upstream v0.8.7 (f37c13f6), the behavior reference and
conformance suite for the pgvector port. Not compiled, never edited.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: Shared harness environment and the C reference build

**Files:**
- Create: `scripts/pgvector/common.sh`
- Create: `scripts/pgvector/build-reference.sh`
- Create: `scripts/pgvector/tests/harness_test.sh`

**Interfaces:**
- Consumes: the Task 1 paths.
- Produces (in `common.sh`, which later tasks source):
  - Variables: `PGV_REPO`, `PG_SRC`, `PGV_SRC`, `PGREF`, `PGV_WORK`, `PG_TOOLS` (`$PGREF/pg`), `PG_VEC` (`$PGREF/pgvec`), `PGV_PERL5LIB`, `PGV_PORT_PGRUST=55491`, `PGV_PORT_REF=55492`, `PGV_PORT_DOCKER=55493`, `PGV_DOCKER_IMAGE`.
  - `die MSG...`: prints `error: MSG` to stderr, exit 1.
  - `ncpu`: prints the CPU count.
  - `pg_regress_bin`: prints the absolute path of the installed `pg_regress`.
- Produces `build-reference.sh [--verify]`: builds missing pieces, then verifies. `--verify` only verifies. Exits 0 when everything is verified.
- Produces `tests/harness_test.sh [test_fn ...]`: runs the named `test_*` functions (default: all) and exits non-zero on any failure. Helpers: `pass NAME`, `fail NAME`, `$here` (the tests dir).

- [ ] **Step 1: Install prerequisites**

```bash
brew install cpanminus
```
Expected: `cpanm --version` prints a version. bison 2.3 and flex 2.6.4 ship with macOS; PostgreSQL 18 requires bison ≥ 2.3.

- [ ] **Step 2: Write `common.sh` (the environment half; Task 3 adds the server half)**

Create `scripts/pgvector/common.sh`:
```bash
# shellcheck shell=bash
# Shared environment for the pgvector conformance harness. Sourced by the
# scripts in this directory; not executable on its own. bash 3.2 compatible.
#
# Two C installs live under $PGREF, both built outside the repo:
#   $PG_TOOLS  PostgreSQL 18.6 only. pgrust reads its share/ (timezone data)
#              and uses its initdb, pg_ctl, psql and pg_regress.
#   $PG_VEC    Copy of $PG_TOOLS plus C pgvector 0.8.7: the reference server.
#              Kept separate so pgrust never sees C pgvector's control file
#              or upgrade scripts.

PGV_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PG_SRC="$PGV_REPO/crates/postgres-18.6-reference"
PGV_SRC="$PGV_REPO/crates/pgvector-0.8.7-reference"

PGREF="${PGREF:-$HOME/.cache/pgrust/pgref-18.6}"
PGV_WORK="${PGV_WORK:-$HOME/.cache/pgrust/pgvector-work}"
PG_TOOLS="$PGREF/pg"
PG_VEC="$PGREF/pgvec"
PGV_PERL5LIB="$PGREF/perl5/lib/perl5"

PGV_PORT_PGRUST=55491
PGV_PORT_REF=55492
PGV_PORT_DOCKER=55493
PGV_DOCKER_IMAGE="pgvector/pgvector:0.8.7-pg18@sha256:2358fcba361ed2233a5ed81b5fe4ca779ccb304120ce531a3bf51c0ed7e2bc11"

die() {
  echo "error: $*" >&2
  exit 1
}

ncpu() {
  sysctl -n hw.ncpu 2>/dev/null || nproc
}

pg_regress_bin() {
  echo "$("$PG_TOOLS/bin/pg_config" --pkglibdir)/pgxs/src/test/regress/pg_regress"
}
```

- [ ] **Step 3: Write the self-test runner with the failing `test_reference`**

Create `scripts/pgvector/tests/harness_test.sh`:
```bash
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
```
Then:
```bash
chmod +x scripts/pgvector/tests/harness_test.sh
scripts/pgvector/tests/harness_test.sh test_reference
```
Expected: `FAIL - reference installs verified`, because `build-reference.sh` doesn't exist yet. Exit 1.

- [ ] **Step 4: Write `build-reference.sh`**

Create `scripts/pgvector/build-reference.sh`:
```bash
#!/usr/bin/env bash
# Build the C reference installs under $PGREF (see common.sh), then verify.
# usage: build-reference.sh [--verify]
set -euo pipefail
. "$(dirname "$0")/common.sh"

verify_reference() {
  local fails=0 tools_share vec_share
  expect() { # expect DESCRIPTION ACTUAL WANTED
    if [ "$2" = "$3" ]; then
      echo "ok   - $1"
    else
      echo "FAIL - $1: got '$2', want '$3'"
      fails=$((fails + 1))
    fi
  }
  tools_share="$("$PG_TOOLS/bin/pg_config" --sharedir 2>/dev/null || true)"
  vec_share="$("$PG_VEC/bin/pg_config" --sharedir 2>/dev/null || true)"
  # pg_ctl and initdb accept only a postgres whose -V output matches exactly.
  expect "tools postgres -V" "$("$PG_TOOLS/bin/postgres" -V 2>/dev/null)" "postgres (PostgreSQL) 18.6"
  expect "tools pg_ctl -V" "$("$PG_TOOLS/bin/pg_ctl" -V 2>/dev/null)" "pg_ctl (PostgreSQL) 18.6"
  expect "reference postgres -V" "$("$PG_VEC/bin/postgres" -V 2>/dev/null)" "postgres (PostgreSQL) 18.6"
  expect "pg_regress installed" "$([ -x "$(pg_regress_bin 2>/dev/null)" ] && echo yes)" "yes"
  expect "reference has pgvector" \
    "$(sed -n "s/^default_version = '\(.*\)'$/\1/p" "$vec_share/extension/vector.control" 2>/dev/null)" "0.8.7"
  expect "tools share has no pgvector" \
    "$([ -e "$tools_share/extension/vector.control" ] && echo present || echo absent)" "absent"
  expect "IPC::Run loadable" "$(PERL5LIB="$PGV_PERL5LIB" perl -MIPC::Run -e 'print "yes"' 2>/dev/null)" "yes"
  [ "$fails" -eq 0 ] || die "$fails reference check(s) failed"
}

if [ "${1:-}" = "--verify" ]; then
  verify_reference
  exit 0
fi

# 1. IPC::Run, needed by `configure --enable-tap-tests` and by the TAP suite.
if ! PERL5LIB="$PGV_PERL5LIB" perl -MIPC::Run -e1 2>/dev/null; then
  command -v cpanm >/dev/null || die "cpanm not found: brew install cpanminus"
  cpanm --notest -l "$PGREF/perl5" IPC::Run
fi
export PERL5LIB="$PGV_PERL5LIB${PERL5LIB:+:$PERL5LIB}"

# 2. PostgreSQL 18.6, VPATH build: nothing is written into the vendored tree.
if [ ! -x "$PG_TOOLS/bin/postgres" ]; then
  build="$PGREF/build/pg"
  rm -rf "$build"
  mkdir -p "$build"
  echo "building PostgreSQL 18.6 in $build (a few minutes)"
  # Explicit && chain: set -e is suspended inside a subshell that is the left
  # side of ||, so a failed configure would otherwise run on into make.
  (
    cd "$build" &&
      "$PG_SRC/configure" --prefix="$PG_TOOLS" --without-icu --without-readline \
        --enable-tap-tests --enable-debug >configure.log 2>&1 &&
      make -j"$(ncpu)" >make.log 2>&1 &&
      make install >install.log 2>&1
  ) || die "PostgreSQL build failed; logs in $build"
fi

# 3. Reference server install: a copy of the tools install (PostgreSQL installs
#    are relocatable) plus C pgvector, built from a copy of the vendored tree.
if [ ! -d "$PG_VEC" ]; then
  cp -R "$PG_TOOLS" "$PG_VEC"
fi
vec_share="$("$PG_VEC/bin/pg_config" --sharedir)"
if [ ! -f "$vec_share/extension/vector.control" ]; then
  build="$PGREF/build/pgvector"
  rm -rf "$build"
  mkdir -p "$build"
  cp -R "$PGV_SRC/." "$build/"
  echo "building pgvector 0.8.7 in $build"
  (
    cd "$build" &&
      make OPTFLAGS="" PG_CONFIG="$PG_VEC/bin/pg_config" >make.log 2>&1 &&
      make install PG_CONFIG="$PG_VEC/bin/pg_config" >install.log 2>&1
  ) || die "pgvector build failed; logs in $build"
fi

verify_reference
```

- [ ] **Step 5: Build, then run the test until it passes**

```bash
chmod +x scripts/pgvector/build-reference.sh
scripts/pgvector/build-reference.sh
scripts/pgvector/tests/harness_test.sh test_reference
git -C /Users/jody/dev_projects/pgrust status --porcelain --ignored -- crates/postgres-18.6-reference crates/pgvector-0.8.7-reference
```
Expected: the build prints seven `ok   - ...` lines, the harness test prints `ok   - reference installs verified` and `all passed`, and the final `git status` prints nothing.

If `configure` fails, read `$PGREF/build/pg/configure.log`. A bison complaint means you should `brew install bison` and rerun with `BISON=$(brew --prefix bison)/bin/bison` exported. Don't change any other configure flag without noting it in the commit message.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/common.sh scripts/pgvector/build-reference.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: shared env and C reference build

Builds PostgreSQL 18.6 (tools for pgrust) and a separate copy with C
pgvector 0.8.7 (reference server) out of tree under $PGREF, and verifies
exact version strings needed by pg_ctl/initdb.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: pgrust binary and server lifecycle

**Files:**
- Modify: `scripts/pgvector/common.sh` (append the server half)
- Create: `scripts/pgvector/build-pgrust.sh`
- Create: `scripts/pgvector/server.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (insert `test_pgrust_binary` and `test_servers` above `# --- runner ---`)

**Interfaces:**
- Consumes: `die`, `PG_TOOLS`, `PG_VEC`, the port variables, and `PGV_WORK` from Task 2.
- Produces, in `common.sh`:
  - `PGRUST_PROFILE` (default `fast-profile`), `PGRUST_BIN` (default `$PGV_REPO/target/$PGRUST_PROFILE/postgres`, overridable).
  - `PGRUST_SERVER_OPTS` (array).
  - `port_for MODE`: `pgrust`→55491, `ref`→55492, anything else → `die`.
  - `pgrust_env`: exports `PGRUST_PGSHAREDIR`, `PGRUST_TZDIR`, `RUST_MIN_STACK`.
  - `pgrust_ext_version`: prints `default_version` from pgrust's staged `vector.control`.
  - `pgv_psql PORT [psql args]`: C psql as user `postgres` over the harness socket, with `-X -q -v ON_ERROR_STOP=1`.
  - `wait_ready PORT`: returns 0 within 60 s once the server accepts connections, 1 otherwise.
  - `server_start MODE DATADIR PORT`: initdb if `DATADIR` is missing, start in the background, wait, check identity. Refuses if the port is in use.
  - `server_stop MODE`: fast shutdown via the pid file. A no-op if nothing is running.
  - `assert_identity MODE PORT`: `die`s with `not a pgrust server` (pgrust mode) or `is a pgrust server` (ref mode) on a mismatch.
- Produces `build-pgrust.sh` and `server.sh {start|fresh|stop|psql} {pgrust|ref} [psql args]`.

- [ ] **Step 1: Write the failing tests**

Insert above `# --- runner ---` in `scripts/pgvector/tests/harness_test.sh`:
```bash
test_pgrust_binary() {
  # pg_ctl (hybrid TAP) accepts only this exact -V output.
  local v
  v="$("$PGRUST_BIN" -V 2>&1 || true)"
  if [ "$v" = "postgres (PostgreSQL) 18.6" ]; then pass "pgrust -V"; else fail "pgrust -V: '$v'"; fi
  if [ -n "$(pgrust_ext_version)" ]; then
    pass "pgrust staged vector.control"
  else
    fail "pgrust staged vector.control missing under target/$PGRUST_PROFILE/share/extension"
  fi
}

test_servers() {
  local mode port v ext want d
  for mode in ref pgrust; do
    port="$(port_for "$mode")"
    if ! "$here/../server.sh" fresh "$mode" >/dev/null 2>&1; then
      fail "$mode: server starts (see $PGV_WORK/log/server-$mode.log)"
      continue
    fi
    pass "$mode: server starts"
    v="$(pgv_psql "$port" -Atc 'select version()' 2>&1 || true)"
    case "$mode:$v" in
      "pgrust:PostgreSQL 18.6"*"(pgrust "*) pass "$mode: version" ;;
      "ref:PostgreSQL 18.6"*"(pgrust "*) fail "$mode: version is pgrust: $v" ;;
      "ref:PostgreSQL 18.6"*) pass "$mode: version" ;;
      *) fail "$mode: version: $v" ;;
    esac
    # Review focus 2: pgrust must load its own staged extension files.
    ext="$(pgv_psql "$port" -Atc 'create extension vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
    if [ "$mode" = ref ]; then want=0.8.7; else want="$(pgrust_ext_version)"; fi
    if [ "$ext" = "$want" ]; then pass "$mode: extension version $want"; else fail "$mode: extension version: got '$ext', want '$want'"; fi
    d="$(pgv_psql "$port" -Atc "select '[1,2,3]'::vector <-> '[4,5,6]'" 2>&1 || true)"
    if [ "$d" = "5.196152422706632" ]; then pass "$mode: l2 distance"; else fail "$mode: l2 distance: '$d'"; fi
    # Review focus 3: a second start on a busy port must refuse.
    if "$here/../server.sh" start "$mode" >/dev/null 2>&1; then
      fail "$mode: second start refused"
    else
      pass "$mode: second start refused"
    fi
    if "$here/../server.sh" stop "$mode" >/dev/null 2>&1; then pass "$mode: server stops"; else fail "$mode: server stops"; fi
  done
}
```
Run:
```bash
scripts/pgvector/tests/harness_test.sh test_pgrust_binary test_servers
```
Expected: FAIL. `port_for`/`pgrust_ext_version` aren't defined yet, so the run aborts with `command not found` and a non-zero exit.

- [ ] **Step 2: Append the server half to `common.sh`**

Append to `scripts/pgvector/common.sh`:
```bash

# --- pgrust binary and servers ---

PGRUST_PROFILE="${PGRUST_PROFILE:-fast-profile}"
PGRUST_BIN="${PGRUST_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/postgres}"
PGRUST_SERVER_OPTS=(-c listen_addresses= -c io_method=sync -c max_stack_depth=60000)

port_for() {
  case "$1" in
    pgrust) echo "$PGV_PORT_PGRUST" ;;
    ref) echo "$PGV_PORT_REF" ;;
    *) die "unknown mode '$1' (expected pgrust or ref)" ;;
  esac
}

# Runtime environment from the README quickstart. pgrust reads timezone data
# from a PostgreSQL share dir; extensions come from its own staged
# target/<profile>/share/extension first.
pgrust_env() {
  local share
  share="$("$PG_TOOLS/bin/pg_config" --sharedir)"
  export PGRUST_PGSHAREDIR="$share"
  export PGRUST_TZDIR="$share/timezone"
  export RUST_MIN_STACK=33554432
}

pgrust_ext_version() {
  sed -n "s/^default_version = '\(.*\)'$/\1/p" \
    "$(dirname "$PGRUST_BIN")/share/extension/vector.control" 2>/dev/null || true
}

pgv_psql() {
  local port="$1"
  shift
  "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 -h "$PGV_WORK/sock" -p "$port" -U postgres -d postgres "$@"
}

wait_ready() {
  local port="$1" i
  for i in $(seq 1 120); do
    if "$PG_TOOLS/bin/pg_isready" -q -h "$PGV_WORK/sock" -p "$port"; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}

assert_identity() {
  local mode="$1" port="$2" v
  v="$(pgv_psql "$port" -Atc 'select version()')" || die "cannot query the server on port $port"
  case "$mode:$v" in
    "pgrust:"*"(pgrust "*) ;;
    "pgrust:"*) die "port $port is not a pgrust server: $v" ;;
    "ref:"*"(pgrust "*) die "port $port is a pgrust server, expected the C reference: $v" ;;
    "ref:PostgreSQL 18.6"*) ;;
    *) die "port $port runs an unexpected server: $v" ;;
  esac
}

server_start() {
  local mode="$1" data="$2" port="$3" log
  mkdir -p "$PGV_WORK/sock" "$PGV_WORK/log"
  if "$PG_TOOLS/bin/pg_isready" -q -h "$PGV_WORK/sock" -p "$port"; then
    die "a server is already running on port $port; stop it first (scripts/pgvector/server.sh stop $mode)"
  fi
  if [ ! -d "$data" ]; then
    "$PG_TOOLS/bin/initdb" -D "$data" --no-locale --encoding UTF8 -U postgres -A trust \
      >"$PGV_WORK/log/initdb-$mode.log" 2>&1 || die "initdb failed; see $PGV_WORK/log/initdb-$mode.log"
  fi
  log="$PGV_WORK/log/server-$mode.log"
  case "$mode" in
    pgrust)
      [ -x "$PGRUST_BIN" ] || die "missing $PGRUST_BIN; run scripts/pgvector/build-pgrust.sh"
      (
        pgrust_env
        ulimit -s 65520
        exec "$PGRUST_BIN" -D "$data" -k "$PGV_WORK/sock" -p "$port" "${PGRUST_SERVER_OPTS[@]}"
      ) >"$log" 2>&1 &
      ;;
    ref)
      (exec "$PG_VEC/bin/postgres" -D "$data" -k "$PGV_WORK/sock" -p "$port" -c listen_addresses=) >"$log" 2>&1 &
      ;;
    *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
  esac
  echo $! >"$PGV_WORK/$mode.pid"
  wait_ready "$port" || die "server ($mode) did not become ready; see $log"
  assert_identity "$mode" "$port"
}

server_stop() {
  local mode="$1" pidfile="$PGV_WORK/$1.pid" pid i
  [ -f "$pidfile" ] || return 0
  pid="$(cat "$pidfile")"
  if kill -0 "$pid" 2>/dev/null; then
    kill -INT "$pid"
    for i in $(seq 1 60); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
    if kill -0 "$pid" 2>/dev/null; then
      die "server ($mode, pid $pid) did not stop"
    fi
  fi
  rm -f "$pidfile"
}
```

- [ ] **Step 3: Write `build-pgrust.sh` and `server.sh`**

Create `scripts/pgvector/build-pgrust.sh`:
```bash
#!/usr/bin/env bash
# Build the pgrust server used by the harness: target/$PGRUST_PROFILE/postgres.
set -euo pipefail
. "$(dirname "$0")/common.sh"

pkg-config --exists re2 || die "RE2 not found (release-family profiles require it): brew install re2 pkg-config"
(cd "$PGV_REPO" && cargo build --profile "$PGRUST_PROFILE" --locked --bin postgres)
v="$("$PGRUST_BIN" -V)"
[ "$v" = "postgres (PostgreSQL) 18.6" ] || die "unexpected pgrust -V output: '$v'"
echo "built $PGRUST_BIN ($v; staged vector extension $(pgrust_ext_version))"
```
Create `scripts/pgvector/server.sh`:
```bash
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
```

- [ ] **Step 4: Install RE2 and build pgrust**

```bash
brew install re2 pkg-config
chmod +x scripts/pgvector/build-pgrust.sh scripts/pgvector/server.sh
scripts/pgvector/build-pgrust.sh
```
Expected: a long first build (the whole ~890-crate workspace; expect tens of minutes), ending with `built .../target/fast-profile/postgres (postgres (PostgreSQL) 18.6; staged vector extension 0.8.5)`.

If `-V` fails because pgrust needs its runtime environment even for `-V`, change only `build-pgrust.sh` to run `v="$(pgrust_env; "$PGRUST_BIN" -V)"` in a subshell, and record that in the commit message.

- [ ] **Step 5: Run the tests until they pass**

```bash
scripts/pgvector/tests/harness_test.sh test_pgrust_binary test_servers
```
Expected: every line `ok   - ...` (2 for the binary, 6 per mode), then `all passed`.

If the pgrust server fails to start, read `$PGV_WORK/log/server-pgrust.log`. Startup failures caused by pgrust core behavior are findings: record them for the baseline and stop to report them. Don't patch pgrust in M0.

- [ ] **Step 6: Commit**

```bash
git add scripts/pgvector/common.sh scripts/pgvector/build-pgrust.sh scripts/pgvector/server.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: pgrust build and server lifecycle

Fresh-cluster start/stop for pgrust and the C reference on fixed ports,
with identity checks so a run can never target the wrong server, and a
check that pgrust loads its own staged extension files.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: Regression-suite runner

**Files:**
- Create: `scripts/pgvector/run-regress.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (insert `assert_reference_clean`, `test_regress_ref`, `test_regress_identity` above `# --- runner ---`)

**Interfaces:**
- Consumes: `server_start`, `server_stop`, `port_for`, `pg_regress_bin`, `PG_TOOLS`, `PGV_SRC`, `PGV_WORK` (Tasks 2–3).
- Produces: `run-regress.sh {pgrust|ref} [test ...]`, which runs the 14 tests in `test/sql` by default. It writes `$PGV_WORK/regress/MODE/{pg_regress.log,summary.tsv,regression.diffs,results/}`. `summary.tsv` has one line per test, `NAME<TAB>ok|FAIL`. The exit code is pg_regress's (0 only if every test passed).

- [ ] **Step 1: Write the failing tests**

Insert above `# --- runner ---`:
```bash
# Review focus 1: suites must never write into the read-only reference trees.
# --ignored matters: pgvector's .gitignore hides results/ and regression.*.
assert_reference_clean() {
  [ -z "$(git -C "$PGV_REPO" status --porcelain --ignored -- crates/pgvector-0.8.7-reference crates/postgres-18.6-reference)" ]
}

test_regress_ref() {
  local n
  if "$here/../run-regress.sh" ref >/dev/null 2>&1; then
    pass "regress ref: exit 0"
  else
    fail "regress ref: exit 0 (see $PGV_WORK/regress/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/regress/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = 14 ]; then pass "regress ref: 14 ok"; else fail "regress ref: '$n' ok, want 14"; fi
  if assert_reference_clean; then pass "regress: reference trees untouched"; else fail "regress: reference trees modified"; fi
}

# Review focus 4: pgrust mode must refuse a server that isn't pgrust.
test_regress_identity() {
  local log="$PGV_WORK/identity-test.log"
  if PGRUST_BIN="$PG_VEC/bin/postgres" "$here/../run-regress.sh" pgrust bit >"$log" 2>&1; then
    fail "pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$log"; then
    pass "pgrust mode refuses a C server"
  else
    fail "pgrust mode refuses a C server: wrong error (see $log)"
  fi
  "$here/../server.sh" stop pgrust >/dev/null 2>&1 || true
}
```
Run:
```bash
scripts/pgvector/tests/harness_test.sh test_regress_ref test_regress_identity
```
Expected: FAIL. `run-regress.sh` doesn't exist yet.

- [ ] **Step 2: Write `run-regress.sh`**

Create `scripts/pgvector/run-regress.sh`:
```bash
#!/usr/bin/env bash
# Run pgvector's SQL regression suite against a fresh server, the same way
# pgvector's `make installcheck` does (--inputdir=test --load-extension=vector).
# usage: run-regress.sh {pgrust|ref} [test ...]   (default: all of test/sql)
set -euo pipefail
. "$(dirname "$0")/common.sh"

[ "$#" -ge 1 ] || die "usage: run-regress.sh {pgrust|ref} [test ...]"
mode="$1"
shift
port="$(port_for "$mode")"
out="$PGV_WORK/regress/$mode"

if [ "$#" -gt 0 ]; then
  tests=("$@")
else
  tests=()
  for f in "$PGV_SRC"/test/sql/*.sql; do
    tests+=("$(basename "$f" .sql)")
  done
fi

rm -rf "$out"
mkdir -p "$out"
server_stop "$mode"
rm -rf "$PGV_WORK/data/$mode"
# Trap first: if server_start dies after launching (e.g. the identity check),
# the server must still be stopped.
trap 'server_stop "$mode"' EXIT
server_start "$mode" "$PGV_WORK/data/$mode" "$port"

set +e
"$(pg_regress_bin)" --bindir="$PG_TOOLS/bin" --host="$PGV_WORK/sock" --port="$port" --user=postgres \
  --inputdir="$PGV_SRC/test" --outputdir="$out" \
  --dbname=contrib_regression --load-extension=vector \
  "${tests[@]}" >"$out/pg_regress.log" 2>&1
rc=$?
set -e

# pg_regress (PG 16+) prints "ok N - name ..." / "not ok N - name ...".
sed -n -E 's/^(not ok|ok)[[:space:]]+[0-9]+[[:space:]]+[-+][[:space:]]+([^[:space:]]+).*$/\2 \1/p' "$out/pg_regress.log" |
  while read -r name result; do
    if [ "$result" = ok ]; then printf '%s\tok\n' "$name"; else printf '%s\tFAIL\n' "$name"; fi
  done >"$out/summary.tsv"

passed="$(grep -c $'\tok$' "$out/summary.tsv" || true)"
echo "regress ($mode): $passed/${#tests[@]} passed; log $out/pg_regress.log; diffs $out/regression.diffs"
exit "$rc"
```

- [ ] **Step 3: Run the tests until they pass**

```bash
chmod +x scripts/pgvector/run-regress.sh
scripts/pgvector/tests/harness_test.sh test_regress_ref test_regress_identity
```
Expected: `ok   - regress ref: exit 0`, `ok   - regress ref: 14 ok`, `ok   - regress: reference trees untouched`, `ok   - pgrust mode refuses a C server`, `all passed`.

If the ref run fails, the harness is wrong, not pgvector: the C reference must pass its own suite. Read `$PGV_WORK/regress/ref/regression.diffs` and fix the runner. A `summary.tsv` with no lines means the `sed` pattern doesn't match the log; compare it against `pg_regress.log`.

- [ ] **Step 4: Record today's pgrust result (expected to fail partly; nothing to fix)**

```bash
scripts/pgvector/run-regress.sh pgrust || true
cat "$PGV_WORK/regress/pgrust/summary.tsv"
```
Expected: 14 lines. Failures are expected: halfvec, sparsevec, bit and IVFFlat aren't ported yet. Task 7 records this.

- [ ] **Step 5: Commit**

```bash
git add scripts/pgvector/run-regress.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: regression-suite runner

Runs pgvector's 14 SQL regression files via pg_regress against a fresh
pgrust or C reference server; output stays in $PGV_WORK. The C reference
passes 14/14, validating the runner.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 5: TAP runner and the hybrid-setup spike

**Files:**
- Create: `scripts/pgvector/run-tap.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (insert `test_tap_ref` above `# --- runner ---`)

**Interfaces:**
- Consumes: `PG_TOOLS`, `PG_VEC`, `PG_SRC`, `PGV_SRC`, `PGV_WORK`, `PGV_PERL5LIB`, `PGRUST_BIN`, `pg_regress_bin` (Tasks 2–3), and `assert_reference_clean` (Task 4, test file).
- Produces: `run-tap.sh {pgrust|ref} [NNN_name.pl ...]`, which runs every `test/t/*.pl` by default. It writes `$PGV_WORK/tap/MODE/{summary.tsv,prove/NAME.log,log/,data/}`. `summary.tsv` has one line per test, `NAME<TAB>ok|FAIL`. Exits 1 if any test fails.
- Produces: the hybrid bin directory at `$PGV_WORK/tap/pgrust/bin`, containing a copy of `pg_ctl` and a `postgres` shim that execs pgrust.

- [ ] **Step 1: Write the failing test**

Insert above `# --- runner ---`:
```bash
test_tap_ref() {
  if "$here/../run-tap.sh" ref 015_hnsw_vector_duplicates.pl >/dev/null 2>&1; then
    pass "tap ref: 015_hnsw_vector_duplicates"
  else
    fail "tap ref: 015_hnsw_vector_duplicates (see $PGV_WORK/tap/ref/prove)"
  fi
  if assert_reference_clean; then pass "tap: reference trees untouched"; else fail "tap: reference trees modified"; fi
}
```
Run:
```bash
scripts/pgvector/tests/harness_test.sh test_tap_ref
```
Expected: FAIL. `run-tap.sh` doesn't exist yet.

- [ ] **Step 2: Write `run-tap.sh`**

Create `scripts/pgvector/run-tap.sh`:
```bash
#!/usr/bin/env bash
# Run pgvector's TAP tests (PostgreSQL::Test::Cluster) the way pgvector's
# `make prove_installcheck` does.
# usage: run-tap.sh {pgrust|ref} [NNN_name.pl ...]   (default: all of test/t)
#   ref:    pure C install ($PG_VEC).
#   pgrust: C initdb creates each cluster, pgrust runs it (hybrid bin dir).
set -euo pipefail
. "$(dirname "$0")/common.sh"

# The hybrid bin dir puts pgrust behind C's pg_ctl:
#  - initdb is NOT here, so PATH finds $PG_TOOLS/bin/initdb, which bootstraps
#    with the C postgres next to it (pgrust has no bootstrap mode).
#  - pg_ctl is a COPY, not a symlink: it looks for "postgres" in its own
#    resolved directory, and that must be the pgrust shim below. Its version
#    check needs `postgres -V` to print exactly "postgres (PostgreSQL) 18.6".
#  - The shim applies pgrust's runtime settings (README quickstart).
make_hybrid_bin() {
  local dir="$1" share
  share="$("$PG_TOOLS/bin/pg_config" --sharedir)"
  mkdir -p "$dir"
  cp "$PG_TOOLS/bin/pg_ctl" "$dir/pg_ctl"
  cat >"$dir/postgres" <<EOF
#!/bin/sh
# Generated by scripts/pgvector/run-tap.sh: run pgrust with its runtime settings.
ulimit -s 65520 2>/dev/null
export PGRUST_PGSHAREDIR='$share'
export PGRUST_TZDIR='$share/timezone'
export RUST_MIN_STACK=33554432
exec '$PGRUST_BIN' "\$@" -c io_method=sync -c max_stack_depth=60000
EOF
  chmod +x "$dir/postgres"
}

[ "$#" -ge 1 ] || die "usage: run-tap.sh {pgrust|ref} [NNN_name.pl ...]"
mode="$1"
shift
out="$PGV_WORK/tap/$mode"
rm -rf "$out"
mkdir -p "$out/prove" "$out/log" "$out/data"

case "$mode" in
  ref) bin_path="$PG_VEC/bin" ;;
  pgrust)
    [ -x "$PGRUST_BIN" ] || die "missing $PGRUST_BIN; run scripts/pgvector/build-pgrust.sh"
    make_hybrid_bin "$out/bin"
    bin_path="$out/bin:$PG_TOOLS/bin"
    ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac

if [ "$#" -gt 0 ]; then
  tests=()
  for t in "$@"; do tests+=("$PGV_SRC/test/t/$t"); done
else
  tests=("$PGV_SRC"/test/t/*.pl)
fi

: >"$out/summary.tsv"
for t in "${tests[@]}"; do
  [ -f "$t" ] || die "no such TAP test: $t"
  name="$(basename "$t" .pl)"
  if (
    # cwd is the work dir so stray relative writes never reach the reference tree.
    cd "$out"
    export PATH="$bin_path:$PATH"
    export PERL5LIB="$PGV_PERL5LIB${PERL5LIB:+:$PERL5LIB}"
    export TESTDIR="$out" TESTDATADIR="$out/data/$name" TESTLOGDIR="$out/log/$name"
    PG_REGRESS="$(pg_regress_bin)"
    export PG_REGRESS
    prove -v -I "$PG_SRC/src/test/perl" -I "$PGV_SRC/test/perl" "$t"
  ) >"$out/prove/$name.log" 2>&1; then
    printf '%s\tok\n' "$name" | tee -a "$out/summary.tsv"
  else
    printf '%s\tFAIL\n' "$name" | tee -a "$out/summary.tsv"
  fi
done

passed="$(grep -c $'\tok$' "$out/summary.tsv" || true)"
echo "tap ($mode): $passed/${#tests[@]} passed; logs $out/prove and $out/log"
[ "$passed" -eq "${#tests[@]}" ]
```

- [ ] **Step 3: Run the test until it passes**

```bash
chmod +x scripts/pgvector/run-tap.sh
scripts/pgvector/tests/harness_test.sh test_tap_ref
```
Expected: `ok   - tap ref: 015_hnsw_vector_duplicates`, `ok   - tap: reference trees untouched`, `all passed`.

If it fails, read `$PGV_WORK/tap/ref/prove/015_hnsw_vector_duplicates.log`. `Can't locate IPC/Run.pm` means `PERL5LIB` is wrong. `initdb: not found` means `PATH` is wrong. The C reference must pass, so fix the runner.

- [ ] **Step 4: Spike: run the same test against pgrust through the hybrid setup**

```bash
scripts/pgvector/run-tap.sh pgrust 015_hnsw_vector_duplicates.pl
```
Expected: `015_hnsw_vector_duplicates	ok` and `tap (pgrust): 1/1 passed`. This is M0's exit criterion.

If it fails, read `$PGV_WORK/tap/pgrust/prove/015_hnsw_vector_duplicates.log` and the server logs under `$PGV_WORK/tap/pgrust/log/015_hnsw_vector_duplicates/`, then classify the failure:
- **Harness mechanics.** Examples: `pg_ctl` says `program "postgres" is needed by pg_ctl but was not found` (version check or shim not executable), or the shim's argument order breaks a `pg_ctl` call. Fix `make_hybrid_bin`, rerun this step, and describe the fix in the commit message.
- **pgrust core behavior.** Examples: pgrust rejects an option that `pg_ctl` or `Cluster.pm` passes (such as `--cluster-name=node`), or rejects a `postgresql.conf` setting `Cluster.pm` writes. Stop and report to the human with the log excerpt. Don't patch pgrust in M0. The spec's §10 risk policy decides what happens next.

- [ ] **Step 5: Commit**

```bash
git add scripts/pgvector/run-tap.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: TAP runner with hybrid C-initdb/pgrust-server bin dir

Runs pgvector's TAP tests via prove. In pgrust mode, C initdb creates each
cluster and a copied pg_ctl starts pgrust through a shim carrying the
quickstart runtime settings. Spike: 015_hnsw_vector_duplicates passes on
both the C reference and pgrust.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```
If Step 4 failed for a core reason, replace the last sentence of the message with the actual outcome before committing.

---

### Task 6: Docker reference helper

**Files:**
- Create: `scripts/pgvector/docker-ref.sh`
- Modify: `scripts/pgvector/tests/harness_test.sh` (insert `test_docker` above `# --- runner ---`)

**Interfaces:**
- Consumes: `PGV_DOCKER_IMAGE`, `PGV_PORT_DOCKER`, `PG_TOOLS`, `die` (Task 2).
- Produces: `docker-ref.sh {up|down|psql} [psql args...]`.
  - `up` starts a container named `pgrust-pgvector-ref` from the pinned image, bound to `127.0.0.1:55493`, with trust auth and `--no-locale --encoding=UTF8`. It replaces any existing container of that name and waits for readiness.
  - `down` removes the container.
  - `psql` connects as `postgres`.
  
  M5 builds on this.

- [ ] **Step 1: Write the failing test**

Insert above `# --- runner ---`:
```bash
test_docker() {
  local ext coll
  if ! "$here/../docker-ref.sh" up >/dev/null 2>&1; then
    fail "docker: up"
    return
  fi
  pass "docker: up"
  ext="$("$here/../docker-ref.sh" psql -Atc 'create extension if not exists vector; select extversion from pg_extension where extname = $$vector$$' 2>&1 || true)"
  if [ "$ext" = 0.8.7 ]; then pass "docker: pgvector 0.8.7"; else fail "docker: extversion '$ext'"; fi
  coll="$("$here/../docker-ref.sh" psql -Atc "select datcollate from pg_database where datname = 'postgres'" 2>&1 || true)"
  if [ "$coll" = C ]; then pass "docker: C collation"; else fail "docker: datcollate '$coll'"; fi
  if "$here/../docker-ref.sh" down >/dev/null 2>&1; then pass "docker: down"; else fail "docker: down"; fi
}
```
Run:
```bash
scripts/pgvector/tests/harness_test.sh test_docker
```
Expected: `FAIL - docker: up`.

- [ ] **Step 2: Write `docker-ref.sh`**

Create `scripts/pgvector/docker-ref.sh`:
```bash
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
    docker rm -f "$name" >/dev/null 2>&1 || true
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
  down) docker rm -f "$name" >/dev/null ;;
  psql) "$PG_TOOLS/bin/psql" -X -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -p "$PGV_PORT_DOCKER" -U postgres -d postgres "$@" ;;
  *) die "unknown command '$cmd'" ;;
esac
```

- [ ] **Step 3: Run the test until it passes**

```bash
chmod +x scripts/pgvector/docker-ref.sh
scripts/pgvector/tests/harness_test.sh test_docker
```
Expected: four `ok` lines and `all passed`. The first run pulls the image.

- [ ] **Step 4: Commit**

```bash
git add scripts/pgvector/docker-ref.sh scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: Docker pgvector reference helper

Digest-pinned pgvector/pgvector:0.8.7-pg18 on 127.0.0.1:55493 with a
C-locale cluster, for the on-disk tier (M5).

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: Run-all wrapper, full reference run, pgrust baseline, docs

**Files:**
- Create: `scripts/pgvector/run-all.sh`
- Create: `docs/superpowers/reports/pgvector-m0-baseline.md`
- Modify: `CLAUDE.md` (the "Vector search (fork work)" section)

**Interfaces:**
- Consumes: `run-regress.sh` and `run-tap.sh` with their `summary.tsv` files (Tasks 4–5).
- Produces: `run-all.sh {pgrust|ref}`, which runs both tiers and writes `$PGV_WORK/report-MODE.md` (a markdown table plus a `**P/T passed**` line). Exits 0 only if every test passed. Later milestones add tiers to it.

- [ ] **Step 1: Write `run-all.sh`**

Create `scripts/pgvector/run-all.sh`:
```bash
#!/usr/bin/env bash
# Run every pgvector conformance tier built so far and print a pass/fail report.
# usage: run-all.sh {pgrust|ref}
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/common.sh"

[ "$#" -eq 1 ] || die "usage: run-all.sh {pgrust|ref}"
mode="$1"
port_for "$mode" >/dev/null

"$here/run-regress.sh" "$mode" || true
"$here/run-tap.sh" "$mode" || true

report="$PGV_WORK/report-$mode.md"
total=0
passed=0
{
  printf '# pgvector conformance: %s\n\n' "$mode"
  printf '| Tier | Test | Result |\n|---|---|---|\n'
  for tier in regress tap; do
    f="$PGV_WORK/$tier/$mode/summary.tsv"
    if [ ! -s "$f" ]; then
      printf '| %s | (no results) | FAIL |\n' "$tier"
      total=$((total + 1))
      continue
    fi
    while IFS=$'\t' read -r name result; do
      total=$((total + 1))
      [ "$result" = ok ] && passed=$((passed + 1))
      printf '| %s | %s | %s |\n' "$tier" "$name" "$result"
    done <"$f"
  done
  printf '\n**%d/%d passed**\n' "$passed" "$total"
} >"$report"
cat "$report"
[ "$passed" -eq "$total" ]
```

- [ ] **Step 2: Run everything against the C reference (validates the whole harness)**

```bash
chmod +x scripts/pgvector/run-all.sh
scripts/pgvector/run-all.sh ref
```
Expected: `**62/62 passed**` (14 regression + 48 TAP), exit 0. The recall tests make this take a while.

If any TAP test fails on the C reference, read its log under `$PGV_WORK/tap/ref/` before going on. A harness problem gets fixed in `run-tap.sh`. A genuine upstream flake on macOS (it must also fail when rerun alone) gets recorded in the baseline report as `ref-flaky` with the error line, and the run moves on.

- [ ] **Step 3: Run the full harness self-test**

```bash
scripts/pgvector/tests/harness_test.sh
```
Expected: `all passed`, covering `test_docker`, `test_pgrust_binary`, `test_reference`, `test_regress_identity`, `test_regress_ref`, `test_servers` and `test_tap_ref`.

- [ ] **Step 4: Run the pgrust baseline**

```bash
scripts/pgvector/run-all.sh pgrust || true
```
Expected: a report with failures. That is the baseline, and nothing should be fixed now.

- [ ] **Step 5: Write the baseline report**

Create `docs/superpowers/reports/pgvector-m0-baseline.md`. Fill it with the actual output from Steps 2 and 4, using this layout:
```markdown
# pgvector conformance baseline (M0)

Date: <run date> · pgrust commit: <git rev-parse --short HEAD> · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (local build)

## Summary
- C reference: <P>/62 passed (<list ref-flaky tests, or "none">)
- pgrust today: <P>/62 passed (regress <p>/14, TAP <p>/48)

## pgrust results
<paste the table from $PGV_WORK/report-pgrust.md>

## Failure causes
One line per failing pgrust test, from regression.diffs (regress) or
prove/<name>.log (TAP), grouped by cause:
- **Not ported yet (halfvec / sparsevec / bit / IVFFlat):** <tests>
- **0.8.5 → 0.8.7 behavior changes:** <tests with the differing line>
- **pgrust core gaps (not vector code):** <test: first error line>
- **Harness-mode exclusions (e.g. WAL tests needing pgrust replication):** <tests>
```
Every failing test must appear in exactly one group, with its first error line or first diff hunk line quoted. Get these from `$PGV_WORK/regress/pgrust/regression.diffs` and `$PGV_WORK/tap/pgrust/prove/*.log`.

- [ ] **Step 6: Document the harness in `CLAUDE.md`**

In `CLAUDE.md`, replace the last line of the "Vector search (fork work)" section, `Direction and roadmap: ...`, with:
~~~markdown
Direction and roadmap: `docs/Vector Search for pgrust Algorithm & Benchmark Survey.md`. Phase 1 spec: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`; plans in `docs/superpowers/plans/`.

The pristine upstream source is vendored at `crates/pgvector-0.8.7-reference/` (read-only, like the PG reference). Conformance harness: `scripts/pgvector/`. All state lives outside the repo in `~/.cache/pgrust/`.

```bash
scripts/pgvector/build-reference.sh      # once: C PG 18.6 + pgvector 0.8.7 under ~/.cache/pgrust/pgref-18.6
scripts/pgvector/build-pgrust.sh         # target/fast-profile/postgres (needs `brew install re2 pkg-config`)
scripts/pgvector/run-regress.sh pgrust [test ...]         # pgvector SQL suite; diffs in ~/.cache/pgrust/pgvector-work/regress/pgrust
scripts/pgvector/run-tap.sh pgrust [NNN_name.pl ...]      # TAP suite: C initdb + pgrust server via a hybrid bin dir
scripts/pgvector/run-all.sh {pgrust|ref}                  # every tier + report; `ref` must always be all-green
scripts/pgvector/tests/harness_test.sh                    # self-tests of the harness itself
```

Baseline: `docs/superpowers/reports/pgvector-m0-baseline.md`.
~~~

- [ ] **Step 7: Commit**

```bash
git add scripts/pgvector/run-all.sh docs/superpowers/reports/pgvector-m0-baseline.md CLAUDE.md
git commit -q -F - <<'EOF'
pgvector harness: run-all wrapper and M0 baseline

C reference passes the full pgvector suite through the harness; records
today's pgrust results with per-test failure causes as the starting point
for M1.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

## M0 exit criteria (spec §9)

- [ ] One pgvector TAP test passes end to end on pgrust through the hybrid setup (Task 5 Step 4), or the spike's failure is reported with its cause.
- [ ] `docs/superpowers/reports/pgvector-m0-baseline.md` is committed, and every failing test is assigned a cause.
- [ ] `scripts/pgvector/run-all.sh ref` is all green, apart from documented upstream flakes.
- [ ] `scripts/pgvector/tests/harness_test.sh` prints `all passed`.

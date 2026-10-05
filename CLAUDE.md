# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

pgrust is a Rust rewrite of PostgreSQL 18.6 that is wire- and SQL-compatible and is meant to match the C implementation's behavior byte for byte. The unmodified C source is vendored at `crates/postgres-18.6-reference/` as the behavior reference. It is **never compiled and never edited** (see its `README-WHY-THIS-IS-HERE.md`). When porting or fixing something, read the matching C file there first.

This checkout is a fork: `origin` = `roadius2/pgrust`, `upstream` = `malisper/pgrust` (upstream does not accept PRs). `main` is a single squashed commit, so older history and files live only on the `archive/*` and `v0.3-beta` branches.

**Missing from this checkout:** the README and many code comments point to `scripts/` (about 600 `scripts/*.sh`, including `scripts/pg-regress-fast.sh`), `docs/conformance/`, `docs/design/*.md`, `notes/`, `benchmarks/`, `Dockerfile`, `AGENTS.md` and CI config. None of them exist on `main`; some are on `origin/v0.3-beta`. Nothing in-repo runs `pg_regress` against pgrust. `proofs/TESTING-STRATEGY.md` describes how it used to be run. The `//!` module doc comments are the real design docs.

## Build and run

Prerequisites (macOS): `brew install re2 pkg-config postgresql@18`. Rust 1.96.0 is pinned by `rust-toolchain.toml`. pgrust has no `initdb`, so use PG18's `initdb`/`psql`.

```bash
cargo build --release --locked --bin postgres   # package `main_main`; binary at target/release/postgres
cargo build --bin postgres                      # dev build
```

- RE2 is enforced in `crates/backend/utils/adt/regexp_alt/build.rs`. Release-family profiles panic without libre2; dev builds only warn and fall back to the slow Spencer engine. `PGRUST_FORCE_NO_RE2=1` is a dev-only escape hatch.
- Profiles: `dev` (line-table debug info, no incremental), `release` (thin LTO), `dist` (fat LTO; published binaries), and `fast-profile`/`profiling`/`dist-prof` for perf work.
- `main_main/build.rs` copies every `crates/contrib/*/extension/` dir into `target/<profile>/share/`.

Run a server (from the README):

```bash
initdb -D /tmp/pgrust-data --no-locale --encoding UTF8 -U postgres
export PGRUST_PGSHAREDIR="$(brew --prefix postgresql@18)/share/postgresql"
export PGRUST_TZDIR="$PGRUST_PGSHAREDIR/timezone"
ulimit -s 65520
RUST_MIN_STACK=33554432 target/release/postgres -D /tmp/pgrust-data -k /tmp -p 5433 \
  -c listen_addresses= -c io_method=sync -c max_stack_depth=60000
psql -h /tmp -p 5433 -U postgres -c "select version()"   # expect "... (pgrust 0.3)"
```

Use a non-5432 port if a real Postgres is running, and check `version()` so you know you're talking to pgrust.

## Tests

- **Package names often differ from directory names** (about 96 crates). Check `name =` in the crate's `Cargo.toml` before using `-p`. For example, `crates/backend/utils/adt/like` is `adt_like`.
- Single test: `cargo test -p crc32c --test c_parity sb8_matches_c`.
- Common layouts: `#[cfg(test)] mod tests;` pointing at `src/tests.rs`; `tests/c_parity.rs` comparing against C-generated vectors baked into `tests/data/`; and `tests/differential.rs`.
- `tests/differential.rs` files shell out to `psql -h /tmp -p 5432` against a **real** Postgres, and they **print SKIP and pass** when none is reachable. A green run doesn't prove they ran.
- `crates/_support/test_boot` (`boot_wal`) boots WAL, shared memory and PGPROC for tests that need it. Elsewhere tests install their own seam stubs, and test_boot only installs a stub if the test hasn't already.
- Simulation-only tests need `RUSTFLAGS="--cfg pgrust_sim"`. Some `#[ignore]` tests must run alone with `--test-threads=1`.
- The repo's own lints are cargo tests that wrap shell scripts with `.allow` files. They pull in most of the workspace:
  - `cargo test -p seams_init --test lint_seam_installs`
  - `--test lint_determinism`: a budget for raw fs/time/rand/spawn/env calls that may only shrink
  - `--test lint_walker_arms`
  - `cargo test -p catalog_index --test lint_inplace_locks`

Separate workspaces, none of which are in the main workspace:
- `proofs/`: Kani proofs of Rust vs vendored C (cargo-kani 0.67). Read `proofs/README.md` and `proofs/TRIAGE.md`. Example: `cd proofs/utf8 && cargo kani -Z c-ffi --c-lib c/pg_wchar.c --c-lib c/pg_wchar_kernels.c --solver kissat --harness islegal_len1 --exact`. Suite: `cd proofs && ./run-suite.sh per-commit`.
- `fuzz/`: cargo-fuzz, needs nightly. In-process differential fuzzing of Rust vs vendored C. Example: `cargo +nightly fuzz run <target>`. The directory has about 390k files, so don't enumerate it.
- `crash-simulator/` (`simharness`): SQL property and crash testing against a live server. `warts.toml` lists accepted divergences.
- `wasm/`: `wasm/wasm-build.sh` (own nightly, `wasm32-wasip1`), then `node wasm/run-node.mjs --sql "SELECT 1"`. See `wasm/README.md`.

SQL-level differential testing against real Postgres uses `crates/bin/fuzzgen` (`diffrunner --a <C-PG host:port> --b <pgrust host:port>`). Accepted divergences go in `docs/fuzzing/rulings.toml`. `regress/overlay/` adds `-- pgrust:` annotations (mostly `rowsort`) to vendored regress SQL and never changes expected outputs.

## Architecture

**One crate per C file.** `crates/` mirrors Postgres' `src/` (`backend/`, `common/`, `port/`, `interfaces/`, `pl/`, `bin/`, plus `contrib/`), about 890 workspace members. Ported code follows these conventions:
- C function names are kept (`heap_insert`, `ThrowErrorData`).
- Comments cite `file.c` and line numbers.
- Intentional differences from C are marked `DIVERGENCE`.
- Errors carry the C source line and function, e.g. `bigint_out_of_range(471, "int8pl")`, because the error-source fields go out on the wire.

Keep these conventions when porting or changing code.

**Seams break dependency cycles.**
- A `foo_seams` crate holds only `seam_core::seam!` function-pointer slots. Callers depend on the seam crate and use `foo_seams::fn_name::call(..)`.
- The owner crate's `pub fn init_seams()` calls `::set(impl)` for each slot. Slots are set once; setting twice or calling an unset slot panics.
- `seams_init::init_all_with_transport` calls every `init_seams()` during single-threaded boot. This is also what links the whole server into `main_main` (`src/bin/postgres.rs`).
- A new seam must be set and its `init_seams` reachable from `seams_init`, or `lint_seam_installs` fails.

**Threads, not processes.**
- Each backend is a thread spawned by `postmaster_child_launch` (`crates/backend/postmaster/launch_backend`), with a synthetic pid. State that C inherits through `fork` is passed as an explicit snapshot.
- C's per-backend globals become const-initialized `thread_local!` values, mostly through the `scalar_global!` macro. Process-wide state uses atomics or `pgsync::process_global!`.
- `proc_exit` unwinds the thread instead of exiting.
- `crates/_support/pgsync` is the only lock library, with native, `cfg(loom)` and `cfg(pgrust_sim)` variants.
- `crates/_support/mcx` provides memory contexts as explicit allocator lifetimes; there is no ambient "current" context.

**Shared types** live in small, mostly `no_std` vocabulary crates under `crates/_support/types/`: `datum`, `fmgr`, `nodes`, `types_core`, `types_error` (`PgError`/`PgResult`), and others. Their purpose is to avoid cycles.

**Extensions have no C ABI.**
- Contrib modules are Rust crates that call `dfmgr::register_builtin_library` with a name→`PGFunction` lookup table.
- Their SQL install scripts live in `crates/contrib/<mod>/extension/`.
- Index AMs are a closed enum, `IndexAmKind` (`crates/backend/access/index/relscan/src/lib.rs`). Non-core AMs such as `hnsw` resolve through `registered_index_am`, and their `*handler` functions are never called through fmgr.

**Executor routing** happens in `execute_plan` (`crates/backend/executor/execmain/src/execmain.rs`):
- A SELECT on a `pgrcolumnar2` relation goes to the vectorized engine `sqe` (`crates/backend/executor/sqe`). There is **no fallback**: unsupported shapes return typed refusals from `execmain/src/sqeshell/refusal.rs`.
- `pgrust.sqe_heap` (default off) routes recognized heap shapes to sqe.
- Everything else uses the ported row-at-a-time executor (`procnode.rs`), which has a few batched "fused arms" for aggregation.
- `pgrcolumnar` (scan-only, mmap) is a separate, older AM that uses the row executor.

**Other pgrust-specific subsystems:**
- **JIT:** copy-and-patch AArch64, `cfg(target_arch = "aarch64")` only. Lives in `crates/backend/executor/execexpr/src/jit.rs` and `crates/_support/jit_deform`. It also runs on macOS arm64; x86_64 interprets. Kill switches: `PGRUST_JIT_QUAL=0` and `PGRUST_JIT_DEFORM=0`.
- **Morsel runtime and scheduler:** `crates/backend/executor/runtime`, covering priority decay and work-stealing parallel query.
- **README vs code:**
  - `crates/backend/postmaster/memwatchdog` only logs and dumps memory contexts; it does not kill a query.
  - Pipelined WAL flush (`transam_xlog/src/flushpipe.rs`) does not release locks early.

## Vector search (fork work)

There is an existing pgvector 0.8.5 port (upstream commit `159b79a`; pgvector is now at 0.8.7):
- `crates/contrib/pgvector`: the `vector` type, functions, aggregates and casts.
- `crates/contrib/pgvector_hnsw` and `pgvector_hnsw_build`: the HNSW AM, using pgvector's page layout and GenericXLog.
- `crates/_support/types/types_hnsw`.

`halfvec`, `sparsevec`, the `bit` opclasses and `ivfflat` are **not ported**, and the extension script is trimmed to match. The SQL test files in `crates/contrib/pgvector/sql/` have no expected outputs in the repo.

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

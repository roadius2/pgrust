# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

pgrust is a Rust rewrite of PostgreSQL 18.6 that is wire- and SQL-compatible and is meant to match the C implementation's behavior byte for byte. The unmodified C source is vendored at `crates/postgres-18.6-reference/` as the behavior reference. It is **never compiled and never edited** (see its `README-WHY-THIS-IS-HERE.md`). When porting or fixing something, read the matching C file there first.

This checkout is a fork: `origin` = `roadius2/pgrust`, `upstream` = `malisper/pgrust` (upstream does not accept PRs). Upstream's `main` is one squashed commit per release (v0.3, v0.3.1, v0.4-beta), so older history and files live only on the `archive/*` and `v0.3-beta` branches. Each release is merged into the fork on its own `merge/upstream-vX` branch, gated, reviewed, then fast-forwarded into `main`; never rebased. The fork is at v0.4-beta.

**Missing from this checkout:** many code comments point to files that don't exist: most `scripts/*.sh` (including `scripts/pg-regress-fast.sh`), most `docs/design/*.md`, all of `docs/gravity/`, `notes/`, `Dockerfile`, `AGENTS.md` and CI config. Upstream ships only a small `scripts/` (`pg-reference.sh`, `gravity-gate/`, a few others), `docs/conformance/tie-ordering.md`, two `docs/design/` files, `benchmarks/pgo/` and `third_party/` (two vendored crates patched into the build). Upstream runs no `pg_regress` against pgrust in-repo; the fork's `scripts/pgvector/` harness does, for the pgvector suite. The `//!` module doc comments are the real design docs.

## Build and run

Prerequisites (macOS): `brew install re2 pkg-config`. Rust 1.96.0 is pinned by `rust-toolchain.toml`. pgrust has its own `initdb` (`crates/bin/initdb`) and a partial `psql` port (`crates/bin/psql`); any PG18 `psql` works as the client, and a data directory made by C PG18's `initdb` still boots (the pgvector harness relies on that).

```bash
cargo build --release --locked -p main_main -p initdb   # target/release/postgres and target/release/initdb
cargo build --bin postgres                              # dev build of the server only
```

- RE2 is enforced in `crates/backend/utils/adt/regexp_alt/build.rs`. Release-family profiles panic without libre2; dev builds only warn and fall back to the slow Spencer engine. `PGRUST_FORCE_NO_RE2=1` is a dev-only escape hatch.
- Profiles: `dev` (line-table debug info, no incremental), `release` (thin LTO), `dist` (fat LTO; published binaries), `fast-profile`/`profiling`/`dist-prof` for perf work, plus `check-test` and `wasm-release`.
- `main_main/build.rs` stages `target/<profile>/share/`: initdb's inputs from `crates/bin/initdb/share` (`postgres.bki`, the setup SQL, `timezone`, `tsearch_data`), and every `crates/contrib/*/extension/` and `crates/pl/*/extension/` file into `share/extension/`. It removes staged files whose source is gone (fork), and skips two test modules unless `PGRUST_TEST_MODULES=1`.

Run a server (from the README):

```bash
target/release/initdb -D /tmp/pgrust-data --no-locale --encoding UTF8 -U postgres
ulimit -s 65520
RUST_MIN_STACK=33554432 target/release/postgres -D /tmp/pgrust-data -k /tmp -p 5433 \
  -c listen_addresses= -c io_method=sync -c max_stack_depth=60000
psql -h /tmp -p 5433 -U postgres -c "select version()"   # expect "PostgreSQL 18.6 (pgrust 0.4-beta, build <sha>) ..."
```

The server reads `share/` beside its binary; `PGRUST_PGSHAREDIR` and `PGRUST_TZDIR` are optional overrides (the pgvector harness sets them). Use a non-5432 port if a real Postgres is running, and check `version()` so you know you're talking to pgrust.

## Tests

- **Package names often differ from directory names** (about 140 crates). Check `name =` in the crate's `Cargo.toml` before using `-p`. For example, `crates/backend/utils/adt/like` is `adt_like`.
- Single test: `cargo test -p crc32c --test c_parity sb8_matches_c`.
- Common layouts: `#[cfg(test)] mod tests;` pointing at `src/tests.rs`; `tests/c_parity.rs` comparing against C-generated vectors baked into `tests/data/`; and `tests/differential.rs`.
- Most `tests/differential.rs` files shell out to `psql -h /tmp -p 5432` against a **real** Postgres, and they **print SKIP and pass** when none is reachable. A green run doesn't prove they ran. (The two under `crates/gravity/{expr,fn}` compare in-process and need no server.)
- `crates/_support/test_boot` (`boot_wal`) boots WAL, shared memory and PGPROC for tests that need it. Elsewhere tests install their own seam stubs, and test_boot only installs a stub if the test hasn't already.
- Simulation-only tests need `RUSTFLAGS="--cfg pgrust_sim"`. Some `#[ignore]` tests must run alone with `--test-threads=1`.
- The repo's own lints are cargo tests, most wrapping shell scripts with `.allow` files. They pull in most of the workspace:
  - `cargo test -p seams_init --test lint_seam_installs`
  - `--test lint_determinism`: a budget for raw fs/time/rand/spawn/env calls that may only shrink
  - `--test lint_alloc`: no new infallible std allocation sized at run time in `crates/{backend,pl,contrib}`; use `mcx::std_vec_*` or `try_reserve` with `mcx::oom_named`
  - `--test lint_walker_arms`
  - `cargo test -p catalog_index --test lint_inplace_locks`
  - `cargo test -p gravity_lint`: Gravity's rule suite
  - `cargo test -p session --lib tls_source_census`: every `thread_local!` under `crates/` has a row in `crates/backend/access/session/tls-census.tsv`. One a pool worker may read also needs a row in `crates/gravity/lint/THREAD-STATE/` and, for a GUC backing, in `worker_arm.rs`'s `Gucs` list.
- **Known upstream failures on macOS arm64** (they fail on upstream v0.4-beta's own tree too): 12 `gravity_hosts` tests in its buffer-pool and dictionary-cache family, among them `d4536_a_pooled_buffer_taken_under_a_scope_is_the_scopes_from_its_take`, and two `mcx` tests that need `--features std` (`d4029d_backstop_pick_allocates_nothing`, `d4092_dump_top_holders`). Use `--no-fail-fast`, or they hide the test targets behind them.

Separate workspaces, none of which are in the main workspace:
- `proofs/`: Kani proofs of Rust vs vendored C (cargo-kani 0.67). Read `proofs/README.md`. Example: `cd proofs/utf8 && cargo kani -Z c-ffi --c-lib c/pg_wchar.c --c-lib c/pg_wchar_kernels.c --solver kissat --harness islegal_len1 --exact`. Suite: `cd proofs && ./run-suite.sh per-commit`.
- `crash-simulator/` (`simharness`): SQL property and crash testing against a live server. `warts.toml` lists accepted divergences.
- `wasm/`: `wasm/wasm-build.sh` (own nightly, `wasm32-wasip1`), then `node wasm/run-node.mjs --sql "SELECT 1"`. See `wasm/README.md`.

SQL-level differential testing against real Postgres uses `crates/bin/fuzzgen` (`diffrunner --a <C-PG host:port> --b <pgrust host:port>`). Accepted divergences go in `docs/fuzzing/rulings.toml`. `regress/isolation-overlay/` holds isolation specs and expected outputs laid over the vendored isolation tree. (Upstream removed `fuzz/` and `regress/overlay/` in v0.4-beta.)

## Architecture

**One crate per C file.** `crates/` mirrors Postgres' `src/` (`backend/`, `common/`, `port/`, `interfaces/`, `pl/`, `bin/`, plus `contrib/`), about 940 workspace members. Two trees don't mirror C: `crates/gravity/` (the query engine) and `crates/tpde/` (the compiler backend it uses). Ported code follows these conventions:
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

**Gravity runs every statement** (`crates/gravity/`; since v0.4-beta the row executor, `sqe` and `pgrcolumnar2` are gone):
- `exec_simple_query` (`crates/backend/tcop/postgres/src/simple_query.rs`) hands a plan to `gravity_entry_seams::simple`; the extended protocol, cursors and SPI reach Gravity through the executor seams, which it owns.
- There is **no fallback**. A plan Gravity can't run fails with 0A000 `gravity: refused (<why>)`. One retry exists: a read-only SELECT of at most one row is replanned under other planner settings (`REPLAN_SETTINGS`).
- Gravity decides by properties, not code paths per AM. Its own index AM enum is `IndexAm` in `crates/gravity/decompose/src/catalog/mod.rs`, mapped from `IndexAmKind` by `am_of` in `crates/gravity/hosts/src/facts.rs`; `crates/gravity/hosts/src/ops/modifytable/write_index_am.rs` lists the AMs whose indexes it maintains. An AM missing from these is refused.
- `pgrcolumnar` is the only columnar AM (`crates/backend/access/pgrcolumnar/pgrc_*`), transactional and served by Gravity.

**Other pgrust-specific subsystems:**
- **JIT:** Gravity compiles its programs through TPDE (`crates/gravity/backend/tpde`, `crates/tpde/`) for AArch64 and x86-64, with an interpreter tier below it. The old copy-and-patch JIT left with the row executor.
- **Morsel runtime and scheduler:** `crates/backend/executor/runtime`, covering priority decay and work-stealing; Gravity drives it through `crates/gravity/pool`. Pool threads don't share a backend's thread-locals: see the `tls_source_census` lint above.
- **Memory budget:** `crates/backend/postmaster/memruntime` cancels the statement holding the most memory past `pgrust.memory_cancel_threshold` (default 90% of the limit); `pgrust.statement_mem_limit` caps one statement.
- **README vs code:** pipelined WAL flush (`transam_xlog/src/flushpipe.rs`) does not release locks early.

## Vector search (fork work)

There is an existing pgvector port (upstream 0.8.5 commit `159b79a`, brought to 0.8.7 behavior for `vector` and HNSW in M1, with `halfvec`, `sparsevec` and the `bit` functions added in M2 and their HNSW opclasses in M3, and IVFFlat (serial builds) in M4). Its extension directory is upstream 0.8.7's, verbatim. The crates:
- `crates/contrib/pgvector`: the `vector`, `halfvec` and `sparsevec` types and the `bit` distance functions: I/O, functions, aggregates, casts and btree opclasses, one module per C file.
- `crates/contrib/pgvector_f16_parity`: test-only. Its `build.rs` compiles pgvector's C f16 routines and checks every f16, every f32 and every pair of halves against the port; it is skipped on wasm32.
- `crates/contrib/pgvector_hnsw` and `pgvector_hnsw_build`: the HNSW AM for all four types, using pgvector's page layout and GenericXLog. An opclass's support proc 3 resolves by its C symbol to hnswutils.c's static type info.
- `crates/contrib/pgvector_hnsw_graph`: the in-memory HNSW build graph that serial and parallel builds share: C's lock protocol on `pgsync`, a model of C's allocator (so builds flush where C's do) and loom models in `tests/loom.rs`. It depends only on `pgsync` and `pairingheap`.
- `crates/contrib/pgvector_ivfflat` and `pgvector_ivfflat_build`: the IVFFlat AM for `vector`, `halfvec` and `bit`, using pgvector's page layout and GenericXLog. k-means runs serially (on the leader in a parallel build) over samples from ANALYZE's block sampler, which now lives in its own `sampling` crate. An opclass's support proc 5 resolves by its C symbol, as HNSW's proc 3 does.
- `crates/contrib/pgvector_parallel`: the parallel-build plumbing both AMs share (hnswbuild.c's and ivfbuild.c's begin, end and worker prologue over the `parallel` crate) and the kill switch.
- `crates/_support/types/types_hnsw`.
- `crates/_support/types/types_ivfflat`.

**Pre-M4 databases:** pgrust builds before M4 installed a trimmed `vector` 0.8.5, so `ALTER EXTENSION vector UPDATE` fails on their databases. To fix one, run `crates/contrib/pgvector/repair/pgrust-trimmed-0.8.5.sql` (generated; `PGVECTOR_REGEN_REPAIR=1` regenerates it) as a superuser in each affected database, then run `ALTER EXTENSION vector UPDATE`.

**Gravity (v0.4-beta merge):** upstream's engine refused both vector AMs, so on a plain merge no vector index could be searched or inserted into. The fork adds `Hnsw` and `Ivfflat` to Gravity's `IndexAm` (ordinary scans that serve `ORDER BY col <op> value`), maps them in `am_of`, and admits them in `write_index_am::insertable`; `index_opc` still counts them as an extension's AMs. IVFFlat's GUC thread-locals are registered for pool workers beside HNSW's (`tls-census.tsv`, `THREAD-STATE/`, `worker_arm.rs`). Recheck these sites after every upstream merge. In `relcache/src/initfile.rs` the fork's option tags count down from 255, because upstream numbers its own upward and both sides once chose 12.

**Accepted divergence (v0.4-beta):** upstream's planner builds no parallel plans any more (`planner/src/lib.rs`: `assess_parallel = false`), so pgvector's TAP `018_aggregates` fails 3 of its 17 checks on pgrust: EXPLAIN shows no `Partial Aggregate`, and `sum(halfvec)` is C's serial answer rather than its three-participant one. `scripts/pgvector/tap-accepted.tsv` pins exactly checks 5, 14 and 16 (user decision, 2026-11-05): the tier counts the test ok, `run-all.sh` lists it under "Accepted divergences", any other failure in that test fails the tier, and `ref` mode never accepts one. Add a row only on the user's decision.

**Parallel builds (M6)** use launched workers from the `parallel` crate (`pgvector_parallel`), not the morsel pool, and print C's `using N parallel workers`. `PGRUST_VECTOR_INDEXBUILD_POOL=0` in the server's environment makes every vector build serial. Since v0.4-beta upstream builds btree indexes in parallel too, through the same `table_index_build_scan_parallel`; `index_build` takes btree's worker count from upstream's `plan_create_index_workers` in `catalog_index` and the vector AMs' from the planner seam's port, two ports of one C function. HNSW participants insert into one shared graph (`pgvector_hnsw_graph`); its loom models: `RUSTFLAGS="--cfg loom" CARGO_TARGET_DIR=target/loom cargo test -p pgvector_hnsw_graph --test loom --release`. IVFFlat workers stream their rows to one leader sort, so the order within a list differs from C's. See `docs/superpowers/reports/pgvector-m6-report.md`.

**On-disk (M5):** a PG18 + pgvector data directory written by C in Docker (stopped cleanly, crashed mid-transaction, or made at 0.8.1 and then updated) boots in pgrust with C's answers and C's replayed pages (`run-ondisk.sh`, which needs Docker). (Tested on arm64 only; pgrust refuses `wal_compression = lz4|zstd` full-page images. See the Coverage limits in `docs/superpowers/reports/pgvector-m5-report.md`.) C booting a pgrust-written directory is reported, not gated.

**Recall (M7):** `run-recall.sh` loads the same GloVe 6B rows into stock C and the subject, builds each of 15 cases three times per side, the two IVFFlat `bit` cases nine times (`PGV_RECALL_BIT_BUILDS`; user decision, 2026-11-05), and compares the medians of recall@10 and @100. Pass rule: `pgrust recall ≥ pgvector recall − 0.01`. In `ref` mode the subject is the seeded C build. The dataset is cached in `~/.cache/pgrust/recall/`: `glove.6B.zip` (822 MB, a download, sha256 pinned in `recall-data.sh`) and one `prep-*` directory per size. `PGV_RECALL_*`, listed in the script's header, change the size; the M7 report's scale run is the example. Which participant flushes a spilling HNSW build isn't controllable, so the tier records it. A row can fail by chance, about 0.5% per run on the IVFFlat `bit` rows at 9 builds (6 to 7% at 3), so a failing row isn't by itself a bug: if root-cause work finds no bug, it goes to the user. See `docs/superpowers/reports/pgvector-m7-report.md`.

Direction and roadmap: `docs/Vector Search for pgrust Algorithm & Benchmark Survey.md`. Phase 1 spec: `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`; plans in `docs/superpowers/plans/`.

The pristine upstream source is vendored at `crates/pgvector-0.8.7-reference/` (read-only, like the PG reference). Conformance harness: `scripts/pgvector/`. All state lives outside the repo in `~/.cache/pgrust/`.

```bash
scripts/pgvector/build-reference.sh      # once: C PG 18.6 + pgvector 0.8.7 under ~/.cache/pgrust/pgref-18.6
scripts/pgvector/build-pgrust.sh         # target/fast-profile/postgres (needs `brew install re2 pkg-config`)
scripts/pgvector/run-regress.sh pgrust [test ...]         # pgvector SQL suite; diffs in ~/.cache/pgrust/pgvector-work/regress/pgrust
scripts/pgvector/run-tap.sh pgrust [NNN_name.pl ...]      # TAP suite: C initdb + pgrust server via a hybrid bin dir; tap-accepted.tsv pins accepted failures
scripts/pgvector/run-all.sh {pgrust|ref}                  # every tier + report; `ref` must always be all-green
scripts/pgvector/tests/harness_test.sh                    # self-tests of the harness itself
scripts/pgvector/server.sh {start|fresh|stop|psql} {pgrust|ref}  # manual servers on the harness ports
scripts/pgvector/docker-ref.sh {up|down|psql}                  # pgvector/pgvector:0.8.7-pg18 reference (port 55493)
scripts/pgvector/run-bytecmp.sh {pgrust|ref} [case ...]         # HNSW and IVFFlat pages vs the seeded C build (pgvec-seeded, -DHNSW_MEMORY -DIVFFLAT_BENCH); --list prints the cases
scripts/pgvector/run-iterscan.sh {pgrust|ref} [case ...]        # iterative-scan stop points vs the seeded C build; --list prints the cases
scripts/pgvector/run-diff.sh {pgrust|ref}                       # exact differential: diffrunner --pgvector + error LOCATIONs
scripts/pgvector/sqldiff.sh {pgrust|ref} [--verbose] <file|->   # one SQL script on fresh ref and subject servers, diffed
scripts/pgvector/run-repair-check.sh                            # the pre-M4 repair on all three trimmed 0.8.5 generations, a non-public schema, a 0.8.7 no-op and a rollback
scripts/pgvector/run-ondisk.sh capture [--force]                            # Docker: C pgvector writes the clean, crash and 0.8.1 directories and C's answers (cached by a key over ondisk-capture.sh, docker-lib.sh, the on-disk SQL, sql/repair-verify.sql, the image pins in common.sh and psql's version; changing any of them recaptures)
scripts/pgvector/run-ondisk.sh {pgrust|ref} [clean|crash|v081|reverse ...]  # on-disk forward gate (7 rows) and the reverse report
scripts/pgvector/run-buildtime.sh {pgrust|ref}                  # build time against C, serial and with 1, 2 and 4 workers (a report, not a gate)
scripts/pgvector/recall-data.sh <rows> <queries> <dim> <nnz>    # prepares GloVe rows and held-out queries under ~/.cache/pgrust/recall and prints the directory (run-recall.sh calls it)
scripts/pgvector/run-recall.sh {pgrust|ref} [case ...]          # recall@10 and @100 on real embeddings vs stock C (34 rows, the last tier of run-all.sh); --list prints the cases
```

Harness pgrust servers pin two C defaults that pgrust changes: `max_parallel_workers_per_gather=2` and (M6) `min_parallel_table_scan_size=8MB` (`common.sh`'s `PGRUST_SERVER_OPTS` and `run-tap.sh`'s shim).

Stock C pgvector never seeds HNSW or IVFFlat builds. `SET pgrust.hnsw_build_seed = 42` and `SET pgrust.ivfflat_build_seed = 42` (unregistered placeholder options) make pgrust match the seeded C build. `SET pgrust.hnsw_memory = on` (also unregistered) makes pgrust scans print the seeded C build's per-batch `memory: N KB, tuples: T` INFO line; `run-iterscan.sh` compares them. Baselines: `docs/superpowers/reports/pgvector-m0-baseline.md`, `docs/superpowers/reports/pgvector-m1-report.md`, `docs/superpowers/reports/pgvector-m2-report.md`, `docs/superpowers/reports/pgvector-m3-report.md`, `docs/superpowers/reports/pgvector-m4-report.md`, `docs/superpowers/reports/pgvector-m4-followups-report.md`, `docs/superpowers/reports/pgvector-m5-report.md`, `docs/superpowers/reports/pgvector-m5-followups-report.md`, `docs/superpowers/reports/pgvector-m6-report.md`, `docs/superpowers/reports/pgvector-m7-report.md`.

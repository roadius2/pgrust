# Borg benchmark, phase 1 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first like-for-like measurement of pgrust against C PostgreSQL 18.6 + pgvector 0.8.7 on Linux x86-64 — speed, memory, CPU, concurrency and a one-hour soak on glove-100 and sift-128 — produced by scripts in the repo, run in matched Incus containers on `borg2`, and written up as a report.

**Architecture:** Host-side bash on `borg2` (`scripts/bench/`) creates three Incus containers — `pgrust-db-c`, `pgrust-db-rust`, `pgrust-client` — pins them to disjoint physical cores, and orchestrates every measured step: it resets and reads the database container's cgroup counters around each step and drives the workload through `incus exec` into the client container, where Python (h5py, psycopg) loads the ann-benchmarks data and computes recall, and `pgbench` supplies concurrency. Results are TSV per contestant; `report.py` turns them into the report's tables. Scripts are shipped to the host with a tar pipe (`sync.sh`); nothing of the campaign lives only on the cluster.

**Tech Stack:** bash, Incus 6.0 (`incus launch/exec/file push/config device`), cgroup v2 (`memory.peak`, `cpu.stat`), PGDG apt (PostgreSQL 18.6, pgvector 0.8.7, psql, pgbench), Rust 1.96 (pgrust `main`), Python 3.12 venv (h5py, numpy, psycopg[binary]).

**Spec:** `docs/superpowers/specs/2026-11-14-borg-benchmark-design.md` (sections 3–5 and 8 bind every task below).

**Facts established before this plan (2026-11-14):** pgrust `main` at `69b9b626c4` builds on x86-64 Linux (noble, gcc 13.3, re2 10.0, Rust 1.96.0) in 6 m 34 s with `cargo build --release --locked -p main_main -p initdb`; its server starts under systemd with the harness's options and answers `PostgreSQL 18.6 (pgrust 0.4-beta, build 69b9b62) on x86_64-unknown-linux-gnu`; `CREATE EXTENSION vector` works. PGDG noble has `postgresql-18` 18.6 and 18.7 and `postgresql-18-pgvector` 0.8.7. The scratch container `pgrust-build` (16 cpu / 40 GiB) holds that build and is deleted in Task 3. Logical CPU *i* and *i*+8 are SMT siblings.

## Global Constraints

- Only `scripts/bench/**`, `docs/superpowers/reports/bench-borg-phase1-*.md`, `CLAUDE.md` (one pointer paragraph) and `docs/HANDOFF_2026-11-05.md` change in this repo. No product code changes; nothing under `crates/`.
- On the cluster, touch only instances named `pgrust-*`; never stop, alter or read other instances. Create instances with `--target borg2`, profile `trusted-lan`, image `images:ubuntu/noble/cloud`, `boot.autostart=false`. Database containers: `limits.cpu=0-5`, `limits.memory=24GiB`. Client: `limits.cpu=6,7,14,15`, `limits.memory=4GiB`. Disable `unattended-upgrades`, `apt-daily.timer` and `apt-daily-upgrade.timer` inside each container after cloud-init finishes.
- Incus gotchas: `incus launch` and `incus exec` read stdin when it is not a terminal — every incus command in a script gets `</dev/null`, and scripts run on the host as files (`ssh -n borg2 bash ~/bench/scripts/x.sh …`), never piped over `ssh … bash -s`. Jobs longer than a few minutes run inside a container under `systemd-run --unit=NAME …` and are read back with `journalctl -u NAME`.
- Server settings (spec §4), identical on both sides through one file, `scripts/bench/common-settings.conf`: `shared_buffers = '8GB'`, `maintenance_work_mem = '8GB'`, `work_mem = '64MB'`, `effective_cache_size = '16GB'`, `max_parallel_workers = 8`, `max_parallel_maintenance_workers = 4`, `max_parallel_workers_per_gather = 2`, `max_wal_size = '8GB'`, `checkpoint_timeout = '30min'`, `log_min_duration_statement = -1`, `listen_addresses = '*'`, `max_connections = 100`. pgrust additionally gets, on its command line only: `-c io_method=sync -c max_stack_depth=60000 -c max_parallel_workers_per_gather=2 -c min_parallel_table_scan_size=8MB`, `LimitSTACK=67092480` and `RUST_MIN_STACK=33554432` in its unit. Nothing else is tuned.
- Fairness (spec §8): one database container running at a time; `settle` (1-minute load < 1.0 on the host, checked for three consecutive 10 s readings) before every timed step; `metrics_reset` immediately before and `metrics_read` immediately after each measured step; identical SQL, client code and `pgbench` invocation for both contestants; recall computed by the client from the dataset's ground truth; one warm-up pass of the queries before any timed search.
- Repetitions: builds, exact-scan passes and loads 3 times (medians in the report, all three in the TSV); each search point (one `ef_search`/`probes` setting × one client count) once, 30 s for `pgbench` points; inserts once per client count; soak once.
- Evidence: `/home/jody/bench/results/<run-id>/` on `borg2`, pulled to `~/.cache/pgrust/bench-borg/<run-id>/` on the Mac. Datasets under `/home/jody/bench/data` on the host, bind-mounted read-only into the client at `/data`; their sha256 pinned in `scripts/bench/client/SHA256SUMS`.
- Timing numbers in the report come only from these runs (release pgrust binary, PGDG C binary), never from the Mac.

## Review Focus

1. **Peak-memory windows that include the previous step.** `memory.peak` must be reset only after the previous step's cleanup (`DROP INDEX`, `DELETE`, `VACUUM`) has returned and `metrics_read` has been taken; otherwise a build's peak absorbs the load's. Pinned in Task 1's `metrics_reset`/`metrics_read` test (a reset followed by an immediate read shows `peak ≈ current`) and Task 5's orchestrator order.
2. **`PGOPTIONS` not honoured by pgrust.** The search sweep sets `hnsw.ef_search`/`ivfflat.probes` per connection through `PGOPTIONS="-c name=value"`; if pgrust ignored startup-packet options every point would run at the default. Pinned in Task 4: `SHOW hnsw.ef_search` under `PGOPTIONS` on both engines must print the set value.
3. **A search that is not an index scan.** The `pgbench` query takes the vector through `\gset`; if either planner chose a sequential scan the QPS would be meaningless. Pinned in Task 4: `EXPLAIN (COSTS OFF)` of the exact query shape shows `Index Scan using … hnsw`/`ivfflat` on both engines, and the orchestrator checks it once per index.
4. **Dataset drift.** ann-benchmarks files are unversioned URLs. Pinned in Task 4: `datasets.sh` fails on a sha256 mismatch once `SHA256SUMS` holds a hash.
5. **Headline taken from the wrong point.** "QPS at recall ≥ 0.95" is the first sweep setting whose single-client recall pass reaches the target, with that setting's 8- and 16-client QPS; no interpolation. Pinned in Task 5's `report.py` test with a fixture TSV.

---

### Task 1: host-side tooling and the three containers

**Files:**
- Create: `scripts/bench/lib.sh`, `scripts/bench/borg.sh`, `scripts/bench/sync.sh`, `scripts/bench/common-settings.conf`, `scripts/bench/README.md`

**Interfaces:**
- Produces (`lib.sh`, sourced by every host script): `die MSG`; `X CONTAINER CMD…` = `incus exec CONTAINER -- "$@" </dev/null`; `XB CONTAINER 'bash -c string'`; `settle` (blocks until `/proc/loadavg`'s 1-minute value < 1.0 three times 10 s apart; prints the readings); `metrics_reset C` (`echo 1 > /sys/fs/cgroup/lxc.payload.C/memory.peak` via `sudo`); `metrics_read C` prints one line `peak=<bytes> current=<bytes> cpu_usec=<usage_usec>`; `now` = `date -u +%FT%TZ`; `row FILE FIELDS…` appends a tab-separated line.
- Produces (`borg.sh`): subcommands `create-db NAME`, `create-client`, `start NAME`, `stop NAME`, `stop-others NAME` (stops every running `pgrust-db-*` except NAME), `disable-upgrades NAME`, `wait-net NAME`, `push-dir NAME SRC DST` (`incus file push -r`), `facts OUTDIR` (writes `host.txt`: `uname -a`, `lscpu`, `incus version`, `cat /proc/loadavg`; `containers.txt`: `incus config show` of each `pgrust-*`; `zfs.txt`: `sudo zfs get compression,recordsize,used local/containers/<each>`), `delete NAME`.
- Produces (`sync.sh`): from the Mac, `tar czf - -C scripts bench | ssh borg2 'mkdir -p ~/bench && tar xzf - -C ~/bench'`; prints what shipped.
- Produces: `common-settings.conf` with exactly the Global Constraints' settings, one per line, appended verbatim to both servers' `postgresql.conf`.

- [ ] **Step 1: write `lib.sh`, `borg.sh`, `sync.sh`, `common-settings.conf`** with the interfaces above. `create-db` runs `incus launch images:ubuntu/noble/cloud NAME -p trusted-lan --target borg2 -c limits.cpu=0-5 -c limits.memory=24GiB -c boot.autostart=false </dev/null`, then `wait-net`, `cloud-init status --wait`, `disable-upgrades`. `create-client` the same with `limits.cpu=6,7,14,15 -c limits.memory=4GiB` and then `incus config device add pgrust-client data disk source=/home/jody/bench/data path=/data readonly=true shift=true` (create the host directory first).
- [ ] **Step 2: `bash -n` every script; `shellcheck` if installed** (no findings in new files, or each one justified in the report).
- [ ] **Step 3: ship and create.** `scripts/bench/sync.sh`, then on the host: `bash ~/bench/scripts/borg.sh create-db pgrust-db-c`, `… create-db pgrust-db-rust`, `… create-client`. Expected: `incus list pgrust-` shows the three RUNNING with LAN IPs; `incus config show pgrust-db-c | grep limits` shows `cpu: 0-5` and `memory: 24GiB`; `grep Cpus_allowed_list /proc/self/status` inside `pgrust-db-c` shows `0-5`.
- [ ] **Step 4: test the metrics primitives.** `metrics_reset pgrust-db-c; metrics_read pgrust-db-c` prints `peak` within 5% of `current`; then `XB pgrust-db-c 'python3 -c "x=bytearray(512*1024*1024); import time; time.sleep(1)"'` and a second `metrics_read` shows `peak` ≥ 512 MiB above the first (Review Focus 1). Record both lines in the task report.
- [ ] **Step 5: `borg.sh facts /home/jody/bench/results/facts-task1`** and check the three files exist and name all three containers.
- [ ] **Step 6: write `scripts/bench/README.md`** (20 lines: what the directory is, the container layout, `sync.sh`, the stdin gotcha, where results go).
- [ ] **Step 7: commit** `bench: host-side tooling for the borg campaign; three pinned containers on borg2`.

### Task 2: the C contestant

**Files:**
- Create: `scripts/bench/install-pg-c.sh`

**Interfaces:**
- Consumes: Task 1's `borg.sh push-dir`, `XB`, `common-settings.conf`.
- Produces: inside `pgrust-db-c`, PostgreSQL 18.6 listening on `*:5432`, database `bench`, role `bench` (superuser, password `bench`, scram), extension `vector` 0.8.7; service `postgresql@18-main`. Host script `install-pg-c.sh` is idempotent (re-running changes nothing).

- [ ] **Step 1: write `install-pg-c.sh`** (runs on the host; pushes itself and `common-settings.conf` into the container with `push-dir`, then runs the inside part with `XB`): PGDG repo (key to `/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc`, list `noble-pgdg main`); `apt-get install -y postgresql-18=18.6-1.pgdg24.04+2 postgresql-client-18=18.6-1.pgdg24.04+2 postgresql-18-pgvector` then `apt-mark hold postgresql-18 postgresql-client-18 postgresql-18-pgvector`; append `common-settings.conf` to `/etc/postgresql/18/main/postgresql.conf` under a `# bench` marker (once); `pg_hba.conf`: `host all bench 192.168.0.0/24 scram-sha-256` prepended; `systemctl restart postgresql@18-main`; as `postgres`: `CREATE ROLE bench SUPERUSER LOGIN PASSWORD 'bench'` (if absent), `CREATE DATABASE bench OWNER bench` (if absent), `CREATE EXTENSION IF NOT EXISTS vector` in `bench`. Prints `version()`, `extversion` and `SELECT name, setting FROM pg_settings WHERE source <> 'default' ORDER BY 1`.
- [ ] **Step 2: run it; expected output** contains `PostgreSQL 18.6 (Ubuntu 18.6-1.pgdg24.04+2)`, `0.8.7`, `shared_buffers | 1048576` (8GB in 8kB pages), `io_method | worker`.
- [ ] **Step 3: run it again** — exit 0, no changes (idempotence).
- [ ] **Step 4: from `pgrust-client`** (install `postgresql-client-18` there if Task 4 has not yet): `psql "postgresql://bench:bench@<pgrust-db-c ip>/bench" -c 'select 1'` returns 1.
- [ ] **Step 5: commit** `bench: install-pg-c.sh — PGDG PostgreSQL 18.6 + pgvector 0.8.7 in pgrust-db-c`.

### Task 3: the pgrust contestant

**Files:**
- Create: `scripts/bench/install-pgrust.sh`, `scripts/bench/pgrust.service`

**Interfaces:**
- Consumes: Task 1's tooling and `common-settings.conf`.
- Produces: inside `pgrust-db-rust`, pgrust (`/opt/pgrust/{postgres,initdb,share/}` built from the public fork's `main` at a recorded sha) running as user `pgrust` under `pgrust.service`, data in `/srv/pgrust/data`, listening on `*:5432`, database `bench`, role `bench` as in Task 2, extension `vector` 0.8.7; PGDG `postgresql-client-18` for `psql`. `/opt/pgrust/BUILD.txt` records the sha, `rustc --version`, the cargo command and the build's wall time.

- [ ] **Step 1: write `pgrust.service`**: `[Service] User=pgrust`, `LimitSTACK=67092480`, `Environment=RUST_MIN_STACK=33554432`, `ExecStart=/opt/pgrust/postgres -D /srv/pgrust/data -k /tmp -p 5432 -c io_method=sync -c max_stack_depth=60000 -c max_parallel_workers_per_gather=2 -c min_parallel_table_scan_size=8MB`, `KillSignal=SIGINT`, `TimeoutStopSec=120`, `Restart=no`; `[Install] WantedBy=multi-user.target`.
- [ ] **Step 2: write `install-pgrust.sh`** (host side, like Task 2's): inside the container: `apt-get install -y build-essential pkg-config libre2-dev git curl ca-certificates` + PGDG `postgresql-client-18`; rustup `-y -q --default-toolchain 1.96.0 --profile minimal`; `git clone --depth 1 --branch ${PGRUST_REF:-main} https://github.com/roadius2/pgrust /root/pgrust` (or `git fetch && reset` when present); `cargo build --release --locked -p main_main -p initdb` under `systemd-run --unit=pgrust-build --wait`; copy `postgres`, `initdb`, `share/` to `/opt/pgrust` (`chmod -R a+rX`); write `BUILD.txt`; `useradd pgrust`; `initdb -D /srv/pgrust/data --no-locale --encoding UTF8 -U postgres` as `pgrust`; append `common-settings.conf` to `/srv/pgrust/data/postgresql.conf` under `# bench`; `pg_hba.conf` as Task 2; install and `systemctl enable --now pgrust.service`; wait for `pg_isready -h /tmp -p 5432`; create role, database and extension as Task 2; print `version()`, `extversion`, the non-default `pg_settings`.
- [ ] **Step 3: run it on `pgrust-db-rust`; expected output** contains `(pgrust 0.4-beta, build <sha7>) on x86_64-unknown-linux-gnu`, `0.8.7`, `shared_buffers | 1048576`, `io_method | sync`. Record the wall time of the build step.
- [ ] **Step 4: idempotence** — run again: exit 0, server still up, no rebuild when the sha is unchanged.
- [ ] **Step 5: from `pgrust-client`:** `psql "postgresql://bench:bench@<pgrust-db-rust ip>/bench" -f` a file with: `create table t (id int, v vector(3), h halfvec(3)); insert … generate_series(1,1000)`; `create index on t using hnsw (v vector_cosine_ops)`; `create index on t using hnsw (h halfvec_l2_ops)`; `create index on t using ivfflat (v vector_l2_ops) with (lists = 10)`; `set enable_seqscan = off;` then one top-3 query per index. Expected: three result sets of three ids, no error.
- [ ] **Step 6: retire the scratch container.** Read `journalctl -u pgrust-unit-tests -o cat` in `pgrust-build` and copy its `test result:` lines into the task report (they are the x86-64 unit-test evidence for the portable f16 route), then `borg.sh delete pgrust-build`.
- [ ] **Step 7: commit** `bench: install-pgrust.sh and pgrust.service — pgrust main built and served in pgrust-db-rust`.

### Task 4: the client — datasets, loader, recall, pgbench scripts

**Files:**
- Create: `scripts/bench/client/setup.sh`, `scripts/bench/client/datasets.sh`, `scripts/bench/client/SHA256SUMS`, `scripts/bench/client/load.py`, `scripts/bench/client/recall.py`, `scripts/bench/client/search.pgbench`, `scripts/bench/client/insert.pgbench`, `scripts/bench/client/soak-search.pgbench`, `scripts/bench/client/soak-insert.pgbench`, `scripts/bench/client/soak_stats.py`

**Interfaces:**
- Consumes: `/data/<name>.hdf5` (bind mount from Task 1); a DSN `postgresql://bench:bench@HOST/bench`.
- Produces (all under `/opt/bench/` in `pgrust-client`, venv at `/opt/bench/venv`):
  - `datasets.sh NAME…` downloads `http://ann-benchmarks.com/NAME.hdf5` to `/data` (host path `/home/jody/bench/data`, so run via the host with the directory writable: the script is run on the host, not in the client) and verifies against `SHA256SUMS`; on first fetch with no entry it appends the hash and prints `PINNED`.
  - `load.py DSN HDF5 --dim D --metric {cosine,l2} [--halfvec] [--subset N]`: creates `items (id int, v vector(D))`, with `--halfvec` also `items_h (id int, v halfvec(D))`, `queries (id int, v vector(D), vh halfvec(D))` from `test`, `truth (qid int, nn int[])` from `neighbors` (100 ids), `extra (id int, v vector(D))` = the first 100,000 `train` rows with `id + 100000000`; loads with `COPY … FROM STDIN` in text format in batches of 10,000 rows; prints one JSON line `{"rows": N, "seconds": S, "rows_per_s": R}` for `items`.
  - `recall.py DSN --table items|items_h --col v --op '<=>'|'<->' [--exact] [--queries N] [--k 10] [--guc hnsw.ef_search=100]`: sets the GUC with `SET`, runs a warm-up pass (N queries), then times each of the N queries (`SELECT id FROM TABLE ORDER BY COL OP %s LIMIT k`), computes recall@k against `truth.nn[:k]`, prints one JSON line `{"recall": r, "n": N, "qps": q, "p50_ms": …, "p95_ms": …, "p99_ms": …, "seconds": s}`. `--exact` sets `enable_indexscan = off` (and is used before any index exists).
  - `search.pgbench`: `\set qid random(0, 9999)` / `SELECT v AS qv FROM queries WHERE id = :qid \gset` / `SELECT id FROM :tbl ORDER BY v :op :'qv' LIMIT 10;` — `tbl` and `op` come from `pgbench -D tbl=items -D op='<=>'`; the per-connection GUC comes from `PGOPTIONS="-c hnsw.ef_search=N"` (or `ivfflat.probes`).
  - `insert.pgbench`: `\set id random(1, 100000)` / `INSERT INTO :tbl (id, v) SELECT id + 100000000, v FROM extra WHERE id = :id;`.
  - `soak-search.pgbench` = `search.pgbench`; `soak-insert.pgbench` = `insert.pgbench`; run as `pgbench -f soak-search.pgbench@9 -f soak-insert.pgbench@1`.
  - `soak_stats.py LOGFILE…` reads `pgbench -l` logs and prints, per 300 s window, `window_start tps p50_ms p99_ms errors`.

- [ ] **Step 1: write `setup.sh`** (runs inside the client via `XB`): PGDG repo + `postgresql-client-18` (psql, pgbench); `python3-venv`; `python3 -m venv /opt/bench/venv && /opt/bench/venv/bin/pip install -q h5py numpy 'psycopg[binary]'`. Run it; expected `pgbench --version` prints 18.x and `python -c 'import h5py, psycopg'` is silent.
- [ ] **Step 2: write `datasets.sh`; fetch** `glove-100-angular` and `sift-128-euclidean` into `/home/jody/bench/data`; expected two `PINNED` lines and a `SHA256SUMS` with two entries; commit that file. Then `h5ls`-equivalent check from the client venv: `train` shapes `(1183514, 100)` and `(1000000, 128)`, `test` `(10000, …)`, `neighbors` `(10000, 100)`.
- [ ] **Step 3: write `load.py`; test** on `pgrust-db-c` with `--subset 100000 --halfvec` on glove: expected `items` 100000 rows, `items_h` 100000, `queries` 10000, `truth` 10000, `extra` 100000; the JSON line printed. Note: with `--subset`, `truth` is still the full dataset's neighbours, so recall is only meaningful without `--subset`; the subset is for pipeline tests.
- [ ] **Step 4: write `recall.py`; test exactness** on the subset: `--exact --queries 200` on both engines prints `recall` of exactly 1.0 against a truth table recomputed by the script from the subset (`--exact` mode computes its own truth with a second exact pass when `--check-exact` is passed) — or, simpler and sufficient: on the **full** glove load of Task 5's dry run, `--exact --queries 200` must print `recall: 1.0`. Pick the full-load check and say so.
- [ ] **Step 5: Review Focus 2 and 3.** On both engines: `PGOPTIONS="-c hnsw.ef_search=77" psql DSN -Atc 'show hnsw.ef_search'` prints `77`; `psql DSN -c "EXPLAIN (COSTS OFF) SELECT id FROM items ORDER BY v <=> '[…100 zeros…]' LIMIT 10"` after a small HNSW build on the subset shows an index scan on both (`Index Scan using items_v_idx` on C; pgrust's plan text recorded as whatever it prints, as long as it names the hnsw index). Record both outputs in the task report.
- [ ] **Step 6: write the four `.pgbench` files and `soak_stats.py`;** run `pgbench -n -c 2 -j 2 -T 5 -D tbl=items -D op='<=>' -f search.pgbench DSN` against both engines (subset, index present): expected `number of failed transactions: 0` on both. Run `pgbench -n -c 2 -t 100 -D tbl=items -f insert.pgbench DSN` then `DELETE FROM items WHERE id >= 100000000` on both: 0 failures. `soak_stats.py` on a 5 s `-l` log prints one window line.
- [ ] **Step 7: commit** `bench: client tooling — ann-benchmarks loader, recall, pgbench scripts, pinned datasets`.

### Task 5: the orchestrator and the report generator; dry run

**Files:**
- Create: `scripts/bench/run-phase1.sh`, `scripts/bench/report.py`, `scripts/bench/tests/fixture.tsv`, `scripts/bench/tests/test_report.sh`

**Interfaces:**
- Consumes: everything above.
- Produces: `run-phase1.sh CONTESTANT RUN_ID [--subset N] [--skip-soak]` (host side; `CONTESTANT` ∈ `pg-c`, `pgrust`) writing `/home/jody/bench/results/RUN_ID/CONTESTANT.tsv` with the header `dataset type index measurement setting clients rep value unit recall p50_ms p95_ms p99_ms peak_bytes cpu_usec wall_s started_at`, plus `CONTESTANT-version.txt`, `CONTESTANT-settings.txt` (non-default `pg_settings`), `CONTESTANT-explain-*.txt`, `CONTESTANT-build-*.log`, `CONTESTANT-pgbench-*.log`, `CONTESTANT-soak.log`, `CONTESTANT-soak-rss.tsv`, and `facts/` from `borg.sh facts`. `report.py RESULTS_DIR` prints markdown.

Measurement sequence per contestant (the script's shape; `D` = each dataset in order glove-100-angular then sift-128-euclidean):
1. `stop-others`, `start` the contestant, wait for `pg_isready`, record version and settings; `settle`.
2. For each `D`: `DROP DATABASE IF EXISTS bench; CREATE DATABASE bench; CREATE EXTENSION vector` (as `postgres` via `XB`); `settle`; **load** (`load.py`, glove with `--halfvec`) → rows `measurement=load`; then `VACUUM ANALYZE`.
3. **exact**: `recall.py --exact --queries 1000` ×3 (rows `measurement=exact clients=1`, value = qps), then `pgbench -c 8 -j 8 -T 30` with `search.pgbench` and `enable_indexscan` irrelevant (no index yet) → `measurement=exact clients=8` value = tps. On glove also on `items_h`.
4. **HNSW builds**: `SET max_parallel_maintenance_workers = 0; SET maintenance_work_mem = '8GB'; CREATE INDEX items_v_idx ON items USING hnsw (v <opclass>) WITH (m = 16, ef_construction = 64)` timed by `\timing` ×3 with `DROP INDEX` between → `measurement=build setting=serial`, value = ms, plus `pg_relation_size` as a second row `measurement=index_size`; then ×3 with `max_parallel_maintenance_workers = 4` (`setting=parallel4`), keeping the last index. `metrics_reset` before each build, `metrics_read` after; `settle` before each.
5. **HNSW search**: for `ef` in 40 64 100 200 400: `recall.py --queries 10000 --guc hnsw.ef_search=ef` → row `measurement=search setting=ef clients=1` (value = qps, recall, latencies); `PGOPTIONS="-c hnsw.ef_search=ef" pgbench -n -c 8 -j 8 -T 30 …` → `clients=8`; `-c 16 -j 16` → `clients=16`. Before the sweep, one `EXPLAIN` saved to `CONTESTANT-explain-D-hnsw.txt` and checked for the index's name.
6. **inserts**: `pgbench -n -c 1 -t 100000 -f insert.pgbench` → `measurement=insert clients=1` (value = rows/s); `DELETE … >= 100000000; VACUUM items`; `settle`; `-c 8 -t 12500` → `clients=8`; delete and vacuum again.
7. On glove, repeat 4–6 for `items_h` with `halfvec_cosine_ops` (`type=halfvec`).
8. **IVFFlat** on `items`: `DROP INDEX items_v_idx`; builds ×3 serial and ×3 parallel with `lists = 1183` (glove) / `1000` (sift); search sweep `probes` in 10 20 40 80 as in step 5 with `ivfflat.probes`; no inserts.
9. After both datasets, **plain SQL** in a fresh `bench`: `pgbench -i -s 100` timed → `measurement=pgbench_init`; `pgbench -S -c 8 -j 8 -T 60` → `measurement=pgbench_select`; `pgbench -c 8 -j 8 -T 60` → `measurement=pgbench_tpcb`.
10. **soak** (unless `--skip-soak`): fresh glove load, HNSW `items_v_idx` with 4 workers; `PGOPTIONS="-c hnsw.ef_search=100" pgbench -n -c 8 -j 8 -T 3600 -P 300 -l --sampling-rate 0.1 -f soak-search.pgbench@9 -f soak-insert.pgbench@1 -D tbl=items -D op='<=>'` under `systemd-run` in the client; meanwhile the host appends `now memory.current` to `CONTESTANT-soak-rss.tsv` every 10 s and runs `VACUUM items` every 600 s; afterwards `soak_stats.py` over the logs → rows `measurement=soak setting=window-N clients=8` (value = tps, p50, p99) and a row `measurement=soak_errors`.
11. `borg.sh facts`, `stop` the contestant.

- [ ] **Step 1: write `run-phase1.sh`** to the sequence above, with `--subset N` passed to `load.py` and shrinking `--queries` to 200 and `-T` to 5 for the dry run, and `--skip-soak`.
- [ ] **Step 2: write `report.py`**: reads every `*.tsv` in the directory; prints (a) a headline table — per dataset/type: HNSW serial build median (C ms, pgrust ms, pgrust÷C), parallel build, QPS at recall ≥ 0.95 and ≥ 0.99 at 8 clients (Review Focus 5 rule), exact-scan QPS, insert rows/s at 8 clients, peak RSS of the serial build, CPU-seconds of the serial build; (b) one table per measurement with all points; (c) the soak windows. Ratios as `x.xx×`.
- [ ] **Step 3: `tests/fixture.tsv` + `tests/test_report.sh`**: a 12-row fixture (two contestants, one dataset, `search` rows at ef 40 (recall 0.93), 64 (0.96), 100 (0.99) with distinct QPS) and the expected headline cells; the test runs `report.py` on the fixture and `grep`s the expected numbers — including that the ≥ 0.95 headline is the ef-64 QPS and the ≥ 0.99 one is ef-100's.
- [ ] **Step 4: dry run** both contestants: `run-phase1.sh pg-c dry1 --subset 100000 --skip-soak` and `run-phase1.sh pgrust dry1 --subset 100000 --skip-soak` (~15 min each). Expected: every measurement present in both TSVs (count rows per `measurement`), no `ERROR` in the logs, `report.py dry1` renders every table. Fix the orchestrator until it does; the dry run's numbers are not results.
- [ ] **Step 5: launch the full runs**, run-id `p1-<YYYYMMDD>`, on the host under `systemd-run --unit=bench-pg-c -p WorkingDirectory=/home/jody/bench --uid=jody … run-phase1.sh pg-c p1-… ` and, **after it finishes**, the same for `pgrust` (the two must not overlap). Record the start time in the ledger. Expected duration: 2–4 h each (the soak is 1 h of it).
- [ ] **Step 6: commit** `bench: run-phase1.sh orchestrator and report.py; dry run of both contestants`.

### Task 6: results, report, pointers

**Files:**
- Create: `docs/superpowers/reports/bench-borg-phase1-<date>.md`
- Modify: `CLAUDE.md` (the Speed paragraph gains two sentences and the pointer), `docs/HANDOFF_2026-11-05.md` §1 (what landed, next phase)

- [ ] **Step 1: pull** `/home/jody/bench/results/p1-*/` to `~/.cache/pgrust/bench-borg/p1-*/` (`scp -r`); run `report.py` on it; check both TSVs are complete (same `measurement` counts as the dry run, plus soak rows) and the soak had 0 errors.
- [ ] **Step 2: write the report** in the speed report's style: header (date, host, container limits, binaries and shas, versions), the headline table, then one section per measurement with the table and two sentences of reading, then **"Where pgrust is behind"**, **"Resources"** (peak RSS and CPU-seconds, both sides, with the caveat that the ZFS ARC is outside the cgroup for both), **"Stability"** (soak windows, RSS over the hour), **"Limits of this measurement"** (one node, one run per search point, cache warm, SMT siblings idle, PGDG C build vs source pgrust, 18.6 vs available 18.7), **"Reproduce"** (the five commands). Every number traces to a TSV row.
- [ ] **Step 3: CLAUDE.md and the handoff**: CLAUDE.md Speed paragraph: one sentence with the Linux headline ratios and the report path; handoff §1: a dated paragraph (branch, commits, report, what the containers' state is, phase 2 next).
- [ ] **Step 4: commit** `docs(bench): phase 1 report — pgrust vs PostgreSQL 18.6 + pgvector 0.8.7 on borg2`.

## Self-review notes

- Spec coverage: §3 environment → Task 1; §4 contestants `pg-c`/`pgrust` and settings → Tasks 2–3 and `common-settings.conf`; §5 datasets (phase 1 rows) → Task 4; measurements 1–7 → Task 5's sequence (1 load, 2 builds, 3 search, 4 exact, 5 inserts, 6 plain SQL, 7 soak); §7 deliverables → Tasks 1, 5, 6; §8 fairness → Global Constraints and the orchestrator's `settle`/metrics order. Measurement 8 and phases 2–5 are later plans by design.
- Types and names used across tasks: `X`/`XB`/`settle`/`metrics_reset`/`metrics_read`/`row` (Task 1) used by Tasks 2, 3, 5; DSN form and role `bench` (Tasks 2–3) used by Tasks 4–5; `items`, `items_h`, `queries`, `truth`, `extra`, index name `items_v_idx` (Task 4) used by Task 5's SQL and `report.py`; the TSV header (Task 5) is what `report.py` and the fixture use.
- Time: Task 3's build ≈ 7 min; Task 4's downloads ≈ 1 GB; Task 5's dry runs ≈ 30 min; the full runs 4–8 h unattended, polled.
- Known soft spots left to the implementer: the exact text pgrust prints in `EXPLAIN` for an HNSW scan (recorded, not asserted beyond naming the index); whether `pgbench -i` succeeds on pgrust (if it refuses a statement, record it as a result, not a blocker); IVFFlat `lists` for sift is 1000 by the `rows/1000` rule.

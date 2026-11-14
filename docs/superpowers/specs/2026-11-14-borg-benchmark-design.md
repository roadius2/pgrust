# pgrust against PostgreSQL + pgvector on Linux x86-64: the borg benchmark campaign — Design

_2026-11-14. The user asked for this on 2026-11-14 ("benchmark for speed and also report on scale abilities for both, also if there is memory or cpu savings … illustrate benefits against C Postgres and pgvector, along with stability proofs; also illustrate if we are ahead of the pgvector Rust releases out there; we could make a Mosaic page after") and said "skip the questions, pick a first benchmark, borg cluster is probably good". Every decision below is therefore Claude's call, made from the repo, the user's cluster and the published state of the alternatives, and marked **(call)**; the user can overrule any of them._

## 1. Intent

Produce numbers a stranger can trust and reproduce that answer: how does pgrust (the Rust rewrite of PostgreSQL 18.6 with its own port of pgvector 0.8.7) compare with stock C PostgreSQL + pgvector on the same Linux x86-64 hardware — in speed, at scale, in memory and CPU, and in stability — and where does it stand against the Rust vector extensions people can install on C Postgres today. The written record is a report in this repo; a Mosaic page is made from it afterwards.

Why now: every speed number so far (`docs/superpowers/reports/pgvector-speed-2026-11-08.md`) is from one macOS arm64 laptop. Nothing has ever been measured on Linux or x86-64 — the platform anyone would actually run this on — and nothing has measured memory, CPU, concurrency or long runs.

## 2. Goals, non-goals, anti-goals

**Goals**
- Like-for-like, reproducible comparisons: same host, same container limits, same server settings where the knob exists, same data, same queries, same recall target. Every number in the report comes from a script in `scripts/bench/` and a raw file in the evidence directory.
- Four axes for each contestant: **speed** (index build, search throughput and latency, exact scan, inserts, plain SQL), **scale** (dataset sizes and concurrency up to where something breaks or degrades, including a memory-constrained run), **resources** (peak RSS and CPU-seconds for each measurement), **stability** (the conformance gate suite on Linux, and a soak run with no errors and no memory growth).
- Rust-ecosystem reference points: VectorChord and pgvectorscale on C Postgres, each at its documented recommended settings, compared at matched recall.
- Honest losses: a "where pgrust is behind" section is mandatory, and known gaps (builds at 1.5–1.6× C on the Mac; IVFFlat `halfvec` at 1.5× its own `vector`) are expected to reappear.

**Non-goals**
- Tuning either side beyond documented, recommended settings; cloud-cost or price-performance comparisons; GPU, distributed or replicated setups; anything on arm64 (the Mac report covers it).

**Anti-goals** (failure even if the numbers look good)
- A number that cannot be re-derived from a script and a raw file; a search comparison at mismatched recall; a single best-of run presented as typical; a competitor measured with settings its own docs would call wrong.

## 3. The environment (settled)

- **Host: `borg2`** (one of the user's three identical Incus-cluster nodes: i9-11900, 8 cores / 16 threads with AVX-512 and F16C, 61 GiB, Ubuntu 25.04, kernel 6.14, ZFS pool `local` on a 2 TB NVMe). Chosen because it is the emptiest node **(call)**. Cluster conventions are in `~/dev_projects/atlas/docs/superpowers/specs/2026-08-23-borg-incus-cluster-design.md`.
- **Incus system containers, not Docker** **(call)**: the cluster runs Incus 6.0.6 without OCI support, and system containers give exact CPU pinning and cgroup metrics from the host. Image `images:ubuntu/noble/cloud` (24.04 LTS, the release PGDG packages target), profile `trusted-lan` (LAN DHCP address), on ZFS pool `local`. `unattended-upgrades` and the `apt-daily` timers are disabled inside benchmark containers for the campaign's duration (a deviation from the cluster's upkeep convention; the containers are temporary).
- **CPU pinning** **(call)**: logical CPU *i* and *i*+8 are SMT siblings. Every database container gets `limits.cpu=0-5` (six whole cores, no sibling sharing); the load-generator container gets `limits.cpu=6,7,14,15` (two whole cores with both threads). Database containers get `limits.memory=24GiB`; the client 4 GiB. Only one database container runs at a time; the others are stopped.
- **Metrics from the host**, not from inside: `/sys/fs/cgroup/lxc.payload.<container>/memory.peak` (reset by writing to it before each measurement), `memory.current`, and `cpu.stat`'s `usage_usec` delta. This counts the whole server (postmaster, backends, workers, checkpointer), which is the number a user pays for.
- **Storage**: each container's root on the ZFS pool, identical dataset properties (`compression`, `recordsize` recorded). `fsync=on` on both sides.

## 4. Contestants

| id | What | How it is installed | Settled by |
|---|---|---|---|
| `pg-c` | PostgreSQL **18.6** + pgvector **0.8.7** | PGDG apt for noble, pinned `postgresql-18=18.6-*` (PGDG also has 18.7; 18.6 is what pgrust ports, so it is the fair baseline — noted in the report) and `postgresql-18-pgvector` (PGDG's current build is `0.8.7-1.pgdg24.04+2`, checked 2026-11-14, so no source build is needed; the installed version is recorded) | **(call)** |
| `pgrust` | pgrust `main` at a recorded sha | `git clone` of the public fork into the container, `cargo build --release --locked -p main_main -p initdb` (the `release` profile is what every existing timing used; a `dist` fat-LTO build is an optional extra, measured only if time allows and reported separately) | **(call)** |
| `pg-vchord` (phase 2) | `pg-c` + VectorChord 1.1.1 (`vchordrq`, IVF + RaBitQ) | the release asset `postgresql-18-vchord_1.1.1-1_amd64.deb` (exists, checked 2026-11-14) | **(call)** |
| `pg-vscale` (phase 2) | `pg-c` + pgvectorscale 0.9.1 (`diskann`) | the release asset `pgvectorscale-0.9.1-pg18-amd64.zip` (exists, checked 2026-11-14; contains the `.deb`) | **(call)** |

pgvecto.rs is dormant (last release 2024-11) and not run; other Rust extensions found (pgContext, ruvector-postgres, pg_pinecone) are noted in the report, not run, unless one turns out to be a real pgvector drop-in **(call)**. The research behind this table is in the campaign's evidence directory (`competitors-2026-11-14.md`).

**Server settings, identical where the knob exists** **(call)**: `shared_buffers=8GB`, `maintenance_work_mem=8GB` (pgvector's README: large enough for the graph), `work_mem=64MB`, `effective_cache_size=16GB`, `max_parallel_workers=8`, `max_parallel_maintenance_workers` 0 for serial builds and 4 for parallel ones, `max_parallel_workers_per_gather=2` (the harness's C-default pin), `max_wal_size=8GB`, `checkpoint_timeout=30min`, `log_min_duration_statement=-1`. pgrust also gets what its README and the harness's `PGRUST_SERVER_OPTS` require to run (`io_method=sync`, `max_stack_depth=60000`, `RUST_MIN_STACK`, `ulimit -s`); `pg-c` keeps PG 18's default `io_method=worker`. The report prints both sides' `pg_settings` where `source <> 'default'`. No other tuning.

## 5. Workloads and measurements

**Datasets** (ann-benchmarks HDF5 with 10,000 held-out queries and top-100 ground truth; sha256 pinned in `scripts/bench/datasets.sh`; cached on borg2 under `/home/jody/bench/data`):

| phase | dataset | rows × dim | metric | types |
|---|---|---|---|---|
| 1 | glove-100-angular | 1,183,514 × 100 | cosine | `vector`, `halfvec` |
| 1 | sift-128-euclidean | 1,000,000 × 128 | L2 | `vector` |
| 3 | deep-image-96-angular | 9,990,000 × 96 | cosine | `vector`, `halfvec` |
| 3 | Qdrant dbpedia-openai3 1536-d | 1,000,000 × 1536 | cosine | `vector`, `halfvec` |

**Measurements per contestant and dataset** (3 repetitions, medians reported, all three kept in the raw data):
1. **Load**: `COPY` of the rows, rows/s.
2. **Index build**: HNSW `m=16, ef_construction=64` (pgvector's defaults), serial and with 4 parallel workers; IVFFlat with `lists = rows/1000` (rows ≤ 1M) or `sqrt(rows)`; wall time, `pg_relation_size`, peak RSS, CPU-seconds.
3. **Search**: top-10 nearest neighbours. HNSW `ef_search ∈ {40, 64, 100, 200, 400}`, IVFFlat `probes ∈ {10, 20, 40, 80}`. For each setting: recall@10 over the 10,000 queries (one pass by the client), then throughput and p50/p95/p99 latency for 60 s at 1, 4, 8 and 16 concurrent clients (`pgbench` with a custom script drawing a random query from a loaded `queries` table), with CPU-seconds and peak RSS. Headline: **QPS at recall ≥ 0.95 and ≥ 0.99**, taken at the first setting of the sweep that reaches the target (the same rule for every contestant), plus the full curve.
4. **Exact scan**: the same queries without an index at 1 and 8 clients — the distance kernels' speed, and pgrust's executor.
5. **Inserts**: 100,000 new rows into the HNSW-indexed table at 1 and 8 clients, rows/s and peak RSS.
6. **Plain SQL**: `pgbench -i -s 100`, then `pgbench -S` (select-only) and the default TPC-B-like script, 8 clients × 60 s. One table in the report, labelled a sanity number for "pgrust as a Postgres".
7. **Soak** (stability, phase 1 form): 1 hour of mixed inserts and searches at 8 clients against the HNSW glove table with a `VACUUM` every 10 minutes; RSS sampled every 10 s; count of errors; p99 per 5-minute window. Pass: zero errors, no monotone RSS growth, p99 stable.
8. **Stability, phase 4 form**: the fork's conformance harness (`scripts/pgvector/run-all.sh pgrust`) run on Linux inside the containers (the C reference built by `build-reference.sh`; the on-disk tier needs Docker and is run nested or skipped and said so), plus a longer soak and a `crash-simulator` session.

**Scale (phase 3)**: the phase-1 measurements on the two larger datasets; concurrency extended to 32 clients; and one **memory-constrained** repeat with `limits.memory=8GiB` and `shared_buffers=2GB`, where the HNSW index no longer fits — the regime EDB's report says pgvector's HNSW is most sensitive to.

## 6. Phases

1. **First benchmark (this is what "pick a first benchmark" means here):** the environment, `pg-c` and `pgrust`, glove-100 and sift, measurements 1–7, the report. Also establishes whether pgrust builds and passes its own tests on x86-64 Linux (a build is running in container `pgrust-build` on borg2 as this is written; if it fails, fixing the build becomes task 1 of the plan).
2. **Rust reference points:** `pg-vchord` and `pg-vscale` on the phase-1 datasets, measurements 2–3 and 5 at their recommended settings; the "ahead of the Rust releases?" section, written as what it is: different algorithms on the same workload, compared at matched recall.
3. **Scale.**
4. **Stability on Linux.**
5. **Mosaic page** from the report (the product-name question in the handoff surfaces here).

Each phase is one plan and one branch (`bench/phase-N`), run the way this repo runs milestones (subagent-driven, ledger, final review), and ends with a report section and pushed `main`.

## 7. Deliverables and where things live

- `scripts/bench/` (new, in the repo): `borg.sh` (host side: create, limit, start/stop containers; cgroup sampling), `install-pg-c.sh`, `install-pgrust.sh`, `datasets.sh`, `load.sh`, `bench-build.sh`, `bench-search.sh`, `bench-exact.sh`, `bench-insert.sh`, `bench-pgbench.sh`, `soak.sh`, `report.py` (TSV → markdown tables). Python (h5py, numpy, psycopg) in a venv in the client container; everything else bash + psql + pgbench.
- Raw results: on borg2 under `/home/jody/bench/results/<run-id>/` and pulled to `~/.cache/pgrust/bench-borg/<run-id>/` on the Mac (TSV per measurement, both sides' `pg_settings`, `version()`, container configs, `incus info`, kernel and CPU facts, dataset checksums).
- Report: `docs/superpowers/reports/bench-borg-phase1-<date>.md` — a results-first document: headline table (speed ratios, memory ratios, CPU ratios), then one section per measurement, "where pgrust is behind", "limits of this measurement", reproduction steps. Later phases append sections or add files.
- Cluster bookkeeping: containers named `pgrust-*`; deleted or stopped at the end of each phase with a note of what remains.

## 8. Fairness rules the builder must keep

- One running database container at a time; the host otherwise idle (check the 1-minute load before each timed step; the Mac lesson: wait for background jobs).
- Reset `memory.peak` and read `cpu.stat` immediately around each measured step; record what was measured and the wall clock.
- The same SQL, the same client code and the same `pgbench` invocation for every contestant; the only differences are the index DDL and the per-engine search GUC.
- Recall is computed by the client from the dataset's ground truth, never trusted from the engine.
- Warm cache for search measurements (one full pass of the queries before timing), cold for nothing; say so.
- Builds of pgrust are from a recorded sha; `version()` output is recorded; the server's actual binary path is recorded.

## 9. Left to the builder

The exact `pgbench` script shape, the loader's batch size, how the recall pass is parallelised, the TSV schemas, the report's chart choices (if any), and the order of the phase-1 tasks — all within the rules above.

## 10. What this is not yet

No number in this document is a result. The first results are phase 1's report.

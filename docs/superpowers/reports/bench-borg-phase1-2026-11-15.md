# pgrust against PostgreSQL 18.6 + pgvector 0.8.7 on Linux x86-64: borg benchmark, phase 1 (2026-11-15)

The first measurements of pgrust off the Mac: both servers on one Linux x86-64 node in matched containers, on two ann-benchmarks datasets, for loads, index builds, searches, exact scans, inserts, plain SQL and a one-hour soak. Design: `docs/superpowers/specs/2026-11-14-borg-benchmark-design.md` (§1–§8 and the §11 amendments); plan: `docs/superpowers/plans/2026-11-14-borg-benchmark-phase1.md`; scripts: `scripts/bench/`. A report, not a gate.

- **Host:** `borg2`, Intel i9-11900 (8 cores, 16 threads; AVX-512, AVX2, FMA and F16C in `lscpu`'s flags), 61 GiB (`free -g`), Ubuntu 25.04, kernel 6.14.0-37, Incus 6.0.6, ZFS pool `local` (`compression=on`, `recordsize=128K` on every container's dataset). Shared with other instances (see Limits). Host facts: `facts/*/host.txt` and `facts/host-extra-2026-11-15.txt` in the run directory.
- **Containers** (Ubuntu 24.04 `noble`, `security.nesting=true`; only one database container ran at a time): `pgrust-db-c` and `pgrust-db-rust` at `limits.cpu=0-5` (six whole cores; the SMT siblings 8–13 are not in the set) and `limits.memory=24GiB`; the load generator `pgrust-client` at `limits.cpu=6,7,14,15` and 4 GiB.
- **pg-c:** PGDG `postgresql-18` 18.6-1.pgdg24.04+2 and `postgresql-18-pgvector` 0.8.7-1.pgdg24.04+1; `version()` = `PostgreSQL 18.6 (Ubuntu 18.6-1.pgdg24.04+2) on x86_64-pc-linux-gnu, compiled by gcc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0, 64-bit`; extension `vector` 0.8.7.
- **pgrust:** fork `main` at `a9c5743a3c`, built in the container with `cargo build --release --locked -p main_main -p initdb` (rustc 1.96.0, 6 min 19 s); `version()` = `PostgreSQL 18.6 (pgrust 0.4-beta, build a9c5743) on x86_64-unknown-linux-gnu, 64-bit`; extension `vector` 0.8.7. That sha includes the 2026-11-13 `halfvec` conversion fix; nothing under `crates/` differs between it and this branch.
- **Client:** PGDG `psql` and `pgbench` 18.6 (`postgresql-18`/`postgresql-client-18` 18.6-1.pgdg24.04+2) over `libpq5` 18.7-1.pgdg24.04+2; Python 3.12.3 with h5py 3.16.0, numpy 2.5.4 and psycopg 3.3.9 (`psycopg[binary]`, its own libpq 18.6) for the loader and the recall passes, pinned in `client/setup.sh` (the versions are `setup.sh`'s report in `.superpowers/sdd/2026-11-14-borg-benchmark-phase1/task-4-report.md`).
- **Settings, identical on both** (`scripts/bench/common-settings.conf`): `shared_buffers=8GB`, `maintenance_work_mem=8GB`, `work_mem=64MB`, `effective_cache_size=16GB`, `max_parallel_workers=8`, `max_parallel_maintenance_workers=4` (each build sets 0 or 4), `max_parallel_workers_per_gather=2`, `max_wal_size=8GB`, `checkpoint_timeout=30min`, `log_min_duration_statement=-1`, `listen_addresses='*'`, `max_connections=100`, `ssl=off`. pgrust also runs with what it requires: `io_method=sync`, `max_stack_depth=60000`, `RUST_MIN_STACK=33554432`, a 64 MiB stack limit, and the harness's `max_parallel_workers_per_gather=2` and `min_parallel_table_scan_size=8MB` on its command line, both equal to C's values (`scripts/bench/pgrust.service`; `pgrust-settings.txt`). C keeps PostgreSQL 18's default `io_method=worker`. Both sides' non-default `pg_settings` are in the evidence.
- **Run:** id `p1-20261114`, orchestrator `scripts/bench/run-phase1.sh` at `a2e6b06f3b` (each side's `facts/*/REVISION`). pg-c ran from 2026-11-14 23:36:54 to 2026-11-15 05:05:35 UTC, pgrust from 05:05:36 to 13:26:20 UTC; both exited 0 with 175 TSV rows and no error rows. The tables below are `scripts/bench/report.py` as of this report's last commit, over that run, plus tables derived from the same TSVs.

## Read this first

On one x86-64 node, with each server alone in a matched 6-core, 24 GiB container and the same settings wherever both servers have the knob, pgrust's pgvector port gives the same recall as C PostgreSQL 18.6 + pgvector 0.8.7 at every search setting (within 0.005) and builds indexes of the same size, but it is slower at almost every vector operation. At matched settings and 8 clients its search throughput is 0.72–0.74× C's on HNSW `vector`, 0.61× on HNSW `halfvec` and 0.76–0.84× on IVFFlat, its builds take 1.90–2.13× C's time serially and 1.58–1.66× with 4 workers on HNSW `vector` (2.74× and 2.19× on `halfvec`, 3.65–4.84× and 2.75–2.97× on IVFFlat), and its inserts under an HNSW index run at 0.85–0.95× C's rate. It spends more CPU for the same work, 1.39–1.64× C's per HNSW search query at the matched `ef_search` 100 and 1.38× per TPC-B-like transaction, and takes 1.34–2.84× C's time on the DELETE + VACUUM after inserts (its CPU was not recorded). It also holds more memory: its container's peak is above C's in every step except the 4-worker HNSW builds (1.5–2.4× during searches, 1.74× at the soak's peak), and below it only in those builds (0.35–0.40×), where C reserves the whole 8 GB shared graph area. The exact scan needs care: CPU-seconds show C using about 2.9 cores at 1 client, consistent with its planner's 2 parallel workers, and pgrust about 1.0, its planner building no parallel plans, so on `vector` pgrust is 0.47–0.51× C at 1 client but 1.06–1.11× at 8 clients, while on `halfvec` it is 0.24× and 0.51× (lead: the portable f16 route pgrust takes on x86-64); per-CPU comparisons there are confounded by C's worker start-up and possible JIT, and no plan was recorded. As a plain PostgreSQL it is close to C (`pgbench -S` 1.10×, though with only 4.8 of 6 cores busy on both sides it may be client-bound; TPC-B-like 0.94×), and the hour-long mixed soak finished with zero failed transactions on both servers, flat 5-minute windows and no memory growth after pgrust's first five minutes, at 0.79× C's throughput. Each search point was run once, on a host shared with other instances, and the memory numbers are container high-water marks, so treat differences of a few percent as noise (build reps spread by up to 8.8%) and the memory comparisons as approximate.

## Headline

`report.py`'s headline blocks, unedited. Ratios are pgrust ÷ pg-c: above 1 is better for pgrust in QPS, rows/s and TPS, and worse in ms, CPU-seconds and GiB.

### glove-100-angular / vector

| metric | pg-c | pgrust | pgrust ÷ pg-c |
|---|---|---|---|
| HNSW serial build, median ms | 373903 | 711935 | 1.90× |
| HNSW parallel build (4 workers), median ms | 139335 | 220752 | 1.58× |
| HNSW QPS at ef_search 100, 8 clients (matched setting) | 2320 (recall 0.7976) | 1711 (recall 0.7968) | 0.74× |
| IVFFlat QPS at probes 20, 8 clients (matched setting) | 680 (recall 0.8263) | 570 (recall 0.8226) | 0.84× |
| HNSW QPS at recall ≥ 0.90, 8 clients | not reached: 758 (ef_search 400, recall 0.8969)† | not reached: 611 (ef_search 400, recall 0.8964)† | 0.81× |
| HNSW QPS at recall ≥ 0.90, 16 clients | not reached: 733 (ef_search 400, recall 0.8969)† | not reached: 609 (ef_search 400, recall 0.8964)† | 0.83× |
| HNSW QPS at recall ≥ 0.95, 8 clients | not reached: 758 (ef_search 400, recall 0.8969) | not reached: 611 (ef_search 400, recall 0.8964) | 0.81× |
| HNSW QPS at recall ≥ 0.95, 16 clients | not reached: 733 (ef_search 400, recall 0.8969) | not reached: 609 (ef_search 400, recall 0.8964) | 0.83× |
| HNSW QPS at recall ≥ 0.99, 8 clients | not reached: 758 (ef_search 400, recall 0.8969) | not reached: 611 (ef_search 400, recall 0.8964) | 0.81× |
| HNSW QPS at recall ≥ 0.99, 16 clients | not reached: 733 (ef_search 400, recall 0.8969) | not reached: 609 (ef_search 400, recall 0.8964) | 0.83× |
| IVFFlat QPS at recall ≥ 0.90, 8 clients | 189 (probes 80) | 160 (probes 80) | 0.85× |
| IVFFlat QPS at recall ≥ 0.90, 16 clients | 185 (probes 80) | 159 (probes 80) | 0.86× |
| IVFFlat QPS at recall ≥ 0.95, 8 clients | not reached: 189 (probes 80, recall 0.9349) | not reached: 160 (probes 80, recall 0.9322) | 0.85× |
| IVFFlat QPS at recall ≥ 0.95, 16 clients | not reached: 185 (probes 80, recall 0.9349) | not reached: 159 (probes 80, recall 0.9322) | 0.86× |
| IVFFlat QPS at recall ≥ 0.99, 8 clients | not reached: 189 (probes 80, recall 0.9349) | not reached: 160 (probes 80, recall 0.9322) | 0.85× |
| IVFFlat QPS at recall ≥ 0.99, 16 clients | not reached: 185 (probes 80, recall 0.9349) | not reached: 159 (probes 80, recall 0.9322) | 0.86× |
| Exact scan QPS, 1 client, median | 12.3 | 6.25 | 0.51× |
| Exact scan TPS, 8 clients | 21.0 | 23.2 | 1.11× |
| HNSW inserts, rows/s, 8 clients | 1092 | 1010 | 0.92× |
| Container peak, serial HNSW build (median), GiB¹ | 3.17 | 3.44 | 1.08× |
| Build increment, rep 1 (approx.), serial HNSW build, GiB¹ | 2.02 | 2.05 | 1.01× |
| CPU-seconds of the serial HNSW build, median | 373 | 719 | 1.93× |

glove-100 is a hard dataset for these parameters: with `m = 16, ef_construction = 64` both engines top out at recall 0.897 at `ef_search` 400, and IVFFlat at 0.935 at 80 probes, so no glove cell reaches 0.95: every 0.95 and 0.99 row on this block, and HNSW's 0.90 row, is a highest-setting fallback. At the matched settings pgrust serves 0.74× C's HNSW and 0.84× C's IVFFlat throughput at the same recall.

### glove-100-angular / halfvec

| metric | pg-c | pgrust | pgrust ÷ pg-c |
|---|---|---|---|
| HNSW serial build, median ms | 437146 | 1197417 | 2.74× |
| HNSW parallel build (4 workers), median ms | 150406 | 328650 | 2.19× |
| HNSW QPS at ef_search 100, 8 clients (matched setting) | 2460 (recall 0.7967) | 1501 (recall 0.7959) | 0.61× |
| HNSW QPS at recall ≥ 0.90, 8 clients | not reached: 812 (ef_search 400, recall 0.8963)† | not reached: 530 (ef_search 400, recall 0.8968)† | 0.65× |
| HNSW QPS at recall ≥ 0.90, 16 clients | not reached: 784 (ef_search 400, recall 0.8963)† | not reached: 529 (ef_search 400, recall 0.8968)† | 0.67× |
| HNSW QPS at recall ≥ 0.95, 8 clients | not reached: 812 (ef_search 400, recall 0.8963) | not reached: 530 (ef_search 400, recall 0.8968) | 0.65× |
| HNSW QPS at recall ≥ 0.95, 16 clients | not reached: 784 (ef_search 400, recall 0.8963) | not reached: 529 (ef_search 400, recall 0.8968) | 0.67× |
| HNSW QPS at recall ≥ 0.99, 8 clients | not reached: 812 (ef_search 400, recall 0.8963) | not reached: 530 (ef_search 400, recall 0.8968) | 0.65× |
| HNSW QPS at recall ≥ 0.99, 16 clients | not reached: 784 (ef_search 400, recall 0.8963) | not reached: 529 (ef_search 400, recall 0.8968) | 0.67× |
| Exact scan QPS, 1 client, median | 13.1 | 3.10 | 0.24× |
| Exact scan TPS, 8 clients | 22.4 | 11.5 | 0.51× |
| HNSW inserts, rows/s, 8 clients | 1065 | 941 | 0.88× |
| Container peak, serial HNSW build (median), GiB¹ | 3.53 | 4.57 | 1.29× |
| Build increment, rep 1 (approx.), serial HNSW build, GiB¹ | 1.50 | 1.08 | 0.72× |
| CPU-seconds of the serial HNSW build, median | 437 | 1222 | 2.80× |

`halfvec` is pgrust's weakest type on x86-64: 0.61× C's search throughput at the matched setting, 2.74× C's serial build time and 0.24× C's 1-client exact scan, where C's `halfvec` runs at about C's own `vector` speed. Its 0.72× build increment rests on a baseline that differs between the two sides (see Builds), so it doesn't show that pgrust uses less memory for the build.

### sift-128-euclidean / vector

| metric | pg-c | pgrust | pgrust ÷ pg-c |
|---|---|---|---|
| HNSW serial build, median ms | 267746 | 569595 | 2.13× |
| HNSW parallel build (4 workers), median ms | 101990 | 169562 | 1.66× |
| HNSW QPS at ef_search 100, 8 clients (matched setting) | 2843 (recall 0.9827) | 2037 (recall 0.9825) | 0.72× |
| IVFFlat QPS at probes 20, 8 clients (matched setting) | 727 (recall 0.9501) | 550 (recall 0.9491) | 0.76× |
| HNSW QPS at recall ≥ 0.90, 8 clients | 5347 (ef_search 40) | 3542 (ef_search 40) | 0.66× |
| HNSW QPS at recall ≥ 0.90, 16 clients | 5204 (ef_search 40) | 3445 (ef_search 40) | 0.66× |
| HNSW QPS at recall ≥ 0.95, 8 clients | 3915 (ef_search 64) | 2700 (ef_search 64) | 0.69× |
| HNSW QPS at recall ≥ 0.95, 16 clients | 3815 (ef_search 64) | 2723 (ef_search 64) | 0.71× |
| HNSW QPS at recall ≥ 0.99, 8 clients | 1641 (ef_search 200)† | 1257 (ef_search 200)† | 0.77× |
| HNSW QPS at recall ≥ 0.99, 16 clients | 1602 (ef_search 200)† | 1266 (ef_search 200)† | 0.79× |
| IVFFlat QPS at recall ≥ 0.90, 8 clients | 727 (probes 20) | 550 (probes 20) | 0.76× |
| IVFFlat QPS at recall ≥ 0.90, 16 clients | 724 (probes 20) | 553 (probes 20) | 0.76× |
| IVFFlat QPS at recall ≥ 0.95, 8 clients | 727 (probes 20)† | 291 (probes 40)† | 0.40× |
| IVFFlat QPS at recall ≥ 0.95, 16 clients | 724 (probes 20)† | 291 (probes 40)† | 0.40× |
| IVFFlat QPS at recall ≥ 0.99, 8 clients | 182 (probes 80)† | 153 (probes 80)† | 0.84× |
| IVFFlat QPS at recall ≥ 0.99, 16 clients | 180 (probes 80)† | 152 (probes 80)† | 0.84× |
| Exact scan QPS, 1 client, median | 13.6 | 6.39 | 0.47× |
| Exact scan TPS, 8 clients | 23.4 | 24.9 | 1.06× |
| HNSW inserts, rows/s, 8 clients | 1160 | 1080 | 0.93× |
| Container peak, serial HNSW build (median), GiB¹ | 3.70 | 3.97 | 1.07× |
| Build increment, rep 1 (approx.), serial HNSW build, GiB¹ | 1.10 | 1.12 | 1.02× |
| CPU-seconds of the serial HNSW build, median | 267 | 580 | 2.18× |

sift is where the recall targets are reached, and its IVFFlat ≥ 0.95 row shows the threshold effect the † marks: C's probes 20 reaches 0.9501 and pgrust's reaches 0.9491, so pgrust's cell comes from probes 40 and reads 0.40×, while at the same probes the ratio is 0.76×. Every row that compares the same setting on both sides lands between 0.66× and 0.84×.

¹ Container peak: the database container's cgroup high-water mark during the step (anonymous memory and touched shared buffers; the file cache lives outside the cgroup), whose floor is the memory resident before the step. Build increment (approx.): rep 1's peak minus the peak of the last measured step before it (an exact pass, an insert step or the previous build group), an upper bound on the baseline and so a lower bound on what the build added; reps 2 and 3 have none. Runs after p1-20261114 log the exact baseline ("measure: ... base_bytes=").

Matched setting: both engines at the same ef_search or probes, with each one's 1-client recall in brackets; where the recalls agree this is the like-for-like search comparison. Recall ≥ R: the first setting whose 1-client recall reaches R ("not reached:" the highest setting, with its recall). The 8- and 16-client figures are pgbench TPS of a two-statement transaction (a primary-key lookup returning the query vector as text, then the search).

† The recall of the cell's setting, or of the setting before it, is within 0.005 of the target. Where two engines' recall curves agree to about that width, which setting first clears the bar, and so the QPS in the cell, can turn on a recall difference at the noise level; read the matched-setting rows instead.

(On this host the "file cache" of the footnote is ZFS's ARC, which is kernel memory outside every container's cgroup, for both servers.)

## Method

The sequence per contestant (`run-phase1.sh`; settle before each load, each build, the first point of each sweep, each insert step, the plain-SQL step and the soak):

- **Datasets:** glove-100-angular (1,183,514 × 100, cosine; `vector` table `items` and `halfvec` table `items_h`) and sift-128-euclidean (1,000,000 × 128, L2; `vector`), each with 10,000 held-out queries and their top-100 ground truth, sha256-pinned in `scripts/bench/client/SHA256SUMS`.
- **Load:** `load.py` formats every row first and then times its `COPY` of `items`; 3 loads, each into a fresh database; rows/s of `items`.
- **Exact scan,** before any index: 3 passes of `recall.py --exact` over the first 1,000 queries at 1 client (a warm-up pass of the same 1,000, then a timed one; `enable_indexscan = off`), then `pgbench` at 8 clients for 30 s drawing random queries from all 10,000.
- **Builds:** 3 serial (`max_parallel_maintenance_workers = 0`) and 3 with 4 workers, `maintenance_work_mem = 8GB`, `psql`'s `\timing`; HNSW `m = 16, ef_construction = 64`; IVFFlat `lists = 1184` on glove and 1000 on sift. Each index is dropped but the last, a 4-worker build, which every search sweep then uses on both sides. Index size is `pg_relation_size` after each build.
- **Search:** top 10 by `ORDER BY v <op> '<query>' LIMIT 10`; HNSW `ef_search` 40, 64, 100, 200 and 400, IVFFlat `probes` 10, 20, 40 and 80. Per setting: one 1-client `recall.py` pass over all 10,000 queries after a warm-up pass (QPS, recall@10 against the ground truth, latency percentiles), then `pgbench` at 8 and at 16 clients for 30 s each (TPS and p50/p99 from its transaction log). Each `pgbench` transaction is two statements, the query's vector looked up by primary key in `queries` and the search.
- **Inserts:** 100,000 single-row inserts into the HNSW-indexed table at 1 client, then 12,500 per client at 8 clients; the rows duplicate indexed vectors (`extra` is training rows drawn with replacement) and are deleted and the table vacuumed after each step (that DELETE + VACUUM is the `vacuum` row).
- **Plain SQL:** `pgbench -i -s 100`, then `-S` and the default TPC-B-like script, 8 clients × 60 s.
- **Soak:** a fresh glove database and a 4-worker HNSW index, then 1 h of `pgbench` at 8 clients mixing searches (`ef_search = 100`, weight 9) and inserts (weight 1), a `VACUUM items` every 10 minutes, and the container's `memory.current` every 10 s.
- **Metrics,** read from the host: the container cgroup's `memory.peak` (reset just before the step) and `cpu.stat` `usage_usec` (the whole server: postmaster, backends, workers and background processes). "Cores busy" below is CPU-seconds ÷ wall seconds.

Departures from the design's §5: the pgbench search points are 30 s, not 60 s, and the sweeps ran at 1, 8 and 16 clients, without 4. The 1-client points come from `recall.py` over psycopg (the extended protocol, one statement per query, 10,000 queries; the exact pass 1,000), the 8- and 16-client points from `pgbench` (the simple protocol, two statements per transaction), so 1-client and 8-client ratios are not like-for-like.

## Load

| dataset | pg-c rows/s (median) | pgrust rows/s (median) | pgrust ÷ pg-c | pg-c reps | pgrust reps | pg-c container peak GiB | pgrust container peak GiB |
|---|---|---|---|---|---|---|---|
| glove | 113448 | 114435 | 1.01× | 113025, 113448, 113872 | 114435, 126559, 94173 | 1.12 | 1.33 |
| sift | 200798 | 188493 | 0.94× | 201790, 200798, 199034 | 188195, 188733, 188493 | 2.55 | 2.80 |

Loads are about equal on glove and 0.94× on sift. pgrust's glove reps spread from 94,173 to 126,559 rows/s while C's stayed within 1%, which is the run's widest spread; the median is the middle rep. Medians are over the reps in `pg-c.tsv` and `pgrust.tsv`; the peak column is the median of the three peaks.

## Exact scan

| dataset | type | clients | pg-c QPS | pgrust QPS | pgrust ÷ pg-c | pg-c cores busy | pgrust cores busy | pg-c CPU ms / query | pgrust CPU ms / query | CPU per query, pgrust ÷ pg-c | recall pg-c / pgrust |
|---|---|---|---|---|---|---|---|---|---|---|---|
| glove | vector | 1 | 12.3 | 6.25 | 0.51× | 2.91 | 1.00 | 236 | 160 | 0.68× | 1.0000 / 1.0000 |
| glove | vector | 8 | 21.0 | 23.2 | 1.11× | 5.83 | 5.91 | 278 | 254 | 0.91× | – |
| glove | halfvec | 1 | 13.1 | 3.10 | 0.24× | 2.92 | 1.00 | 224 | 323 | 1.44× | 0.9994 / 0.9995 |
| glove | halfvec | 8 | 22.4 | 11.5 | 0.51× | 5.89 | 5.86 | 262 | 508 | 1.94× | – |
| sift | vector | 1 | 13.6 | 6.39 | 0.47× | 2.90 | 1.00 | 214 | 157 | 0.73× | 0.9993 / 0.9994 |
| sift | vector | 8 | 23.4 | 24.9 | 1.06× | 5.88 | 5.88 | 251 | 236 | 0.94× | – |

(1-client rows: medians of 3 passes; CPU per query is the step's CPU over its 2,000 queries, the warm-up included. 8-client rows: CPU over TPS × wall seconds.)

- **The 1-client gap on `vector` comes from parallelism (cause, from the CPU-seconds).** C's container is 2.9 cores busy through a single-client exact pass and pgrust's is 1.00, consistent with C's planner running the scan as a parallel sequential scan with `max_parallel_workers_per_gather = 2` workers beside the leader; pgrust's planner, upstream v0.4-beta's, builds no parallel plans (`planner/src/lib.rs`: `assess_parallel = false`; this repo's `CLAUDE.md`, "Accepted divergence"). No `EXPLAIN` of the exact query was recorded, so the plan shape is inferred from the CPU, not read. Per CPU-second pgrust's `vector` scan looks faster than C's, at 0.68× (glove) and 0.73× (sift) the CPU per query at 1 client and, at 8 clients where both use all six cores, 1.11× and 1.06× the throughput. That is a lead with two confounds: C's CPU per query includes launching two parallel workers and the Gather for every query, and with `jit` at its default `on` possibly JIT compilation (the scan's cost is near `jit_above_cost`). The same overheads sit on C's side of the `halfvec` comparison, so its 1.44× understates the per-process gap.
- **`halfvec` is slower per process (lead).** pgrust spends 1.44× C's CPU per query at 1 client and 1.94× at 8. On x86-64 pgrust's f16→f32 conversion takes the portable block route, which LLVM can vectorize with SSE2 (`halves_to_floats`, `crates/contrib/pgvector/src/halfutils.rs`; there is no F16C route, decision 5 of `docs/superpowers/plans/2026-11-13-pgvector-halfvec-f16-conversion.md`) and the kernels keep C's scalar summation order (user ruling 2026-10-06). C pgvector's x86-64 build picks AVX/F16C/FMA kernels at run time when the CPU has them (`halfutils.c:20` and `:291` in the vendored 0.8.7 source). That is the likely cause, not a proven one: nothing here profiles it.
- **Both engines cost more CPU per query at 8 clients than at 1** (C 236 → 278 ms on glove `vector`, pgrust 160 → 254 ms). With every query reading the whole table, memory-bandwidth contention is a candidate (lead, unmeasured), and it may compress the 8-client `vector` ratio toward 1.
- **Recall of the exact scan** is 1.0000 on glove `vector` over its 1,000 queries, 0.9993 (C) and 0.9994 (pgrust) on sift `vector`, and 0.9994 and 0.9995 on glove `halfvec`. Recall here is the overlap of id sets, so a tie at the 10th neighbour (sift's integer-valued vectors make exact distance ties likely) counts as a miss when the engine picks a different id from the ground truth's; ann-benchmarks itself compares distances with an ε. On `halfvec` the stored values are rounded to f16, which moves some distances. Tie-breaking can differ between the engines, so recall differences in the fourth decimal are noise here and below.

## Builds, index sizes and build memory

| dataset | type | AM | setting | pg-c s | pgrust s | pgrust ÷ pg-c | pg-c CPU s | pgrust CPU s | pg-c container peak GiB | pgrust container peak GiB | peak, pgrust ÷ pg-c | index MiB pg-c / pgrust |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| glove | vector | hnsw | serial | 374 | 712 | 1.90× | 373 | 719 | 3.17 | 3.44 | 1.08× | 841 / 841 |
| glove | vector | hnsw | parallel4 | 139 | 221 | 1.58× | 674 | 1091 | 9.98 | 3.45 | 0.35× | 841 / 841 |
| glove | halfvec | hnsw | serial | 437 | 1197 | 2.74× | 437 | 1222 | 3.53 | 4.57 | 1.29× | 579 / 579 |
| glove | halfvec | hnsw | parallel4 | 150 | 329 | 2.19× | 733 | 1636 | 10.56 | 4.19 | 0.40× | 579 / 579 |
| sift | vector | hnsw | serial | 268 | 570 | 2.13× | 267 | 580 | 3.70 | 3.97 | 1.07× | 782 / 782 |
| sift | vector | hnsw | parallel4 | 102 | 170 | 1.66× | 490 | 837 | 10.60 | 3.97 | 0.37× | 782 / 782 |
| glove | vector | ivfflat | serial | 34.6 | 126 | 3.65× | 34.6 | 127 | 3.08 | 4.28 | 1.39× | 491 / 492 |
| glove | vector | ivfflat | parallel4 | 22.8 | 62.8 | 2.75× | 52.7 | 171 | 3.08 | 4.29 | 1.39× | 491 / 491 |
| sift | vector | ivfflat | serial | 24.4 | 118 | 4.84× | 24.0 | 119 | 3.12 | 4.16 | 1.33× | 525 / 525 |
| sift | vector | ivfflat | parallel4 | 15.5 | 46.0 | 2.97× | 40.3 | 158 | 3.12 | 4.16 | 1.33× | 525 / 525 |

(Medians of 3 builds; sizes are the median of the three `pg_relation_size` values. Every 4-worker build's log shows `using 4 parallel workers` and five participants on both sides.)

- **HNSW:** 1.90× (glove) and 2.13× (sift) C's time serially, 1.58× and 1.66× with 4 workers; `halfvec` 2.74× and 2.19×. Serial builds are single-threaded on both sides, so CPU-seconds track wall time (373 against 374 s for C on glove, 719 against 712 for pgrust); with 4 workers both keep 4.8 to 5.0 cores busy. pgrust gains more from workers (3.23× to 3.64× its own serial time, against C's 2.63× to 2.91×), consistent with its serial part being the slower one (lead: the parallel phase may hide per-element cost better; not measured). The reps agree within 0.2% to 2.8%, except pgrust's glove `vector` 4-worker builds (206.9, 220.8 and 220.8 s; `pgrust.tsv`).
- **IVFFlat:** 3.65× (glove) and 4.84× (sift) C's time serially, 2.75× and 2.97× with 4 workers. These are the largest gaps in the run.
- **Index sizes are equal** on every row (HNSW to the MiB; IVFFlat 491 or 492 MiB on glove, on both sides, depending on the build).
- **Against the Mac** (`pgvector-speed-2026-11-08.md`, 100,000 GloVe rows on an M5 Max): the serial HNSW `vector` build was 1.56× to 1.73× C there, the `halfvec` one 2.15× after the conversion fix, and the serial IVFFlat `vector` build 2.25× to 2.29×. On x86-64 every build gap is wider. A lead, not a measured cause: C pgvector is compiled with `-ftree-vectorize -fassociative-math -ffp-contract=fast` (the vendored `Makefile`), which lets GCC vectorize its distance reductions, and on x86-64 it adds an FMA clone of the `vector` kernels (`vector.c:42-43`) and AVX/F16C/FMA `halfvec` kernels chosen at run time. pgrust keeps C's source order element by element (the 2026-10-06 ruling), which rules such vectorization out on any target, and has no F16C route. 256-bit AVX gives C's kernels twice the lanes NEON gave them on the Mac, so a wider gap on x86-64 is plausible. Whether PGDG's package also builds with `-march=native` is not recorded. The datasets differ (100,000 rows there, 1–1.18 million here), so the comparison is rough. IVFFlat's k-means is almost entirely distance computations, which would explain why its gap is the widest; it is not profiled.

**Build memory.** C's 4-worker HNSW builds peak at 9.98 to 10.60 GiB, against 3.17 to 3.70 GiB for its serial builds. pgvector's parallel HNSW build sizes its shared graph area from `maintenance_work_mem` (`hnswbuild.c:964-1013`), and PostgreSQL's POSIX DSM reserves a segment with `posix_fallocate` (`dsm_impl.c:383`), so the 8 GB area is charged to the container whether or not the graph fills it (the likely cause: it follows from the source and matches the size, but was not traced). pgrust's 4-worker builds peak at about its serial builds' level (3.45 to 4.19 GiB); it evidently doesn't commit that area up front, but the mechanism is not traced here. For serial builds the container peak is 1.07× to 1.29× C's, pgrust higher. That peak includes everything the container already held, and pgrust's resting memory is higher throughout the run (Resources). The approximate increment, rep 1's peak minus the previous measured step's peak, is about equal on `vector` (2.02 against 2.05 GiB on glove, 1.10 against 1.12 on sift) and 0.72× on `halfvec` (1.50 against 1.08 GiB). The `halfvec` group's previous step is the `vector` 8-client insert step, which peaked at 2.03 GiB on C and 3.49 GiB on pgrust (`pg-c.tsv`, `pgrust.tsv`). That is an upper bound on each side's baseline at the build's reset, and nothing says how far above the real baseline it sits on each side. So the 0.72× is a lower bound against a lower bound, not evidence that pgrust's build uses less. This run's logs have no `base_bytes` lines; the orchestrator logs them from `574941a56d` on.

## HNSW search

| dataset | type | ef_search | pg-c recall | pgrust recall | 1 client, pgrust ÷ pg-c | pg-c QPS, 8 clients | pgrust QPS, 8 clients | 8 clients, pgrust ÷ pg-c | 16 clients, pgrust ÷ pg-c | pg-c p99 ms, 8 clients | pgrust p99 ms, 8 clients |
|---|---|---|---|---|---|---|---|---|---|---|---|
| glove | vector | 40 | 0.6931 | 0.6896 | 0.77× | 4367 | 2989 | 0.68× | 0.69× | 4.34 | 6.66 |
| glove | vector | 64 | 0.7510 | 0.7511 | 0.80× | 3195 | 2265 | 0.71× | 0.74× | 6.28 | 9.21 |
| glove | vector | 100 | 0.7976 | 0.7968 | 0.83× | 2320 | 1711 | 0.74× | 0.76× | 9.28 | 11.8 |
| glove | vector | 200 | 0.8525 | 0.8523 | 0.86× | 1370 | 1060 | 0.77× | 0.79× | 13.7 | 17.1 |
| glove | vector | 400 | 0.8969 | 0.8964 | 0.89× | 758 | 611 | 0.81× | 0.83× | 23.3 | 27.6 |
| glove | halfvec | 40 | 0.6893 | 0.6909 | 0.63× | 4566 | 2584 | 0.57× | 0.59× | 4.15 | 7.83 |
| glove | halfvec | 64 | 0.7499 | 0.7483 | 0.65× | 3353 | 1993 | 0.59× | 0.61× | 5.99 | 10.6 |
| glove | halfvec | 100 | 0.7967 | 0.7959 | 0.66× | 2460 | 1501 | 0.61× | 0.62× | 8.59 | 12.7 |
| glove | halfvec | 200 | 0.8517 | 0.8528 | 0.68× | 1463 | 920 | 0.63× | 0.64× | 12.9 | 19.0 |
| glove | halfvec | 400 | 0.8963 | 0.8968 | 0.69× | 812 | 530 | 0.65× | 0.67× | 21.8 | 31.0 |
| sift | vector | 40 | 0.9359 | 0.9352 | 0.73× | 5347 | 3542 | 0.66× | 0.66× | 3.29 | 5.33 |
| sift | vector | 64 | 0.9667 | 0.9660 | 0.76× | 3915 | 2700 | 0.69× | 0.71× | 4.79 | 7.05 |
| sift | vector | 100 | 0.9827 | 0.9825 | 0.78× | 2843 | 2037 | 0.72× | 0.74× | 6.91 | 9.91 |
| sift | vector | 200 | 0.9945 | 0.9946 | 0.83× | 1641 | 1257 | 0.77× | 0.79× | 11.8 | 14.1 |
| sift | vector | 400 | 0.9982 | 0.9981 | 0.86× | 947 | 750 | 0.79× | 0.83× | 18.8 | 22.5 |

- **Recall agrees** at every setting, by at most 0.0035 (glove `vector`, `ef_search` 40) and with no consistent sign. The curves are the same curve.
- **Throughput at 8 clients** is 0.66× to 0.81× C on `vector` and 0.57× to 0.65× on `halfvec`; 16 clients add nothing on either side (both containers keep 5.6 to 5.9 of their 6 cores busy at 8 clients, so these ratios are also per-CPU ratios). p99 at 8 clients is 1.19× to 1.62× C's on `vector` and 1.42× to 1.89× on `halfvec`. The 1-client ratios are higher (0.73× to 0.89× on `vector`), consistent with their including the Python client's own time per query, which both sides pay (lead).
- **The ratio improves with `ef_search`** (glove `vector` 0.68× at 40, 0.81× at 400), so pgrust's extra cost is largest per query, not per distance. A least-squares line through each side's CPU per query (8 clients, five settings) separates the two:

| dataset | type | CPU ms per query at ef 40, pg-c / pgrust | at ef 400 | fixed part ms, pg-c / pgrust | fixed, pgrust ÷ pg-c | µs per unit of ef_search, pg-c / pgrust | per ef, pgrust ÷ pg-c |
|---|---|---|---|---|---|---|---|
| glove | vector | 1.30 / 1.90 | 7.72 / 9.62 | 0.64 / 1.18 | 1.84× | 17.8 / 21.2 | 1.19× |
| glove | halfvec | 1.23 / 2.21 | 7.17 / 11.10 | 0.64 / 1.32 | 2.06× | 16.4 / 24.6 | 1.50× |
| sift | vector | 1.05 / 1.61 | 6.12 / 7.83 | 0.56 / 1.04 | 1.86× | 14.0 / 17.2 | 1.22× |

  (CPU per query = the step's CPU-seconds ÷ (TPS × wall seconds), from `pg-c.tsv` and `pgrust.tsv`; the fit is a derivation, not a measurement.) Each transaction's fixed part, which covers the lookup statement, parsing, planning, executor start-up and the protocol, costs pgrust 0.5 to 0.7 ms of CPU more than C, 1.84× to 2.06× C's. The work per unit of `ef_search`, which is graph walking and distance computations, costs 1.19× to 1.22× on `vector` and 1.50× on `halfvec`. In absolute terms pgrust's extra cost on glove `vector` is 0.54 ms fixed plus 3.4 µs per unit of `ef_search`: the fixed part is the larger below about `ef_search` 160, the per-ef part from 200 on (1.37 ms at 400; sift is alike). On `halfvec` it is 0.68 ms fixed plus 8.2 µs per unit, so the per-ef part, the distance work, is already the larger at the matched `ef_search` 100 (0.82 ms). The intercept also holds `ef_search`-independent index work (the upper-layer descent, the scan's start-up, fetching the 10 heap tuples), so what the fixed part is spent on is not attributable from this data. This is a lead for where to profile, not an attribution.

## IVFFlat search

| dataset | type | probes | pg-c recall | pgrust recall | 1 client, pgrust ÷ pg-c | pg-c QPS, 8 clients | pgrust QPS, 8 clients | 8 clients, pgrust ÷ pg-c | 16 clients, pgrust ÷ pg-c | pg-c p99 ms, 8 clients | pgrust p99 ms, 8 clients |
|---|---|---|---|---|---|---|---|---|---|---|---|
| glove | vector | 10 | 0.7492 | 0.7472 | 0.71× | 1278 | 1038 | 0.81× | 0.82× | 16.5 | 18.1 |
| glove | vector | 20 | 0.8263 | 0.8226 | 0.75× | 680 | 570 | 0.84× | 0.85× | 28.5 | 31.1 |
| glove | vector | 40 | 0.8886 | 0.8837 | 0.77× | 360 | 305 | 0.85× | 0.86× | 49.7 | 52.5 |
| glove | vector | 80 | 0.9349 | 0.9322 | 0.79× | 189 | 160 | 0.85× | 0.86× | 84.5 | 92.1 |
| sift | vector | 10 | 0.8733 | 0.8704 | 0.65× | 1381 | 1010 | 0.73× | 0.75× | 15.5 | 19.1 |
| sift | vector | 20 | 0.9501 | 0.9491 | 0.68× | 727 | 550 | 0.76× | 0.76× | 25.8 | 32.4 |
| sift | vector | 40 | 0.9870 | 0.9861 | 0.70× | 383 | 291 | 0.76× | 0.77× | 43.8 | 55.5 |
| sift | vector | 80 | 0.9975 | 0.9975 | 0.78× | 182 | 153 | 0.84× | 0.84× | 82.5 | 97.2 |

- At 8 clients pgrust serves 0.81× to 0.85× C on glove and 0.73× to 0.84× on sift; at the matched 20 probes 0.84× and 0.76×. p99 is 1.06× to 1.27× C's. The same line fit as for HNSW, over the four probes settings, gives each probe's CPU (the fixed part is too small against the probe cost to fit well; only C's sift intercept is negative, −0.17 ms, while pgrust's is +1.19 ms):

| dataset | µs of CPU per probe, pg-c / pgrust | pgrust ÷ pg-c |
|---|---|---|
| glove | 380 / 445 | 1.17× |
| sift | 404 / 470 | 1.16× |

- pgrust's recall is lower than C's at 7 of the 8 settings, by 0.0009 to 0.0048 (raw values in the TSVs), and equal to four decimals at the eighth. All four settings of a dataset search the same index, the last of six builds on each side, and k-means is seeded randomly on both, so this is one draw of each side's clustering seen four times, not shown to be a systematic difference. (pgrust's parallel IVFFlat build also orders the rows within a list differently from C's, per `CLAUDE.md`.) Several builds per side would settle it.
- **`lists` is 1184 on glove**, one more than the design's `rows / 1000` = 1183: `a2e6b06f3b` rounded where the design floors, fixed in `574941a56d`. Both sides got 1184; sift got 1000.

## Inserts and the vacuum after them

| dataset | type | clients | pg-c rows/s | pgrust rows/s | pgrust ÷ pg-c | pg-c cores busy | pgrust cores busy | pg-c CPU ms / row | pgrust CPU ms / row | CPU per row, pgrust ÷ pg-c | pg-c DELETE+VACUUM s | pgrust DELETE+VACUUM s | vacuum, pgrust ÷ pg-c |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| glove | vector | 1 | 172 | 163 | 0.95× | 0.64 | 0.69 | 3.70 | 4.23 | 1.14× | 125 | 184 | 1.47× |
| glove | vector | 8 | 1092 | 1010 | 0.92× | 2.93 | 3.16 | 2.68 | 3.14 | 1.17× | 118 | 172 | 1.46× |
| glove | halfvec | 1 | 180 | 153 | 0.85× | 0.66 | 0.71 | 3.65 | 4.67 | 1.28× | 124 | 344 | 2.77× |
| glove | halfvec | 8 | 1065 | 941 | 0.88× | 2.83 | 3.33 | 2.68 | 3.56 | 1.33× | 112 | 318 | 2.84× |
| sift | vector | 1 | 207 | 184 | 0.89× | 0.61 | 0.68 | 2.97 | 3.71 | 1.25× | 8.52 | 11.6 | 1.36× |
| sift | vector | 8 | 1160 | 1080 | 0.93× | 2.69 | 2.99 | 2.33 | 2.78 | 1.19× | 8.44 | 11.3 | 1.34× |

- Inserts run at 0.85× to 0.95× C (0.88× to 0.93× at 8 clients). Neither side is CPU-bound: one client keeps 0.6 to 0.7 cores busy, consistent with each single-row transaction waiting mostly for its commit (`fsync = on`, ZFS) and the client round trip (lead; the waits were not measured). pgrust uses 1.14× to 1.33× C's CPU per row, which the rate ratio understates. On the Mac, where the workload was 1,000 inserts in one statement, pgrust took 1.52× C's time.
- The DELETE + VACUUM that removes the 100,000 rows takes pgrust 1.34× to 1.47× C's time on `vector` and 2.77× to 2.84× on `halfvec`. HNSW's vacuum repairs the neighbours of deleted elements, which is distance work, so the kernel leads above apply (lead). The `halfvec` multiple matches its build gap.

## Plain SQL

| step | unit | pg-c | pgrust | pgrust ÷ pg-c | pg-c cores busy | pgrust cores busy | pg-c CPU ms / tx | pgrust CPU ms / tx | CPU per tx, pgrust ÷ pg-c | pg-c container peak GiB | pgrust container peak GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| pgbench_init | s | 6.42 | 6.31 | 0.98× | 1.36 | 1.22 | – | – | – | 2.97 | 3.31 |
| pgbench_select | tps | 73135 | 80495 | 1.10× | 4.77 | 4.79 | 0.065 | 0.059 | 0.91× | 2.61 | 3.05 |
| pgbench_tpcb | tps | 1795 | 1679 | 0.94× | 2.03 | 2.62 | 1.132 | 1.559 | 1.38× | 2.61 | 2.85 |

`pgbench -S` runs at 1.10× C with 0.91× C's CPU per transaction. Neither server saturated its six cores (4.8 busy), so the 2-core client container or the round trip may bound both. The TPC-B-like script runs at 0.94× and costs pgrust 1.38× C's CPU per transaction. Neither side is CPU-bound there (2.0 and 2.6 cores busy), consistent with commits waiting for WAL flushes (lead, not measured), and pgrust's `io_method = sync` against C's `worker` is one of the differences in that path. `pgbench -i -s 100` takes the same time (6.31 against 6.42 s). These are single 60 s runs: a sanity check of pgrust as a PostgreSQL, not a tuning study.

## Soak

`report.py`'s soak tables, unedited:

| window | pg-c tps | pg-c p50 ms | pg-c p99 ms | pg-c clients | pgrust tps | pgrust p50 ms | pgrust p99 ms | pgrust clients |
|---|---|---|---|---|---|---|---|---|
| window-0 | 2242 | 2.96 | 9.69 | 8 | 1723 | 3.89 | 11.6 | 8 |
| window-1 | 2417 | 2.77 | 9.09 | 8 | 1828 | 3.69 | 11.1 | 8 |
| window-2 | 2301 | 2.93 | 9.45 | 8 | 1810 | 3.76 | 11.1 | 8 |
| window-3 | 2373 | 2.80 | 9.12 | 8 | 1741 | 3.90 | 11.2 | 8 |
| window-4 | 2253 | 2.97 | 9.40 | 8 | 1896 | 3.57 | 10.7 | 8 |
| window-5 | 2292 | 2.90 | 9.43 | 8 | 1772 | 3.84 | 11.2 | 8 |
| window-6 | 2244 | 2.96 | 9.61 | 8 | 1740 | 3.86 | 11.5 | 8 |
| window-7 | 2254 | 2.95 | 9.63 | 8 | 1836 | 3.70 | 10.9 | 8 |
| window-8 | 2246 | 2.92 | 9.89 | 8 | 1723 | 3.91 | 11.6 | 8 |
| window-9 | 2267 | 2.92 | 9.64 | 8 | 1935 | 3.47 | 10.6 | 8 |
| window-10 | 2222 | 2.98 | 9.67 | 8 | 1684 | 3.97 | 11.9 | 8 |
| window-11 | 2143 | 2.98 | 10.7 | 8 | 1783 | 3.72 | 11.5 | 8 |

| contestant | failed transactions | peak GiB | memory first GiB | memory max GiB | memory last GiB | samples |
|---|---|---|---|---|---|---|
| pg-c | 0 | 2.64 | 2.60 | 2.62 | 2.62 | 350 |
| pgrust | 0 | 4.58 | 3.60 | 4.53 | 4.27 | 352 |

The windows' latencies come from a 10% sample of the transaction log (`--sampling-rate 0.1`). Over the hour C served 8,170,960 transactions (2269.7 TPS) and pgrust 6,440,755 (1789.1 TPS, 0.79×), both with 0 failed (`*-soak.log`). Twelve 5-minute windows per side stay flat: C between 2143 and 2417 TPS with p99 9.09 to 10.7 ms, pgrust between 1684 and 1935 TPS with p99 10.6 to 11.9 ms. C's last window has its highest p99 (10.7 ms); it is one window. Memory over the hour is in Stability.

## Where pgrust is behind

Every ratio worse than 0.9× (or above 1.1× for time, CPU and memory), with its row above and what explains it, labelled.

- **IVFFlat builds: 3.65× and 4.84× C serially, 2.75× and 2.97× with 4 workers** (Builds), with CPU-seconds to match (3.68× and 4.95× serial; Resources). Lead: k-means is almost all distance computations, and C's kernels are vectorized (`-fassociative-math`, an FMA clone) where pgrust's keep C's source order element by element. Not profiled; the Mac's 2.25× to 2.29× (100,000 rows) was smaller.
- **HNSW builds: 1.90× to 2.13× C serially, 1.58× to 1.66× with 4 workers on `vector`; 2.74× and 2.19× on `halfvec`** (Headline; Builds), with CPU-seconds to match (1.93×, 2.18×, 2.80× serial; 1.62×, 1.71×, 2.23× with 4 workers; Resources). Leads: the same kernel vectorization difference, wider on x86-64 than on the Mac; for `halfvec` also the portable f16 route; and from a Mac profile of the HNSW build, `hashbrown::HashMap::insert` costing about as much as the distance kernel (`docs/HANDOFF_2026-11-05.md` §1). None of these is measured on x86-64.
- **Exact scan at 1 client: 0.51× and 0.47× on `vector`** (Exact scan). Cause, from the CPU-seconds: C's container is about 2.9 cores busy, consistent with its planner's 2 parallel workers (no `EXPLAIN` of the query was recorded), pgrust's 1.00, its planner building no parallel plans (an accepted divergence inherited from upstream v0.4-beta). Per CPU-second pgrust looks ahead here, a lead confounded by C's worker start-up and possible JIT (Exact scan). Whether the Mac's 1.45× exact-scan gap of 2026-11-08 had the same asymmetry is unknown: no plan of that scan was recorded.
- **Exact scan on `halfvec`: 0.24× at 1 client, 0.51× at 8; 1.44× and 1.94× C's CPU per query** (Exact scan). Lead: pgrust's portable f16→f32 route on x86-64 (no F16C) and its scalar summation order, against C's AVX/F16C/FMA kernels. C's worker start-up and possible JIT sit in its CPU per query, so the 1.44× understates the per-process gap.
- **HNSW search at 8 clients: 0.66× to 0.81× on `vector` (0.72× and 0.74× at the matched `ef_search` 100), 0.57× to 0.65× on `halfvec` (0.61× matched); p99 1.19× to 1.89× C's; 1.39×, 1.41× and 1.64× C's CPU per query at the matched `ef_search` 100 (glove `vector`, sift, `halfvec`; Resources)** (HNSW search). Lead, from the line fit: a fixed per-query CPU cost 1.84× to 2.06× C's (0.5 to 0.7 ms more per transaction) plus 1.19× to 1.22× per unit of `ef_search` on `vector` and 1.50× on `halfvec`. On `vector` the fixed part is the larger share of the gap at low `ef_search` and the per-ef part from about 200; on `halfvec` the per-ef part, the distance work, is the larger from `ef_search` 100. What the fixed part is spent on (planning, Gravity's start-up, the protocol, or index work that doesn't scale with `ef_search`) is not attributable from this data; profile both.
- **Search at 1 client: 0.63× to 0.89× on HNSW, 0.65× to 0.79× on IVFFlat** (the search tables). On HNSW the 1-client ratios are higher than the 8-client ones; on IVFFlat they are lower. That opposite direction is unexplained (the 1-client points also use another client and protocol; Method).
- **IVFFlat search at 8 clients: 0.73× to 0.85× (0.76× and 0.84× at the matched 20 probes), p99 up to 1.27×** (IVFFlat search). Lead: 1.16× to 1.17× the CPU per probe (distance work), plus the fixed part.
- **Recall-target cells as low as 0.40×** (sift IVFFlat ≥ 0.95). Cause: the threshold effect marked †; at the same setting the ratio is 0.76×. Not a separate gap.
- **Inserts: 0.85× (`halfvec`, 1 client), 0.88× (`halfvec`, 8 clients), 0.89× (sift, 1 client); 1.14× to 1.33× C's CPU per row** (Inserts). Lead: the HNSW insert's neighbour search is the same distance work. The rate ratios sit nearer 1 than the CPU ratios, consistent with commit waits dominating each insert (lead).
- **DELETE + VACUUM after inserts: 1.34× to 1.47× on `vector`, 2.77× to 2.84× on `halfvec`** (Inserts). Lead: HNSW vacuum's neighbour repair is distance work; the `halfvec` multiple tracks its build gap.
- **TPC-B-like CPU per transaction: 1.38×**, at 0.94× the TPS (Plain SQL). No lead; one 60 s run.
- **Soak: 0.79× C's TPS, p99 10.6 to 11.9 ms against C's 9.09 to 10.7** (Soak), the search share being the HNSW `ef_search` 100 gap above.
- **Memory held: pgrust's container sits higher in every step but the 4-worker HNSW builds** (Resources): 1.10× to 1.19× C's peak during loads, 1.29× during the serial `halfvec` build, 1.33× and 1.39× during the IVFFlat builds (sift, glove; Builds), 1.5× to 2.4× during search steps and 1.74× at the soak's peak (4.58 against 2.64 GiB). No cause is attributed. Candidates to measure: per-connection thread stacks (`RUST_MIN_STACK` = 32 MiB), memory retained by Gravity between statements, and how each side touches shared buffers.

## Where pgrust is ahead or equal

- **Recall:** the same at every search setting (within 0.0035 on HNSW and 0.0048 on IVFFlat) and in the exact scans.
- **Index sizes:** equal to the MiB on all 30 builds (`index_size` rows); they differ by up to 216 KiB (0.04%).
- **Plain SQL:** `pgbench -S` 1.10× C at 0.91× C's CPU per transaction; `pgbench -i` equal (0.98× C's time); TPC-B-like 0.94×.
- **Exact `vector` scan per CPU (lead):** 0.68× (glove) and 0.73× (sift) C's CPU per query at 1 client; 1.11× and 1.06× C's throughput at 8 clients, where both use all six cores. Confounded by C's parallel-worker start-up and possible JIT, which no recorded plan separates (Exact scan).
- **Load:** 1.01× on glove (median), 0.94× on sift.
- **Inserts on `vector`:** 0.92× to 0.95× on glove and 0.93× at 8 clients on sift, within the 0.9× band.
- **Parallel HNSW build memory:** pgrust peaks at 0.35× to 0.40× C's container peak (3.45 to 4.19 against 9.98 to 10.60 GiB), most likely because C's DSM reserves the whole `maintenance_work_mem`-sized area (Builds).
- **Parallel scaling of builds:** pgrust's 4-worker HNSW build is 3.23× to 3.64× faster than its serial one, C's 2.63× to 2.91×. This is consistent with pgrust's serial part being the slower one (lead), so it is not an advantage in itself.
- **Stability:** zero failed transactions over an hour on both sides, and no error rows anywhere in 175 rows each.

## Resources

CPU-seconds and container peaks, both sides, from the rows above (`cpu_usec` and `peak_bytes` in `pg-c.tsv` and `pgrust.tsv`):

| step | pg-c | pgrust | pgrust ÷ pg-c |
|---|---|---|---|
| serial HNSW build, CPU s (glove `vector` / `halfvec` / sift) | 373 / 437 / 267 | 719 / 1222 / 580 | 1.93× / 2.80× / 2.18× |
| 4-worker HNSW build, CPU s (same order) | 674 / 733 / 490 | 1091 / 1636 / 837 | 1.62× / 2.23× / 1.71× |
| serial IVFFlat build, CPU s (glove / sift) | 34.6 / 24.0 | 127 / 119 | 3.68× / 4.95× |
| HNSW search at matched `ef_search` 100, CPU ms per query (glove `vector` / `halfvec` / sift) | 2.45 / 2.33 / 2.00 | 3.40 / 3.83 / 2.81 | 1.39× / 1.64× / 1.41× |
| exact scan, CPU ms per query, 1 client (glove `vector` / `halfvec` / sift) | 236 / 224 / 214 | 160 / 323 / 157 | 0.68× / 1.44× / 0.73× |
| soak, CPU s over the hour | 20125 | 20538 | 1.02× (at 0.79× the transactions) |
| container peak during loads, GiB (glove / sift) | 1.12 / 2.55 | 1.33 / 2.80 | 1.19× / 1.10× |
| container peak, serial HNSW build, GiB (glove `vector` / `halfvec` / sift) | 3.17 / 3.53 / 3.70 | 3.44 / 4.57 / 3.97 | 1.08× / 1.29× / 1.07× |
| container peak, 4-worker HNSW build, GiB (same order) | 9.98 / 10.56 / 10.60 | 3.45 / 4.19 / 3.97 | 0.35× / 0.40× / 0.37× |
| container peak during search steps, GiB (range over all points) | 1.96–2.78 | 3.10–5.58 | – |
| soak memory, first / max / last, GiB | 2.60 / 2.62 / 2.62 | 3.60 / 4.53 / 4.27 | – |

- **What the memory numbers are.** `peak_bytes` is the container cgroup's `memory.peak` during the step: private memory of every server process plus the shared-memory pages touched (shared buffers and DSM segments). Its floor is whatever the container already held at the reset, so a step's peak carries the earlier steps' resident memory with it. The ZFS ARC, which holds the file data both servers read, is kernel memory outside every container's cgroup, so neither side's file cache counts. The build increment of the headline is defined in its footnote.
- **CPU** is the container's `usage_usec` over the step, every process and thread of the server, but not the client's.
- At 8 and 16 clients both servers keep 5.6 to 5.9 cores busy in every search step, so throughput ratios there are CPU-efficiency ratios. The matched-setting CPU per query row is the HNSW table's `ef_search` 100 points.

## Stability

- **Errors:** none. Both runs exited 0; neither TSV has a `*_error` or `plan_not_index` row; the soak's `soak_errors` row is 0 on both, as are pgbench's own counts (`*-soak.log`: `number of failed transactions: 0 (0.000%)`, per script too).
- **Latency over the hour:** flat on both (Soak); no window's p99 departs more than 12% from that side's mean (C 9.61 ms, pgrust 11.25 ms).
- **Memory over the hour** (`*-soak-rss.tsv`, 10 s samples of `memory.current`):

| contestant | failed transactions | window tps, min–max | window p99 ms, min–max | CPU s | memory first GiB | memory max GiB | mean memory by quarter hour GiB | memory last GiB |
|---|---|---|---|---|---|---|---|---|
| pg-c | 0 | 2143–2417 | 9.09–10.66 | 20125 | 2.60 | 2.62 | 2.61 / 2.61 / 2.62 / 2.62 | 2.62 |
| pgrust | 0 | 1684–1935 | 10.58–11.95 | 20538 | 3.60 | 4.53 | 4.35 / 4.36 / 4.33 / 4.32 | 4.27 |

  C rises by 0.02 GiB over the hour, steadily. That is consistent with shared buffers taking in the new rows' pages, but not traced. pgrust rises from 3.60 GiB to about 4.3 GiB in its first five minutes, then moves between 4.18 and 4.49 GiB (its maximum, 4.53 GiB, came about 290 s in) with quarter-hour means of 4.35, 4.36, 4.33 and 4.32 GiB: no growth after the first minutes, and a higher level than C's throughout.
- **VACUUM during the soak:** five on each side, every 10 minutes, each exit 0 in 0.13 to 0.24 s (the run logs' `soak: VACUUM items` lines). The soak only inserts, so these do little.

## Limits of this measurement

- **One node, one run.** Everything is from `borg2` in one campaign. Each search point is one 1-client recall pass and one 30 s `pgbench` run per client count. Builds, loads and exact passes have three reps; inserts, plain SQL and the soak have one.
- **A shared host.** The node also runs other Incus instances: `abbyerp` and `litellm2` were running on `borg2` when the facts were captured after the run (`facts/host-extra-2026-11-15.txt`, which lists the cluster's instances with their locations; the others run on `borg0` and `borg1`). Nothing reserves the database cores 0–5 or their SMT siblings 8–13 for the database container, so a neighbour can be scheduled on them. `settle` (`scripts/bench/lib.sh`) waited before each step for three 1-minute load readings below 1.0, 10 s apart. It never hit its 600 s cap. The run logs (`pg-c.log`, `pgrust.log`) record every reading. In the table, "wait beyond 20 s" is each call's readings beyond its first three times 10 s, with the median over all 53 calls (zeros included); "waits in which the load rose" counts the calls in which some reading exceeded the one before it by more than 0.05:

| contestant | settle calls | passed on the first 3 readings | waited longer | wait beyond 20 s: total / median / max | first reading: median / max | capped at 600 s | waits in which the load rose |
|---|---|---|---|---|---|---|---|
| pg-c | 53 | 10 | 43 | 2580 s / 10 s / 150 s | 1.14 / 9.58 | 0 | 1 |
| pgrust | 53 | 5 | 48 | 3060 s / 60 s / 150 s | 1.75 / 9.56 | 0 | 4 |

  Most of what settle waited out was the decay of the run's own load: every first reading of 5 or more (8 per side, up to 9.6) follows an 8- or 16-client step, a 4-worker build or, for pgrust's first, C's soak. The five calls in which the load rose during the wait show someone else's work. Settle checks the load only before a step, so a neighbour that started during a step went unseen. The CPU frequency (boost enabled, `lscpu`) was not pinned either, and depends on the whole package's load.
- **Warm cache, SMT siblings.** Every search point follows a warm-up pass; nothing was measured cold. The plan meant the database cores' SMT siblings (8–13) to stay idle; nothing reserved them (above).
- **The builds differ in origin.** C is PGDG's packaged build (GCC 13.3; pgvector's own run-time dispatch; whether it was built with `-march=native` is not recorded). pgrust was built from source in the container at the `release` profile (thin LTO, the default x86-64 target, no `target-cpu`).
- **The baseline is PostgreSQL 18.6**, the version pgrust ports. PGDG also ships 18.7, and the C side pins pgvector's `+1` build of 0.8.7 because the `+2` build needs 18.7. The client's `libpq5` is 18.7 (`psql` and `pgbench` 18.6), the same for both servers.
- **Sequential, not interleaved:** C ran from 23:36 to 05:05 UTC and pgrust from 05:05 to 13:26 UTC, one after the other, so they ran under different neighbour load (settle saw the load rise in 4 of pgrust's waits against 1 of C's).
- **Server differences not equalised or not recorded:**
  - Debian's `postgresql@18-main` unit has `OOMScoreAdjust=-900`, `pgrust.service` 0. No step approached the 24 GiB limit.
  - Cluster locale: C.UTF-8 on pg-c (from `pg_createcluster`), C on pgrust (`--no-locale`), per the `lc_*` rows of `*-settings.txt`. This is negligible for integer ids and vectors.
  - `jit`: the default `on` on both, but with different machinery (C's LLVM; pgrust's Gravity compiles through TPDE).
  - Data checksums: recorded on neither side.
- **IVFFlat `lists` = 1184 on glove** (the design's 1183; equal on both sides), 1000 on sift.
- **Memory semantics:** container high-water marks with each step's floor included; the build increment is a lower bound with no exact baseline in this run (no `base_bytes` lines); the ZFS ARC is outside both containers. Memory comparisons are approximate in both directions.
- **Recall ties:** recall is computed by id-set overlap, so exact-distance ties at the 10th neighbour count as misses (exact-scan recall 0.9993 to 0.9995 where it isn't 1.0000), and tie-breaking may differ between engines. Differences in the fourth decimal are noise. IVFFlat's recall comes from one build per side.
- **`io_method`:** pgrust runs `sync` (its requirement), C runs PostgreSQL 18's default `worker`. This touches every read and the WAL path (TPC-B-like, inserts).
- **No CHECKPOINT alignment:** `checkpoint_timeout = 30min` and `max_wal_size = 8GB` on both, but checkpoints were not forced before steps, so a checkpoint can land inside any step on either side.
- **The exact scan's parallelism is asymmetric** (C: a 2-worker parallel seq scan; pgrust: none, by upstream's planner). The 1-client exact ratios measure that, not the kernels; use the CPU-per-query column.
- **Client limits:** the 1-client QPS includes Python's per-query time; `pgbench -S` did not saturate either server, so the 2-core client may have bounded it.
- **Inserted rows duplicate indexed vectors** (`extra`), the same on both sides; they are deleted before any later recall pass.
- **One version of the orchestrator:** the run used `a2e6b06f3b`. The later commits (`574941a56d`, `0fa69a863f`, `d8974e4d81`) change how `report.py` presents the same TSV and how future runs log baselines and compute `lists`, not anything measured here.

## Reproduce

From the Mac, in this repo, with `ssh borg2` working. Another site must first change the constants and meet the prerequisites listed in `scripts/bench/README.md`, "Site constants and prerequisites" (each script's header documents its usage):

```bash
scripts/bench/sync.sh                                             # ship scripts/bench/ to borg2:~/bench/scripts/
ssh -n borg2 bash ~/bench/scripts/borg.sh create-db pgrust-db-c   # once; also pgrust-db-rust and: borg.sh create-client
ssh -n borg2 bash ~/bench/scripts/install-pg-c.sh                 # PGDG 18.6 + pgvector 0.8.7, common settings
ssh -n borg2 'PGRUST_REF=a9c5743a3ca35502680d990125eed590b176d50a bash ~/bench/scripts/install-pgrust.sh'   # pgrust at the measured sha (full 40 characters; default: main)
ssh -n borg2 bash ~/bench/scripts/client/setup.sh                 # psql/pgbench 18.6, the venv, the client scripts
ssh -n borg2 bash ~/bench/scripts/client/datasets.sh glove-100-angular sift-128-euclidean
ssh -n borg2 "sudo -n systemd-run --unit=bench-p1-RUN_ID --collect --uid=jody --gid=jody \
  -p WorkingDirectory=/home/jody/bench --setenv=HOME=/home/jody /bin/bash -c \
  'bash ~/bench/scripts/run-phase1.sh pg-c RUN_ID; bash ~/bench/scripts/run-phase1.sh pgrust RUN_ID'"   # 14 h
scp -r borg2:bench/results/RUN_ID ~/.cache/pgrust/bench-borg/
python3 scripts/bench/report.py ~/.cache/pgrust/bench-borg/RUN_ID
```

To repeat this run exactly, check out `a2e6b06f3b`'s `scripts/bench/` before `sync.sh`. The current scripts compute glove's `lists` as 1183 and log each step's `base_bytes`. `bash scripts/bench/tests/test_report.sh` checks `report.py`'s headline rules. The section tables that are not `report.py`'s were derived from the same two TSVs (medians of reps, CPU per query or row, the line fit); each table's note says how.

## Evidence

- **On the Mac:** `~/.cache/pgrust/bench-borg/p1-20261114/`, a copy of the host's directory:
  - `pg-c.tsv`, `pgrust.tsv` (175 rows each);
  - `*-version.txt`, `*-settings.txt` (non-default `pg_settings` and the GUC probes);
  - `*-explain-*.txt` (the sweeps' plans: `Index Scan using items_v_idx` on both sides, `items_h_v_idx` for `halfvec`);
  - `*-build-*.log` (`\timing` and the parallel-worker DEBUG lines);
  - `*-pgbench-*.log`;
  - `pg-c.log`, `pgrust.log` (the run logs with every `settle:` reading);
  - `*-soak.log`, `*-soak-stats.txt`, `*-soak-rss.tsv`, `*-soak-txlogs.tar.gz` (the soak's sampled transaction logs);
  - `facts/{pg-c,pgrust}/` (`host.txt` with `lscpu`, `containers.txt` with each container's Incus config, `zfs.txt`, `options.txt`, `REVISION`), and `facts/host-extra-2026-11-15.txt` (`free -g`, the OS release and `incus list` with locations, captured read-only after the run).
- `~/.cache/pgrust/bench-borg/dry1/` is the dry run (one build per cell, 5 s points) that preceded this run; it has the same measurement counts less the build reps and the soak. `~/.cache/pgrust/bench-borg/competitors-2026-11-14.md` is the research behind phase 2.
- **On the host:** `borg2:/home/jody/bench/results/p1-20261114/` (the original; nothing changed or deleted), `borg2:/home/jody/bench/results/dry1/`, the datasets in `borg2:/home/jody/bench/data/`.
- **Cluster state after the run:** `pgrust-db-c` and `pgrust-db-rust` stopped, `pgrust-client` running; all three kept for phase 2.

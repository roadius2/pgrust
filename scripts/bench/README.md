# scripts/bench: the borg benchmark campaign

Scripts that benchmark pgrust against C PostgreSQL 18.6 + pgvector 0.8.7 on
Linux x86-64, in matched Incus containers on the cluster node `borg2`
(plan: `docs/superpowers/plans/2026-11-14-borg-benchmark-phase1.md`).

Containers (Ubuntu noble, profile `trusted-lan`, `boot.autostart=false`, apt
upgrade timers disabled), made by `borg.sh create-db NAME` and `create-client`:

| Name             | CPUs (SMT pairs i, i+8) | Memory | Role                         |
|------------------|-------------------------|--------|------------------------------|
| `pgrust-db-c`    | 0-5                     | 24 GiB | C PostgreSQL + pgvector      |
| `pgrust-db-rust` | 0-5                     | 24 GiB | pgrust                       |
| `pgrust-client`  | 6,7,14,15               | 4 GiB  | load, recall, pgbench; `/data` |

Only one database container runs during a measurement (`borg.sh stop-others`).
`settle` (in `lib.sh`) waits for the host's 1-minute load to read below 1.0
three times 10 s apart, but at most 600 s; then it proceeds. `run-phase1.sh`
calls it before each load, each index build, the first point of each sweep,
each insert step, the plain-SQL step and the soak (not between consecutive
pgbench points).

Edit here, never on the host: `scripts/bench/sync.sh` replaces `~/bench/scripts/`
on borg2 with this directory (a tar pipe; the host has no rsync). Then run a
script there as a file: `ssh -n borg2 bash ~/bench/scripts/borg.sh ...`.
`incus launch` and `incus exec` read stdin when it is not a terminal, so every
incus call in these scripts takes `</dev/null` and none is piped over `ssh bash -s`.

Phase 1 (`run-phase1.sh CONTESTANT RUN_ID [--subset N] [--skip-soak]`) runs
for hours, so the full runs go under one transient host unit, pg-c then pgrust,
never overlapping:

    sudo -n systemd-run --unit=bench-p1-RUN_ID --collect --uid=jody --gid=jody \
      -p WorkingDirectory=/home/jody/bench --setenv=HOME=/home/jody /bin/bash -c \
      'bash ~/bench/scripts/run-phase1.sh pg-c RUN_ID; bash ~/bench/scripts/run-phase1.sh pgrust RUN_ID'

Each run logs to `results/RUN_ID/CONTESTANT.log`. `report.py RESULTS_DIR` prints
the markdown tables; `tests/test_report.sh` checks its headline rule.

On borg2: datasets in `/home/jody/bench/data` (read-only at `/data` in the
client), results in `/home/jody/bench/results/<run-id>/`, pulled to
`~/.cache/pgrust/bench-borg/<run-id>/` on the Mac.

## Site constants and prerequisites

**Constants to change on another site:**

- the cluster member `--target borg2` and the Incus profile `trusted-lan` (`borg.sh`'s `launch`);
- `/home/jody/bench` (`DATA_DIR` in `borg.sh` and `client/datasets.sh`, `RESULTS` in `run-phase1.sh`, the unit in this README);
- the client network `192.168.0.0/24` in `pg_hba` (`HBA_LINE` in both server installers);
- `BUILD_CPUS=0-15` for pgrust's build (`install-pgrust.sh`);
- the container pins: `0-5` for a database, `6,7,14,15` for the client. These assume a CPU whose SMT siblings are *i* and *i*+8.

**Host prerequisites:**

- `jq` and `python3`;
- passwordless `sudo`, for `lib.sh`'s memory.peak helper and `zfs get`;
- the `zfs` utilities, and `systemd-run` for the long runs;
- Incus 6.0 or later. Containers get `security.nesting=true`: without it Incus's AppArmor profile denies signals between the container's label and the stacked label that systemd-started processes carry, so PostgreSQL's AIO workers hang and servers cannot be stopped (design §11).

**Pinned versions:**

- PGDG `postgresql-18`, `postgresql-client-18` and the client's server package at `18.6-1.pgdg24.04+2`;
- `postgresql-18-pgvector` `0.8.7-1.pgdg24.04+1`;
- Rust 1.96.0 (`install-pgrust.sh`);
- the client venv's `h5py==3.16.0 numpy==2.5.4 psycopg[binary]==3.3.9`.

PGDG drops superseded builds from `apt.postgresql.org`, and they move to `apt-archive.postgresql.org`. When a pin stops resolving, add a second list line in each installer's `install_repo` and re-run it:

    deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] https://apt-archive.postgresql.org/pub/repos/apt noble-pgdg-archive main

**Entry points** (each script's header has its usage):

- `sync.sh`: ships this directory to `borg2:~/bench/scripts/`.
- `borg.sh`: creates, starts and stops the `pgrust-*` containers and writes `facts`.
- `install-pg-c.sh`: PGDG PostgreSQL 18.6 + pgvector 0.8.7 in `pgrust-db-c`.
- `install-pgrust.sh`: builds and serves pgrust in `pgrust-db-rust` (`PGRUST_REF` = a branch, a tag or a full 40-character sha).
- `client/setup.sh`: psql, pgbench and the venv in `pgrust-client`.
- `client/datasets.sh`: fetches the HDF5 datasets and checks them against `client/SHA256SUMS`.
- `run-phase1.sh`: one contestant's whole phase-1 run.
- `report.py`: a results directory to markdown tables.

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

#!/opt/bench/venv/bin/python
"""Summarize pgbench per-transaction logs (pgbench -l) in fixed time windows.

Reads every LOGFILE given (pgbench -l writes one per thread with -j > 1:
pgbench_log.PID and pgbench_log.PID.N) and prints one line per window, with
no header:
  window_start tps p50_ms p99_ms errors clients
  window_start  the window's offset in seconds from the start of the run (the
                earliest transaction's start)
  tps           the transactions that completed in the window, divided by
                --sampling-rate and by the window's length
  p50_ms p99_ms the latencies of those transactions, or "-" when the window
                has none
  errors        the failed transactions in the window, also divided by
                --sampling-rate, so it is on the same footing as tps (with
                --sampling-rate 1 it is the count)
  clients       the number of distinct client ids with a line in the window
The last window's length is the part of it the run covered; a last part
shorter than a tenth of a window (the transactions finishing as -T expires)
joins the window before it. Every script of a multi-script run
(-f search.pgbench@9 -f insert.pgbench@1) is counted together. Pass the run's
pgbench --sampling-rate.

pgbench logs a transaction as failed only for serialization and deadlock
errors ("failed", or "serialization"/"deadlock" with --failures-detailed). Any
other error (a refused statement, a cancel, a lost connection) aborts that
client, which then logs nothing more. So after printing the windows the script
exits 1, naming the clients, if any client's last line falls before the last
window: that client aborted. (With sampling, a client so slow that none of its
transactions in the last window was sampled would look the same.)

Log lines are "client_id transaction_no time script_no time_epoch time_us
[schedule_lag] [retries]", where time is the latency in microseconds, or
"skipped", or a failure as above; time_epoch.time_us is when the transaction
completed. Aggregated logs (--aggregate-interval) are not supported.
"""

import argparse
import math
import sys

import numpy

FAILURES = {"failed", "serialization", "deadlock"}


def parse_args():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("logfile", nargs="+", help="pgbench -l log files of one run")
    p.add_argument("--window", type=float, default=300.0, help="window seconds (default 300)")
    p.add_argument("--sampling-rate", type=float, default=1.0,
                   help="the run's pgbench --sampling-rate; tps and errors are divided "
                        "by it (default 1.0)")
    return p.parse_args()


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def read_logs(paths):
    """[(client id, completion s, latency ms or None for a failure)]; skipped ones are left out."""
    records = []
    for path in paths:
        try:
            f = open(path)
        except OSError as e:
            die(f"cannot read {path}: {e}")
        with f:
            for n, line in enumerate(f, 1):
                fields = line.split()
                try:
                    if len(fields) < 6:
                        raise ValueError
                    client, time, at = int(fields[0]), fields[2], int(fields[4]) + int(fields[5]) / 1e6
                    if time == "skipped":
                        continue
                    ms = None if time in FAILURES else int(time) / 1000.0
                except ValueError:
                    die(f"{path}:{n}: not a pgbench -l transaction line: {line.strip()!r}")
                records.append((client, at, ms))
    if not records:
        die("no transactions in the logs")
    return records


def main():
    args = parse_args()
    if args.window <= 0 or not 0 < args.sampling_rate <= 1:
        die("--window must be positive and --sampling-rate in (0, 1]")
    records = read_logs(args.logfile)
    start = min(at - (ms or 0) / 1000.0 for _, at, ms in records)
    total = max(at for _, at, _ in records) - start
    nwin = max(1, math.ceil(total / args.window))
    if nwin > 1 and total - (nwin - 1) * args.window < 0.1 * args.window:
        nwin -= 1  # a run's last few transactions are not a window of their own
    lat = [[] for _ in range(nwin)]
    errors = [0] * nwin
    clients = [set() for _ in range(nwin)]
    last = {}  # client id -> the window of its last line
    for client, at, ms in records:
        w = min(int((at - start) / args.window), nwin - 1)
        clients[w].add(client)
        last[client] = max(last.get(client, 0), w)
        if ms is None:
            errors[w] += 1
        else:
            lat[w].append(ms)
    for w in range(nwin):
        w_start = w * args.window
        length = args.window if w < nwin - 1 else total - w_start
        tps = len(lat[w]) / args.sampling_rate / length if length > 0 else 0.0
        if lat[w]:
            p50, p99 = (f"{x:.3f}" for x in numpy.percentile(lat[w], [50, 99]))
        else:
            p50 = p99 = "-"
        err = errors[w] / args.sampling_rate
        print(f"{w_start:.0f} {tps:.2f} {p50} {p99} {err:g} {len(clients[w])}")
    aborted = sorted(c for c, w in last.items() if w < nwin - 1)
    if aborted:
        sys.stdout.flush()
        die(f"client(s) {', '.join(map(str, aborted))} stopped logging before the last window "
            f"(window {nwin - 1}): aborted on an error?")


if __name__ == "__main__":
    main()

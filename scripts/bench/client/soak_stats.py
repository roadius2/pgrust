#!/opt/bench/venv/bin/python
"""Summarize pgbench per-transaction logs (pgbench -l) in fixed time windows.

Reads every LOGFILE given (pgbench -l writes one per thread with -j > 1:
pgbench_log.PID and pgbench_log.PID.N) and prints one line per window, with
no header:
  window_start tps p50_ms p99_ms errors
window_start is the window's offset in seconds from the start of the run (the
earliest transaction's start); tps counts the transactions that completed in
the window, divided by --sampling-rate (pass the run's pgbench
--sampling-rate) and by the window's length (for the last window, the part of
it the run covered; a last part shorter than a tenth of a window, the
transactions finishing as -T expires, joins the window before it); p50_ms and p99_ms are the latencies of those
transactions; errors counts the transactions pgbench logged as failed. Every
script of a multi-script run (-f a@9 -f b@1) is counted together.

Log lines are "client_id transaction_no time script_no time_epoch time_us
[schedule_lag] [retries]", where time is the latency in microseconds, or
"failed" or "skipped"; time_epoch.time_us is when the transaction completed.
Aggregated logs (--aggregate-interval) are not supported.
"""

import argparse
import math
import sys

import numpy


def parse_args():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("logfile", nargs="+", help="pgbench -l log files of one run")
    p.add_argument("--window", type=float, default=300.0, help="window seconds (default 300)")
    p.add_argument("--sampling-rate", type=float, default=1.0,
                   help="the run's pgbench --sampling-rate (default 1.0)")
    return p.parse_args()


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def read_logs(paths):
    """Returns (done, failed): done = [(completion s, latency ms)], failed = [completion s]."""
    done, failed = [], []
    for path in paths:
        try:
            f = open(path)
        except OSError as e:
            die(f"cannot read {path}: {e}")
        with f:
            for n, line in enumerate(f, 1):
                fields = line.split()
                if len(fields) < 6:
                    die(f"{path}:{n}: not a pgbench -l transaction line: {line.strip()!r}")
                try:
                    at = int(fields[4]) + int(fields[5]) / 1e6
                except ValueError:
                    die(f"{path}:{n}: not a pgbench -l transaction line: {line.strip()!r}")
                if fields[2] == "failed":
                    failed.append(at)
                elif fields[2] != "skipped":
                    done.append((at, int(fields[2]) / 1000.0))
    if not done and not failed:
        die("no transactions in the logs")
    return done, failed


def main():
    args = parse_args()
    if args.window <= 0 or not 0 < args.sampling_rate <= 1:
        die("--window must be positive and --sampling-rate in (0, 1]")
    done, failed = read_logs(args.logfile)
    start = min([at - ms / 1000.0 for at, ms in done] + failed)
    end = max([at for at, _ in done] + failed)
    total = end - start
    nwin = max(1, math.ceil(total / args.window))
    if nwin > 1 and total - (nwin - 1) * args.window < 0.1 * args.window:
        nwin -= 1  # a run's last few transactions are not a window of their own
    lat = [[] for _ in range(nwin)]
    errors = [0] * nwin
    for at, ms in done:
        lat[min(int((at - start) / args.window), nwin - 1)].append(ms)
    for at in failed:
        errors[min(int((at - start) / args.window), nwin - 1)] += 1
    for w in range(nwin):
        w_start = w * args.window
        length = args.window if w < nwin - 1 else total - w_start
        tps = len(lat[w]) / args.sampling_rate / length if length > 0 else 0.0
        p50, p99 = numpy.percentile(lat[w], [50, 99]) if lat[w] else (math.nan, math.nan)
        print(f"{w_start:.0f} {tps:.2f} {p50:.3f} {p99:.3f} {errors[w]}")


if __name__ == "__main__":
    main()

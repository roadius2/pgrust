#!/usr/bin/env python3
"""Markdown tables from a borg benchmark run directory.

  report.py RESULTS_DIR

Reads every CONTESTANT.tsv in RESULTS_DIR (run-phase1.sh's rows; the file's
stem names the contestant: pg-c, pgrust) and prints:

  (a) a headline table per dataset and vector type: HNSW serial and parallel
      build medians, QPS at recall >= 0.95 and >= 0.99 at 8 and 16 clients
      (HNSW and IVFFlat), exact-scan QPS, insert rows/s at 8 clients, and the
      peak memory and CPU-seconds of the serial HNSW build;
  (b) one table per measurement with every point;
  (c) the soak windows, the soak's failed transactions and its memory over
      the hour (CONTESTANT-soak-rss.tsv).

The "QPS at recall >= R" rule: the first setting of the sweep (ef_search or
probes, ascending) whose single-client recall pass reaches R; the cell is that
setting's 8- (or 16-) client QPS, with the setting named. No interpolation; a
sweep that never reaches R prints "not reached". Ratios are the second
contestant's value divided by the first's (pgrust / pg-c), as x.xx×.
Medians are of the repetitions in the TSV. Standard library only.
"""

import csv
import os
import statistics
import sys

HEADER = ("dataset type index measurement setting clients rep value unit recall "
          "p50_ms p95_ms p99_ms peak_bytes cpu_usec wall_s started_at").split()
ORDER = ["pg-c", "pgrust"]  # contestants first in this order, others after
GUC = {"hnsw": "ef_search", "ivfflat": "probes"}
SOAK = {"soak", "soak_errors"}


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def num(s):
    try:
        return float(s)
    except (TypeError, ValueError):
        return None


def load(directory):
    """{contestant: [row dict]}"""
    runs = {}
    for name in sorted(os.listdir(directory)):
        if not name.endswith(".tsv") or name.endswith("-soak-rss.tsv"):
            continue
        path = os.path.join(directory, name)
        with open(path, newline="") as f:
            reader = csv.reader(f, delimiter="\t")
            header = next(reader, None)
            if header != HEADER:
                print(f"warning: {path}: not a run-phase1.sh TSV, skipped", file=sys.stderr)
                continue
            runs[name[:-4]] = [dict(zip(HEADER, r + [""] * (len(HEADER) - len(r))))
                               for r in reader if r]
    if not runs:
        die(f"no run-phase1.sh TSV in {directory}")
    return runs


def contestants(runs):
    return sorted(runs, key=lambda c: (ORDER.index(c) if c in ORDER else len(ORDER), c))


def fmt(v):
    if v is None:
        return "–"
    a = abs(v)
    if a >= 100:
        return f"{v:.0f}"
    if a >= 10:
        return f"{v:.1f}"
    if a >= 1:
        return f"{v:.2f}"
    return f"{v:.3g}"


def ratio(a, b):
    """b / a as x.xx×, or – ."""
    if a is None or b is None or a == 0:
        return "–"
    return f"{b / a:.2f}×"


def gib(b):
    return None if b is None else b / 2**30


def table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    out += ["| " + " | ".join(r) + " |" for r in rows]
    return "\n".join(out)


def select(rows, **kw):
    return [r for r in rows if all(r[k] == v for k, v in kw.items())]


def median(rows, field="value"):
    vals = [num(r[field]) for r in rows if num(r[field]) is not None]
    return statistics.median(vals) if vals else None


def at_recall(rows, ds, ty, index, target, clients):
    """(QPS, setting) at the first sweep setting whose 1-client recall >= target.

    None when the sweep has no 1-client points; (None, None) when it never
    reaches the target; (None, setting) when that setting's point at
    `clients` is missing (an error row instead).
    """
    single = [r for r in select(rows, dataset=ds, type=ty, index=index, measurement="search",
                                clients="1") if num(r["recall"]) is not None]
    if not single:
        return None
    single.sort(key=lambda r: num(r["setting"]))
    for r in single:
        if num(r["recall"]) >= target:
            point = select(rows, dataset=ds, type=ty, index=index, measurement="search",
                           setting=r["setting"], clients=str(clients))
            return (num(point[0]["value"]) if point else None), r["setting"]
    return None, None


def headline(runs, cs):
    groups = []
    for c in cs:
        for r in runs[c]:
            key = (r["dataset"], r["type"])
            if r["dataset"] != "pgbench" and r["measurement"] not in SOAK and key not in groups:
                groups.append(key)
    out = ["## Headline", ""]
    two = len(cs) >= 2
    head = ["metric"] + cs + ([f"{cs[1]} ÷ {cs[0]}"] if two else [])
    for ds, ty in groups:
        lines = []

        def simple(label, value_of):
            vals = [value_of(runs[c]) for c in cs]
            if all(v is None for v in vals):
                return
            lines.append([label] + [fmt(v) for v in vals] + ([ratio(vals[0], vals[1])] if two else []))

        def build(setting, field="value", scale=lambda v: v):
            def f(rows):
                m = median(select(rows, dataset=ds, type=ty, index="hnsw", measurement="build",
                                  setting=setting), field)
                return None if m is None else scale(m)
            return f

        simple("HNSW serial build, median ms", build("serial"))
        simple("HNSW parallel build (4 workers), median ms", build("parallel4"))
        for index in ("hnsw", "ivfflat"):
            for target in (0.95, 0.99):
                for clients in (8, 16):
                    res = [at_recall(runs[c], ds, ty, index, target, clients) for c in cs]
                    if all(x is None for x in res):
                        continue
                    cells = []
                    for x in res:
                        if x is None:
                            cells.append("–")
                        elif x[1] is None:
                            cells.append("not reached")
                        elif x[0] is None:
                            cells.append(f"no point ({GUC[index]} {x[1]})")
                        else:
                            cells.append(f"{fmt(x[0])} ({GUC[index]} {x[1]})")
                    vals = [x[0] if x else None for x in res]
                    lines.append([f"{index.upper() if index == 'hnsw' else 'IVFFlat'} QPS at "
                                  f"recall ≥ {target:.2f}, {clients} clients"] + cells
                                 + ([ratio(vals[0], vals[1])] if two else []))
        simple("Exact scan QPS, 1 client, median",
               lambda rows: median(select(rows, dataset=ds, type=ty, measurement="exact", clients="1")))
        simple("Exact scan TPS, 8 clients",
               lambda rows: median(select(rows, dataset=ds, type=ty, measurement="exact", clients="8")))
        simple("HNSW inserts, rows/s, 8 clients",
               lambda rows: median(select(rows, dataset=ds, type=ty, index="hnsw",
                                          measurement="insert", clients="8")))
        simple("Peak memory of the serial HNSW build, median GiB", build("serial", "peak_bytes", gib))
        simple("CPU-seconds of the serial HNSW build, median", build("serial", "cpu_usec", lambda v: v / 1e6))
        if lines:
            out += [f"### {ds} / {ty}", "", table(head, lines), ""]
    return out


def flags(runs, cs):
    """Error and plan rows, listed before the tables."""
    lines = []
    for c in cs:
        for r in runs[c]:
            if r["measurement"].endswith("_error") or r["measurement"] == "plan_not_index":
                lines.append(f"- {c}: {r['measurement']} {r['dataset']} {r['type']} {r['index']} "
                             f"setting={r['setting']} clients={r['clients']} rep={r['rep']} "
                             f"value={r['value']} ({r['started_at']})")
    return (["## Errors and flags", ""] + lines + [""]) if lines else []


# (field, column label, scale, formatter) shown per contestant when present
EXTRA = [
    ("recall", "recall", lambda v: v, lambda v: "–" if v is None else f"{v:.4f}"),
    ("p50_ms", "p50 ms", lambda v: v, fmt),
    ("p95_ms", "p95 ms", lambda v: v, fmt),
    ("p99_ms", "p99 ms", lambda v: v, fmt),
    ("peak_bytes", "peak GiB", gib, lambda v: "–" if v is None else f"{v:.2f}"),
    ("cpu_usec", "CPU s", lambda v: v / 1e6, fmt),
    ("wall_s", "wall s", lambda v: v, fmt),
]
KEY = ("dataset", "type", "index", "setting", "clients", "rep")


def measurements(runs, cs):
    names = []
    for c in cs:
        for r in runs[c]:
            if r["measurement"] not in SOAK and r["measurement"] not in names:
                names.append(r["measurement"])
    out = ["## All points", ""]
    two = len(cs) >= 2
    for m in names:
        keys, by = [], {}
        for c in cs:
            for r in select(runs[c], measurement=m):
                k = tuple(r[f] for f in KEY)
                if k not in by:
                    keys.append(k)
                    by[k] = {}
                by[k][c] = r
        units = {r["unit"] for k in keys for r in by[k].values()}
        to_mib = units == {"bytes"}
        unit = "MiB" if to_mib else ", ".join(sorted(units))
        extra = [e for e in EXTRA
                 if any(num(r[e[0]]) is not None for k in keys for r in by[k].values())]
        head = list(KEY) + [f"{c} {unit}" for c in cs] + ([f"{cs[1]} ÷ {cs[0]}"] if two else [])
        for _, label, _, _ in extra:
            head += [f"{c} {label}" for c in cs]
        lines = []
        for k in keys:
            vals = [num(by[k][c]["value"]) if c in by[k] else None for c in cs]
            if to_mib:
                vals = [None if v is None else v / 2**20 for v in vals]
            line = list(k) + [fmt(v) for v in vals] + ([ratio(vals[0], vals[1])] if two else [])
            for field, _, scale, f in extra:
                for c in cs:
                    v = num(by[k][c][field]) if c in by[k] else None
                    line.append(f(None if v is None else scale(v)))
            lines.append(line)
        out += [f"### {m}", "", table(head, lines), ""]
    return out


def soak(runs, cs, directory):
    rows = {c: select(runs[c], measurement="soak") for c in cs}
    if not any(rows.values()):
        return ["## Soak", "", "No soak rows.", ""]
    windows = []
    for c in cs:
        for r in rows[c]:
            if r["setting"] not in windows:
                windows.append(r["setting"])
    head = ["window"]
    for c in cs:
        head += [f"{c} tps", f"{c} p50 ms", f"{c} p99 ms", f"{c} clients"]
    lines = []
    for w in windows:
        line = [w]
        for c in cs:
            r = select(rows[c], setting=w)
            line += ([fmt(num(r[0]["value"])), fmt(num(r[0]["p50_ms"])), fmt(num(r[0]["p99_ms"])),
                      r[0]["clients"]] if r else ["–"] * 4)
        lines.append(line)
    out = ["## Soak", "", table(head, lines), ""]
    summary = []
    for c in cs:
        e = select(runs[c], measurement="soak_errors")
        mem = []
        path = os.path.join(directory, f"{c}-soak-rss.tsv")
        if os.path.exists(path):
            with open(path) as f:
                mem = [num(line.split("\t")[1]) for line in f.read().splitlines()[1:] if "\t" in line]
                mem = [m for m in mem if m is not None]
        summary.append([c, e[0]["value"] if e else "–",
                        fmt(num(e[0]["peak_bytes"]) / 2**30) if e and num(e[0]["peak_bytes"]) else "–",
                        f"{mem[0] / 2**30:.2f}" if mem else "–",
                        f"{max(mem) / 2**30:.2f}" if mem else "–",
                        f"{mem[-1] / 2**30:.2f}" if mem else "–",
                        str(len(mem))])
    out += [table(["contestant", "failed transactions", "peak GiB", "memory first GiB",
                   "memory max GiB", "memory last GiB", "samples"], summary), ""]
    return out


def main():
    if len(sys.argv) != 2:
        die("usage: report.py RESULTS_DIR")
    directory = sys.argv[1]
    runs = load(directory)
    cs = contestants(runs)
    out = [f"# {os.path.basename(os.path.normpath(directory))}: {', '.join(cs)}", ""]
    out += headline(runs, cs)
    out += flags(runs, cs)
    out += measurements(runs, cs)
    out += soak(runs, cs, directory)
    print("\n".join(out))


if __name__ == "__main__":
    main()

#!/opt/bench/venv/bin/python
"""Measure recall@k and single-client latency of a k-NN search.

For the first N test queries (queries.id 0..N-1) it runs
  SELECT id FROM TABLE ORDER BY COL OP '<query vector>' LIMIT k
once as a warm-up pass and then once timed, one query at a time on one
connection. The query vector is queries.v (for items) or queries.vh (for
items_h) as text, written into the statement as a quoted literal, so the SQL
is the same text search.pgbench sends. Recall is computed here, against
truth.nn[1:k] from the dataset's ground truth, never taken from the engine;
it is meaningful only on a full load (load.py without --subset).

--guc NAME=VALUE (repeatable) is applied with SET before the warm-up and
checked with SHOW; --exact adds enable_indexscan = off, for exact-scan passes
before any index exists.

Prints one JSON line:
  {"recall": r, "n": N, "qps": q, "p50_ms": ..., "p95_ms": ..., "p99_ms": ...,
   "seconds": s}
where s is the timed pass's wall time, q = N / s and the percentiles are of
the timed pass's per-query latencies.
"""

import argparse
import json
import re
import sys
import time

import numpy
import psycopg
from psycopg import sql

GUC_NAME = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)?$")
QUERY_COLUMN = {"items": "v", "items_h": "vh"}  # TABLE -> the queries column


def parse_args():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("dsn", help="e.g. postgresql://bench:bench@HOST/bench?sslmode=disable")
    p.add_argument("--table", choices=sorted(QUERY_COLUMN), required=True)
    p.add_argument("--col", default="v", help="the vector column of TABLE (default v)")
    p.add_argument("--op", choices=["<=>", "<->"], required=True,
                   help="<=> cosine distance, <-> L2 distance")
    p.add_argument("--exact", action="store_true", help="SET enable_indexscan = off")
    p.add_argument("--queries", type=int, default=10000, metavar="N",
                   help="number of test queries, ids 0..N-1 (default 10000)")
    p.add_argument("--k", type=int, default=10, help="neighbours per query (default 10)")
    p.add_argument("--guc", action="append", default=[], metavar="NAME=VALUE",
                   help="a setting to SET first, e.g. hnsw.ef_search=100 (repeatable)")
    return p.parse_args()


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def parse_gucs(args):
    gucs = [("enable_indexscan", "off")] if args.exact else []
    for g in args.guc:
        name, sep, value = g.partition("=")
        if not sep or not GUC_NAME.match(name):
            die(f"--guc '{g}' is not NAME=VALUE")
        gucs.append((name, value))
    return gucs


def apply_gucs(conn, gucs):
    for name, value in gucs:
        conn.execute(sql.SQL("SET {} = {}").format(sql.SQL(name), sql.Literal(value)))
        shown = conn.execute(sql.SQL("SHOW {}").format(sql.SQL(name))).fetchone()[0]
        if shown != value:
            die(f"SET {name} = {value} did not take: SHOW {name} prints {shown}")


def fetch_queries(conn, qcol, n, k):
    """[(query vector text, true neighbour ids)] for query ids 0..n-1."""
    rows = conn.execute(
        sql.SQL("SELECT q.id, q.{}::text, t.nn[1:%s] FROM queries q JOIN truth t ON t.qid = q.id"
                " WHERE q.id < %s ORDER BY q.id").format(sql.Identifier(qcol)),
        (k, n),
    ).fetchall()
    if len(rows) != n:
        die(f"wanted {n} queries with ground truth, the tables hold {len(rows)}")
    for _, _, nn in rows:
        if len(nn) < k:
            die(f"truth holds {len(nn)} neighbours per query, fewer than --k {k}")
    return [(vec, set(nn)) for _, vec, nn in rows]


def search_sql(args, vec):
    return sql.SQL("SELECT id FROM {} ORDER BY {} {} {} LIMIT {}").format(
        sql.Identifier(args.table), sql.Identifier(args.col), sql.SQL(args.op),
        sql.Literal(vec), sql.Literal(args.k))


def run_pass(conn, statements):
    """Run each statement; returns (result id lists, latencies in ms, wall seconds)."""
    results, latencies = [], []
    start = time.perf_counter()
    for stmt in statements:
        t = time.perf_counter()
        ids = [r[0] for r in conn.execute(stmt, prepare=False).fetchall()]
        latencies.append((time.perf_counter() - t) * 1000)
        results.append(ids)
    return results, latencies, time.perf_counter() - start


def main():
    args = parse_args()
    if args.queries <= 0 or args.k <= 0:
        die("--queries and --k must be positive")
    gucs = parse_gucs(args)
    try:
        with psycopg.connect(args.dsn, autocommit=True) as conn:
            apply_gucs(conn, gucs)
            queries = fetch_queries(conn, QUERY_COLUMN[args.table], args.queries, args.k)
            statements = [search_sql(args, vec).as_string(conn) for vec, _ in queries]
            run_pass(conn, statements)  # warm-up
            results, latencies, seconds = run_pass(conn, statements)
    except psycopg.Error as e:
        state = f" [{e.sqlstate}]" if e.sqlstate else ""
        die(f"{type(e).__name__}{state}: {str(e).strip()}")
    hits = sum(len(truth & set(ids)) for (_, truth), ids in zip(queries, results))
    p50, p95, p99 = numpy.percentile(latencies, [50, 95, 99])
    print(json.dumps({
        "recall": round(hits / (args.k * args.queries), 6),
        "n": args.queries,
        "qps": round(args.queries / seconds, 2),
        "p50_ms": round(p50, 3),
        "p95_ms": round(p95, 3),
        "p99_ms": round(p99, 3),
        "seconds": round(seconds, 3),
    }), flush=True)


if __name__ == "__main__":
    main()

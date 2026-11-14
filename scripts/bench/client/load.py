#!/opt/bench/venv/bin/python
"""Load an ann-benchmarks HDF5 dataset into a benchmark server.

Creates, dropping any previous copy:
  items   (id int, v vector(D))            train rows; id = 0-based row index
  items_h (id int, v halfvec(D))           the same rows (only with --halfvec)
  queries (id int primary key, v vector(D), vh halfvec(D))   the test rows
  truth   (qid int, nn int[])              each test row's 100 true neighbours
                                           (0-based train row indices)
  extra   (id int primary key, v vector(D))  train rows 0..99999 with ids
                                           1..100000; insert.pgbench inserts
                                           extra row i into items as id
                                           i + 100000000

queries and extra are looked up by id once per pgbench transaction, so they
have a primary key: without one each lookup would be a sequential scan of the
whole table, and the search and insert rates would measure that scan.

Every table is filled with COPY ... FROM STDIN (text format), one COPY per
batch of 10,000 rows, in autocommit. The vectors are formatted with %.9g,
which round-trips every float32 exactly. Three worker processes format all of
a table's batches before its first COPY (about 1.5 GB of text for glove's
items), so the timed COPYs measure the server and the network, not the
client's formatting, which is no faster than about 100,000 rows/s here.

Prints one JSON line on stdout for items:
  {"rows": N, "seconds": S, "rows_per_s": R}
where S is the wall time of items' COPYs; progress for every table goes to
stderr.

--subset N loads only the first N train rows into items and items_h, for
pipeline tests. truth is still the full dataset's, so a recall measured on a
subset load is meaningless; measure recall only on a full load.
"""

import argparse
import json
import multiprocessing
import sys
import time

import h5py
import psycopg

BATCH = 10_000
EXTRA_ROWS = 100_000
EXTRA_ID_OFFSET = 100_000_000  # insert.pgbench's id + 100000000
FORMAT_WORKERS = 3
METRICS = {"cosine": "angular", "l2": "euclidean"}  # --metric -> HDF5 "distance"

_h5 = None  # the open HDF5 file in a formatting worker


def parse_args():
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    p.add_argument("dsn", help="e.g. postgresql://bench:bench@HOST/bench?sslmode=disable")
    p.add_argument("hdf5", help="ann-benchmarks file, e.g. /data/glove-100-angular.hdf5")
    p.add_argument("--dim", type=int, required=True, help="vector dimension D")
    p.add_argument("--metric", choices=sorted(METRICS), required=True,
                   help="checked against the file's distance attribute")
    p.add_argument("--halfvec", action="store_true", help="also create and load items_h")
    p.add_argument("--subset", type=int, metavar="N",
                   help="load only the first N train rows (pipeline tests only)")
    return p.parse_args()


def die(msg):
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def check_file(f, dim, metric):
    for name in ("train", "test", "neighbors"):
        if name not in f:
            die(f"{f.filename} has no '{name}' dataset")
    distance = f.attrs.get("distance")
    if distance is not None and distance != METRICS[metric]:
        die(f"{f.filename} is a '{distance}' dataset, not {metric}")
    for name in ("train", "test"):
        if f[name].shape[1] != dim:
            die(f"{f.filename}: {name} has dimension {f[name].shape[1]}, not --dim {dim}")


def _open_worker(path):
    global _h5
    _h5 = h5py.File(path, "r")


def format_batch(job):
    """One COPY batch of text rows: job = (kind, dataset, lo, hi, id_base)."""
    kind, dataset, lo, hi, id_base = job
    rows = _h5[dataset][lo:hi].tolist()
    vec = "[" + ",".join(["%.9g"] * _h5[dataset].shape[1]) + "]"
    if kind == "vec":  # id, v
        out = ["%d\t%s\n" % (id_base + i, vec % tuple(r)) for i, r in enumerate(rows)]
    elif kind == "query":  # id, v, vh
        out = []
        for i, r in enumerate(rows):
            v = vec % tuple(r)
            out.append("%d\t%s\t%s\n" % (id_base + i, v, v))
    else:  # truth: qid, nn
        out = ["%d\t{%s}\n" % (id_base + i, ",".join(map(str, r))) for i, r in enumerate(rows)]
    return "".join(out).encode()


def copy_table(conn, pool, table, columns, kind, dataset, nrows, id_base=0):
    """COPY nrows rows of dataset into table, one COPY per batch.

    Returns the seconds the COPYs took: the batches are formatted first, so
    the time is the server's and the network's, not the client's formatting.
    """
    jobs = [(kind, dataset, lo, min(lo + BATCH, nrows), id_base + lo)
            for lo in range(0, nrows, BATCH)]
    batches = pool.map(format_batch, jobs)
    start = time.perf_counter()
    with conn.cursor() as cur:
        for data in batches:
            with cur.copy(f"COPY {table} ({columns}) FROM STDIN") as cp:
                cp.write(data)
    seconds = time.perf_counter() - start
    log(f"{table}: {nrows} rows in {seconds:.3f} s ({nrows / seconds:.0f} rows/s)")
    return seconds


def create_tables(conn, dim, halfvec):
    ddl = [
        "DROP TABLE IF EXISTS items, items_h, queries, truth, extra",
        f"CREATE TABLE items (id int, v vector({dim}))",
        f"CREATE TABLE queries (id int PRIMARY KEY, v vector({dim}), vh halfvec({dim}))",
        "CREATE TABLE truth (qid int, nn int[])",
        f"CREATE TABLE extra (id int PRIMARY KEY, v vector({dim}))",
    ]
    if halfvec:
        ddl.append(f"CREATE TABLE items_h (id int, v halfvec({dim}))")
    for stmt in ddl:
        conn.execute(stmt)


def load(args, f, conn, pool):
    ntrain = f["train"].shape[0]
    nrows = ntrain if args.subset is None else min(args.subset, ntrain)
    create_tables(conn, args.dim, args.halfvec)
    seconds = copy_table(conn, pool, "items", "id, v", "vec", "train", nrows)
    if args.halfvec:
        copy_table(conn, pool, "items_h", "id, v", "vec", "train", nrows)
    copy_table(conn, pool, "queries", "id, v, vh", "query", "test", f["test"].shape[0])
    copy_table(conn, pool, "truth", "qid, nn", "truth", "neighbors", f["neighbors"].shape[0])
    copy_table(conn, pool, "extra", "id, v", "vec", "train", min(EXTRA_ROWS, ntrain), id_base=1)
    return {"rows": nrows, "seconds": round(seconds, 3), "rows_per_s": round(nrows / seconds, 1)}


def main():
    args = parse_args()
    if args.subset is not None and args.subset <= 0:
        die("--subset must be positive")
    try:
        f = h5py.File(args.hdf5, "r")
    except OSError as e:
        die(f"cannot open {args.hdf5}: {e}")
    with f:
        check_file(f, args.dim, args.metric)
        try:
            with psycopg.connect(args.dsn, autocommit=True) as conn, \
                    multiprocessing.Pool(FORMAT_WORKERS, _open_worker, (args.hdf5,)) as pool:
                result = load(args, f, conn, pool)
        except psycopg.Error as e:
            state = f" [{e.sqlstate}]" if e.sqlstate else ""
            die(f"{type(e).__name__}{state}: {str(e).strip()}")
    print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()

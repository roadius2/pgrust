-- Parallel IVFFlat and HNSW builds against C (M6). Workers are forced the
-- way pgvector's TAP tests force them (min_parallel_table_scan_size = 1, or
-- the parallel_workers reloption). sqldiff drops the per-participant
-- "leader/worker processed N tuples" lines and every "CONTEXT:  parallel
-- worker" line; what is compared is index.c's request line, pgvector's
-- "using N parallel workers", NOTICEs, errors and query results. DEBUG1 is on
-- only around the builds.

SET min_parallel_table_scan_size = 1;
SET enable_seqscan = off;

-- IVFFlat ---------------------------------------------------------------------
-- 20000 rows; the planner allows one worker at the default 64MB.
CREATE TABLE ivf (id int, v vector(3));
INSERT INTO ivf SELECT i, ARRAY[i % 7, (i * 3) % 11, (i * 5) % 13] FROM generate_series(1, 20000) i;
SET client_min_messages = debug1;
CREATE INDEX ivf_l2 ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
-- Every row is in some list: probing every list returns every row.
SET ivfflat.probes = 10;
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_l2;

-- The reloption skips the memory clamp: two workers even at 4MB
-- (planner.c:7193-7205).
ALTER TABLE ivf SET (parallel_workers = 2);
SET maintenance_work_mem = '4MB';
SET client_min_messages = debug1;
CREATE INDEX ivf_rel ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
RESET maintenance_work_mem;
ALTER TABLE ivf RESET (parallel_workers);
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_rel;

-- Review Focus 2: CREATE INDEX CONCURRENTLY with workers.
SET client_min_messages = debug1;
CREATE INDEX CONCURRENTLY ivf_cic ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
SELECT count(*) FROM (SELECT id FROM ivf ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP INDEX ivf_cic;

-- Review Focus 5: builds that stay serial.
-- Under 64MB of maintenance_work_mem the planner allows no worker
-- (planner.c:7220-7232).
SET maintenance_work_mem = '32MB';
SET client_min_messages = debug1;
CREATE INDEX ivf_mem ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
RESET maintenance_work_mem;
DROP INDEX ivf_mem;
-- parallel_workers = 0 (planner.c:7193-7205).
ALTER TABLE ivf SET (parallel_workers = 0);
SET client_min_messages = debug1;
CREATE INDEX ivf_zero ON ivf USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
ALTER TABLE ivf RESET (parallel_workers);
DROP INDEX ivf_zero;
-- A temp table (planner.c:7185).
CREATE TEMP TABLE ivf_temp AS SELECT * FROM ivf;
SET client_min_messages = debug1;
CREATE INDEX ivf_temp_l2 ON ivf_temp USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
DROP TABLE ivf_temp;
-- A parallel-unsafe index expression (planner.c:7186-7190); PL/pgSQL so
-- neither server inlines it away.
CREATE FUNCTION unsafe_v(v vector) RETURNS vector LANGUAGE plpgsql IMMUTABLE PARALLEL UNSAFE
  AS $$ BEGIN RETURN v; END $$;
SET client_min_messages = debug1;
CREATE INDEX ivf_unsafe ON ivf USING ivfflat (unsafe_v(v) vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
DROP INDEX ivf_unsafe;
-- An unlogged table: the main fork builds in parallel, the init fork
-- serially (ivfbuild.c:1094-1097).
CREATE UNLOGGED TABLE ivf_unlogged AS SELECT * FROM ivf;
SET client_min_messages = debug1;
CREATE INDEX ivf_unlogged_l2 ON ivf_unlogged USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
SELECT count(*) FROM (SELECT id FROM ivf_unlogged ORDER BY v <-> '[3,5,7]' LIMIT 20000) s;
DROP TABLE ivf_unlogged;
-- Backpressure: two workers stream wide values (vector(512), stored out of
-- line) while the leader's tuplesort spills, so the workers' mailboxes fill
-- and each worker waits for the leader. IVFFlat's sample buffer alone needs
-- 20MB (ivfutils.c:126-134), so 24MB is the smallest round size that builds;
-- the sorted values (about 41MB) still overflow it.
CREATE TABLE ivf_wide (id int, v vector(512));
INSERT INTO ivf_wide SELECT i, (array_fill((i % 97)::real, ARRAY[256]) || array_fill((i % 89)::real, ARRAY[256]))::vector(512)
  FROM generate_series(1, 20000) i;
ALTER TABLE ivf_wide SET (parallel_workers = 2);
SET maintenance_work_mem = '24MB';
SET client_min_messages = debug1;
CREATE INDEX ivf_wide_l2 ON ivf_wide USING ivfflat (v vector_l2_ops) WITH (lists = 10);
RESET client_min_messages;
RESET maintenance_work_mem;
SELECT count(*) FROM (SELECT id FROM ivf_wide ORDER BY v <-> array_fill(3::real, ARRAY[512])::vector(512) LIMIT 20000) s;
DROP TABLE ivf_wide;
RESET ivfflat.probes;

-- HNSW ------------------------------------------------------------------------
-- HNSW asks the planner only whether workers are allowed, then requests
-- max_parallel_maintenance_workers (hnswbuild.c:1071-1086): one worker in
-- index.c's request line, two launched.
SET hnsw.ef_search = 200;
CREATE TABLE h (id int, v vector(3));
INSERT INTO h SELECT i, ARRAY[i % 23, (i * 7) % 29, (i * 11) % 31] FROM generate_series(1, 5000) i;
SET client_min_messages = debug1;
CREATE INDEX h_l2 ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
-- Each value is unique, so a row's nearest neighbor is itself.
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_l2;

-- 4MB with parallel_workers = 2: the shared area is 1MB, no more than the
-- 1MB margin, so the first tuple flushes ("after 0 tuples", TAP 012's on-disk
-- block) and every tuple is inserted on disk.
ALTER TABLE h SET (parallel_workers = 2);
SET maintenance_work_mem = '4MB';
SET client_min_messages = debug1;
CREATE INDEX h_disk ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
RESET maintenance_work_mem;
ALTER TABLE h RESET (parallel_workers);
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_disk;

-- Review Focus 2: CREATE INDEX CONCURRENTLY with workers.
SET client_min_messages = debug1;
CREATE INDEX CONCURRENTLY h_cic ON h USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SELECT id FROM h ORDER BY v <-> (SELECT v FROM h WHERE id = 2500) LIMIT 1;
DROP INDEX h_cic;

-- Review Focus 5: a temp table and a parallel-unsafe expression stay serial;
-- a parallel-safe expression and an unlogged table's main fork go parallel.
CREATE TEMP TABLE h_temp AS SELECT * FROM h;
SET client_min_messages = debug1;
CREATE INDEX h_temp_l2 ON h_temp USING hnsw (v vector_l2_ops);
RESET client_min_messages;
DROP TABLE h_temp;
SET client_min_messages = debug1;
CREATE INDEX h_unsafe ON h USING hnsw (unsafe_v(v) vector_l2_ops);
RESET client_min_messages;
DROP INDEX h_unsafe;
SET client_min_messages = debug1;
CREATE INDEX h_bq ON h USING hnsw ((binary_quantize(v)::bit(3)) bit_hamming_ops);
RESET client_min_messages;
DROP INDEX h_bq;
CREATE UNLOGGED TABLE h_unlogged AS SELECT * FROM h;
SET client_min_messages = debug1;
CREATE INDEX h_unlogged_l2 ON h_unlogged USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SELECT id FROM h_unlogged ORDER BY v <-> (SELECT v FROM h_unlogged WHERE id = 2500) LIMIT 1;
DROP TABLE h_unlogged;

-- Review Focus 4: duplicate-heavy data built in parallel loses no heap TID:
-- an iterative scan reads every row back through the index.
CREATE TABLE d (id int, v vector(3));
INSERT INTO d SELECT i, ARRAY[i % 20, 0, 0] FROM generate_series(1, 2000) i;
SET client_min_messages = debug1;
CREATE INDEX d_l2 ON d USING hnsw (v vector_l2_ops);
RESET client_min_messages;
SET hnsw.iterative_scan = relaxed_order;
SELECT count(*) FROM (SELECT id FROM d ORDER BY v <-> '[0,0,0]' LIMIT 2000) s;
RESET hnsw.iterative_scan;

-- Review Focus 1: a row error inside a worker. HNSW indexes at most 1000
-- non-zeros (hnswutils.c:1392); whichever participant reads the last row
-- raises C's ERROR, and the build leaves no index and no worker.
CREATE TABLE sv_bad (id int, v sparsevec(2000));
INSERT INTO sv_bad SELECT i, ('{' || (i % 2000) + 1 || ':1}/2000')::sparsevec FROM generate_series(1, 5000) i;
INSERT INTO sv_bad SELECT 5001, (SELECT '{' || string_agg(j || ':1', ',') || '}/2000' FROM generate_series(1, 1001) j)::sparsevec;
SET client_min_messages = debug1;
CREATE INDEX sv_bad_l2 ON sv_bad USING hnsw (v sparsevec_l2_ops);
RESET client_min_messages;
SELECT count(*) FROM pg_class WHERE relname = 'sv_bad_l2';
SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'parallel worker';

-- Review Focus 3: statement_timeout fires mid-build (512MB keeps the graph
-- in memory, so no NOTICE races the timeout).
CREATE TABLE big (id int, v vector(3));
INSERT INTO big SELECT i, ARRAY[i % 101, (i * 7) % 103, (i * 11) % 107] FROM generate_series(1, 100000) i;
SET maintenance_work_mem = '512MB';
SET statement_timeout = '200ms';
CREATE INDEX big_l2 ON big USING hnsw (v vector_l2_ops);
RESET statement_timeout;
RESET maintenance_work_mem;
SELECT count(*) FROM pg_class WHERE relname = 'big_l2';
SELECT count(*) FROM pg_stat_activity WHERE backend_type = 'parallel worker';
RESET hnsw.ef_search;

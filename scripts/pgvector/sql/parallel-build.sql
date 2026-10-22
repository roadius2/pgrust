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
RESET ivfflat.probes;

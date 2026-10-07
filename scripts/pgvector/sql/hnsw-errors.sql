-- HNSW error sites (run-diff.sh's hnsw_errors row): one statement per
-- SQL-reachable ereport of InitBuildState (hnswbuild.c), HnswCheckDim and
-- SparsevecCheckValue (hnswutils.c), for every type. sqldiff.sh --verbose
-- compares each LOCATION. The scans also pin M1 Review Focus 2 and 3: a
-- wrong-dimension query errors even on an empty index, and a NULL query
-- value still scans. HnswCheckDim's build and insert calls cannot be reached
-- from SQL (a column's values always have its typmod's dimensions); unit
-- tests pin them.
SET enable_seqscan = off;
-- vector
CREATE TABLE v_nodim (val vector);
CREATE INDEX ON v_nodim USING hnsw (val vector_l2_ops);
CREATE TABLE v_wide (val vector(2001));
CREATE INDEX ON v_wide USING hnsw (val vector_l2_ops);
CREATE TABLE v (val vector(3));
CREATE INDEX ON v USING hnsw (val vector_l2_ops) WITH (m = 40, ef_construction = 64);
CREATE INDEX ON v USING hnsw (val vector_l2_ops);
CREATE INDEX ON v USING hnsw (val vector_cosine_ops);
SELECT * FROM v ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT * FROM v ORDER BY val <=> '[1,2]' LIMIT 1;
INSERT INTO v VALUES ('[1,2,3]'), ('[4,5,6]');
SELECT * FROM v ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM v ORDER BY val <-> (SELECT NULL::vector) LIMIT 5) s;
SELECT * FROM v ORDER BY val <-> '[1,2,3]' LIMIT 1;

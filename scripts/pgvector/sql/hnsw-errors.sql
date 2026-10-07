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
-- halfvec
CREATE TABLE h_nodim (val halfvec);
CREATE INDEX ON h_nodim USING hnsw (val halfvec_l2_ops);
CREATE TABLE h_wide (val halfvec(4001));
CREATE INDEX ON h_wide USING hnsw (val halfvec_l2_ops);
CREATE TABLE h (val halfvec(3));
CREATE INDEX ON h USING hnsw (val halfvec_l2_ops);
CREATE INDEX ON h USING hnsw (val halfvec_cosine_ops);
SELECT * FROM h ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT * FROM h ORDER BY val <=> '[1,2]' LIMIT 1;
INSERT INTO h VALUES ('[1,2,3]'), ('[0,0,0]');
SELECT * FROM h ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM h ORDER BY val <=> (SELECT NULL::halfvec) LIMIT 5) s;
-- bit
CREATE TABLE b_var (val varbit(3));
CREATE INDEX ON b_var USING hnsw (val bit_hamming_ops);
CREATE INDEX ON b_var USING hnsw ((val::bit(64001)) bit_hamming_ops);
CREATE TABLE b (val bit(3));
CREATE INDEX ON b USING hnsw (val bit_hamming_ops);
CREATE INDEX ON b USING hnsw (val bit_jaccard_ops);
SELECT * FROM b ORDER BY val <~> B'10' LIMIT 1;
SELECT * FROM b ORDER BY val <%> B'10' LIMIT 1;
INSERT INTO b VALUES (B'101');
SELECT * FROM b ORDER BY val <~> B'10' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM b ORDER BY val <~> (SELECT NULL::bit(3)) LIMIT 5) s;
-- sparsevec: SparsevecCheckValue runs at build and insert, before the norm
-- check of the cosine opclass (Review Focus 3); 1,000 non-zeros index.
CREATE TABLE s_nodim (val sparsevec);
CREATE INDEX ON s_nodim USING hnsw (val sparsevec_l2_ops);
CREATE TABLE s (val sparsevec(1001));
INSERT INTO s VALUES (array_fill(1, ARRAY[1001])::vector::sparsevec);
CREATE INDEX ON s USING hnsw (val sparsevec_l2_ops);
CREATE INDEX ON s USING hnsw (val sparsevec_cosine_ops);
TRUNCATE s;
CREATE INDEX ON s USING hnsw (val sparsevec_l2_ops);
CREATE INDEX ON s USING hnsw (val sparsevec_cosine_ops);
SELECT * FROM s ORDER BY val <-> '{1:1}/2' LIMIT 1;
INSERT INTO s VALUES (array_fill(1, ARRAY[1001])::vector::sparsevec);
INSERT INTO s VALUES ((array_fill(1, ARRAY[1000]) || 0)::vector::sparsevec);
SELECT count(*) FROM s;
SELECT * FROM s ORDER BY val <-> '{1:1}/2' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM s ORDER BY val <-> (SELECT NULL::sparsevec) LIMIT 5) s2;

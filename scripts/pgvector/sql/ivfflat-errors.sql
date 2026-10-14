-- Every SQL-reachable IVFFlat error site, LOCATION included, for the diff
-- tier (sqldiff.sh --verbose): InitBuildState (ivfbuild.c:358, :364, :369,
-- :384), IvfflatCheckMemoryUsage (ivfutils.c:133), IvfflatCheckExpectedDim
-- (ivfutils.c:289, from the scan, even on an empty index) and the
-- little-data NOTICE (ivfbuild.c:477). Errors raised by core (the lists
-- reloption, the ivfflat.* settings) have their LOCATIONs compared in
-- core-error-locations.sql (the core_error_locations row). NULL queries
-- use the (SELECT NULL::type) form so they reach the index scan (a literal
-- NULL is constant-folded), and EXPLAIN pins that plan; a zero query on a
-- cosine index orders NaN distances by the sort's tie order.
SET enable_seqscan = off;

-- vector
CREATE TABLE v_nodim (val vector);
CREATE INDEX ON v_nodim USING ivfflat (val vector_l2_ops);
CREATE TABLE v_wide (val vector(2001));
CREATE INDEX ON v_wide USING ivfflat (val vector_l2_ops);
CREATE TABLE v_one (val vector(1));
CREATE INDEX ON v_one USING ivfflat (val vector_cosine_ops);
CREATE INDEX ON v_one USING ivfflat (val vector_ip_ops);
CREATE INDEX ON v_one USING ivfflat (val vector_l2_ops) WITH (lists = 1);
CREATE TABLE v (val vector(3));
CREATE INDEX v_l2 ON v USING ivfflat (val vector_l2_ops) WITH (lists = 1);
SELECT * FROM v ORDER BY val <-> '[1,2]';
INSERT INTO v VALUES ('[1,2,3]'), ('[1,1,1]'), ('[0,0,0]'), (NULL);
SELECT * FROM v ORDER BY val <-> '[1,2]';
SELECT * FROM v ORDER BY val <-> '[3,3,3]';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM v ORDER BY val <-> (SELECT NULL::vector)) t;
SELECT COUNT(*) FROM (SELECT * FROM v ORDER BY val <-> (SELECT NULL::vector)) t;
CREATE INDEX v_cos ON v USING ivfflat (val vector_cosine_ops) WITH (lists = 1);
SELECT * FROM v ORDER BY val <=> '[0,0,0]';
SELECT * FROM v ORDER BY val <=> '[1,2]';
SET maintenance_work_mem = '1MB';
CREATE TABLE v_mem (val vector(2000));
CREATE INDEX ON v_mem USING ivfflat (val vector_l2_ops) WITH (lists = 1000);
RESET maintenance_work_mem;

-- halfvec
CREATE TABLE h_nodim (val halfvec);
CREATE INDEX ON h_nodim USING ivfflat (val halfvec_l2_ops);
CREATE TABLE h_wide (val halfvec(4001));
CREATE INDEX ON h_wide USING ivfflat (val halfvec_l2_ops);
CREATE TABLE h_one (val halfvec(1));
CREATE INDEX ON h_one USING ivfflat (val halfvec_cosine_ops);
CREATE TABLE h (val halfvec(3));
CREATE INDEX h_l2 ON h USING ivfflat (val halfvec_l2_ops) WITH (lists = 1);
SELECT * FROM h ORDER BY val <-> '[1,2]';
INSERT INTO h VALUES ('[1,2,3]'), ('[1,1,1]'), ('[0,0,0]'), (NULL);
SELECT * FROM h ORDER BY val <-> '[1,2]';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM h ORDER BY val <-> (SELECT NULL::halfvec)) t;
SELECT COUNT(*) FROM (SELECT * FROM h ORDER BY val <-> (SELECT NULL::halfvec)) t;

-- bit
CREATE TABLE b_var (val varbit(3));
CREATE INDEX ON b_var USING ivfflat (val bit_hamming_ops);
CREATE TABLE b_wide (val bit(64001));
CREATE INDEX ON b_wide USING ivfflat (val bit_hamming_ops);
CREATE INDEX ON b_var USING ivfflat ((val::bit(64001)) bit_hamming_ops);
CREATE TABLE b (val bit(3));
CREATE INDEX b_ham ON b USING ivfflat (val bit_hamming_ops) WITH (lists = 1);
SELECT * FROM b ORDER BY val <~> '01';
INSERT INTO b VALUES ('000'), ('100'), ('111'), (NULL);
SELECT * FROM b ORDER BY val <~> '01';
EXPLAIN (COSTS OFF) SELECT COUNT(*) FROM (SELECT * FROM b ORDER BY val <~> (SELECT NULL::bit)) t;
SELECT COUNT(*) FROM (SELECT * FROM b ORDER BY val <~> (SELECT NULL::bit)) t;

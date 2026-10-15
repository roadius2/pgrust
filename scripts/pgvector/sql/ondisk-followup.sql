-- DML and maintenance after the subject boots a fixture (run-ondisk.sh):
-- inserts into every index, deletes, VACUUM and REINDEX. New HNSW elements
-- draw random levels and REINDEX reruns k-means, so after this point index
-- answers are compared by row count, and exact answers (no index) row by
-- row.
\set lo 10001
\set hi 10300
\ir ondisk-rows.sql
DELETE FROM vt WHERE id % 7 = 3;
DELETE FROM ct WHERE id % 7 = 3;
DELETE FROM ut WHERE id % 7 = 3;
VACUUM vt;
VACUUM ct;
VACUUM ut;
VACUUM wide;
SELECT count(*) AS vt FROM vt;
SELECT count(*) AS ct FROM ct;
SELECT count(*) AS ut FROM ut;
SELECT count(*) AS wide FROM wide;
\set q8 '[3,1,4,1,5,9,2,6]'
\set q16 '[1,-1,1,1,-1,-1,1,-1,1,1,1,-1,-1,1,-1,1]'
\set s32 '{2:3,10:5,18:1,27:4}/32'
\set b64 '1100100100001111110110101010001000100001011010001100001000110100'
\set u4 '[2,3,4,5]'
-- Exact answers, no index
SET enable_indexscan = off;
SET enable_bitmapscan = off;
SELECT id, v <-> :'q8' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, h <#> :'q8' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, s <+> :'s32' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY d, id LIMIT 10;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY d, id LIMIT 10;
SELECT id, u <-> :'u4' AS d FROM ut ORDER BY d, id LIMIT 10;
SELECT id, e <-> (SELECT e FROM wide WHERE id = 1230) AS d FROM wide ORDER BY d, id LIMIT 5;
RESET enable_indexscan;
RESET enable_bitmapscan;
-- Index answers: row counts, before and after REINDEX
SET enable_seqscan = off;
SET ivfflat.probes = 10;
SELECT (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY v <-> :'q8' LIMIT 10) s) AS v_l2,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY vc <=> :'q16' LIMIT 10) s) AS vc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY h <+> :'q8' LIMIT 10) s) AS h_l1,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY s <#> :'s32' LIMIT 10) s) AS s_ip,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY b <%> :'b64' LIMIT 10) s) AS b_jac,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY gc <=> :'q16' LIMIT 10) s) AS gc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY cb <~> :'b64' LIMIT 10) s) AS cb_ham,
       (SELECT count(*) FROM (SELECT 1 FROM ut ORDER BY u <-> :'u4' LIMIT 10) s) AS u_l2,
       (SELECT count(*) FROM (SELECT 1 FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5) s) AS e_l2;
REINDEX TABLE vt;
REINDEX TABLE ct;
REINDEX TABLE ut;
REINDEX TABLE wide;
SELECT (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY v <-> :'q8' LIMIT 10) s) AS v_l2,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY vc <=> :'q16' LIMIT 10) s) AS vc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY h <+> :'q8' LIMIT 10) s) AS h_l1,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY s <#> :'s32' LIMIT 10) s) AS s_ip,
       (SELECT count(*) FROM (SELECT 1 FROM vt ORDER BY b <%> :'b64' LIMIT 10) s) AS b_jac,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY gc <=> :'q16' LIMIT 10) s) AS gc_cos,
       (SELECT count(*) FROM (SELECT 1 FROM ct ORDER BY cb <~> :'b64' LIMIT 10) s) AS cb_ham,
       (SELECT count(*) FROM (SELECT 1 FROM ut ORDER BY u <-> :'u4' LIMIT 10) s) AS u_l2,
       (SELECT count(*) FROM (SELECT 1 FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5) s) AS e_l2;

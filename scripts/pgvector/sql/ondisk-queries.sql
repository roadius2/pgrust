-- Index scans over the on-disk fixture (run-ondisk.sh). The subject's
-- transcript must equal C's on the same data directory: the same index
-- pages give the same traversal (spec §8.2). Per index: an EXPLAIN proving
-- the scan uses it, then nearest-neighbour queries at the default and at a
-- narrower hnsw.ef_search or ivfflat.probes. Query values follow
-- ondisk-rows.sql's exact-arithmetic rules.
SET enable_seqscan = off;
\set q8 '[3,1,4,1,5,9,2,6]'
\set q8b '[10,0,5,12,3,8,20,7]'
\set q16 '[1,-1,1,1,-1,-1,1,-1,1,1,1,-1,-1,1,-1,1]'
\set s32 '{2:3,10:5,18:1,27:4}/32'
\set s64 '{1:1,6:-1,11:1,16:1,17:-1,22:1,27:-1,32:1,33:1,38:-1,43:1,48:-1,49:1,54:1,59:-1,64:1}/64'
\set b64 '1100100100001111110110101010001000100001011010001100001000110100'
\set u4 '[2,3,4,5]'
SELECT count(*) AS vt FROM vt;
SELECT count(*) AS ct FROM ct;
SELECT count(*) AS ut FROM ut;
SELECT count(*) AS wide FROM wide;
SELECT count(*) AS inflight FROM vt WHERE id BETWEEN 4001 AND 4200;
SELECT pg_relation_size(reltoastrelid) > 0 AS wide_toasted FROM pg_class WHERE relname = 'wide';
-- HNSW: vector
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <-> :'q8' LIMIT 10;
SELECT id, v <-> :'q8' AS d FROM vt ORDER BY v <-> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <#> :'q8' LIMIT 10;
SELECT id, v <#> :'q8' AS d FROM vt ORDER BY v <#> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY v <+> :'q8' LIMIT 10;
SELECT id, v <+> :'q8' AS d FROM vt ORDER BY v <+> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, v <-> :'q8b' AS d FROM vt ORDER BY v <-> :'q8b' LIMIT 10;
SELECT id, v <#> :'q8b' AS d FROM vt ORDER BY v <#> :'q8b' LIMIT 10;
SELECT id, v <+> :'q8b' AS d FROM vt ORDER BY v <+> :'q8b' LIMIT 10;
SELECT id, vc <=> :'q16' AS d FROM vt ORDER BY vc <=> :'q16' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: halfvec
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <-> :'q8' LIMIT 10;
SELECT id, h <-> :'q8' AS d FROM vt ORDER BY h <-> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <#> :'q8' LIMIT 10;
SELECT id, h <#> :'q8' AS d FROM vt ORDER BY h <#> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY h <+> :'q8' LIMIT 10;
SELECT id, h <+> :'q8' AS d FROM vt ORDER BY h <+> :'q8' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
SELECT id, hc <=> :'q16' AS d FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, h <-> :'q8b' AS d FROM vt ORDER BY h <-> :'q8b' LIMIT 10;
SELECT id, h <#> :'q8b' AS d FROM vt ORDER BY h <#> :'q8b' LIMIT 10;
SELECT id, h <+> :'q8b' AS d FROM vt ORDER BY h <+> :'q8b' LIMIT 10;
SELECT id, hc <=> :'q16' AS d FROM vt ORDER BY hc <=> :'q16' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: sparsevec
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <-> :'s32' LIMIT 10;
SELECT id, s <-> :'s32' AS d FROM vt ORDER BY s <-> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <#> :'s32' LIMIT 10;
SELECT id, s <#> :'s32' AS d FROM vt ORDER BY s <#> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY s <+> :'s32' LIMIT 10;
SELECT id, s <+> :'s32' AS d FROM vt ORDER BY s <+> :'s32' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
SELECT id, sc <=> :'s64' AS d FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, s <-> :'s32' AS d FROM vt ORDER BY s <-> :'s32' LIMIT 10;
SELECT id, sc <=> :'s64' AS d FROM vt ORDER BY sc <=> :'s64' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: bit, and binary_quantize
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY b <~> :'b64' LIMIT 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY b <~> :'b64' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY b <%> :'b64' LIMIT 10;
SELECT id, b <%> :'b64' AS d FROM vt ORDER BY b <%> :'b64' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM vt ORDER BY binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) LIMIT 10;
SELECT id, binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) AS d FROM vt
  ORDER BY binary_quantize(vc)::bit(16) <~> binary_quantize(:'q16'::vector) LIMIT 10;
SET hnsw.ef_search = 10;
SELECT id, b <~> :'b64' AS d FROM vt ORDER BY b <~> :'b64' LIMIT 10;
SELECT id, b <%> :'b64' AS d FROM vt ORDER BY b <%> :'b64' LIMIT 10;
RESET hnsw.ef_search;
-- HNSW: the unlogged and the TOASTed tables
EXPLAIN (COSTS OFF) SELECT id FROM ut ORDER BY u <-> :'u4' LIMIT 10;
SELECT id, u <-> :'u4' AS d FROM ut ORDER BY u <-> :'u4' LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5;
SELECT id, e <-> (SELECT e FROM wide WHERE id = 1230) AS d FROM wide ORDER BY e <-> (SELECT e FROM wide WHERE id = 1230) LIMIT 5;
-- IVFFlat: vector and halfvec around ct's clusters (the query is row 13's point)
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY g <#> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
SELECT id, g <#> (SELECT g FROM ct WHERE id = 13) AS d FROM ct ORDER BY g <#> (SELECT g FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gc <=> (SELECT gc FROM ct WHERE id = 13) AS d FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gh <-> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gh <-> (SELECT gh FROM ct WHERE id = 13) AS d FROM ct ORDER BY gh <-> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
SELECT id, gh <#> (SELECT gh FROM ct WHERE id = 13) AS d FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY ghc <=> (SELECT ghc FROM ct WHERE id = 13) LIMIT 10;
SELECT id, ghc <=> (SELECT ghc FROM ct WHERE id = 13) AS d FROM ct ORDER BY ghc <=> (SELECT ghc FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 13) LIMIT 10;
SELECT id, cb <~> (SELECT cb FROM ct WHERE id = 13) AS d FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 13) LIMIT 10;
EXPLAIN (COSTS OFF) SELECT id FROM ut ORDER BY u <#> :'u4' LIMIT 10;
SELECT id, u <#> :'u4' AS d FROM ut ORDER BY u <#> :'u4' LIMIT 10;
SET ivfflat.probes = 3;
SELECT id, g <-> (SELECT g FROM ct WHERE id = 27) AS d FROM ct ORDER BY g <-> (SELECT g FROM ct WHERE id = 27) LIMIT 10;
SELECT id, gc <=> (SELECT gc FROM ct WHERE id = 27) AS d FROM ct ORDER BY gc <=> (SELECT gc FROM ct WHERE id = 27) LIMIT 10;
SELECT id, gh <#> (SELECT gh FROM ct WHERE id = 27) AS d FROM ct ORDER BY gh <#> (SELECT gh FROM ct WHERE id = 27) LIMIT 10;
SELECT id, cb <~> (SELECT cb FROM ct WHERE id = 27) AS d FROM ct ORDER BY cb <~> (SELECT cb FROM ct WHERE id = 27) LIMIT 10;
RESET ivfflat.probes;
-- Iterative scans with a filter
SET hnsw.iterative_scan = strict_order;
SELECT id FROM vt WHERE id % 9 = 4 ORDER BY v <-> :'q8' LIMIT 20;
SET hnsw.iterative_scan = relaxed_order;
SELECT id FROM vt WHERE id % 9 = 4 ORDER BY h <-> :'q8' LIMIT 20;
RESET hnsw.iterative_scan;
SET ivfflat.iterative_scan = relaxed_order;
SELECT id FROM ct WHERE id % 7 = 2 ORDER BY g <-> (SELECT g FROM ct WHERE id = 13) LIMIT 20;
RESET ivfflat.iterative_scan;
-- A query of the wrong dimension against each access method's metapage
SELECT id FROM vt ORDER BY v <-> '[1,2,3]' LIMIT 1;
SELECT id FROM ct ORDER BY g <-> '[1,2,3]' LIMIT 1;

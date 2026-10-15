-- The on-disk tier's fixture (spec §8.2 "On-disk"; run-ondisk.sh): every
-- HNSW and IVFFlat opclass, a binary_quantize expression index, an unlogged
-- table, a TOASTed 1536-dimension table, deletes, VACUUM and ANALYZE. Runs
-- unchanged on C pgvector 0.8.1 and 0.8.7 and on pgrust. Row values:
-- ondisk-rows.sql.
CREATE EXTENSION vector;
CREATE TABLE vt (id int PRIMARY KEY, v vector(8), h halfvec(8), s sparsevec(32), b bit(64),
  vc vector(16), hc halfvec(16), sc sparsevec(64));
CREATE TABLE ct (id int PRIMARY KEY, g vector(8), gh halfvec(8), gc vector(16), ghc halfvec(16), cb bit(64));
CREATE UNLOGGED TABLE ut (id int PRIMARY KEY, u vector(4));
CREATE TABLE wide (id int PRIMARY KEY, e vector(1536));
\set lo 1
\set hi 3000
\ir ondisk-rows.sql
CREATE INDEX vt_v_l2 ON vt USING hnsw (v vector_l2_ops) WITH (m = 8, ef_construction = 32);
CREATE INDEX vt_v_ip ON vt USING hnsw (v vector_ip_ops);
CREATE INDEX vt_v_l1 ON vt USING hnsw (v vector_l1_ops) WITH (m = 24);
CREATE INDEX vt_vc_cos ON vt USING hnsw (vc vector_cosine_ops);
CREATE INDEX vt_h_l2 ON vt USING hnsw (h halfvec_l2_ops);
CREATE INDEX vt_h_ip ON vt USING hnsw (h halfvec_ip_ops);
CREATE INDEX vt_h_l1 ON vt USING hnsw (h halfvec_l1_ops);
CREATE INDEX vt_hc_cos ON vt USING hnsw (hc halfvec_cosine_ops);
CREATE INDEX vt_s_l2 ON vt USING hnsw (s sparsevec_l2_ops);
CREATE INDEX vt_s_ip ON vt USING hnsw (s sparsevec_ip_ops);
CREATE INDEX vt_s_l1 ON vt USING hnsw (s sparsevec_l1_ops);
CREATE INDEX vt_sc_cos ON vt USING hnsw (sc sparsevec_cosine_ops);
CREATE INDEX vt_b_ham ON vt USING hnsw (b bit_hamming_ops);
CREATE INDEX vt_b_jac ON vt USING hnsw (b bit_jaccard_ops);
CREATE INDEX vt_bq ON vt USING hnsw ((binary_quantize(vc)::bit(16)) bit_hamming_ops);
CREATE INDEX ct_g_l2 ON ct USING ivfflat (g vector_l2_ops) WITH (lists = 10);
CREATE INDEX ct_g_ip ON ct USING ivfflat (g vector_ip_ops) WITH (lists = 10);
CREATE INDEX ct_gc_cos ON ct USING ivfflat (gc vector_cosine_ops) WITH (lists = 10);
CREATE INDEX ct_gh_l2 ON ct USING ivfflat (gh halfvec_l2_ops) WITH (lists = 10);
CREATE INDEX ct_gh_ip ON ct USING ivfflat (gh halfvec_ip_ops) WITH (lists = 10);
CREATE INDEX ct_ghc_cos ON ct USING ivfflat (ghc halfvec_cosine_ops) WITH (lists = 10);
CREATE INDEX ct_cb_ham ON ct USING ivfflat (cb bit_hamming_ops) WITH (lists = 10);
CREATE INDEX ut_u_l2 ON ut USING hnsw (u vector_l2_ops);
CREATE INDEX ut_u_ip ON ut USING ivfflat (u vector_ip_ops) WITH (lists = 4);
CREATE INDEX wide_e_l2 ON wide USING hnsw (e vector_l2_ops);
DELETE FROM vt WHERE id % 10 = 0;
DELETE FROM ct WHERE id % 10 = 0;
DELETE FROM ut WHERE id % 60 = 0;
VACUUM ANALYZE vt;
VACUUM ANALYZE ct;
VACUUM ANALYZE ut;
VACUUM ANALYZE wide;

-- Core error LOCATIONs that pgvector SQL reaches (run-diff.sh's
-- core_error_locations row, sqldiff.sh --verbose): reloptions.c
-- parse_one_reloption / parseRelOptionsInternal, guc.c parse_and_validate_value,
-- assignable_custom_variable_name and MarkGUCPrefixReserved (M4 follow-ups,
-- Task 4); float.c and mcxt.c follow (Task 5). Placeholders come first: the
-- library loads at the first vector call, when C defines the hnsw.* and
-- ivfflat.* settings and reserves their prefixes.
SET hnsw.placeholder_a = 1;
SET ivfflat.placeholder_b = 1;
SELECT '[1]'::vector;
SET hnsw.nonexistent = 1;
SET ivfflat.nonexistent = 1;
SET ivfflat..x = 1;
SET nonexistent_param = 1;
SET hnsw.ef_search = 0;
SET hnsw.ef_search = 1001;
SET hnsw.max_scan_tuples = 0;
SET ivfflat.probes = 0;
SET ivfflat.probes = 32769;
SET ivfflat.max_probes = 0;
SET hnsw.ef_search = 'abc';
SET ivfflat.probes = 'abc';
SET ivfflat.max_probes = 99999999999;
SET hnsw.scan_mem_multiplier = 0;
SET hnsw.scan_mem_multiplier = 1001;
SET hnsw.scan_mem_multiplier = 'abc';
SET hnsw.iterative_scan = bogus;
SET ivfflat.iterative_scan = bogus;
SET ivfflat.iterative_scan = strict_order;
SET enable_seqscan = maybe;
SET hnsw.ef_search = 100;
SHOW hnsw.ef_search;
CREATE TABLE t (id int, v vector(3));
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 1);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 101);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 3);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 1001);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 'abc');
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (ef_construction = 'abc');
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (m = 8, m = 9);
CREATE INDEX ON t USING hnsw (v vector_l2_ops) WITH (nope = 1);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 0);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 32769);
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (lists = 'abc');
CREATE INDEX ON t USING ivfflat (v vector_l2_ops) WITH (nope = 1);
CREATE INDEX t_h ON t USING hnsw (v vector_l2_ops) WITH (m = 8);
ALTER INDEX t_h SET (m = 1);
ALTER INDEX t_h SET (ef_construction = 'abc');

-- Where a session first loads vector (running _PG_init, which reserves the
-- hnsw and ivfflat GUC prefixes and removes their placeholders with a
-- WARNING): C loads it whenever fmgr resolves one of its functions, including
-- the AM handler (amapi.c:38 OidFunctionCall0) at CREATE INDEX (indexcmds.c:864),
-- at a session's first open of an hnsw/ivfflat index (relcache.c:1432) and
-- in amutils' property functions, but not at SELECT ... FROM pg_am or at
-- CREATE ACCESS METHOD (amcmds.c:78 looks the handler up without calling it).
-- sqldiff.sh creates the extension in another session; each \c below starts
-- a session with vector unloaded. Plain verbosity: core_error_locations
-- compares these messages' LOCATIONs.

-- Negative control: reading pg_am loads nothing.
SET hnsw.typo = 1;
SELECT amname FROM pg_am WHERE amname IN ('hnsw', 'ivfflat') ORDER BY 1;
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- Negative control: CREATE ACCESS METHOD loads nothing (lookup_am_handler_func,
-- amcmds.c:78, checks the handler's return type and never calls it).
\c
SET hnsw.typo = 1;
CREATE ACCESS METHOD hnsw2 TYPE INDEX HANDLER hnswhandler;
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- amutils on the AM loads vector.
\c
SET hnsw.typo = 1;
SET ivfflat.typo = 1;
SELECT pg_indexam_has_property(oid, 'can_order') FROM pg_am WHERE amname = 'hnsw';
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- A session's first open of an hnsw index loads vector (the vector(3)
-- column's typmod input already loaded it in the session that made the table).
CREATE TABLE t (id int, v vector(3));
CREATE INDEX t_v_idx ON t USING hnsw (v vector_l2_ops);
\c
SET hnsw.typo = 1;
INSERT INTO t (id) VALUES (1);
SET hnsw.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- CREATE INDEX loads vector at DefineIndex's handler call (indexcmds.c:864),
-- before it resolves the opclass: int has no default ivfflat opclass, so the
-- statement fails after the load and before anything else could load vector.
\c
SET ivfflat.typo = 1;
CREATE INDEX ON t USING ivfflat (id);
SET ivfflat.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

-- A CREATE INDEX that succeeds loads vector at the same point.
\c
SET ivfflat.typo = 1;
CREATE INDEX t_v_ivf ON t USING ivfflat (v vector_l2_ops) WITH (lists = 1);
SET ivfflat.typo2 = 1;
SELECT module_name, version FROM pg_get_loaded_modules() WHERE module_name = 'vector';

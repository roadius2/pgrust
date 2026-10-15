-- name|pagemask rmgr|path of every relation the fixture owns: its tables,
-- their TOAST tables, and the indexes of both (run-ondisk.sh compares their
-- pages after crash recovery).
\pset format unaligned
\pset tuples_only on
WITH t AS (SELECT oid, reltoastrelid FROM pg_class WHERE relnamespace = 'public'::regnamespace AND relkind = 'r'),
r AS (SELECT oid FROM t
      UNION SELECT reltoastrelid FROM t WHERE reltoastrelid <> 0
      UNION SELECT indexrelid FROM pg_index WHERE indrelid IN (SELECT oid FROM t UNION SELECT reltoastrelid FROM t))
SELECT c.relname || '|' || CASE WHEN c.relkind IN ('r', 't') THEN 'heap' WHEN a.amname = 'btree' THEN 'btree' ELSE 'generic' END
       || '|' || pg_relation_filepath(c.oid)
FROM pg_class c JOIN r ON r.oid = c.oid LEFT JOIN pg_am a ON a.oid = c.relam
ORDER BY 1;

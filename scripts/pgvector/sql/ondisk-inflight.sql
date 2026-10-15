-- The transaction still open when the crash fixture stops immediately
-- (run-ondisk.sh): its rows reach the heap and every index, and recovery
-- must leave them invisible.
BEGIN;
\set lo 4001
\set hi 4200
\ir ondisk-rows.sql
SELECT pg_sleep(600);

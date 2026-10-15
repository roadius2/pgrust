-- Writes after the crash fixture's CHECKPOINT (run-ondisk.sh): on-disk
-- inserts into every index, deletes, and VACUUM's index cleanup, all WAL that
-- crash recovery must replay. Every transaction here commits.
\set lo 3001
\set hi 3600
\ir ondisk-rows.sql
DELETE FROM vt WHERE id % 10 = 5;
DELETE FROM ct WHERE id % 10 = 5;
DELETE FROM ut WHERE id % 60 = 30;
VACUUM vt;
VACUUM ct;
VACUUM ut;
\set lo 3601
\set hi 3800
\ir ondisk-rows.sql

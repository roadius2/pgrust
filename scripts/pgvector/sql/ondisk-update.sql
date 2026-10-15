-- The 0.8.1 fixture after a binary upgrade (run-ondisk.sh): the index
-- queries on the 0.8.1 catalog, ALTER EXTENSION vector UPDATE, the catalog
-- it leaves (repair-verify.sql: version, members and comments, function
-- definitions, amop, amproc, opclasses), then the queries again.
SELECT extversion FROM pg_extension WHERE extname = 'vector';
\ir ondisk-queries.sql
ALTER EXTENSION vector UPDATE;
\ir repair-verify.sql
\pset format aligned
\pset tuples_only off
\ir ondisk-queries.sql

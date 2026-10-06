-- Error locations (run-diff.sh's error_locations row): one statement per
-- ereport/elog in halfvec.c, halfutils.h, sparsevec.c and bitvec.c that SQL
-- can reach. sqldiff.sh --verbose compares each LOCATION line (C function,
-- file, and the call's last line). Binary-only sites (the recv functions)
-- are pinned by unit tests instead.
-- halfvec.c
SELECT '[1,2]'::halfvec + '[3]';
SELECT '[1,2,3]'::halfvec(2);
SELECT '{}'::real[]::halfvec;
SELECT array_fill(0, ARRAY[16000])::halfvec || '[1]';
SELECT '[NaN,1]'::halfvec;
SELECT '[Infinity,1]'::halfvec;
SELECT halfvec_avg('{{2,2,4,6}}');
SELECT '1,2,3'::halfvec;
SELECT '[]'::halfvec;
SELECT ('[' || array_to_string(array_fill(1, ARRAY[16001]), ',') || ']')::halfvec;
SELECT '['::halfvec;
SELECT '[hello,1]'::halfvec;
SELECT '[65520]'::halfvec;
SELECT '[1,2,3'::halfvec;
SELECT '[1,2,3]9'::halfvec;
SELECT '[1,2,3]'::halfvec(3, 2);
SELECT '[1,2,3]'::halfvec(0);
SELECT '[1,2,3]'::halfvec(16001);
SELECT '{{1}}'::real[]::halfvec;
SELECT '{NULL}'::real[]::halfvec;
SELECT subvector('[1,2,3,4,5]'::halfvec, 1, 0);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2147483647, 10);
-- halfutils.h
SELECT '{65520}'::real[]::halfvec;
-- sparsevec.c
SELECT '{1:1}/2'::sparsevec <-> '{1:1}/3';
SELECT '{}/3'::sparsevec(2);
SELECT '{}/-1'::sparsevec;
SELECT '{}/1000000001'::sparsevec;
SELECT array_agg(n)::sparsevec FROM generate_series(1, 16001) n;
SELECT '{0:1}/1'::sparsevec;
SELECT '{1:1,1:1}/2'::sparsevec;
SELECT '{1:NaN}/1'::sparsevec;
SELECT '{1:Infinity}/1'::sparsevec;
SELECT ('{' || repeat('1:1,', 16001) || '1:1}/1')::sparsevec;
SELECT '1:1}/1'::sparsevec;
SELECT '{'::sparsevec;
SELECT '{:1}/1'::sparsevec;
SELECT '{1a:1}/1'::sparsevec;
SELECT '{1:}/1'::sparsevec;
SELECT '{1:4e38}/1'::sparsevec;
SELECT '{1:1a}/1'::sparsevec;
SELECT '{}'::sparsevec;
SELECT '{}/'::sparsevec;
SELECT '{}/1a'::sparsevec;
SELECT '{}/3'::sparsevec(3, 2);
SELECT '{}/3'::sparsevec(0);
SELECT '{}/3'::sparsevec(1000000001);
SELECT '{{1}}'::real[]::sparsevec;
SELECT '{NULL}'::real[]::sparsevec;
-- bitvec.c
SELECT hamming_distance('111', '00');

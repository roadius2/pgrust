-- pgvector exact-differential deck (diffrunner --pgvector; pgvector Phase 1
-- spec §8.2). One statement per line, each run on both servers. A
-- `-- section: <name>` line starts a section; other `--` lines and blank
-- lines are skipped. Statements start with SELECT or COPY (. Integer and
-- dyadic data keep float sums exact; the few non-integral distances and
-- norms (sections distance and norm, the only tolerant ones) fall under the
-- pgvector-float-rel ruling. Sections normalize and denormal compare
-- exactly: l2_normalize is elementwise, and the denormal cases have one
-- non-zero term each (no cancellation, no order dependence) and only
-- non-negative products that underflow, so fused and unfused sums agree.

-- section: io
SELECT '[1,2,3]'::vector, '[-1.5,0,2.25]'::vector, ' [ 1 , 2 ] '::vector;
SELECT '[1e-46,1e-45,-1e-40,3.4e38,-3.4e38]'::vector;
SELECT '[0x1p-3,-0x1.8p1,1.5e+2]'::vector, '[0x1p-3,-0x1.8p1,1.5e+2]'::halfvec;
SELECT '[-0,0]'::vector, '[-0,0]'::halfvec, '{1:-0}/1'::sparsevec;
SELECT '[1,2,3]'::halfvec, '[1.23456]'::halfvec, '[0.1,0.2,0.3]'::halfvec;
SELECT '[65504,-65504,65519,6e-08,5.96e-08,3e-08,2049,2051]'::halfvec;
SELECT '[1e-8,-1e-8,1e-46]'::halfvec;
SELECT '{1:1.5,3:3.5}/5'::sparsevec, ' { 3 : -2 , 1 : 1 } / 4 '::sparsevec, '{}/1'::sparsevec;
SELECT '{1:0,2:1,3:0}/3'::sparsevec, '{1:1e-40,2:-1.5e-38}/2'::sparsevec;
SELECT '{1000000000:1}/1000000000'::sparsevec, '{10:0}/3'::sparsevec;
SELECT '[1,2'::vector;
SELECT '[1,2,3]x'::halfvec;
SELECT '[hello]'::halfvec;
SELECT '[]'::halfvec;
SELECT '[NaN]'::halfvec;
SELECT '[-Infinity]'::halfvec;
SELECT '[65520]'::halfvec;
SELECT '[4e38]'::halfvec;
SELECT '[1,,2]'::halfvec;
SELECT '[1, ]'::halfvec;
SELECT '{1:1,1:2}/2'::sparsevec;
SELECT '{2:1,1:2,2:3}/2'::sparsevec;
SELECT '{0:1}/1'::sparsevec;
SELECT '{1:1e-46}/1'::sparsevec;
SELECT '{1:4e38}/1'::sparsevec;
SELECT '{1:Infinity}/1'::sparsevec;
SELECT '{1:NaN}/1'::sparsevec;
SELECT '{}/0'::sparsevec;
SELECT '{}/1000000001'::sparsevec;
SELECT '{}/9223372036854775808'::sparsevec;
SELECT '{}/-9223372036854775809'::sparsevec;
SELECT '{2147483648:1}/1'::sparsevec;
SELECT '{-2147483649:1}/1'::sparsevec;
SELECT '{1:1'::sparsevec;
SELECT '1:1}/1'::sparsevec;
SELECT '{1:1}/1 x'::sparsevec;
SELECT '{1:1}'::sparsevec;
SELECT '{1:1}/'::sparsevec;
SELECT '{a:1}/1'::sparsevec;
SELECT '{1 2}/2'::sparsevec;
SELECT '{1:}/1'::sparsevec;
SELECT halfvec_send('[1,2,-0.5,65504,6e-8,-0]'::halfvec), sparsevec_send('{1:1.5,3:-2}/5'::sparsevec), sparsevec_send('{}/7'::sparsevec), sparsevec_send('{1000000000:1}/1000000000'::sparsevec);
COPY (VALUES ('[1,-2.5,0]'::vector, '[1,-0.5,6e-8]'::halfvec, '{1:1.5,3:-2}/5'::sparsevec), ('[-0,1e-40,3.4e38]', '[65504,-0,-6e-8]', '{}/5')) TO STDOUT (FORMAT binary);

-- section: typmod
SELECT '[1,2,3]'::vector(3), '[1,2,3]'::halfvec(3), '{}/3'::sparsevec(3);
SELECT '[1,2,3]'::halfvec(2);
SELECT '{}/3'::sparsevec(2);
SELECT '[1,2,3]'::halfvec(0);
SELECT '[1,2,3]'::halfvec(16001);
SELECT '{}/3'::sparsevec(1000000001);
SELECT '[1,2,3]'::halfvec(3, 2);
SELECT '{}/3'::sparsevec(3, 2);
SELECT '{"[1,2,3]","[4,5,6]"}'::halfvec(3)[];
SELECT '{"[1,2,3]"}'::halfvec(2)[];
SELECT '{"{1:1}/3","{}/3"}'::sparsevec(3)[];
SELECT '{"{}/3"}'::sparsevec(4)[];

-- section: cast
SELECT ARRAY[1,2,3]::vector, ARRAY[1,2,3]::halfvec, ARRAY[1,0,3]::sparsevec;
SELECT ARRAY[1.5,0,-2.25]::numeric[]::halfvec, ARRAY[1.5,0,-2.25]::numeric[]::sparsevec;
SELECT '{1,2,3}'::real[]::halfvec(3), '{0,0,1}'::real[]::sparsevec(3);
SELECT '{1,2,3}'::double precision[]::halfvec, '{1e-46,0,1}'::double precision[]::sparsevec;
SELECT '{65520}'::real[]::halfvec;
SELECT '{4e38}'::double precision[]::halfvec;
SELECT '{4e38}'::double precision[]::sparsevec;
SELECT '{1e-8,-1e-8}'::real[]::halfvec;
SELECT '{NaN}'::real[]::sparsevec;
SELECT '{NULL,1}'::real[]::halfvec;
SELECT '{{1,2}}'::real[]::sparsevec;
SELECT '{}'::real[]::halfvec;
SELECT '{}'::real[]::sparsevec;
SELECT '{1,2}'::real[]::sparsevec(3);
SELECT '[1,2,3]'::vector::halfvec, '[1.5,0,-2]'::vector::sparsevec;
SELECT '[0.1,0.2]'::vector::halfvec::vector;
SELECT '[65520]'::vector::halfvec;
SELECT '[1,2,3]'::halfvec::vector, '[0,1.5,0]'::halfvec::sparsevec, '[1,2,3]'::halfvec::real[];
SELECT '{2:1.5,4:-3.5}/5'::sparsevec::vector, '{2:1.5,4:-3.5}/5'::sparsevec::halfvec;
SELECT '{1:65520}/1'::sparsevec::halfvec;
SELECT '{1:1e-8}/1'::sparsevec::halfvec;
SELECT '{}/16001'::sparsevec::vector;
SELECT '{}/16001'::sparsevec::halfvec;
SELECT '{1:1}/3'::sparsevec::vector(2);
SELECT '[1,2,3]'::vector::sparsevec(2);
SELECT '[1,2,3]'::halfvec::vector(2);
SELECT '[0,0,0]'::vector::sparsevec, '[0,-0]'::halfvec::sparsevec;
SELECT array_agg(n)::halfvec IS NULL FROM generate_series(1, 16001) n;
SELECT array_agg(n)::sparsevec IS NULL FROM generate_series(1, 16001) n;

-- section: arith
SELECT '[1,2,3]'::halfvec + '[4,5,6]', '[1,2,3]'::halfvec - '[4,5,6]', '[1,2,3]'::halfvec * '[4,5,6]';
SELECT '[0.1,0.2,0.3]'::halfvec + '[0.3,0.2,0.1]', '[0.1,0.2,0.3]'::halfvec * '[3,3,3]', '[0.1]'::halfvec - '[0.3]';
SELECT '[1.5,-2.25]'::vector + '[1e-40,1e-40]', '[1,2]'::vector * '[0.5,0.25]';
SELECT '[65504]'::halfvec + '[1]', '[2048]'::halfvec + '[1]', '[2048]'::halfvec + '[3]';
SELECT '[65504]'::halfvec + '[16]';
SELECT '[-65504]'::halfvec - '[32]';
SELECT '[300]'::halfvec * '[300]';
SELECT '[1e-4]'::halfvec * '[1e-4]';
SELECT '[0]'::halfvec * '[1e-4]', '[6e-8]'::halfvec * '[1]';
SELECT '[6e-8]'::halfvec * '[0.5]';
SELECT '[1,2]'::halfvec + '[1]';
SELECT '[1,2]'::halfvec || '[3]', '[1]'::vector || '[2,3]';
SELECT array_fill(0, ARRAY[16000])::halfvec || '[1]';
SELECT sum(v) FROM (VALUES ('[1,2]'::halfvec), ('[3,4]'), ('[0.5,0.25]')) t(v);

-- section: distance
SELECT l2_distance('[0,0]'::vector, '[3,4]'), inner_product('[1,2]'::vector, '[3,4]'), cosine_distance('[1,2]'::vector, '[2,4]'), l1_distance('[0,0]'::vector, '[3,4]');
SELECT l2_distance('[0,0]'::halfvec, '[3,4]'), inner_product('[1,2]'::halfvec, '[3,4]'), cosine_distance('[1,2]'::halfvec, '[2,4]'), l1_distance('[0,0]'::halfvec, '[3,4]');
SELECT l2_distance('{}/2'::sparsevec, '{1:3,2:4}/2'), inner_product('{1:1,2:2}/2'::sparsevec, '{1:3,2:4}/2'), cosine_distance('{1:1,2:2}/2'::sparsevec, '{1:2,2:4}/2'), l1_distance('{}/2'::sparsevec, '{1:3,2:4}/2');
SELECT '[1,2,3]'::vector <-> '[3,2,1]', '[1,2,3]'::vector <#> '[3,2,1]', '[1,2,3]'::vector <=> '[3,2,1]', '[1,2,3]'::vector <+> '[3,2,1]';
SELECT '[1,2,3]'::halfvec <-> '[3,2,1]', '[1,2,3]'::halfvec <#> '[3,2,1]', '[1,2,3]'::halfvec <=> '[3,2,1]', '[1,2,3]'::halfvec <+> '[3,2,1]';
SELECT '{1:1,3:3}/3'::sparsevec <-> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <#> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <=> '{2:2,3:1}/3', '{1:1,3:3}/3'::sparsevec <+> '{2:2,3:1}/3';
SELECT l2_distance('{1:1,3:3,5:5,7:7}/9'::sparsevec, '{2:2,4:4,6:6,8:8,9:9}/9'), l1_distance('{9:1}/9'::sparsevec, '{1:1,2:2}/9'), inner_product('{1:1,3:3,5:5}/5'::sparsevec, '{2:4,3:6,4:8}/5');
SELECT l2_distance('{1:1,2:2,3:3}/3'::sparsevec, '{3:3}/3'), l1_distance('{3:3}/3'::sparsevec, '{1:1,2:2,3:3}/3'), inner_product('{2:2}/3'::sparsevec, '{1:1,2:2,3:3}/3');
SELECT vector_l2_squared_distance('[1,2]', '[3,5]'), vector_negative_inner_product('[1,2]', '[3,5]'), vector_spherical_distance('[0.6,0.8]', '[0.8,0.6]');
SELECT halfvec_l2_squared_distance('[1,2]', '[3,5]'), halfvec_negative_inner_product('[1,2]', '[3,5]'), halfvec_spherical_distance('[0.6,0.8]', '[0.8,0.6]');
SELECT sparsevec_l2_squared_distance('{1:1}/2', '{2:5}/2'), sparsevec_negative_inner_product('{1:1,2:2}/2', '{1:3,2:5}/2');
SELECT cosine_distance('[0,0]'::vector, '[1,1]'), cosine_distance('[0,0]'::halfvec, '[1,1]'), cosine_distance('{}/2'::sparsevec, '{1:1}/2');
SELECT cosine_distance('[1,1]'::halfvec, '[-1.1,-1.1]'), cosine_distance('{1:3e38}/1'::sparsevec, '{1:3e38}/1');
SELECT inner_product('[65504]'::halfvec, '[65504]'), inner_product('{1:3e38}/1'::sparsevec, '{1:3e38}/1');
SELECT l2_distance('[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9]'::vector, '[0.9,0.8,0.7,0.6,0.5,0.4,0.3,0.2,0.1]');
SELECT l2_distance('[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9]'::halfvec, '[0.9,0.8,0.7,0.6,0.5,0.4,0.3,0.2,0.1]');
SELECT l2_distance(array_fill(1, ARRAY[16000])::vector, array_fill(2, ARRAY[16000])::vector), l1_distance(array_fill(1, ARRAY[16000])::halfvec, array_fill(2, ARRAY[16000])::halfvec);
SELECT l2_distance('[1,2]'::halfvec, '[3]');
SELECT inner_product('{1:1}/2'::sparsevec, '{1:1}/3');
SELECT l1_distance('[1,2]'::vector, '[3]');

-- section: norm
SELECT vector_norm('[3,4]'), l2_norm('[3,4]'::halfvec), l2_norm('{1:3,2:4}/2'::sparsevec), l2_norm('{}/2'::sparsevec);
SELECT l2_norm('{1:3e37,2:4e37}/2'::sparsevec)::real, l2_norm('[65504,65504]'::halfvec);

-- section: normalize
SELECT l2_normalize('[3,4]'::vector), l2_normalize('[3,4]'::halfvec), l2_normalize('{1:3,2:4}/2'::sparsevec);
SELECT l2_normalize('[0,0]'::vector), l2_normalize('[0,0]'::halfvec), l2_normalize('{}/2'::sparsevec);
SELECT l2_normalize('[0.1,0.2,0.3]'::vector), l2_normalize('[0.1,0.2,0.3]'::halfvec), l2_normalize('{1:0.1,3:0.3}/4'::sparsevec);
SELECT l2_normalize('[65504]'::halfvec), l2_normalize('[6e-8]'::halfvec), l2_normalize('{1:3e38}/1'::sparsevec);
SELECT l2_normalize('{1:3e38,2:1e-37}/2'::sparsevec), l2_normalize('{2:3e37,4:3e-37,6:4e37,8:4e-37}/9'::sparsevec);
SELECT l2_normalize('[1e-40,0]'::vector), l2_normalize('{2:1.4e-45}/2'::sparsevec), l2_normalize('[0,6e-8]'::halfvec);

-- section: denormal
SELECT l2_distance('[1e-20]'::vector, '[0]'), l2_distance('[1e-40]'::vector, '[0]'), l1_distance('[1e-40]'::vector, '[-1e-40]'), l1_distance('[1.4e-45]'::vector, '[0]');
SELECT inner_product('[1e-40]'::vector, '[1e-5]'), inner_product('[1e-20]'::vector, '[1e-20]'), inner_product('[1e-40]'::vector, '[1e-40]'), '[1e-20]'::vector <#> '[1e-20]';
SELECT cosine_distance('[1e-20]'::vector, '[2e-20]'), '[1e-40]'::vector <=> '[1e-40]', '[1e-20,0,0,0,0]'::vector <-> '[0,0,0,0,0]', '[0,0,0,1.4e-45]'::vector <+> '[0,0,0,0]';
SELECT vector_norm('[1e-40]'), vector_norm('[1e-40,1e-40]'), vector_norm('[1.4e-45]');
SELECT l2_distance('[6e-8]'::halfvec, '[0]'), l1_distance('[6e-8]'::halfvec, '[-6e-8]'), inner_product('[6e-8]'::halfvec, '[6e-8]'), cosine_distance('[6e-8]'::halfvec, '[1.2e-7]');
SELECT l2_norm('[6e-8]'::halfvec), l2_norm('[6e-8,-6e-8]'::halfvec), '[6e-8]'::halfvec <-> '[1.2e-7]', '[6e-8]'::halfvec <+> '[0]', '[6e-8]'::halfvec <#> '[1]';
SELECT l2_norm('{1:1e-40}/1'::sparsevec), l2_norm('{1:1.4e-45}/3'::sparsevec), l2_distance('{1:1e-20}/2'::sparsevec, '{}/2'), l2_distance('{1:1e-40}/2'::sparsevec, '{2:1e-40}/2');
SELECT inner_product('{1:1e-40}/2'::sparsevec, '{1:1e-5}/2'), inner_product('{2:1e-20}/2'::sparsevec, '{2:1e-20}/2'), l1_distance('{1:1e-40}/2'::sparsevec, '{2:-1.4e-45}/2'), '{1:1e-20}/1'::sparsevec <=> '{1:2e-20}/1';

-- section: agg
SELECT avg(v), sum(v) FROM (VALUES ('[1,2,3]'::halfvec), ('[3,5,7]'), (NULL)) t(v);
SELECT avg(v), sum(v) FROM (VALUES ('[1,2,3]'::vector), ('[3,5,7]')) t(v);
SELECT avg(v) FROM (VALUES ('[0.1,0.2]'::halfvec), ('[0.3,0.4]'), ('[0.5,0.6]')) t(v);
SELECT avg(v), sum(v) FROM (SELECT '[1]'::halfvec WHERE false) t(v);
SELECT avg(v) FROM (VALUES ('[1,2]'::halfvec), ('[3]')) t(v);
SELECT sum(v) FROM (VALUES ('[65504]'::halfvec), ('[65504]')) t(v);
SELECT avg(v) FROM (VALUES ('[65504]'::halfvec), ('[65504]')) t(v);
SELECT halfvec_accum('{0}', '[1,2,3]'), halfvec_accum('{1,1,2,3}', '[1,2,3]');
SELECT halfvec_accum('{0,0}', '[1,2,3]');
SELECT halfvec_accum('{1,1.7976931348623157e308}', '[65504]');
SELECT halfvec_avg('{2,2,4,6}'), halfvec_avg('{0}');
SELECT halfvec_avg('{{2,2,4,6}}');
SELECT halfvec_avg('{1,70000}');
SELECT halfvec_combine('{1,2,3}', '{2,4,6}'), halfvec_combine('{0}', '{1,1}');

-- section: cmp
SELECT '[1,2,3]'::halfvec < '[1,2,3]', '[1,2,3]'::halfvec <= '[1,2]', '[1,2]'::halfvec = '[1,2]', '[1,2]'::halfvec <> '[1,2,3]', '[2]'::halfvec >= '[1,9]', '[0.1]'::halfvec > '[0.10001]';
SELECT halfvec_cmp('[1,2]', '[1,2,3]'), halfvec_cmp('[2,3]', '[1,2,3]'), halfvec_cmp('[-0]', '[0]');
SELECT '{1:1,2:2,3:3}/3'::sparsevec < '{1:1,2:2}/2', '{1:1}/2'::sparsevec = '{1:1}/2', '{1:1}/2'::sparsevec <> '{1:1}/3', '{1:1}/2'::sparsevec >= '{2:1}/2';
SELECT sparsevec_cmp('{1:1,2:2}/2', '{1:2,2:3,3:4}/3'), sparsevec_cmp('{1:2,2:3}/2', '{1:1,2:2,3:3}/3'), sparsevec_cmp('{2:-1}/3', '{1:1}/3'), sparsevec_cmp('{1:-1}/3', '{2:1}/3');
SELECT sparsevec_cmp('{}/3', '{3:-1}/3'), sparsevec_cmp('{3:1}/3', '{}/2'), sparsevec_cmp('{1:1}/3', '{1:1,3:-1}/3'), sparsevec_cmp('{1:1,2:5}/2', '{1:1}/1');
SELECT '{1:2}/2'::sparsevec > '{1:1}/2', '{1:1}/2'::sparsevec > '{1:1}/2', '{}/3'::sparsevec > '{3:-1}/3', '{1:1}/2'::sparsevec <= '{1:1}/3';
SELECT v FROM (VALUES ('[1,2]'::halfvec), ('[1]'), ('[0,5]'), ('[1,2,0]'), ('[-1]')) t(v) ORDER BY v;
SELECT v FROM (VALUES ('{1:1}/3'::sparsevec), ('{}/3'), ('{2:-1}/3'), ('{1:-1}/2'), ('{3:2}/3'), ('{}/2')) t(v) ORDER BY v;
SELECT DISTINCT v FROM (VALUES ('[1,2]'::halfvec), ('[1,2]'), ('[2,1]')) t(v) ORDER BY v;

-- section: misc
SELECT vector_dims('[1,2,3]'::halfvec), vector_dims('[1]'::vector);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2, 3), subvector('[1,2,3,4,5]'::halfvec, -1, 3), subvector('[1,2,3,4,5]'::halfvec, 3, 9);
SELECT subvector('[1,2,3,4,5]'::halfvec, 3, 2147483647), subvector('[1,2,3,4,5]'::halfvec, -2147483644, 2147483647);
SELECT subvector('[1,2,3,4,5]'::halfvec, 1, 0);
SELECT subvector('[1,2,3,4,5]'::halfvec, -1, 2);
SELECT subvector('[1,2,3,4,5]'::halfvec, 6, 1);
SELECT subvector('[1,2,3,4,5]'::halfvec, 2147483647, 10);
SELECT binary_quantize('[1,0,-1,0.5,-0.5,2,-2,3,0.25]'::vector), binary_quantize('[1,0,-1,0.5,-0.5,2,-2,3,0.25]'::halfvec);
SELECT binary_quantize('[0,-0]'::halfvec), binary_quantize('[6e-8]'::halfvec), binary_quantize('[-6e-8]'::halfvec);
SELECT binary_quantize('[1,2,3,-4,5,6,-7,8,1,-2,-3,4,5,-6,7,8,-1,2,3]'::halfvec) <~> binary_quantize('[1,2,3,-4,5,6,-7,8,1,-2,-3,4,5,-6,7,8,-1,2,3]'::vector);
SELECT pg_typeof('[1]'::vector::halfvec), pg_typeof('[1]'::vector::sparsevec), pg_typeof('[1]'::halfvec::vector), pg_typeof('[1]'::halfvec::sparsevec), pg_typeof('[1]'::halfvec::real[]), pg_typeof('{1:1}/1'::sparsevec::vector), pg_typeof('{1:1}/1'::sparsevec::halfvec), pg_typeof(ARRAY[1]::halfvec), pg_typeof(ARRAY[1]::sparsevec), pg_typeof('[1]'::halfvec + '[1]'), pg_typeof('[1]'::halfvec - '[1]'), pg_typeof('[1]'::halfvec * '[1]'), pg_typeof('[1]'::halfvec || '[1]'), pg_typeof('[1]'::vector + '[1]'), pg_typeof(l2_normalize('[1]'::vector)), pg_typeof(l2_normalize('[1]'::halfvec)), pg_typeof(l2_normalize('{1:1}/1'::sparsevec)), pg_typeof(subvector('[1,2]'::halfvec, 1, 1)), pg_typeof(binary_quantize('[1]'::halfvec)), pg_typeof(l2_norm('[1]'::halfvec)), pg_typeof(l2_norm('{1:1}/1'::sparsevec)), (SELECT pg_typeof(avg(v)) FROM (VALUES ('[1]'::halfvec)) t(v)), (SELECT pg_typeof(sum(v)) FROM (VALUES ('[1]'::halfvec)) t(v)), pg_typeof(halfvec_accum('{0}', '[1]')), pg_typeof(halfvec_avg('{1,1}'));

-- section: bit
SELECT hamming_distance('111', '111'), hamming_distance('111', '010'), hamming_distance('', ''), jaccard_distance('', '');
SELECT hamming_distance(repeat('10', 300)::bit(600), repeat('01', 300)::bit(600)), jaccard_distance(repeat('110', 200)::bit(600), repeat('011', 200)::bit(600));
SELECT '1010'::bit(4) <~> '0101', '1100'::bit(4) <%> '1010', '1111'::varbit <~> '0000'::varbit;
SELECT jaccard_distance('0000', '0000'), jaccard_distance('1000', '0100'), jaccard_distance('1100', '1000');
SELECT hamming_distance('111', '000'::varbit(4)), jaccard_distance('1111', '0000'::varbit(5));
SELECT hamming_distance('111', '0000'::varbit(4));
SELECT jaccard_distance('1111', '000');

-- section: knn
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <-> '[1,1]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <#> '[1,2]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::vector), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <=> '[1,2]', i;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <-> '[1,1]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <+> '[1,2]', i LIMIT 4;
SELECT i FROM (VALUES (1, '[1,1]'::halfvec), (2, '[2,2]'), (3, '[0,1]'), (4, '[1,0]'), (5, '[-1,-1]'), (6, '[0,0]')) t(i, v) ORDER BY v <=> '[1,2]', i;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <-> '{1:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <#> '{1:1,3:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <+> '{2:1}/3', i LIMIT 4;
SELECT i FROM (VALUES (1, '{1:1}/3'::sparsevec), (2, '{2:2}/3'), (3, '{}/3'), (4, '{1:1,3:1}/3'), (5, '{3:-2}/3'), (6, '{1:2,2:2,3:2}/3')) t(i, v) ORDER BY v <=> '{1:1}/3', i;
SELECT i FROM (VALUES (1, B'1010'), (2, B'0101'), (3, B'1110'), (4, B'1011')) t(i, v) ORDER BY v <~> '1010', i;
SELECT i FROM (VALUES (1, B'1010'), (2, B'0101'), (3, B'1110'), (4, B'1011')) t(i, v) ORDER BY v <%> '1010', i;

-- section: limits
SELECT vector_dims(array_fill(1, ARRAY[16000])::halfvec), l2_norm(array_fill(1, ARRAY[16000])::halfvec);
SELECT ('[' || array_to_string(array_fill(1, ARRAY[16001]), ',') || ']')::halfvec IS NULL;
SELECT '{1000000000:1}/1000000000'::sparsevec <-> '{1:1}/1000000000', l2_norm('{1000000000:-2}/1000000000'::sparsevec);
SELECT l2_normalize('{1000000000:4,1:3}/1000000000'::sparsevec);
SELECT '{1000000000:1}/1000000000'::sparsevec::vector;
SELECT '{1000000000:1}/1000000000'::sparsevec::halfvec;
SELECT ('{' || repeat('1:1,', 16000) || '1:1}/1')::sparsevec IS NULL;
SELECT vector_dims(array_agg(n)::sparsevec::vector) FROM generate_series(1, 16000) n;
SELECT array_agg(n % 2)::sparsevec IS NOT NULL FROM generate_series(1, 32000) n;
SELECT ('{' || string_agg(n || ':1', ',') || '}/16000')::sparsevec IS NOT NULL FROM generate_series(1, 16000) n;

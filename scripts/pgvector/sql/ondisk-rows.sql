-- Rows :lo..:hi of every on-disk fixture table (run-ondisk.sh). Included by
-- ondisk-fixture.sql, ondisk-crash.sql, ondisk-inflight.sql and
-- ondisk-followup.sql with \set lo / \set hi. Every value keeps the
-- distances that index scans return exact, whatever the summation order or
-- FMA (only IVFFlat's distances from a query to its k-means centers round;
-- docs/superpowers/reports/pgvector-m5-report.md, "The fixture and the
-- float question"):
--   v, h, s: small integers (L2, inner product and L1 sums stay below 2^24);
--   vc, hc, sc, gc, ghc: 16 entries of +-1, so every norm is 4 and every
--     normalized entry is +-0.25 (cosine opclasses);
--   b, cb: bits (Hamming and Jaccard counts);
--   g, gh: integer points around ten irregular centers 50 * (0..31) apart
--     per dimension (IVFFlat), offsets -1..2;
--   e: 1536 md5-derived integers in 0..4 (6,152 bytes, stored out of line in
--     TOAST).
INSERT INTO vt
SELECT i,
  ARRAY[i % 7, i % 11, i % 13, i % 17, i % 19, i % 23, i % 29, i % 31]::vector,
  ARRAY[(i * 3) % 17, (i * 5) % 19, (i * 7) % 23, (i * 11) % 29, (i * 13) % 31, (i * 2) % 13, (i * 4) % 11, (i * 6) % 7]::halfvec,
  format('{%s:%s,%s:%s,%s:%s,%s:%s}/32', 1 + i % 8, 1 + i % 5, 9 + (i / 8) % 8, 1 + i % 7,
         17 + (i / 64) % 8, 1 + i % 3, 25 + (i * 7) % 8, 1 + i % 11)::sparsevec,
  ('x' || substr(md5(i::text), 1, 16))::bit(64),
  (SELECT array_agg((1 - 2 * ((i::bigint * 2654435761 >> (j + 16)) & 1))::int ORDER BY j) FROM generate_series(0, 15) j)::vector,
  (SELECT array_agg((1 - 2 * ((i::bigint * 2246822519 >> (j + 16)) & 1))::int ORDER BY j) FROM generate_series(0, 15) j)::halfvec,
  (SELECT '{' || string_agg(format('%s:%s', 4 * j + ((i::bigint * 2654435761 >> (2 * j + 12)) & 3) + 1,
                                   1 - 2 * ((i::bigint * 2246822519 >> (j + 20)) & 1)), ',' ORDER BY j) || '}/64'
     FROM generate_series(0, 15) j)::sparsevec
FROM generate_series(:lo, :hi) i;

INSERT INTO ct
SELECT i,
  (SELECT array_agg((50 * (((i % 10)::bigint * 2654435761 >> (3 * d + 8)) & 31) + ((i::bigint * 2246822519 >> (2 * d + 12)) & 3) - 1)::int ORDER BY d)
     FROM generate_series(0, 7) d)::vector,
  (SELECT array_agg((50 * (((i % 10)::bigint * 2654435761 >> (3 * d + 8)) & 31) + ((i::bigint * 2246822519 >> (2 * d + 12)) & 3) - 1)::int ORDER BY d)
     FROM generate_series(0, 7) d)::halfvec,
  (SELECT array_agg((1 - 2 * ((((i % 10)::bigint * 2654435761 >> (d + 16)) & 1) # (((i::bigint * 2246822519 >> (d + 20)) & 15) = 0)::int))::int ORDER BY d)
     FROM generate_series(0, 15) d)::vector,
  (SELECT array_agg((1 - 2 * ((((i % 10)::bigint * 2654435761 >> (d + 16)) & 1) # (((i::bigint * 2246822519 >> (d + 20)) & 15) = 0)::int))::int ORDER BY d)
     FROM generate_series(0, 15) d)::halfvec,
  ('x' || substr(md5((i % 10)::text), 1, 16))::bit(64)
    # (('x' || substr(md5(i::text), 1, 16))::bit(64) & ('x' || substr(md5((i * 7)::text), 1, 16))::bit(64)
       & ('x' || substr(md5((i * 13)::text), 1, 16))::bit(64))
FROM generate_series(:lo, :hi) i;

INSERT INTO ut
SELECT i, ARRAY[i % 5, i % 7, i % 9, i % 11]::vector
FROM generate_series(:lo, :hi) i WHERE i % 6 = 0;

INSERT INTO wide
SELECT i, (SELECT array_agg(get_byte(decode(md5(i || '/' || j / 16), 'hex'), j % 16) % 5 ORDER BY j) FROM generate_series(1, 1536) j)::vector
FROM generate_series(:lo, :hi) i WHERE i % 10 = 0;

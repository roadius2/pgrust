-- Repair for databases created by pgrust builds before M4 (pgvector
-- extversion 0.8.5 from pgrust's trimmed install script). Those builds
-- shipped a subset of upstream vector.sql under the 0.8.5 label, so
-- ALTER EXTENSION vector UPDATE fails at vector--0.8.6--0.8.7.sql (its
-- COMMENTs name objects the trimmed script never created). This script
-- creates exactly the missing upstream objects as members of the
-- extension; then run ALTER EXTENSION vector UPDATE. Run as a superuser,
-- once in each database whose vector is at 0.8.5; it is atomic, idempotent,
-- and a no-op elsewhere. Generated from upstream sql/vector.sql by
-- crates/contrib/pgvector/src/repair_gen.rs; do not edit by hand.
DO $repair$
DECLARE
  v_ver text;
  v_nsp text;
  v_nsp_oid oid;
  v_old_path text := current_setting('search_path');
  v_old_cmm text := current_setting('client_min_messages');
  n int := 0;
BEGIN
  SELECT e.extversion, n2.nspname, n2.oid INTO v_ver, v_nsp, v_nsp_oid
    FROM pg_extension e JOIN pg_namespace n2 ON n2.oid = e.extnamespace
   WHERE e.extname = 'vector';
  IF v_ver IS NULL THEN
    RAISE EXCEPTION 'extension "vector" is not installed in this database';
  END IF;
  IF v_ver <> '0.8.5' THEN
    RAISE NOTICE 'vector is at %; nothing to repair', v_ver;
    RETURN;
  END IF;
  -- As execute_extension_script runs an extension script: the extension's
  -- schema, then pg_temp, with pg_catalog searched implicitly before both
  -- (listing it after the schema "would be bad for security",
  -- extension.c:1290-1312); and notices below WARNING off
  -- (extension.c:1271-1274).
  PERFORM set_config('search_path', quote_ident(v_nsp) || ', pg_temp', true);
  IF current_setting('client_min_messages') NOT IN ('warning', 'error') THEN
    PERFORM set_config('client_min_messages', 'warning', true);
  END IF;
  IF to_regtype('vector') IS NULL THEN
    CREATE TYPE vector;
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_in(cstring, oid, integer)') IS NULL THEN
    CREATE FUNCTION vector_in(cstring, oid, integer) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_in(cstring, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_out(vector)') IS NULL THEN
    CREATE FUNCTION vector_out(vector) RETURNS cstring
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_out(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_typmod_in(cstring[])') IS NULL THEN
    CREATE FUNCTION vector_typmod_in(cstring[]) RETURNS integer
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_typmod_in(cstring[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_recv(internal, oid, integer)') IS NULL THEN
    CREATE FUNCTION vector_recv(internal, oid, integer) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_recv(internal, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_send(vector)') IS NULL THEN
    CREATE FUNCTION vector_send(vector) RETURNS bytea
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_send(vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'vector' AND typnamespace = v_nsp_oid AND typisdefined) THEN
    CREATE TYPE vector (
    	INPUT     = vector_in,
    	OUTPUT    = vector_out,
    	TYPMOD_IN = vector_typmod_in,
    	RECEIVE   = vector_recv,
    	SEND      = vector_send,
    	STORAGE   = external
    );
    ALTER EXTENSION vector ADD TYPE vector;
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_distance(vector, vector)') IS NULL THEN
    CREATE FUNCTION l2_distance(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_distance(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('inner_product(vector, vector)') IS NULL THEN
    CREATE FUNCTION inner_product(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION inner_product(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('cosine_distance(vector, vector)') IS NULL THEN
    CREATE FUNCTION cosine_distance(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION cosine_distance(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('l1_distance(vector, vector)') IS NULL THEN
    CREATE FUNCTION l1_distance(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l1_distance(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_dims(vector)') IS NULL THEN
    CREATE FUNCTION vector_dims(vector) RETURNS integer
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_dims(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_norm(vector)') IS NULL THEN
    CREATE FUNCTION vector_norm(vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_norm(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_normalize(vector)') IS NULL THEN
    CREATE FUNCTION l2_normalize(vector) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_normalize(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('binary_quantize(vector)') IS NULL THEN
    CREATE FUNCTION binary_quantize(vector) RETURNS bit
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION binary_quantize(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('subvector(vector, int, int)') IS NULL THEN
    CREATE FUNCTION subvector(vector, int, int) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION subvector(vector, int, int);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_add(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_add(vector, vector) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_add(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_sub(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_sub(vector, vector) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_sub(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_mul(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_mul(vector, vector) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_mul(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_concat(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_concat(vector, vector) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_concat(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_lt(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_lt(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_lt(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_le(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_le(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_le(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_eq(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_eq(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_eq(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_ne(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_ne(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_ne(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_ge(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_ge(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_ge(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_gt(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_gt(vector, vector) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_gt(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_cmp(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_cmp(vector, vector) RETURNS int4
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_cmp(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_l2_squared_distance(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_l2_squared_distance(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_l2_squared_distance(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_negative_inner_product(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_negative_inner_product(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_negative_inner_product(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_spherical_distance(vector, vector)') IS NULL THEN
    CREATE FUNCTION vector_spherical_distance(vector, vector) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_spherical_distance(vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_accum(double precision[], vector)') IS NULL THEN
    CREATE FUNCTION vector_accum(double precision[], vector) RETURNS double precision[]
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_accum(double precision[], vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_avg(double precision[])') IS NULL THEN
    CREATE FUNCTION vector_avg(double precision[]) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_avg(double precision[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_combine(double precision[], double precision[])') IS NULL THEN
    CREATE FUNCTION vector_combine(double precision[], double precision[]) RETURNS double precision[]
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_combine(double precision[], double precision[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('avg(vector)') IS NULL THEN
    CREATE AGGREGATE avg(vector) (
    	SFUNC = vector_accum,
    	STYPE = double precision[],
    	FINALFUNC = vector_avg,
    	COMBINEFUNC = vector_combine,
    	INITCOND = '{0}',
    	PARALLEL = SAFE
    );
    ALTER EXTENSION vector ADD AGGREGATE avg(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('sum(vector)') IS NULL THEN
    CREATE AGGREGATE sum(vector) (
    	SFUNC = vector_add,
    	STYPE = vector,
    	COMBINEFUNC = vector_add,
    	PARALLEL = SAFE
    );
    ALTER EXTENSION vector ADD AGGREGATE sum(vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector(vector, integer, boolean)') IS NULL THEN
    CREATE FUNCTION vector(vector, integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector(vector, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_vector(integer[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_vector(integer[], integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_vector(integer[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_vector(real[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_vector(real[], integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_vector(real[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_vector(double precision[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_vector(double precision[], integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_vector(double precision[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_vector(numeric[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_vector(numeric[], integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_vector(numeric[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_to_float4(vector, integer, boolean)') IS NULL THEN
    CREATE FUNCTION vector_to_float4(vector, integer, boolean) RETURNS real[]
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_to_float4(vector, integer, boolean);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('vector') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (vector AS vector)
    	WITH FUNCTION vector(vector, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (vector AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('vector') AND casttarget = to_regtype('real[]')) THEN
    CREATE CAST (vector AS real[])
    	WITH FUNCTION vector_to_float4(vector, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (vector AS real[]);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('integer[]') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (integer[] AS vector)
    	WITH FUNCTION array_to_vector(integer[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (integer[] AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('real[]') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (real[] AS vector)
    	WITH FUNCTION array_to_vector(real[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (real[] AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('double precision[]') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (double precision[] AS vector)
    	WITH FUNCTION array_to_vector(double precision[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (double precision[] AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('numeric[]') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (numeric[] AS vector)
    	WITH FUNCTION array_to_vector(numeric[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (numeric[] AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<->(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <-> (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = l2_distance,
    	COMMUTATOR = '<->'
    );
    ALTER EXTENSION vector ADD OPERATOR <-> (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<#>(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <#> (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_negative_inner_product,
    	COMMUTATOR = '<#>'
    );
    ALTER EXTENSION vector ADD OPERATOR <#> (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=>(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <=> (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = cosine_distance,
    	COMMUTATOR = '<=>'
    );
    ALTER EXTENSION vector ADD OPERATOR <=> (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<+>(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <+> (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = l1_distance,
    	COMMUTATOR = '<+>'
    );
    ALTER EXTENSION vector ADD OPERATOR <+> (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('+(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR + (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_add,
    	COMMUTATOR = +
    );
    ALTER EXTENSION vector ADD OPERATOR + (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('-(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR - (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_sub
    );
    ALTER EXTENSION vector ADD OPERATOR - (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('*(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR * (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_mul,
    	COMMUTATOR = *
    );
    ALTER EXTENSION vector ADD OPERATOR * (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('||(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR || (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_concat
    );
    ALTER EXTENSION vector ADD OPERATOR || (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR < (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_lt,
    	COMMUTATOR = > , NEGATOR = >= ,
    	RESTRICT = scalarltsel, JOIN = scalarltjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR < (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <= (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_le,
    	COMMUTATOR = >= , NEGATOR = > ,
    	RESTRICT = scalarlesel, JOIN = scalarlejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <= (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('=(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR = (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_eq,
    	COMMUTATOR = = , NEGATOR = <> ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR = (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<>(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <> (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_ne,
    	COMMUTATOR = <> , NEGATOR = = ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <> (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>=(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR >= (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_ge,
    	COMMUTATOR = <= , NEGATOR = < ,
    	RESTRICT = scalargesel, JOIN = scalargejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR >= (vector, vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>(vector,vector)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR > (
    	LEFTARG = vector, RIGHTARG = vector, PROCEDURE = vector_gt,
    	COMMUTATOR = < , NEGATOR = <= ,
    	RESTRICT = scalargtsel, JOIN = scalargtjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR > (vector, vector);
    n := n + 1;
  END IF;
  IF to_regprocedure('ivfflathandler(internal)') IS NULL THEN
    CREATE FUNCTION ivfflathandler(internal) RETURNS index_am_handler
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION ivfflathandler(internal);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_am WHERE amname = 'ivfflat') THEN
    CREATE ACCESS METHOD ivfflat TYPE INDEX HANDLER ivfflathandler;
    ALTER EXTENSION vector ADD ACCESS METHOD ivfflat;
    COMMENT ON ACCESS METHOD ivfflat IS 'ivfflat index access method';
    n := n + 1;
  END IF;
  IF to_regprocedure('hnswhandler(internal)') IS NULL THEN
    CREATE FUNCTION hnswhandler(internal) RETURNS index_am_handler
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION hnswhandler(internal);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_am WHERE amname = 'hnsw') THEN
    CREATE ACCESS METHOD hnsw TYPE INDEX HANDLER hnswhandler;
    ALTER EXTENSION vector ADD ACCESS METHOD hnsw;
    COMMENT ON ACCESS METHOD hnsw IS 'hnsw index access method';
    n := n + 1;
  END IF;
  IF to_regprocedure('ivfflat_halfvec_support(internal)') IS NULL THEN
    CREATE FUNCTION ivfflat_halfvec_support(internal) RETURNS internal
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION ivfflat_halfvec_support(internal);
    n := n + 1;
  END IF;
  IF to_regprocedure('ivfflat_bit_support(internal)') IS NULL THEN
    CREATE FUNCTION ivfflat_bit_support(internal) RETURNS internal
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION ivfflat_bit_support(internal);
    n := n + 1;
  END IF;
  IF to_regprocedure('hnsw_halfvec_support(internal)') IS NULL THEN
    CREATE FUNCTION hnsw_halfvec_support(internal) RETURNS internal
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION hnsw_halfvec_support(internal);
    n := n + 1;
  END IF;
  IF to_regprocedure('hnsw_bit_support(internal)') IS NULL THEN
    CREATE FUNCTION hnsw_bit_support(internal) RETURNS internal
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION hnsw_bit_support(internal);
    n := n + 1;
  END IF;
  IF to_regprocedure('hnsw_sparsevec_support(internal)') IS NULL THEN
    CREATE FUNCTION hnsw_sparsevec_support(internal) RETURNS internal
    	AS '$libdir/vector' LANGUAGE C;
    ALTER EXTENSION vector ADD FUNCTION hnsw_sparsevec_support(internal);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_ops' AND a.amname = 'btree' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_ops
    	DEFAULT FOR TYPE vector USING btree AS
    	OPERATOR 1 < ,
    	OPERATOR 2 <= ,
    	OPERATOR 3 = ,
    	OPERATOR 4 >= ,
    	OPERATOR 5 > ,
    	FUNCTION 1 vector_cmp(vector, vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_ops USING btree;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_ops USING btree;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_l2_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_l2_ops
    	DEFAULT FOR TYPE vector USING ivfflat AS
    	OPERATOR 1 <-> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_l2_squared_distance(vector, vector),
    	FUNCTION 3 l2_distance(vector, vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_l2_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_l2_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_ip_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_ip_ops
    	FOR TYPE vector USING ivfflat AS
    	OPERATOR 1 <#> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_negative_inner_product(vector, vector),
    	FUNCTION 3 vector_spherical_distance(vector, vector),
    	FUNCTION 4 vector_norm(vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_ip_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_ip_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_cosine_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_cosine_ops
    	FOR TYPE vector USING ivfflat AS
    	OPERATOR 1 <=> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_negative_inner_product(vector, vector),
    	FUNCTION 2 vector_norm(vector),
    	FUNCTION 3 vector_spherical_distance(vector, vector),
    	FUNCTION 4 vector_norm(vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_cosine_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_cosine_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_l2_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_l2_ops
    	FOR TYPE vector USING hnsw AS
    	OPERATOR 1 <-> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_l2_squared_distance(vector, vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_l2_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_l2_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_ip_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_ip_ops
    	FOR TYPE vector USING hnsw AS
    	OPERATOR 1 <#> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_negative_inner_product(vector, vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_ip_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_ip_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_cosine_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_cosine_ops
    	FOR TYPE vector USING hnsw AS
    	OPERATOR 1 <=> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 vector_negative_inner_product(vector, vector),
    	FUNCTION 2 vector_norm(vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_cosine_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_cosine_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'vector_l1_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS vector_l1_ops
    	FOR TYPE vector USING hnsw AS
    	OPERATOR 1 <+> (vector, vector) FOR ORDER BY float_ops,
    	FUNCTION 1 l1_distance(vector, vector);
    ALTER EXTENSION vector ADD OPERATOR CLASS vector_l1_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY vector_l1_ops USING hnsw;
    n := n + 1;
  END IF;
  IF to_regtype('halfvec') IS NULL THEN
    CREATE TYPE halfvec;
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_in(cstring, oid, integer)') IS NULL THEN
    CREATE FUNCTION halfvec_in(cstring, oid, integer) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_in(cstring, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_out(halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_out(halfvec) RETURNS cstring
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_out(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_typmod_in(cstring[])') IS NULL THEN
    CREATE FUNCTION halfvec_typmod_in(cstring[]) RETURNS integer
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_typmod_in(cstring[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_recv(internal, oid, integer)') IS NULL THEN
    CREATE FUNCTION halfvec_recv(internal, oid, integer) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_recv(internal, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_send(halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_send(halfvec) RETURNS bytea
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_send(halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'halfvec' AND typnamespace = v_nsp_oid AND typisdefined) THEN
    CREATE TYPE halfvec (
    	INPUT     = halfvec_in,
    	OUTPUT    = halfvec_out,
    	TYPMOD_IN = halfvec_typmod_in,
    	RECEIVE   = halfvec_recv,
    	SEND      = halfvec_send,
    	STORAGE   = external
    );
    ALTER EXTENSION vector ADD TYPE halfvec;
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_distance(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION l2_distance(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector', 'halfvec_l2_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_distance(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('inner_product(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION inner_product(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector', 'halfvec_inner_product' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION inner_product(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('cosine_distance(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION cosine_distance(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector', 'halfvec_cosine_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION cosine_distance(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l1_distance(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION l1_distance(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector', 'halfvec_l1_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l1_distance(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_dims(halfvec)') IS NULL THEN
    CREATE FUNCTION vector_dims(halfvec) RETURNS integer
    	AS '$libdir/vector', 'halfvec_vector_dims' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_dims(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_norm(halfvec)') IS NULL THEN
    CREATE FUNCTION l2_norm(halfvec) RETURNS float8
    	AS '$libdir/vector', 'halfvec_l2_norm' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_norm(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_normalize(halfvec)') IS NULL THEN
    CREATE FUNCTION l2_normalize(halfvec) RETURNS halfvec
    	AS '$libdir/vector', 'halfvec_l2_normalize' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_normalize(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('binary_quantize(halfvec)') IS NULL THEN
    CREATE FUNCTION binary_quantize(halfvec) RETURNS bit
    	AS '$libdir/vector', 'halfvec_binary_quantize' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION binary_quantize(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('subvector(halfvec, int, int)') IS NULL THEN
    CREATE FUNCTION subvector(halfvec, int, int) RETURNS halfvec
    	AS '$libdir/vector', 'halfvec_subvector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION subvector(halfvec, int, int);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_add(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_add(halfvec, halfvec) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_add(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_sub(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_sub(halfvec, halfvec) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_sub(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_mul(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_mul(halfvec, halfvec) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_mul(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_concat(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_concat(halfvec, halfvec) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_concat(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_lt(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_lt(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_lt(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_le(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_le(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_le(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_eq(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_eq(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_eq(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_ne(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_ne(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_ne(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_ge(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_ge(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_ge(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_gt(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_gt(halfvec, halfvec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_gt(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_cmp(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_cmp(halfvec, halfvec) RETURNS int4
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_cmp(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_l2_squared_distance(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_l2_squared_distance(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_l2_squared_distance(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_negative_inner_product(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_negative_inner_product(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_negative_inner_product(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_spherical_distance(halfvec, halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_spherical_distance(halfvec, halfvec) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_spherical_distance(halfvec, halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_accum(double precision[], halfvec)') IS NULL THEN
    CREATE FUNCTION halfvec_accum(double precision[], halfvec) RETURNS double precision[]
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_accum(double precision[], halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_avg(double precision[])') IS NULL THEN
    CREATE FUNCTION halfvec_avg(double precision[]) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_avg(double precision[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_combine(double precision[], double precision[])') IS NULL THEN
    CREATE FUNCTION halfvec_combine(double precision[], double precision[]) RETURNS double precision[]
    	AS '$libdir/vector', 'vector_combine' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_combine(double precision[], double precision[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('avg(halfvec)') IS NULL THEN
    CREATE AGGREGATE avg(halfvec) (
    	SFUNC = halfvec_accum,
    	STYPE = double precision[],
    	FINALFUNC = halfvec_avg,
    	COMBINEFUNC = halfvec_combine,
    	INITCOND = '{0}',
    	PARALLEL = SAFE
    );
    ALTER EXTENSION vector ADD AGGREGATE avg(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sum(halfvec)') IS NULL THEN
    CREATE AGGREGATE sum(halfvec) (
    	SFUNC = halfvec_add,
    	STYPE = halfvec,
    	COMBINEFUNC = halfvec_add,
    	PARALLEL = SAFE
    );
    ALTER EXTENSION vector ADD AGGREGATE sum(halfvec);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec(halfvec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION halfvec(halfvec, integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec(halfvec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_to_vector(halfvec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION halfvec_to_vector(halfvec, integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_to_vector(halfvec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_to_halfvec(vector, integer, boolean)') IS NULL THEN
    CREATE FUNCTION vector_to_halfvec(vector, integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_to_halfvec(vector, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_halfvec(integer[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_halfvec(integer[], integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_halfvec(integer[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_halfvec(real[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_halfvec(real[], integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_halfvec(real[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_halfvec(double precision[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_halfvec(double precision[], integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_halfvec(double precision[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_halfvec(numeric[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_halfvec(numeric[], integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_halfvec(numeric[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_to_float4(halfvec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION halfvec_to_float4(halfvec, integer, boolean) RETURNS real[]
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_to_float4(halfvec, integer, boolean);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('halfvec') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (halfvec AS halfvec)
    	WITH FUNCTION halfvec(halfvec, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (halfvec AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('halfvec') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (halfvec AS vector)
    	WITH FUNCTION halfvec_to_vector(halfvec, integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (halfvec AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('vector') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (vector AS halfvec)
    	WITH FUNCTION vector_to_halfvec(vector, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (vector AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('halfvec') AND casttarget = to_regtype('real[]')) THEN
    CREATE CAST (halfvec AS real[])
    	WITH FUNCTION halfvec_to_float4(halfvec, integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (halfvec AS real[]);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('integer[]') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (integer[] AS halfvec)
    	WITH FUNCTION array_to_halfvec(integer[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (integer[] AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('real[]') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (real[] AS halfvec)
    	WITH FUNCTION array_to_halfvec(real[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (real[] AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('double precision[]') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (double precision[] AS halfvec)
    	WITH FUNCTION array_to_halfvec(double precision[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (double precision[] AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('numeric[]') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (numeric[] AS halfvec)
    	WITH FUNCTION array_to_halfvec(numeric[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (numeric[] AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<->(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <-> (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = l2_distance,
    	COMMUTATOR = '<->'
    );
    ALTER EXTENSION vector ADD OPERATOR <-> (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<#>(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <#> (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_negative_inner_product,
    	COMMUTATOR = '<#>'
    );
    ALTER EXTENSION vector ADD OPERATOR <#> (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=>(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <=> (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = cosine_distance,
    	COMMUTATOR = '<=>'
    );
    ALTER EXTENSION vector ADD OPERATOR <=> (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<+>(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <+> (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = l1_distance,
    	COMMUTATOR = '<+>'
    );
    ALTER EXTENSION vector ADD OPERATOR <+> (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('+(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR + (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_add,
    	COMMUTATOR = +
    );
    ALTER EXTENSION vector ADD OPERATOR + (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('-(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR - (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_sub
    );
    ALTER EXTENSION vector ADD OPERATOR - (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('*(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR * (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_mul,
    	COMMUTATOR = *
    );
    ALTER EXTENSION vector ADD OPERATOR * (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('||(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR || (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_concat
    );
    ALTER EXTENSION vector ADD OPERATOR || (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR < (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_lt,
    	COMMUTATOR = > , NEGATOR = >= ,
    	RESTRICT = scalarltsel, JOIN = scalarltjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR < (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <= (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_le,
    	COMMUTATOR = >= , NEGATOR = > ,
    	RESTRICT = scalarlesel, JOIN = scalarlejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <= (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('=(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR = (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_eq,
    	COMMUTATOR = = , NEGATOR = <> ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR = (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<>(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <> (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_ne,
    	COMMUTATOR = <> , NEGATOR = = ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <> (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>=(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR >= (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_ge,
    	COMMUTATOR = <= , NEGATOR = < ,
    	RESTRICT = scalargesel, JOIN = scalargejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR >= (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>(halfvec,halfvec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR > (
    	LEFTARG = halfvec, RIGHTARG = halfvec, PROCEDURE = halfvec_gt,
    	COMMUTATOR = < , NEGATOR = <= ,
    	RESTRICT = scalargtsel, JOIN = scalargtjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR > (halfvec, halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_ops' AND a.amname = 'btree' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_ops
    	DEFAULT FOR TYPE halfvec USING btree AS
    	OPERATOR 1 < ,
    	OPERATOR 2 <= ,
    	OPERATOR 3 = ,
    	OPERATOR 4 >= ,
    	OPERATOR 5 > ,
    	FUNCTION 1 halfvec_cmp(halfvec, halfvec);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_ops USING btree;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_ops USING btree;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_l2_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_l2_ops
    	FOR TYPE halfvec USING ivfflat AS
    	OPERATOR 1 <-> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_l2_squared_distance(halfvec, halfvec),
    	FUNCTION 3 l2_distance(halfvec, halfvec),
    	FUNCTION 5 ivfflat_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_l2_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_l2_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_ip_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_ip_ops
    	FOR TYPE halfvec USING ivfflat AS
    	OPERATOR 1 <#> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_negative_inner_product(halfvec, halfvec),
    	FUNCTION 3 halfvec_spherical_distance(halfvec, halfvec),
    	FUNCTION 4 l2_norm(halfvec),
    	FUNCTION 5 ivfflat_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_ip_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_ip_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_cosine_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_cosine_ops
    	FOR TYPE halfvec USING ivfflat AS
    	OPERATOR 1 <=> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_negative_inner_product(halfvec, halfvec),
    	FUNCTION 2 l2_norm(halfvec),
    	FUNCTION 3 halfvec_spherical_distance(halfvec, halfvec),
    	FUNCTION 4 l2_norm(halfvec),
    	FUNCTION 5 ivfflat_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_cosine_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_cosine_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_l2_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_l2_ops
    	FOR TYPE halfvec USING hnsw AS
    	OPERATOR 1 <-> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_l2_squared_distance(halfvec, halfvec),
    	FUNCTION 3 hnsw_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_l2_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_l2_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_ip_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_ip_ops
    	FOR TYPE halfvec USING hnsw AS
    	OPERATOR 1 <#> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_negative_inner_product(halfvec, halfvec),
    	FUNCTION 3 hnsw_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_ip_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_ip_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_cosine_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_cosine_ops
    	FOR TYPE halfvec USING hnsw AS
    	OPERATOR 1 <=> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 halfvec_negative_inner_product(halfvec, halfvec),
    	FUNCTION 2 l2_norm(halfvec),
    	FUNCTION 3 hnsw_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_cosine_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_cosine_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'halfvec_l1_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS halfvec_l1_ops
    	FOR TYPE halfvec USING hnsw AS
    	OPERATOR 1 <+> (halfvec, halfvec) FOR ORDER BY float_ops,
    	FUNCTION 1 l1_distance(halfvec, halfvec),
    	FUNCTION 3 hnsw_halfvec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS halfvec_l1_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY halfvec_l1_ops USING hnsw;
    n := n + 1;
  END IF;
  IF to_regprocedure('hamming_distance(bit, bit)') IS NULL THEN
    CREATE FUNCTION hamming_distance(bit, bit) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION hamming_distance(bit, bit);
    n := n + 1;
  END IF;
  IF to_regprocedure('jaccard_distance(bit, bit)') IS NULL THEN
    CREATE FUNCTION jaccard_distance(bit, bit) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION jaccard_distance(bit, bit);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<~>(bit,bit)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <~> (
    	LEFTARG = bit, RIGHTARG = bit, PROCEDURE = hamming_distance,
    	COMMUTATOR = '<~>'
    );
    ALTER EXTENSION vector ADD OPERATOR <~> (bit, bit);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<%>(bit,bit)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <%> (
    	LEFTARG = bit, RIGHTARG = bit, PROCEDURE = jaccard_distance,
    	COMMUTATOR = '<%>'
    );
    ALTER EXTENSION vector ADD OPERATOR <%> (bit, bit);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'bit_hamming_ops' AND a.amname = 'ivfflat' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS bit_hamming_ops
    	FOR TYPE bit USING ivfflat AS
    	OPERATOR 1 <~> (bit, bit) FOR ORDER BY float_ops,
    	FUNCTION 1 hamming_distance(bit, bit),
    	FUNCTION 3 hamming_distance(bit, bit),
    	FUNCTION 5 ivfflat_bit_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS bit_hamming_ops USING ivfflat;
    ALTER EXTENSION vector ADD OPERATOR FAMILY bit_hamming_ops USING ivfflat;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'bit_hamming_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS bit_hamming_ops
    	FOR TYPE bit USING hnsw AS
    	OPERATOR 1 <~> (bit, bit) FOR ORDER BY float_ops,
    	FUNCTION 1 hamming_distance(bit, bit),
    	FUNCTION 3 hnsw_bit_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS bit_hamming_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY bit_hamming_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'bit_jaccard_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS bit_jaccard_ops
    	FOR TYPE bit USING hnsw AS
    	OPERATOR 1 <%> (bit, bit) FOR ORDER BY float_ops,
    	FUNCTION 1 jaccard_distance(bit, bit),
    	FUNCTION 3 hnsw_bit_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS bit_jaccard_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY bit_jaccard_ops USING hnsw;
    n := n + 1;
  END IF;
  IF to_regtype('sparsevec') IS NULL THEN
    CREATE TYPE sparsevec;
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_in(cstring, oid, integer)') IS NULL THEN
    CREATE FUNCTION sparsevec_in(cstring, oid, integer) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_in(cstring, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_out(sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_out(sparsevec) RETURNS cstring
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_out(sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_typmod_in(cstring[])') IS NULL THEN
    CREATE FUNCTION sparsevec_typmod_in(cstring[]) RETURNS integer
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_typmod_in(cstring[]);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_recv(internal, oid, integer)') IS NULL THEN
    CREATE FUNCTION sparsevec_recv(internal, oid, integer) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_recv(internal, oid, integer);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_send(sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_send(sparsevec) RETURNS bytea
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_send(sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sparsevec' AND typnamespace = v_nsp_oid AND typisdefined) THEN
    CREATE TYPE sparsevec (
    	INPUT     = sparsevec_in,
    	OUTPUT    = sparsevec_out,
    	TYPMOD_IN = sparsevec_typmod_in,
    	RECEIVE   = sparsevec_recv,
    	SEND      = sparsevec_send,
    	STORAGE   = external
    );
    ALTER EXTENSION vector ADD TYPE sparsevec;
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_distance(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION l2_distance(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector', 'sparsevec_l2_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_distance(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('inner_product(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION inner_product(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector', 'sparsevec_inner_product' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION inner_product(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('cosine_distance(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION cosine_distance(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector', 'sparsevec_cosine_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION cosine_distance(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l1_distance(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION l1_distance(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector', 'sparsevec_l1_distance' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l1_distance(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_norm(sparsevec)') IS NULL THEN
    CREATE FUNCTION l2_norm(sparsevec) RETURNS float8
    	AS '$libdir/vector', 'sparsevec_l2_norm' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_norm(sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('l2_normalize(sparsevec)') IS NULL THEN
    CREATE FUNCTION l2_normalize(sparsevec) RETURNS sparsevec
    	AS '$libdir/vector', 'sparsevec_l2_normalize' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION l2_normalize(sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_lt(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_lt(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_lt(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_le(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_le(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_le(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_eq(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_eq(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_eq(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_ne(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_ne(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_ne(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_ge(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_ge(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_ge(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_gt(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_gt(sparsevec, sparsevec) RETURNS bool
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_gt(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_cmp(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_cmp(sparsevec, sparsevec) RETURNS int4
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_cmp(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_l2_squared_distance(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_l2_squared_distance(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_l2_squared_distance(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_negative_inner_product(sparsevec, sparsevec)') IS NULL THEN
    CREATE FUNCTION sparsevec_negative_inner_product(sparsevec, sparsevec) RETURNS float8
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_negative_inner_product(sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec(sparsevec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION sparsevec(sparsevec, integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec(sparsevec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('vector_to_sparsevec(vector, integer, boolean)') IS NULL THEN
    CREATE FUNCTION vector_to_sparsevec(vector, integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION vector_to_sparsevec(vector, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_to_vector(sparsevec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION sparsevec_to_vector(sparsevec, integer, boolean) RETURNS vector
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_to_vector(sparsevec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('halfvec_to_sparsevec(halfvec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION halfvec_to_sparsevec(halfvec, integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION halfvec_to_sparsevec(halfvec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('sparsevec_to_halfvec(sparsevec, integer, boolean)') IS NULL THEN
    CREATE FUNCTION sparsevec_to_halfvec(sparsevec, integer, boolean) RETURNS halfvec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION sparsevec_to_halfvec(sparsevec, integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_sparsevec(integer[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_sparsevec(integer[], integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_sparsevec(integer[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_sparsevec(real[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_sparsevec(real[], integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_sparsevec(real[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_sparsevec(double precision[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_sparsevec(double precision[], integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_sparsevec(double precision[], integer, boolean);
    n := n + 1;
  END IF;
  IF to_regprocedure('array_to_sparsevec(numeric[], integer, boolean)') IS NULL THEN
    CREATE FUNCTION array_to_sparsevec(numeric[], integer, boolean) RETURNS sparsevec
    	AS '$libdir/vector' LANGUAGE C IMMUTABLE STRICT PARALLEL SAFE;
    ALTER EXTENSION vector ADD FUNCTION array_to_sparsevec(numeric[], integer, boolean);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('sparsevec') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (sparsevec AS sparsevec)
    	WITH FUNCTION sparsevec(sparsevec, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (sparsevec AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('sparsevec') AND casttarget = to_regtype('vector')) THEN
    CREATE CAST (sparsevec AS vector)
    	WITH FUNCTION sparsevec_to_vector(sparsevec, integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (sparsevec AS vector);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('vector') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (vector AS sparsevec)
    	WITH FUNCTION vector_to_sparsevec(vector, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (vector AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('sparsevec') AND casttarget = to_regtype('halfvec')) THEN
    CREATE CAST (sparsevec AS halfvec)
    	WITH FUNCTION sparsevec_to_halfvec(sparsevec, integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (sparsevec AS halfvec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('halfvec') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (halfvec AS sparsevec)
    	WITH FUNCTION halfvec_to_sparsevec(halfvec, integer, boolean) AS IMPLICIT;
    ALTER EXTENSION vector ADD CAST (halfvec AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('integer[]') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (integer[] AS sparsevec)
    	WITH FUNCTION array_to_sparsevec(integer[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (integer[] AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('real[]') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (real[] AS sparsevec)
    	WITH FUNCTION array_to_sparsevec(real[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (real[] AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('double precision[]') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (double precision[] AS sparsevec)
    	WITH FUNCTION array_to_sparsevec(double precision[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (double precision[] AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_cast WHERE castsource = to_regtype('numeric[]') AND casttarget = to_regtype('sparsevec')) THEN
    CREATE CAST (numeric[] AS sparsevec)
    	WITH FUNCTION array_to_sparsevec(numeric[], integer, boolean) AS ASSIGNMENT;
    ALTER EXTENSION vector ADD CAST (numeric[] AS sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<->(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <-> (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = l2_distance,
    	COMMUTATOR = '<->'
    );
    ALTER EXTENSION vector ADD OPERATOR <-> (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<#>(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <#> (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_negative_inner_product,
    	COMMUTATOR = '<#>'
    );
    ALTER EXTENSION vector ADD OPERATOR <#> (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=>(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <=> (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = cosine_distance,
    	COMMUTATOR = '<=>'
    );
    ALTER EXTENSION vector ADD OPERATOR <=> (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<+>(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <+> (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = l1_distance,
    	COMMUTATOR = '<+>'
    );
    ALTER EXTENSION vector ADD OPERATOR <+> (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR < (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_lt,
    	COMMUTATOR = > , NEGATOR = >= ,
    	RESTRICT = scalarltsel, JOIN = scalarltjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR < (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<=(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <= (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_le,
    	COMMUTATOR = >= , NEGATOR = > ,
    	RESTRICT = scalarlesel, JOIN = scalarlejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <= (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('=(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR = (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_eq,
    	COMMUTATOR = = , NEGATOR = <> ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR = (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('<>(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR <> (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_ne,
    	COMMUTATOR = <> , NEGATOR = = ,
    	RESTRICT = eqsel, JOIN = eqjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR <> (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>=(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR >= (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_ge,
    	COMMUTATOR = <= , NEGATOR = < ,
    	RESTRICT = scalargesel, JOIN = scalargejoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR >= (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_operator WHERE oid = to_regoperator('>(sparsevec,sparsevec)') AND oprcode::oid <> 0) THEN
    CREATE OPERATOR > (
    	LEFTARG = sparsevec, RIGHTARG = sparsevec, PROCEDURE = sparsevec_gt,
    	COMMUTATOR = < , NEGATOR = <= ,
    	RESTRICT = scalargtsel, JOIN = scalargtjoinsel
    );
    ALTER EXTENSION vector ADD OPERATOR > (sparsevec, sparsevec);
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'sparsevec_ops' AND a.amname = 'btree' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS sparsevec_ops
    	DEFAULT FOR TYPE sparsevec USING btree AS
    	OPERATOR 1 < ,
    	OPERATOR 2 <= ,
    	OPERATOR 3 = ,
    	OPERATOR 4 >= ,
    	OPERATOR 5 > ,
    	FUNCTION 1 sparsevec_cmp(sparsevec, sparsevec);
    ALTER EXTENSION vector ADD OPERATOR CLASS sparsevec_ops USING btree;
    ALTER EXTENSION vector ADD OPERATOR FAMILY sparsevec_ops USING btree;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'sparsevec_l2_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS sparsevec_l2_ops
    	FOR TYPE sparsevec USING hnsw AS
    	OPERATOR 1 <-> (sparsevec, sparsevec) FOR ORDER BY float_ops,
    	FUNCTION 1 sparsevec_l2_squared_distance(sparsevec, sparsevec),
    	FUNCTION 3 hnsw_sparsevec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS sparsevec_l2_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY sparsevec_l2_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'sparsevec_ip_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS sparsevec_ip_ops
    	FOR TYPE sparsevec USING hnsw AS
    	OPERATOR 1 <#> (sparsevec, sparsevec) FOR ORDER BY float_ops,
    	FUNCTION 1 sparsevec_negative_inner_product(sparsevec, sparsevec),
    	FUNCTION 3 hnsw_sparsevec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS sparsevec_ip_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY sparsevec_ip_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'sparsevec_cosine_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS sparsevec_cosine_ops
    	FOR TYPE sparsevec USING hnsw AS
    	OPERATOR 1 <=> (sparsevec, sparsevec) FOR ORDER BY float_ops,
    	FUNCTION 1 sparsevec_negative_inner_product(sparsevec, sparsevec),
    	FUNCTION 2 l2_norm(sparsevec),
    	FUNCTION 3 hnsw_sparsevec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS sparsevec_cosine_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY sparsevec_cosine_ops USING hnsw;
    n := n + 1;
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_opclass c JOIN pg_am a ON a.oid = c.opcmethod WHERE c.opcname = 'sparsevec_l1_ops' AND a.amname = 'hnsw' AND c.opcnamespace = v_nsp_oid) THEN
    CREATE OPERATOR CLASS sparsevec_l1_ops
    	FOR TYPE sparsevec USING hnsw AS
    	OPERATOR 1 <+> (sparsevec, sparsevec) FOR ORDER BY float_ops,
    	FUNCTION 1 l1_distance(sparsevec, sparsevec),
    	FUNCTION 3 hnsw_sparsevec_support(internal);
    ALTER EXTENSION vector ADD OPERATOR CLASS sparsevec_l1_ops USING hnsw;
    ALTER EXTENSION vector ADD OPERATOR FAMILY sparsevec_l1_ops USING hnsw;
    n := n + 1;
  END IF;
  PERFORM set_config('search_path', v_old_path, true);
  PERFORM set_config('client_min_messages', v_old_cmm, true);
  RAISE NOTICE 'vector: created % missing objects; now run ALTER EXTENSION vector UPDATE', n;
END
$repair$;

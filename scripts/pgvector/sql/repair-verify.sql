\pset format unaligned
\pset tuples_only on
WITH ext AS (SELECT oid FROM pg_extension WHERE extname = 'vector'),
m AS (SELECT d.classid, d.objid, d.objsubid FROM pg_depend d, ext WHERE d.refclassid = 'pg_extension'::regclass AND d.refobjid = ext.oid AND d.deptype = 'e')
SELECT 'member|' || pg_describe_object(classid, objid, objsubid) || '|' || coalesce(obj_description(objid, (SELECT relname FROM pg_class WHERE oid = classid)), '') FROM m
UNION ALL
SELECT 'funcdef|' || md5(pg_get_functiondef(objid)) || '|' || pg_describe_object(classid, objid, 0) FROM m JOIN pg_proc pr ON pr.oid = m.objid WHERE classid = 'pg_proc'::regclass AND pr.prokind = 'f'
UNION ALL
SELECT 'amop|' || opf.opfname || '|' || am.amname || '|' || ao.amopstrategy::text || '|' || ao.amoppurpose::text || '|' || ao.amopopr::regoperator::text || '|' || ao.amopsortfamily::text FROM pg_amop ao JOIN pg_opfamily opf ON opf.oid = ao.amopfamily JOIN pg_am am ON am.oid = opf.opfmethod WHERE opf.opfname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'amproc|' || opf.opfname || '|' || am.amname || '|' || ap.amprocnum::text || '|' || ap.amproc::regprocedure::text FROM pg_amproc ap JOIN pg_opfamily opf ON opf.oid = ap.amprocfamily JOIN pg_am am ON am.oid = opf.opfmethod WHERE opf.opfname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'opclass|' || oc.opcname || '|' || am.amname || '|' || oc.opcdefault::text || '|' || oc.opcintype::regtype::text || '|' || (SELECT opfname FROM pg_opfamily f WHERE f.oid = oc.opcfamily) FROM pg_opclass oc JOIN pg_am am ON am.oid = oc.opcmethod WHERE oc.opcname ~ '^(vector|halfvec|sparsevec|bit)_'
UNION ALL
SELECT 'ext|' || extversion || '|' || extrelocatable::text FROM pg_extension WHERE extname = 'vector'
ORDER BY 1;

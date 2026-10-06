use datum::Datum;
use mcx::{Mcx, PgVec};
use types_core::{Oid, FLOAT4OID, FLOAT8OID, INT4OID, NUMERICOID};
use types_error::{
    PgError, PgResult, ERRCODE_DATA_EXCEPTION, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_NULL_VALUE_NOT_ALLOWED,
};
use types_fmgr::{cstring_result, FmgrInfo, FunctionCallInfoBaseData as Fcinfo};
use types_tuple::varatt;

use crate::vec::{
    check_dim, check_dims, check_element, check_expected_dim, inner_product,
    cosine_similarity, l1_distance, l2_squared_distance, parse_vector, vector_cmp_internal,
    vector_norm, VecBuilder, VecView, VECTOR_MAX_DIM,
};

pub(crate) fn image_datum(img: PgVec<'_, u8>) -> Datum {
    let d = Datum::from_usize(img.as_ptr() as usize);
    core::mem::forget(img);
    d
}

// SAFETY contract of callers: arg i is a non-null vector varlena (strict fns).
pub(crate) unsafe fn arg_vector<'a>(fcinfo: &'a Fcinfo, i: usize) -> PgResult<VecView<'a>> {
    let v = unsafe { fcinfo.arg_varlena_packed(i)? };
    VecView::from_payload(v.data())
}

pub(crate) unsafe fn detoasted_image<'m>(mcx: Mcx<'m>, d: Datum) -> PgResult<&'m [u8]> {
    let p = d.as_usize() as *const u8;
    unsafe {
        if varatt::varatt_is_4b_u(p) {
            Ok(core::slice::from_raw_parts(p, varatt::varsize_4b(p)))
        } else if varatt::varatt_is_1b(p) && !varatt::varatt_is_1b_e(p) {
            let src = core::slice::from_raw_parts(
                p.add(varatt::VARHDRSZ_SHORT),
                varatt::varsize_1b(p) - varatt::VARHDRSZ_SHORT,
            );
            let total = 4 + src.len();
            let mut buf: PgVec<'m, u8> = mcx::vec_with_capacity_in(mcx, total)?;
            mcx::vec_append_bytes(
                &mut buf,
                &varatt::set_varsize_4b_word(total as u32).to_ne_bytes(),
            )?;
            mcx::vec_append_bytes(&mut buf, src)?;
            let out = core::slice::from_raw_parts(buf.as_ptr(), buf.len());
            core::mem::forget(buf);
            Ok(out)
        } else {
            let raw = core::slice::from_raw_parts(p, varatt::varsize_any(p));
            let flat = detoast::detoast_attr(mcx, raw)?;
            let out = core::slice::from_raw_parts(flat.as_ptr(), flat.len());
            core::mem::forget(flat);
            Ok(out)
        }
    }
}

pub fn fc_vector_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict input fn — arg0 cstring, arg2 typmod.
    let lit = unsafe { fcinfo.arg_cstring(0) }.to_bytes();
    let typmod = fcinfo.arg_i32(2);
    let mut x = [0.0f32; VECTOR_MAX_DIM];
    let dim = parse_vector(lit, typmod, &mut x)?;
    let mut b = VecBuilder::new(fcinfo.result_mcx(), dim)?;
    for (i, v) in x[..dim].iter().enumerate() {
        b.set(i, *v);
    }
    Ok(image_datum(b.image()))
}

pub fn fc_vector_out(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let dim = v.dim();
    let mcx = fcinfo.result_mcx();
    let mut out: PgVec<'_, u8> =
        mcx::vec_with_capacity_in(mcx, ryu::FLOAT_SHORTEST_DECIMAL_LEN * dim + 3)?;
    let mut scratch = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
    mcx::vec_append_bytes(&mut out, b"[")?;
    for i in 0..dim {
        if i > 0 {
            mcx::vec_append_bytes(&mut out, b",")?;
        }
        let n = ryu::float_to_shortest_decimal_bufn(v.x(i), &mut scratch);
        mcx::vec_append_bytes(&mut out, &scratch[..n])?;
    }
    mcx::vec_append_bytes(&mut out, b"]\0")?;
    Ok(cstring_result(out))
}

pub fn fc_vector_typmod_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 cstring[].
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let tl = arrayfuncs::array_get_integer_typmods(mcx, arr)?;
    if tl.len() != 1 {
        return Err(PgError::error("invalid type modifier")
            .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
            .into());
    }
    if tl[0] < 1 {
        return Err(
            PgError::error("dimensions for type vector must be at least 1")
                .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
                .into(),
        );
    }
    if tl[0] as usize > VECTOR_MAX_DIM {
        return Err(PgError::error(format!(
            "dimensions for type vector cannot exceed {VECTOR_MAX_DIM}"
        ))
        .with_sqlstate(ERRCODE_INVALID_PARAMETER_VALUE)
        .into());
    }
    Ok(Datum::from_i32(tl[0]))
}

pub fn fc_vector_recv(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: recv arg0 is the live StringInfo per the recv ABI.
    let buf = unsafe { fcinfo.arg_stringinfo(0) };
    let typmod = fcinfo.arg_i32(2);
    let dim = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    let unused = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    if dim < 1 {
        check_dim(0)?;
    }
    check_dim(dim as usize)?;
    check_expected_dim(typmod, dim as usize)?;
    if unused != 0 {
        return Err(
            PgError::error(format!("expected unused to be 0, not {unused}"))
                .with_sqlstate(ERRCODE_DATA_EXCEPTION)
                .into(),
        );
    }
    let mut b = VecBuilder::new(fcinfo.result_mcx(), dim as usize)?;
    for i in 0..dim as usize {
        let x = pqformat::pq_getmsgfloat4(buf)?;
        check_element(x)?;
        b.set(i, x);
    }
    Ok(image_datum(b.image()))
}

pub fn fc_vector_send(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let mut buf = pqformat::pq_begintypsend(fcinfo.result_mcx())?;
    pqformat::pq_sendint(&mut buf, v.dim() as u32, 2)?;
    pqformat::pq_sendint(&mut buf, 0, 2)?;
    for i in 0..v.dim() {
        pqformat::pq_sendfloat4(&mut buf, v.x(i))?;
    }
    Ok(types_fmgr::varlena_result(pqformat::pq_endtypsend(buf)))
}

pub fn fc_vector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, arg1 typmod.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_expected_dim(typmod, v.dim())?;
    Ok(fcinfo.arg(0))
}

pub fn fc_array_to_vector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 array, arg1 typmod.
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let typmod = fcinfo.arg_i32(1);

    if arrayfuncs::arr_ndim(arr) > 1 {
        return Err(PgError::error("array must be 1-D")
            .with_sqlstate(ERRCODE_DATA_EXCEPTION)
            .into());
    }
    if arrayfuncs::arr_hasnull(arr) && arrayfuncs::array_contains_nulls(arr) {
        return Err(PgError::error("array must not contain nulls")
            .with_sqlstate(ERRCODE_NULL_VALUE_NOT_ALLOWED)
            .into());
    }

    let elemtype: Oid = arrayfuncs::arr_elemtype(arr);
    // numeric is not in builtin_meta: varlena, int-aligned.
    let (elmlen, elmbyval, elmalign) = if elemtype == NUMERICOID {
        (-1, false, b'i')
    } else {
        arrayfuncs::construct::builtin_meta(elemtype)?
    };
    let (elems, _nulls) = arrayfuncs::deconstruct_array(mcx, arr, elmlen, elmbyval, elmalign, true)?;
    let n = elems.len();
    check_dim(n)?;
    check_expected_dim(typmod, n)?;

    let mut b = VecBuilder::new(mcx, n)?;
    match elemtype {
        INT4OID => {
            for (i, d) in elems.iter().enumerate() {
                b.set(i, d.as_i32() as f32);
            }
        }
        FLOAT8OID => {
            for (i, d) in elems.iter().enumerate() {
                b.set(i, d.as_f64() as f32);
            }
        }
        FLOAT4OID => {
            for (i, d) in elems.iter().enumerate() {
                b.set(i, d.as_f32());
            }
        }
        NUMERICOID => {
            for (i, d) in elems.iter().enumerate() {
                let p = d.as_usize() as *const u8;
                // SAFETY: non-null numeric element datum inside the array image.
                let payload = unsafe {
                    let total = varatt::varsize_any(p);
                    let hdr = if varatt::varatt_is_1b(p) { 1 } else { 4 };
                    core::slice::from_raw_parts(p.add(hdr), total - hdr)
                };
                b.set(i, adt_numeric::ops::numeric_float4(
                    adt_numeric::Num::from_payload(payload),
                )?);
            }
        }
        _ => {
            return Err(PgError::error("unsupported array type")
                .with_sqlstate(ERRCODE_DATA_EXCEPTION)
                .into())
        }
    }

    for i in 0..n {
        check_element(b.get(i))?;
    }
    Ok(image_datum(b.image()))
}

/// Where an array cast's two up-front ereports sit in its C file.
pub(crate) struct ArraySite {
    pub file: &'static str,
    pub func: &'static str,
    pub ndim_line: i32,
    pub nulls_line: i32,
}

// The array casts' shared prologue (halfvec.c:453-464, sparsevec.c:707-718):
// reject a multi-dimensional array or one holding NULLs, then deconstruct it.
// fc_array_to_vector keeps its own copy (pre-M2 code, unchanged).
pub(crate) fn cast_array_elems<'m>(
    mcx: Mcx<'m>,
    arr: &[u8],
    site: &ArraySite,
) -> PgResult<(Oid, PgVec<'m, Datum>)> {
    if arrayfuncs::arr_ndim(arr) > 1 {
        return Err(PgError::error("array must be 1-D")
            .with_sqlstate(ERRCODE_DATA_EXCEPTION)
            .with_location(site.file, site.ndim_line, site.func)
            .into());
    }
    if arrayfuncs::arr_hasnull(arr) && arrayfuncs::array_contains_nulls(arr) {
        return Err(PgError::error("array must not contain nulls")
            .with_sqlstate(ERRCODE_NULL_VALUE_NOT_ALLOWED)
            .with_location(site.file, site.nulls_line, site.func)
            .into());
    }
    let elemtype: Oid = arrayfuncs::arr_elemtype(arr);
    // numeric is not in builtin_meta: varlena, int-aligned.
    let (elmlen, elmbyval, elmalign) = if elemtype == NUMERICOID {
        (-1, false, b'i')
    } else {
        arrayfuncs::construct::builtin_meta(elemtype)?
    };
    let (elems, _nulls) = arrayfuncs::deconstruct_array(mcx, arr, elmlen, elmbyval, elmalign, true)?;
    Ok((elemtype, elems))
}
// int4 and float8 to float implicitly (halfvec.c:474, 479; sparsevec.c:763)
// and numeric through numeric_float4. None: unsupported element type.
pub(crate) fn cast_elem_f32(elemtype: Oid, d: Datum) -> PgResult<Option<f32>> {
    Ok(Some(match elemtype {
        INT4OID => d.as_i32() as f32,
        FLOAT8OID => d.as_f64() as f32,
        FLOAT4OID => d.as_f32(),
        NUMERICOID => {
            let p = d.as_usize() as *const u8;
            // SAFETY: non-null numeric element datum inside the array image.
            let payload = unsafe {
                let total = varatt::varsize_any(p);
                let hdr = if varatt::varatt_is_1b(p) { 1 } else { 4 };
                core::slice::from_raw_parts(p.add(hdr), total - hdr)
            };
            adt_numeric::ops::numeric_float4(adt_numeric::Num::from_payload(payload))?
        }
        _ => return Ok(None),
    }))
}

pub fn fc_vector_to_float4(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut datums: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, v.dim())?;
    for i in 0..v.dim() {
        datums.push(Datum::from_f32(v.x(i)));
    }
    let img = arrayfuncs::construct_array(mcx, &datums, FLOAT4OID, 4, true, b'i')?;
    Ok(image_datum(img))
}

// halfvec_to_vector (vector.c:538-557).
pub fn fc_halfvec_to_vector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { crate::halfvec::arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_dim(v.dim())?;
    check_expected_dim(typmod, v.dim())?;
    let mut b = VecBuilder::new(fcinfo.result_mcx(), v.dim())?;
    for i in 0..v.dim() {
        b.set(i, crate::halfutils::half_to_float4(v.x(i)));
    }
    Ok(image_datum(b.image()))
}

fn binary_2arg<'a>(fcinfo: &'a Fcinfo) -> PgResult<(VecView<'a>, VecView<'a>)> {
    // SAFETY: strict fns — args 0 and 1 are vectors.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    let b = unsafe { arg_vector(fcinfo, 1)? };
    Ok((a, b))
}

pub fn fc_l2_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64((l2_squared_distance(&a, &b) as f64).sqrt()))
}

pub fn fc_vector_l2_squared_distance(
    _f: Option<&mut FmgrInfo>,
    fcinfo: &mut Fcinfo,
) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(l2_squared_distance(&a, &b) as f64))
}

pub fn fc_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(inner_product(&a, &b) as f64))
}

pub fn fc_vector_negative_inner_product(
    _f: Option<&mut FmgrInfo>,
    fcinfo: &mut Fcinfo,
) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(-(inner_product(&a, &b) as f64)))
}

pub fn fc_cosine_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut similarity = cosine_similarity(&a, &b);
    if similarity > 1.0 {
        similarity = 1.0;
    } else if similarity < -1.0 {
        similarity = -1.0;
    }
    Ok(Datum::from_f64(1.0 - similarity))
}

pub fn fc_vector_spherical_distance(
    _f: Option<&mut FmgrInfo>,
    fcinfo: &mut Fcinfo,
) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut distance = inner_product(&a, &b) as f64;
    if distance > 1.0 {
        distance = 1.0;
    } else if distance < -1.0 {
        distance = -1.0;
    }
    Ok(Datum::from_f64(distance.acos() / core::f64::consts::PI))
}

pub fn fc_l1_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(l1_distance(&a, &b) as f64))
}

pub fn fc_vector_dims(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    Ok(Datum::from_i32(a.dim() as i32))
}

pub fn fc_vector_norm(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    Ok(Datum::from_f64(vector_norm(&a)))
}

pub fn fc_l2_normalize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    let mut b = VecBuilder::new(fcinfo.result_mcx(), a.dim())?;
    let mut norm = 0.0f64;
    for x in a.iter() {
        norm += x as f64 * x as f64;
    }
    norm = norm.sqrt();
    if norm > 0.0 {
        for i in 0..a.dim() {
            b.set(i, (a.x(i) as f64 / norm) as f32);
        }
        for i in 0..a.dim() {
            if b.get(i).is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
        }
    }
    Ok(image_datum(b.image()))
}

fn elementwise(
    fcinfo: &mut Fcinfo,
    op: impl Fn(f32, f32) -> f32,
    check_underflow: bool,
) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut r = VecBuilder::new(fcinfo.result_mcx(), a.dim())?;
    for i in 0..a.dim() {
        r.set(i, op(a.x(i), b.x(i)));
    }
    for i in 0..a.dim() {
        let v = r.get(i);
        if v.is_infinite() {
            return Err(Box::new(adt_float::float_overflow_error()));
        }
        if check_underflow && v == 0.0 && !(a.x(i) == 0.0 || b.x(i) == 0.0) {
            return Err(Box::new(adt_float::float_underflow_error()));
        }
    }
    Ok(image_datum(r.image()))
}

pub fn fc_vector_add(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x + y, false)
}

pub fn fc_vector_sub(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x - y, false)
}

pub fn fc_vector_mul(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    elementwise(fcinfo, |x, y| x * y, true)
}

pub fn fc_vector_concat(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    let dim = a.dim() + b.dim();
    check_dim(dim)?;
    let mut r = VecBuilder::new(fcinfo.result_mcx(), dim)?;
    for i in 0..a.dim() {
        r.set(i, a.x(i));
    }
    for i in 0..b.dim() {
        r.set(a.dim() + i, b.x(i));
    }
    Ok(image_datum(r.image()))
}

// VarBit image: 4B varlena header, i32 bit length, zero-padded data bytes.
pub fn fc_binary_quantize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    let dim = a.dim();
    let nbytes = dim.div_ceil(8);
    let total = 4 + 4 + nbytes;
    let mut img: PgVec<'_, u8> = mcx::vec_with_capacity_in(fcinfo.result_mcx(), total)?;
    img.resize(total, 0);
    img[..4].copy_from_slice(&((total as u32) << 2).to_ne_bytes());
    img[4..8].copy_from_slice(&(dim as i32).to_ne_bytes());
    for i in 0..dim {
        if a.x(i) > 0.0 {
            img[8 + i / 8] |= 1 << (7 - (i % 8));
        }
    }
    Ok(image_datum(img))
}

pub fn fc_subvector(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, args 1-2 int4.
    let a = unsafe { arg_vector(fcinfo, 0)? };
    let mut start = fcinfo.arg_i32(1);
    let count = fcinfo.arg_i32(2);

    if count < 1 {
        check_dim(0)?;
    }
    let adim = a.dim() as i32;
    // start + count without i32 overflow (both checked positive / bounded).
    let end = if start > adim - count { adim + 1 } else { start + count };
    if start < 1 {
        start = 1;
    } else if start > adim {
        check_dim(0)?;
    }
    // C's CheckDim takes a signed dim: a negative (end - start) must raise
    // the 22000 "at least 1 dimension" error, not wrap through usize into
    // the 54000 max-dimension branch.
    let dim = end - start;
    if dim < 1 {
        check_dim(0)?;
    }
    let dim = dim as usize;
    check_dim(dim)?;
    let mut r = VecBuilder::new(fcinfo.result_mcx(), dim)?;
    for i in 0..dim {
        r.set(i, a.x((start - 1) as usize + i));
    }
    Ok(image_datum(r.image()))
}

pub fn fc_vector_lt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) < 0))
}

pub fn fc_vector_le(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) <= 0))
}

pub fn fc_vector_eq(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) == 0))
}

pub fn fc_vector_ne(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) != 0))
}

pub fn fc_vector_ge(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) >= 0))
}

pub fn fc_vector_gt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_bool(vector_cmp_internal(&a, &b) > 0))
}

pub fn fc_vector_cmp(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = binary_2arg(fcinfo)?;
    Ok(Datum::from_i32(vector_cmp_internal(&a, &b)))
}

struct StateArray<'a> {
    img: &'a [u8],
    n_items: usize,
}

impl<'a> StateArray<'a> {
    fn check(mcx: Mcx<'a>, d: Datum, caller: &str) -> PgResult<StateArray<'a>> {
        // SAFETY: caller passes a non-null float8[] state datum.
        let img = unsafe { detoasted_image(mcx, d)? };
        if arrayfuncs::arr_ndim(img) != 1
            || arrayfuncs::arr_dim(img, 0) < 1
            || arrayfuncs::arr_hasnull(img)
            || arrayfuncs::arr_elemtype(img) != FLOAT8OID
        {
            return Err(PgError::error(format!("{caller}: expected state array")).into());
        }
        Ok(StateArray {
            img,
            n_items: arrayfuncs::arr_dim(img, 0) as usize,
        })
    }

    // STATE_DIMS: dims[0] - 1.
    fn state_dims(&self) -> usize {
        self.n_items - 1
    }

    fn value(&self, i: usize) -> f64 {
        let off = arrayfuncs::arr_data_offset(self.img) + 8 * i;
        f64::from_ne_bytes(self.img[off..off + 8].try_into().unwrap())
    }
}

fn build_state_array<'m>(mcx: Mcx<'m>, vals: &[Datum]) -> PgResult<PgVec<'m, u8>> {
    arrayfuncs::construct_array(mcx, vals, FLOAT8OID, 8, true, b'd')
}

pub fn fc_vector_accum(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let state = StateArray::check(mcx, fcinfo.arg(0), "vector_accum")?;
    // SAFETY: strict fn — arg1 vector.
    let newval = unsafe { arg_vector(fcinfo, 1)? };

    let mut dim = state.state_dims();
    let newarr = dim == 0;
    if newarr {
        dim = newval.dim();
    } else {
        check_expected_dim(dim as i32, newval.dim())?;
    }

    let n = state.value(0) + 1.0;
    let mut datums: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
    datums.push(Datum::from_f64(n));
    if newarr {
        for i in 0..dim {
            datums.push(Datum::from_f64(newval.x(i) as f64));
        }
    } else {
        for i in 0..dim {
            let v = state.value(i + 1) + newval.x(i) as f64;
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            datums.push(Datum::from_f64(v));
        }
    }
    Ok(image_datum(build_state_array(mcx, &datums)?))
}

// CreateStateDatums(dim) (vector.c) followed by a copy of one partial state's
// sums; statedatums[0] is filled in by the caller.
fn copy_state_datums<'m>(mcx: Mcx<'m>, s: &StateArray<'_>, dim: usize) -> PgResult<PgVec<'m, Datum>> {
    let mut d: PgVec<'m, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
    d.push(Datum::null());
    for i in 1..=dim {
        d.push(Datum::from_f64(s.value(i)));
    }
    Ok(d)
}

// vector_combine (vector.c:1211-1284, pgvector 0.8.7; also halfvec_combine's
// symbol). Branches on the partial states' dimensions, CheckDims every
// non-empty side, and sets statedatums[0] = n1 + n2 (vector.c:1275).
pub fn fc_vector_combine(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let s1 = StateArray::check(mcx, fcinfo.arg(0), "vector_combine")?;
    let s2 = StateArray::check(mcx, fcinfo.arg(1), "vector_combine")?;

    let n1 = s1.value(0);
    let n2 = s2.value(0);
    let dim1 = s1.state_dims();
    let dim2 = s2.state_dims();

    let mut datums: PgVec<'_, Datum> = if dim1 == 0 && dim2 == 0 {
        copy_state_datums(mcx, &s1, 0)?
    } else if dim1 == 0 {
        check_dim(dim2)?;
        copy_state_datums(mcx, &s2, dim2)?
    } else if dim2 == 0 {
        check_dim(dim1)?;
        copy_state_datums(mcx, &s1, dim1)?
    } else {
        let dim = dim1;
        check_dim(dim)?;
        check_expected_dim(dim as i32, dim2)?;
        let mut d: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
        d.push(Datum::null());
        for i in 1..=dim {
            let v = s1.value(i) + s2.value(i);
            // Check for overflow
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            d.push(Datum::from_f64(v));
        }
        d
    };
    datums[0] = Datum::from_f64(n1 + n2);
    Ok(image_datum(build_state_array(mcx, &datums)?))
}

pub fn fc_vector_avg(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let state = StateArray::check(mcx, fcinfo.arg(0), "vector_avg")?;
    let n = state.value(0);
    if n == 0.0 {
        fcinfo.isnull = true;
        return Ok(Datum::null());
    }
    let dim = state.state_dims();
    check_dim(dim)?;
    let mut b = VecBuilder::new(mcx, dim)?;
    for i in 0..dim {
        let v = (state.value(i + 1) / n) as f32;
        check_element(v)?;
        b.set(i, v);
    }
    Ok(image_datum(b.image()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Upstream array_to_vector deconstructs through get_typlenbyvalalign +
    // deconstruct_array, so real[] converts (pgvector test/expected/cast.out);
    // deconstruct_array_builtin's table has no FLOAT4OID row.
    #[test]
    fn real_array_casts_to_vector() {
        let ctx = mcx::MemoryContext::new("pgvector-test");
        let mcx = ctx.mcx();
        let elems = [Datum::from_f32(1.0), Datum::from_f32(2.5), Datum::from_f32(-3.0)];
        let arr = arrayfuncs::construct_array(mcx, &elems, FLOAT4OID, 4, true, b'i').unwrap();
        let mut fcinfo = types_fmgr::LocalFcinfo::<2>::new(0);
        // SAFETY: ctx outlives the call.
        unsafe { fcinfo.set_result_mcx(mcx) };
        fcinfo.set_arg(0, Datum::from_usize(arr.as_ptr() as usize));
        fcinfo.set_arg(1, Datum::from_i32(-1));
        let out = fc_array_to_vector(None, &mut fcinfo).unwrap();
        let mut probe = types_fmgr::LocalFcinfo::<1>::new(0);
        probe.set_arg(0, out);
        // SAFETY: out is a live vector varlena in ctx.
        let v = unsafe { arg_vector(&probe, 0) }.unwrap();
        assert_eq!(v.dim(), 3);
        assert_eq!((v.x(0), v.x(1), v.x(2)), (1.0, 2.5, -3.0));
    }

    fn combine(m: Mcx<'_>, a: &[f64], b: &[f64]) -> Result<Vec<f64>, String> {
        let mk = |v: &[f64]| {
            let e: Vec<Datum> = v.iter().map(|x| Datum::from_f64(*x)).collect();
            arrayfuncs::construct_array(m, &e, FLOAT8OID, 8, true, b'd').unwrap()
        };
        let (a, b) = (mk(a), mk(b));
        let mut fc = types_fmgr::LocalFcinfo::<2>::new(0);
        // SAFETY: the context outlives the call.
        unsafe { fc.set_result_mcx(m) };
        fc.set_arg(0, Datum::from_usize(a.as_ptr() as usize));
        fc.set_arg(1, Datum::from_usize(b.as_ptr() as usize));
        match fc_vector_combine(None, &mut fc) {
            Ok(d) => {
                let s = StateArray::check(m, d, "test").unwrap();
                Ok((0..s.n_items).map(|i| s.value(i)).collect())
            }
            Err(e) => Err(e.message().to_string()),
        }
    }

    // vector_combine (vector.c:1211-1284, 0.8.7) branches on STATE_DIMS, not
    // on the counts, and CheckDims every non-empty side.
    #[test]
    fn vector_combine_matches_0_8_7() {
        let ctx = mcx::MemoryContext::new("pgvector-test");
        let m = ctx.mcx();
        assert_eq!(combine(m, &[0.0], &[0.0]).unwrap(), vec![0.0]);
        assert_eq!(combine(m, &[1.0, 2.0], &[3.0, 4.0]).unwrap(), vec![4.0, 6.0]);
        // n1 == 0 but dims > 0 (0.8.5 copied s2: {3,4,5}).
        assert_eq!(combine(m, &[0.0, 1.0, 2.0], &[3.0, 4.0, 5.0]).unwrap(), vec![3.0, 5.0, 7.0]);
        // dim1 == 0 but n1 != 0 (0.8.5: "expected 0 dimensions, not 1").
        assert_eq!(combine(m, &[5.0], &[3.0, 4.0]).unwrap(), vec![8.0, 4.0]);
        assert_eq!(combine(m, &[1.0, 2.0], &[3.0, 4.0, 5.0]).unwrap_err(), "expected 1 dimensions, not 2");
        // regress vector_type (expected lines 723-727): 16,001 dimensions.
        let big: Vec<f64> = (1..=16002).map(|n| n as f64).collect();
        for (a, b) in [(&[0.0][..], &big[..]), (&big[..], &[0.0][..]), (&big[..], &big[..])] {
            assert_eq!(combine(m, a, b).unwrap_err(), "vector cannot have more than 16000 dimensions");
        }
    }
}

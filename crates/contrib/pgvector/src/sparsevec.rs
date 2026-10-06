//! sparsevec.c (pgvector 0.8.7): the sparsevec type, a varlena of
//! `i32 dim, i32 nnz, i32 unused, i32 indices[nnz], f32 values[nnz]`
//! (sparsevec.h:21-48) with 0-based, strictly increasing indices, at most
//! 1e9 dimensions and 16,000 non-zero values; its I/O, casts and functions.
//! sparsevec_to_vector lives in funcs.rs (vector.c) and sparsevec_to_halfvec
//! in halfvec.rs (halfvec.c). Errors carry their sparsevec.c location.

use datum::Datum;
use mcx::{Mcx, PgVec};
use stringinfo::StringInfo;
use types_error::{
    PgError, PgResult, SqlState, ERRCODE_DATA_EXCEPTION, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_INVALID_TEXT_REPRESENTATION, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED,
};
use types_fmgr::{cstring_result, FmgrInfo, FunctionCallInfoBaseData as Fcinfo};
use types_error::ERRCODE_INTERNAL_ERROR;

use crate::funcs::{arg_vector, cast_array_elems, cast_elem_f32, ArraySite};
use crate::funcs::{detoasted_image, image_datum};
use crate::halfutils::{half_is_zero, half_to_float4};
use crate::vec::{strtof_prefix, vector_isspace as sparsevec_isspace, StrtofVal};

pub const SPARSEVEC_MAX_DIM: i32 = 1_000_000_000;
pub const SPARSEVEC_MAX_NNZ: i32 = 16000;

// Payload layout after the 4-byte varlena header: i32 dim, i32 nnz, i32 unused.
pub const SPARSEVEC_PAYLOAD_HDR: usize = 12;

// An ereport/elog of sparsevec.c, located at the call's last line.
#[cold]
pub(crate) fn ereport(code: SqlState, msg: impl Into<String>, line: i32, func: &'static str) -> Box<PgError> {
    PgError::error(msg).with_sqlstate(code).with_location("sparsevec.c", line, func).into()
}

#[derive(Clone, Copy)]
pub struct SparseView<'a> {
    data: &'a [u8],
    nnz: usize,
}

impl<'a> SparseView<'a> {
    pub fn from_payload(data: &'a [u8]) -> PgResult<SparseView<'a>> {
        let corrupt = || -> Box<PgError> { PgError::error("corrupt sparsevec datum").into() };
        if data.len() < SPARSEVEC_PAYLOAD_HDR {
            return Err(corrupt());
        }
        let dim = i32::from_ne_bytes(data[0..4].try_into().unwrap());
        let nnz = i32::from_ne_bytes(data[4..8].try_into().unwrap());
        if !(1..=SPARSEVEC_MAX_DIM).contains(&dim) || !(0..=SPARSEVEC_MAX_NNZ).contains(&nnz) || nnz > dim {
            return Err(corrupt());
        }
        if data.len() < SPARSEVEC_PAYLOAD_HDR + 8 * nnz as usize {
            return Err(corrupt());
        }
        Ok(SparseView { data, nnz: nnz as usize })
    }

    #[inline]
    pub fn dim(&self) -> i32 {
        i32::from_ne_bytes(self.data[0..4].try_into().unwrap())
    }

    #[inline]
    pub fn nnz(&self) -> usize {
        self.nnz
    }

    #[inline]
    pub fn index(&self, i: usize) -> i32 {
        let off = SPARSEVEC_PAYLOAD_HDR + 4 * i;
        i32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }

    // SPARSEVEC_VALUES (sparsevec.h:44-48): the values follow the indices.
    #[inline]
    pub fn value(&self, i: usize) -> f32 {
        let off = SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        f32::from_ne_bytes(self.data[off..off + 4].try_into().unwrap())
    }
}

pub struct SparseBuilder<'mcx> {
    img: PgVec<'mcx, u8>,
    nnz: usize,
}

// InitSparseVector (sparsevec.c:153-166): full varlena image, zeroed body.
impl<'mcx> SparseBuilder<'mcx> {
    pub fn new(mcx: Mcx<'mcx>, dim: i32, nnz: usize) -> PgResult<SparseBuilder<'mcx>> {
        let size = 4 + SPARSEVEC_PAYLOAD_HDR + 8 * nnz;
        let mut img: PgVec<'mcx, u8> = mcx::vec_with_capacity_in(mcx, size)?;
        img.resize(size, 0);
        img[..4].copy_from_slice(&((size as u32) << 2).to_ne_bytes());
        img[4..8].copy_from_slice(&dim.to_ne_bytes());
        img[8..12].copy_from_slice(&(nnz as i32).to_ne_bytes());
        Ok(SparseBuilder { img, nnz })
    }

    #[inline]
    pub fn nnz(&self) -> usize {
        self.nnz
    }

    #[inline]
    pub fn set_index(&mut self, i: usize, index: i32) {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * i;
        self.img[off..off + 4].copy_from_slice(&index.to_ne_bytes());
    }

    #[inline]
    pub fn set_value(&mut self, i: usize, value: f32) {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        self.img[off..off + 4].copy_from_slice(&value.to_ne_bytes());
    }

    #[inline]
    pub fn index(&self, i: usize) -> i32 {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * i;
        i32::from_ne_bytes(self.img[off..off + 4].try_into().unwrap())
    }

    #[inline]
    pub fn value(&self, i: usize) -> f32 {
        let off = 4 + SPARSEVEC_PAYLOAD_HDR + 4 * self.nnz + 4 * i;
        f32::from_ne_bytes(self.img[off..off + 4].try_into().unwrap())
    }

    pub fn image(self) -> PgVec<'mcx, u8> {
        self.img
    }
}

// SAFETY contract of callers: arg i is a non-null sparsevec varlena (strict fns).
pub(crate) unsafe fn arg_sparsevec<'a>(fcinfo: &'a Fcinfo, i: usize) -> PgResult<SparseView<'a>> {
    let v = unsafe { fcinfo.arg_varlena_packed(i)? };
    SparseView::from_payload(v.data())
}

// CheckExpectedDim (sparsevec.c:56-63).
fn check_expected_dim(typmod: i32, dim: i32) -> PgResult<()> {
    if typmod != -1 && typmod != dim {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected {typmod} dimensions, not {dim}"),
            62,
            "CheckExpectedDim",
        ));
    }
    Ok(())
}

// CheckDim (sparsevec.c:68-80).
pub(crate) fn check_dim(dim: i32) -> PgResult<()> {
    if dim < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec must have at least 1 dimension", 74, "CheckDim"));
    }
    if dim > SPARSEVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_DIM} dimensions"),
            79,
            "CheckDim",
        ));
    }
    Ok(())
}

// CheckNnz (sparsevec.c:85-102).
fn check_nnz(nnz: i32, dim: i32) -> PgResult<()> {
    if nnz < 0 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec cannot have negative number of elements", 91, "CheckNnz"));
    }
    if nnz > SPARSEVEC_MAX_NNZ {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_NNZ} non-zero elements"),
            96,
            "CheckNnz",
        ));
    }
    if nnz > dim {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            "sparsevec cannot have more elements than dimensions",
            101,
            "CheckNnz",
        ));
    }
    Ok(())
}

// CheckIndex (sparsevec.c:107-131) on the i-th index written so far.
fn check_index(b: &SparseBuilder<'_>, i: usize, dim: i32) -> PgResult<()> {
    let index = b.index(i);
    if index < 0 || index >= dim {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec index out of bounds", 116, "CheckIndex"));
    }
    if i > 0 {
        if index < b.index(i - 1) {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec indices must be in ascending order", 124, "CheckIndex"));
        }
        if index == b.index(i - 1) {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "sparsevec indices must not contain duplicates", 129, "CheckIndex"));
        }
    }
    Ok(())
}

// CheckElement (sparsevec.c:136-148).
fn check_element(value: f32) -> PgResult<()> {
    if value.is_nan() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "NaN not allowed in sparsevec", 142, "CheckElement"));
    }
    if value.is_infinite() {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "infinite value not allowed in sparsevec", 147, "CheckElement"));
    }
    Ok(())
}

#[cold]
fn invalid_text(lit: &[u8], detail: Option<&str>, line: i32) -> Box<PgError> {
    let mut e = PgError::error(format!(
        "invalid input syntax for type sparsevec: \"{}\"",
        String::from_utf8_lossy(lit)
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("sparsevec.c", line, "sparsevec_in");
    if let Some(d) = detail {
        e = e.with_detail(d);
    }
    e.into()
}

// strtol(pt, &stringEnd, 10) with C's 64-bit long (sparsevec.c:275, 365):
// skips isspace, takes an optional sign and decimal digits, saturates at
// i64::MIN/MAX. None when no digit follows (stringEnd == pt).
fn strtol_prefix(s: &[u8]) -> Option<(i64, usize)> {
    let mut i = 0usize;
    while i < s.len() && pg_string::isspace_c_locale(s[i]) {
        i += 1;
    }
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let first_digit = i;
    // Accumulate negatively so i64::MIN is reachable without overflow.
    let mut acc: i64 = 0;
    let mut saturated = false;
    while i < s.len() && s[i].is_ascii_digit() {
        match acc.checked_mul(10).and_then(|v| v.checked_sub((s[i] - b'0') as i64)) {
            Some(v) => acc = v,
            None => saturated = true,
        }
        i += 1;
    }
    if i == first_digit {
        return None;
    }
    let value = match (saturated, neg) {
        (true, true) => i64::MIN,
        (true, false) => i64::MAX,
        (false, true) => acc,
        (false, false) => acc.checked_neg().unwrap_or(i64::MAX),
    };
    Some((value, i))
}

// strtof set errno = ERANGE and returned zero (sparsevec.c:315): a literal
// with a nonzero significand digit that rounded to zero.
fn underflowed_to_zero(token: &[u8], value: f32) -> bool {
    if value != 0.0 {
        return false;
    }
    let t: Vec<u8> = token
        .iter()
        .copied()
        .skip_while(|c| pg_string::isspace_c_locale(*c) || *c == b'+' || *c == b'-')
        .collect();
    let (digits, exponent_mark) = if t.len() >= 2 && t[0] == b'0' && (t[1] | 0x20) == b'x' {
        (&t[2..], b'p')
    } else {
        (&t[..], b'e')
    };
    digits
        .iter()
        .take_while(|c| (**c | 0x20) != exponent_mark)
        .any(|c| c.is_ascii_hexdigit() && *c != b'0')
}

// sparsevec_in (sparsevec.c:203-406): `{index:value,...}/dim` with 1-based
// indices; zero values are dropped; the elements are sorted by index
// (qsort, sparsevec.c:393) and then checked.
pub fn sparsevec_in_body<'m>(mcx: Mcx<'m>, lit: &[u8], typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let max_nnz = 1 + lit.iter().filter(|c| **c == b',').count();
    if max_nnz > SPARSEVEC_MAX_NNZ as usize {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("sparsevec cannot have more than {SPARSEVEC_MAX_NNZ} non-zero elements"),
            230,
            "sparsevec_in",
        ));
    }
    let mut elements: PgVec<'m, (i32, f32)> = mcx::vec_with_capacity_in(mcx, max_nnz)?;
    let n = lit.len();
    let mut pt = 0usize;

    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'{' {
        return Err(invalid_text(lit, Some("Vector contents must start with \"{\"."), 243));
    }
    pt += 1;
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt < n && lit[pt] == b'}' {
        pt += 1;
    } else {
        loop {
            // Safety check
            if elements.len() >= max_nnz {
                return Err(ereport(
                    ERRCODE_INVALID_TEXT_REPRESENTATION,
                    format!("ran out of buffer: \"{}\"", String::from_utf8_lossy(lit)),
                    263,
                    "sparsevec_in",
                ));
            }
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            // Check for empty string like float4in
            if pt >= n {
                return Err(invalid_text(lit, None, 272));
            }
            // Use similar logic as int2vectorin
            let Some((index, used)) = strtol_prefix(&lit[pt..]) else {
                return Err(invalid_text(lit, None, 280));
            };
            // Keep in int range for correct error message later
            let index = index.clamp(i32::MIN as i64 + 1, i32::MAX as i64) as i32;
            pt += used;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            if pt >= n || lit[pt] != b':' {
                return Err(invalid_text(lit, None, 296));
            }
            pt += 1;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            let Some((val, consumed)) = strtof_prefix(&lit[pt..]) else {
                return Err(invalid_text(lit, None, 312));
            };
            let token = &lit[pt..pt + consumed];
            // Check for range error like float4in
            let value = match val {
                StrtofVal::Ok(v) if !underflowed_to_zero(token, v) => v,
                _ => {
                    return Err(ereport(
                        ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
                        format!("\"{}\" is out of range for type sparsevec", String::from_utf8_lossy(token)),
                        318,
                        "sparsevec_in",
                    ))
                }
            };
            check_element(value)?;
            // Do not store zero values
            if value != 0.0 {
                // Convert 1-based numbering (SQL) to 0-based (C); index > i32::MIN.
                elements.push((index - 1, value));
            }
            pt += consumed;
            while pt < n && sparsevec_isspace(lit[pt]) {
                pt += 1;
            }
            if pt < n && lit[pt] == b',' {
                pt += 1;
            } else if pt < n && lit[pt] == b'}' {
                pt += 1;
                break;
            } else {
                return Err(invalid_text(lit, None, 346));
            }
        }
    }

    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'/' {
        return Err(invalid_text(lit, Some("Unexpected end of input."), 357));
    }
    pt += 1;
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    let Some((dim, used)) = strtol_prefix(&lit[pt..]) else {
        return Err(invalid_text(lit, None, 370));
    };
    // Keep in int range for correct error message later
    let dim = dim.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    pt += used;
    // Only whitespace is allowed after the closing brace
    while pt < n && sparsevec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt != n {
        return Err(invalid_text(lit, Some("Junk after closing."), 388));
    }

    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;

    elements.sort_unstable_by_key(|e| e.0);
    let mut b = SparseBuilder::new(mcx, dim, elements.len())?;
    for (i, (index, value)) in elements.iter().enumerate() {
        b.set_index(i, *index);
        b.set_value(i, *value);
        check_index(&b, i, dim)?;
    }
    Ok(b.image())
}

pub fn fc_sparsevec_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict input fn — arg0 cstring, arg2 typmod.
    let lit = unsafe { fcinfo.arg_cstring(0) }.to_bytes();
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(sparsevec_in_body(fcinfo.result_mcx(), lit, typmod)?))
}

// sparsevec_out (sparsevec.c:425-473): 1-based indices, shortest floats.
pub fn fc_sparsevec_out(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut out: PgVec<'_, u8> =
        mcx::vec_with_capacity_in(mcx, (12 + ryu::FLOAT_SHORTEST_DECIMAL_LEN) * v.nnz() + 15)?;
    let mut num = [0u8; 12];
    let mut scratch = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
    mcx::vec_append_bytes(&mut out, b"{")?;
    for i in 0..v.nnz() {
        if i > 0 {
            mcx::vec_append_bytes(&mut out, b",")?;
        }
        // Convert 0-based numbering (C) to 1-based (SQL)
        let k = numutils::pg_ltoa(v.index(i) + 1, &mut num);
        mcx::vec_append_bytes(&mut out, &num[..k])?;
        mcx::vec_append_bytes(&mut out, b":")?;
        let k = ryu::float_to_shortest_decimal_bufn(v.value(i), &mut scratch);
        mcx::vec_append_bytes(&mut out, &scratch[..k])?;
    }
    mcx::vec_append_bytes(&mut out, b"}/")?;
    let k = numutils::pg_ltoa(v.dim(), &mut num);
    mcx::vec_append_bytes(&mut out, &num[..k])?;
    mcx::vec_append_bytes(&mut out, b"\0")?;
    Ok(cstring_result(out))
}

// sparsevec_typmod_in (sparsevec.c:478-504).
pub fn fc_sparsevec_typmod_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 cstring[].
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let tl = arrayfuncs::array_get_integer_typmods(mcx, arr)?;
    if tl.len() != 1 {
        return Err(ereport(ERRCODE_INVALID_PARAMETER_VALUE, "invalid type modifier", 491, "sparsevec_typmod_in"));
    }
    if tl[0] < 1 {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            "dimensions for type sparsevec must be at least 1",
            496,
            "sparsevec_typmod_in",
        ));
    }
    if tl[0] > SPARSEVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            format!("dimensions for type sparsevec cannot exceed {SPARSEVEC_MAX_DIM}"),
            501,
            "sparsevec_typmod_in",
        ));
    }
    Ok(Datum::from_i32(tl[0]))
}

// sparsevec_recv's body (sparsevec.c:511-556): int32 dim, nnz and unused,
// nnz 0-based indices, then nnz float4 values; zero values are rejected.
pub(crate) fn sparsevec_recv_body<'m>(mcx: Mcx<'m>, buf: &mut StringInfo<'_>, typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let dim = pqformat::pq_getmsgint(buf, 4)? as i32;
    let nnz = pqformat::pq_getmsgint(buf, 4)? as i32;
    let unused = pqformat::pq_getmsgint(buf, 4)? as i32;
    check_dim(dim)?;
    check_nnz(nnz, dim)?;
    check_expected_dim(typmod, dim)?;
    if unused != 0 {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected unused to be 0, not {unused}"),
            532,
            "sparsevec_recv",
        ));
    }
    let mut b = SparseBuilder::new(mcx, dim, nnz as usize)?;
    // Binary representation uses zero-based numbering for indices
    for i in 0..nnz as usize {
        b.set_index(i, pqformat::pq_getmsgint(buf, 4)? as i32);
        check_index(&b, i, dim)?;
    }
    for i in 0..nnz as usize {
        let value = pqformat::pq_getmsgfloat4(buf)?;
        check_element(value)?;
        if value == 0.0 {
            return Err(ereport(
                ERRCODE_DATA_EXCEPTION,
                "binary representation of sparsevec cannot contain zero values",
                552,
                "sparsevec_recv",
            ));
        }
        b.set_value(i, value);
    }
    Ok(b.image())
}

pub fn fc_sparsevec_recv(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: recv arg0 is the live StringInfo per the recv ABI.
    let buf = unsafe { fcinfo.arg_stringinfo(0) };
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(sparsevec_recv_body(fcinfo.result_mcx(), buf, typmod)?))
}

// sparsevec_send (sparsevec.c:561-582).
pub fn fc_sparsevec_send(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mut buf = pqformat::pq_begintypsend(fcinfo.result_mcx())?;
    pqformat::pq_sendint32(&mut buf, v.dim() as u32)?;
    pqformat::pq_sendint32(&mut buf, v.nnz() as u32)?;
    pqformat::pq_sendint32(&mut buf, 0)?; // svec->unused, always zero
    // Binary representation uses zero-based numbering for indices
    for i in 0..v.nnz() {
        pqformat::pq_sendint32(&mut buf, v.index(i) as u32)?;
    }
    for i in 0..v.nnz() {
        pqformat::pq_sendfloat4(&mut buf, v.value(i))?;
    }
    Ok(types_fmgr::varlena_result(pqformat::pq_endtypsend(buf)))
}

// CheckDims (sparsevec.c:44-51).
fn check_dims(a: &SparseView<'_>, b: &SparseView<'_>) -> PgResult<()> {
    if a.dim() != b.dim() {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("different sparsevec dimensions {} and {}", a.dim(), b.dim()),
            50,
            "CheckDims",
        ));
    }
    Ok(())
}

fn sparsevec_2arg(fcinfo: &Fcinfo) -> PgResult<(SparseView<'_>, SparseView<'_>)> {
    // SAFETY: strict fns — args 0 and 1 are sparsevecs.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let b = unsafe { arg_sparsevec(fcinfo, 1)? };
    Ok((a, b))
}

// sparsevec (sparsevec.c:588-598): applies the type modifier.
pub fn fc_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec, arg1 typmod.
    let v = unsafe { arg_sparsevec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_expected_dim(typmod, v.dim())?;
    Ok(fcinfo.arg(0))
}

// vector_to_sparsevec (sparsevec.c:603-642).
pub fn fc_vector_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, arg1 typmod.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = v.dim() as i32;
    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;
    let nnz = v.iter().filter(|x| *x != 0.0).count();
    check_nnz(nnz as i32, dim)?;
    let mut r = SparseBuilder::new(fcinfo.result_mcx(), dim, nnz)?;
    let mut j = 0usize;
    for i in 0..v.dim() {
        if v.x(i) != 0.0 {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 633, "vector_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, v.x(i));
            j += 1;
        }
    }
    Ok(image_datum(r.image()))
}

// halfvec_to_sparsevec (sparsevec.c:647-686).
pub fn fc_halfvec_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { crate::halfvec::arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    let dim = v.dim() as i32;
    check_dim(dim)?;
    check_expected_dim(typmod, dim)?;
    let nnz = (0..v.dim()).filter(|&i| !half_is_zero(v.x(i))).count();
    check_nnz(nnz as i32, dim)?;
    let mut r = SparseBuilder::new(fcinfo.result_mcx(), dim, nnz)?;
    let mut j = 0usize;
    for i in 0..v.dim() {
        if !half_is_zero(v.x(i)) {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 677, "halfvec_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, half_to_float4(v.x(i)));
            j += 1;
        }
    }
    Ok(image_datum(r.image()))
}

// array_to_sparsevec (sparsevec.c:691-818). C converts each element twice
// (count pass, fill pass, sparsevec.c:730-799); converting once into
// `values` gives the same floats and the same first error.
pub fn fc_array_to_sparsevec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 array, arg1 typmod.
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let typmod = fcinfo.arg_i32(1);
    let site = ArraySite { file: "sparsevec.c", func: "array_to_sparsevec", ndim_line: 710, nulls_line: 715 };
    let (elemtype, elems) = cast_array_elems(mcx, arr, &site)?;
    let n = elems.len() as i32;
    check_dim(n)?;
    check_expected_dim(typmod, n)?;
    let mut values: PgVec<'_, f32> = mcx::vec_with_capacity_in(mcx, elems.len())?;
    for d in elems.iter() {
        match cast_elem_f32(elemtype, *d)? {
            Some(v) => values.push(v),
            None => return Err(ereport(ERRCODE_DATA_EXCEPTION, "unsupported array type", 754, "array_to_sparsevec")),
        }
    }
    // IS_NOT_ZERO (sparsevec.c:727): NaN and infinities count as non-zero.
    let nnz = values.iter().filter(|v| **v != 0.0).count();
    check_nnz(nnz as i32, n)?;
    let mut r = SparseBuilder::new(mcx, n, nnz)?;
    let mut j = 0usize;
    for (i, v) in values.iter().enumerate() {
        if *v != 0.0 {
            // Safety check
            if j >= r.nnz() {
                return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 767, "array_to_sparsevec"));
            }
            r.set_index(j, i as i32);
            r.set_value(j, *v);
            j += 1;
        }
    }
    if j != r.nnz() {
        return Err(ereport(ERRCODE_INTERNAL_ERROR, "correctness check failed", 811, "array_to_sparsevec"));
    }
    // Check elements
    for i in 0..r.nnz() {
        check_element(r.value(i))?;
    }
    Ok(image_datum(r.image()))
}

// SparsevecL2SquaredDistance (sparsevec.c:823-866): one merge over the two
// sorted index lists.
pub fn sparsevec_l2_squared_distance(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        let mut bi = -1i32;
        for j in bpos..b.nnz() {
            bi = b.index(j);
            if ai == bi {
                let diff = a.value(i) - b.value(j);
                distance += diff * diff;
            } else if ai > bi {
                distance += b.value(j) * b.value(j);
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
        if ai != bi {
            distance += a.value(i) * a.value(i);
        }
    }
    for j in bpos..b.nnz() {
        distance += b.value(j) * b.value(j);
    }
    distance
}

// SparsevecInnerProduct (sparsevec.c:902-933). Upstream builds with
// -ffp-contract=fast (Makefile:38), and this merge loop compiles to a scalar
// fmadd chain in source order, so the sum fuses like C's. That makes it bit
// exact, including the sign of a zero when every product underflows.
pub fn sparsevec_inner_product(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        for j in bpos..b.nnz() {
            let bi = b.index(j);
            // Only update when the same index
            if ai == bi {
                distance = a.value(i).mul_add(b.value(j), distance);
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
    }
    distance
}

// sparsevec_l1_distance's merge (sparsevec.c:1021-1054), factored out like
// the L2 and inner-product kernels.
pub fn sparsevec_l1_distance(a: &SparseView<'_>, b: &SparseView<'_>) -> f32 {
    let mut distance = 0.0f32;
    let mut bpos = 0usize;
    for i in 0..a.nnz() {
        let ai = a.index(i);
        let mut bi = -1i32;
        for j in bpos..b.nnz() {
            bi = b.index(j);
            if ai == bi {
                distance += (a.value(i) - b.value(j)).abs();
            } else if ai > bi {
                distance += b.value(j).abs();
            }
            // Update start for next iteration
            if ai >= bi {
                bpos = j + 1;
            }
            // Found or passed it
            if bi >= ai {
                break;
            }
        }
        if ai != bi {
            distance += a.value(i).abs();
        }
    }
    for j in bpos..b.nnz() {
        distance += b.value(j).abs();
    }
    distance
}

// sparsevec_l2_distance (sparsevec.c:871-881).
pub fn fc_sparsevec_l2_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64((sparsevec_l2_squared_distance(&a, &b) as f64).sqrt()))
}

// sparsevec_l2_squared_distance (sparsevec.c:887-897).
pub fn fc_sparsevec_l2_squared_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_l2_squared_distance(&a, &b) as f64))
}

// sparsevec_inner_product (sparsevec.c:938-948).
pub fn fc_sparsevec_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_inner_product(&a, &b) as f64))
}

// sparsevec_negative_inner_product (sparsevec.c:953-963).
pub fn fc_sparsevec_negative_inner_product(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(-sparsevec_inner_product(&a, &b) as f64))
}

// sparsevec_cosine_distance (sparsevec.c:968-1008).
pub fn fc_sparsevec_cosine_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    let mut similarity = sparsevec_inner_product(&a, &b) as f64;
    let mut norma = 0.0f32;
    for i in 0..a.nnz() {
        norma += a.value(i) * a.value(i);
    }
    let mut normb = 0.0f32;
    for i in 0..b.nnz() {
        normb += b.value(i) * b.value(i);
    }
    // Use sqrt(a * b) over sqrt(a) * sqrt(b)
    similarity /= ((norma as f64) * (normb as f64)).sqrt();
    // Keep in range
    if similarity > 1.0 {
        similarity = 1.0;
    } else if similarity < -1.0 {
        similarity = -1.0;
    }
    Ok(Datum::from_f64(1.0 - similarity))
}

// sparsevec_l1_distance (sparsevec.c:1013-1057).
pub fn fc_sparsevec_l1_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    check_dims(&a, &b)?;
    Ok(Datum::from_f64(sparsevec_l1_distance(&a, &b) as f64))
}

// sparsevec_vector_dims (sparsevec.c:1064-1071); not in the 0.8.7 SQL.
pub fn fc_sparsevec_vector_dims(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    Ok(Datum::from_i32(a.dim()))
}

// sparsevec_l2_norm (sparsevec.c:1076-1089).
pub fn fc_sparsevec_l2_norm(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mut norm = 0.0f64;
    for i in 0..a.nnz() {
        norm += a.value(i) as f64 * a.value(i) as f64;
    }
    Ok(Datum::from_f64(norm.sqrt()))
}

// sparsevec_l2_normalize (sparsevec.c:1094-1158).
pub fn fc_sparsevec_l2_normalize(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 sparsevec.
    let a = unsafe { arg_sparsevec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut r = SparseBuilder::new(mcx, a.dim(), a.nnz())?;
    let mut norm = 0.0f64;
    for i in 0..a.nnz() {
        norm += a.value(i) as f64 * a.value(i) as f64;
    }
    norm = norm.sqrt();
    // Return zero vector for zero norm
    if norm > 0.0 {
        let mut zeros = 0usize;
        for i in 0..a.nnz() {
            r.set_index(i, a.index(i));
            // C stores the double quotient into a float.
            let v = (a.value(i) as f64 / norm) as f32;
            r.set_value(i, v);
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            if v == 0.0 {
                zeros += 1;
            }
        }
        // Allocate a new vector in the unlikely event there are zeros
        if zeros > 0 {
            let mut n = SparseBuilder::new(mcx, a.dim(), a.nnz() - zeros)?;
            let mut j = 0usize;
            for i in 0..a.nnz() {
                if r.value(i) == 0.0 {
                    continue;
                }
                // Safety check
                if j >= n.nnz() {
                    return Err(ereport(ERRCODE_INTERNAL_ERROR, "index out of bounds", 1144, "sparsevec_l2_normalize"));
                }
                n.set_index(j, r.index(i));
                n.set_value(j, r.value(i));
                j += 1;
            }
            return Ok(image_datum(n.image()));
        }
    }
    Ok(image_datum(r.image()))
}

// sparsevec_cmp_internal (sparsevec.c:1163-1199).
fn sparsevec_cmp_internal(a: &SparseView<'_>, b: &SparseView<'_>) -> i32 {
    let nnz = a.nnz().min(b.nnz());
    // Check values before dimensions to be consistent with Postgres arrays
    for i in 0..nnz {
        if a.index(i) < b.index(i) {
            return if a.value(i) < 0.0 { -1 } else { 1 };
        }
        if a.index(i) > b.index(i) {
            return if b.value(i) < 0.0 { 1 } else { -1 };
        }
        if a.value(i) < b.value(i) {
            return -1;
        }
        if a.value(i) > b.value(i) {
            return 1;
        }
    }
    if a.nnz() < b.nnz() && b.index(nnz) < a.dim() {
        return if b.value(nnz) < 0.0 { 1 } else { -1 };
    }
    if a.nnz() > b.nnz() && a.index(nnz) < b.dim() {
        return if a.value(nnz) < 0.0 { -1 } else { 1 };
    }
    if a.dim() < b.dim() {
        return -1;
    }
    if a.dim() > b.dim() {
        return 1;
    }
    0
}

// sparsevec_lt (sparsevec.c:1204-1212).
pub fn fc_sparsevec_lt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) < 0))
}

// sparsevec_le (sparsevec.c:1217-1225).
pub fn fc_sparsevec_le(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) <= 0))
}

// sparsevec_eq (sparsevec.c:1230-1238).
pub fn fc_sparsevec_eq(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) == 0))
}

// sparsevec_ne (sparsevec.c:1243-1251).
pub fn fc_sparsevec_ne(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) != 0))
}

// sparsevec_ge (sparsevec.c:1256-1264).
pub fn fc_sparsevec_ge(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) >= 0))
}

// sparsevec_gt (sparsevec.c:1269-1277).
pub fn fc_sparsevec_gt(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_bool(sparsevec_cmp_internal(&a, &b) > 0))
}

// sparsevec_cmp (sparsevec.c:1282-1290).
pub fn fc_sparsevec_cmp(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = sparsevec_2arg(fcinfo)?;
    Ok(Datum::from_i32(sparsevec_cmp_internal(&a, &b)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // "{1:1.5,3:3.5}/5"-style text, 1-based; test values print exactly.
    fn render(v: &SparseView<'_>) -> String {
        let els: Vec<String> = (0..v.nnz()).map(|i| format!("{}:{}", v.index(i) + 1, v.value(i))).collect();
        format!("{{{}}}/{}", els.join(","), v.dim())
    }

    fn input(s: &str) -> Result<String, String> {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let r = match sparsevec_in_body(ctx.mcx(), s.as_bytes(), -1) {
            Ok(img) => Ok(render(&SparseView::from_payload(&img[4..]).unwrap())),
            Err(e) => Err(e.message().to_string()),
        };
        r
    }

    // sparsevec.out: indices sort; zero values are dropped.
    #[test]
    fn input_parses_like_c() {
        assert_eq!(input("{1:1.5,3:3.5}/5").unwrap(), "{1:1.5,3:3.5}/5");
        assert_eq!(input(" { 1 : 1.5 ,  3  :  3.5  } / 5 ").unwrap(), "{1:1.5,3:3.5}/5");
        assert_eq!(input("{1:0,2:1,3:0}/3").unwrap(), "{2:1}/3");
        assert_eq!(input("{2:1,1:1}/2").unwrap(), "{1:1,2:1}/2");
        assert_eq!(input("{}/5").unwrap(), "{}/5");
        assert_eq!(input("{10:0}/3").unwrap(), "{}/3");
    }

    #[test]
    fn input_errors_match_sparsevec_out() {
        let err = |s: &str| input(s).unwrap_err();
        assert_eq!(err("{1:1,1:1}/2"), "sparsevec indices must not contain duplicates");
        assert_eq!(err("{1:1,2:1,1:1}/2"), "sparsevec indices must not contain duplicates");
        assert_eq!(err("{}/-1"), "sparsevec must have at least 1 dimension");
        assert_eq!(err("{}/1000000001"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{}/2147483648"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{}/-2147483649"), "sparsevec must have at least 1 dimension");
        assert_eq!(err("{}/9223372036854775808"), "sparsevec cannot have more than 1000000000 dimensions");
        assert_eq!(err("{2147483647:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{2147483648:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{-2147483649:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{0:1}/1"), "sparsevec index out of bounds");
        assert_eq!(err("{1:1e-46,2:1}/2"), "\"1e-46\" is out of range for type sparsevec");
        assert_eq!(err("{1:-4e38,2:1}/2"), "\"-4e38\" is out of range for type sparsevec");
        assert_eq!(err("{1:NaN,2:1}/2"), "NaN not allowed in sparsevec");
        assert_eq!(err("{1:-Infinity,2:1}/2"), "infinite value not allowed in sparsevec");
        assert_eq!(err("{}"), "invalid input syntax for type sparsevec: \"{}\"");
        assert_eq!(err("{1:}/1"), "invalid input syntax for type sparsevec: \"{1:}/1\"");
        assert_eq!(err(""), "invalid input syntax for type sparsevec: \"\"");
        let many = format!("{{{}1:1}}/1", "1:1,".repeat(16000));
        assert_eq!(err(&many), "sparsevec cannot have more than 16000 non-zero elements");
    }

    // Review Focus 1.
    #[test]
    fn input_survives_every_prefix() {
        let lit = " { 1 : 1.5 , 3:-2e-3,4294967296:1, -1:0 } / 9223372036854775808 ";
        for end in 0..=lit.len() {
            let _ = input(&lit[..end]);
        }
    }

    #[test]
    fn strtol_saturates_like_64_bit_long() {
        assert_eq!(strtol_prefix(b"  42:"), Some((42, 4)));
        assert_eq!(strtol_prefix(b"-7"), Some((-7, 2)));
        assert_eq!(strtol_prefix(b"+0x5"), Some((0, 2)));
        assert_eq!(strtol_prefix(b"9223372036854775808"), Some((i64::MAX, 19)));
        assert_eq!(strtol_prefix(b"-9223372036854775808"), Some((i64::MIN, 20)));
        assert_eq!(strtol_prefix(b"-9223372036854775809"), Some((i64::MIN, 20)));
        assert_eq!(strtol_prefix(b"-"), None);
        assert_eq!(strtol_prefix(b" :1"), None);
    }

    #[test]
    fn strtof_underflow_is_a_nonzero_literal_read_as_zero() {
        assert!(underflowed_to_zero(b"1e-46", 0.0));
        assert!(underflowed_to_zero(b"-0.0001e-42", 0.0));
        assert!(underflowed_to_zero(b"0x1p-200", 0.0));
        assert!(!underflowed_to_zero(b"0e5", 0.0));
        assert!(!underflowed_to_zero(b"-0.000", 0.0));
        assert!(!underflowed_to_zero(b"0x0p-3", 0.0));
        assert!(!underflowed_to_zero(b"1e-45", 1.4e-45));
    }

    fn recv(bytes: &[u8], typmod: i32) -> Result<String, String> {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let mcx = ctx.mcx();
        let mut buf = stringinfo::StringInfo::new_in(mcx).unwrap();
        buf.append_bytes(bytes).unwrap();
        let r = match sparsevec_recv_body(mcx, &mut buf, typmod) {
            Ok(img) => Ok(render(&SparseView::from_payload(&img[4..]).unwrap())),
            Err(e) => Err(e.message().to_string()),
        };
        r
    }

    fn msg(dim: i32, nnz: i32, unused: i32, idx: &[i32], vals: &[f32]) -> Vec<u8> {
        let mut v = Vec::new();
        for x in [dim, nnz, unused].iter().chain(idx) {
            v.extend(x.to_be_bytes());
        }
        for x in vals {
            v.extend(x.to_bits().to_be_bytes());
        }
        v
    }

    // Review Focus 2: binary input is validated like sparsevec_recv
    // (sparsevec.c:521-553), before allocating.
    #[test]
    fn recv_validates_like_c() {
        assert_eq!(recv(&msg(5, 2, 0, &[0, 2], &[1.5, -2.0]), -1).unwrap(), "{1:1.5,3:-2}/5");
        let err = |m: Vec<u8>, t: i32| recv(&m, t).unwrap_err();
        assert_eq!(err(msg(0, 0, 0, &[], &[]), -1), "sparsevec must have at least 1 dimension");
        assert_eq!(err(msg(5, -1, 0, &[], &[]), -1), "sparsevec cannot have negative number of elements");
        assert_eq!(err(msg(1_000_000_000, 16001, 0, &[], &[]), -1), "sparsevec cannot have more than 16000 non-zero elements");
        assert_eq!(err(msg(2, 3, 0, &[], &[]), -1), "sparsevec cannot have more elements than dimensions");
        assert_eq!(err(msg(5, 0, 0, &[], &[]), 4), "expected 4 dimensions, not 5");
        assert_eq!(err(msg(5, 0, 1, &[], &[]), -1), "expected unused to be 0, not 1");
        assert_eq!(err(msg(5, 2, 0, &[2, 0], &[1.0, 1.0]), -1), "sparsevec indices must be in ascending order");
        assert_eq!(err(msg(5, 2, 0, &[1, 1], &[1.0, 1.0]), -1), "sparsevec indices must not contain duplicates");
        assert_eq!(err(msg(5, 1, 0, &[5], &[1.0]), -1), "sparsevec index out of bounds");
        assert_eq!(err(msg(5, 1, 0, &[0], &[0.0]), -1), "binary representation of sparsevec cannot contain zero values");
        assert_eq!(err(msg(5, 1, 0, &[0], &[f32::NAN]), -1), "NaN not allowed in sparsevec");
        assert_eq!(err(msg(5, 2, 0, &[0], &[]), -1), "insufficient data left in message");
    }

    #[test]
    fn from_payload_rejects_corrupt_images() {
        let hdr = |dim: i32, nnz: i32, extra: usize| {
            let mut v = [dim.to_ne_bytes(), nnz.to_ne_bytes(), 0i32.to_ne_bytes()].concat();
            v.resize(12 + extra, 0);
            v
        };
        let corrupt = |p: Vec<u8>| SparseView::from_payload(&p).err().unwrap().message().to_string();
        assert_eq!(corrupt(hdr(0, 0, 0)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, -1, 0)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, 6, 48)), "corrupt sparsevec datum");
        assert_eq!(corrupt(hdr(5, 2, 8)), "corrupt sparsevec datum");
        assert_eq!(SparseView::from_payload(&hdr(5, 2, 16)).unwrap().nnz(), 2);
    }

    struct XorShift(u64);

    impl XorShift {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    // A sparse image of a random dense vector with integer values (a third
    // of them non-zero), plus the dense form.
    fn random_sparse<'m>(m: Mcx<'m>, r: &mut XorShift, dim: i32) -> (PgVec<'m, u8>, Vec<f32>) {
        let mut dense = vec![0.0f32; dim as usize];
        for x in dense.iter_mut() {
            if r.below(3) == 0 {
                *x = r.below(9) as f32 - 4.0;
            }
        }
        let nz: Vec<usize> = (0..dense.len()).filter(|&i| dense[i] != 0.0).collect();
        let mut b = SparseBuilder::new(m, dim, nz.len()).unwrap();
        for (j, &i) in nz.iter().enumerate() {
            b.set_index(j, i as i32);
            b.set_value(j, dense[i]);
        }
        (b.image(), dense)
    }

    // Review Focus 4: the merge kernels against a dense expansion over
    // disjoint, interleaved, prefix/suffix and empty index sets. Integer
    // values keep every f32 sum exact, so equality is exact.
    #[test]
    fn sparse_kernels_match_dense_expansion() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let mut r = XorShift(0x9E37_79B9_7F4A_7C15);
        for _ in 0..5000 {
            let dim = 1 + r.below(12) as i32;
            let (ia, da) = random_sparse(m, &mut r, dim);
            let (ib, db) = random_sparse(m, &mut r, dim);
            let a = SparseView::from_payload(&ia[4..]).unwrap();
            let b = SparseView::from_payload(&ib[4..]).unwrap();
            let l2: f32 = da.iter().zip(&db).map(|(x, y)| (x - y) * (x - y)).sum();
            let ip: f32 = da.iter().zip(&db).map(|(x, y)| x * y).sum();
            let l1: f32 = da.iter().zip(&db).map(|(x, y)| (x - y).abs()).sum();
            assert_eq!(sparsevec_l2_squared_distance(&a, &b), l2, "{da:?} {db:?}");
            assert_eq!(sparsevec_inner_product(&a, &b), ip, "{da:?} {db:?}");
            assert_eq!(sparsevec_l1_distance(&a, &b), l1, "{da:?} {db:?}");
        }
    }

    // inner_product(sparsevec, sparsevec) on the C reference (macOS arm64),
    // bit for bit, sign of zero included: the merge loop compiles to a
    // scalar fmadd chain. The first pair is diff tier finding seed=1
    // stmt_index=1180 (every product underflows; the last one is negative).
    #[test]
    fn inner_product_fuses_like_c() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let cases = [
            (
                "{1:1e-40,2:1.2e-38,3:-1.4e-45,4:1e-40,5:-1.4e-45,6:1e-40,7:1e-40,8:1e-40}/8",
                "{1:-1.4e-45,2:1.2e-38,3:1.2e-38,5:-1.4e-45,6:-1.4e-45,7:-1.4e-45,8:0}/8",
                "-0",
            ),
            ("{1:0.1,2:0.2,3:0.3}/3", "{1:0.4,2:0.5,3:0.6}/3", "0.320000022649765"),
            ("{1:1.1,2:-2.3,4:0.7}/4", "{1:0.3,2:0.9,4:-1.9}/4", "-3.069999933242798"),
            (
                "{1:-1.244658,3:-1.052791,5:1.120312,6:-0.725007,10:1.131244,13:-1.870595,14:1.642657}/16",
                "{1:0.501723,2:0.713599,3:0.125273,4:1.498753,5:-0.959627,7:-0.742455,13:-1.352874,14:-1.795352}/16",
                "-2.2499096393585205",
            ),
        ];
        for (sa, sb, want) in cases {
            let (ia, ib) = (sparsevec_in_body(m, sa.as_bytes(), -1).unwrap(), sparsevec_in_body(m, sb.as_bytes(), -1).unwrap());
            let a = SparseView::from_payload(&ia[4..]).unwrap();
            let b = SparseView::from_payload(&ib[4..]).unwrap();
            let got = sparsevec_inner_product(&a, &b) as f64;
            assert_eq!(got.to_bits(), want.parse::<f64>().unwrap().to_bits(), "{sa} . {sb}: got {got}, C {want}");
        }
    }

    // sparsevec.out: sparsevec_cmp follows Postgres array order.
    #[test]
    fn cmp_matches_sparsevec_out() {
        let ctx = mcx::MemoryContext::new("sparsevec-test");
        let m = ctx.mcx();
        let img = |s: &str| sparsevec_in_body(m, s.as_bytes(), -1).unwrap();
        let cmp = |a: &str, b: &str| {
            let (ia, ib) = (img(a), img(b));
            sparsevec_cmp_internal(&SparseView::from_payload(&ia[4..]).unwrap(), &SparseView::from_payload(&ib[4..]).unwrap())
        };
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{1:1,2:2,3:3}/3"), 0);
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{}/3"), 1);
        assert_eq!(cmp("{}/3", "{1:1,2:2,3:3}/3"), -1);
        assert_eq!(cmp("{1:1,2:2}/2", "{1:1,2:2,3:3}/3"), -1);
        assert_eq!(cmp("{1:1,2:2,3:3}/3", "{1:1,2:2}/2"), 1);
        assert_eq!(cmp("{1:1,2:2}/2", "{1:2,2:3,3:4}/3"), -1);
        assert_eq!(cmp("{1:2,2:3}/2", "{1:1,2:2,3:3}/3"), 1);
    }
}

//! halfvec.c (pgvector 0.8.7): the halfvec type, a varlena of
//! `i16 dim, i16 unused, half x[dim]` (halfvec.h:62-68) holding at most
//! 16,000 dimensions; its I/O, casts, functions and aggregates.
//! halfvec_to_vector lives in funcs.rs (vector.c); C keeps the sparsevec
//! casts other than sparsevec_to_halfvec in sparsevec.c, and so do we.
//!
//! Arithmetic widens to f32 and rounds once (halfvec.c's non-FLT16_SUPPORT
//! path); that equals C's _Float16 arithmetic bit for bit
//! (pgvector_f16_parity). Errors carry their halfvec.c location.

use datum::Datum;
use mcx::{Mcx, PgVec};
use stringinfo::StringInfo;
use types_core::FLOAT4OID;
use types_error::{
    PgError, PgResult, SqlState, ERRCODE_DATA_EXCEPTION, ERRCODE_INVALID_PARAMETER_VALUE,
    ERRCODE_INVALID_TEXT_REPRESENTATION, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
    ERRCODE_PROGRAM_LIMIT_EXCEEDED,
};
use types_fmgr::{cstring_result, FmgrInfo, FunctionCallInfoBaseData as Fcinfo};

use crate::funcs::{arg_vector, cast_array_elems, cast_elem_f32, detoasted_image, image_datum, ArraySite};
use crate::halfutils::{float4_to_half, float4_to_half_unchecked, half_is_inf, half_is_nan, half_to_float4, Half};
use crate::vec::{strtof_prefix, vector_isspace as halfvec_isspace, StrtofVal};

pub const HALFVEC_MAX_DIM: i32 = 16000;

// Payload layout after the 4-byte varlena header: i16 dim, i16 unused, half x[dim].
pub const HALFVEC_PAYLOAD_HDR: usize = 4;

// An ereport/elog of halfvec.c, located at the call's last line.
#[cold]
pub(crate) fn ereport(code: SqlState, msg: impl Into<String>, line: i32, func: &'static str) -> Box<PgError> {
    PgError::error(msg).with_sqlstate(code).with_location("halfvec.c", line, func).into()
}

#[derive(Clone, Copy)]
pub struct HalfView<'a> {
    data: &'a [u8],
}

impl<'a> HalfView<'a> {
    pub fn from_payload(data: &'a [u8]) -> PgResult<HalfView<'a>> {
        // Like VecView: check the raw i16 dim before widening it, so a corrupt
        // image never reaches the kernels with a bogus dim().
        if data.len() < HALFVEC_PAYLOAD_HDR {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        let raw_dim = i16::from_ne_bytes([data[0], data[1]]);
        if raw_dim < 1 || raw_dim as i32 > HALFVEC_MAX_DIM {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        if data.len() < HALFVEC_PAYLOAD_HDR + 2 * raw_dim as usize {
            return Err(PgError::error("corrupt halfvec datum").into());
        }
        Ok(HalfView { data })
    }

    #[inline]
    pub fn dim(&self) -> usize {
        i16::from_ne_bytes([self.data[0], self.data[1]]) as usize
    }

    #[inline]
    pub fn x(&self, i: usize) -> Half {
        let off = HALFVEC_PAYLOAD_HDR + 2 * i;
        Half::from_ne_bytes([self.data[off], self.data[off + 1]])
    }

    /// The `dim` halves as native-endian bytes, for the halfutils kernels.
    #[inline]
    pub fn xs(&self) -> &'a [u8] {
        &self.data[HALFVEC_PAYLOAD_HDR..HALFVEC_PAYLOAD_HDR + 2 * self.dim()]
    }
}

pub struct HalfBuilder<'mcx> {
    img: PgVec<'mcx, u8>,
}

// InitHalfVector (halfvec.c:132-144): full varlena image, zeroed halves.
impl<'mcx> HalfBuilder<'mcx> {
    pub fn new(mcx: Mcx<'mcx>, dim: usize) -> PgResult<HalfBuilder<'mcx>> {
        let size = 4 + HALFVEC_PAYLOAD_HDR + dim * 2;
        let mut img: PgVec<'mcx, u8> = mcx::vec_with_capacity_in(mcx, size)?;
        img.resize(size, 0);
        img[..4].copy_from_slice(&((size as u32) << 2).to_ne_bytes());
        img[4..6].copy_from_slice(&(dim as i16).to_ne_bytes());
        Ok(HalfBuilder { img })
    }

    #[inline]
    pub fn set(&mut self, i: usize, v: Half) {
        let off = 4 + HALFVEC_PAYLOAD_HDR + 2 * i;
        self.img[off..off + 2].copy_from_slice(&v.to_ne_bytes());
    }

    #[inline]
    pub fn get(&self, i: usize) -> Half {
        let off = 4 + HALFVEC_PAYLOAD_HDR + 2 * i;
        Half::from_ne_bytes([self.img[off], self.img[off + 1]])
    }

    pub fn image(self) -> PgVec<'mcx, u8> {
        self.img
    }
}

// SAFETY contract of callers: arg i is a non-null halfvec varlena (strict fns).
pub(crate) unsafe fn arg_halfvec<'a>(fcinfo: &'a Fcinfo, i: usize) -> PgResult<HalfView<'a>> {
    let v = unsafe { fcinfo.arg_varlena_packed(i)? };
    HalfView::from_payload(v.data())
}

// CheckDims (halfvec.c:74-81).
fn check_dims(a: &HalfView<'_>, b: &HalfView<'_>) -> PgResult<()> {
    if a.dim() != b.dim() {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("different halfvec dimensions {} and {}", a.dim(), b.dim()),
            80,
            "CheckDims",
        ));
    }
    Ok(())
}

// CheckExpectedDim (halfvec.c:86-93).
fn check_expected_dim(typmod: i32, dim: i32) -> PgResult<()> {
    if typmod != -1 && typmod != dim {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected {typmod} dimensions, not {dim}"),
            92,
            "CheckExpectedDim",
        ));
    }
    Ok(())
}

// CheckDim (halfvec.c:98-110). Signed like C's int: a negative dimension
// (an i16 off the wire, a subvector span) reports "at least 1 dimension".
pub(crate) fn check_dim(dim: i32) -> PgResult<()> {
    if dim < 1 {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 104, "CheckDim"));
    }
    if dim > HALFVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_PROGRAM_LIMIT_EXCEEDED,
            format!("halfvec cannot have more than {HALFVEC_MAX_DIM} dimensions"),
            109,
            "CheckDim",
        ));
    }
    Ok(())
}

// CheckElement (halfvec.c:115-127).
fn check_element(value: Half) -> PgResult<()> {
    if half_is_nan(value) {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "NaN not allowed in halfvec", 121, "CheckElement"));
    }
    if half_is_inf(value) {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "infinite value not allowed in halfvec", 126, "CheckElement"));
    }
    Ok(())
}

#[cold]
fn invalid_text(lit: &[u8], detail: Option<&str>, line: i32) -> Box<PgError> {
    let mut e = PgError::error(format!(
        "invalid input syntax for type halfvec: \"{}\"",
        String::from_utf8_lossy(lit)
    ))
    .with_sqlstate(ERRCODE_INVALID_TEXT_REPRESENTATION)
    .with_location("halfvec.c", line, "halfvec_in");
    if let Some(d) = detail {
        e = e.with_detail(d);
    }
    e.into()
}

// halfvec_in's parse (halfvec.c:182-286): strtof per element, each rounded
// to half; an element finite as a float but infinite as a half is out of
// range.
pub fn parse_halfvec(lit: &[u8], typmod: i32, x: &mut [Half; HALFVEC_MAX_DIM as usize]) -> PgResult<usize> {
    let n = lit.len();
    let mut pt = 0usize;
    let mut dim = 0usize;

    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt >= n || lit[pt] != b'[' {
        return Err(invalid_text(lit, Some("Vector contents must start with \"[\"."), 198));
    }
    pt += 1;
    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt < n && lit[pt] == b']' {
        return Err(ereport(ERRCODE_DATA_EXCEPTION, "halfvec must have at least 1 dimension", 208, "halfvec_in"));
    }

    loop {
        if dim == HALFVEC_MAX_DIM as usize {
            return Err(ereport(
                ERRCODE_PROGRAM_LIMIT_EXCEEDED,
                format!("halfvec cannot have more than {HALFVEC_MAX_DIM} dimensions"),
                218,
                "halfvec_in",
            ));
        }
        while pt < n && halfvec_isspace(lit[pt]) {
            pt += 1;
        }
        // Check for empty string like float4in
        if pt >= n {
            return Err(invalid_text(lit, None, 227));
        }
        let Some((val, consumed)) = strtof_prefix(&lit[pt..]) else {
            return Err(invalid_text(lit, None, 237));
        };
        // Check for range error like float4in
        let out_of_range = match val {
            StrtofVal::Erange(_) => true,
            StrtofVal::Ok(v) => {
                x[dim] = float4_to_half_unchecked(v);
                half_is_inf(x[dim]) && !v.is_infinite()
            }
        };
        if out_of_range {
            return Err(ereport(
                ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE,
                format!(
                    "\"{}\" is out of range for type halfvec",
                    String::from_utf8_lossy(&lit[pt..pt + consumed])
                ),
                245,
                "halfvec_in",
            ));
        }
        check_element(x[dim])?;
        dim += 1;
        pt += consumed;

        while pt < n && halfvec_isspace(lit[pt]) {
            pt += 1;
        }
        if pt < n && lit[pt] == b',' {
            pt += 1;
        } else if pt < n && lit[pt] == b']' {
            pt += 1;
            break;
        } else {
            return Err(invalid_text(lit, None, 265));
        }
    }

    // Only whitespace is allowed after the closing brace
    while pt < n && halfvec_isspace(lit[pt]) {
        pt += 1;
    }
    if pt != n {
        return Err(invalid_text(lit, Some("Junk after closing right brace."), 276));
    }

    check_dim(dim as i32)?;
    check_expected_dim(typmod, dim as i32)?;
    Ok(dim)
}

// halfvec_in (halfvec.c:180-286).
pub fn fc_halfvec_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict input fn — arg0 cstring, arg2 typmod.
    let lit = unsafe { fcinfo.arg_cstring(0) }.to_bytes();
    let typmod = fcinfo.arg_i32(2);
    let mut x = [0 as Half; HALFVEC_MAX_DIM as usize];
    let dim = parse_halfvec(lit, typmod, &mut x)?;
    let mut b = HalfBuilder::new(fcinfo.result_mcx(), dim)?;
    for (i, v) in x[..dim].iter().enumerate() {
        b.set(i, *v);
    }
    Ok(image_datum(b.image()))
}

// halfvec_out (halfvec.c:294-335): the shortest float text of each half.
pub fn fc_halfvec_out(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let dim = v.dim();
    let mcx = fcinfo.result_mcx();
    let mut out: PgVec<'_, u8> = mcx::vec_with_capacity_in(mcx, ryu::FLOAT_SHORTEST_DECIMAL_LEN * dim + 3)?;
    let mut scratch = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
    mcx::vec_append_bytes(&mut out, b"[")?;
    for i in 0..dim {
        if i > 0 {
            mcx::vec_append_bytes(&mut out, b",")?;
        }
        let n = ryu::float_to_shortest_decimal_bufn(half_to_float4(v.x(i)), &mut scratch);
        mcx::vec_append_bytes(&mut out, &scratch[..n])?;
    }
    mcx::vec_append_bytes(&mut out, b"]\0")?;
    Ok(cstring_result(out))
}

// halfvec_typmod_in (halfvec.c:340-366).
pub fn fc_halfvec_typmod_in(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 cstring[].
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let tl = arrayfuncs::array_get_integer_typmods(mcx, arr)?;
    if tl.len() != 1 {
        return Err(ereport(ERRCODE_INVALID_PARAMETER_VALUE, "invalid type modifier", 353, "halfvec_typmod_in"));
    }
    if tl[0] < 1 {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            "dimensions for type halfvec must be at least 1",
            358,
            "halfvec_typmod_in",
        ));
    }
    if tl[0] > HALFVEC_MAX_DIM {
        return Err(ereport(
            ERRCODE_INVALID_PARAMETER_VALUE,
            format!("dimensions for type halfvec cannot exceed {HALFVEC_MAX_DIM}"),
            363,
            "halfvec_typmod_in",
        ));
    }
    Ok(Datum::from_i32(tl[0]))
}

// halfvec_recv's body (halfvec.c:373-400): i16 dim, i16 unused, then dim raw
// halves (pq_getmsghalf, halfvec.c:42-53).
pub(crate) fn halfvec_recv_body<'m>(mcx: Mcx<'m>, buf: &mut StringInfo<'_>, typmod: i32) -> PgResult<PgVec<'m, u8>> {
    let dim = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    let unused = pqformat::pq_getmsgint(buf, 2)? as u16 as i16;
    check_dim(dim as i32)?;
    check_expected_dim(typmod, dim as i32)?;
    if unused != 0 {
        return Err(ereport(
            ERRCODE_DATA_EXCEPTION,
            format!("expected unused to be 0, not {unused}"),
            390,
            "halfvec_recv",
        ));
    }
    let mut b = HalfBuilder::new(mcx, dim as usize)?;
    for i in 0..dim as usize {
        let x = pqformat::pq_getmsgint(buf, 2)? as Half;
        check_element(x)?;
        b.set(i, x);
    }
    Ok(b.image())
}

// halfvec_recv (halfvec.c:371-400).
pub fn fc_halfvec_recv(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: recv arg0 is the live StringInfo per the recv ABI.
    let buf = unsafe { fcinfo.arg_stringinfo(0) };
    let typmod = fcinfo.arg_i32(2);
    Ok(image_datum(halfvec_recv_body(fcinfo.result_mcx(), buf, typmod)?))
}

// halfvec_send (halfvec.c:405-419): dim, unused, then each half's raw bits
// (pq_sendhalf, halfvec.c:58-69).
pub fn fc_halfvec_send(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let mut buf = pqformat::pq_begintypsend(fcinfo.result_mcx())?;
    pqformat::pq_sendint16(&mut buf, v.dim() as u16)?;
    pqformat::pq_sendint16(&mut buf, 0)?; // vec->unused, always zero
    for i in 0..v.dim() {
        pqformat::pq_sendint16(&mut buf, v.x(i))?;
    }
    Ok(types_fmgr::varlena_result(pqformat::pq_endtypsend(buf)))
}

// halfvec (halfvec.c:425-435): applies the type modifier.
pub fn fc_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec, arg1 typmod.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_expected_dim(typmod, v.dim() as i32)?;
    Ok(fcinfo.arg(0))
}

// array_to_halfvec (halfvec.c:440-509).
pub fn fc_array_to_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    // SAFETY: strict fn — arg0 array, arg1 typmod.
    let arr = unsafe { detoasted_image(mcx, fcinfo.arg(0))? };
    let typmod = fcinfo.arg_i32(1);
    let site = ArraySite { file: "halfvec.c", func: "array_to_halfvec", ndim_line: 456, nulls_line: 461 };
    let (elemtype, elems) = cast_array_elems(mcx, arr, &site)?;
    check_dim(elems.len() as i32)?;
    check_expected_dim(typmod, elems.len() as i32)?;
    let mut b = HalfBuilder::new(mcx, elems.len())?;
    for (i, d) in elems.iter().enumerate() {
        let Some(v) = cast_elem_f32(elemtype, *d)? else {
            return Err(ereport(ERRCODE_DATA_EXCEPTION, "unsupported array type", 495, "array_to_halfvec"));
        };
        b.set(i, float4_to_half(v)?);
    }
    // Check elements
    for i in 0..elems.len() {
        check_element(b.get(i))?;
    }
    Ok(image_datum(b.image()))
}

// halfvec_to_float4 (halfvec.c:514-533).
pub fn fc_halfvec_to_float4(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 halfvec.
    let v = unsafe { arg_halfvec(fcinfo, 0)? };
    let mcx = fcinfo.result_mcx();
    let mut datums: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, v.dim())?;
    for i in 0..v.dim() {
        datums.push(Datum::from_f32(half_to_float4(v.x(i))));
    }
    // Use TYPALIGN_INT for float4
    let img = arrayfuncs::construct_array(mcx, &datums, FLOAT4OID, 4, true, b'i')?;
    Ok(image_datum(img))
}

// vector_to_halfvec (halfvec.c:538-555).
pub fn fc_vector_to_halfvec(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    // SAFETY: strict fn — arg0 vector, arg1 typmod.
    let v = unsafe { arg_vector(fcinfo, 0)? };
    let typmod = fcinfo.arg_i32(1);
    check_dim(v.dim() as i32)?;
    check_expected_dim(typmod, v.dim() as i32)?;
    let mut b = HalfBuilder::new(fcinfo.result_mcx(), v.dim())?;
    for i in 0..v.dim() {
        b.set(i, float4_to_half(v.x(i))?);
    }
    Ok(image_datum(b.image()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Vec<f32>, String> {
        let mut x = Box::new([0 as Half; HALFVEC_MAX_DIM as usize]);
        match parse_halfvec(s.as_bytes(), -1, &mut x) {
            Ok(n) => Ok(x[..n].iter().map(|h| half_to_float4(*h)).collect()),
            Err(e) => Err(e.message().to_string()),
        }
    }

    // halfvec.out: every element rounds to half.
    #[test]
    fn parse_rounds_to_half() {
        assert_eq!(parse("[1,2,3]").unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(parse(" [ 1,  2 ,    3  ] ").unwrap(), vec![1.0, 2.0, 3.0]);
        assert_eq!(parse("[1.23456]").unwrap(), vec![1.234375]);
        assert_eq!(parse("[65519,-65519]").unwrap(), vec![65504.0, -65504.0]);
        assert_eq!(parse("[1e-8,-1e-8]").unwrap(), vec![0.0, -0.0]);
        assert_eq!(parse("[1e-46,1]").unwrap(), vec![0.0, 1.0]);
        assert_eq!(parse("[0x1p-3]").unwrap(), vec![0.125]);
    }

    #[test]
    fn parse_errors_match_halfvec_out() {
        let err = |s: &str| parse(s).unwrap_err();
        assert_eq!(err("[hello,1]"), "invalid input syntax for type halfvec: \"[hello,1]\"");
        assert_eq!(err("[NaN,1]"), "NaN not allowed in halfvec");
        assert_eq!(err("[Infinity,1]"), "infinite value not allowed in halfvec");
        assert_eq!(err("[65520,-65520]"), "\"65520\" is out of range for type halfvec");
        assert_eq!(err("[4e38,1]"), "\"4e38\" is out of range for type halfvec");
        assert_eq!(err("[]"), "halfvec must have at least 1 dimension");
        assert_eq!(err("[1,2,3"), "invalid input syntax for type halfvec: \"[1,2,3\"");
        assert_eq!(err("1,2,3"), "invalid input syntax for type halfvec: \"1,2,3\"");
        assert_eq!(err(""), "invalid input syntax for type halfvec: \"\"");
    }

    // Review Focus 1: every truncation of a valid literal is a clean error
    // or a value, never a panic.
    #[test]
    fn parse_survives_every_prefix() {
        let lit = " [ 1.5e-3 , -2 ,0x1p-3, 65504 ] ";
        for end in 0..=lit.len() {
            let _ = parse(&lit[..end]);
        }
        let want = vec![half_to_float4(float4_to_half_unchecked(1.5e-3)), -2.0, 0.125, 65504.0];
        assert_eq!(parse(lit).unwrap(), want);
    }

    fn recv(bytes: &[u8], typmod: i32) -> Result<Vec<f32>, String> {
        let ctx = mcx::MemoryContext::new("halfvec-test");
        let mcx = ctx.mcx();
        let mut buf = stringinfo::StringInfo::new_in(mcx).unwrap();
        buf.append_bytes(bytes).unwrap();
        let res = match halfvec_recv_body(mcx, &mut buf, typmod) {
            Ok(img) => {
                let v = HalfView::from_payload(&img[4..]).unwrap();
                Ok((0..v.dim()).map(|i| half_to_float4(v.x(i))).collect())
            }
            Err(e) => Err(e.message().to_string()),
        };
        res
    }

    fn msg(dim: i16, unused: i16, xs: &[u16]) -> Vec<u8> {
        let mut v = dim.to_be_bytes().to_vec();
        v.extend(unused.to_be_bytes());
        for x in xs {
            v.extend(x.to_be_bytes());
        }
        v
    }

    // Review Focus 2: binary input is validated like halfvec_recv
    // (halfvec.c:381-397).
    #[test]
    fn recv_validates_like_c() {
        assert_eq!(recv(&msg(2, 0, &[0x3C00, 0xC000]), -1).unwrap(), vec![1.0, -2.0]);
        assert_eq!(recv(&msg(0, 0, &[]), -1).unwrap_err(), "halfvec must have at least 1 dimension");
        assert_eq!(recv(&msg(-1, 0, &[]), -1).unwrap_err(), "halfvec must have at least 1 dimension");
        assert_eq!(recv(&msg(2, 0, &[0x3C00, 0x3C00]), 3).unwrap_err(), "expected 3 dimensions, not 2");
        assert_eq!(recv(&msg(1, 7, &[0x3C00]), -1).unwrap_err(), "expected unused to be 0, not 7");
        assert_eq!(recv(&msg(1, 0, &[0x7E00]), -1).unwrap_err(), "NaN not allowed in halfvec");
        assert_eq!(recv(&msg(1, 0, &[0xFC00]), -1).unwrap_err(), "infinite value not allowed in halfvec");
        assert_eq!(recv(&msg(3, 0, &[0x3C00]), -1).unwrap_err(), "insufficient data left in message");
    }

    #[test]
    fn from_payload_rejects_corrupt_images() {
        let corrupt = |p: &[u8]| HalfView::from_payload(p).err().unwrap().message().to_string();
        assert_eq!(corrupt(&[0xFF, 0xFF, 0, 0]), "corrupt halfvec datum");
        assert_eq!(corrupt(&[0, 0, 0, 0]), "corrupt halfvec datum");
        let mut big = vec![0u8; 4 + 2 * 16001];
        big[..2].copy_from_slice(&16001i16.to_ne_bytes());
        assert_eq!(corrupt(&big), "corrupt halfvec datum");
        let mut short = vec![0u8; 4 + 2];
        short[..2].copy_from_slice(&2i16.to_ne_bytes());
        assert_eq!(corrupt(&short), "corrupt halfvec datum");
        let mut ok = vec![0u8; 4 + 4];
        ok[..2].copy_from_slice(&2i16.to_ne_bytes());
        assert_eq!(HalfView::from_payload(&ok).unwrap().dim(), 2);
    }
}

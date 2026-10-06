//! bitvec.c (pgvector 0.8.7): hamming_distance and jaccard_distance over
//! Postgres' bit/varbit, and InitBitVector for binary_quantize.

use datum::Datum;
use mcx::{Mcx, PgVec};
use types_error::{PgError, PgResult, ERRCODE_DATA_EXCEPTION};
use types_fmgr::{FmgrInfo, FunctionCallInfoBaseData as Fcinfo};

use crate::bitutils::{bit_hamming_distance, bit_jaccard_distance};

// InitBitVector (bitvec.c:16-28): full varbit image (4-byte varlena header,
// i32 bit length, zeroed bits).
pub(crate) fn init_bit_vector<'m>(mcx: Mcx<'m>, dim: usize) -> PgResult<PgVec<'m, u8>> {
    let total = 4 + 4 + dim.div_ceil(8);
    let mut img: PgVec<'m, u8> = mcx::vec_with_capacity_in(mcx, total)?;
    img.resize(total, 0);
    img[..4].copy_from_slice(&((total as u32) << 2).to_ne_bytes());
    img[4..8].copy_from_slice(&(dim as i32).to_ne_bytes());
    Ok(img)
}

// CheckDims (bitvec.c:33-40). VARBITLEN prints with %u.
fn check_dims(a: &[u8], b: &[u8]) -> PgResult<()> {
    let (alen, blen) = (adt_varbit::payload_bitlen(a) as u32, adt_varbit::payload_bitlen(b) as u32);
    if alen != blen {
        return Err(PgError::error(format!("different bit lengths {alen} and {blen}"))
            .with_sqlstate(ERRCODE_DATA_EXCEPTION)
            .with_location("bitvec.c", 39, "CheckDims")
            .into());
    }
    Ok(())
}

// Payloads of the two bit arguments: [i32 bit length][bits].
fn bit_2arg(fcinfo: &Fcinfo) -> PgResult<(&[u8], &[u8])> {
    // SAFETY: strict fns — args 0 and 1 are non-null bit/varbit varlenas.
    let a = unsafe { fcinfo.arg_varlena_packed(0)? }.data();
    let b = unsafe { fcinfo.arg_varlena_packed(1)? }.data();
    Ok((a, b))
}

// hamming_distance (bitvec.c:45-55).
pub fn fc_hamming_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = bit_2arg(fcinfo)?;
    check_dims(a, b)?;
    let (ax, bx) = (adt_varbit::payload_bits(a), adt_varbit::payload_bits(b));
    Ok(Datum::from_f64(bit_hamming_distance(ax.len(), ax, bx, 0) as f64))
}

// jaccard_distance (bitvec.c:60-70).
pub fn fc_jaccard_distance(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let (a, b) = bit_2arg(fcinfo)?;
    check_dims(a, b)?;
    let (ax, bx) = (adt_varbit::payload_bits(a), adt_varbit::payload_bits(b));
    Ok(Datum::from_f64(bit_jaccard_distance(ax.len(), ax, bx, 0, 0, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_bit_vector_is_a_zeroed_varbit() {
        let ctx = mcx::MemoryContext::new("bitvec-test");
        let img = init_bit_vector(ctx.mcx(), 9).unwrap();
        assert_eq!(img.len(), 4 + 4 + 2);
        assert_eq!(adt_varbit::payload_bitlen(&img[4..]), 9);
        assert_eq!(adt_varbit::payload_bits(&img[4..]), &[0, 0]);
    }

    #[test]
    fn check_dims_names_both_lengths() {
        let three = [3i32.to_ne_bytes().as_slice(), &[0b1110_0000]].concat();
        let two = [2i32.to_ne_bytes().as_slice(), &[0b0000_0000]].concat();
        let e = check_dims(&three, &two).unwrap_err();
        assert_eq!(e.message(), "different bit lengths 3 and 2");
    }
}

//! halfutils.h / halfutils.c (pgvector 0.8.7): the `half` (IEEE binary16)
//! conversions and the halfvec distance kernels.
//!
//! The conversions port the software path halfutils.h takes when neither
//! F16C_SUPPORT nor FLT16_SUPPORT is defined (halfutils.h:62-239). IEEE
//! defines f32<->f16 round-to-nearest-even exactly, so they give the same
//! bits as the F16C and _Float16 paths; crate pgvector_f16_parity checks
//! every input. The kernels port the `*Default` loops (halfutils.c:29-207).
//! Upstream compiles with -fassociative-math -ffp-contract=fast (Makefile:38)
//! on every target, so the C extension's summation order and fusion are
//! compiler-chosen, not only under the x86 F16C dispatch. These loops keep C
//! source order (no reassociation, no fusion), as vec.rs's kernels do.

use types_error::{PgError, PgResult, ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE};

/// `half` without FLT16_SUPPORT (halfvec.h:56-58): the raw binary16 bits.
pub type Half = u16;

// HalfIsNan (halfutils.h:23-31).
#[inline]
pub fn half_is_nan(num: Half) -> bool {
    (num & 0x7C00) == 0x7C00 && (num & 0x7FFF) != 0x7C00
}

// HalfIsInf (halfutils.h:36-44).
#[inline]
pub fn half_is_inf(num: Half) -> bool {
    (num & 0x7FFF) == 0x7C00
}

// HalfIsZero (halfutils.h:49-57).
#[inline]
pub fn half_is_zero(num: Half) -> bool {
    (num & 0x7FFF) == 0x0000
}

// HalfToFloat4 (halfutils.h:62-141), software path.
#[inline]
pub fn half_to_float4(num: Half) -> f32 {
    let bin = num as u32;
    let mut exponent: i32 = ((bin & 0x7C00) >> 10) as i32;
    let mut mantissa: u32 = bin & 0x03FF;
    // Sign
    let mut result: u32 = (bin & 0x8000) << 16;
    if exponent == 31 {
        if mantissa == 0 {
            // Infinite
            result |= 0x7F80_0000;
        } else {
            // NaN
            result |= 0x7FC0_0000;
        }
    } else if exponent == 0 {
        // Subnormal
        if mantissa != 0 {
            exponent = -14;
            for _ in 0..10 {
                mantissa <<= 1;
                exponent -= 1;
                if (mantissa >> 10) % 2 == 1 {
                    mantissa &= 0x03ff;
                    break;
                }
            }
            result |= ((exponent + 127) as u32) << 23;
        }
    } else {
        // Normal
        result |= ((exponent - 15 + 127) as u32) << 23;
    }
    result |= mantissa << 13;
    f32::from_bits(result)
}

// Float4ToHalfUnchecked (halfutils.h:146-239), software path.
#[inline]
pub fn float4_to_half_unchecked(num: f32) -> Half {
    let bin = num.to_bits();
    let mut exponent: i32 = ((bin & 0x7F80_0000) >> 23) as i32;
    let mut mantissa: i32 = (bin & 0x007F_FFFF) as i32;
    // Sign
    let mut result: u16 = ((bin & 0x8000_0000) >> 16) as u16;
    if num.is_infinite() {
        // Infinite
        result |= 0x7C00;
    } else if num.is_nan() {
        // NaN
        result |= 0x7E00;
        result |= (mantissa >> 13) as u16;
    } else if exponent > 98 {
        exponent -= 127;
        let mut s = mantissa & 0x0000_0FFF;
        // Subnormal
        if exponent < -14 {
            let diff = -exponent - 14;
            mantissa >>= diff;
            mantissa += 1 << (23 - diff);
            s |= mantissa & 0x0000_0FFF;
        }
        let mut m = mantissa >> 13;
        // Round
        let gr = (mantissa >> 12) % 4;
        if gr == 3 || (gr == 1 && s != 0) {
            m += 1;
        }
        if m == 1024 {
            m = 0;
            exponent += 1;
        }
        if exponent > 15 {
            // Infinite
            result |= 0x7C00;
        } else {
            if exponent >= -14 {
                result |= ((exponent + 15) << 10) as u16;
            }
            result |= m as u16;
        }
    }
    result
}

// Float4ToHalf (halfutils.h:244-261): a finite float that rounds to an
// infinite half is out of range.
pub fn float4_to_half(num: f32) -> PgResult<Half> {
    let result = float4_to_half_unchecked(num);
    if half_is_inf(result) && !num.is_infinite() {
        let mut buf = [0u8; ryu::FLOAT_SHORTEST_DECIMAL_LEN];
        let n = ryu::float_to_shortest_decimal_bufn(num, &mut buf);
        return Err(PgError::error(format!(
            "\"{}\" is out of range for type halfvec",
            String::from_utf8_lossy(&buf[..n])
        ))
        .with_sqlstate(ERRCODE_NUMERIC_VALUE_OUT_OF_RANGE)
        .with_location("halfutils.h", 257, "Float4ToHalf")
        .into());
    }
    Ok(result)
}

// Element i of a halfvec's native-endian half array, as a float.
#[inline]
fn hx(x: &[u8], i: usize) -> f32 {
    half_to_float4(Half::from_ne_bytes([x[2 * i], x[2 * i + 1]]))
}

// HalfvecL2SquaredDistanceDefault (halfutils.c:29-43).
pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        let diff = hx(ax, i) - hx(bx, i);
        distance += diff * diff;
    }
    distance
}

// HalfvecInnerProductDefault (halfutils.c:81-91).
pub fn halfvec_inner_product(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        distance += hx(ax, i) * hx(bx, i);
    }
    distance
}

// HalfvecCosineSimilarityDefault (halfutils.c:124-144).
pub fn halfvec_cosine_similarity(dim: usize, ax: &[u8], bx: &[u8]) -> f64 {
    let mut similarity = 0.0f32;
    let mut norma = 0.0f32;
    let mut normb = 0.0f32;
    for i in 0..dim {
        let (axi, bxi) = (hx(ax, i), hx(bx, i));
        similarity += axi * bxi;
        norma += axi * axi;
        normb += bxi * bxi;
    }
    // Use sqrt(a * b) over sqrt(a) * sqrt(b)
    similarity as f64 / ((norma as f64) * (normb as f64)).sqrt()
}

// HalfvecL1DistanceDefault (halfutils.c:197-207).
pub fn halfvec_l1_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for i in 0..dim {
        distance += (hx(ax, i) - hx(bx, i)).abs();
    }
    distance
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halves(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| float4_to_half_unchecked(*x).to_ne_bytes()).collect()
    }

    // halfvec.out values: 1.23456 stores 1.234375; 65519 rounds down to
    // HALF_MAX, 65520 up to infinity; 1e-8 underflows to signed zero.
    #[test]
    fn conversion_spot_values() {
        assert_eq!(float4_to_half_unchecked(1.0), 0x3C00);
        assert_eq!(float4_to_half_unchecked(-2.0), 0xC000);
        assert_eq!(float4_to_half_unchecked(1.23456), 0x3CF0);
        assert_eq!(float4_to_half_unchecked(65504.0), 0x7BFF);
        assert_eq!(float4_to_half_unchecked(65519.0), 0x7BFF);
        assert_eq!(float4_to_half_unchecked(65520.0), 0x7C00);
        assert_eq!(float4_to_half_unchecked(2f32.powi(-24)), 0x0001);
        assert_eq!(float4_to_half_unchecked(1e-8), 0x0000);
        assert_eq!(float4_to_half_unchecked(-1e-8), 0x8000);
        assert_eq!(half_to_float4(0x3CF0), 1.234375);
        assert_eq!(half_to_float4(0x0001), 2f32.powi(-24));
        assert_eq!(half_to_float4(0x7BFF), 65504.0);
        assert!(half_to_float4(0x7C00).is_infinite());
        assert!(half_to_float4(0x7E00).is_nan());
        assert!(half_is_nan(0x7E00) && !half_is_nan(0x7C00));
        assert!(half_is_inf(0xFC00) && !half_is_inf(0x7BFF));
        assert!(half_is_zero(0x8000) && !half_is_zero(0x0001));
    }

    // Float4ToHalf (halfutils.h:244-261): only a finite input that rounds
    // to infinity is out of range; an infinite input is CheckElement's job.
    #[test]
    fn float4_to_half_rejects_finite_overflow_only() {
        assert_eq!(float4_to_half(65519.0).unwrap(), 0x7BFF);
        let e = float4_to_half(65520.0).unwrap_err();
        assert_eq!(e.message(), "\"65520\" is out of range for type halfvec");
        assert_eq!(float4_to_half(f32::INFINITY).unwrap(), 0x7C00);
    }

    #[test]
    fn kernels_on_small_vectors() {
        let (a, b) = (halves(&[0.0, 0.0]), halves(&[3.0, 4.0]));
        assert_eq!(halfvec_l2_squared_distance(2, &a, &b), 25.0);
        assert_eq!(halfvec_l1_distance(2, &a, &b), 7.0);
        let (c, d) = (halves(&[1.0, 2.0]), halves(&[3.0, 4.0]));
        assert_eq!(halfvec_inner_product(2, &c, &d), 11.0);
        assert_eq!(halfvec_cosine_similarity(2, &c, &halves(&[2.0, 4.0])), 1.0);
    }
}

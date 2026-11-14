//! halfutils.h / halfutils.c (pgvector 0.8.7): the `half` (IEEE binary16)
//! conversions and the halfvec distance kernels.
//!
//! The conversions port the software path halfutils.h takes when neither
//! F16C_SUPPORT nor FLT16_SUPPORT is defined (halfutils.h:62-239). IEEE
//! defines f32<->f16 round-to-nearest-even exactly, so they give the same
//! bits as the F16C and _Float16 paths; crate pgvector_f16_parity checks
//! every input. `half_to_float4` stays the per-element software port; the
//! kernels convert in blocks through `halves_to_floats` (NEON `fcvtl` on
//! little-endian AArch64, a vectorizable branch-free routine elsewhere), and
//! every route is bit-identical (the exhaustive test here, and the parity
//! crate against C). The kernels port the `*Default` loops (halfutils.c:29-207).
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

/// Halves the distance kernels convert per step: two blocks of this many `f32`
/// live on the kernels' stack.
pub const BLOCK: usize = 32;

// The behaviour of HalfToFloat4 (halfutils.h:62-141) without a branch the
// compiler cannot turn into a select, so a loop over it vectorizes.
#[inline(always)]
pub fn half_to_float4_portable(num: Half) -> f32 {
    let h = num as u32;
    let sign = (h & 0x8000) << 16;
    let e = h & 0x7C00;
    let m = h & 0x03FF;
    // Normal: the fields shifted up 13 and the exponent rebiased by 127 - 15, exact.
    let normal = ((h & 0x7FFF) << 13) + (112u32 << 23);
    // Subnormal or zero: (2^-14 * (1 + m/1024)) - 2^-14 = m * 2^-24, exact (Sterbenz).
    let sub = (f32::from_bits(0x3880_0000 | (m << 13)) - f32::from_bits(0x3880_0000)).to_bits();
    // Infinity or NaN: exponent 255, payload shifted, NaN quieted as the software path does.
    let quiet = if m != 0 { 0x0040_0000 } else { 0 };
    let infnan = 0x7F80_0000 | (m << 13) | quiet;
    let r = if e == 0 {
        sub
    } else if e == 0x7C00 {
        infnan
    } else {
        normal
    };
    f32::from_bits(sign | r)
}

// `half_to_float4_portable` over `dst.len()` halves of `src`.
#[inline]
fn portable_block(src: &[u8], dst: &mut [f32]) {
    for (d, h) in dst.iter_mut().zip(src.chunks_exact(2)) {
        *d = half_to_float4_portable(Half::from_ne_bytes([h[0], h[1]]));
    }
}

/// `dst[i] = half_to_float4(Half::from_ne_bytes([src[2*i], src[2*i+1]]))` for
/// every `i < dst.len()`. Reads only `src[..2 * dst.len()]`, which it slices
/// once (panics if `src` is shorter). `src` may start at any byte. Widening
/// f16 to f32 is exact, so every route gives `half_to_float4`'s bits.
#[inline]
pub fn halves_to_floats(src: &[u8], dst: &mut [f32]) {
    let src = &src[..2 * dst.len()];
    #[cfg(all(target_arch = "aarch64", target_endian = "little", target_feature = "neon"))]
    {
        use std::arch::aarch64::*;
        let mut s = src.chunks_exact(16);
        let mut d = dst.chunks_exact_mut(8);
        for (sc, dc) in (&mut s).zip(&mut d) {
            // SAFETY: `sc` is 16 readable bytes and `dc` eight writable f32s;
            // vld1q_u8 takes a byte-aligned pointer and vst1q_f32 an
            // f32-aligned one, which `&mut [f32]` gives.
            unsafe {
                let h = vreinterpretq_u16_u8(vld1q_u8(sc.as_ptr()));
                let lo = vcvt_f32_f16(vreinterpret_f16_u16(vget_low_u16(h)));
                let hi = vcvt_high_f32_f16(vreinterpretq_f16_u16(h));
                vst1q_f32(dc.as_mut_ptr(), lo);
                vst1q_f32(dc.as_mut_ptr().add(4), hi);
            }
        }
        portable_block(s.remainder(), d.into_remainder());
    }
    #[cfg(not(all(target_arch = "aarch64", target_endian = "little", target_feature = "neon")))]
    portable_block(src, dst);
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

// The `dim` halves of a halfvec's native-endian array, as floats. Slicing
// the array once bounds-checks it once per kernel, not once per element.
#[inline]
fn half_floats(x: &[u8], dim: usize) -> impl Iterator<Item = f32> + '_ {
    x[..2 * dim].chunks_exact(2).map(|h| half_to_float4(Half::from_ne_bytes([h[0], h[1]])))
}

// HalfvecL2SquaredDistanceDefault (halfutils.c:29-43).
pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        let diff = a - b;
        distance += diff * diff;
    }
    distance
}

// HalfvecInnerProductDefault (halfutils.c:81-91).
pub fn halfvec_inner_product(dim: usize, ax: &[u8], bx: &[u8]) -> f32 {
    let mut distance = 0.0f32;
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        distance += a * b;
    }
    distance
}

// HalfvecCosineSimilarityDefault (halfutils.c:124-144).
pub fn halfvec_cosine_similarity(dim: usize, ax: &[u8], bx: &[u8]) -> f64 {
    let mut similarity = 0.0f32;
    let mut norma = 0.0f32;
    let mut normb = 0.0f32;
    for (axi, bxi) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
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
    for (a, b) in half_floats(ax, dim).zip(half_floats(bx, dim)) {
        distance += (a - b).abs();
    }
    distance
}

#[cfg(test)]
mod tests {
    use super::*;

    struct XorShift(u64);

    impl XorShift {
        // A random finite half (the kernels never see NaN or infinity:
        // CheckElement rejects them on input).
        fn half(&mut self) -> Half {
            loop {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                let h = self.0 as u16;
                if !half_is_nan(h) && !half_is_inf(h) {
                    return h;
                }
            }
        }
    }

    // Every half through both lane routes: the portable routine on its own, and
    // the block conversion at every position of a 37-wide block, so on AArch64
    // each half crosses the NEON lanes 0..31 and the portable tail 32..36.
    #[test]
    fn block_conversion_matches_per_element_for_every_half() {
        const W: usize = 37;
        let mut src = [0u8; 2 * W];
        let mut dst = [1.5f32; W];
        for h in 0..=u16::MAX {
            let want = half_to_float4(h).to_bits();
            assert_eq!(half_to_float4_portable(h).to_bits(), want, "portable, half {h:#06x}");
            for pos in 0..W {
                src.fill(0);
                src[2 * pos..2 * pos + 2].copy_from_slice(&h.to_ne_bytes());
                halves_to_floats(&src, &mut dst);
                assert_eq!(dst[pos].to_bits(), want, "block, half {h:#06x} at {pos}");
                assert!(
                    dst.iter().enumerate().all(|(i, f)| i == pos || f.to_bits() == 0),
                    "zeros elsewhere, half {h:#06x} at {pos}"
                );
            }
        }
    }

    // The source may start at any byte: the same 100 halves converted from an
    // even and from an odd offset of one buffer.
    #[test]
    fn block_conversion_is_alignment_free() {
        let mut r = XorShift(0x2545_F491_4F6C_DD1D);
        let halves: Vec<Half> = (0..100).map(|_| r.half()).collect();
        let bytes: Vec<u8> = halves.iter().flat_map(|h| h.to_ne_bytes()).collect();
        let mut shifted = vec![0u8; 1];
        shifted.extend_from_slice(&bytes);
        let (mut even, mut odd) = (vec![0f32; 100], vec![0f32; 100]);
        halves_to_floats(&bytes, &mut even);
        halves_to_floats(&shifted[1..], &mut odd);
        for i in 0..100 {
            assert_eq!(even[i].to_bits(), half_to_float4(halves[i]).to_bits());
            assert_eq!(odd[i].to_bits(), even[i].to_bits());
        }
    }

    // Nothing to convert, and a source exactly as long as the destination needs.
    #[test]
    fn block_conversion_of_nothing() {
        let mut none: [f32; 0] = [];
        halves_to_floats(&[], &mut none);
        halves_to_floats(&[1, 2, 3, 4], &mut none);
        let mut two = [9.0f32; 2];
        let mut bytes = [0u8; 4];
        bytes[..2].copy_from_slice(&0x3C00u16.to_ne_bytes());
        bytes[2..].copy_from_slice(&0x4000u16.to_ne_bytes());
        halves_to_floats(&bytes, &mut two);
        assert_eq!(two, [1.0, 2.0]);
    }

    // The slice-once kernels against the per-element form they replace, bit
    // for bit, over random finite halves of every magnitude.
    #[test]
    fn kernels_match_per_element_reference() {
        let hx = |x: &[u8], i: usize| half_to_float4(Half::from_ne_bytes([x[2 * i], x[2 * i + 1]]));
        let mut r = XorShift(0x9E37_79B9_7F4A_7C15);
        for n in 0..2000usize {
            let dim = 1 + n % 40;
            let a: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
            let b: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
            let (mut l2, mut ip, mut l1) = (0.0f32, 0.0f32, 0.0f32);
            let (mut sim, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..dim {
                let (x, y) = (hx(&a, i), hx(&b, i));
                let diff = x - y;
                l2 += diff * diff;
                ip += x * y;
                l1 += (x - y).abs();
                sim += x * y;
                na += x * x;
                nb += y * y;
            }
            let cos = sim as f64 / ((na as f64) * (nb as f64)).sqrt();
            assert_eq!(halfvec_l2_squared_distance(dim, &a, &b).to_bits(), l2.to_bits());
            assert_eq!(halfvec_inner_product(dim, &a, &b).to_bits(), ip.to_bits());
            assert_eq!(halfvec_l1_distance(dim, &a, &b).to_bits(), l1.to_bits());
            assert_eq!(halfvec_cosine_similarity(dim, &a, &b).to_bits(), cos.to_bits());
        }
    }

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

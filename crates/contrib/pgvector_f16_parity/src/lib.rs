//! Exhaustive parity of pgvector::halfutils with pgvector 0.8.7's C f16
//! code (spec §8.2, unit tier): every f16 and every f32 bit pattern through
//! the conversions, and every pair of halves through + - *, against the
//! routines build.rs compiles. Test-only: nothing depends on this crate.

#[cfg(all(test, pgv_c_half))]
mod tests {
    use pgvector::halfutils::{float4_to_half_unchecked, half_is_nan, half_to_float4, Half};

    extern "C" {
        fn pgv_sw_half_to_float4(h: u16) -> u32;
        fn pgv_sw_float4_to_half(bits: u32) -> u16;
        fn pgv_has_float16() -> i32;
        fn pgv_hw_half_to_float4(h: u16) -> u32;
        fn pgv_hw_float4_to_half(bits: u32) -> u16;
        fn pgv_hw_add(a: u16, b: u16) -> u16;
        fn pgv_hw_sub(a: u16, b: u16) -> u16;
        fn pgv_hw_mul(a: u16, b: u16) -> u16;
    }

    fn has_float16() -> bool {
        // SAFETY: pure C function, no preconditions.
        unsafe { pgv_has_float16() != 0 }
    }

    /// Sum of `check(lo, hi)` over 64 chunks of `0..n`, run in parallel.
    fn par_count(n: u64, check: impl Fn(u64, u64) -> u64 + Sync) -> u64 {
        const CHUNKS: u64 = 64;
        let step = n.div_ceil(CHUNKS);
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..CHUNKS)
                .map(|c| {
                    let check = &check;
                    s.spawn(move || check(c * step, ((c + 1) * step).min(n)))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).sum()
        })
    }

    #[test]
    fn f16_half_to_float4_matches_c_for_every_half() {
        let hw = has_float16();
        for h in 0..=u16::MAX {
            let ours = half_to_float4(h).to_bits();
            // SAFETY: pure C functions, no preconditions.
            assert_eq!(ours, unsafe { pgv_sw_half_to_float4(h) }, "software path, half {h:#06x}");
            if hw {
                // SAFETY: as above.
                assert_eq!(ours, unsafe { pgv_hw_half_to_float4(h) }, "_Float16, half {h:#06x}");
            }
        }
    }

    #[test]
    fn f16_float4_to_half_matches_c_for_every_float() {
        let hw = has_float16();
        let bad = par_count(1 << 32, |lo, hi| {
            let mut bad = 0;
            for bits in lo..hi {
                let bits = bits as u32;
                let ours = float4_to_half_unchecked(f32::from_bits(bits));
                // SAFETY: pure C functions, no preconditions.
                let sw = unsafe { pgv_sw_float4_to_half(bits) };
                let hw_bad = hw && ours != unsafe { pgv_hw_float4_to_half(bits) };
                if ours != sw || hw_bad {
                    bad += 1;
                }
            }
            bad
        });
        assert_eq!(bad, 0, "f32 inputs whose half differs from C");
    }

    // halfvec_add/sub/mul round the f32 result once (halfvec.c:786, 825,
    // 864); the FLT16_SUPPORT build computes in _Float16 (halfvec.c:784, 823,
    // 862). f32 carries >= 2*11+2 significand bits, so the two agree. NaN
    // operands, which CheckElement rejects before any arithmetic, may differ
    // only in payload.
    #[test]
    fn f16_arithmetic_matches_float16_for_every_pair() {
        if !has_float16() {
            eprintln!("skipped: the C compiler has no _Float16");
            return;
        }
        let bad = par_count(1 << 16, |lo, hi| {
            let mut bad = 0;
            for a in lo..hi {
                let a = a as Half;
                let fa = half_to_float4(a);
                for b in 0..=u16::MAX {
                    let fb = half_to_float4(b);
                    // SAFETY: pure C functions, no preconditions.
                    let pairs = unsafe {
                        [
                            (float4_to_half_unchecked(fa + fb), pgv_hw_add(a, b)),
                            (float4_to_half_unchecked(fa - fb), pgv_hw_sub(a, b)),
                            (float4_to_half_unchecked(fa * fb), pgv_hw_mul(a, b)),
                        ]
                    };
                    for (ours, c) in pairs {
                        if ours != c && !(half_is_nan(ours) && half_is_nan(c)) {
                            bad += 1;
                        }
                    }
                }
            }
            bad
        });
        assert_eq!(bad, 0, "operand pairs whose result differs from _Float16");
    }
}

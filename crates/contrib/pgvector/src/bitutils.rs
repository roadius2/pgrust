//! bitutils.c (pgvector 0.8.7): Hamming and Jaccard distance over bit
//! strings. Ports the portable `*Default` kernels (bitutils.c:49-131); the
//! AVX-512 dispatch (bitutils.c:75-160) computes the same integer counts.

// BitHammingDistanceDefault (bitutils.c:49-73): popcount of a XOR b, eight
// bytes at a time, then the tail bytes.
pub fn bit_hamming_distance(bytes: usize, ax: &[u8], bx: &[u8], mut distance: u64) -> u64 {
    let (mut ax, mut bx, mut bytes) = (ax, bx, bytes);
    while bytes >= 8 {
        let a = u64::from_ne_bytes(ax[..8].try_into().unwrap());
        let b = u64::from_ne_bytes(bx[..8].try_into().unwrap());
        distance += (a ^ b).count_ones() as u64;
        ax = &ax[8..];
        bx = &bx[8..];
        bytes -= 8;
    }
    for i in 0..bytes {
        distance += (ax[i] ^ bx[i]).count_ones() as u64;
    }
    distance
}

// BitJaccardDistanceDefault (bitutils.c:98-131).
pub fn bit_jaccard_distance(
    bytes: usize,
    ax: &[u8],
    bx: &[u8],
    mut ab: u64,
    mut aa: u64,
    mut bb: u64,
) -> f64 {
    let (mut ax, mut bx, mut bytes) = (ax, bx, bytes);
    while bytes >= 8 {
        let a = u64::from_ne_bytes(ax[..8].try_into().unwrap());
        let b = u64::from_ne_bytes(bx[..8].try_into().unwrap());
        ab += (a & b).count_ones() as u64;
        aa += a.count_ones() as u64;
        bb += b.count_ones() as u64;
        ax = &ax[8..];
        bx = &bx[8..];
        bytes -= 8;
    }
    for i in 0..bytes {
        ab += (ax[i] & bx[i]).count_ones() as u64;
        aa += ax[i].count_ones() as u64;
        bb += bx[i].count_ones() as u64;
    }
    if ab == 0 {
        1.0
    } else {
        1.0 - (ab as f64 / ((aa + bb - ab) as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // bit.out: '111' vs '110' differ in one bit; 17 bytes cross the
    // eight-byte loop and the tail.
    #[test]
    fn hamming_counts_differing_bits_across_words_and_tail() {
        assert_eq!(bit_hamming_distance(1, &[0b1110_0000], &[0b1100_0000], 0), 1);
        assert_eq!(bit_hamming_distance(17, &[0xFF; 17], &[0x0F; 17], 0), 17 * 4);
        assert_eq!(bit_hamming_distance(0, &[], &[], 0), 0);
    }

    // bit.out: jaccard_distance('1111', '1100') = 0.5; no common set bit
    // (including all-zero inputs) is distance 1 (bitutils.c:127-128).
    #[test]
    fn jaccard_matches_c_formula() {
        assert_eq!(bit_jaccard_distance(1, &[0b1111_0000], &[0b1100_0000], 0, 0, 0), 0.5);
        assert_eq!(bit_jaccard_distance(1, &[0], &[0], 0, 0, 0), 1.0);
        assert_eq!(bit_jaccard_distance(9, &[0xAA; 9], &[0x55; 9], 0, 0, 0), 1.0);
        assert_eq!(bit_jaccard_distance(9, &[0xAA; 9], &[0xAA; 9], 0, 0, 0), 0.0);
    }
}

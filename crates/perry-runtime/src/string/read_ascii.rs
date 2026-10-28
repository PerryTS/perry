//! ASCII admission for initialized native-input chunks. No byte outside the
//! supplied slice is read; a rejected chunk still uses the UTF-8 decoder.

#[inline]
pub(super) fn all_ascii(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // SAFETY: the CPU supplies AVX2; each load is bounded below.
        return unsafe { avx2(bytes) };
    }
    bytes.is_ascii()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn avx2(bytes: &[u8]) -> bool {
    use std::arch::x86_64::*;
    let mut blocks = bytes.chunks_exact(128);
    for block in &mut blocks {
        let p = block.as_ptr().cast::<__m256i>();
        // All four unaligned vectors are contained in this complete block.
        let a = _mm256_or_si256(_mm256_loadu_si256(p), _mm256_loadu_si256(p.add(1)));
        let b = _mm256_or_si256(_mm256_loadu_si256(p.add(2)), _mm256_loadu_si256(p.add(3)));
        if _mm256_movemask_epi8(_mm256_or_si256(a, b)) != 0 {
            return false;
        }
    }
    blocks.remainder().is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(bytes: &[u8]) {
        let expected = bytes.is_ascii();
        assert_eq!(all_ascii(bytes), expected);
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            assert_eq!(unsafe { avx2(bytes) }, expected);
        }
    }

    #[test]
    fn all_alignments_and_block_tails_match_scalar_ascii() {
        for offset in 0..32 {
            for len in [
                0, 1, 15, 16, 31, 32, 63, 64, 127, 128, 129, 255, 256, 257, 511,
            ] {
                // Non-ASCII sentinels surround the admitted initialized slice.
                let mut storage = vec![0xff; offset + len + 32];
                let bytes = &mut storage[offset..offset + len];
                bytes.fill(b'a');
                check(bytes);
                for at in 0..len {
                    for nonascii in [0x80, 0xff] {
                        bytes[at] = nonascii;
                        check(bytes);
                    }
                    bytes[at] = b'a';
                }
            }
        }
    }

    #[test]
    fn every_byte_class_and_empty_input_match_scalar_ascii() {
        check(&[]);
        for byte in 0..=u8::MAX {
            check(&[byte]);
            check(&[byte; 128]);
            check(&[byte; 129]);
        }
    }
}

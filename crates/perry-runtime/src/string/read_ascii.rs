//! ASCII admission for initialized native-input chunks. No byte outside the
//! supplied slice is read; a rejected chunk still uses the UTF-8 decoder.

#[inline]
pub(super) fn all_ascii(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw") {
        // SAFETY: both required CPU features are available.
        return unsafe { avx512(bytes) };
    }
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
    let (mut a, mut b, mut c, mut d) = (
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
        _mm256_setzero_si256(),
    );
    for block in &mut blocks {
        let p = block.as_ptr().cast::<__m256i>();
        // All four loads are inside a complete block. Independent reductions
        // avoid a serial sign-mask/exit dependency in each iteration.
        a = _mm256_or_si256(a, _mm256_loadu_si256(p));
        b = _mm256_or_si256(b, _mm256_loadu_si256(p.add(1)));
        c = _mm256_or_si256(c, _mm256_loadu_si256(p.add(2)));
        d = _mm256_or_si256(d, _mm256_loadu_si256(p.add(3)));
    }
    let bits = _mm256_or_si256(_mm256_or_si256(a, b), _mm256_or_si256(c, d));
    _mm256_movemask_epi8(bits) == 0 && blocks.remainder().is_ascii()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn avx512(bytes: &[u8]) -> bool {
    use std::arch::x86_64::*;
    let mut blocks = bytes.chunks_exact(256);
    let (mut a, mut b, mut c, mut d) = (
        _mm512_setzero_si512(),
        _mm512_setzero_si512(),
        _mm512_setzero_si512(),
        _mm512_setzero_si512(),
    );
    for block in &mut blocks {
        let p = block.as_ptr().cast::<__m512i>();
        // No load crosses this complete block or the initialized input slice.
        a = _mm512_or_si512(a, _mm512_loadu_si512(p));
        b = _mm512_or_si512(b, _mm512_loadu_si512(p.add(1)));
        c = _mm512_or_si512(c, _mm512_loadu_si512(p.add(2)));
        d = _mm512_or_si512(d, _mm512_loadu_si512(p.add(3)));
    }
    let bits = _mm512_or_si512(_mm512_or_si512(a, b), _mm512_or_si512(c, d));
    _mm512_movepi8_mask(bits) == 0 && blocks.remainder().is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(bytes: &[u8]) {
        let expected = bytes.is_ascii();
        assert_eq!(all_ascii(bytes), expected);
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw") {
            assert_eq!(unsafe { avx512(bytes) }, expected);
        }
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") {
            assert_eq!(unsafe { avx2(bytes) }, expected);
        }
    }

    #[test]
    fn all_alignments_and_block_tails_match_scalar_ascii() {
        for offset in 0..64 {
            for len in [
                0, 1, 15, 16, 31, 32, 63, 64, 127, 128, 129, 255, 256, 257, 511, 512, 513,
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

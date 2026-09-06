//! Bounded byte scanners shared by JSON parsing, depth checks and escaping.

/// Find a string terminator or an illegal unescaped control byte.
#[inline(always)]
pub(crate) fn find_string_terminator(bytes: &[u8]) -> Option<usize> {
    find_special::<true, false>(bytes)
}

/// Also stop at a potential WTF-8 surrogate so the escaper can inspect it.
#[inline(always)]
pub(crate) fn find_string_escape(bytes: &[u8]) -> Option<usize> {
    find_special::<true, true>(bytes)
}

/// The depth preflight is not a validator: only quotes and escapes affect
/// whether later brackets belong to a string. Raw controls must not end it.
#[inline(always)]
pub(crate) fn find_quote_or_backslash(bytes: &[u8]) -> Option<usize> {
    find_special::<false, false>(bytes)
}

#[inline(always)]
fn special<const CONTROL: bool, const SURROGATE: bool>(b: u8) -> bool {
    b == b'"' || b == b'\\' || (CONTROL && b < 0x20) || (SURROGATE && b == 0xED)
}

#[inline(always)]
fn find_special<const CONTROL: bool, const SURROGATE: bool>(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        find_neon::<CONTROL, SURROGATE>(bytes)
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        find_sse2::<CONTROL, SURROGATE>(bytes)
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse2")
    )))]
    {
        find_word::<CONTROL, SURROGATE>(bytes)
    }
}

/// Word-sized scan for short strings/tails and platforms without SIMD.
/// A hit is resolved bytewise: the zero-byte trick can borrow into a later
/// lane, so its mask must not be used directly as a first-byte index.
#[inline(always)]
fn find_word<const CONTROL: bool, const SURROGATE: bool>(bytes: &[u8]) -> Option<usize> {
    const LOW: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    #[inline(always)]
    fn has_zero(word: u64) -> bool {
        word.wrapping_sub(LOW) & !word & HIGH != 0
    }
    let mut i = 0;
    while i + 8 <= bytes.len() {
        // The slice check above covers the complete unaligned load.
        let word = unsafe { bytes.as_ptr().add(i).cast::<u64>().read_unaligned() };
        let hit = has_zero(word ^ 0x2222_2222_2222_2222)
            || has_zero(word ^ 0x5C5C_5C5C_5C5C_5C5C)
            || (CONTROL && word.wrapping_sub(0x2020_2020_2020_2020) & !word & HIGH != 0)
            || (SURROGATE && has_zero(word ^ 0xEDED_EDED_EDED_EDED));
        if hit {
            return bytes[i..i + 8]
                .iter()
                .position(|&b| special::<CONTROL, SURROGATE>(b))
                .map(|j| i + j);
        }
        i += 8;
    }
    bytes[i..]
        .iter()
        .position(|&b| special::<CONTROL, SURROGATE>(b))
        .map(|j| i + j)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn find_neon<const CONTROL: bool, const SURROGATE: bool>(bytes: &[u8]) -> Option<usize> {
    use std::arch::aarch64::*;
    unsafe {
        let quote = vdupq_n_u8(b'"');
        let bslash = vdupq_n_u8(b'\\');
        let mut i = 0;
        while i + 16 <= bytes.len() {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            let mut mask = vorrq_u8(vceqq_u8(chunk, quote), vceqq_u8(chunk, bslash));
            if CONTROL {
                mask = vorrq_u8(mask, vcltq_u8(chunk, vdupq_n_u8(0x20)));
            }
            if SURROGATE {
                mask = vorrq_u8(mask, vceqq_u8(chunk, vdupq_n_u8(0xED)));
            }
            if vmaxvq_u8(mask) != 0 {
                let mut lanes = [0u8; 16];
                vst1q_u8(lanes.as_mut_ptr(), mask);
                return lanes.iter().position(|&b| b != 0).map(|j| i + j);
            }
            i += 16;
        }
        find_word::<CONTROL, SURROGATE>(&bytes[i..]).map(|j| i + j)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
#[inline(always)]
fn find_sse2<const CONTROL: bool, const SURROGATE: bool>(bytes: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    unsafe {
        let quote = _mm_set1_epi8(b'"' as i8);
        let bslash = _mm_set1_epi8(b'\\' as i8);
        let mut i = 0;
        while i + 16 <= bytes.len() {
            let chunk = _mm_loadu_si128(bytes.as_ptr().add(i).cast());
            let mut mask =
                _mm_or_si128(_mm_cmpeq_epi8(chunk, quote), _mm_cmpeq_epi8(chunk, bslash));
            if CONTROL {
                // Unsigned comparison: non-ASCII UTF-8 bytes are not controls.
                mask = _mm_or_si128(
                    mask,
                    _mm_cmpeq_epi8(_mm_min_epu8(chunk, _mm_set1_epi8(0x1F)), chunk),
                );
            }
            if SURROGATE {
                mask = _mm_or_si128(mask, _mm_cmpeq_epi8(chunk, _mm_set1_epi8(0xEDu8 as i8)));
            }
            let bits = _mm_movemask_epi8(mask) as u32;
            if bits != 0 {
                return Some(i + bits.trailing_zeros() as usize);
            }
            i += 16;
        }
        find_word::<CONTROL, SURROGATE>(&bytes[i..]).map(|j| i + j)
    }
}

#[cfg(test)]
#[path = "simd_tests.rs"]
mod tests;

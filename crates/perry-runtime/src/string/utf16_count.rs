//! Allocation-free UTF-16 length counting for validated UTF-8.
//!
//! Every non-continuation byte starts one code point. A four-byte lead adds
//! the second UTF-16 unit of a surrogate pair. Classifying bytes independently
//! avoids decoding code points and permits vector boundaries inside a sequence.
//! The caller must still validate raw/WTF-8 bytes before constructing `&str`.

// Keep large-string validation and vector counting out of the inlined small
// allocation path. The compat validator fails early on malformed input, then
// the existing bounded WTF-8 counter preserves its original interpretation.
#[inline(never)]
pub(super) fn count_bytes(bytes: &[u8]) -> u32 {
    match simdutf8::compat::from_utf8(bytes) {
        Ok(s) => count(s) as u32,
        Err(_) => super::compute_utf16_len_wtf8(bytes),
    }
}

#[inline]
pub(super) fn count(s: &str) -> usize {
    if s.len() < 64 {
        return s.encode_utf16().count();
    }
    let bytes = s.as_bytes();
    let mut units = 0;
    let mut i = 0;

    #[cfg(target_arch = "aarch64")]
    unsafe {
        use std::arch::aarch64::*;
        while bytes.len() - i >= 64 {
            let mut counts = vdupq_n_u8(0);
            for offset in [0, 16, 32, 48] {
                // All four unaligned loads stay inside this 64-byte block.
                let v = vld1q_u8(bytes.as_ptr().add(i + offset));
                let lead = vcgeq_s8(vreinterpretq_s8_u8(v), vdupq_n_s8(-64));
                let four = vcgeq_u8(v, vdupq_n_u8(0xf0));
                // Comparison masks are 0xff for true. Subtraction turns them
                // into positive counts; each lane accumulates at most eight.
                counts = vsubq_u8(counts, vaddq_u8(lead, four));
            }
            units += vaddlvq_u8(counts) as usize;
            i += 64;
        }
    }

    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    unsafe {
        use std::arch::x86_64::*;
        let zero = _mm_setzero_si128();
        while bytes.len() - i >= 64 {
            let mut counts = zero;
            for offset in [0, 16, 32, 48] {
                let v = _mm_loadu_si128(bytes.as_ptr().add(i + offset).cast());
                let lead = _mm_cmpgt_epi8(v, _mm_set1_epi8(-65));
                let four = _mm_cmpeq_epi8(_mm_and_si128(v, _mm_set1_epi8(-16)), _mm_set1_epi8(-16));
                counts = _mm_sub_epi8(counts, _mm_add_epi8(lead, four));
            }
            let sums = _mm_sad_epu8(counts, zero);
            units += _mm_cvtsi128_si64(sums) as usize
                + _mm_cvtsi128_si64(_mm_srli_si128::<8>(sums)) as usize;
            i += 64;
        }
    }

    for &b in &bytes[i..] {
        units += usize::from((b as i8) >= -64) + usize::from(b >= 0xf0);
    }
    units
}

#[cfg(test)]
#[path = "utf16_count_tests.rs"]
mod tests;

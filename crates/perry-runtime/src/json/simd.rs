//! SIMD-accelerated string-terminator scanning used by the direct JSON parser.

/// Find the offset of the first `"` or `\` in `bytes`. Returns `None`
/// if neither is found before end-of-input (which is a JSON error — the
/// caller handles that by failing the parse).
///
/// Issue #179 tier 1 #3: SIMD-accelerated on aarch64 (NEON) and x86_64
/// (SSE2); scalar on other targets. The hot path on
/// `bench_json_roundtrip` — per-record string scanning — previously
/// ran one byte at a time in the tight zero-copy fast-path loop. 16-byte
/// SIMD chunks cut the per-iteration overhead substantially on long
/// records, and the scalar tail handles the trailing <16 bytes.
#[inline(always)]
pub(crate) fn find_string_terminator(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        find_string_terminator_neon(bytes)
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        return find_string_terminator_sse2(bytes);
    }
    #[cfg(not(any(
        target_arch = "aarch64",
        all(target_arch = "x86_64", target_feature = "sse2")
    )))]
    {
        find_string_terminator_scalar(bytes)
    }
}

/// Scalar fallback used on non-SIMD targets and as the tail handler
/// for the SIMD variants. Always inlined so the caller's tight loop
/// doesn't pay a call-site cost for the <16-byte tail.
#[inline(always)]
pub(crate) fn find_string_terminator_scalar(bytes: &[u8]) -> Option<usize> {
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' || b == b'\\' {
            return Some(i);
        }
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub(crate) fn find_string_terminator_neon(bytes: &[u8]) -> Option<usize> {
    use std::arch::aarch64::*;
    unsafe {
        let quote = vdupq_n_u8(b'"');
        let bslash = vdupq_n_u8(b'\\');
        let mut i: usize = 0;
        while i + 16 <= bytes.len() {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            let eq_q = vceqq_u8(chunk, quote);
            let eq_b = vceqq_u8(chunk, bslash);
            let mask = vorrq_u8(eq_q, eq_b);
            // Fast rejection: reduce the 16-byte mask to a single byte
            // (max across all lanes). Zero => no match in this chunk.
            if vmaxvq_u8(mask) == 0 {
                i += 16;
                continue;
            }
            // Hit somewhere in this chunk — scan the 16 bytes to find
            // the exact offset. Branchless via per-lane comparison.
            // `mask` has 0xFF at matching lane positions and 0x00
            // elsewhere; store-and-scan is portable and fast enough
            // for a 16-byte region.
            let mut lanes = [0u8; 16];
            vst1q_u8(lanes.as_mut_ptr(), mask);
            for (j, &lane) in lanes.iter().enumerate() {
                if lane != 0 {
                    return Some(i + j);
                }
            }
            // Unreachable — vmaxvq_u8 said there's a match.
            unreachable!();
        }
        // Tail: <16 bytes left, scalar scan.
        find_string_terminator_scalar(&bytes[i..]).map(|off| i + off)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
#[inline(always)]
pub(crate) fn find_string_terminator_sse2(bytes: &[u8]) -> Option<usize> {
    use std::arch::x86_64::*;
    unsafe {
        let quote = _mm_set1_epi8(b'"' as i8);
        let bslash = _mm_set1_epi8(b'\\' as i8);
        let mut i: usize = 0;
        while i + 16 <= bytes.len() {
            let chunk = _mm_loadu_si128(bytes.as_ptr().add(i) as *const _);
            let eq_q = _mm_cmpeq_epi8(chunk, quote);
            let eq_b = _mm_cmpeq_epi8(chunk, bslash);
            let mask = _mm_or_si128(eq_q, eq_b);
            let bitmask = _mm_movemask_epi8(mask) as u32;
            if bitmask != 0 {
                return Some(i + bitmask.trailing_zeros() as usize);
            }
            i += 16;
        }
        // Tail.
        find_string_terminator_scalar(&bytes[i..]).map(|off| i + off)
    }
}

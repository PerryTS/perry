//! One-pass UTF-16 counting without asserting Unicode validity.
//!
//! The scalar fallback skips a width determined only by each lead byte: two
//! for C0..DF, three for E0..EF and four for F0..FF. Continuation bytes at the
//! cursor are skipped. Therefore counting non-continuation bytes, plus one
//! extra unit for F0..FF, gives exactly the same result whenever every byte
//! skipped after a lead is a continuation. Stray continuations, overlong forms,
//! surrogates, out-of-range code points and truncated final sequences do not
//! invalidate this counting proof. A skipped non-continuation requires the
//! existing scalar interpretation instead.

/// Count byte-shaped UTF-16 units, preserving the runtime's bounded WTF-8
/// interpretation even for invalid UTF-8. Return None if a vector block contains
/// a non-continuation byte that the scalar walker would skip inside a sequence.
pub(super) fn count(bytes: &[u8]) -> Option<u32> {
    use std::arch::aarch64::*;
    let mut units = 0u32;
    let mut i = 0usize;
    unsafe {
        let mut previous_two = vdupq_n_u8(0);
        let mut previous_three = vdupq_n_u8(0);
        let mut previous_four = vdupq_n_u8(0);
        while bytes.len() - i >= 64 {
            let chunks = [
                vld1q_u8(bytes.as_ptr().add(i)),
                vld1q_u8(bytes.as_ptr().add(i + 16)),
                vld1q_u8(bytes.as_ptr().add(i + 32)),
                vld1q_u8(bytes.as_ptr().add(i + 48)),
            ];
            let combined = vorrq_u8(
                vorrq_u8(chunks[0], chunks[1]),
                vorrq_u8(chunks[2], chunks[3]),
            );
            if vmaxvq_u8(combined) < 0x80 {
                if i != 0 && (bytes[i - 1] >= 0xc0 || bytes[i - 2] >= 0xe0 || bytes[i - 3] >= 0xf0)
                {
                    return None;
                }
                previous_two = vdupq_n_u8(0);
                previous_three = vdupq_n_u8(0);
                previous_four = vdupq_n_u8(0);
                units += 64;
                i += 64;
                continue;
            }
            let mut counts = vdupq_n_u8(0);
            let mut invalid = vdupq_n_u8(0);
            for v in chunks {
                let non_cont = vcgeq_s8(vreinterpretq_s8_u8(v), vdupq_n_s8(-64));
                let two = vcgeq_u8(v, vdupq_n_u8(0xc0));
                let three = vcgeq_u8(v, vdupq_n_u8(0xe0));
                let four = vcgeq_u8(v, vdupq_n_u8(0xf0));
                let required = vorrq_u8(
                    vextq_u8::<15>(previous_two, two),
                    vorrq_u8(
                        vextq_u8::<14>(previous_three, three),
                        vextq_u8::<13>(previous_four, four),
                    ),
                );
                invalid = vorrq_u8(invalid, vandq_u8(required, non_cont));
                counts = vsubq_u8(counts, vaddq_u8(non_cont, four));
                previous_two = two;
                previous_three = three;
                previous_four = four;
            }
            if vmaxvq_u8(invalid) != 0 {
                return None;
            }
            units += vaddlvq_u8(counts) as u32;
            i += 64;
        }
    }
    // A lead in the last vector can claim up to three bytes in the scalar tail.
    // Stray continuation bytes and truncated sequences are permitted by the
    // legacy walker; only a claimed non-continuation would change its count.
    for distance in 1..=3 {
        if i < distance {
            break;
        }
        let b = bytes[i - distance];
        let width: usize = if b >= 0xf0 {
            4
        } else if b >= 0xe0 {
            3
        } else if b >= 0xc0 {
            2
        } else {
            0
        };
        let remaining = width.saturating_sub(distance);
        if bytes[i..].iter().take(remaining).any(|&b| (b as i8) >= -64) {
            return None;
        }
    }
    Some(units + super::super::compute_utf16_len_wtf8(&bytes[i..]))
}

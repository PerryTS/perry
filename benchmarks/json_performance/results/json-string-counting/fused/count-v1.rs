/// Count byte-shaped UTF-16 units, preserving the runtime's bounded WTF-8
/// interpretation even for invalid UTF-8. Return None if a vector block contains
/// a non-continuation byte that the scalar walker would skip inside a sequence.
pub fn count(bytes: &[u8]) -> Option<u32> {
    use std::arch::aarch64::*;
    let mut units = 0u32;
    let mut i = 0usize;
    unsafe {
        let mut previous_two = vdupq_n_u8(0);
        let mut previous_three = vdupq_n_u8(0);
        let mut previous_four = vdupq_n_u8(0);
        while bytes.len() - i >= 64 {
            let mut counts = vdupq_n_u8(0);
            let mut invalid = vdupq_n_u8(0);
            for offset in [0, 16, 32, 48] {
                let v = vld1q_u8(bytes.as_ptr().add(i + offset));
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
        let width: usize = if b >= 0xf0 { 4 } else if b >= 0xe0 { 3 } else if b >= 0xc0 { 2 } else { 0 };
        let remaining = width.saturating_sub(distance);
        if bytes[i..].iter().take(remaining).any(|&b| (b as i8) >= -64) {
            return None;
        }
    }
    Some(units + super::legacy(&bytes[i..]))
}

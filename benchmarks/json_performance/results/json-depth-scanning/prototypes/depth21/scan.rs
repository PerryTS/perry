#[inline(always)]
pub(crate) fn quoted_end(bytes: &[u8]) -> Option<usize> {
    let pos = crate::qscan::find_quote_or_backslash(bytes)?;
    if bytes[pos] == b'"' {
        Some(pos)
    } else {
        dense(bytes, pos)
    }
}

const ONES: u64 = 0x1111_1111_1111_1111;
const EVEN: u64 = 0x0101_0101_0101_0101;
const ODD: u64 = 0x1010_1010_1010_1010;

// Each input byte has a nibble: F for a match, zero otherwise. Adding one
// at the start of a run of backslash nibbles carries to its first non-slash.
// The opposite-parity endpoint identifies an odd-length run. A carry out of
// an odd-position run escapes byte zero in the next 16-byte window.
#[inline(always)]
pub(crate) fn unescaped(quotes: u64, mut slashes: u64, incoming: bool) -> (u64, bool) {
    if incoming {
        slashes &= !0xf;
    }
    let starts = slashes & !(slashes << 4) & ONES;
    let even_sum = slashes.wrapping_add(starts & EVEN);
    let (odd_sum, carry) = slashes.overflowing_add(starts & ODD);
    let escaped = (even_sum & ODD) | (odd_sum & EVEN) | u64::from(incoming);
    ((quotes & ONES) & !escaped, carry)
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn masks(ptr: *const u8) -> (u64, u64) {
    use std::arch::aarch64::*;
    let bytes = vld1q_u8(ptr);
    let quote = vceqq_u8(bytes, vdupq_n_u8(b'"'));
    let slash = vceqq_u8(bytes, vdupq_n_u8(b'\\'));
    let quote = vshrn_n_u16::<4>(vreinterpretq_u16_u8(quote));
    let slash = vshrn_n_u16::<4>(vreinterpretq_u16_u8(slash));
    (vget_lane_u64::<0>(vreinterpret_u64_u8(quote)),
     vget_lane_u64::<0>(vreinterpret_u64_u8(slash)))
}

#[inline(never)]
fn dense(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut incoming = false;
    #[cfg(target_arch = "aarch64")]
    while bytes.len() - pos >= 16 {
        let (quotes, slashes) = unsafe { masks(bytes.as_ptr().add(pos)) };
        if quotes | slashes == 0 {
            // An ordinary run clears a pending escaped byte. Keep the wider
            // existing scanner for long plain text following a sparse escape.
            pos += 16;
            incoming = false;
            pos += crate::qscan::find_quote_or_backslash(&bytes[pos..])?;
            if bytes[pos] == b'"' {
                return Some(pos);
            }
            continue;
        }
        let (quotes, carry) = unescaped(quotes, slashes, incoming);
        if quotes != 0 {
            return Some(pos + quotes.trailing_zeros() as usize / 4);
        }
        incoming = carry;
        pos += 16;
    }
    if incoming && pos < bytes.len() {
        pos += 1;
    }
    while pos < bytes.len() {
        if bytes[pos] == b'"' {
            return Some(pos);
        }
        pos += if bytes[pos] == b'\\' { 2 } else { 1 };
    }
    None
}

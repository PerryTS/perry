//! Allocation-free quoted-span scanning for the nesting preflight on ARM64.
//! This only tracks quotes and escaped bytes; syntax validation stays in the parser.

#[inline(always)]
pub(super) fn find_quote_or_backslash(bytes: &[u8]) -> Option<usize> {
    if bytes.len() >= 16 {
        use std::arch::aarch64::*;
        // The length check covers the complete unaligned load.
        unsafe {
            let chunk = vld1q_u8(bytes.as_ptr());
            let mask = vorrq_u8(
                vceqq_u8(chunk, vdupq_n_u8(b'"')),
                vceqq_u8(chunk, vdupq_n_u8(b'\\')),
            );
            if vmaxvq_u8(mask) != 0 {
                let mut lanes = [0u8; 16];
                vst1q_u8(lanes.as_mut_ptr(), mask);
                return lanes.iter().position(|&b| b != 0);
            }
            return long_run(&bytes[16..]).map(|n| n + 16);
        }
    }
    super::super::simd::find_quote_or_backslash(bytes)
}

#[inline(never)]
fn long_run(bytes: &[u8]) -> Option<usize> {
    unsafe {
        use std::arch::aarch64::*;
        let quote = vdupq_n_u8(b'"');
        let slash = vdupq_n_u8(b'\\');
        let mut pos = 0;
        while bytes.len() - pos >= 16 {
            // Both loop bounds cover every load, including all four lanes
            // of the ordinary 64-byte run below.
            let chunk = vld1q_u8(bytes.as_ptr().add(pos));
            let mask = vorrq_u8(vceqq_u8(chunk, quote), vceqq_u8(chunk, slash));
            let bits = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(
                vreinterpretq_u16_u8(mask),
            )));
            if bits != 0 {
                return Some(pos + bits.trailing_zeros() as usize / 4);
            }
            pos += 16;
            while bytes.len() - pos >= 64 {
                let a = vld1q_u8(bytes.as_ptr().add(pos));
                let b = vld1q_u8(bytes.as_ptr().add(pos + 16));
                let c = vld1q_u8(bytes.as_ptr().add(pos + 32));
                let d = vld1q_u8(bytes.as_ptr().add(pos + 48));
                let ma = vorrq_u8(vceqq_u8(a, quote), vceqq_u8(a, slash));
                let mb = vorrq_u8(vceqq_u8(b, quote), vceqq_u8(b, slash));
                let mc = vorrq_u8(vceqq_u8(c, quote), vceqq_u8(c, slash));
                let md = vorrq_u8(vceqq_u8(d, quote), vceqq_u8(d, slash));
                let mask = vorrq_u8(vorrq_u8(ma, mb), vorrq_u8(mc, md));
                if vmaxvq_u8(mask) != 0 {
                    break;
                }
                pos += 64;
            }
        }
        super::super::simd::find_quote_or_backslash(&bytes[pos..]).map(|n| pos + n)
    }
}

#[inline(always)]
pub(super) fn quoted_end(bytes: &[u8]) -> Option<usize> {
    let pos = find_quote_or_backslash(bytes)?;
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
pub(super) fn unescaped(quotes: u64, mut slashes: u64, incoming: bool) -> (u64, bool) {
    if incoming {
        // Byte zero was escaped in the previous window. It cannot start a
        // new run, even if it is another backslash.
        slashes &= !0xf;
    }
    let starts = slashes & !(slashes << 4) & ONES;
    let even_sum = slashes.wrapping_add(starts & EVEN);
    let (odd_sum, carry) = slashes.overflowing_add(starts & ODD);
    let escaped = (even_sum & ODD) | (odd_sum & EVEN) | u64::from(incoming);
    ((quotes & ONES) & !escaped, carry)
}

#[inline(always)]
// Caller must provide at least 16 readable bytes.
unsafe fn masks(ptr: *const u8) -> (u64, u64) {
    use std::arch::aarch64::*;
    let bytes = vld1q_u8(ptr);
    let quote = vceqq_u8(bytes, vdupq_n_u8(b'"'));
    let slash = vceqq_u8(bytes, vdupq_n_u8(b'\\'));
    let quote = vshrn_n_u16::<4>(vreinterpretq_u16_u8(quote));
    let slash = vshrn_n_u16::<4>(vreinterpretq_u16_u8(slash));
    (
        vget_lane_u64::<0>(vreinterpret_u64_u8(quote)),
        vget_lane_u64::<0>(vreinterpret_u64_u8(slash)),
    )
}

#[inline(never)]
fn dense(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut incoming = false;
    while bytes.len() - pos >= 16 {
        let (quotes, slashes) = unsafe { masks(bytes.as_ptr().add(pos)) };
        if quotes | slashes == 0 {
            // An ordinary run clears a pending escaped byte. Keep the bulk
            // scanner for long plain text following a sparse escape.
            pos += 16;
            incoming = false;
            pos += find_quote_or_backslash(&bytes[pos..])?;
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

//! JSON token scanning with a bounded WTF-8 surrogate-prefix check.
//! Excluding ED A0..BF lets borrowed tokens retain escape-free provenance.
//! A following byte is read only through the slice; Korean ED80..9F is ordinary.
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

#[inline(always)]
pub(super) fn scan(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        scan_neon(bytes)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        scalar(bytes)
    }
}

// Word probing keeps short ASCII tokens on the existing bounded word path.
// A non-surrogate ED match resumes after that byte without growing scratch.
#[inline(always)]
fn scalar(bytes: &[u8]) -> Option<usize> {
    let mut offset = 0;
    while let Some(hit) = super::find_word::<true, true>(&bytes[offset..]) {
        let i = offset + hit;
        if bytes[i] != 0xed || bytes.get(i + 1).is_some_and(|n| n & 0xe0 == 0xa0) {
            return Some(i);
        }
        offset = i + 1;
    }
    None
}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn mask(v: uint8x16_t, next: uint8x16_t) -> uint8x16_t {
    let ordinary = vorrq_u8(
        vorrq_u8(vceqq_u8(v, vdupq_n_u8(b'"')), vceqq_u8(v, vdupq_n_u8(0x5c))),
        vcltq_u8(v, vdupq_n_u8(32)),
    );
    let mid = vextq_u8::<1>(v, next);
    let surrogate = vandq_u8(
        vceqq_u8(v, vdupq_n_u8(0xed)),
        vceqq_u8(vandq_u8(mid, vdupq_n_u8(0xe0)), vdupq_n_u8(0xa0)),
    );
    vorrq_u8(ordinary, surrogate)
}
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn first(v: uint8x16_t) -> usize {
    let mut lanes = [0u8; 16];
    vst1q_u8(lanes.as_mut_ptr(), v);
    lanes.iter().position(|&b| b != 0).unwrap()
}
#[cfg(target_arch = "aarch64")]
#[inline(never)]
fn scan_neon(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 16 {
        return scalar(bytes);
    }
    unsafe {
        let v = vld1q_u8(bytes.as_ptr());
        let m = mask(v, vdupq_n_u8(bytes.get(16).copied().unwrap_or(0)));
        if vmaxvq_u8(m) != 0 {
            return Some(first(m));
        }
        let mut i = 16;
        while bytes.len() - i >= 64 {
            let a = vld1q_u8(bytes.as_ptr().add(i));
            let b = vld1q_u8(bytes.as_ptr().add(i + 16));
            let c = vld1q_u8(bytes.as_ptr().add(i + 32));
            let d = vld1q_u8(bytes.as_ptr().add(i + 48));
            let masks = [
                mask(a, b),
                mask(b, c),
                mask(c, d),
                mask(d, vdupq_n_u8(bytes.get(i + 64).copied().unwrap_or(0))),
            ];
            let combined = vorrq_u8(vorrq_u8(masks[0], masks[1]), vorrq_u8(masks[2], masks[3]));
            if vmaxvq_u8(combined) != 0 {
                for (k, m) in masks.into_iter().enumerate() {
                    if vmaxvq_u8(m) != 0 {
                        return Some(i + k * 16 + first(m));
                    }
                }
            }
            i += 64;
        }
        while bytes.len() - i >= 16 {
            let v = vld1q_u8(bytes.as_ptr().add(i));
            let m = mask(v, vdupq_n_u8(bytes.get(i + 16).copied().unwrap_or(0)));
            if vmaxvq_u8(m) != 0 {
                return Some(i + first(m));
            }
            i += 16;
        }
        scalar(&bytes[i..]).map(|n| i + n)
    }
}
#[cfg(test)]
#[path = "simd_json_string_tests.rs"]
mod tests;

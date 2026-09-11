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

// Each mask byte is either all zeroes or all ones. Extract one word at a
// time so locating a short token's first special byte does not branch once
// per preceding character. Surrogate lookahead is needed only on ED hits.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
fn scan_neon(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    unsafe {
        while bytes.len() - i >= 16 {
            let v = vld1q_u8(bytes.as_ptr().add(i));
            let special = vorrq_u8(
                vorrq_u8(vceqq_u8(v, vdupq_n_u8(b'"')), vceqq_u8(v, vdupq_n_u8(0x5c))),
                vorrq_u8(vcltq_u8(v, vdupq_n_u8(32)), vceqq_u8(v, vdupq_n_u8(0xed))),
            );
            if vmaxvq_u8(special) != 0 {
                let words = vreinterpretq_u64_u8(special);
                for (lane, word) in [
                    (0, u64::from_le(vgetq_lane_u64::<0>(words))),
                    (8, u64::from_le(vgetq_lane_u64::<1>(words))),
                ] {
                    let mut hits = word & 0x8080_8080_8080_8080;
                    while hits != 0 {
                        let hit = i + lane + hits.trailing_zeros() as usize / 8;
                        if bytes[hit] != 0xed
                            || bytes.get(hit + 1).is_some_and(|n| n & 0xe0 == 0xa0)
                        {
                            return Some(hit);
                        }
                        hits &= hits - 1;
                    }
                }
            }
            i += 16;
        }
    }
    scalar(&bytes[i..]).map(|n| i + n)
}
#[cfg(test)]
#[path = "simd_json_string_tests.rs"]
mod tests;

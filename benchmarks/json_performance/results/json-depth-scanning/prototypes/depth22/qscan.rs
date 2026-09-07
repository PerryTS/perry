#[inline(always)]
pub(crate) fn find_quote_or_backslash(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    if bytes.len() >= 16 {
        use std::arch::aarch64::*;
        unsafe {
            let chunk = vld1q_u8(bytes.as_ptr());
            let mask = vorrq_u8(vceqq_u8(chunk, vdupq_n_u8(b'"')), vceqq_u8(chunk, vdupq_n_u8(b'\\')));
            if vmaxvq_u8(mask) != 0 {
                let mut lanes = [0u8; 16];
                vst1q_u8(lanes.as_mut_ptr(), mask);
                return lanes.iter().position(|&b| b != 0);
            }
            return long_run(&bytes[16..]).map(|n| n + 16);
        }
    }
    crate::simd::find_quote_or_backslash(bytes)
}

#[inline(never)]
fn long_run(bytes: &[u8]) -> Option<usize> {
    #[cfg(target_arch = "aarch64")]
    unsafe {
        use std::arch::aarch64::*;
        let quote = vdupq_n_u8(b'"');
        let slash = vdupq_n_u8(b'\\');
        let mut pos = 0;
        while bytes.len() - pos >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr().add(pos));
            let mask = vorrq_u8(vceqq_u8(chunk, quote), vceqq_u8(chunk, slash));
            let bits = vget_lane_u64::<0>(vreinterpret_u64_u8(vshrn_n_u16::<4>(vreinterpretq_u16_u8(mask))));
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
        return crate::simd::find_quote_or_backslash(&bytes[pos..]).map(|n| pos + n);
    }
    #[cfg(not(target_arch = "aarch64"))]
    crate::simd::find_quote_or_backslash(bytes)
}

#[inline(always)]
fn old(bytes: &[u8]) -> bool {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    unsafe {
        let array = vdupq_n_u8(b'[');
        let object = vdupq_n_u8(b'{');
        while bytes.len() - i >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            if vmaxvq_u8(vorrq_u8(vceqq_u8(chunk, array), vceqq_u8(chunk, object))) != 0 {
                return true;
            }
            i += 16;
        }
    }
    bytes[i..].iter().any(|&b| matches!(b, b'[' | b'{'))
}

#[inline(always)]
fn contains_open_container(bytes: &[u8]) -> bool {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    unsafe {
        // '[' and '{' differ only by bit 0x20. No other byte folds to '{'.
        let fold = vdupq_n_u8(0x20);
        let object = vdupq_n_u8(b'{');
        // Preserve early admission for arrays whose first record opens here.
        if bytes.len() >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr());
            if vmaxvq_u8(vceqq_u8(vorrq_u8(chunk, fold), object)) != 0 {
                return true;
            }
            i = 16;
        }
        while bytes.len() - i >= 64 {
            let p = bytes.as_ptr().add(i);
            let a = vceqq_u8(vorrq_u8(vld1q_u8(p), fold), object);
            let b = vceqq_u8(vorrq_u8(vld1q_u8(p.add(16)), fold), object);
            let c = vceqq_u8(vorrq_u8(vld1q_u8(p.add(32)), fold), object);
            let d = vceqq_u8(vorrq_u8(vld1q_u8(p.add(48)), fold), object);
            if vmaxvq_u8(vorrq_u8(vorrq_u8(a, b), vorrq_u8(c, d))) != 0 {
                return true;
            }
            i += 64;
        }
        while bytes.len() - i >= 16 {
            let chunk = vld1q_u8(bytes.as_ptr().add(i));
            if vmaxvq_u8(vceqq_u8(vorrq_u8(chunk, fold), object)) != 0 {
                return true;
            }
            i += 16;
        }
    }
    bytes[i..].iter().any(|&b| matches!(b, b'[' | b'{'))
}

#[test]
fn each_byte_and_vector_boundary_matches_original() {
    for len in 0..260 {
        for byte in 0..=255u8 {
            let mut bytes = vec![byte; len + 16];
            for offset in 0..16 {
                let slice = &bytes[offset..offset + len];
                assert_eq!(old(slice), contains_open_container(slice), "{len} {byte} {offset}");
            }
            for pos in 0..len {
                bytes[pos] = b'{';
                assert_eq!(old(&bytes[..len]), contains_open_container(&bytes[..len]));
                bytes[pos] = b'[';
                assert_eq!(old(&bytes[..len]), contains_open_container(&bytes[..len]));
                bytes[pos] = byte;
            }
        }
    }
}
#[test]
fn pseudorandom_buffers_match_original() {
    let mut seed = 10034u64;
    for len in [15,16,17,63,64,65,79,80,81,255,256,257,1024,4096] {
        for _ in 0..1000 {
            let bytes: Vec<u8> = (0..len).map(|_| { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed as u8 }).collect();
            assert_eq!(old(&bytes), contains_open_container(&bytes));
        }
    }
}

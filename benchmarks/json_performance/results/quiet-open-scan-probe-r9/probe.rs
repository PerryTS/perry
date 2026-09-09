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


#[no_mangle]
pub unsafe extern "C" fn scan_old(data: *const u8, len: usize, count: usize) -> usize {
    let bytes = std::slice::from_raw_parts(data, len);
    let mut sum = 0;
    for _ in 0..count { sum += std::hint::black_box(old(std::hint::black_box(bytes))) as usize; }
    sum
}
#[no_mangle]
pub unsafe extern "C" fn scan_new(data: *const u8, len: usize, count: usize) -> usize {
    let bytes = std::slice::from_raw_parts(data, len);
    let mut sum = 0;
    for _ in 0..count { sum += std::hint::black_box(contains_open_container(std::hint::black_box(bytes))) as usize; }
    sum
}

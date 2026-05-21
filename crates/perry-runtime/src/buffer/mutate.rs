use super::*;

/// `crypto.getRandomValues(buf)` — fill an existing buffer with random
/// bytes in-place. Returns the same buffer pointer.
#[no_mangle]
pub extern "C" fn js_buffer_fill_random(buf_ptr: f64) -> f64 {
    use rand::RngCore;
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() {
        return buf_ptr;
    }
    unsafe {
        let len = (*buf).length as usize;
        let data = buffer_data_mut(buf);
        let bytes = std::slice::from_raw_parts_mut(data, len);
        rand::thread_rng().fill_bytes(bytes);
    }
    buf_ptr
}

/// `buf.swap16()` — pairs of bytes are swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap16(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() {
        return;
    }
    unsafe {
        let len = (*buf).length as usize;
        if !len.is_multiple_of(2) {
            return;
        }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(2) {
            let a = *data.add(i);
            *data.add(i) = *data.add(i + 1);
            *data.add(i + 1) = a;
        }
    }
}

/// `buf.swap32()` — groups of 4 bytes byte-swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap32(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() {
        return;
    }
    unsafe {
        let len = (*buf).length as usize;
        if !len.is_multiple_of(4) {
            return;
        }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(4) {
            let b0 = *data.add(i);
            let b1 = *data.add(i + 1);
            let b2 = *data.add(i + 2);
            let b3 = *data.add(i + 3);
            *data.add(i) = b3;
            *data.add(i + 1) = b2;
            *data.add(i + 2) = b1;
            *data.add(i + 3) = b0;
        }
    }
}

/// `buf.swap64()` — groups of 8 bytes byte-swapped in-place.
#[no_mangle]
pub extern "C" fn js_buffer_swap64(buf_ptr: f64) {
    let buf = unbox_buffer_ptr(buf_ptr.to_bits()) as *mut BufferHeader;
    if buf.is_null() {
        return;
    }
    unsafe {
        let len = (*buf).length as usize;
        if !len.is_multiple_of(8) {
            return;
        }
        let data = buffer_data_mut(buf);
        for i in (0..len).step_by(8) {
            for j in 0..4 {
                let a = *data.add(i + j);
                *data.add(i + j) = *data.add(i + 7 - j);
                *data.add(i + 7 - j) = a;
            }
        }
    }
}

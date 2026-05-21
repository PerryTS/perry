use super::*;

fn throw_invalid_buffer_size() -> ! {
    static REGISTER_RANGE_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_RANGE_ERROR.call_once(|| {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_RANGE_ERROR);
    });
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_RANGE_ERROR, 4);
    unsafe {
        let set = |key: &[u8], value: f64| {
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key_ptr, value);
        };
        let str_val = |s: &[u8]| -> f64 {
            let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
            f64::from_bits(crate::JSValue::string_ptr(ptr).bits())
        };
        set(b"name", str_val(b"RangeError"));
        set(b"code", str_val(b"ERR_INVALID_BUFFER_SIZE"));
        set(
            b"message",
            str_val(b"Buffer size must be a multiple of the requested word size"),
        );
    }
    crate::exception::js_throw(crate::value::js_nanbox_pointer(obj as i64))
}

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
            throw_invalid_buffer_size();
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
            throw_invalid_buffer_size();
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
            throw_invalid_buffer_size();
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

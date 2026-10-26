fn throw_invalid_buffer_size() -> ! {
    static REGISTER_RANGE_ERROR: std::sync::Once = std::sync::Once::new();
    REGISTER_RANGE_ERROR.call_once(|| {
        crate::object::js_register_class_extends_error(crate::error::CLASS_ID_RANGE_ERROR);
    });
    let obj = crate::object::js_object_alloc(crate::error::CLASS_ID_RANGE_ERROR, 4);
    {
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

/// Reverse each word without holding a byte borrow across error construction.
fn swap_words(value: f64, width: usize) {
    let result = super::bytes::no_gc(|scope| unsafe {
        let Ok(bytes) = super::bytes::bytes_mut(value, scope) else {
            return true;
        };
        if !bytes.len().is_multiple_of(width) {
            return false;
        }
        for word in bytes.chunks_exact_mut(width) {
            word.reverse();
        }
        true
    });
    if !result {
        throw_invalid_buffer_size();
    }
}

#[no_mangle]
pub extern "C" fn js_buffer_swap16(value: f64) {
    swap_words(value, 2);
}
#[no_mangle]
pub extern "C" fn js_buffer_swap32(value: f64) {
    swap_words(value, 4);
}
#[no_mangle]
pub extern "C" fn js_buffer_swap64(value: f64) {
    swap_words(value, 8);
}

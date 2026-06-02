use crate::JSValue;

fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn invalid_key(value: f64) -> ! {
    let message = format!(
        "The \"key\" argument must be an instance of CryptoKey. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(super) unsafe fn key_object_from(value: f64) -> f64 {
    let addr = value_addr(value);
    let Some((_algo, _hash, kind, _extractable, _usages)) = crate::buffer::crypto_key_meta(addr)
    else {
        invalid_key(value);
    };
    if kind != 1 || !crate::buffer::is_registered_buffer(addr) {
        invalid_key(value);
    }

    let src = addr as *const crate::buffer::BufferHeader;
    let len = (*src).length as usize;
    let out = crate::buffer::buffer_alloc(len as u32);
    if !out.is_null() {
        std::ptr::copy_nonoverlapping(
            crate::buffer::buffer_data(src),
            crate::buffer::buffer_data_mut(out),
            len,
        );
        (*out).length = len as u32;
        crate::buffer::mark_as_uint8array(out as usize);
        crate::buffer::mark_as_secret_key(out as usize);
    }
    f64::from_bits(JSValue::pointer(out as *const u8).bits())
}

use super::*;

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_sync(
    size_bits: f64,
    options_bits: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let size = match nanboxed_to_usize(size_bits) {
        Some(s) => s,
        None => return alloc_buffer_from_slice(&[]),
    };
    let options_raw = options_bits.to_bits();
    let safe = object_field_bool(options_raw, b"safe").unwrap_or(false);
    let add = object_field_u128(options_raw, b"add");
    let rem = object_field_u128(options_raw, b"rem");
    let Some(prime) = generate_prime_u128(size, safe, add, rem) else {
        return alloc_buffer_from_slice(&[]);
    };
    alloc_buffer_from_slice(&prime_to_be_bytes(prime, size))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_sync(
    candidate_bits: f64,
    _options_bits: f64,
) -> f64 {
    let bytes = crypto_value_bytes(candidate_bits);
    let Some(n) = bytes_to_u128(&bytes) else {
        return js_bool(false);
    };
    js_bool(is_prime_u128(n))
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_prime_async(
    size_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let buf = js_crypto_generate_prime_sync(size_bits, options_bits);
    let value = if buf.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(buf as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_check_prime_async(
    candidate_bits: f64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let result = js_crypto_check_prime_sync(candidate_bits, options_bits);
    call_node_style_callback2(
        callback_bits,
        f64::from_bits(JSValue::null().bits()),
        result,
    );
    f64::from_bits(JSValue::undefined().bits())
}

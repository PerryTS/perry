use super::*;

/// Check if an object is a Buffer (using the buffer registry)
#[no_mangle]
pub extern "C" fn js_buffer_is_buffer(ptr: i64) -> i32 {
    if ptr == 0 || (ptr as u64) < 0x1000 {
        return 0;
    }
    // Strip NaN-boxing tags if present
    let addr = if ((ptr as u64) >> 48) != 0 {
        (ptr as u64) & 0x0000_FFFF_FFFF_FFFF
    } else {
        ptr as u64
    };
    if is_registered_buffer(addr as usize) {
        1
    } else {
        0
    }
}

/// Check if a value is a Node Buffer encoding name.
#[no_mangle]
pub extern "C" fn js_buffer_is_encoding(value: f64) -> i32 {
    let str_ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        if len == 0 || len > 32 {
            return 0;
        }
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);
        fn eq_ascii_lower(a: &[u8], b: &[u8]) -> bool {
            if a.len() != b.len() {
                return false;
            }
            a.iter()
                .zip(b.iter())
                .all(|(x, y)| x.to_ascii_lowercase() == *y)
        }
        let ok = eq_ascii_lower(bytes, b"utf8")
            || eq_ascii_lower(bytes, b"utf-8")
            || eq_ascii_lower(bytes, b"hex")
            || eq_ascii_lower(bytes, b"base64")
            || eq_ascii_lower(bytes, b"base64url")
            || eq_ascii_lower(bytes, b"ascii")
            || eq_ascii_lower(bytes, b"latin1")
            || eq_ascii_lower(bytes, b"binary")
            || eq_ascii_lower(bytes, b"ucs2")
            || eq_ascii_lower(bytes, b"ucs-2")
            || eq_ascii_lower(bytes, b"utf16le")
            || eq_ascii_lower(bytes, b"utf-16le");
        ok as i32
    }
}

fn native_buffer_from_value(value: f64) -> Option<*const BufferHeader> {
    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);
    let raw_ptr = if jsval.is_pointer() || jsval.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && bits >= 0x1000 && bits < 0x0001_0000_0000_0000 {
        bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        Some(raw_ptr as *const BufferHeader)
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn js_native_buffer_data_ptr(value: f64) -> *const u8 {
    native_buffer_from_value(value)
        .map(buffer_data)
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_native_buffer_byte_len(value: f64) -> usize {
    native_buffer_from_value(value)
        .map(|buf| unsafe { (*buf).length as usize })
        .unwrap_or(0)
}

fn value_bytes(value: f64) -> Option<&'static [u8]> {
    if let Some(buf) = native_buffer_from_value(value) {
        return unsafe {
            Some(std::slice::from_raw_parts(
                buffer_data(buf),
                (*buf).length as usize,
            ))
        };
    }
    None
}

#[no_mangle]
pub extern "C" fn js_buffer_is_ascii(value: f64) -> f64 {
    let ok = value_bytes(value)
        .map(|bytes| bytes.iter().all(|b| *b <= 0x7f))
        .unwrap_or(false);
    f64::from_bits(crate::JSValue::bool(ok).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_is_utf8(value: f64) -> f64 {
    let ok = value_bytes(value)
        .map(|bytes| std::str::from_utf8(bytes).is_ok())
        .unwrap_or(false);
    f64::from_bits(crate::JSValue::bool(ok).bits())
}

/// Get the byte length of a string (when encoded to UTF-8)
#[no_mangle]
pub extern "C" fn js_buffer_byte_length(str_ptr: *const StringHeader) -> i32 {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }
    unsafe { (*str_ptr).byte_len as i32 }
}

/// Node-style `Buffer.byteLength(value, encoding?)`.
#[no_mangle]
pub extern "C" fn js_buffer_byte_length_value(value: f64, encoding: f64) -> i32 {
    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    let raw_ptr = if jsval.is_pointer() || jsval.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && bits >= 0x1000 && bits < 0x0001_0000_0000_0000 {
        bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        return unsafe { (*(raw_ptr as *const BufferHeader)).length as i32 };
    }

    let str_ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return 0;
    }

    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data_ptr, len);

        let enc_ptr = crate::value::js_get_string_pointer_unified(encoding) as *const StringHeader;
        let enc_bytes = if enc_ptr.is_null() || (enc_ptr as usize) < 0x1000 {
            &[][..]
        } else {
            let enc_len = (*enc_ptr).byte_len as usize;
            let enc_data = (enc_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            std::slice::from_raw_parts(enc_data, enc_len)
        };
        fn eq_ascii_lower(a: &[u8], b: &[u8]) -> bool {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(x, y)| x.to_ascii_lowercase() == *y)
        }

        if eq_ascii_lower(enc_bytes, b"hex") {
            return (len / 2) as i32;
        }
        if eq_ascii_lower(enc_bytes, b"base64") || eq_ascii_lower(enc_bytes, b"base64url") {
            // Node's Buffer.byteLength assumes valid base64/base64url input
            // instead of decoding. It only discounts up to two trailing `=`
            // padding characters, then applies the 3/4 base64 ratio.
            let mut code_units = std::str::from_utf8(bytes)
                .map(|s| s.encode_utf16().count())
                .unwrap_or(len);
            if code_units > 0 && bytes.last() == Some(&b'=') {
                code_units -= 1;
                if code_units > 1 && bytes.get(bytes.len().saturating_sub(2)) == Some(&b'=') {
                    code_units -= 1;
                }
            }
            return ((code_units * 3) / 4) as i32;
        }
        if eq_ascii_lower(enc_bytes, b"ascii")
            || eq_ascii_lower(enc_bytes, b"latin1")
            || eq_ascii_lower(enc_bytes, b"binary")
        {
            // Node's Buffer.byteLength(str, 'ascii'|'latin1'|'binary') returns
            // the input string's UTF-16 code-unit length (one byte per unit
            // after the encoding's `& 0xFF` truncation). For astral chars this
            // is 2, not 1 — so use `encode_utf16().count()`, not `chars().count()`.
            return std::str::from_utf8(bytes)
                .map(|s| s.encode_utf16().count() as i32)
                .unwrap_or(len as i32);
        }
        if eq_ascii_lower(enc_bytes, b"ucs2")
            || eq_ascii_lower(enc_bytes, b"ucs-2")
            || eq_ascii_lower(enc_bytes, b"utf16le")
            || eq_ascii_lower(enc_bytes, b"utf-16le")
        {
            return std::str::from_utf8(bytes)
                .map(|s| (s.encode_utf16().count() * 2) as i32)
                .unwrap_or((len * 2) as i32);
        }
        len as i32
    }
}

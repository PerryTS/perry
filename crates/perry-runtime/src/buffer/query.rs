use super::*;

/// FFI helper: return a pointer to the raw bytes of a `Buffer` or
/// `TypedArray` value (NaN-boxed bits passed as `f64`), writing the byte
/// length through `out_len`. Returns null (and sets `*out_len = 0`) for any
/// value that is neither a Buffer nor a TypedArray.
///
/// Used by out-of-crate FFI callers (`perry-ext-http-server`'s
/// `getUnpackedSettings`) that must read a *program-allocated* Buffer's
/// bytes. Going through this extern symbol ensures the Buffer-registry
/// lookup runs in the same runtime copy that allocated the Buffer (via the
/// `js_buffer_alloc` extern), avoiding the staticlib thread-local
/// divergence that direct `is_registered_buffer` calls from an ext crate
/// would hit.
///
/// # Safety
/// `out_len` must be a valid writable `*mut u32` or null. The returned
/// pointer borrows the live allocation and is valid only until the next GC.
#[no_mangle]
pub unsafe extern "C" fn js_value_buffer_or_typedarray_data(
    bits: f64,
    out_len: *mut u32,
) -> *const u8 {
    if !out_len.is_null() {
        *out_len = 0;
    }
    let raw = bits.to_bits();
    // Buffer? (registry lookup via the canonical extern dispatch)
    if js_buffer_is_buffer(raw as i64) == 1 {
        let addr = if (raw >> 48) != 0 {
            raw & 0x0000_FFFF_FFFF_FFFF
        } else {
            raw
        } as usize;
        let buf = addr as *const BufferHeader;
        if !buf.is_null() {
            if !out_len.is_null() {
                *out_len = (*buf).length;
            }
            // #6515: resolve a registered view to its backing window so native
            // consumers see the bytes JS reads/writes, not the stale local copy.
            return resolve_span_data_ptr(buf);
        }
    }
    // TypedArray? (Uint8Array etc. backing bytes)
    let addr = if (raw >> 48) >= 0x7FF8 {
        (raw & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        raw as usize
    };
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        let ta = addr as *const crate::typedarray::TypedArrayHeader;
        if let Some(bytes) = crate::typedarray::typed_array_bytes(ta) {
            if !out_len.is_null() {
                *out_len = bytes.len() as u32;
            }
            return bytes.as_ptr();
        }
    }
    std::ptr::null()
}

// Referenced only from the prebuilt `perry-ext-http-server` archive, so the
// auto-optimize LTO pass would otherwise dead-strip it. Pin it.
#[used]
static KEEP_JS_VALUE_BUFFER_OR_TYPEDARRAY_DATA: unsafe extern "C" fn(f64, *mut u32) -> *const u8 =
    js_value_buffer_or_typedarray_data;

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

fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    }
}

fn native_buffer_from_value(value: f64) -> Option<*const BufferHeader> {
    let raw_ptr = raw_addr_from_value(value);
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        Some(raw_ptr as *const BufferHeader)
    } else {
        None
    }
}

#[no_mangle]
pub extern "C" fn js_native_buffer_data_ptr(value: f64) -> *const u8 {
    // #6515: a Uint8Array view over an ArrayBuffer must expose its backing
    // window here, not its stale local copy (see `view::resolve_data_ptr`).
    native_buffer_from_value(value)
        .map(|buf| unsafe { resolve_span_data_ptr(buf) })
        .unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_native_buffer_byte_len(value: f64) -> usize {
    native_buffer_from_value(value)
        .map(|buf| unsafe { (*buf).length as usize })
        .unwrap_or(0)
}

fn describe_binary_input(value: f64) -> String {
    let addr = raw_addr_from_value(value);
    if addr != 0 && is_data_view(addr) {
        return "an instance of DataView".to_string();
    }
    crate::fs::validate::describe_received(value)
}

fn throw_invalid_binary_input(value: f64) -> ! {
    let msg = format!(
        "The \"input\" argument must be an instance of ArrayBuffer, Buffer, or TypedArray. Received {}",
        describe_binary_input(value)
    );
    crate::fs::validate::throw_type_error_with_code(&msg, "ERR_INVALID_ARG_TYPE")
}

fn value_bytes(value: f64) -> &'static [u8] {
    let addr = raw_addr_from_value(value);
    if addr != 0 && is_data_view(addr) {
        throw_invalid_binary_input(value);
    }
    if let Some(buf) = native_buffer_from_value(value) {
        return unsafe { std::slice::from_raw_parts(buffer_data(buf), (*buf).length as usize) };
    }
    if addr != 0 && crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return unsafe {
            crate::typedarray::typed_array_bytes(addr as *const crate::typedarray::TypedArrayHeader)
                .unwrap_or_else(|| throw_invalid_binary_input(value))
        };
    }
    throw_invalid_binary_input(value)
}

#[no_mangle]
pub extern "C" fn js_buffer_is_ascii(value: f64) -> f64 {
    let ok = value_bytes(value).iter().all(|b| *b <= 0x7f);
    f64::from_bits(crate::JSValue::bool(ok).bits())
}

#[no_mangle]
pub extern "C" fn js_buffer_is_utf8(value: f64) -> f64 {
    let ok = std::str::from_utf8(value_bytes(value)).is_ok();
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
    // #2013: reject a non string/Buffer/ArrayBuffer/TypedArray first argument
    // with `ERR_INVALID_ARG_TYPE`, matching Node.
    super::validate::validate_byte_length_arg(value);
    let bits = value.to_bits();
    let jsval = crate::JSValue::from_bits(bits);

    let raw_ptr = if jsval.is_pointer() || jsval.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
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

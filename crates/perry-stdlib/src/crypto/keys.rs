use super::*;

/// `crypto.createSecretKey(key, encoding?)` — produce a key Buffer
/// that jose / jsonwebtoken / etc. can use as an HS* signing key.
///
/// In Node's surface this returns a `KeyObject`, but for the V8-fallback
/// JWT path that's where Perry's JS shim lives. From native Perry the
/// shortest correct value is a `BufferHeader` of the raw key bytes,
/// marked as a `Uint8Array` so that:
///   - jose's `key instanceof Uint8Array` check passes after the native
///     -> V8 marshal turns the BufferHeader into a real `v8::Uint8Array`
///     (`bridge.rs:native_object_to_v8`),
///   - `instanceof KeyObject` is not required for HS* algorithms (jose
///     accepts Uint8Array directly per `getSignVerifyKey`).
///
/// `key_ptr` may point at a Buffer (already bytes) or a StringHeader
/// (utf8 string literal). The `encoding` arg honors Node/Buffer string
/// decoding semantics (#2954): `hex`/`base64`/`base64url` are lenient (hex
/// stops at the first invalid/incomplete pair, base64 ignores noise),
/// `latin1`/`ascii`/`utf16le`/`ucs2` affect the bytes, and an unknown
/// encoding name throws `TypeError [ERR_UNKNOWN_ENCODING]` — mirroring
/// `Buffer.from(string, encoding)`.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_create_secret_key(
    key_ptr: i64,
    encoding_ptr: i64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let raw = bytes_from_ptr(key_ptr);
    let bytes = if encoding_ptr >= 0x1000 {
        // The `encoding` arg is passed as a raw StringHeader pointer (i64),
        // not NaN-boxed, by the codegen call site. Read its name directly.
        let name = String::from_utf8(bytes_from_ptr(encoding_ptr))
            .unwrap_or_default()
            .to_ascii_lowercase();
        match encoding_tag_from_name(&name) {
            Some(tag) => decode_string_bytes_with_tag(&raw, tag),
            // Unknown encoding name → Node throws ERR_UNKNOWN_ENCODING.
            None => throw_unknown_encoding(&name),
        }
    } else {
        // No encoding arg: the input is a Buffer (already bytes) or a utf8
        // string literal — pass the raw bytes through unchanged.
        raw
    };
    let buf = alloc_buffer_from_slice(&bytes);
    if !buf.is_null() {
        // Mark as Uint8Array so `instanceof Uint8Array` works, both in
        // perry-native code and after the bridge materializes a v8
        // Uint8Array on the V8 side.
        perry_runtime::buffer::mark_as_uint8array(buf as usize);
        perry_runtime::buffer::mark_as_secret_key(buf as usize);
    }
    buf
}

/// `crypto.generateKeySync("aes"|"hmac", { length })` -> secret KeyObject.
///
/// Node reports secret keys in bytes; AES requires 128/192/256-bit lengths
/// while HMAC accepts bit lengths and truncates to whole bytes.
#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_sync(
    alg_ptr: i64,
    options_bits: f64,
) -> *mut perry_runtime::buffer::BufferHeader {
    let alg = String::from_utf8(bytes_from_ptr(alg_ptr))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let length_bits = match object_field_bits(options_bits.to_bits(), b"length") {
        Some(bits) => bits,
        None => return std::ptr::null_mut(),
    };
    let length = f64::from_bits(length_bits) as i32;
    let byte_len = match alg.as_str() {
        "aes" => match length {
            128 => 16,
            192 => 24,
            256 => 32,
            _ => return std::ptr::null_mut(),
        },
        "hmac" if length >= 8 => (length / 8) as usize,
        _ => return std::ptr::null_mut(),
    };
    let mut bytes = vec![0u8; byte_len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let buf = alloc_buffer_from_slice(&bytes);
    if !buf.is_null() {
        perry_runtime::buffer::mark_as_uint8array(buf as usize);
        perry_runtime::buffer::mark_as_secret_key(buf as usize);
    }
    buf
}

pub(super) unsafe fn call_node_style_callback2(callback_bits: f64, err: f64, value: f64) {
    let raw = callback_bits.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    if raw < 0x1000 {
        return;
    }
    perry_runtime::closure::js_closure_call2(
        raw as *const perry_runtime::ClosureHeader,
        err,
        value,
    );
}

pub(super) unsafe fn call_node_style_callback3(callback_bits: f64, err: f64, a: f64, b: f64) {
    let raw = callback_bits.to_bits() & 0x0000_FFFF_FFFF_FFFF;
    if raw < 0x1000 {
        return;
    }
    perry_runtime::closure::js_closure_call3(raw as *const perry_runtime::ClosureHeader, err, a, b);
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_async(
    alg_ptr: i64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let key = js_crypto_generate_key_sync(alg_ptr, options_bits);
    let value = if key.is_null() {
        f64::from_bits(JSValue::undefined().bits())
    } else {
        f64::from_bits(JSValue::pointer(key as *const u8).bits())
    };
    call_node_style_callback2(callback_bits, f64::from_bits(JSValue::null().bits()), value);
    f64::from_bits(JSValue::undefined().bits())
}

#[no_mangle]
pub unsafe extern "C" fn js_crypto_generate_key_pair_async(
    alg_ptr: i64,
    options_bits: f64,
    callback_bits: f64,
) -> f64 {
    let alg = string_from_header(alg_ptr as *const StringHeader).unwrap_or_default();
    let pair = match alg.as_slice() {
        b"ec" => js_crypto_generate_key_pair_sync_ec_p256(options_bits),
        b"ed25519" => js_crypto_generate_key_pair_sync_ed25519(options_bits),
        b"x25519" => js_crypto_generate_key_pair_sync_x25519(options_bits),
        _ => js_crypto_generate_key_pair_sync_rsa(options_bits),
    };
    let null = f64::from_bits(JSValue::null().bits());
    let undefined = f64::from_bits(JSValue::undefined().bits());
    if pair.is_null() {
        call_node_style_callback3(callback_bits, null, undefined, undefined);
        return undefined;
    }
    let public_key =
        js_object_get_field_by_name(pair, js_string_from_bytes(b"publicKey".as_ptr(), 9));
    let private_key =
        js_object_get_field_by_name(pair, js_string_from_bytes(b"privateKey".as_ptr(), 10));
    call_node_style_callback3(
        callback_bits,
        null,
        f64::from_bits(public_key.bits()),
        f64::from_bits(private_key.bits()),
    );
    undefined
}

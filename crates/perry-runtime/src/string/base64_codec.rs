//! `atob` / `btoa` — base64 codec entry points.
//!
//! Filename is `base64_codec.rs` (not `base64.rs`) to avoid shadowing the
//! external `base64` crate.

use super::*;

fn value_to_string_bytes(value: f64) -> &'static [u8] {
    let str_ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(string_data(str_ptr), (*str_ptr).byte_len as usize) }
}

fn throw_invalid_character() -> ! {
    let msg = b"The string to be decoded is not correctly encoded.";
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_error_new_with_name_message(b"InvalidCharacterError", msg_ptr);
    crate::node_submodules::set_error_user_prop(err as usize, "code", 5.0);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn is_base64_alphabet(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')
}

/// atob(base64) — decode a base64-encoded string to a binary string.
/// Output is a raw *const StringHeader (codegen NaN-boxes).
#[no_mangle]
pub extern "C" fn js_atob(value: f64) -> *const StringHeader {
    use base64::Engine as _;

    let mut cleaned = Vec::new();
    for &byte in value_to_string_bytes(value) {
        if !matches!(byte, b'\t' | b'\n' | 0x0c | b'\r' | b' ') {
            cleaned.push(byte);
        }
    }
    if cleaned.len() % 4 == 0 {
        if cleaned.ends_with(b"==") {
            cleaned.truncate(cleaned.len() - 2);
        } else if cleaned.ends_with(b"=") {
            cleaned.truncate(cleaned.len() - 1);
        }
    }
    if cleaned.len() % 4 == 1 || cleaned.iter().any(|&byte| !is_base64_alphabet(byte)) {
        throw_invalid_character();
    }
    match base64::engine::general_purpose::STANDARD_NO_PAD.decode(&cleaned) {
        Ok(decoded) => js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32),
        Err(_) => throw_invalid_character(),
    }
}

/// btoa(string) — base64-encode a binary string.
#[no_mangle]
pub extern "C" fn js_btoa(value: f64) -> *const StringHeader {
    use base64::Engine as _;

    let input = value_to_string_bytes(value);
    let mut bytes = Vec::with_capacity(input.len());
    match std::str::from_utf8(input) {
        Ok(s) => {
            for ch in s.chars() {
                let code = ch as u32;
                if code > 0xff {
                    throw_invalid_character();
                }
                bytes.push(code as u8);
            }
        }
        Err(_) => bytes.extend_from_slice(input),
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

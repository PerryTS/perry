//! `atob` / `btoa` — base64 codec entry points.
//!
//! Filename is `base64_codec.rs` (not `base64.rs`) to avoid shadowing the
//! external `base64` crate.

use super::*;

/// atob(base64) — decode a base64-encoded string to a binary string.
/// Input is a NaN-boxed STRING_TAG f64. Output is a raw *const StringHeader (codegen NaN-boxes).
#[no_mangle]
pub extern "C" fn js_atob(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    match base64::engine::general_purpose::STANDARD.decode(s.as_bytes()) {
        Ok(decoded) => js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32),
        Err(_) => js_string_from_bytes(ptr::null(), 0),
    }
}

/// btoa(string) — base64-encode a binary string.
#[no_mangle]
pub extern "C" fn js_btoa(value: f64) -> *const StringHeader {
    use base64::Engine as _;
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != STRING_TAG {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
    if !is_valid_string_ptr(str_ptr) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let s = string_as_str(str_ptr);
    let encoded = base64::engine::general_purpose::STANDARD.encode(s.as_bytes());
    js_string_from_bytes(encoded.as_ptr(), encoded.len() as u32)
}

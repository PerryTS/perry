//! NanoID module (nanoid compatible)
//!
//! Native implementation of the 'nanoid' npm package.
//! Generates short, URL-friendly unique IDs.

use nanoid::nanoid;
use perry_runtime::{js_string_from_bytes, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Generate a nanoid with default settings (21 chars, URL-safe alphabet)
/// nanoid() -> string
#[no_mangle]
pub extern "C" fn js_nanoid() -> *mut StringHeader {
    let id = nanoid!();
    js_string_from_bytes(id.as_ptr(), id.len() as u32)
}

/// Generate a nanoid with custom length
/// nanoid(size) -> string
#[no_mangle]
pub extern "C" fn js_nanoid_sized(size: f64) -> *mut StringHeader {
    let size = size as usize;
    if size == 0 {
        return js_nanoid();
    }
    let id = nanoid!(size);
    js_string_from_bytes(id.as_ptr(), id.len() as u32)
}

/// Generate a nanoid with custom alphabet and size
/// customAlphabet(alphabet, size)() -> string
/// For simplicity, we combine this into one call: nanoid.custom(alphabet, size)
#[no_mangle]
pub unsafe extern "C" fn js_nanoid_custom(
    alphabet_ptr: *const StringHeader,
    size: f64,
) -> *mut StringHeader {
    let alphabet = match string_from_header(alphabet_ptr) {
        Some(a) => a,
        None => return js_nanoid(),
    };

    let size = if size <= 0.0 { 21 } else { size as usize };
    let alphabet_chars: Vec<char> = alphabet.chars().collect();

    if alphabet_chars.is_empty() {
        return js_nanoid();
    }

    // Generate ID using custom alphabet
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let id: String = (0..size)
        .map(|_| {
            let idx = rng.gen_range(0..alphabet_chars.len());
            alphabet_chars[idx]
        })
        .collect();

    js_string_from_bytes(id.as_ptr(), id.len() as u32)
}

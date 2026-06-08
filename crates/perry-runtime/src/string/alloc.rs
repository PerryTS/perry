//! Heap allocation, SSO construction, and basic accessors for `StringHeader`.

use super::*;

/// Create a string from raw bytes
/// Returns a pointer to StringHeader
#[no_mangle]
pub extern "C" fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(data, len, len)
}

/// Materialize an inline SSO value into a heap `StringHeader`.
/// For call sites that need a real `*mut StringHeader` pointer and
/// can't easily be migrated to use `str_bytes_from_jsvalue`. No-op
/// for values already backed by a heap string.
///
/// Allocation implication: this defeats the SSO win for the
/// materialized value, so use sparingly — only as a last-resort
/// compatibility shim on paths that truly need the heap
/// representation.
#[no_mangle]
pub extern "C" fn js_string_materialize_to_heap(value: f64) -> *mut StringHeader {
    let bits = value.to_bits();
    let jsval = crate::value::JSValue::from_bits(bits);
    if jsval.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut buf);
        return js_string_from_bytes(buf.as_ptr(), n as u32);
    }
    if jsval.is_string() {
        return jsval.as_string_ptr() as *mut StringHeader;
    }
    std::ptr::null_mut()
}

/// SSO-aware string construction. Returns a NaN-boxed `JSValue` as
/// raw f64 bits. When `len <= SHORT_STRING_MAX_LEN` the result is
/// an inline `SHORT_STRING_TAG` value with no heap allocation;
/// otherwise falls back to `js_string_from_bytes` + `STRING_TAG`.
///
/// Tier 1 #2 entry point. Callers that NaN-box the result anyway
/// (as opposed to dereferencing the raw `StringHeader` pointer)
/// can migrate to this function without changing their downstream
/// consumers — as long as those consumers use
/// `JSValue::is_any_string()` + branch on `is_short_string()`
/// vs `is_string()` to decode. Call sites that need a
/// `*mut StringHeader` unconditionally should stay on
/// `js_string_from_bytes` for now.
#[no_mangle]
pub extern "C" fn js_string_new_sso(data: *const u8, len: u32) -> f64 {
    unsafe {
        let ulen = len as usize;
        // SSO stores its length tag as the JS `.length`, which is only valid
        // when byte length == UTF-16 length — i.e. pure ASCII. Non-ASCII (incl.
        // WTF-8 lone surrogates) must take the heap path so `compute_utf16_len`
        // records the correct code-unit count (#4793).
        if ulen <= crate::value::SHORT_STRING_MAX_LEN && (ulen == 0 || !data.is_null()) {
            let bytes = std::slice::from_raw_parts(data, ulen);
            if bytes.iter().all(|&b| b < 0x80) {
                if let Some(v) = crate::value::JSValue::try_short_string(bytes) {
                    return f64::from_bits(v.bits());
                }
            }
        }
        let ptr = js_string_from_bytes(data, len);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}

/// Create a string from raw bytes in the **longlived arena** (issue #179).
/// Intended for cache-resident strings that explicit root scanners keep
/// alive for the program's lifetime (`PARSE_KEY_CACHE` interned keys,
/// shape-cache `keys_array` string elements). Allocating these in a
/// dedicated arena prevents them from anchoring general-arena blocks
/// where per-iteration parse output is co-located, breaking the
/// block-persistence cascade documented in the issue.
///
/// Same layout and wire format as `js_string_from_bytes` — only the
/// backing arena differs.
#[no_mangle]
pub extern "C" fn js_string_from_bytes_longlived(data: *const u8, len: u32) -> *mut StringHeader {
    let (ptr, data_ptr) = string_storage_alloc_longlived(len);
    unsafe {
        let u16len = compute_utf16_len(data, len);
        init_string_header(ptr, u16len, len, len, 0, 0);
        if len > 0 && !data.is_null() {
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }
    ptr
}

/// Create a string from raw bytes with extra capacity for future appending
#[no_mangle]
pub extern "C" fn js_string_from_bytes_with_capacity(
    data: *const u8,
    len: u32,
    capacity: u32,
) -> *mut StringHeader {
    let capacity = capacity.max(len); // Ensure capacity >= len
    let (ptr, data_ptr) = string_storage_alloc(capacity);

    unsafe {
        let u16len = compute_utf16_len(data, len);
        // shared by default — caller can set refcount to 1 if uniquely owned.
        init_string_header(ptr, u16len, len, capacity, 0, 0);

        // Copy string data after header
        if len > 0 && !data.is_null() {
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }

    ptr
}

/// Create a StringHeader from WTF-8 bytes (may contain lone-surrogate sequences).
/// Sets STRING_FLAG_HAS_LONE_SURROGATES so isWellFormed()/toWellFormed() work correctly.
#[no_mangle]
pub extern "C" fn js_string_from_wtf8_bytes(data: *const u8, len: u32) -> *mut StringHeader {
    let (ptr, data_ptr) = string_storage_alloc(len);
    unsafe {
        let bytes = if len > 0 && !data.is_null() {
            slice::from_raw_parts(data, len as usize)
        } else {
            &[]
        };
        let u16len = compute_utf16_len_wtf8(bytes);
        init_string_header(ptr, u16len, len, len, 0, STRING_FLAG_HAS_LONE_SURROGATES);
        if len > 0 && !data.is_null() {
            ptr::copy_nonoverlapping(data, data_ptr, len as usize);
        }
    }
    ptr
}

/// Create an empty string with initial capacity (for building strings)
#[no_mangle]
pub extern "C" fn js_string_builder_new(initial_capacity: u32) -> *mut StringHeader {
    js_string_from_bytes_with_capacity(ptr::null(), 0, initial_capacity.max(16))
}

/// Mark a string as shared (refcount=0) so `js_string_append` won't mutate it in-place.
/// Called by codegen when a string pointer is copied to another variable (`let y = x`),
/// passed as a function argument, or stored into an array/object.
/// This is a NaN-boxed f64 input — extract the raw pointer first.
#[no_mangle]
pub extern "C" fn js_string_addref(s: *mut StringHeader) {
    if is_valid_string_ptr(s as *const StringHeader) {
        unsafe {
            (*s).refcount = 0; // Mark as shared — prevent in-place mutation
        }
    }
}

/// Get string length in UTF-16 code units (JS `.length` semantics)
#[no_mangle]
pub extern "C" fn js_string_length(s: *const StringHeader) -> u32 {
    if !is_valid_string_ptr(s) {
        return 0;
    }
    unsafe { (*s).utf16_len }
}

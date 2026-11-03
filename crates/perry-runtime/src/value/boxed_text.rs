//! Shared boxed string and object-field conversions.
use crate::{js_string_from_bytes, ObjectHeader};

// ---------------------------------------------------------------------------
// Shared helpers used across the URL sub-modules. Promoted to `pub(crate)` so
// the sibling modules (which `use super::*`) can reach them without going
// back through the public re-export surface.
// ---------------------------------------------------------------------------

/// Create a string from a Rust str (returns a StringHeader pointer as f64)
/// Uses proper NaN-boxing with STRING_TAG so is_string() will return true
pub(crate) fn create_string_f64(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    // Use js_nanbox_string to properly tag the string pointer
    crate::value::js_nanbox_string(ptr as i64)
}

/// Get string content from a NaN-boxed StringHeader pointer (passed as f64)
#[inline]
pub(crate) fn get_string_content(ptr_f64: f64) -> String {
    let jsval = crate::value::JSValue::from_bits(ptr_f64.to_bits());
    // Heap string first: the URL helpers call this once per stored field, and
    // this arm is exactly the pre-#11519 body.
    if jsval.is_string() {
        let ptr = jsval.as_string_ptr();
        if unsafe { crate::value::addr_class::try_read_string_header(ptr) }.is_none() {
            return String::new();
        }
        unsafe {
            let len = (*ptr).byte_len as usize;
            let slice = std::slice::from_raw_parts(crate::string::string_data(ptr), len);
            return String::from_utf8_lossy(slice).into_owned();
        }
    }
    if jsval.is_short_string() {
        return short_string_content(jsval);
    }
    String::new()
}

/// The inline-SSO arm of [`get_string_content`] (#11519): a short string built
/// at runtime (`new URLSearchParams("a" + "=1")`, `url.parse(String(n))`) used
/// to read as "" because only the heap tag was unboxed. Out of line so the
/// heap path stays as small as it was.
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) fn short_string_content(jsval: crate::value::JSValue) -> String {
    let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let n = jsval.short_string_to_buf(&mut buf);
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

pub(crate) fn string_from_header(ptr: *mut crate::StringHeader) -> String {
    if unsafe { crate::value::addr_class::try_read_string_header(ptr) }.is_none() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    }
}

pub(crate) fn object_from_f64(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) == 0x7FFD_0000_0000_0000 {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader;
        if !ptr.is_null() {
            return Some(ptr);
        }
    }
    None
}

pub(crate) fn object_prop_string(obj: *mut ObjectHeader, key: &str) -> String {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let val = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    get_string_content(val)
}

pub(crate) fn object_prop_f64(obj: *mut ObjectHeader, key: &str) -> f64 {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_get_field_by_name_f64(obj, key_ptr)
}

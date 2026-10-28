//! Shared JSON-compatible string quoting. This is a primitive operation used
//! by received-value descriptions as well as the JSON serializer.
use super::byte_scan as simd;
use super::short_copy as stringify_copy;
use crate::{js_string_from_bytes, JSValue, StringHeader};
use escaping::write_escaped_string;
pub(crate) mod escaping;
pub(crate) mod stringify_escaped_output;
pub(crate) mod stringify_string;

// Keep the shared implementation separate from the exported JSON facade so
// LTO cannot give ordinary error formatting the facade's subsystem identity.
#[inline(never)]
pub(crate) unsafe fn quote_header(str_ptr: *const StringHeader) -> *mut StringHeader {
    if let Some(ptr) =
        stringify_string::try_heap_string(JSValue::string_ptr(str_ptr as *mut _).bits())
    {
        return ptr;
    }
    if !crate::value::addr_class::is_plausible_heap_addr(str_ptr as usize) {
        return std::ptr::null_mut();
    }
    let len = (*str_ptr).byte_len as usize;
    let mut buf = String::with_capacity(len + 16);
    {
        let bytes = std::slice::from_raw_parts(crate::string::string_data(str_ptr), len);
        // Runtime strings may contain lone UTF-16 surrogates encoded as WTF-8.
        // Only validated UTF-8 may become &str; the byte emitter validates WTF-8
        // before quoting it. Finish the owned copy before managed allocation.
        match std::str::from_utf8(bytes) {
            Ok(s) => write_escaped_string(&mut buf, s),
            Err(_) => {
                if !escaping::write_wtf8_key(&mut buf, bytes) {
                    return std::ptr::null_mut();
                }
            }
        }
    }
    js_string_from_bytes(buf.as_ptr(), buf.len() as u32)
}

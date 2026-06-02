//! JSON serialization surface — extern declaration of perry-runtime's
//! `js_json_stringify` plus a safe wrapper.
//!
//! # Why
//!
//! Wrappers that take JSValue NaN-boxed args at the FFI boundary
//! (passed as `f64` to satisfy the SysV AMD64 ABI — perry-codegen's
//! `NA_F64` coercion) often need to convert those values into a
//! string representation. The mongodb wrapper's `*_value` variants
//! are the canonical case: user code calls `collection.insertOne(doc)`
//! where `doc` is a JS object literal, and the wrapper serializes it
//! to BSON via JSON-stringify → `serde_json` → `bson::Document`.
//!
//! Today's surface is intentionally minimal — just the umbrella
//! `js_json_stringify` (`type_hint = 0` = auto-detect from
//! NaN-boxing tags). The typed variants (`js_json_stringify_string`
//! / `_number` / `_bool` / `_null`) wait until a real wrapper
//! demands them.

use crate::{JsValue, StringHeader};

extern "C" {
    /// Serialize a NaN-boxed `JsValue` to a JSON string.
    ///
    /// `type_hint` is a perry-codegen optimization — pass `0` to
    /// auto-detect the value's type from its NaN-boxing tag. Returns
    /// null on serialization failure (deeply nested cycles, etc.) or
    /// a fresh `StringHeader` allocated in the perry runtime arena.
    fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
}

/// Safe wrapper around perry-runtime's `js_json_stringify`. Returns
/// `None` on a null pointer (serialization error) or invalid UTF-8
/// (shouldn't happen for `JSON.stringify` output but the read path
/// is strict).
///
/// ```ignore
/// let value = JsValue::from_string_ptr(alloc_string("hello").as_raw());
/// let json = perry_ffi::json_stringify(value).unwrap();
/// assert_eq!(json, "\"hello\"");
/// ```
pub fn json_stringify(value: JsValue) -> Option<String> {
    // SAFETY: `js_json_stringify` accepts any NaN-boxed JsValue —
    // it's the umbrella entry the runtime exposes. We pass the
    // bits as f64 to match the extern signature.
    let ptr = unsafe { js_json_stringify(f64::from_bits(value.bits()), 0) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: a non-null return is a valid runtime-allocated
    // StringHeader. Read its bytes and validate UTF-8.
    unsafe {
        let header = &*ptr;
        let len = header.byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).ok().map(String::from)
    }
}

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;
    use crate::alloc_string;

    #[test]
    fn stringify_string() {
        let v = JsValue::from_string_ptr(alloc_string("hello").as_raw());
        let json = json_stringify(v).expect("stringify");
        assert_eq!(json, "\"hello\"");
    }

    #[test]
    fn stringify_number() {
        let v = JsValue::from_number(42.0);
        let json = json_stringify(v).expect("stringify");
        assert_eq!(json, "42");
    }

    #[test]
    fn stringify_null() {
        let json = json_stringify(JsValue::NULL).expect("stringify");
        assert_eq!(json, "null");
    }

    #[test]
    fn stringify_bool() {
        let json_true = json_stringify(JsValue::TRUE).expect("stringify");
        let json_false = json_stringify(JsValue::FALSE).expect("stringify");
        assert_eq!(json_true, "true");
        assert_eq!(json_false, "false");
    }
}

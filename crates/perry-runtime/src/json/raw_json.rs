//! `JSON.rawJSON(text)` / `JSON.isRawJSON(value)` (#2900).
//!
//! `JSON.rawJSON(text)` validates `text` as a single JSON *scalar* (number,
//! string, `true`, `false`, or `null` — no surrounding whitespace, no object
//! or array) and returns a wrapper object that `JSON.stringify` emits
//! verbatim. The wrapper carries:
//!   - a `rawJSON` own property holding the original text (Node exposes this
//!     so `wrapper.rawJSON === text`), and
//!   - a reserved `class_id` (`RAW_JSON_CLASS_ID`) so detection survives GC
//!     moves without a pointer side-table.
//!
//! `JSON.isRawJSON(value)` returns `true` iff `value` is such a wrapper.
//!
//! The stringify traversal (`stringify_value` / `stringify_value_depth`)
//! checks `class_id == RAW_JSON_CLASS_ID` before the generic object path and
//! writes the stored text directly into the output buffer.

use crate::{js_string_from_bytes, JSValue, ObjectHeader, StringHeader};

/// Reserved class id stamped onto raw-JSON wrapper objects. Kept distinct
/// from every other reserved id used by `instanceof` / typeof so it can't be
/// confused with a real user class or built-in.
pub(crate) const RAW_JSON_CLASS_ID: u32 = 0xFFFF_00A0;

/// True if `text` is an acceptable argument to `JSON.rawJSON` per Node: a
/// single JSON scalar with no leading/trailing whitespace, and not an object
/// or array. Empty strings and bare identifiers (`NaN`, `undefined`) are
/// rejected.
fn is_valid_raw_json_text(text: &str) -> bool {
    // Node rejects any surrounding whitespace and empty input.
    if text.is_empty() || text != text.trim() {
        return false;
    }
    // Object / array literals are rejected — only scalars are allowed.
    let first = text.as_bytes()[0];
    if first == b'{' || first == b'[' {
        return false;
    }
    // Must be valid JSON that parses to a scalar.
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Object(_)) | Ok(serde_json::Value::Array(_)) => false,
        Ok(_) => true,
        Err(_) => false,
    }
}

/// If `value` is a raw-JSON wrapper, return its stored text bytes.
pub(crate) unsafe fn raw_json_text_bytes(ptr: *const u8) -> Option<&'static [u8]> {
    let obj = ptr as *const ObjectHeader;
    if obj.is_null() || crate::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }
    if (*obj).class_id != RAW_JSON_CLASS_ID {
        return None;
    }
    let key = raw_json_key();
    let val = crate::object::js_object_get_field_by_name_f64(obj, key);
    let bits = val.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == crate::value::STRING_TAG {
        let str_ptr = (bits & crate::value::POINTER_MASK) as *const StringHeader;
        if str_ptr.is_null() {
            return None;
        }
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return Some(std::slice::from_raw_parts(data, len));
    }
    None
}

/// Whether the value at `ptr` is a raw-JSON wrapper object (used by stringify
/// and `js_json_is_raw_json`).
pub(crate) unsafe fn ptr_is_raw_json_wrapper(ptr: *const u8) -> bool {
    let obj = ptr as *const ObjectHeader;
    !obj.is_null()
        && !crate::value::addr_class::is_handle_band(ptr as usize)
        && (*obj).class_id == RAW_JSON_CLASS_ID
}

/// Cached `"rawJSON"` key string used for the wrapper's own property.
fn raw_json_key() -> *const StringHeader {
    use std::cell::Cell;
    thread_local! {
        static KEY: Cell<*mut StringHeader> = const { Cell::new(std::ptr::null_mut()) };
    }
    KEY.with(|c| {
        let p = c.get();
        if !p.is_null() {
            return p as *const StringHeader;
        }
        let np = js_string_from_bytes(b"rawJSON".as_ptr(), 7);
        c.set(np);
        np as *const StringHeader
    })
}

/// `JSON.rawJSON(text)` — validate and wrap. Throws `SyntaxError` for invalid
/// input (non-scalar, surrounding whitespace, empty, etc.).
///
/// Returns a NaN-boxed pointer to the wrapper object.
#[no_mangle]
pub unsafe extern "C" fn js_json_raw_json(text: f64) -> f64 {
    let bits = text.to_bits();
    let s: String = {
        let tag = bits & 0xFFFF_0000_0000_0000;
        if tag == crate::value::STRING_TAG {
            let str_ptr = (bits & crate::value::POINTER_MASK) as *const StringHeader;
            super::stringify_api::string_from_header(str_ptr).unwrap_or_default()
        } else if tag == crate::value::SHORT_STRING_TAG {
            let jv = JSValue::from_bits(bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jv.short_string_to_buf(&mut scratch);
            String::from_utf8_lossy(&scratch[..n]).into_owned()
        } else {
            // Non-string argument: Node coerces via ToString. Fall back to the
            // runtime string-conversion so e.g. a number argument behaves like
            // its decimal text. (Most call sites pass a string literal.)
            let sp = crate::js_jsvalue_to_string(text);
            super::stringify_api::string_from_header(sp).unwrap_or_default()
        }
    };

    if !is_valid_raw_json_text(&s) {
        throw_raw_json_syntax_error();
    }

    let obj = crate::object::js_object_alloc_null_proto(RAW_JSON_CLASS_ID, 1);
    let text_str = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    let key = raw_json_key();
    crate::object::js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(JSValue::string_ptr(text_str).bits()),
    );
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

/// `JSON.isRawJSON(value)` — returns a NaN-boxed boolean.
#[no_mangle]
pub unsafe extern "C" fn js_json_is_raw_json(value: f64) -> f64 {
    let true_val = f64::from_bits(crate::value::TAG_TRUE);
    let false_val = f64::from_bits(crate::value::TAG_FALSE);
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // Only POINTER_TAG heap objects can be wrappers.
    if tag != crate::value::POINTER_TAG {
        return false_val;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as *const u8;
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return false_val;
    }
    if ptr_is_raw_json_wrapper(ptr) {
        true_val
    } else {
        false_val
    }
}

#[cold]
fn throw_raw_json_syntax_error() -> ! {
    let msg = b"Invalid value for JSON.rawJSON";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_syntaxerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

// Keepalive anchors: these `#[no_mangle]` entry points are called only from
// generated `.o`; the auto-optimize whole-program bitcode rebuild would
// otherwise dead-strip them (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_RAW_JSON: unsafe extern "C" fn(f64) -> f64 = js_json_raw_json;
#[used]
static KEEP_IS_RAW_JSON: unsafe extern "C" fn(f64) -> f64 = js_json_is_raw_json;

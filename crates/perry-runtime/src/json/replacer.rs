//! `JSON.stringify` variants that accept a replacer/spacer.
//!
//! - `stringify_value_with_replacer` & friends: the closure-replacer arm
//! - `stringify_*_pretty`: indent-aware (3-arg `JSON.stringify(v, r, indent)`)
//! - `stringify_object_with_array_replacer`: the array-of-keys whitelist arm
//! - Public FFI: `js_json_stringify_with_replacer` and the 3-arg
//!   `js_json_stringify_full`

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};
use std::fmt::Write as FmtWrite;

// ─── JSON.stringify with replacer ────────────────────────────────────────────

/// Call a replacer closure with (key, value) and return the result as f64
#[inline]
pub(crate) unsafe fn call_replacer(
    replacer: *const crate::ClosureHeader,
    key_f64: f64,
    value_f64: f64,
) -> f64 {
    crate::js_closure_call2(replacer, key_f64, value_f64)
}

/// Stringify a value with replacer support.
/// The replacer is called as replacer(key, value) for each property.
/// Returns the replaced value serialized into the buffer.
pub(crate) unsafe fn stringify_value_with_replacer(
    key_f64: f64,
    value: f64,
    type_hint: u32,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    // Call the replacer with (key, value)
    let replaced = call_replacer(replacer, key_f64, value);
    let replaced_bits = replaced.to_bits();

    // If replacer returns undefined, skip this value
    if replaced_bits == TAG_UNDEFINED {
        return;
    }

    // Check if the replaced value is the same as the original (common case)
    // If it is, and the original is an object/array, recurse into it with replacer
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;

    // If the replaced value is a string, serialize it as a JSON string
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }
    if replaced_tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(replaced_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // If it's null/bool/number, serialize directly
    if replaced_bits == TAG_NULL {
        buf.push_str("null");
        return;
    }
    if replaced_bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if replaced_bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    // Check for BigInt tag - serialize as number (toString)
    if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            // BigInt toString gives a plain number string, write it directly (no quotes)
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    // Check for pointer (object/array) - recurse with replacer
    if let Some(ptr) = extract_pointer(replaced_bits) {
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, buf);
        } else {
            // Try to detect: object vs array
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_with_replacer(ptr, replacer, buf);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, buf);
            } else {
                // Fallback: serialize as plain value (without replacer)
                stringify_value(replaced, TYPE_UNKNOWN, buf);
            }
        }
        return;
    }

    // Plain number
    write_number(buf, replaced);
}

pub(crate) unsafe fn stringify_object_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    buf.push('{');

    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;

    // Use keys_len as the iteration count since field_count may include pre-allocated slots.
    let actual_fields = std::cmp::min(num_fields, keys_len);
    let mut first = true;
    for f in 0..actual_fields {
        // Get the key as a string
        let (key_str_ptr, key_str_opt) = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let kp = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            (kp, str_from_header(kp))
        } else {
            (std::ptr::null(), None)
        };

        // Create NaN-boxed key for replacer
        let key_f64_for_replacer = if !key_str_ptr.is_null() {
            nanbox_string_f64(key_str_ptr)
        } else {
            // Fallback: create a "fieldN" string
            let fallback = format!("field{}", f);
            let fallback_ptr = js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32);
            nanbox_string_f64(fallback_ptr)
        };

        // Get the field value
        let field_val = *fields_ptr.add(f as usize);

        // Call replacer with (key, value)
        let replaced = call_replacer(replacer, key_f64_for_replacer, field_val);
        let replaced_bits = replaced.to_bits();

        // If replacer returns undefined, skip this property
        if replaced_bits == TAG_UNDEFINED {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        // Write the key
        if let Some(key_str) = key_str_opt {
            buf.push('"');
            buf.push_str(key_str);
            buf.push_str("\":");
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }

        // Stringify the replaced value
        // For nested objects/arrays, we need to recurse with the replacer
        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(replaced_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let arr = inner_ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push('}');
}

pub(crate) unsafe fn stringify_array_with_replacer(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        let elem = *elements.add(i as usize);

        // Create key string for the index
        let idx_str = i.to_string();
        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let key_f64 = nanbox_string_f64(idx_ptr);

        // Call replacer with (index_string, value)
        let replaced = call_replacer(replacer, key_f64, elem);
        let replaced_bits = replaced.to_bits();

        // For arrays, undefined becomes null (per JSON spec)
        if replaced_bits == TAG_UNDEFINED {
            buf.push_str("null");
            continue;
        }

        let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
        if replaced_tag == STRING_TAG {
            let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(replaced_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if replaced_bits == TAG_NULL {
            buf.push_str("null");
        } else if replaced_bits == TAG_TRUE {
            buf.push_str("true");
        } else if replaced_bits == TAG_FALSE {
            buf.push_str("false");
        } else if replaced_tag == BIGINT_TAG {
            let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
            let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
            if let Some(s) = str_from_header(str_ptr) {
                buf.push_str(s);
            } else {
                buf.push_str("null");
            }
        } else if let Some(inner_ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(inner_ptr) {
                stringify_object_with_replacer(inner_ptr, replacer, buf);
            } else {
                let inner_arr = inner_ptr as *const crate::ArrayHeader;
                if !inner_arr.is_null() {
                    let inner_len = (*inner_arr).length;
                    let inner_cap = (*inner_arr).capacity;
                    if inner_len <= inner_cap && inner_cap > 0 && inner_cap < 10000 {
                        stringify_array_with_replacer(inner_ptr, replacer, buf);
                    } else {
                        stringify_value(replaced, TYPE_UNKNOWN, buf);
                    }
                } else {
                    stringify_value(replaced, TYPE_UNKNOWN, buf);
                }
            }
        } else {
            write_number(buf, replaced);
        }
    }
    buf.push(']');
}

/// JSON.stringify with replacer function
/// value: the JSValue to stringify (NaN-boxed f64)
/// type_hint: 0=unknown, 1=object, 2=array
/// replacer_ptr: pointer to a ClosureHeader (the replacer function)
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_with_replacer(
    value: f64,
    type_hint: u32,
    replacer_ptr: i64,
) -> *mut StringHeader {
    let replacer = replacer_ptr as *const crate::ClosureHeader;
    if replacer.is_null() {
        // Fall back to normal stringify if replacer is null
        return js_json_stringify(value, type_hint);
    }

    // Per JSON spec, the initial call to the replacer is with key="" and the root value
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);

    // Call replacer with ("", root_value)
    let replaced_root = call_replacer(replacer, empty_key_f64, value);
    let replaced_bits = replaced_root.to_bits();

    // If replacer returns undefined for root, return undefined (represented as "undefined" string? No, just return null)
    if replaced_bits == TAG_UNDEFINED {
        return std::ptr::null_mut();
    }

    // Non-reentrant fast path (issue #67): same depth-counter trick as
    // js_json_stringify — skip shape_cache save for the outermost call.
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let estimated = estimate_json_size(value, type_hint);
    let mut buf = take_stringify_buf();
    if buf.capacity() < estimated {
        buf.reserve(estimated - buf.capacity());
    }

    // Check what the replacer returned
    let replaced_tag = replaced_bits & 0xFFFF_0000_0000_0000;
    if replaced_tag == STRING_TAG {
        let str_ptr = (replaced_bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(&mut buf, s);
        } else {
            buf.push_str("null");
        }
    } else if replaced_tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(replaced_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(&mut buf, s);
        } else {
            buf.push_str("null");
        }
    } else if replaced_bits == TAG_NULL {
        buf.push_str("null");
    } else if replaced_bits == TAG_TRUE {
        buf.push_str("true");
    } else if replaced_bits == TAG_FALSE {
        buf.push_str("false");
    } else if replaced_tag == BIGINT_TAG {
        let bigint_ptr = (replaced_bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
    } else if let Some(ptr) = extract_pointer(replaced_bits) {
        // Object or array - recurse with replacer
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_with_replacer(ptr, replacer, &mut buf);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_with_replacer(ptr, replacer, &mut buf);
        } else {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, replacer, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null() {
                    let len = (*arr).length;
                    let cap = (*arr).capacity;
                    if len <= cap && cap > 0 && cap < 10000 {
                        stringify_array_with_replacer(ptr, replacer, &mut buf);
                    } else {
                        stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                    }
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        }
    } else {
        // Number
        write_number(&mut buf, replaced_root);
    }

    let result = js_string_from_bytes(buf.as_ptr(), buf.len() as u32);
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    result
}

// ─── Pretty-print stringify ─────────────────────────────────────────────────

pub(crate) unsafe fn stringify_value_pretty(
    value: f64,
    type_hint: u32,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    let bits: u64 = value.to_bits();

    if bits == TAG_NULL || bits == TAG_UNDEFINED {
        buf.push_str("null");
        return;
    }
    if bits == TAG_TRUE {
        buf.push_str("true");
        return;
    }
    if bits == TAG_FALSE {
        buf.push_str("false");
        return;
    }

    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }
    // SSO (v0.5.213): decode inline 5-byte string, emit escaped.
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if tag == BIGINT_TAG {
        let bigint_ptr = (bits & POINTER_MASK) as *const crate::BigIntHeader;
        let str_ptr = crate::bigint::js_bigint_to_string(bigint_ptr);
        if let Some(s) = str_from_header(str_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        if type_hint == TYPE_OBJECT || (type_hint == TYPE_UNKNOWN && is_object_pointer(ptr)) {
            stringify_object_pretty(ptr, buf, indent, depth);
        } else if type_hint == TYPE_ARRAY {
            stringify_array_pretty(ptr, buf, indent, depth);
        } else {
            let arr = ptr as *const crate::ArrayHeader;
            if !arr.is_null() {
                let len = (*arr).length;
                let cap = (*arr).capacity;
                if len <= cap && cap > 0 && cap < 10000 && !is_object_pointer(ptr) {
                    stringify_array_pretty(ptr, buf, indent, depth);
                    return;
                }
            }
            if is_object_pointer(ptr) {
                stringify_object_pretty(ptr, buf, indent, depth);
            } else {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
        }
        return;
    }

    write_number(buf, value);
}

pub(crate) unsafe fn stringify_object_pretty(
    ptr: *const u8,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    // Check for toJSON method
    if let Some(to_json_val) = object_get_to_json(ptr) {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        stringify_value_pretty(to_json_val, TYPE_UNKNOWN, buf, indent, depth);
        return;
    }

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Collect non-undefined, non-closure fields
    let mut entries: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let field_bits = field_val.to_bits();
        if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
            continue;
        }
        let key_name = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        entries.push((key_name, field_val));
    }

    if entries.is_empty() {
        buf.push_str("{}");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    buf.push_str("{\n");
    let inner_indent_count = depth + 1;
    for (i, (key_name, field_val)) in entries.iter().enumerate() {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        buf.push('"');
        buf.push_str(key_name);
        buf.push_str("\": ");
        stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        if i + 1 < entries.len() {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

pub(crate) unsafe fn stringify_array_pretty(
    ptr: *const u8,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    if len == 0 {
        buf.push_str("[]");
        return;
    }

    buf.push_str("[\n");
    let inner_indent_count = depth + 1;
    for i in 0..len {
        for _ in 0..inner_indent_count {
            buf.push_str(indent);
        }
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null");
        } else {
            stringify_value_pretty(elem, TYPE_UNKNOWN, buf, indent, inner_indent_count);
        }
        if i + 1 < len {
            buf.push(',');
        }
        buf.push('\n');
    }
    for _ in 0..depth {
        buf.push_str(indent);
    }
    buf.push(']');
}

// ─── Array replacer (key whitelist) stringify ────────────────────────────────

pub(crate) unsafe fn stringify_object_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    // Circular reference check
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        // Use js_typeerror_new so error_kind == ERROR_KIND_TYPE_ERROR and
        // `e instanceof TypeError` returns true (matching Node).
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let keys_arr = (*obj).keys_array;
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let actual_fields = std::cmp::min(num_fields, keys_len);

    // Build a map of key_name -> field_value for the object
    let mut field_map: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let field_val = *fields_ptr.add(f as usize);
        let key_name = if f < keys_len {
            let key_f64 = *keys_elements.add(f as usize);
            let key_bits = key_f64.to_bits();
            let key_tag = key_bits & 0xFFFF_0000_0000_0000;
            let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
                (key_bits & POINTER_MASK) as *const StringHeader
            } else {
                key_bits as *const StringHeader
            };
            str_from_header(key_ptr)
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("field{}", f))
        } else {
            format!("field{}", f)
        };
        field_map.push((key_name, field_val));
    }

    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        if let Some((_, field_val)) = field_map.iter().find(|(k, _)| k == allowed_key) {
            let field_bits = field_val.to_bits();
            if field_bits == TAG_UNDEFINED || is_closure_value(field_bits) {
                continue;
            }
            if !first {
                buf.push(',');
            }
            first = false;
            if use_pretty {
                buf.push('\n');
                let inner_indent_count = depth + 1;
                for _ in 0..inner_indent_count {
                    buf.push_str(indent);
                }
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\": ");
                stringify_value_pretty(*field_val, TYPE_UNKNOWN, buf, indent, inner_indent_count);
            } else {
                buf.push('"');
                buf.push_str(allowed_key);
                buf.push_str("\":");
                stringify_value(*field_val, TYPE_UNKNOWN, buf);
            }
        }
    }
    if use_pretty && !first {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push('}');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

// ─── Extract array of strings from a JSValue array ──────────────────────────

pub(crate) unsafe fn extract_string_array(ptr: *const u8) -> Vec<String> {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut result = Vec::new();
    for i in 0..len {
        let elem = *elements.add(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;
        if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        } else if elem_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(elem_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                result.push(s.to_string());
            }
        } else if is_raw_pointer(elem_bits) {
            let str_ptr = elem_bits as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                result.push(s.to_string());
            }
        }
    }
    result
}

/// Detect whether a NaN-boxed value is an array (not an object).
#[inline]
pub(crate) unsafe fn is_array_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        if is_object_pointer(ptr) {
            return false;
        }
        let arr = ptr as *const crate::ArrayHeader;
        let len = (*arr).length;
        let cap = (*arr).capacity;
        len <= cap && cap > 0 && cap < 10000
    } else {
        false
    }
}

// ─── Full JSON.stringify(value, replacer, spacer) ───────────────────────────

/// JSON.stringify(value, replacer, spacer) — the full 3-arg form.
///
/// - `value`: NaN-boxed JSValue to stringify
/// - `replacer_f64`: NaN-boxed — a closure (function replacer), array (key whitelist), or null
/// - `spacer_f64`: NaN-boxed — a number (indent count), string (indent string), or null
///
/// Returns i64 JSValue bits: a NaN-boxed string pointer, or TAG_UNDEFINED when
/// `JSON.stringify(undefined)` should return `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_json_stringify_full(
    value: f64,
    replacer_f64: f64,
    spacer_f64: f64,
) -> i64 {
    let value_bits = value.to_bits();

    // JSON.stringify(undefined) returns undefined per spec
    if value_bits == TAG_UNDEFINED {
        return TAG_UNDEFINED as i64;
    }

    // If the value is a closure/function, return undefined per spec
    if is_closure_value(value_bits) {
        return TAG_UNDEFINED as i64;
    }

    // Issue #179 Phase 4: lazy-stringify fast path for unmutated
    // lazy arrays — only when no replacer / no indent (matches the
    // output `JSON.stringify(value)` produces; replacer/indent
    // require a real tree walk). The bench's 2-arg form (and most
    // real usage) hits this path.
    let replacer_bits = replacer_f64.to_bits();
    let spacer_bits = spacer_f64.to_bits();
    let no_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;
    let no_spacer =
        spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE;
    if no_replacer && no_spacer {
        if let Some(ptr) = try_stringify_lazy_array(value) {
            return JSValue::string_ptr(ptr).bits() as i64;
        }
    }
    // Lazy-but-materialized: the fast path's `materialized.is_null()`
    // check above returns None; fall back to the tree walk, but
    // point it at the materialized tree (not the lazy header
    // whose fields aren't element f64s).
    let value = redirect_lazy_to_materialized(value);
    let value_bits = value.to_bits();

    // Determine spacer/indent
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        indent_str = str_from_header(sp_ptr).unwrap_or("").to_string();
    } else if spacer_tag == crate::value::SHORT_STRING_TAG {
        // v0.5.213 SSO: spacer passed as inline short string
        // (e.g. `JSON.stringify(obj, null, "  ")` where "  " is 2
        // bytes — fits SSO). Decode into scratch, copy into the
        // indent_str buffer for the formatter.
        let jsval = JSValue::from_bits(spacer_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        indent_str = std::str::from_utf8(&scratch[..n]).unwrap_or("").to_string();
    } else if spacer_bits == TAG_TRUE {
        indent_str = String::new();
    } else {
        // Number — use that many spaces (clamped to 10)
        let n = spacer_f64 as usize;
        let n = n.min(10);
        indent_str = " ".repeat(n);
    }
    let use_pretty = !indent_str.is_empty();

    // Determine replacer type
    let replacer_bits = replacer_f64.to_bits();
    let is_null_replacer = replacer_bits == TAG_NULL || replacer_bits == TAG_UNDEFINED;

    // Check if replacer is an array (key whitelist)
    let array_replacer = if !is_null_replacer && is_array_value(replacer_bits) {
        let arr_ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (replacer_bits & POINTER_MASK) as *const u8
        } else {
            replacer_bits as *const u8
        };
        Some(extract_string_array(arr_ptr))
    } else {
        None
    };

    // Check if replacer is a closure (function)
    let closure_replacer =
        if !is_null_replacer && array_replacer.is_none() && is_closure_value(replacer_bits) {
            let ptr = if (replacer_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG {
                (replacer_bits & POINTER_MASK) as *const crate::closure::ClosureHeader
            } else {
                replacer_bits as *const crate::closure::ClosureHeader
            };
            Some(ptr)
        } else {
            None
        };

    // Non-reentrant fast path (issue #67): same depth-counter trick as
    // js_json_stringify — skip shape_cache save for the outermost call.
    // Skip the pre-call STRINGIFY_STACK clear: the exit path below always
    // clears it on normal return, and the deep-recursion check at depth
    // > MAX_FAST_DEPTH is robust to leftover entries from a prior panic
    // (a stale ptr that happens to match is a false-positive TypeError,
    // which is a defensible degradation for pathological reentrant cases).
    let prior_depth = STRINGIFY_DEPTH.with(|d| {
        let c = d.get();
        d.set(c + 1);
        c
    });
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();

    if let Some(ref allowed_keys) = array_replacer {
        // Array replacer: only applies to objects at the top level
        if let Some(ptr) = extract_pointer(value_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_array_replacer(
                    ptr,
                    allowed_keys,
                    &mut buf,
                    &indent_str,
                    0,
                    use_pretty,
                );
            } else if use_pretty {
                stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
            } else {
                stringify_value(value, TYPE_UNKNOWN, &mut buf);
            }
        } else if use_pretty {
            stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
        } else {
            stringify_value(value, TYPE_UNKNOWN, &mut buf);
        }
    } else if let Some(closure_ptr) = closure_replacer {
        // Function replacer — use existing with_replacer path
        // First call replacer with ("", root_value)
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let replaced_root = call_replacer(closure_ptr, empty_key_f64, value);
        let replaced_bits = replaced_root.to_bits();
        if replaced_bits == TAG_UNDEFINED {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            // Restore shape cache and decrement depth before early return
            // (we already incremented STRINGIFY_DEPTH and took the cache).
            restore_stringify_buf(buf);
            match saved_cache {
                Some(s) => restore_shape_cache(s),
                None => clear_shape_cache(),
            }
            STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
            return TAG_UNDEFINED as i64;
        }
        // For simplicity: when function replacer is used with pretty, we don't
        // interleave pretty-printing (matches simple spec behavior). Serialize
        // normally with the replacer.
        if let Some(ptr) = extract_pointer(replaced_bits) {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer(ptr, closure_ptr, &mut buf);
            } else {
                let arr = ptr as *const crate::ArrayHeader;
                if !arr.is_null()
                    && (*arr).length <= (*arr).capacity
                    && (*arr).capacity > 0
                    && (*arr).capacity < 10000
                {
                    stringify_array_with_replacer(ptr, closure_ptr, &mut buf);
                } else {
                    stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
                }
            }
        } else {
            stringify_value(replaced_root, TYPE_UNKNOWN, &mut buf);
        }
    } else if use_pretty {
        // No replacer, but has spacer — pretty-print
        stringify_value_pretty(value, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
    } else {
        // Plain stringify
        stringify_value(value, TYPE_UNKNOWN, &mut buf);
    }

    // Only touch STRINGIFY_STACK if we actually pushed to it (depth >
    // MAX_FAST_DEPTH was hit). The `borrow` path avoids the borrow_mut
    // cost on the common empty-stack case. Unpopped entries only exist
    // after a panic mid-traversal; see the entry-side comment for the
    // correctness argument.
    STRINGIFY_STACK.with(|s| {
        let stack = s.borrow();
        if !stack.is_empty() {
            drop(stack);
            s.borrow_mut().clear();
        }
    });

    let result_ptr = json_string_from_output_bytes(buf.as_bytes());
    restore_stringify_buf(buf);
    match saved_cache {
        Some(s) => restore_shape_cache(s),
        None => clear_shape_cache(),
    }
    STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
    // Return as NaN-boxed string
    (STRING_TAG | (result_ptr as u64 & POINTER_MASK)) as i64
}

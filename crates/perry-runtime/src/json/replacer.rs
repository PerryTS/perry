//! `JSON.stringify` variants that accept a replacer/spacer.
//!
//! - `stringify_{object,array}_with_replacer{,_pretty}`: the closure-replacer
//!   walk. Per spec `SerializeJSONProperty` each value runs toJSON → replacer →
//!   recurse, and the `_pretty` variants thread the indent string + depth so
//!   the 3-arg `JSON.stringify(v, r, indent)` form pretty-prints.
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

/// Resolve `value.toJSON(key)` if `value` is an object with a callable
/// `toJSON` field, per spec `SerializeJSONProperty` step 2 (run BEFORE the
/// replacer). Mirrors the no-replacer path's `object_get_to_json`, which only
/// fires when the object actually has a closure-typed `toJSON` field. Returns
/// the (possibly substituted) value.
#[inline]
unsafe fn apply_to_json(value: f64) -> f64 {
    let bits = value.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        // Only plain JS objects carry a `toJSON` field worth probing; arrays /
        // buffers / errors don't, and probing them would walk an unrelated
        // layout. `object_get_to_json` itself guards on a null keys_array.
        if gc_obj_type(ptr) == crate::gc::GC_TYPE_OBJECT
            && !crate::buffer::is_registered_buffer(ptr as usize)
        {
            if let Some(to_json_val) = object_get_to_json(ptr) {
                return to_json_val;
            }
        }
    }
    value
}

/// Write a non-pointer (or fully-resolved) JSON scalar. Returns `true` when the
/// value was a scalar handled here; `false` when it is a pointer the caller must
/// recurse into. Shared by both the compact and pretty walks.
#[inline]
unsafe fn write_replaced_scalar(buf: &mut String, replaced: f64) -> bool {
    let replaced_bits = replaced.to_bits();
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
            // BigInt toString gives a plain number string (no quotes).
            buf.push_str(s);
        } else {
            buf.push_str("null");
        }
    } else if extract_pointer(replaced_bits).is_some() {
        // Pointer — caller recurses with the replacer.
        return false;
    } else {
        // Plain number (or Date via DATE_REGISTRY in write_number).
        write_number(buf, replaced);
    }
    true
}

/// Resolve `value.toJSON(key)` (spec `SerializeJSONProperty` step 2 — run
/// BEFORE the replacer). `key_f64` is the property key passed to `toJSON`.
#[inline]
unsafe fn apply_to_json_keyed(value: f64, _key_f64: f64) -> f64 {
    // `object_get_to_json` calls toJSON with the empty-string key arg, matching
    // the no-replacer path. (Effect's Inspectable.toJSON ignores its argument;
    // Node passes the property key. We mirror the no-replacer path's empty key
    // to stay byte-identical with the rest of Perry's JSON suite.)
    apply_to_json(value)
}

/// Dispatch a pointer value to the object/array replacer walk using the GC type
/// tag (robust object/array discrimination), with a structural fallback for
/// untagged pointers.
#[inline]
unsafe fn dispatch_pointer_with_replacer(
    ptr: *const u8,
    replaced: f64,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    // Buffer / Uint8Array have no GcHeader — detect before gc_obj_type so the
    // tag read doesn't deref unrelated memory (issue #639 pattern). This
    // dispatch serves both compact (indent == "") and pretty replacer walks,
    // so pick the matching buffer serializer.
    if crate::buffer::is_registered_buffer(ptr as usize) {
        if indent.is_empty() {
            stringify_buffer(ptr, buf);
        } else {
            stringify_buffer_pretty(ptr, buf, indent, depth);
        }
        return;
    }
    match gc_obj_type(ptr) {
        crate::gc::GC_TYPE_ARRAY => {
            stringify_array_with_replacer_pretty(ptr, replacer, buf, indent, depth)
        }
        crate::gc::GC_TYPE_OBJECT => {
            if is_object_pointer(ptr) {
                stringify_object_with_replacer_pretty(ptr, replacer, buf, indent, depth);
            } else if (*(ptr as *const crate::ObjectHeader)).keys_array.is_null() {
                // Genuinely-empty object (#1704): emit "{}" not "null".
                buf.push_str("{}");
            } else {
                buf.push_str("null");
            }
        }
        crate::gc::GC_TYPE_STRING => {
            let str_ptr = ptr as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        }
        crate::gc::GC_TYPE_ERROR => {
            // Error objects have a dedicated layout; Node emits "{}" (#928).
            buf.push_str("{}");
        }
        crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET => {
            // Map/Set have a non-ObjectHeader layout; Node serializes both
            // as "{}". Must not reach the catch-all (segfault) — same fix as
            // the plain-stringify paths in `stringify.rs`.
            buf.push_str("{}");
        }
        _ => {
            // Untagged pointer: structural fallback (no replacer recursion is
            // safe here — we don't know the layout). Defer to plain stringify.
            if is_object_pointer(ptr) {
                stringify_object_with_replacer_pretty(ptr, replacer, buf, indent, depth);
            } else {
                stringify_value(replaced, TYPE_UNKNOWN, buf);
            }
        }
    }
}

/// Object walk with optional pretty-printing. For each field: toJSON →
/// replacer → recurse, threading indent/depth. Drops fields whose replacer
/// result is undefined or a closure (spec / Node behavior).
pub(crate) unsafe fn stringify_object_with_replacer_pretty(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    // Circular-reference detection (mirrors the pretty/array-replacer paths).
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
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

    // Use keys_len as the iteration count since field_count may include pre-allocated slots.
    let actual_fields = std::cmp::min(num_fields, keys_len);
    let use_pretty = !indent.is_empty();
    let inner_depth = depth + 1;
    // A function replacer only sees own ENUMERABLE keys (EnumerableOwnProperty
    // Names); gated for the common no-descriptor case.
    let filter_non_enum = crate::object::descriptors_in_use();
    buf.push('{');
    let mut first = true;
    for f in 0..actual_fields {
        // Skip non-enumerable own keys before invoking the replacer.
        if filter_non_enum
            && f < keys_len
            && super::stringify::json_key_non_enumerable(obj, *keys_elements.add(f as usize))
        {
            continue;
        }
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

        // Create NaN-boxed key for replacer / toJSON
        let key_f64_for_replacer = if !key_str_ptr.is_null() {
            nanbox_string_f64(key_str_ptr)
        } else {
            let fallback = format!("field{}", f);
            let fallback_ptr = js_string_from_bytes(fallback.as_ptr(), fallback.len() as u32);
            nanbox_string_f64(fallback_ptr)
        };

        // Get the field value (invoking an own getter, as spec [[Get]] does),
        // resolve toJSON, then apply the replacer.
        let mut field_val = *fields_ptr.add(f as usize);
        if filter_non_enum && f < keys_len {
            if let Some(gv) =
                crate::object::json_object_getter_value(obj, *keys_elements.add(f as usize))
            {
                field_val = gv;
            }
        }
        let field_after_to_json = apply_to_json_keyed(field_val, key_f64_for_replacer);
        let replaced = call_replacer(replacer, key_f64_for_replacer, field_after_to_json);
        let replaced_bits = replaced.to_bits();

        // Omit the property if the replacer returns undefined or a function.
        if replaced_bits == TAG_UNDEFINED || is_closure_value(replaced_bits) {
            continue;
        }

        if !first {
            buf.push(',');
        }
        first = false;

        if use_pretty {
            buf.push('\n');
            for _ in 0..inner_depth {
                buf.push_str(indent);
            }
        }

        // Write the key
        if let Some(key_str) = key_str_opt {
            buf.push('"');
            buf.push_str(key_str);
            buf.push_str(if use_pretty { "\": " } else { "\":" });
        } else {
            let _ = write!(buf, "\"field{}\"{}", f, if use_pretty { ": " } else { ":" });
        }

        // Write scalar inline, or recurse into the pointer with the replacer.
        if !write_replaced_scalar(buf, replaced) {
            let inner_ptr = extract_pointer(replaced_bits).unwrap();
            dispatch_pointer_with_replacer(inner_ptr, replaced, replacer, buf, indent, inner_depth);
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

/// Array walk with optional pretty-printing. For each element: toJSON →
/// replacer → recurse. undefined / closure results serialize to `null` (spec).
pub(crate) unsafe fn stringify_array_with_replacer_pretty(
    ptr: *const u8,
    replacer: *const crate::ClosureHeader,
    buf: &mut String,
    indent: &str,
    depth: usize,
) {
    // Circular-reference detection.
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;

    if len == 0 {
        buf.push_str("[]");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    let use_pretty = !indent.is_empty();
    let inner_depth = depth + 1;
    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        if use_pretty {
            buf.push('\n');
            for _ in 0..inner_depth {
                buf.push_str(indent);
            }
        }
        let elem = *elements.add(i as usize);

        // Index key as a string for toJSON / replacer.
        let idx_str = i.to_string();
        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let key_f64 = nanbox_string_f64(idx_ptr);

        let elem_after_to_json = apply_to_json_keyed(elem, key_f64);
        let replaced = call_replacer(replacer, key_f64, elem_after_to_json);
        let replaced_bits = replaced.to_bits();

        // Array holes / undefined / functions become null (per JSON spec).
        if replaced_bits == TAG_UNDEFINED || is_closure_value(replaced_bits) {
            buf.push_str("null");
            continue;
        }

        if !write_replaced_scalar(buf, replaced) {
            let inner_ptr = extract_pointer(replaced_bits).unwrap();
            dispatch_pointer_with_replacer(inner_ptr, replaced, replacer, buf, indent, inner_depth);
        }
    }
    if use_pretty {
        buf.push('\n');
        for _ in 0..depth {
            buf.push_str(indent);
        }
    }
    buf.push(']');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
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

    // Per JSON spec, the initial call to the replacer is with key="" and the
    // root value — but toJSON runs FIRST (SerializeJSONProperty step 2).
    let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_key_f64 = nanbox_string_f64(empty_str);
    let value_after_to_json = apply_to_json_keyed(value, empty_key_f64);

    // Call replacer with ("", root_value)
    let replaced_root = call_replacer(replacer, empty_key_f64, value_after_to_json);
    let replaced_bits = replaced_root.to_bits();

    // If replacer returns undefined for root, return undefined.
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
    // Defensive: clear the one-shot `toJSON` suppression guard at the outermost
    // entry so a throw during a prior stringify can't leak it across calls.
    if prior_depth == 0 {
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
    }
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

    // Serialize the (toJSON-resolved, replacer-applied) root value: scalars
    // inline, pointers via the GC-tag dispatch (compact, no indent).
    if !write_replaced_scalar(&mut buf, replaced_root) {
        let ptr = extract_pointer(replaced_bits).unwrap();
        dispatch_pointer_with_replacer(ptr, replaced_root, replacer, &mut buf, "", 0);
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
        // #3857: a boxed primitive wrapper (`new String`/`Number`/`Boolean`,
        // `Object(1n)`) serializes as its underlying primitive. Must run before
        // the `is_object_pointer` probes below, which would deref the wrapper
        // as a plain object (emitting `{}`) — and, in the 3-arg pretty form,
        // crash on its empty key layout.
        if let Some(prim) = crate::builtins::boxed_primitive_json_value(value) {
            stringify_value_pretty(prim, TYPE_UNKNOWN, buf, indent, depth);
            return;
        }
        // Buffer / Map / Set / Error have non-ObjectHeader layouts; detect them
        // before the `is_object_pointer` probes below, which would deref their
        // internals as a `keys_array` and segfault. Buffers (no GcHeader, so
        // checked first) pretty-print their `{type,data}` / index form; Map/
        // Set/Error serialize as "{}" in Node (no enumerable own props).
        if crate::buffer::is_registered_buffer(ptr as usize) {
            stringify_buffer_pretty(ptr, buf, indent, depth);
            return;
        }
        // #2900: raw-JSON wrapper — emit stored text verbatim (pretty-print
        // output never indents a scalar, so no indentation is applied here
        // either).
        if let Some(raw) = super::raw_json_text_bytes(ptr) {
            buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
            return;
        }
        if matches!(
            gc_obj_type(ptr),
            crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET | crate::gc::GC_TYPE_ERROR
        ) {
            buf.push_str("{}");
            return;
        }
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
        arm_to_json_result_guard(to_json_val);
        stringify_value_pretty(to_json_val, TYPE_UNKNOWN, buf, indent, depth);
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
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
    // Only own ENUMERABLE keys are serialized (gated for the common case).
    let filter_non_enum = crate::object::descriptors_in_use();

    // Collect non-undefined, non-closure fields
    let mut entries: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        // Skip non-enumerable own keys (`Object.defineProperty(o, k,
        // { enumerable: false })`) before touching the value.
        if filter_non_enum
            && f < keys_len
            && super::stringify::json_key_non_enumerable(obj, *keys_elements.add(f as usize))
        {
            continue;
        }
        let mut field_val = *fields_ptr.add(f as usize);
        // Own accessor properties: serialize the getter's return value.
        if filter_non_enum && f < keys_len {
            if let Some(gv) =
                crate::object::json_object_getter_value(obj, *keys_elements.add(f as usize))
            {
                field_val = gv;
            }
        }
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
    // Defensive: clear the one-shot `toJSON` suppression guard at the outermost
    // entry so a throw during a prior stringify can't leak it across calls.
    if prior_depth == 0 {
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
    }
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
        // Function replacer. Per spec SerializeJSONProperty: toJSON FIRST, then
        // the replacer, then serialize — threading `indent_str` so the 3-arg
        // form (replacer + space) pretty-prints, matching Node.
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let value_after_to_json = apply_to_json_keyed(value, empty_key_f64);
        let replaced_root = call_replacer(closure_ptr, empty_key_f64, value_after_to_json);
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
        // Serialize the root: scalars inline, pointers via the GC-tag dispatch
        // (object vs array) so the indent threads through nested structures.
        if !write_replaced_scalar(&mut buf, replaced_root) {
            let ptr = extract_pointer(replaced_bits).unwrap();
            dispatch_pointer_with_replacer(
                ptr,
                replaced_root,
                closure_ptr,
                &mut buf,
                &indent_str,
                0,
            );
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

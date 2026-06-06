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

/// Call a replacer closure as `replacer.call(holder, key, value)` per spec
/// (SerializeJSONProperty step 3) and return the result as f64. `holder_f64` is
/// the containing object/array (or the `{"": value}` wrapper at the root), bound
/// as the replacer's `this` (replacer-function-arguments / -wrapper).
#[inline]
pub(crate) unsafe fn call_replacer(
    replacer: *const crate::ClosureHeader,
    key_f64: f64,
    value_f64: f64,
    holder_f64: f64,
) -> f64 {
    let prev = crate::object::js_implicit_this_set(holder_f64);
    let result = crate::js_closure_call2(replacer, key_f64, value_f64);
    crate::object::js_implicit_this_set(prev);
    result
}

/// Build the `{"": value}` wrapper object that holds the root value for the
/// initial `SerializeJSONProperty("", wrapper)` call — used as the replacer's
/// `this` for the root invocation (replacer-function-wrapper).
#[inline]
unsafe fn make_root_wrapper(value: f64) -> f64 {
    let wrapper = crate::object::js_object_alloc(0, 1);
    let empty_key = js_string_from_bytes(b"".as_ptr(), 0);
    crate::object::js_object_set_field_by_name(wrapper, empty_key, value);
    nanbox_pointer_f64(wrapper as *const u8)
}

/// Resolve `value.toJSON(key)` if `value` is an object with a callable
/// `toJSON` field, per spec `SerializeJSONProperty` step 2 (run BEFORE the
/// replacer). Mirrors the no-replacer path's `object_get_to_json`, which only
/// fires when the object actually has a closure-typed `toJSON` field. Returns
/// the (possibly substituted) value.
#[inline]
unsafe fn apply_to_json(value: f64) -> f64 {
    let bits = value.to_bits();
    // A BigInt resolves `BigInt.prototype.toJSON` too (SerializeJSONProperty
    // step 2 applies to BigInt values). If absent, the BigInt flows through
    // unchanged and the terminal serializer throws the BigInt TypeError.
    if (bits & 0xFFFF_0000_0000_0000) == BIGINT_TAG {
        if let Some(r) =
            super::stringify::bigint_resolve_to_json(value, super::stringify::json_empty_key())
        {
            return r;
        }
        return value;
    }
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
        // A BigInt that survived toJSON + replacer is not JSON-serializable
        // (ECMA-262 25.5.2 step 11) — throw a TypeError (value-bigint-order).
        super::stringify::throw_bigint_json_error();
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
    // A boxed primitive returned by the replacer serializes as its underlying
    // primitive (value-boolean-object: a `new Boolean(true)` replacer result
    // becomes `true`, not `{}`).
    if let Some(prim) = crate::builtins::boxed_primitive_json_value(replaced) {
        if write_replaced_scalar(buf, prim) {
            return;
        }
    }
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
            } else if super::stringify::object_has_no_own_keys(ptr) {
                // Empty object (#1704) incl. a class instance with no own fields
                // (only prototype methods/getters): emit "{}" not "null".
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
        let holder = nanbox_pointer_f64(ptr);
        let replaced = call_replacer(replacer, key_f64_for_replacer, field_after_to_json, holder);
        let replaced_bits = replaced.to_bits();

        // Omit the property if the replacer returns undefined, a function, or
        // a Symbol.
        if replaced_bits == TAG_UNDEFINED
            || is_closure_value(replaced_bits)
            || super::stringify::is_symbol_bits(replaced_bits)
        {
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

        // Write the key (escaped — property names are JSON strings).
        if let Some(key_str) = key_str_opt {
            write_escaped_string(buf, key_str);
            buf.push_str(if use_pretty { ": " } else { ":" });
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
        let holder = nanbox_pointer_f64(ptr);
        let replaced = call_replacer(replacer, key_f64, elem_after_to_json, holder);
        let replaced_bits = replaced.to_bits();

        // Array holes / undefined / functions / Symbols become null (per spec).
        if replaced_bits == TAG_UNDEFINED
            || is_closure_value(replaced_bits)
            || super::stringify::is_symbol_bits(replaced_bits)
        {
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

    // Call replacer with ("", root_value), bound to the {"": value} wrapper.
    let root_holder = make_root_wrapper(value);
    let replaced_root = call_replacer(replacer, empty_key_f64, value_after_to_json, root_holder);
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
        // Apply `BigInt.prototype.toJSON` or throw (see stringify.rs).
        match super::stringify::bigint_resolve_to_json(value, super::stringify::json_empty_key()) {
            Some(r) => stringify_value_pretty(r, TYPE_UNKNOWN, buf, indent, depth),
            None => super::stringify::throw_bigint_json_error(),
        }
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        // A Symbol or function reaching the value dispatcher serializes as
        // `null`; object-field omission is handled at the loop.
        if super::stringify::is_symbol_bits(bits) || is_closure_value(bits) {
            buf.push_str("null");
            return;
        }
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
        // An empty object (incl. a class instance with no own fields — only
        // prototype methods/getters) fails `is_object_pointer` and would be
        // misdetected as an array by the `else` fallback below. Emit "{}" after
        // a `toJSON` probe (a `class { toJSON() {…} }` instance carries no own
        // field but must still honour the prototype method).
        if gc_obj_type(ptr) == crate::gc::GC_TYPE_OBJECT
            && super::stringify::object_has_no_own_keys(ptr)
        {
            if (*(ptr as *const crate::ObjectHeader)).class_id != 0 {
                if let Some(to_json_val) = object_get_to_json(ptr) {
                    arm_to_json_result_guard(to_json_val);
                    stringify_value_pretty(to_json_val, TYPE_UNKNOWN, buf, indent, depth);
                    SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                    return;
                }
            }
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
        if field_bits == TAG_UNDEFINED
            || is_closure_value(field_bits)
            || super::stringify::is_symbol_bits(field_bits)
        {
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
        write_escaped_string(buf, key_name);
        buf.push_str(": ");
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

/// Serialize one already-resolved value (post-`toJSON`) under an array
/// replacer. Dispatches scalars, boxed primitives, dates, buffers, raw-JSON,
/// objects (filtered by `allowed_keys`), and arrays. The `allowed_keys`
/// PropertyList propagates into every nested OBJECT (ECMA-262 SerializeJSONObject
/// uses the same PropertyList at every depth) — arrays serialize all elements,
/// but objects reached through them are still filtered.
unsafe fn stringify_resolved_array_replacer(
    value: f64,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    // Scalars (null/bool/string/bigint/number) emit inline.
    if write_replaced_scalar(buf, value) {
        return;
    }
    let bits = value.to_bits();
    let ptr = match extract_pointer(bits) {
        Some(p) => p,
        None => {
            buf.push_str("null");
            return;
        }
    };
    // Boxed primitive wrapper -> underlying primitive.
    if let Some(prim) = crate::builtins::boxed_primitive_json_value(value) {
        if write_replaced_scalar(buf, prim) {
            return;
        }
    }
    // Date -> toJSON() ISO string (or null for an Invalid Date).
    if crate::date::is_date_cell_addr(ptr as usize) {
        let s_ptr = crate::date::js_date_to_json(value);
        if let Some(s) = str_from_header(s_ptr) {
            write_escaped_string(buf, s);
        } else {
            buf.push_str("null");
        }
        return;
    }
    // Raw-JSON wrapper -> stored text verbatim.
    if let Some(raw) = super::raw_json_text_bytes(ptr) {
        buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
        return;
    }
    // Buffer / Uint8Array (no GcHeader) -> dedicated serializer.
    if crate::buffer::is_registered_buffer(ptr as usize) {
        if use_pretty {
            stringify_buffer_pretty(ptr, buf, indent, depth);
        } else {
            stringify_buffer(ptr, buf);
        }
        return;
    }
    match gc_obj_type(ptr) {
        crate::gc::GC_TYPE_ARRAY => {
            stringify_array_with_array_replacer(ptr, allowed_keys, buf, indent, depth, use_pretty)
        }
        crate::gc::GC_TYPE_OBJECT => {
            if is_object_pointer(ptr) {
                stringify_object_with_array_replacer(
                    ptr,
                    allowed_keys,
                    buf,
                    indent,
                    depth,
                    use_pretty,
                );
            } else if super::stringify::object_has_no_own_keys(ptr) {
                buf.push_str("{}");
            } else {
                buf.push_str("null");
            }
        }
        crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET | crate::gc::GC_TYPE_ERROR => {
            buf.push_str("{}");
        }
        crate::gc::GC_TYPE_STRING => {
            let str_ptr = ptr as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        }
        _ => {
            if is_object_pointer(ptr) {
                stringify_object_with_array_replacer(
                    ptr,
                    allowed_keys,
                    buf,
                    indent,
                    depth,
                    use_pretty,
                );
            } else {
                stringify_value(value, TYPE_UNKNOWN, buf);
            }
        }
    }
}

/// SerializeJSONProperty for the array-replacer path: apply `toJSON`, then
/// decide whether the value is serializable. Returns `false` (writing nothing)
/// when the resolved value is undefined / a function / a Symbol — the caller
/// omits the property (object) or emits `null` (array). Otherwise writes the
/// serialized value to `buf` and returns `true`.
unsafe fn serialize_property_array_replacer(
    value: f64,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) -> bool {
    let value = apply_to_json(value);
    let bits = value.to_bits();
    if bits == TAG_UNDEFINED || is_closure_value(bits) || super::stringify::is_symbol_bits(bits) {
        return false;
    }
    stringify_resolved_array_replacer(value, allowed_keys, buf, indent, depth, use_pretty);
    true
}

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

    // Build a map of key_name -> own property VALUE. For accessor properties
    // this invokes the getter exactly once (so a key listed twice in the
    // replacer still only triggers a single [[Get]] — replacer-array-duplicates).
    let mut field_map: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let key_f64 = *keys_elements.add(f as usize);
        let key_bits = key_f64.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        let key_name = str_from_header(key_ptr)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("field{}", f));
        // Invoke an own getter ([[Get]]); fall back to the stored slot.
        let value = crate::object::json_object_getter_value(obj, key_f64)
            .unwrap_or_else(|| *fields_ptr.add(f as usize));
        field_map.push((key_name, value));
    }

    let inner_depth = depth + 1;
    buf.push('{');
    let mut first = true;
    for allowed_key in allowed_keys {
        let field_val = match field_map.iter().find(|(k, _)| k == allowed_key) {
            Some((_, v)) => *v,
            // Property absent -> [[Get]] returns undefined -> omitted.
            None => continue,
        };
        // Tentatively write the separator + key, then serialize. If the value
        // resolves to undefined/function/Symbol, roll back the whole entry.
        let save = buf.len();
        if !first {
            buf.push(',');
        }
        if use_pretty {
            buf.push('\n');
            for _ in 0..inner_depth {
                buf.push_str(indent);
            }
            write_escaped_string(buf, allowed_key);
            buf.push_str(": ");
        } else {
            write_escaped_string(buf, allowed_key);
            buf.push(':');
        }
        if serialize_property_array_replacer(
            field_val,
            allowed_keys,
            buf,
            indent,
            inner_depth,
            use_pretty,
        ) {
            first = false;
        } else {
            buf.truncate(save);
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

/// Array walk under an array replacer. Arrays ignore the PropertyList for their
/// own indices (all elements serialize), but objects reached through them are
/// still filtered by `allowed_keys`. undefined / function / Symbol elements
/// serialize as `null` (spec).
pub(crate) unsafe fn stringify_array_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));

    let arr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader);
    if arr.is_null() {
        buf.push_str("[]");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    if len == 0 {
        buf.push_str("[]");
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }
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
        if !serialize_property_array_replacer(
            elem,
            allowed_keys,
            buf,
            indent,
            inner_depth,
            use_pretty,
        ) {
            buf.push_str("null");
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

// ─── Build the PropertyList from an array replacer ──────────────────────────

/// Build the `PropertyList` from an array replacer per ECMA-262 25.5.2 step 5.b:
///
///   * a String entry is kept verbatim,
///   * a Number entry is coerced via `ToString`,
///   * a Number/String **wrapper object** (has `[[NumberData]]`/`[[StringData]]`)
///     is coerced via `ToString` (which invokes its `toString`),
///   * everything else (Boolean, null, undefined, Symbol, BigInt, plain object,
///     function) is ignored,
///
/// and duplicate keys are dropped, preserving first-seen order.
pub(crate) unsafe fn build_property_list(ptr: *const u8) -> Vec<String> {
    let arr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader);
    if arr.is_null() {
        return Vec::new();
    }
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut result: Vec<String> = Vec::new();
    let mut push_unique = |s: String, result: &mut Vec<String>| {
        if !result.iter().any(|k| *k == s) {
            result.push(s);
        }
    };
    for i in 0..len {
        let elem = *elements.add(i as usize);
        let bits = elem.to_bits();
        let tag = bits & 0xFFFF_0000_0000_0000;
        if tag == STRING_TAG {
            let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                push_unique(s.to_string(), &mut result);
            }
        } else if tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                push_unique(s.to_string(), &mut result);
            }
        } else if let Some(p) = extract_pointer(bits) {
            // GC_TYPE_STRING heap string entry.
            if !is_object_pointer(p) && gc_obj_type(p) == crate::gc::GC_TYPE_STRING {
                if let Some(s) = str_from_header(p as *const StringHeader) {
                    push_unique(s.to_string(), &mut result);
                }
                continue;
            }
            // Number/String wrapper object -> ToString(v) (invokes toString).
            // Boxed Boolean/BigInt/Symbol and plain objects are ignored.
            match crate::builtins::boxed_primitive_to_string_tag(elem) {
                Some("Number") | Some("String") => {
                    let s_ptr = crate::value::js_jsvalue_to_string(elem);
                    if let Some(s) = str_from_header(s_ptr) {
                        push_unique(s.to_string(), &mut result);
                    }
                }
                _ => {}
            }
        } else if tag == BIGINT_TAG
            || bits == TAG_NULL
            || bits == TAG_TRUE
            || bits == TAG_FALSE
            || bits == TAG_UNDEFINED
        {
            // Ignored entry types.
        } else {
            // Plain Number primitive -> ToString.
            push_unique(crate::string::js_format_f64(elem), &mut result);
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

    // A top-level Symbol serializes to undefined unless a function replacer
    // substitutes it (handled in the closure branch below). An array replacer
    // never substitutes the root value, so it stays undefined there too.
    if super::stringify::is_symbol_bits(value_bits) && !is_closure_value(replacer_f64.to_bits()) {
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

    // Determine spacer/indent. A boxed Number/String wrapper space argument is
    // first unwrapped to its primitive — spec ToNumber/ToString on an Object
    // `space` with [[NumberData]]/[[StringData]] (space-number-object,
    // space-string-object).
    let spacer_f64 = crate::builtins::boxed_primitive_json_value(spacer_f64).unwrap_or(spacer_f64);
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        // Spec: a string space is clamped to its first 10 code units
        // (space-string-range).
        indent_str = str_from_header(sp_ptr)
            .unwrap_or("")
            .chars()
            .take(10)
            .collect();
    } else if spacer_tag == crate::value::SHORT_STRING_TAG {
        // v0.5.213 SSO: spacer passed as inline short string
        // (e.g. `JSON.stringify(obj, null, "  ")` where "  " is 2
        // bytes — fits SSO). Decode into scratch, copy into the
        // indent_str buffer for the formatter. (SSO max is 5 bytes < 10,
        // so the clamp is a no-op here but kept uniform.)
        let jsval = JSValue::from_bits(spacer_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        indent_str = std::str::from_utf8(&scratch[..n])
            .unwrap_or("")
            .chars()
            .take(10)
            .collect();
    } else if spacer_bits == TAG_TRUE {
        indent_str = String::new();
    } else {
        // Number — ToInteger spaces, clamped to 0..=10 (space-number-float:
        // a non-integer / negative count truncates toward zero then clamps).
        let n = if spacer_f64.is_nan() {
            0.0
        } else {
            spacer_f64.trunc()
        };
        let n = n.clamp(0.0, 10.0) as usize;
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
        Some(build_property_list(arr_ptr))
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
        // SerializeJSONProperty("", {"": value}): apply toJSON to the root, then
        // serialize. The PropertyList filters every nested OBJECT (incl. objects
        // reached through arrays). A root that resolves to undefined / function /
        // Symbol yields `undefined`.
        let resolved = apply_to_json(value);
        let rbits = resolved.to_bits();
        if rbits == TAG_UNDEFINED
            || is_closure_value(rbits)
            || super::stringify::is_symbol_bits(rbits)
        {
            STRINGIFY_STACK.with(|s| s.borrow_mut().clear());
            restore_stringify_buf(buf);
            match saved_cache {
                Some(s) => restore_shape_cache(s),
                None => clear_shape_cache(),
            }
            STRINGIFY_DEPTH.with(|d| d.set(d.get() - 1));
            return TAG_UNDEFINED as i64;
        }
        stringify_resolved_array_replacer(
            resolved,
            allowed_keys,
            &mut buf,
            &indent_str,
            0,
            use_pretty,
        );
    } else if let Some(closure_ptr) = closure_replacer {
        // Function replacer. Per spec SerializeJSONProperty: toJSON FIRST, then
        // the replacer, then serialize — threading `indent_str` so the 3-arg
        // form (replacer + space) pretty-prints, matching Node.
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let value_after_to_json = apply_to_json_keyed(value, empty_key_f64);
        let root_holder = make_root_wrapper(value);
        let replaced_root =
            call_replacer(closure_ptr, empty_key_f64, value_after_to_json, root_holder);
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

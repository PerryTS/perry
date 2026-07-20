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

/// Call a replacer closure with (key, value) and return the result as f64.
///
/// Per ECMA-262 `SerializeJSONProperty`, the replacer is invoked with `this`
/// bound to the *holder* — the object/array that contains the property (or, for
/// the root value, the `{ "": value }` wrapper). Code that relies on the holder
/// (e.g. `this[key] instanceof Date`, or React's Flight reply encoder which
/// keys its already-serialized/dedup Maps by `this`) breaks without it — the
/// Flight encoder's `referenceMap.get(this)` then never finds the parent path,
/// so it re-serializes endlessly (Next.js standalone startup runaway). Mirror
/// the reviver path (`internalize_json_property`), which sets the implicit
/// `this` to the holder around the user-callback call.
#[inline]
pub(crate) unsafe fn call_replacer(
    replacer: *const crate::ClosureHeader,
    key_f64: f64,
    value_f64: f64,
    holder_f64: f64,
) -> f64 {
    let prev_this = crate::object::js_implicit_this_set(holder_f64);
    let result = crate::js_closure_call2(replacer, key_f64, value_f64);
    crate::object::js_implicit_this_set(prev_this);
    // The user callback may have installed/removed `Object.prototype.toJSON`
    // (#6009 fast-probe cache).
    super::invalidate_object_proto_tojson_state();
    result
}

/// NaN-box a heap object/array pointer as the holder `this` for `call_replacer`.
#[inline]
unsafe fn holder_value(ptr: *const u8) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

/// Build the spec root holder `{ "": value }` (ECMA-262 `JSON.stringify` step:
/// `Let wrapper be OrdinaryObjectCreate(...); CreateDataPropertyOrThrow(wrapper,
/// "", value)`), so the root replacer call sees `this` = the wrapper. GC-safe
/// (mirrors `apply_reviver_with_source`'s root-holder wrapper).
unsafe fn root_holder(value_f64: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let val_handle = scope.root_nanbox_f64(value_f64);
    let wrapper = crate::object::js_object_alloc(0, 1);
    let wrapper_handle = scope.root_raw_mut_ptr(wrapper);
    let empty = js_string_from_bytes(b"".as_ptr(), 0);
    let empty_handle = scope.root_string_ptr(empty);
    crate::object::js_object_set_field_by_name(
        wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        empty_handle.get_raw_const_ptr::<StringHeader>(),
        val_handle.get_nanbox_f64(),
    );
    holder_value(wrapper_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as *const u8)
}

/// Resolve `value.toJSON(key)` if `value` is an object with a callable
/// `toJSON` field, per spec `SerializeJSONProperty` step 2 (run BEFORE the
/// replacer). Mirrors the no-replacer path's `object_get_to_json`, which only
/// fires when the object actually has a closure-typed `toJSON` field. Returns
/// the (possibly substituted) value.
#[inline]
/// #5989: a real GC heap object pointer is in the low canonical VA range
/// (top 16 bits 0 or 1) and 8-byte aligned. A value whose extracted
/// "pointer" fails this is a corrupted / mis-encoded pointer, never a
/// dereferenceable `GcHeader` — feeding it to `gc_obj_type` SIGBUSes.
#[inline]
fn ptr_derefable(ptr: usize) -> bool {
    // Top-16-bits check: a real heap pointer sits in the low canonical VA
    // range (bits 48-63 are 0 or 1). On 32-bit targets (arm64_32/watchOS)
    // `usize` has no bits above 31, so this is vacuously true — and emitting
    // `ptr >> 48` there is a compile-time overflow. Gate it by pointer width.
    #[cfg(target_pointer_width = "64")]
    let high_bits_ok = (ptr >> 48) <= 1;
    #[cfg(not(target_pointer_width = "64"))]
    let high_bits_ok = true;
    high_bits_ok && ptr >= 0x10000 && (ptr & 0x7) == 0
}

unsafe fn apply_to_json(value: f64) -> f64 {
    let bits = value.to_bits();
    // A BigInt is a primitive, not a POINTER_TAG value — `extract_pointer`
    // below never matches it, so without this check `BigInt.prototype.toJSON`
    // is silently skipped for a top-level/replacer-walked BigInt (test262
    // JSON/stringify/value-bigint-order). Mirror the no-replacer path's
    // `serialize_bigint`, which already applies this.
    if (bits & 0xFFFF_0000_0000_0000) == BIGINT_TAG {
        if let Some(converted) = super::stringify::bigint_apply_to_json(value) {
            return converted;
        }
        return value;
    }
    if let Some(ptr) = extract_pointer(bits) {
        // A small-handle-band id (revocable-Proxy id, fetch/zlib/stream
        // handle) is never a dereferenceable heap pointer.
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return value;
        }
        // #5989: a mis-aligned or out-of-range pointer is a corrupted value, not
        // a real GC object; `gc_obj_type` below would deref its `GcHeader` and
        // SIGBUS. Guard by magnitude + 8-byte alignment (mirrors
        // `is_object_pointer`'s pre-load sanity) — skip the toJSON probe.
        if !ptr_derefable(ptr as usize) {
            return value;
        }
        // An array can carry an own `toJSON` expando too (test262
        // JSON/stringify/value-tojson-result) — checked via the array-named-
        // property side table, not `object_get_to_json` (arrays have no
        // `keys_array`). Buffer/TypedArray have no `GcHeader`, so exclude
        // them first — `gc_obj_type` would otherwise misread their raw bytes
        // as a GC_TYPE_ARRAY tag (see `stringify_value`'s matching guards).
        if gc_obj_type(ptr) == crate::gc::GC_TYPE_ARRAY
            && !crate::buffer::is_registered_buffer(ptr as usize)
            && crate::typedarray::lookup_typed_array_kind(ptr as usize).is_none()
        {
            if let Some(to_json_val) =
                super::stringify::array_get_to_json(ptr as *const crate::ArrayHeader)
            {
                return to_json_val;
            }
            return value;
        }
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
        // A BigInt surviving toJSON + the replacer is still unserializable
        // (ECMA-262 SerializeJSONProperty: the BigInt Type check runs
        // AFTER the replacer, unconditionally) — throw, don't silently
        // print its digits (test262 JSON/stringify/value-bigint-order).
        super::stringify::throw_bigint_serialize();
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
unsafe fn apply_to_json_keyed(value: f64, key_f64: f64) -> f64 {
    // SerializeJSONProperty step 2.b.i passes the property key to `toJSON`
    // (#5909, test262 JSON/stringify/value-tojson-arguments). The replacer walk
    // already carries the key here (empty String at the root, own key for a
    // member, stringified index for an element); record it so the shared
    // `object_get_to_json` / `array_get_to_json` / `bigint_apply_to_json` probes
    // hand it to `toJSON`.
    set_to_json_key_value(key_f64);
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
    // A POINTER_TAG / raw-pointer-shaped field can carry a small-handle-band id
    // (revocable-Proxy id, fetch/zlib/stream handle), never a dereferenceable
    // heap pointer. Next.js render reaches `JSON.stringify(value, replacer)`
    // over an object holding such an id; deref'ing `id - 8` as a GcHeader (or
    // its `keys_array` in `is_object_pointer`) segfaults. Classify by magnitude
    // FIRST and emit "null" (the field is not a serializable object), matching
    // the plain-stringify path's `is_handle_band` guards (#4904/#1843).
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        buf.push_str("null");
        return;
    }
    // #5989: a mis-aligned / out-of-range pointer is a corrupted value, not a
    // GC object — `gc_obj_type` (and the buffer/typed-array registry probes)
    // would deref its header and SIGBUS. Emit "null" (unserializable), matching
    // the handle-band fallback above, rather than crash the render.
    if !ptr_derefable(ptr as usize) {
        buf.push_str("null");
        return;
    }
    // #3857 follow-up: a boxed primitive wrapper returned by a replacer function
    // (`new Boolean(true)`, `new Number(n)`, `new String(s)`) must serialize as
    // its underlying primitive, not as `{}`. Must run before the GC-type dispatch
    // below, which would route it to stringify_object and emit `{}`.
    if let Some(prim) = crate::builtins::boxed_primitive_json_value(replaced) {
        if indent.is_empty() {
            stringify_value(prim, TYPE_UNKNOWN, buf);
        } else {
            stringify_value_pretty(prim, TYPE_UNKNOWN, buf, indent, depth);
        }
        return;
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
    // Issue #5111: TypedArray (no GcHeader on small ones) detection before
    // gc_obj_type, same rationale as the buffer check above.
    if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
        if indent.is_empty() {
            stringify_typed_array(ptr, buf);
        } else {
            stringify_typed_array_pretty(ptr, buf, indent, depth);
        }
        return;
    }
    // #6519: a nested WHATWG `URL` (the value the replacer passed through)
    // serializes as its `href` string. Its `searchParams` field points back at
    // the URL, so the generic object walk below would trip the circular-
    // structure detector. The href is a plain string, so the emit is identical
    // for compact and pretty walks. See `write_url_href_json`.
    if crate::url::is_url_object_shape(ptr as *mut crate::ObjectHeader) {
        super::stringify::write_url_href_json(ptr as *mut crate::ObjectHeader, buf);
        return;
    }
    match gc_obj_type(ptr) {
        crate::gc::GC_TYPE_ARRAY => {
            // #5989: an array grown past its capacity leaves a GC_FLAG_FORWARDED
            // stub at the OLD location (`js_array_grow`, issue #233) — its first
            // 8 bytes now hold the forwarding pointer to the grown array, so
            // reading them as length/capacity yields a bogus multi-GB "length".
            // A stale pre-grow pointer reaches here from the object graph (e.g.
            // React's RSC flight stores a `[key, value]` pair, then `pair[i] = …`
            // grows it while the payload still holds the pre-grow reference).
            // Follow the forwarding chain — as `clean_arr_ptr` does for every hot
            // accessor and as the plain-JSON path (`json/stringify.rs`) already
            // does — so the CURRENT grown array is serialized instead of the
            // defunct stub. Without this, the raw read produced a garbage length
            // that only the 10M cap below (emit "null") kept from SIGBUS-ing,
            // silently dropping the real data.
            let ptr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader) as *const u8;
            if ptr.is_null() {
                buf.push_str("null");
                return;
            }
            let len = (*(ptr as *const crate::ArrayHeader)).length;
            if len > 10_000_000 {
                // Defensive backstop for a genuinely mis-classified pointer.
                buf.push_str("null");
            } else {
                stringify_array_with_replacer_pretty(ptr, replacer, buf, indent, depth)
            }
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

    // GC-safety: same rooting discipline as the array variant above — the
    // replacer / toJSON / getter callbacks can trigger a moving GC, so every
    // raw pointer derived from `ptr` / `keys_arr` / `replacer` must be
    // re-derived from a rewritable root after each callback.
    let gc_scope = crate::gc::RuntimeHandleScope::new();
    let obj_root = gc_scope.root_raw_const_ptr(ptr);
    let replacer_root = gc_scope.root_raw_const_ptr(replacer);
    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;
    let Some(keys_arr) = super::stringify::object_keys_array_checked(obj) else {
        // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
        // reached here via a static TYPE_OBJECT hint). Node serializes those as
        // `{}`; walking the slot as an ArrayHeader would fault.
        buf.push_str("{}");
        return;
    };
    let keys_root = gc_scope.root_raw_const_ptr(keys_arr);
    let keys_len = (*keys_arr).length;

    // #5989 (mirrors the plain-stringify #307 fix): iterate up to keys_len, not
    // min(num_fields, keys_len). Objects with ≥9 fields cap field_count at the
    // inline alloc limit and store the overflow values in OVERFLOW_FIELDS, so
    // num_fields can be smaller than keys_len — the min() silently DROPPED
    // every overflow property from replacer serialization (react-server-dom's
    // flight props objects routinely exceed 8 keys).
    let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
    let actual_fields = keys_len;
    let use_pretty = !indent.is_empty();
    let inner_depth = depth + 1;
    // A function replacer only sees own ENUMERABLE keys (EnumerableOwnProperty
    // Names); gated for the common no-descriptor case.
    let filter_non_enum = crate::object::descriptors_in_use();
    buf.push('{');
    let mut first = true;
    for f in 0..actual_fields {
        let obj = obj_root.get_raw_const_ptr::<crate::ObjectHeader>();
        let keys_elements = (keys_root.get_raw_const_ptr::<u8>())
            .add(std::mem::size_of::<crate::ArrayHeader>())
            as *const f64;
        let fields_ptr = (obj_root.get_raw_const_ptr::<u8>())
            .add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
        let replacer = replacer_root.get_raw_const_ptr::<crate::ClosureHeader>();
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
        // resolve toJSON, then apply the replacer. Overflow slots (f >=
        // alloc_limit) route through js_object_get_field's OVERFLOW_FIELDS
        // lookup.
        let mut field_val = if f < alloc_limit {
            *fields_ptr.add(f as usize)
        } else {
            f64::from_bits(crate::object::js_object_get_field(obj, f).bits())
        };
        if filter_non_enum && f < keys_len {
            if let Some(gv) =
                crate::object::json_object_getter_value(obj, *keys_elements.add(f as usize))
            {
                field_val = gv;
            }
        }
        let field_after_to_json = apply_to_json_keyed(field_val, key_f64_for_replacer);
        let replaced = call_replacer(
            replacer,
            key_f64_for_replacer,
            field_after_to_json,
            holder_value(obj_root.get_raw_const_ptr::<u8>()),
        );
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

        // Write the key. Must go through the escaper, not a raw `push_str` —
        // a key can contain `"`/`\`/control characters (test262
        // JSON/stringify/value-string-escape-ascii pattern).
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

    // GC-safety (#gscmaster ~10-render crash): the replacer / toJSON callbacks
    // run arbitrary JS, which allocates — a minor GC mid-loop can PROMOTE
    // (move) this array, the replacer closure, or both. The raw `elements`
    // base and `replacer` pointer would then dangle: the next `*elements
    // .add(i)` read garbage f64s off the old nursery copy and the NaN-boxed
    // "pointer" they produced faulted in whatever shape-probe touched it
    // first (temporal::dispatch::get_property, url::search_params, …).
    // Root both in a RuntimeHandleScope (rewritten on evacuation) and
    // re-derive the raw pointers after every callback.
    let gc_scope = crate::gc::RuntimeHandleScope::new();
    let arr_root = gc_scope.root_raw_const_ptr(ptr);
    let replacer_root = gc_scope.root_raw_const_ptr(replacer);
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;

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
        let arr_base = arr_root.get_raw_const_ptr::<u8>();
        let elements = arr_base.add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
        let replacer = replacer_root.get_raw_const_ptr::<crate::ClosureHeader>();
        let elem = *elements.add(i as usize);
        // #5989: a sparse-array HOLE slot must surface to toJSON / the replacer
        // as `undefined` (spec: Get() on a missing index yields undefined),
        // never as the raw TAG_HOLE sentinel — the sentinel is an unrecognized
        // quiet-NaN bit pattern, so user code saw a number-NaN and e.g.
        // react-server-dom's flight encoder serialized "$NaN" where node emits
        // "$undefined" (Next.js sparse flightRouterState tuples:
        // `seg[4] = flags` on a length-2 array).
        let elem = if elem.to_bits() == crate::value::TAG_HOLE {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            elem
        };

        // Index key as a string for toJSON / replacer.
        let idx_str = i.to_string();
        let idx_ptr = js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let key_f64 = nanbox_string_f64(idx_ptr);

        let elem_after_to_json = apply_to_json_keyed(elem, key_f64);
        let replaced = call_replacer(
            replacer,
            key_f64,
            elem_after_to_json,
            holder_value(arr_root.get_raw_const_ptr::<u8>()),
        );
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

    // Call replacer with ("", root_value), `this` = the `{ "": value }` wrapper.
    // Per spec the holder wraps the ORIGINAL root value (so a root replacer's
    // `this[""]` observes the pre-`toJSON` value); only the replacer's value
    // argument is post-`toJSON`. CodeRabbit (PR #5438).
    let replaced_root = call_replacer(
        replacer,
        empty_key_f64,
        value_after_to_json,
        root_holder(value),
    );
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
    // Arbitrary user code ran since the last stringify, so the cached
    // `Object.prototype`-has-`toJSON` verdict must be recomputed too (#6009).
    if prior_depth == 0 {
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        super::invalidate_object_proto_tojson_state();
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
        // A small-handle-band id (revocable-Proxy id, fetch/zlib/stream handle)
        // is never a serializable heap value. Reading its ArrayHeader/keys_array
        // below (the `(*arr).length` probe and `is_object_pointer`) would deref
        // unmapped memory — Next.js render reaches here via the array-replacer
        // fall-through with a Proxy id in the `[0xF0000,0x100000)` band. Reject
        // by magnitude first and emit "null" (#4904/#1843 pattern).
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            buf.push_str("null");
            return;
        }
        // #2089: a Date is a NaN-boxed `DateCell` pointer — emit `toJSON()` (ISO
        // string, or `null` for an Invalid Date) per ECMA-262 25.5.2, before any
        // object/array deref of the small cell. The plain path has always done
        // this; the pretty path did not, so `JSON.stringify(new Date(), null, 2)`
        // walked the cell as a plain object and produced `""` instead of the ISO
        // string — silently corrupting every indented JSON file with a date in it.
        if crate::date::is_date_cell_addr(ptr as usize) {
            let s_ptr = crate::date::js_date_to_json(value);
            if let Some(s) = str_from_header(s_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
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
        // Issue #5111: TypedArray detection before gc_obj_type (see above).
        if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
            stringify_typed_array_pretty(ptr, buf, indent, depth);
            return;
        }
        // #2900: raw-JSON wrapper — emit stored text verbatim (pretty-print
        // output never indents a scalar, so no indentation is applied here
        // either).
        if let Some(raw) = super::raw_json_text_bytes(ptr) {
            buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
            return;
        }
        // A RegExp has no enumerable own properties, so Node serializes it as `{}`.
        // Perry's `RegExpHeader` is not an `ObjectHeader`, so without this the
        // generic object walk below read its internal slots as fields and emitted
        // `{"field0":null}`. Detected by the header magic (never a raw deref).
        if crate::regex::regex_header_has_magic(ptr as *const crate::regex::RegExpHeader) {
            buf.push_str("{}");
            return;
        }
        // #6519: a nested WHATWG `URL` must serialize as its `href` string, not
        // be walked as a plain object (its `searchParams` back-reference trips
        // the circular-structure detector). Mirrors the compact-path branch in
        // `stringify_object_inner`; see `write_url_href_json`.
        if crate::url::is_url_object_shape(ptr as *mut crate::ObjectHeader) {
            super::stringify::write_url_href_json(ptr as *mut crate::ObjectHeader, buf);
            return;
        }
        if matches!(
            gc_obj_type(ptr),
            crate::gc::GC_TYPE_MAP
                | crate::gc::GC_TYPE_SET
                | crate::gc::GC_TYPE_ERROR
                // A Promise has no enumerable own properties either — Node emits `{}`.
                // Perry's PromiseHeader is not an ObjectHeader, so the generic walk
                // below read its slots as fields (it fell all the way through to the
                // StringHeader fallback and emitted `""`).
                | crate::gc::GC_TYPE_PROMISE
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
    // Same deref-safety gate the plain path applies in `is_object_pointer`: the
    // `field_count` / `keys_array` reads below load straight through `ptr`, so an
    // in-range-but-unmapped garbage address (a denormal double that survived the
    // tag probes) SIGSEGVs here. Require a genuinely GC-tracked allocation first
    // and emit `null` otherwise, rather than faulting inside JSON.stringify.
    if !super::stringify::ptr_is_tracked_heap_object(ptr) {
        buf.push_str("null");
        return;
    }
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
    let Some(keys_arr) = super::stringify::object_keys_array_checked(obj) else {
        // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
        // reached here via a static TYPE_OBJECT hint). Node serializes those as
        // `{}`; walking the slot as an ArrayHeader would fault.
        buf.push_str("{}");
        return;
    };
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    // Iterate keys_len, not min(...): ≥9-field objects keep overflow values in
    // OVERFLOW_FIELDS (see the function-replacer walk above / plain #307 fix).
    let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
    let actual_fields = keys_len;
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
        let mut field_val = if f < alloc_limit {
            *fields_ptr.add(f as usize)
        } else {
            f64::from_bits(crate::object::js_object_get_field(obj, f).bits())
        };
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
        // Escape the key, not a raw `push_str` — see the compact path's
        // matching fix (test262 JSON/stringify/value-string-escape-ascii).
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
    // Same gate as `stringify_object_pretty`: this is the fall-through branch for
    // a pointer that failed the object probes, so a corrupted pointer lands here
    // and the `(*arr).length` read below would fault.
    if !super::stringify::ptr_is_tracked_heap_object(ptr) {
        buf.push_str("null");
        return;
    }
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
        // TAG_HOLE: sparse-array holes serialize as null, same as undefined.
        // A function element also serializes as `null` (JSON.stringify only drops
        // a function when it is an object *property*; in an array it becomes null).
        // Every other array path already did this — without it here, the pretty
        // printer fell through to `stringify_value_pretty`, which read the closure's
        // pointer bits as a string payload: `[function(){}]` came out as `[""]`
        // and, for most closures, dereferenced unmapped memory and segfaulted.
        if elem_bits == TAG_UNDEFINED
            || elem_bits == crate::value::TAG_HOLE
            || is_closure_value(elem_bits)
        {
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
    let Some(keys_arr) = super::stringify::object_keys_array_checked(obj) else {
        // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
        // reached here via a static TYPE_OBJECT hint). Node serializes those as
        // `{}`; walking the slot as an ArrayHeader would fault.
        buf.push_str("{}");
        return;
    };
    let keys_len = (*keys_arr).length;
    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let fields_ptr =
        (ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    // Iterate keys_len, not min(...): ≥9-field objects keep overflow values in
    // OVERFLOW_FIELDS (see the function-replacer walk above / plain #307 fix).
    let alloc_limit = std::cmp::max(num_fields, crate::object::INLINE_SLOT_FLOOR as u32);
    let actual_fields = keys_len;

    // Build a map of key_name -> field_value for the object. An own accessor
    // (`get key()`) holds no value in its raw slot, so resolve it through the
    // getter — matching the function-replacer walk (test262
    // replacer-array-duplicates, whose whitelisted key is a getter).
    let filter_non_enum = crate::object::descriptors_in_use();
    let mut field_map: Vec<(String, f64)> = Vec::new();
    for f in 0..actual_fields {
        let mut field_val = if f < alloc_limit {
            *fields_ptr.add(f as usize)
        } else {
            f64::from_bits(crate::object::js_object_get_field(obj, f).bits())
        };
        if filter_non_enum && f < keys_len {
            if let Some(gv) =
                crate::object::json_object_getter_value(obj, *keys_elements.add(f as usize))
            {
                field_val = gv;
            }
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
                write_escaped_string(buf, allowed_key);
                buf.push_str(": ");
                stringify_value_with_array_replacer(
                    *field_val,
                    allowed_keys,
                    buf,
                    indent,
                    inner_indent_count,
                    true,
                );
            } else {
                write_escaped_string(buf, allowed_key);
                buf.push(':');
                stringify_value_with_array_replacer(
                    *field_val,
                    allowed_keys,
                    buf,
                    indent,
                    depth,
                    false,
                );
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

/// Serialize one value while an array replacer (PropertyList) is active. Per
/// spec the key whitelist applies to EVERY nested object in the tree, not just
/// the top-level one — so a plain object recurses through
/// `stringify_object_with_array_replacer` and an array through
/// `stringify_array_with_array_replacer`. Scalars, boxed primitive wrappers,
/// buffers, Map/Set, etc. defer to the ordinary (whitelist-agnostic) serializer.
pub(crate) unsafe fn stringify_value_with_array_replacer(
    val: f64,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    let bits = val.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        if !crate::buffer::is_registered_buffer(ptr as usize)
            && crate::builtins::boxed_primitive_json_value(val).is_none()
        {
            match gc_obj_type(ptr) {
                crate::gc::GC_TYPE_ARRAY => {
                    stringify_array_with_array_replacer(
                        ptr,
                        allowed_keys,
                        buf,
                        indent,
                        depth,
                        use_pretty,
                    );
                    return;
                }
                crate::gc::GC_TYPE_OBJECT if is_object_pointer(ptr) => {
                    stringify_object_with_array_replacer(
                        ptr,
                        allowed_keys,
                        buf,
                        indent,
                        depth,
                        use_pretty,
                    );
                    return;
                }
                _ => {}
            }
        }
    }
    if use_pretty {
        stringify_value_pretty(val, TYPE_UNKNOWN, buf, indent, depth);
    } else {
        stringify_value(val, TYPE_UNKNOWN, buf);
    }
}

/// Array walk under an active array replacer. Every element is serialized (the
/// PropertyList only filters object keys, never array indices); undefined /
/// function elements become `null`. Nested objects/arrays recurse carrying the
/// same key whitelist.
pub(crate) unsafe fn stringify_array_with_array_replacer(
    ptr: *const u8,
    allowed_keys: &[String],
    buf: &mut String,
    indent: &str,
    depth: usize,
    use_pretty: bool,
) {
    // #5989: follow GC_FLAG_FORWARDED array-growth stubs (`js_array_grow`, issue
    // #233) so a stale pre-grow pointer serializes the current grown array —
    // mirrors the resolution in `dispatch_pointer_with_replacer`'s array arm.
    let ptr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader) as *const u8;
    if ptr.is_null() {
        buf.push_str("null");
        return;
    }
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
        let elem_bits = elem.to_bits();
        // TAG_HOLE: sparse-array holes serialize as null, same as undefined.
        if elem_bits == TAG_UNDEFINED
            || elem_bits == crate::value::TAG_HOLE
            || is_closure_value(elem_bits)
        {
            buf.push_str("null");
        } else {
            stringify_value_with_array_replacer(
                elem,
                allowed_keys,
                buf,
                indent,
                inner_depth,
                use_pretty,
            );
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

// ─── Extract array of strings from a JSValue array ──────────────────────────

pub(crate) unsafe fn extract_string_array(ptr: *const u8) -> Vec<String> {
    let arr = ptr as *const crate::ArrayHeader;
    let len = (*arr).length;
    let elements = (arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut result: Vec<String> = Vec::new();
    for i in 0..len {
        let elem = *elements.add(i as usize);
        // Per ECMA-262 SerializeJSONObject: a PropertyList item is kept when it
        // is a String, a Number, or a String/Number wrapper object (Numbers and
        // wrappers coerce to their canonical string form). Duplicate names are
        // dropped — first occurrence wins.
        if let Some(key) = json_property_list_key(elem) {
            if !result.contains(&key) {
                result.push(key);
            }
        }
    }
    result
}

/// Resolve one PropertyList element to its key name (or `None` to skip it).
/// String/Number values and `Number`/`String` wrapper objects qualify; all
/// other types (booleans, null, plain objects, symbols, …) are skipped.
unsafe fn json_property_list_key(elem: f64) -> Option<String> {
    let bits = elem.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == STRING_TAG {
        let str_ptr = (bits & POINTER_MASK) as *const StringHeader;
        return str_from_header(str_ptr).map(|s| s.to_string());
    }
    if tag == crate::value::SHORT_STRING_TAG {
        let jsval = JSValue::from_bits(bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        return std::str::from_utf8(&scratch[..n])
            .ok()
            .map(|s| s.to_string());
    }
    // Heap-string literal pointer (denormal-range bits, never a real number).
    if is_raw_pointer(bits) {
        let str_ptr = bits as *const StringHeader;
        return str_from_header(str_ptr).map(|s| s.to_string());
    }
    let jsval = JSValue::from_bits(bits);
    if jsval.is_int32() {
        return Some(jsval.as_int32().to_string());
    }
    if jsval.is_number() {
        let p = crate::string::js_number_to_string(elem);
        return str_from_header(p).map(|s| s.to_string());
    }
    // String / Number wrapper objects → ToString(v). This runs the object's
    // own `toString`/`valueOf` (ToPrimitive), not a raw read of [[*Data]] — so
    // `new Number(10)` with an overridden `toString` contributes that string,
    // not "10" (test262 replacer-array-{number,string}-object).
    if let Some((cid, _)) = crate::builtins::boxed_primitive_payload(elem) {
        const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
        const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
        if cid == CLASS_ID_BOXED_NUMBER || cid == CLASS_ID_BOXED_STRING {
            let s = crate::value::js_jsvalue_to_string(elem);
            return str_from_header(s).map(|s| s.to_string());
        }
    }
    None
}

/// Detect whether a NaN-boxed value is an array (not an object).
#[inline]
pub(crate) unsafe fn is_array_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        // A small-handle-band id is neither array nor object; deref'ing its
        // ArrayHeader would fault (#4904/#1843).
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return false;
        }
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

/// Spec §25.5.2 step 7a: truncate a spacer string to its first 10 UTF-16 code
/// units. Walks `char_indices` to stay on Rust char boundaries.
///
/// Known deviation: when the 10th UTF-16 unit would be the high surrogate of a
/// supplementary-plane character (e.g. spacer `"123456789😀"`), we stop before
/// the character rather than emitting a lone surrogate. The spec's WTF-16
/// semantics would produce a lone high surrogate at position 10. Perry does not
/// support lone surrogates in its WTF-8 string representation (known gap), so
/// this deviation is accepted. The test262 `space-string-range.js` test uses
/// only ASCII spacers and passes regardless.
fn truncate_to_10_utf16_units(s: &str) -> String {
    let mut utf16_units = 0usize;
    for (byte_idx, c) in s.char_indices() {
        let n = c.len_utf16();
        if utf16_units + n > 10 {
            return s[..byte_idx].to_string();
        }
        utf16_units += n;
    }
    s.to_string()
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

    // A top-level Symbol is likewise unserializable — return undefined
    // (test262 JSON/stringify/value-symbol).
    if is_symbol_value(value_bits) {
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
    let _value_bits = value.to_bits();

    // Determine spacer/indent. A `Number`/`String` wrapper object spacer
    // (`JSON.stringify(v, null, new Number(2))`) is coerced per ECMA-262
    // 25.5.2.1: a Number wrapper via ToNumber, a String wrapper via ToString —
    // both of which run the object's own `valueOf`/`toString` (so an overridden
    // `valueOf` is observed, and a throwing one propagates). Without this the
    // NaN-boxed pointer would be read as a raw indent count (test262
    // space-{number,string}-object).
    let spacer_f64 = match crate::builtins::boxed_primitive_payload(spacer_f64) {
        Some((cid, _)) => {
            const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
            const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
            if cid == CLASS_ID_BOXED_NUMBER {
                crate::builtins::js_number_coerce(spacer_f64)
            } else if cid == CLASS_ID_BOXED_STRING {
                let s = crate::value::js_jsvalue_to_string(spacer_f64);
                f64::from_bits(STRING_TAG | (s as u64 & POINTER_MASK))
            } else {
                spacer_f64
            }
        }
        None => spacer_f64,
    };
    let indent_str: String;
    let spacer_bits = spacer_f64.to_bits();
    let spacer_tag = spacer_bits & 0xFFFF_0000_0000_0000;
    if spacer_bits == TAG_NULL || spacer_bits == TAG_UNDEFINED || spacer_bits == TAG_FALSE {
        indent_str = String::new();
    } else if spacer_tag == STRING_TAG {
        let sp_ptr = (spacer_bits & POINTER_MASK) as *const StringHeader;
        // Spec §25.5.2 step 7a: only first 10 UTF-16 code units of string space are used.
        let full = str_from_header(sp_ptr).unwrap_or("");
        indent_str = truncate_to_10_utf16_units(full);
    } else if spacer_tag == crate::value::SHORT_STRING_TAG {
        // v0.5.213 SSO: spacer passed as inline short string
        // (e.g. `JSON.stringify(obj, null, "  ")` where "  " is 2
        // bytes — fits SSO). Decode into scratch, copy into the
        // indent_str buffer for the formatter.
        let jsval = JSValue::from_bits(spacer_bits);
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut scratch);
        // SSO strings are at most 5 bytes, always ≤ 10 UTF-16 code units; no truncation needed.
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
    // Arbitrary user code ran since the last stringify, so the cached
    // `Object.prototype`-has-`toJSON` verdict must be recomputed too (#6009).
    if prior_depth == 0 {
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        super::invalidate_object_proto_tojson_state();
    }
    let saved_cache = if prior_depth > 0 {
        Some(take_shape_cache())
    } else {
        None
    };
    let mut buf = take_stringify_buf();

    if let Some(ref allowed_keys) = array_replacer {
        // Array replacer (PropertyList): the key whitelist applies recursively
        // to EVERY nested object, whether the root is an object or an array
        // (`JSON.stringify([1, {a:2}], [])` → "[1,{}]"). The shared dispatcher
        // routes objects/arrays into the whitelist-aware walks and everything
        // else into the plain serializer.
        stringify_value_with_array_replacer(
            value,
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
        let replaced_root = call_replacer(
            closure_ptr,
            empty_key_f64,
            value_after_to_json,
            root_holder(value_after_to_json),
        );
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
    } else {
        // No replacer. Pre-resolve the ROOT value's own `toJSON` here (same
        // `apply_to_json_keyed` the function-replacer branch above uses) so a
        // root whose `toJSON` returns `undefined` (or a function/Symbol) can
        // short-circuit to `JSON.stringify`'s `undefined` return — the
        // buffer-based walk below has no way to signal that once it starts
        // writing bytes (test262 JSON/stringify/value-tojson-arguments,
        // value-tojson-result's `arr.toJSON = () => {}` case). Arm the
        // one-shot suppression guard so the walk below doesn't re-invoke
        // `toJSON` on the same (already-resolved) root value.
        let empty_str = js_string_from_bytes(b"".as_ptr(), 0);
        let empty_key_f64 = nanbox_string_f64(empty_str);
        let value_after_to_json = apply_to_json_keyed(value, empty_key_f64);
        let after_bits = value_after_to_json.to_bits();
        if after_bits == TAG_UNDEFINED
            || is_closure_value(after_bits)
            || is_symbol_value(after_bits)
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
        // Only arm suppression when `toJSON` actually substituted a
        // different value — arming unconditionally would suppress a
        // legitimate nested `toJSON` (e.g. `{a:1, b:{toJSON(){...}}}`) any
        // time the root object itself has no closure field to consume the
        // flag on its own dispatch (a plain data object never calls
        // `object_get_to_json` at all, so the flag would otherwise leak
        // straight through to the first real nested `toJSON`).
        if after_bits != value.to_bits() {
            arm_to_json_result_guard(value_after_to_json);
        }
        if use_pretty {
            // No replacer, but has spacer — pretty-print
            stringify_value_pretty(value_after_to_json, TYPE_UNKNOWN, &mut buf, &indent_str, 0);
        } else {
            // Plain stringify
            stringify_value(value_after_to_json, TYPE_UNKNOWN, &mut buf);
        }
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
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

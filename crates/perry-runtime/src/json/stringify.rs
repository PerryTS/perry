//! Core `JSON.stringify` traversal: scalar/object/array/buffer emitters,
//! shape-template fast path, and number/escape-string formatters.
//!
//! Public FFI entry points (`js_json_stringify`, etc.) live in
//! `stringify_api.rs`; this file is the shared traversal those entry points
//! and the replacer path call into.

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};

pub(crate) use super::stringify_scalars::{
    bigint_apply_to_json, serialize_bigint, throw_bigint_serialize, write_escaped_string,
    write_heap_string, write_number, write_short_string,
};
// The homogeneous-array shape template lives in a sibling (file-size gate);
// both the object and the array emitter below drive it.
use super::stringify_shape_template::{build_shape_prefix_template, try_emit_shape_element};
// The object walk lives in a sibling (file-size gate); every dispatch here
// reaches it.
pub(crate) use super::stringify_object::stringify_object_inner;

// ─── JSON.stringify ───────────────────────────────────────────────────────────

pub(crate) const MAX_STRINGIFY_NESTING_DEPTH: usize = 1_000;

#[inline]
pub(crate) fn check_stringify_nesting_depth(depth: usize) {
    if depth > MAX_STRINGIFY_NESTING_DEPTH {
        let message = "JSON.stringify: value nested deeper than 1000 levels";
        let message_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let error = crate::error::js_rangeerror_new(message_ptr);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64));
    }
}

#[inline]
/// True only when `ptr` is an address the GC actually tracks — an arena
/// allocation (nursery/old/longlived per the page-map) or a registered malloc
/// object. Both checks are dereference-FREE (page-metadata / registry lookups),
/// so this rejects a forged pointer before any field read.
///
/// A magnitude check alone is not enough here: a type-erased `JSON.stringify`
/// walk can reach `is_object_pointer` with a primitive `number` whose f64 bits
/// land INSIDE the heap magnitude window (~2–5 TB, e.g. `0x0000_0347_0000_0000`)
/// yet point at an UNMAPPED page — `is_valid_obj_ptr` accepts it and the
/// subsequent `crate::object::object_keys_array(obj)` read then SIGSEGVs. Mirrors the `path.rs` /
/// `current_heap_header_for_user_ptr` Unknown→malloc rule.
pub unsafe fn ptr_is_tracked_heap_object(ptr: *const u8) -> bool {
    let addr = ptr as usize;
    if crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    // Arena-resident (nursery/old/longlived) is decided by the page map alone —
    // no header read at all.
    if !matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Unknown
    ) {
        return true;
    }
    // Otherwise the only way to be tracked is a registered malloc object, which
    // requires a real GcHeader at `addr - GC_HEADER_SIZE`. Route through
    // `addr_class::try_read_gc_header` rather than re-casting: it layers the
    // magnitude guard AND the small-buffer-slab guard on top of the handle-band
    // check. A slab address is heap-plausible but carries NO header, so a raw
    // cast would read the previous slab entry's data bytes as a fake header.
    match crate::value::addr_class::try_read_gc_header(addr) {
        Some(header) => crate::gc::gc_malloc_header_is_tracked(header),
        None => false,
    }
}

/// Decode an object keys-array slot without assuming heap-string storage.
/// Dynamic SSO writes keep short property names inline; module-slot shapes may
/// still carry the legacy validated raw `StringHeader` pointer form.
#[inline]
pub(super) unsafe fn object_key_str<'a>(
    key_bits: u64,
    sso: &'a mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<&'a str> {
    let key = JSValue::from_bits(key_bits);
    if let Some(bytes) = crate::string::js_string_key_bytes(key, sso) {
        return std::str::from_utf8(bytes).ok();
    }
    if ptr_is_tracked_heap_object(key_bits as *const u8) {
        return str_from_header(key_bits as *const StringHeader);
    }
    None
}

pub(crate) unsafe fn is_object_pointer(ptr: *const u8) -> bool {
    // A small-handle-band id (revocable-Proxy id, fetch/zlib/stream handle) is
    // never a real ObjectHeader; reading its `keys_array` field would deref
    // unmapped memory (#4904/#1843 pattern). Reject by magnitude before any load.
    // The upper end matters too: a plausible-but-unmapped in-range garbage address
    // (a primitive number whose f64 bits land in the heap window) would likewise
    // SIGSEGV on the `keys_array` read below — require a genuinely GC-tracked
    // allocation before dereferencing anything.
    if !ptr_is_tracked_heap_object(ptr) {
        return false;
    }
    let obj = ptr as *const crate::ObjectHeader;
    let potential_keys_ptr = crate::object::object_keys(obj).arr() as u64;
    // `ptr` being GC-tracked only proves the *allocation* is real — not that it is
    // an `ObjectHeader`. A Promise / WeakMap / ArrayBuffer / any other GC layout
    // reaches here too (e.g. via a static TYPE_OBJECT hint), and then this slot is
    // some unrelated field read as a pointer. The alignment/magnitude heuristic
    // below is far too weak to catch that — a garbage word like 0x223af100 is
    // 8-aligned and in range, and the `(*keys_arr).length` load below then faults.
    // Require the *keys array itself* to be a tracked allocation before loading it.
    let looks_like_valid_pointer = potential_keys_ptr > 0x10000
        && (potential_keys_ptr & 0x7) == 0
        && ptr_is_tracked_heap_object(potential_keys_ptr as *const u8);

    if looks_like_valid_pointer {
        let keys_arr_view = crate::object::object_keys(obj);
        let keys_arr = keys_arr_view.arr();
        let keys_len = keys_arr_view.count();
        let keys_cap = (*keys_arr).capacity;
        let field_count = crate::object::object_live_slot_count(obj);
        // keys_len is authoritative — the logical property count. field_count
        // can be EITHER less than keys_len (parser-built objects with ≥9
        // fields cap field_count at the inline alloc_limit; closes #307;
        // overflow values live in OVERFLOW_FIELDS — see object.rs:32) OR
        // greater than keys_len (pre-allocated objects like
        // `js_object_alloc(0, 8)` for 2 actual keys). Both shapes are real
        // objects worth stringifying; just sanity-check both fields are
        // within reasonable bounds.
        // Previously caps were `< 1000` — any object with 1000+ keys
        // failed the check and `JSON.stringify` emitted "null". Raised
        // to 10M which still catches a corrupted ObjectHeader (first-
        // fields bytes reading as 0x4059... — orders of magnitude
        // above 10M) but allows realistic object sizes through.
        keys_len <= keys_cap && keys_len > 0 && keys_cap < 10_000_000 && field_count < 10_000_000
    } else {
        false
    }
}

/// True when `ptr` is a valid object with NO own (enumerable) keys: either a
/// null `keys_array` (`{}`, `Object.fromEntries([])`) or a valid-but-empty one
/// — the shape of a `class C {}` instance or a class whose only members are
/// prototype methods/getters (those are not own properties). Such objects
/// serialize as `{}`, never `null` or an array. Used by the value dispatchers
/// to disambiguate an empty object from a corrupted pointer after the
/// `keys_len > 0` `is_object_pointer` probe fails.
pub(crate) unsafe fn object_has_no_own_keys(ptr: *const u8) -> bool {
    // Same deref-safety gate as `is_object_pointer`: this is called on the same
    // value right after that probe returns false (to tell an empty object apart
    // from a corrupted pointer), so an unmapped in-range garbage address must be
    // rejected here too before the `keys_array` field read.
    if !ptr_is_tracked_heap_object(ptr) {
        return false;
    }
    let keys_view = crate::object::object_keys(ptr as *const crate::ObjectHeader);
    let keys = keys_view.arr();
    if keys.is_null() {
        return true;
    }
    // Same reasoning as `is_object_pointer`: a tracked `ptr` does not prove an
    // `ObjectHeader` layout, so this slot may be an unrelated field. Require the
    // keys array itself to be a tracked allocation before loading its length.
    if !ptr_is_tracked_heap_object(keys as *const u8) {
        return false;
    }
    keys_view.count() == 0
}

/// The object's keys array, but only when it is genuinely a tracked heap
/// allocation. A GC allocation that is not an `ObjectHeader` (a Promise, WeakMap,
/// ArrayBuffer, ...) can still reach the object walkers — via a static
/// `TYPE_OBJECT` hint from codegen — and its bytes at this offset are some other
/// field. Loading that as an `ArrayHeader` faults. Walkers bail to `{}` on `None`.
pub(super) unsafe fn object_keys_array_checked(
    obj: *const crate::ObjectHeader,
) -> Option<crate::object::ObjectKeys> {
    tracked_keys(crate::object::object_keys(obj))
}

/// [`object_keys_array_checked`] for keys the caller already derived.
pub(super) unsafe fn tracked_keys(
    view: crate::object::ObjectKeys,
) -> Option<crate::object::ObjectKeys> {
    let keys = view.arr() as *const crate::ArrayHeader;
    if keys.is_null() || !ptr_is_tracked_heap_object(keys as *const u8) {
        return None;
    }
    Some(view)
}

/// Check if a NaN-boxed value is a closure (function).
#[inline]
pub(crate) unsafe fn is_closure_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        // #2154 / #4904 — a POINTER_TAG field can be a native *handle id* (a
        // small integer, e.g. an `http.Agent` stored in an options object),
        // not a real heap pointer. Reading the GC header of
        // such a value segfaults (stringifying `{ agent, lookup: () => {} }`
        // crashed exactly here). Skip the low-memory guard range, matching
        // the has-function probe below.
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return false;
        }
        // The kind is the GC header's type byte.
        crate::closure::closure_kind_probe(ptr as usize)
    } else {
        false
    }
}

/// Check if a NaN-boxed value is a Symbol. Per ECMA-262 `SerializeJSONProperty`
/// step 11, a Symbol is unserializable — treated exactly like a function
/// (omitted from objects, `null` in arrays, `undefined` at the top level).
/// Symbols are POINTER_TAG'd (`js_is_symbol`) but allocated as `GC_TYPE_STRING`
/// (see `symbol.rs::alloc_symbol`) — without this check, a Symbol reaching the
/// generic `GC_TYPE_STRING` dispatch has its `SymbolHeader` bytes misread as a
/// `StringHeader` (test262 JSON/stringify/value-symbol).
#[inline]
pub(crate) unsafe fn is_symbol_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return false;
        }
        crate::symbol::is_registered_symbol(ptr as usize)
    } else {
        false
    }
}

/// Check if an object has a `toJSON` method — resolved as an OWN property *or*
/// anywhere on its prototype / class-method chain. If a callable `toJSON` is
/// found, invoke it with `this = the object` (empty-string key arg, per the
/// rest of Perry's JSON suite) and return its result as f64. Returns `None`
/// when no callable `toJSON` exists (the caller then serializes the object
/// normally).
///
/// `SerializeJSONProperty` (ECMA-262 §25.5.2.2 step 2) calls `value.toJSON(key)`
/// whenever `toJSON` resolves to a callable, regardless of whether it's an own
/// property or inherited. Effect's `Inspectable` and any plain `class { toJSON()
/// {…} }` define `toJSON` on the prototype, so an own-key-only walk (the
/// pre-#321 behaviour) silently dropped it. We mirror the object→string
/// coercion fix (#2102, `value/to_string.rs`) and the inherited-method dispatch
/// (#1969/#1982): resolve via `js_object_get_field_by_name` (own + prototype),
/// rebind `this` to the receiver with `clone_closure_rebind_this`, and call
/// through the canonical `js_native_call_value` dispatcher.
#[inline]
pub(crate) unsafe fn object_get_to_json(ptr: *const u8) -> Option<f64> {
    // One-shot suppression: this object is itself the result of a `toJSON`
    // call, so per spec we serialize its own fields WITHOUT re-invoking
    // `toJSON`. Consume the flag and bail.
    if SUPPRESS_NEXT_TO_JSON.with(|c| c.replace(false)) {
        return None;
    }
    // Only resolve `toJSON` on a genuine plain object / class instance
    // (`GC_TYPE_OBJECT`). Map/Set (`GC_TYPE_MAP`/`GC_TYPE_SET`), buffers,
    // typed arrays, errors, regexes etc. have a DIFFERENT heap layout —
    // `js_object_get_field_by_name` would mis-read their internals as an
    // ObjectHeader keys/fields region and segfault (a `new Map()` reaches the
    // catch-all object path in `stringify_value`). Those types don't carry a
    // user-visible `toJSON` anyway, so bail to normal serialization. Mirrors
    // the existing `gc_obj_type == GC_TYPE_OBJECT && !is_registered_buffer`
    // guard the replacer path already applies before calling this helper.
    if gc_obj_type(ptr) != crate::gc::GC_TYPE_OBJECT
        || crate::buffer::is_registered_buffer(ptr as usize)
    {
        return None;
    }
    // #6009 fast path: when direct reads prove no `toJSON` can resolve
    // anywhere on this object's lookup chain, skip the generic
    // `js_object_get_field_by_name` dispatch (whose miss path recursively
    // re-enters itself through the subclass/prototype fallbacks) and all the
    // per-probe allocations below.
    if to_json_definitely_absent(ptr) {
        return None;
    }
    // `js_object_get_field_by_name` expects a raw (masked) heap pointer for the
    // ordinary-object path; the receiver `this` is the same value NaN-boxed
    // with POINTER_TAG.
    let recv = f64::from_bits(make_pointer_bits(ptr));
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_handle = scope.root_nanbox_f64(recv);

    let key = crate::string::canonical_key(b"toJSON");
    let key_handle = scope.root_string_ptr(key);

    let method = key_handle.with_const_ptr(|key: *const crate::string::StringHeader| {
        crate::object::native_get::get_by_canonical_key(recv_handle.get_nanbox_f64(), key)
    });

    // Only treat it as toJSON if it actually resolved to a callable closure
    // (POINTER_TAG + closure). A plain object with no `toJSON`, or a `toJSON`
    // data field that isn't a function, returns `None` → serialize normally.
    let method_bits = method.to_bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return None;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return None;
    }

    // Rebind `this` to the receiver. For an OWN method or a class-instance
    // bound-method closure this is a correct no-op; for an inherited
    // `Object.create(proto)` method whose reserved `this` slot was baked to the
    // prototype at construction, this restores the proper receiver (#1982).
    let recv = recv_handle.get_nanbox_f64();
    let bound = crate::closure::clone_closure_rebind_this(method_bits, recv);

    // Per spec (§25.5.2.2 step 2.b.i), `toJSON(key)` receives the property key
    // of the value being serialized — the empty String at the root, the own key
    // for an object member, the stringified index for an array element (#5909,
    // test262 JSON/stringify/value-tojson-arguments). The serialization loops
    // record it in `TO_JSON_KEY` before recursing here.
    let key_f64_arg = current_to_json_key_arg();

    let result = crate::closure::native_call_value_this(
        f64::from_bits(bound),
        crate::closure::JsThis::from_f64(recv_handle.get_nanbox_f64()),
        &key_f64_arg,
        1,
    );
    // The user callback may have installed/removed `Object.prototype.toJSON`.
    invalidate_object_proto_tojson_state();
    Some(result)
}

/// Check if an array has a callable `toJSON` method. An own expando lives in
/// the array-named-property side table because `ArrayHeader` has no
/// `keys_array`; a user-installed `Object.prototype.toJSON` is resolved
/// through the ordinary Array/Object prototype chain. Per ECMA-262 §25.5.2.2,
/// `SerializeJSONProperty` step
/// 2 applies to ANY object, including arrays, BEFORE the `IsArray` check
/// (step 10) that would otherwise route straight into `SerializeJSONArray`
/// (test262 JSON/stringify/value-tojson-result,
/// value-tojson-array-circular). Returns `None` when there's no callable
/// `toJSON` (the caller then serializes the array's elements normally).
#[inline]
pub(crate) unsafe fn array_get_to_json(arr: *const crate::ArrayHeader) -> Option<f64> {
    // One-shot suppression — see `object_get_to_json`.
    if SUPPRESS_NEXT_TO_JSON.with(|c| c.replace(false)) {
        return None;
    }
    let method = match crate::array::array_named_property_get_by_name(arr, "toJSON") {
        Some(method) => method,
        None => {
            // Keep the common array path allocation-free. The signature probe
            // directly detects Object.prototype key changes; only a positive
            // verdict pays for the full prototype lookup and key allocation.
            if !super::stringify_tojson_probe::object_proto_may_have_to_json() {
                return None;
            }
            f64::from_bits(
                crate::object::array_prototype_property_value("toJSON", arr as usize)?.bits(),
            )
        }
    };
    let method_bits = method.to_bits();
    if (method_bits & 0xFFFF_0000_0000_0000) != POINTER_TAG {
        return None;
    }
    let method_ptr = (method_bits & POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(method_ptr) {
        return None;
    }
    let recv = f64::from_bits(make_pointer_bits(arr as *const u8));
    let scope = crate::gc::RuntimeHandleScope::new();
    let recv_handle = scope.root_nanbox_f64(recv);
    // `toJSON(key)` receives the property key of this array value (#5909).
    let key_f64_arg = current_to_json_key_arg();
    let result = crate::closure::native_call_value_this(
        f64::from_bits(method_bits),
        crate::closure::JsThis::from_f64(recv_handle.get_nanbox_f64()),
        &key_f64_arg,
        1,
    );
    // The user callback may have installed/removed `Object.prototype.toJSON`.
    invalidate_object_proto_tojson_state();
    Some(result)
}

/// Serialize the RESULT of a `toJSON` call. Per ECMA-262 §25.5.2.2, `toJSON`
/// runs at most once per value: the returned value is then serialized as an
/// ordinary object/array WITHOUT re-invoking `toJSON` on it (only its child
/// properties get their own `toJSON` applied). When the result is an OBJECT
/// *or* ARRAY we arm the one-shot `SUPPRESS_NEXT_TO_JSON` guard so the
/// result's own probe (`object_get_to_json` / `array_get_to_json`) is
/// skipped, then always disarm it afterward so a result that never reaches
/// either probe (a plain `class_id == 0` literal with no own `toJSON` field)
/// can't leak the flag onto an unrelated later object/array.
#[inline]
pub(crate) unsafe fn arm_to_json_result_guard(result: f64) {
    if let Some(res_ptr) = extract_pointer(result.to_bits()) {
        let ty = gc_obj_type(res_ptr);
        if (ty == crate::gc::GC_TYPE_OBJECT || ty == crate::gc::GC_TYPE_ARRAY)
            && !crate::buffer::is_registered_buffer(res_ptr as usize)
        {
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(true));
        }
    }
}

/// Serialize an `Error`'s own ENUMERABLE properties, the way node does.
///
/// Errors do not have the JSObject keys/values layout, so they cannot go
/// through `stringify_object` — but they are still ordinary property bearers to
/// an observer. `exotic_own_keys(.., enumerable_only = true)` is the same
/// enumeration `Object.keys` uses on an error, so JSON output and `Object.keys`
/// cannot disagree.
///
/// `depth` is `Some` when called from the depth-tracking variant, so nested
/// values keep the cycle/recursion budget of the caller.
unsafe fn stringify_error_own_props(ptr: *const u8, buf: &mut String, depth: Option<u32>) {
    let ptr = ptr as usize;
    use crate::object::exotic_expando::{exotic_get_own_property, exotic_own_keys, ExoticKind};
    let keys = exotic_own_keys(ExoticKind::Error, ptr, true);
    buf.push('{');
    let mut first = true;
    for key in keys {
        let Some(v) = exotic_get_own_property(
            ptr,
            ExoticKind::Error,
            &key,
            f64::from_bits(bits_of_ptr(ptr)),
        ) else {
            continue;
        };
        // `undefined` own properties are omitted from objects, per JSON.stringify.
        if v.to_bits() == crate::value::TAG_UNDEFINED {
            continue;
        }
        if !first {
            buf.push(',');
        }
        first = false;
        write_escaped_string(buf, &key);
        buf.push(':');
        match depth {
            Some(d) => stringify_value_depth(v, 0, buf, d + 1),
            None => stringify_value(v, 0, buf),
        }
    }
    buf.push('}');
}

/// NaN-box `ptr` back into the pointer value an exotic `[[Get]]` wants as its
/// `receiver` (used only to rebind `this` for an accessor property).
#[inline]
fn bits_of_ptr(ptr: usize) -> u64 {
    crate::value::js_nanbox_pointer(ptr as i64).to_bits()
}

#[inline]
pub(crate) unsafe fn stringify_value(value: f64, type_hint: u32, buf: &mut String) {
    let bits: u64 = value.to_bits();

    if bits == TAG_NULL {
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

    // BigInt: apply `BigInt.prototype.toJSON` if present, else throw a TypeError
    // (test262 JSON/stringify/value-bigint*).
    if tag == BIGINT_TAG {
        serialize_bigint(value, buf);
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        // #2154 — see stringify_value_depth: skip native handle ids that aren't
        // real heap objects, so JSON.stringify of an object holding e.g. an
        // `http.Agent`, a fetch/zlib/stream handle, or a revocable-Proxy id
        // emits `null` instead of segfaulting on the ArrayHeader/keys_array
        // deref below. The whole small-handle band `[0, 0x100000)` is bogus, not
        // just the `< 0x1000` low guard (#4904/#1843).
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            buf.push_str("null");
            return;
        }
        // #3857: a boxed primitive wrapper (`new String`/`Number`/`Boolean`,
        // `Object(1n)`) serializes as its underlying primitive, not the empty
        // wrapper object (which produced `{}`). Recurse on the unwrapped value.
        if let Some(prim) = crate::builtins::boxed_primitive_json_value(value) {
            // An own `toJSON` expando on the wrapper itself (`str.toJSON =
            // fn`) must be honored BEFORE the primitive unwrap — a boxed
            // primitive is a real `ObjectHeader` and can carry one (test262
            // JSON/stringify/value-tojson-result).
            if let Some(to_json_val) = object_get_to_json(ptr) {
                arm_to_json_result_guard(to_json_val);
                stringify_value(to_json_val, TYPE_UNKNOWN, buf);
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                return;
            }
            stringify_value(prim, TYPE_UNKNOWN, buf);
            return;
        }
        // #2089: a Date is a NaN-boxed `DateCell` pointer. Apply the shared
        // Date.prototype.toJSON algorithm before any object/array deref of the
        // small cell. This must perform the observable `toISOString` lookup:
        // an own override participates in JSON.stringify (#9529).
        if crate::date::is_date_cell_addr(ptr as usize) {
            let result = crate::object::date_to_json_value(value);
            stringify_value(result, TYPE_UNKNOWN, buf);
            return;
        }
        // Temporal (#4686): `JSON.stringify(temporal)` calls `toJSON`, which
        // returns the canonical ISO string — emitted quoted. Detect before the
        // generic object path (the cell is not an enumerable ObjectHeader).
        #[cfg(feature = "temporal")]
        if crate::temporal::is_temporal_cell_addr(ptr as usize) {
            if let Some(s) = crate::temporal::temporal_iso_string(value) {
                write_escaped_string(buf, &s);
            } else {
                buf.push_str("null");
            }
            return;
        }
        // #2900: a `JSON.rawJSON(text)` wrapper emits its stored text verbatim
        // (no quoting, no re-escaping) — at the root, as an object field, or as
        // an array element. Detect via the reserved class id before the
        // generic object path so the wrapper's `rawJSON` own property is never
        // serialized as `{"rawJSON":...}`.
        if let Some(raw) = super::raw_json_text_bytes(ptr) {
            buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
            return;
        }
        if type_hint == TYPE_OBJECT {
            stringify_object(ptr, buf);
            return;
        }
        if type_hint == TYPE_ARRAY {
            stringify_array(ptr, buf);
            return;
        }

        // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type —
        // BufferHeader has no GcHeader, so the gc-tag read would read
        // unrelated memory and the resulting dispatch could segfault on
        // is_object_pointer's `keys_array` deref.
        if crate::buffer::is_registered_buffer(ptr as usize) {
            stringify_buffer(ptr, buf);
            return;
        }
        // Issue #5111: TypedArray (no GcHeader on small ones) detection BEFORE
        // gc_obj_type, same rationale as the buffer check above.
        if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
            stringify_typed_array(ptr, buf);
            return;
        }
        // A Symbol is POINTER_TAG'd but allocated as GC_TYPE_STRING (see
        // `is_symbol_value`) — detect it before the GC-tag dispatch below,
        // which would otherwise misread its SymbolHeader bytes as a
        // StringHeader (test262 JSON/stringify/value-symbol).
        if is_symbol_value(bits) {
            buf.push_str("null");
            return;
        }

        // Prefer the GC header's obj_type tag for dispatch — the old
        // capacity heuristic (`cap < 10000`) misidentified legitimate
        // arrays that had grown past 10k as strings, panicking on
        // `JSON.stringify(arr)` where `arr.length >= 10000` (issue #43).
        // An elements-backed Array-subclass instance IS an Array to
        // `JSON.stringify` (IsArray is true): serialize its elements.
        if let Some((_, elements)) = crate::array::subclass_elements::backed(ptr as usize) {
            return stringify_array(elements as *const u8, buf);
        }
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_ARRAY => stringify_array(ptr, buf),
            // A function has no ordinary object/array/string/error/map/set
            // layout; without this arm it falls into the untagged-pointer
            // fallback below and its ClosureHeader bytes get misread as an
            // ObjectHeader/ArrayHeader/StringHeader (test262
            // JSON/stringify/value-function).
            crate::gc::GC_TYPE_CLOSURE => buf.push_str("null"),
            crate::gc::GC_TYPE_OBJECT => {
                if crate::node_stream::try_stringify_node_stream_json(ptr, buf) {
                    return;
                }
                if is_object_pointer(ptr) {
                    // `stringify_object_inner` (via `stringify_object`) probes
                    // the prototype `toJSON` itself, so no extra check needed.
                    stringify_object(ptr, buf);
                } else {
                    // Object failed `is_object_pointer` (zero own enumerable
                    // properties). A class instance with no instance fields but
                    // a prototype `toJSON` (e.g. `class { toJSON() {…} }`) lands
                    // here — honour `toJSON` before the empty-object fallback.
                    // (#321) Plain `{}` / `Object.fromEntries([])` carry
                    // `class_id == 0`, so the probe is skipped for them.
                    if (*(ptr as *const crate::ObjectHeader)).class_id != 0 {
                        if let Some(to_json_val) = object_get_to_json(ptr) {
                            arm_to_json_result_guard(to_json_val);
                            stringify_value(to_json_val, TYPE_UNKNOWN, buf);
                            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                            return;
                        }
                    }
                    if object_has_no_own_keys(ptr) {
                        // A valid object with no own keys (null keys_array like
                        // `Object.fromEntries([])` / `{}`, OR a valid-but-empty
                        // keys_array like a `class C {}` instance or a class with
                        // only prototype methods/getters) fails `is_object_pointer`'s
                        // `keys_len > 0` guard but is still `{}`, not `null`. A
                        // non-empty object that fails the check is corrupted → "null".
                        buf.push_str("{}");
                    } else {
                        buf.push_str("null");
                    }
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
                // Issue #928: Built-in Error objects (and subclasses like
                // TypeError) have a dedicated `ErrorHeader` layout — not the
                // JSObject keys/values layout — so they must never reach
                // `stringify_object`, which would deref garbage as a
                // `keys_array` pointer and segfault.
                //
                // They are NOT always "{}", though. Node emits an error's own
                // ENUMERABLE properties like any other object; `{}` is merely
                // what a *plain* error produces, because `message`/`name`/
                // `stack` are non-enumerable:
                //
                //   JSON.stringify(new Error("x"))            -> {}
                //   e.foo = 1; JSON.stringify(e)              -> {"foo":1}
                //   JSON.stringify(fsError)                   -> {"errno":-2,"code":"ENOENT",…}
                //
                // Hardcoding "{}" silently dropped every one of those, so any
                // code that logs a caught error as JSON lost its whole payload.
                stringify_error_own_props(ptr, buf, None);
            }
            crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET => {
                // Map/Set have a `{size, capacity, entries/elements}` header,
                // NOT the JSObject keys/values layout — routing them through
                // the catch-all `is_object_pointer` path derefs their internals
                // as a `keys_array` pointer and segfaults. Node serializes both
                // as "{}" (their contents aren't enumerable own props).
                buf.push_str("{}");
            }
            // A Promise has no enumerable own properties — Node emits "{}". Its
            // `PromiseHeader` is not the JSObject keys/values layout, so falling
            // through to the structural heuristics below read its slots as a
            // StringHeader and emitted `""`.
            crate::gc::GC_TYPE_PROMISE => {
                buf.push_str("{}");
            }
            _ => {
                // Unknown/untagged pointer: fall back to the structural
                // heuristics for safety (e.g. pointers to non-GC-tracked
                // memory). Arrays up to 10k cap are dispatched here;
                // above that we defensively emit "null" rather than
                // trying to treat them as strings.
                if is_object_pointer(ptr) {
                    stringify_object(ptr, buf);
                } else {
                    let arr = ptr as *const crate::ArrayHeader;
                    if !arr.is_null() {
                        let len = (*arr).length;
                        let cap = (*arr).capacity;
                        if len <= cap && cap > 0 && cap < 10000 {
                            stringify_array(ptr, buf);
                            return;
                        }
                    }
                    let str_ptr = ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
            }
        }
        return;
    }

    write_number(buf, value);
}

/// Depth-aware stringify for recursive calls from stringify_object_inner.
/// For non-pointer values this is identical to stringify_value; for
/// objects/arrays it threads the depth counter through.
#[inline]
pub(crate) unsafe fn stringify_value_depth(
    value: f64,
    type_hint: u32,
    buf: &mut String,
    depth: u32,
) {
    let bits: u64 = value.to_bits();

    // Fast path: non-pointer values don't recurse
    if bits == TAG_NULL {
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
        serialize_bigint(value, buf);
        return;
    }

    if let Some(ptr) = extract_pointer(bits) {
        // #2154 — a POINTER_TAG value can carry a native *handle id* (a small
        // integer like `2`, e.g. an `http.Agent` in an object literal, a fetch/
        // zlib/stream handle, or a revocable-Proxy id) rather than a real heap
        // pointer. Such values aren't JSON-serializable and dereferencing them
        // (gc_obj_type → is_object_pointer / array probe) segfaults. Emit `null`,
        // the same way closures are dropped. The whole small-handle band
        // `[0, 0x100000)` is bogus, not just the `< 0x1000` low guard
        // (#4904/#1843 — a Proxy id at 0xF0005 crashed Next.js render).
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            buf.push_str("null");
            return;
        }
        // #3857: a boxed primitive wrapper serializes as its underlying
        // primitive (see the matching branch in `stringify_value`).
        if let Some(prim) = crate::builtins::boxed_primitive_json_value(value) {
            // An own `toJSON` expando — see the matching branch in
            // `stringify_value`.
            if let Some(to_json_val) = object_get_to_json(ptr) {
                arm_to_json_result_guard(to_json_val);
                stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth);
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                return;
            }
            stringify_value_depth(prim, TYPE_UNKNOWN, buf, depth);
            return;
        }
        // A RegExp has no enumerable own properties, so Node serializes it as `{}`.
        // Perry's `RegExpHeader` is not an `ObjectHeader`, so without this the
        // generic object walk read its internal slots as fields and emitted
        // `{"field0":null}`. Detected by the header magic (never a raw deref).
        if crate::regex::regex_header_has_magic(ptr as *const crate::regex::RegExpHeader) {
            buf.push_str("{}");
            return;
        }
        // Apply Date.prototype.toJSON before any object/array handling so the
        // small cell is never deref'd as an `ObjectHeader`/`ArrayHeader`. The
        // shared algorithm performs the observable `toISOString` lookup.
        if crate::date::is_date_cell_addr(ptr as usize) {
            let result = crate::object::date_to_json_value(value);
            stringify_value_depth(result, TYPE_UNKNOWN, buf, depth);
            return;
        }
        // Temporal (#4686): `toJSON` → quoted ISO string. See the matching
        // branch in `stringify_value`.
        #[cfg(feature = "temporal")]
        if crate::temporal::is_temporal_cell_addr(ptr as usize) {
            if let Some(s) = crate::temporal::temporal_iso_string(value) {
                write_escaped_string(buf, &s);
            } else {
                buf.push_str("null");
            }
            return;
        }
        // #2900: raw-JSON wrapper — emit stored text verbatim. See the matching
        // branch in `stringify_value`.
        if let Some(raw) = super::raw_json_text_bytes(ptr) {
            buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
            return;
        }
        if type_hint == TYPE_OBJECT {
            stringify_object_inner(ptr, buf, depth);
            return;
        }
        if type_hint == TYPE_ARRAY {
            stringify_array_depth(ptr, buf, depth);
            return;
        }
        // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type — see
        // the matching branch in `stringify_value`.
        if crate::buffer::is_registered_buffer(ptr as usize) {
            stringify_buffer(ptr, buf);
            return;
        }
        // Issue #5111: TypedArray detection BEFORE gc_obj_type (see above).
        if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
            stringify_typed_array(ptr, buf);
            return;
        }
        // A Symbol — see the matching branch in `stringify_value`.
        if is_symbol_value(bits) {
            buf.push_str("null");
            return;
        }
        if let Some((_, elements)) = crate::array::subclass_elements::backed(ptr as usize) {
            return stringify_array_depth(elements as *const u8, buf, depth);
        }
        match gc_obj_type(ptr) {
            crate::gc::GC_TYPE_OBJECT => stringify_object_inner(ptr, buf, depth),
            crate::gc::GC_TYPE_ARRAY => stringify_array_depth(ptr, buf, depth),
            // A function — see the matching branch in `stringify_value`.
            crate::gc::GC_TYPE_CLOSURE => buf.push_str("null"),
            crate::gc::GC_TYPE_STRING => {
                let str_ptr = ptr as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            }
            crate::gc::GC_TYPE_ERROR => {
                // Issue #928: see the matching branch in `stringify_value`.
                stringify_error_own_props(ptr, buf, Some(depth));
            }
            crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET => {
                // See the matching branch in `stringify_value` — Map/Set
                // serialize as "{}" and must not reach the object catch-all.
                buf.push_str("{}");
            }
            // A Promise has no enumerable own properties — Node emits "{}". Its
            // `PromiseHeader` is not the JSObject keys/values layout, so falling
            // through to the structural heuristics below read its slots as a
            // StringHeader and emitted `""`.
            crate::gc::GC_TYPE_PROMISE => {
                buf.push_str("{}");
            }
            _ => {
                if is_object_pointer(ptr) {
                    stringify_object_inner(ptr, buf, depth);
                } else {
                    let arr = ptr as *const crate::ArrayHeader;
                    if !arr.is_null() {
                        let len = (*arr).length;
                        let cap = (*arr).capacity;
                        if len <= cap && cap > 0 && cap < 10000 {
                            stringify_array_depth(ptr, buf, depth);
                            return;
                        }
                    }
                    let str_ptr = ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
            }
        }
        return;
    }

    write_number(buf, value);
}

/// JSON.stringify serializes only own ENUMERABLE string-keyed properties.
/// Returns `true` when the own key `key_f64` on `obj` carries an explicit
/// `enumerable: false` descriptor (`Object.defineProperty`, `freeze`/`seal`,
/// or a builtin descriptor such as `Uint8Array.prototype.BYTES_PER_ELEMENT`),
/// so the caller must skip it. Callers gate this behind
/// `crate::object::descriptors_in_use()` so the common no-descriptor object
/// pays only a single relaxed atomic load and never touches the descriptor map.
pub(crate) unsafe fn json_key_non_enumerable(
    obj: *const crate::ObjectHeader,
    key_f64: f64,
) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some(kb) =
        crate::string::js_string_key_bytes(crate::JSValue::from_bits(key_f64.to_bits()), &mut sso)
    {
        if let Ok(ks) = std::str::from_utf8(kb) {
            if let Some(attrs) = crate::object::get_property_attrs(obj as usize, ks) {
                return !attrs.enumerable();
            }
        }
    }
    false
}

#[inline]
pub(crate) unsafe fn stringify_object(ptr: *const u8, buf: &mut String) {
    stringify_object_inner(ptr, buf, 0)
}

/// #6519: emit a WHATWG `URL` instance as its `href` string — Node serializes a
/// URL through `URL.prototype.toJSON()`. A URL is a plain `GC_TYPE_OBJECT` whose
/// `searchParams` field points back at the URL, so the generic object walk trips
/// the circular-structure detector; every stringify walker (compact, pretty, and
/// replacer, in both `stringify.rs` and `replacer.rs`) short-circuits URL objects
/// through here. Callers must have confirmed `crate::url::is_url_object_shape`.
/// `href` is a string by construction, so this never recurses into an object and
/// is depth-independent (emitting it just escapes + quotes the string).
#[inline]
pub(crate) unsafe fn write_url_href_json(url: *mut crate::ObjectHeader, buf: &mut String) {
    let href = crate::url::url_class::js_url_get_href(url);
    stringify_value(href, TYPE_UNKNOWN, buf);
}

pub(crate) unsafe fn stringify_array(ptr: *const u8, buf: &mut String) {
    stringify_array_depth(ptr, buf, 0)
}

/// Depth-aware variant of stringify_array for recursive calls.
pub(crate) unsafe fn stringify_array_depth(ptr: *const u8, buf: &mut String, depth: u32) {
    check_stringify_nesting_depth(depth as usize);
    // Issue #2021: an array that has grown past its initial inline capacity
    // (16) was reallocated to a new block, leaving a GC_FLAG_FORWARDED stub
    // at the old address. Callers (and element decoders) hand us whatever
    // pointer the NaN-boxed value held, which for a grown array is that
    // stale stub — reading its first 8 bytes as (length, capacity) yields
    // the forwarding pointer reinterpreted as a huge length and walks off
    // into garbage (Bus error in stringify, the original #2021 crash).
    // `clean_arr_ptr` follows the forwarding chain exactly as every other
    // array accessor does (#233); element reads via js_array_get already do
    // this, which is why field access worked while whole-array stringify
    // crashed. Resolving here is the single chokepoint for the top-level,
    // nested-array, and object-field-array paths.
    let arr = crate::array::clean_arr_ptr(ptr as *const crate::ArrayHeader);
    if arr.is_null() {
        buf.push_str("[]");
        return;
    }
    // An own `toJSON` (SerializeJSONProperty step 2) runs BEFORE the
    // IsArray/SerializeJSONArray dispatch — see `array_get_to_json`. Must run
    // before the circular-stack push below: a `toJSON` result that re-enters
    // an array still open on the stack (test262 value-tojson-array-circular)
    // needs that still-open entry to trip the check.
    if let Some(to_json_val) = array_get_to_json(arr) {
        arm_to_json_result_guard(to_json_val);
        stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
        return;
    }
    // A primitive-only array cannot recurse, invoke user code or collect.
    // Its emitter borrows the resolved element storage for this call only;
    // complex values fall back before writing any output.
    if super::stringify_primitive_array::try_emit(arr, buf) {
        return;
    }
    // Circular-reference detection (ECMA-262 25.5.2 SerializeJSONArray step
    // 1-2). Unlike objects, the compact array path does NOT bump `depth`, so
    // an all-array cycle (`a=[]; a.push(a)`) would otherwise recurse until the
    // native stack overflows (crash, no output). Track the open-array pointers
    // in `STRINGIFY_STACK` and throw a `TypeError` on revisit. The stack is
    // cleared at the outermost `js_json_stringify` entry, so a `longjmp` out of
    // this throw (which skips the pops below) cannot leak across top-level
    // calls. A sibling array reused in two positions is NOT a cycle: each
    // position pushes-then-pops, so the second visit sees an empty-of-it stack.
    if STRINGIFY_STACK.with(|s| s.borrow().contains(&(arr as usize))) {
        let msg = "Converting circular structure to JSON";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_typeerror_new(msg_ptr);
        crate::exception::js_throw(f64::from_bits(
            POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
        ));
    }
    STRINGIFY_STACK.with(|s| s.borrow_mut().push(arr as usize));
    let len = (*arr).length;
    // A descriptor/prototype-aware Get can invoke a getter, mutate later
    // indices or move the array. Raw slot reads (including template probing)
    // cannot preserve those effects. Keep this walk outside the dense path.
    let array_flags = crate::value::addr_class::try_read_tracked_gc_header(arr as usize)
        .map_or(0, |header| header.as_ref()._reserved);
    if crate::array::array_iteration_is_exotic_resolved(arr, array_flags)
        || crate::object::prototype_chain::object_static_prototype(arr as usize).is_some()
    {
        stringify_exotic_array(arr, len, buf, depth);
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }
    if super::stringify_nested_records::try_emit(arr, buf, depth) {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }
    // Root the array and re-derive the element base per access: a nested
    // `toJSON` / getter / any allocation inside the recursive serialization
    // below can trigger a GC that sweeps or moves this array while a hoisted
    // `elements` pointer still aims at the old storage (no conservative
    // stack scan in production; alloc-point minors can be moving under the
    // evacuation policy).
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let elem_at = |i: usize| -> f64 {
        arr_handle.with_const_ptr(|arr: *const crate::ArrayHeader| {
            *(crate::array::array_elements_ptr(arr as *const crate::ArrayHeader) as *const f64)
                .add(i)
        })
    };

    // Homogeneous-shape fast path for arrays of objects sharing one
    // `keys_array` (issue #59). The template is built from element 0 and
    // reused for every subsequent element whose shape matches; mismatches
    // fall back per-element via `stringify_value_depth`, so mixed arrays
    // still produce correct output. Pre-check the tag inline to skip the
    // function call entirely for arrays of primitives (issue #64) — common
    // for nested fields like `tags: ["x","y"]` that fired per-element.
    let template = if len >= 2 {
        let first_bits = elem_at(0).to_bits();
        let tag = first_bits & 0xFFFF_0000_0000_0000;
        let first_ptr = if tag == POINTER_TAG {
            (first_bits & POINTER_MASK) as *const u8
        } else {
            first_bits as *const u8
        };
        if (tag == POINTER_TAG || is_raw_pointer(first_bits))
            // A small-handle-band id (Proxy id, fetch/zlib/stream handle) is not
            // an object; building a shape template from it would deref unmapped
            // memory (#4904/#1843). Fall through to per-element handling.
            && !crate::value::addr_class::is_handle_band(first_ptr as usize)
            // #2089: a Date element is a small `DateCell`, not an object with a
            // `keys_array` — don't build an object-shape template from it.
            && !crate::date::is_date_cell_addr((first_bits & POINTER_MASK) as usize)
            // #2900: a raw-JSON wrapper must emit its stored text verbatim, not
            // be templated as a `{"rawJSON":...}` object.
            && !(first_ptr as usize >= 0x1000 && super::ptr_is_raw_json_wrapper(first_ptr))
            // #3857: a boxed primitive wrapper has no own enumerable keys, so an
            // object-shape template would render it (and the whole array) as
            // `{}`. Fall through to per-element handling, which unwraps it.
            && crate::builtins::boxed_primitive_json_value(elem_at(0)).is_none()
        {
            build_shape_prefix_template(first_bits)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(ref tmpl) = template {
        let mut data_record_global_proof = false;
        buf.push('[');
        for i in 0..len {
            if i > 0 {
                buf.push(',');
            }
            // Re-derived per element: the previous element's serialization can
            // have run user code / allocated (and moved this array).
            let elem = elem_at(i as usize);
            let elem_bits = elem.to_bits();
            if !try_emit_shape_element(
                elem_bits,
                tmpl,
                buf,
                depth + 1,
                Some(i as usize),
                &mut data_record_global_proof,
            ) {
                // The element is one container below its enclosing array.
                // `try_emit_shape_element` can decline before reaching its
                // callback-capable section, so publish the index for fallback.
                set_to_json_key_index(i as usize);
                stringify_value_depth(elem, TYPE_UNKNOWN, buf, depth + 1);
            }
        }
        buf.push(']');
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
        return;
    }

    buf.push('[');
    for i in 0..len {
        if i > 0 {
            buf.push(',');
        }
        // Re-derived per element: the previous element's serialization can
        // have run user code / allocated (and moved this array).
        let elem = elem_at(i as usize);
        let elem_bits = elem.to_bits();
        let elem_tag = elem_bits & 0xFFFF_0000_0000_0000;

        if elem_bits == TAG_UNDEFINED {
            buf.push_str("null");
        } else if elem_tag == STRING_TAG {
            let str_ptr = (elem_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_tag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(elem_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if elem_bits == TAG_NULL {
            buf.push_str("null");
        } else if elem_bits == TAG_TRUE {
            buf.push_str("true");
        } else if elem_bits == TAG_FALSE {
            buf.push_str("false");
        } else if elem_tag == BIGINT_TAG {
            // A BigInt element's `toJSON` key is its stringified index (#5909).
            set_to_json_key_index(i as usize);
            serialize_bigint(elem, buf);
        } else if elem_tag == POINTER_TAG || is_raw_pointer(elem_bits) {
            // An element's `toJSON` key is its stringified index (#5909) — set
            // before the object/array `toJSON` probes and the recursion below.
            set_to_json_key_index(i as usize);
            let elem_ptr = if elem_tag == POINTER_TAG {
                (elem_bits & POINTER_MASK) as *const u8
            } else {
                elem_bits as *const u8
            };
            // A small-handle-band element (revocable-Proxy id, fetch/zlib/stream
            // handle) is not a serializable heap value; the `gc_obj_type` /
            // `is_object_pointer` / ArrayHeader-length probes below would deref
            // unmapped memory. Emit "null" before any load (#4904/#1843 — a
            // Proxy element in a Next.js render array crashed exactly here).
            if crate::value::addr_class::is_handle_band(elem_ptr as usize) {
                buf.push_str("null");
                continue;
            }
            // A function or Symbol element serializes as `null` (ECMA-262
            // SerializeJSONArray step 8b — SerializeJSONProperty returns
            // `undefined` for both, and the array walk substitutes `null`).
            // Neither has a `GC_TYPE_ARRAY`/`GC_TYPE_OBJECT`/`GC_TYPE_STRING`
            // shape, so without this check they fall into the `gc_obj_type`
            // match's catch-all below and get misread as an object/string
            // (test262 JSON/stringify/value-function, value-symbol).
            if is_closure_value(elem_bits) || is_symbol_value(elem_bits) {
                buf.push_str("null");
                continue;
            }
            // #3857: a boxed primitive wrapper element serializes as its
            // underlying primitive, not the empty wrapper object.
            if let Some(prim) = crate::builtins::boxed_primitive_json_value(elem) {
                // An own `toJSON` expando — see the matching branch in
                // `stringify_value` (test262 JSON/stringify/value-tojson-result).
                if let Some(to_json_val) = object_get_to_json(elem_ptr) {
                    arm_to_json_result_guard(to_json_val);
                    stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
                    SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                    continue;
                }
                stringify_value_depth(prim, TYPE_UNKNOWN, buf, depth + 1);
                continue;
            }
            // A Date element applies the shared Date.prototype.toJSON
            // algorithm before any object/array deref of the small cell.
            if crate::date::is_date_cell_addr(elem_ptr as usize) {
                let result = crate::object::date_to_json_value(elem);
                stringify_value_depth(result, TYPE_UNKNOWN, buf, depth + 1);
                continue;
            }
            // #2900: raw-JSON wrapper element — emit stored text verbatim.
            if let Some(raw) = super::raw_json_text_bytes(elem_ptr) {
                buf.push_str(std::str::from_utf8(raw).unwrap_or("null"));
                continue;
            }
            // Issue #639: Buffer/Uint8Array detection BEFORE gc_obj_type — see
            // the matching branch in `stringify_value`.
            if crate::buffer::is_registered_buffer(elem_ptr as usize) {
                stringify_buffer(elem_ptr, buf);
                continue;
            }
            // Issue #5111: TypedArray element detection BEFORE gc_obj_type.
            if crate::typedarray::lookup_typed_array_kind(elem_ptr as usize).is_some() {
                stringify_typed_array(elem_ptr, buf);
                continue;
            }
            match gc_obj_type(elem_ptr) {
                crate::gc::GC_TYPE_OBJECT => stringify_object_inner(elem_ptr, buf, depth + 1),
                crate::gc::GC_TYPE_ARRAY => stringify_array_depth(elem_ptr, buf, depth + 1),
                crate::gc::GC_TYPE_STRING => {
                    let str_ptr = elem_ptr as *const StringHeader;
                    if let Some(s) = str_from_header(str_ptr) {
                        write_escaped_string(buf, s);
                    } else {
                        buf.push_str("null");
                    }
                }
                crate::gc::GC_TYPE_MAP | crate::gc::GC_TYPE_SET => {
                    // See `stringify_value` — Map/Set serialize as "{}" and
                    // must not reach the object catch-all (segfault).
                    buf.push_str("{}");
                }
                // A Promise has no enumerable own properties — Node emits "{}". Its
                // `PromiseHeader` is not the JSObject keys/values layout, so falling
                // through to the structural heuristics below read its slots as a
                // StringHeader and emitted `""`.
                crate::gc::GC_TYPE_PROMISE => {
                    buf.push_str("{}");
                }
                _ => {
                    if is_object_pointer(elem_ptr) {
                        stringify_object_inner(elem_ptr, buf, depth + 1);
                    } else {
                        let arr_elem = elem_ptr as *const crate::ArrayHeader;
                        let arr_len = (*arr_elem).length;
                        let arr_cap = (*arr_elem).capacity;
                        if arr_len <= arr_cap && arr_cap > 0 && arr_cap < 10000 {
                            stringify_array_depth(elem_ptr, buf, depth + 1);
                        } else {
                            let str_ptr = elem_ptr as *const StringHeader;
                            if let Some(s) = str_from_header(str_ptr) {
                                write_escaped_string(buf, s);
                            } else {
                                buf.push_str("null");
                            }
                        }
                    }
                }
            }
        } else {
            // Number — or Date, handled centrally by `write_number`
            // via DATE_REGISTRY lookup.
            write_number(buf, elem);
        }
    }
    buf.push(']');
    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
}

unsafe fn stringify_exotic_array(
    arr: *const crate::ArrayHeader,
    len: u32,
    buf: &mut String,
    depth: u32,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(arr);
    buf.push('[');
    for i in 0..len {
        if i != 0 {
            buf.push(',');
        }
        let element = input.with_const_ptr(|arr| crate::array::array_spec_get(arr, i));
        let bits = element.to_bits();
        if bits == TAG_UNDEFINED
            || bits == crate::value::TAG_HOLE
            || is_closure_value(bits)
            || is_symbol_value(bits)
        {
            buf.push_str("null");
        } else {
            let element_scope = crate::gc::RuntimeHandleScope::new();
            let element = element_scope.root_nanbox_f64(element);
            set_to_json_key_index(i as usize);
            stringify_value_depth(element.get_nanbox_f64(), TYPE_UNKNOWN, buf, depth + 1);
        }
    }
    buf.push(']');
}

#[inline]
pub(crate) unsafe fn estimate_json_size(value: f64, type_hint: u32) -> usize {
    let bits = value.to_bits();
    if let Some(ptr) = extract_pointer(bits) {
        // A small-handle-band id is not a heap object; reading its ArrayHeader
        // length would deref unmapped memory. Use the scalar estimate.
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return 4096;
        }
        if type_hint == TYPE_ARRAY || (!is_object_pointer(ptr) && type_hint != TYPE_OBJECT) {
            let arr = ptr as *const crate::ArrayHeader;
            let len = (*arr).length as usize;
            return (len * 300).max(256);
        }
        if type_hint == TYPE_OBJECT || is_object_pointer(ptr) {
            let obj = ptr as *const crate::ObjectHeader;
            let fields = crate::object::object_live_slot_count(obj) as usize;
            return (fields * 200).max(256);
        }
    }
    4096
}

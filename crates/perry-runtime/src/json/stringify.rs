//! Core `JSON.stringify` traversal: scalar/object/array/buffer emitters,
//! shape-template fast path, and number/escape-string formatters.
//!
//! Public FFI entry points (`js_json_stringify`, etc.) live in
//! `stringify_api.rs`; this file is the shared traversal those entry points
//! and the replacer path call into.

use super::*;
use crate::{js_string_from_bytes, JSValue, StringHeader};
use std::fmt::Write as FmtWrite;

pub(crate) use super::stringify_scalars::{
    bigint_apply_to_json, serialize_bigint, throw_bigint_serialize, write_escaped_string,
    write_number,
};

// ─── JSON.stringify ───────────────────────────────────────────────────────────

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
/// subsequent `(*obj).keys_array` read then SIGSEGVs. Mirrors the `path.rs` /
/// `current_heap_header_for_user_ptr` Unknown→malloc rule.
#[inline]
pub(super) unsafe fn ptr_is_tracked_heap_object(ptr: *const u8) -> bool {
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
    let potential_keys_ptr = (*obj).keys_array as u64;
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
        let keys_arr = (*obj).keys_array;
        let keys_len = (*keys_arr).length;
        let keys_cap = (*keys_arr).capacity;
        let field_count = (*obj).field_count;
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
    let keys = (*(ptr as *const crate::ObjectHeader)).keys_array;
    if keys.is_null() {
        return true;
    }
    // Same reasoning as `is_object_pointer`: a tracked `ptr` does not prove an
    // `ObjectHeader` layout, so this slot may be an unrelated field. Require the
    // keys array itself to be a tracked allocation before loading its length.
    if !ptr_is_tracked_heap_object(keys as *const u8) {
        return false;
    }
    (*keys).length == 0
}

/// The object's keys array, but only when it is genuinely a tracked heap
/// allocation. A GC allocation that is not an `ObjectHeader` (a Promise, WeakMap,
/// ArrayBuffer, ...) can still reach the object walkers — via a static
/// `TYPE_OBJECT` hint from codegen — and its bytes at this offset are some other
/// field. Loading that as an `ArrayHeader` faults. Walkers bail to `{}` on `None`.
pub(super) unsafe fn object_keys_array_checked(
    obj: *const crate::ObjectHeader,
) -> Option<*const crate::ArrayHeader> {
    let keys = (*obj).keys_array as *const crate::ArrayHeader;
    if keys.is_null() || !ptr_is_tracked_heap_object(keys as *const u8) {
        return None;
    }
    Some(keys)
}

/// Check if a NaN-boxed value is a closure (function).
#[inline]
pub(crate) unsafe fn is_closure_value(bits: u64) -> bool {
    if let Some(ptr) = extract_pointer(bits) {
        // #2154 / #4904 — a POINTER_TAG field can be a native *handle id* (a
        // small integer, e.g. an `http.Agent` stored in an options object),
        // not a real heap pointer. Reading the CLOSURE_MAGIC tag at offset 12
        // of such a value segfaults (stringifying `{ agent, lookup: () => {} }`
        // crashed exactly here). Skip the low-memory guard range, matching
        // the has-function probe below.
        if crate::value::addr_class::is_handle_band(ptr as usize) {
            return false;
        }
        // Check for ClosureHeader magic at offset 8 (type_tag field)
        let type_tag =
            *((ptr as *const u8).add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32);
        type_tag == crate::closure::CLOSURE_MAGIC
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

    let key = js_string_from_bytes(b"toJSON".as_ptr(), 6);
    let key_handle = scope.root_string_ptr(key);

    let obj_ptr = recv_handle.get_nanbox_f64();
    let obj_ptr = (obj_ptr.to_bits() & POINTER_MASK) as *const crate::ObjectHeader;
    let method = crate::object::js_object_get_field_by_name(
        obj_ptr,
        key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
    );

    // Only treat it as toJSON if it actually resolved to a callable closure
    // (POINTER_TAG + closure). A plain object with no `toJSON`, or a `toJSON`
    // data field that isn't a function, returns `None` → serialize normally.
    let method_bits = method.bits();
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

    let prev_this = crate::object::js_implicit_this_set(recv);
    let result = crate::closure::js_native_call_value(f64::from_bits(bound), &key_f64_arg, 1);
    crate::object::js_implicit_this_set(prev_this);
    // The user callback may have installed/removed `Object.prototype.toJSON`.
    invalidate_object_proto_tojson_state();
    Some(result)
}

/// Check if an array has an own `toJSON` method (an expando property, e.g.
/// `arr.toJSON = function() {...}`, stored in the array-named-property side
/// table since an `ArrayHeader` has no `keys_array`) — the array analog of
/// `object_get_to_json`. Per ECMA-262 §25.5.2.2, `SerializeJSONProperty` step
/// 2 applies to ANY object, including arrays, BEFORE the `IsArray` check
/// (step 10) that would otherwise route straight into `SerializeJSONArray`
/// (test262 JSON/stringify/value-tojson-result,
/// value-tojson-array-circular). Returns `None` when there's no callable own
/// `toJSON` (the caller then serializes the array's elements normally).
#[inline]
pub(crate) unsafe fn array_get_to_json(arr: *const crate::ArrayHeader) -> Option<f64> {
    // One-shot suppression — see `object_get_to_json`.
    if SUPPRESS_NEXT_TO_JSON.with(|c| c.replace(false)) {
        return None;
    }
    let method = crate::array::array_named_property_get_by_name(arr, "toJSON")?;
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
    let prev_this = crate::object::js_implicit_this_set(recv_handle.get_nanbox_f64());
    let result = crate::closure::js_native_call_value(f64::from_bits(method_bits), &key_f64_arg, 1);
    crate::object::js_implicit_this_set(prev_this);
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
        // #2089: a Date is a NaN-boxed `DateCell` pointer — emit `toJSON()`
        // (ISO string, or `null` for an Invalid Date) per ECMA-262 25.5.2,
        // before any object/array deref of the small cell.
        if crate::date::is_date_cell_addr(ptr as usize) {
            let s_ptr = crate::date::js_date_to_json(value);
            if let Some(s) = str_from_header(s_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
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
                // Issue #928: Built-in Error objects (and subclasses
                // like TypeError) have a dedicated `ErrorHeader` layout —
                // not the JSObject keys/values layout. Routing them
                // through `stringify_object` derefs garbage as a
                // `keys_array` pointer and segfaults the process.
                // Node's `JSON.stringify(new Error("x"))` returns "{}"
                // because Error's intrinsic props (`message`, `name`,
                // `stack`) are non-enumerable; mirror that.
                buf.push_str("{}");
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
        // #2089: a Date is a NaN-boxed `DateCell` pointer. JSON.stringify must
        // emit `toJSON()` → the ISO string (or `null` for an Invalid Date) per
        // ECMA-262 25.5.2. Check before any object/array handling so the small
        // cell is never deref'd as an `ObjectHeader`/`ArrayHeader`.
        if crate::date::is_date_cell_addr(ptr as usize) {
            let s_ptr = crate::date::js_date_to_json(value);
            if let Some(s) = str_from_header(s_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
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
                buf.push_str("{}");
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

/// SerializeJSONProperty step 2 (`toJSON`) for a heap-valued object member,
/// applied by the object walk BEFORE the member's key is written so a member
/// whose `toJSON` returns `undefined` can be OMITTED per spec (test262
/// JSON/stringify/value-tojson-arguments) instead of emitting `"k":null` — the
/// key used to be written first, with `toJSON` running only in the value
/// recursion below.
///
/// Returns `Some(result)` ONLY when a callable `toJSON` actually ran (the value
/// is a plain object/array carrying one); `result` is what it returned. The
/// caller then omits the member if `result` is `undefined`/function/Symbol, or
/// serializes `result` with the one-shot `SUPPRESS_NEXT_TO_JSON` guard armed so
/// its own walk doesn't re-invoke `toJSON`. Returns `None` when no `toJSON`
/// applies — a plain object/array without one, or any value that isn't a plain
/// object/array — so the caller serializes the ORIGINAL value through the
/// normal dispatch (never arming the guard: arming it for a value that then
/// doesn't self-probe would leak the one-shot into the next member's `toJSON`).
/// Because `None` means "serialize normally", a plain data object member keeps
/// the #6009 fast path (its own walk skips the `toJSON` probe when
/// `class_id == 0`), so this adds no probe there.
///
/// Guards, in an order safe for the `gc_obj_type` read below: handle ids and
/// buffers/typed arrays carry no `GcHeader`; RegExp shares the
/// `GC_TYPE_OBJECT` tag but is not an `ObjectHeader`; a boxed primitive is a
/// real object but must serialize as its primitive (see `stringify_value_depth`)
/// so it is left to the normal dispatch. Date/Temporal cells carry their own
/// `GC_TYPE_*` tags, so the `gc_obj_type` match's `_` arm already skips them.
/// The pending `toJSON` key must already be recorded.
unsafe fn member_to_json(value: f64) -> Option<f64> {
    let bits = value.to_bits();
    let ptr = extract_pointer(bits)?;
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return None;
    }
    if crate::buffer::is_registered_buffer(ptr as usize) {
        return None;
    }
    if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
        return None;
    }
    if crate::regex::regex_header_has_magic(ptr as *const crate::regex::RegExpHeader) {
        return None;
    }
    if crate::builtins::boxed_primitive_json_value(value).is_some() {
        return None;
    }
    match gc_obj_type(ptr) {
        crate::gc::GC_TYPE_ARRAY => array_get_to_json(ptr as *const crate::ArrayHeader),
        crate::gc::GC_TYPE_OBJECT => object_get_to_json(ptr),
        _ => None,
    }
}

pub(crate) unsafe fn stringify_object_inner(ptr: *const u8, buf: &mut String, depth: u32) {
    // #6519: a WHATWG `URL` instance is a plain `GC_TYPE_OBJECT` (class_id 0)
    // whose `searchParams` field points back at the URL — walking its fields
    // trips the circular-structure detector. Node serializes a URL via
    // `URL.prototype.toJSON()`, i.e. its `href` string. Top-level
    // `JSON.stringify(url)` is intercepted at HIR-lowering time
    // (`UrlInstanceToJSON`, module_static.rs), but a URL *nested* inside another
    // object/array is invisible to that interception and only reaches this
    // runtime walker — so detect the URL shape here and emit its href. This is
    // the single chokepoint every object walk funnels through (the direct
    // dispatch arms, the array slow loop, and per-field descent all land here);
    // `is_url_object_shape` validates the GC header before reading any field, so
    // it is safe to call on the non-object pointers that reach the `TYPE_OBJECT`
    // hint / catch-all callers.
    if crate::url::is_url_object_shape(ptr as *mut crate::ObjectHeader) {
        write_url_href_json(ptr as *mut crate::ObjectHeader, buf);
        return;
    }
    // #1704: an object with a null `keys_array` has no own enumerable
    // properties — empty objects come out of `js_object_alloc` with
    // `keys_array == null` and only get one once a field is set. This is the
    // shape of `Object.fromEntries([])`, `Object.fromEntries(emptyURLSearchParams)`,
    // and a never-mutated `{}` literal. Recursion into a nested empty object
    // reaches here directly (the `GC_TYPE_OBJECT` arm in `stringify_value_depth`
    // skips `is_object_pointer`), so the `(*keys_arr).length` read below would
    // dereference null and segfault (the `Object.fromEntries(URL.searchParams)`
    // crash inside a `@hono/perry-server` handler). Emit "{}" and return — an
    // empty object has no children, so it can't be part of a cycle and the
    // circular-reference tracking below is unnecessary.
    if (*(ptr as *const crate::ObjectHeader)).keys_array.is_null() {
        // A null `keys_array` means no own enumerable properties — but a class
        // instance with no instance fields (only methods, e.g. a `class {
        // toJSON() {…} }`) still has a `toJSON` on its prototype/vtable that
        // must be honoured before falling back to "{}". A plain empty object
        // literal / `Object.fromEntries([])` carries `class_id == 0`, so the
        // probe is skipped for them. (#321)
        if (*(ptr as *const crate::ObjectHeader)).class_id != 0 {
            if let Some(to_json_val) = object_get_to_json(ptr) {
                arm_to_json_result_guard(to_json_val);
                // Thread depth so a `toJSON` returning a cycle trips the
                // circular-detection push instead of overflowing the stack —
                // see the matching note in the keyed-object branch below.
                stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                return;
            }
        }
        buf.push_str("{}");
        return;
    }
    if depth > MAX_FAST_DEPTH {
        // Deep nesting — switch to full circular detection
        if STRINGIFY_STACK.with(|s| s.borrow().contains(&(ptr as usize))) {
            let msg = "Converting circular structure to JSON";
            let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err_ptr = crate::error::js_typeerror_new(msg_ptr);
            crate::exception::js_throw(f64::from_bits(
                POINTER_TAG | (err_ptr as u64 & POINTER_MASK),
            ));
        }
        STRINGIFY_STACK.with(|s| s.borrow_mut().push(ptr as usize));
    }

    let obj = ptr as *const crate::ObjectHeader;
    let num_fields = (*obj).field_count;

    // Templated fast path (#64 follow-up): if this object's shape has been
    // seen before in this stringify call, emit via the cached prefix table
    // and skip per-object `has_pointer_fields` / `object_get_to_json` /
    // key-lookup work. `try_emit_shape_element` rolls back the buffer and
    // returns false on any element-specific mismatch (different shape,
    // stray UNDEFINED, closure), at which point we fall through to the
    // slow path below.
    //
    // Guard (issue #67): skip the template machinery for small objects.
    // `shape_template_for` allocates a Box<ShapeTemplate> + Vec<String>
    // + one String per field on miss (~4-5 heap allocs), and the cache
    // is wiped at every top-level call exit — so for a one-shot small
    // top-level stringify the build is pure overhead vs. the inline slow
    // path below. The arrayof-objects fast path (stringify_array_depth)
    // uses a separate build_shape_prefix_template that's unaffected.
    // Skip the shape-template fast path when the object has overflow fields
    // (keys_len > num_fields — see object.rs:32 OVERFLOW_FIELDS, ≥9 stored
    // fields per #307). The template's per-field key prefix array is built
    // from `min(keys_len, field_count)`, so an overflow object would only
    // emit its first 8 fields. Falling through to the slow path below uses
    // `read_field_bits` which routes overflow reads through
    // `js_object_get_field`'s overflow_get fallback.
    let has_overflow_fields = unsafe {
        let keys_arr = (*obj).keys_array;
        !keys_arr.is_null() && (*keys_arr).length > num_fields
    };
    // The shape-template fast path emits every key in the shape; it can't
    // honor per-key `enumerable: false`, so fall through to the slow path
    // (which filters) whenever any descriptor exists on this thread.
    // Class instances (class_id != 0) route through the slow path: it honours a
    // prototype/own `toJSON` and filters private (`#x`) elements, neither of
    // which the shape-template fast path handles. Plain data objects (class_id
    // == 0 — the common JSON shape) keep the fast path.
    if num_fields >= 5
        && !has_overflow_fields
        && !crate::object::descriptors_in_use()
        && (*obj).class_id == 0
    {
        if let Some(tmpl_ptr) = shape_template_for(ptr) {
            if try_emit_shape_element(make_pointer_bits(ptr), &*tmpl_ptr, buf, depth) {
                if depth > MAX_FAST_DEPTH {
                    STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
                }
                return;
            }
        }
    }
    let Some(keys_arr) = object_keys_array_checked(obj) else {
        // Not an ObjectHeader after all (a Promise / WeakMap / ArrayBuffer that
        // reached here via a static TYPE_OBJECT hint). Node serializes those as
        // `{}`; walking the slot as an ArrayHeader would fault.
        buf.push_str("{}");
        return;
    };
    let keys_len = (*keys_arr).length;
    // Root the object for the enumeration below and re-derive the keys/field
    // buffers from the CURRENT header on every access: a user getter
    // (`json_object_getter_value`), a `toJSON` somewhere inside a nested
    // value, or any allocation in the recursive `stringify_value_depth` call
    // can trigger a GC that sweeps or moves this object — bare Rust locals
    // are invisible to the collector (production runs no conservative stack
    // scan), and alloc-point minors can be MOVING under the evacuation
    // policy. The keys array is re-derived THROUGH the object header (its
    // `keys_array` field is rewritten by the collector when it moves).
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_const_ptr(obj);
    let cur_obj = || obj_handle.get_raw_const_ptr::<crate::ObjectHeader>();
    let key_at = |f: u32| -> f64 {
        let keys_arr = (*cur_obj()).keys_array;
        let keys_elements =
            (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
        *keys_elements.add(f as usize)
    };
    // Closes #307: iterate up to keys_len, not min(num_fields, keys_len).
    // Parser-built objects with ≥9 fields cap field_count at the inline
    // alloc_limit (max(field_count, 8) physical slots) and store the overflow
    // values in OVERFLOW_FIELDS (object.rs:32) — so num_fields can be smaller
    // than keys_len. For inline slots (f < alloc_limit) we still read directly
    // off fields_ptr; for overflow slots we route through `js_object_get_field`
    // which checks field_count and falls through to `overflow_get`. Pre-fix
    // (`std::cmp::min(num_fields, keys_len)`) silently dropped the overflow
    // fields and `is_object_pointer`'s `keys_len <= field_count` guard
    // returned false, so `JSON.stringify` emitted the literal string "null"
    // for any parsed object with ≥9 fields.
    let alloc_limit = std::cmp::max(num_fields, 8);
    let read_field_bits = |f: u32| -> u64 {
        let obj = cur_obj();
        if f < alloc_limit {
            let fields_ptr =
                (obj as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
            (*fields_ptr.add(f as usize)).to_bits()
        } else {
            crate::object::js_object_get_field(obj, f).bits()
        }
    };
    let actual_fields = keys_len;

    // #2438: enumerate own keys in ECMA-262 OrdinaryOwnPropertyKeys order —
    // array-index keys first (ascending numeric), then string keys in
    // insertion order. `None` means no array-index keys, so insertion order
    // already matches spec and the loop walks `0..actual_fields` directly.
    let key_order = crate::object::ecma_own_key_order(keys_arr);

    // Deferred toJSON + closure checks (issue #67 tightening): scan fields
    // once to detect if any field is actually a closure. For data-only
    // objects with nested arrays/objects (e.g. `{a:1, b:"", c:[...]}`) the
    // earlier has_pointer_fields heuristic false-positived because any
    // POINTER_TAG field triggered the `object_get_to_json` key walk — even
    // though a toJSON method requires the *value* at the "toJSON" key to
    // be a closure. Reading offset 12 (CLOSURE_MAGIC) per pointer field is
    // cheaper (~3ns/field) than walking the keys array looking for a
    // "toJSON" string that almost never exists (~15ns).
    let has_closure_field = {
        let mut found = false;
        for f in 0..actual_fields {
            let bits = read_field_bits(f);
            let tag = bits & 0xFFFF_0000_0000_0000;
            let ptr_candidate = if tag == POINTER_TAG {
                (bits & POINTER_MASK) as *const u8
            } else if is_raw_pointer(bits) {
                bits as *const u8
            } else {
                std::ptr::null()
            };
            // #2154 — a POINTER_TAG field can be a native *handle id* (a small
            // integer, e.g. an `http.Agent` in an object literal, a fetch/zlib/
            // stream handle, or a revocable-Proxy id), not a real heap pointer.
            // Reading the CLOSURE_MAGIC tag at offset 12 of such a value
            // segfaults. Skip the whole small-handle band `[0, 0x100000)` — not
            // just the `< 0x1000` low guard (#4904/#1843 — a Proxy id at 0xF000D
            // in a Next.js render object crashed exactly here). Real closures
            // live far above the band.
            if crate::value::addr_class::is_above_handle_band(ptr_candidate as usize) {
                let type_tag =
                    *(ptr_candidate.add(crate::closure::CLOSURE_TYPE_TAG_OFFSET) as *const u32);
                // A Symbol-valued field must also be dropped (test262
                // JSON/stringify/value-symbol): a Symbol is POINTER_TAG'd but
                // not a closure, so it needs its own probe alongside the
                // CLOSURE_MAGIC check.
                if type_tag == crate::closure::CLOSURE_MAGIC
                    || crate::symbol::is_registered_symbol(ptr_candidate as usize)
                {
                    found = true;
                    break;
                }
            }
        }
        found
    };

    // A `toJSON` can live as an OWN closure-typed field (a plain object
    // literal `{ toJSON() {…} }`) OR on the object's prototype / class-method
    // chain — a `class { toJSON() {…} }` instance stores `toJSON` on the class
    // vtable, and an `Object.create(proto)` result inherits it from `proto`.
    // Neither of those carries an own closure field, so the cheap
    // `has_closure_field` scan misses them; they DO carry a non-zero
    // `class_id` linking to the prototype/vtable (a plain data object literal
    // has `class_id == 0`), so probe `object_get_to_json` (which resolves
    // own+prototype via `js_object_get_field_by_name`) in that case too. This
    // is what lets `JSON.stringify` honour a prototype `toJSON` (#321 — Effect
    // `Inspectable`).
    let has_prototype_chain = (*obj).class_id != 0;
    if has_closure_field || has_prototype_chain {
        if let Some(to_json_val) = object_get_to_json(ptr) {
            if depth > MAX_FAST_DEPTH {
                STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
            }
            arm_to_json_result_guard(to_json_val);
            // Thread the current depth into the toJSON-result walk (do NOT
            // reset to the depth-0 `stringify_value` entry). A `toJSON` that
            // returns a structure re-entering an object still open higher in
            // the walk (`obj.toJSON = () => circular; circular.prop = obj`)
            // would otherwise recurse forever: each re-entry restarted at
            // depth 0, so the `depth > MAX_FAST_DEPTH` circular-detection push
            // never engaged and the stack overflowed (SIGSEGV). Accumulating
            // depth makes the detection fire and throw the spec TypeError
            // (test262 JSON/stringify/value-tojson-object-circular).
            stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            return;
        }
    }

    buf.push('{');
    let mut first = true;
    // Only own ENUMERABLE keys are serialized; gated on the process-wide
    // atomic AND the per-object `OBJ_FLAG_HAS_DESCRIPTORS` header flag
    // (#6009) — the global flag flips for good the first time ANY program
    // descriptor is installed, which made every later stringify pay a
    // per-key thread-local HashMap probe (`json_key_non_enumerable` +
    // `json_object_getter_value`) on objects that never had a descriptor.
    let filter_non_enum =
        crate::object::descriptors_in_use() && crate::object::object_has_descriptors(ptr as usize);
    // `pos(j)` maps the j-th enumerated slot to its key/field index: spec
    // order when array-index keys are present, else slot `j` (no allocation).
    let pos = |j: u32| -> u32 {
        match &key_order {
            Some(ord) => ord[j as usize],
            None => j,
        }
    };
    for j in 0..actual_fields {
        let f = pos(j);
        // Private elements (`#x`) live in a class instance's keys_array but are
        // not serializable own properties. (`has_prototype_chain` == class_id != 0.)
        if has_prototype_chain
            && crate::object::instance_private_key_hidden(
                cur_obj(),
                JSValue::from_bits(key_at(f).to_bits()),
            )
        {
            continue;
        }
        // Skip non-enumerable own keys (e.g. `Object.defineProperty(o, k,
        // { enumerable: false })`) before touching the value.
        if filter_non_enum && json_key_non_enumerable(cur_obj(), key_at(f)) {
            continue;
        }
        let mut field_bits = read_field_bits(f);
        // Own accessor properties: serialize the getter's return value (Node
        // invokes the getter), not the raw slot — which holds the getter
        // closure (object-literal `get x() {}`) or an empty placeholder
        // (`Object.defineProperty(o, k, { get })`). Gated on the descriptor flag.
        // The getter is USER CODE: every pointer below is re-derived from the
        // rooted handle after it returns.
        if filter_non_enum {
            if let Some(gv) = crate::object::json_object_getter_value(cur_obj(), key_at(f)) {
                field_bits = gv.to_bits();
            }
        }
        let mut field_val = f64::from_bits(field_bits);
        // Skip undefined per JSON spec (incl. a getter that returned undefined).
        if field_bits == TAG_UNDEFINED {
            continue;
        }
        // Skip closures and Symbols per JSON spec (only possible for
        // pointer-tagged values). Guarded by has_closure_field: if no field
        // is a closure/Symbol, the in-loop check is skipped entirely for
        // every field.
        if has_closure_field && (is_closure_value(field_bits) || is_symbol_value(field_bits)) {
            continue;
        }

        // Resolve the member key up front — needed both to record the `toJSON`
        // key (#5909) and to write the property name below.
        let key_f64 = key_at(f);
        let key_bits = key_f64.to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };

        // SerializeJSONProperty step 2 (#5909): apply a heap-valued member's
        // `toJSON` HERE, before the comma/key are written, so a member whose
        // `toJSON` returns `undefined` (or a function/Symbol) is OMITTED per
        // spec — the value recursion below runs `toJSON` only AFTER the key, so
        // such a member wrongly emitted `"k":null`. A member's `toJSON` key is
        // its own property name; the synthetic `field{f}` fallback name is
        // unreadable, so pass "" as it did before.
        let mut member_probed = false;
        if (field_bits & 0xFFFF_0000_0000_0000) == POINTER_TAG || is_raw_pointer(field_bits) {
            set_to_json_key_str(str_from_header(key_ptr).unwrap_or(""));
            if let Some(resolved) = member_to_json(field_val) {
                let rb = resolved.to_bits();
                if rb == TAG_UNDEFINED || is_closure_value(rb) || is_symbol_value(rb) {
                    continue;
                }
                field_bits = rb;
                field_val = resolved;
                // `toJSON` already ran; arm the one-shot guard so the resolved
                // value's own serialization doesn't invoke `toJSON` a second
                // time (SerializeJSONProperty applies it once). Disarmed after
                // the pointer dispatch below, gated on `member_probed`.
                arm_to_json_result_guard(resolved);
                member_probed = true;
            }
        }

        if !first {
            buf.push(',');
        }
        first = false;

        if let Some(key_str) = str_from_header(key_ptr) {
            // A key can itself contain `"`/`\`/control characters (e.g. a
            // `Symbol`-adjacent computed key or `Object.defineProperty`
            // literal name) — must go through the same escaper as string
            // values, not a raw `push_str` (test262
            // JSON/stringify/value-string-escape-ascii, where the property
            // name embeds all 32 ASCII control characters).
            write_escaped_string(buf, key_str);
            buf.push(':');
        } else {
            let _ = write!(buf, "\"field{}\":", f);
        }

        // Inline value dispatch for common types to avoid function call
        // overhead. `field_bits`/`field_val` are the post-`toJSON` value when a
        // `toJSON` ran above.
        let val_tag = field_bits & 0xFFFF_0000_0000_0000;
        if field_bits == TAG_NULL {
            buf.push_str("null");
        } else if field_bits == TAG_TRUE {
            buf.push_str("true");
        } else if field_bits == TAG_FALSE {
            buf.push_str("false");
        } else if val_tag == STRING_TAG {
            let str_ptr = (field_bits & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == crate::value::SHORT_STRING_TAG {
            // v0.5.213 SSO — decode inline 5-byte string and emit.
            let jsval = JSValue::from_bits(field_bits);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if val_tag == POINTER_TAG || is_raw_pointer(field_bits) {
            // Nested object/array (or the object/array `toJSON` returned). The
            // `toJSON` key was recorded above; `member_probed` armed the guard.
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
            if member_probed {
                SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            }
        } else {
            // Number (most common for data objects) — or Date, handled
            // centrally by `write_number` via DATE_REGISTRY lookup. A BigInt
            // member funnels through `write_number` to `serialize_bigint` /
            // `bigint_apply_to_json`, which reads the pending `toJSON` key, so
            // record this member's key first (#5909).
            if val_tag == BIGINT_TAG {
                set_to_json_key_str(str_from_header(key_ptr).unwrap_or(""));
            }
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    if depth > MAX_FAST_DEPTH {
        STRINGIFY_STACK.with(|s| s.borrow_mut().pop());
    }
}

pub(crate) unsafe fn stringify_array(ptr: *const u8, buf: &mut String) {
    stringify_array_depth(ptr, buf, 0)
}

/// Cached shape template for a homogeneous array of objects.
pub(crate) struct ShapeTemplate {
    pub(crate) keys_arr: *mut crate::ArrayHeader,
    pub(crate) prefixes: Vec<String>,
    pub(crate) shape_fields: u32,
    /// True when element 0's fields are all primitives (no POINTER_TAG /
    /// UNDEFINED). Lets the emit path skip its per-element pre-scan.
    pub(crate) primitive_only: bool,
}

/// Look up (or build & insert) the shape template for an object. Returns
/// `None` if the object isn't templatable (no keys array, too many fields,
/// malformed key strings) or if the cache is full and missed.
///
/// Returns a raw pointer because lifetimes can't survive the TLS borrow.
/// The pointer stays valid until the next `take_shape_cache` (top-level
/// entry/exit) — within one stringify traversal we only `push`, and
/// `Box`'s heap address is stable across `Vec` growth.
#[inline]
pub(crate) unsafe fn shape_template_for(obj_ptr: *const u8) -> Option<*const ShapeTemplate> {
    let obj = obj_ptr as *const crate::ObjectHeader;
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }

    SHAPE_CACHE.with(|c| {
        // Fast path: linear scan from the back — recently-used entries
        // cluster there for typical traversal orders (shape A's elements
        // recurse into shape B repeatedly).
        {
            let cache = c.borrow();
            for entry in cache.iter().rev() {
                if entry.0 == keys_arr {
                    return Some(&*entry.1 as *const ShapeTemplate);
                }
            }
            if cache.len() >= SHAPE_CACHE_CAP {
                return None;
            }
        }

        // Miss — build, insert, return raw pointer to the boxed template.
        let elem_bits = make_pointer_bits(obj_ptr);
        let template = build_shape_prefix_template(elem_bits)?;
        let mut cache = c.borrow_mut();
        // Re-check cap after the borrow round-trip (a recursive call
        // during template build could have filled the cache).
        if cache.len() >= SHAPE_CACHE_CAP {
            return None;
        }
        cache.push((keys_arr, Box::new(template)));
        Some(&*cache.last().unwrap().1 as *const ShapeTemplate)
    })
}

/// Build a per-shape key-prefix template for a homogeneous array of objects.
///
/// When every element of an array shares the same `keys_array` pointer (same
/// shape), we can pre-format the key portion of each field once and reuse it
/// across every element — turning the per-field key lookup (load key f64,
/// extract pointer, `str_from_header`, 3 `push`/`push_str` calls) into a
/// single `push_str` of a cached prefix.
///
/// Prefix layout for N fields with keys k0..kN-1:
///   `prefixes[0]   = "{\"k0\":"`        (opening brace fused with first key)
///   `prefixes[f>0] = ",\"kf\":"`        (comma fused with key)
/// Close with `}`. This compresses ~7 per-field write ops down to ~2.
///
/// Returns `None` when the first element isn't a regular object, the keys
/// array is invalid, or any key string is malformed — callers fall back to
/// the generic slow path in that case.
pub(crate) unsafe fn build_shape_prefix_template(first_elem_bits: u64) -> Option<ShapeTemplate> {
    let tag = first_elem_bits & 0xFFFF_0000_0000_0000;
    let first_ptr = if tag == POINTER_TAG {
        (first_elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(first_elem_bits) {
        first_elem_bits as *const u8
    } else {
        return None;
    };
    // Issue #639: Buffer / Uint8Array have no GcHeader, so `gc_obj_type`
    // would read 8 bytes before the BufferHeader (unrelated memory) and
    // could randomly return GC_TYPE_OBJECT. Bail to the per-element
    // slow path which dispatches via `is_registered_buffer`.
    if crate::buffer::is_registered_buffer(first_ptr as usize) {
        return None;
    }
    if gc_obj_type(first_ptr) != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = first_ptr as *const crate::ObjectHeader;
    // A non-zero `class_id` means this object may resolve `toJSON` (or other
    // serialization-affecting methods) on its prototype / class vtable — which
    // the prefix-template emit path can't see (it only inspects own fields).
    // Bail to the per-element slow path (`stringify_object_inner`), which
    // probes the prototype chain via `object_get_to_json`. Plain data object
    // literals and `JSON.parse` output carry `class_id == 0`, so the
    // array-of-objects fast path is unaffected for them. (#321 — a homogeneous
    // array of `class { toJSON() {…} }` instances must honour the prototype
    // `toJSON`.)
    if (*obj).class_id != 0 {
        return None;
    }
    // #6519: a URL instance is a class_id-0 object but must serialize as its
    // `href` string (handled by `stringify_object_inner`'s URL branch), never
    // as a templated dump of its 12 internal fields (which would also walk the
    // `searchParams` back-reference and throw). Bail so an array whose first
    // element is a URL routes every element through the per-element slow path.
    if crate::url::is_url_object_shape(obj as *mut crate::ObjectHeader) {
        return None;
    }
    let keys_arr = (*obj).keys_array;
    if keys_arr.is_null() {
        return None;
    }
    // #2438: array-index keys must enumerate first in ascending numeric order,
    // which the insertion-ordered prefix template can't express. Bail to the
    // generic slow path (`stringify_object_inner`), which reorders per spec.
    if crate::object::keys_contain_array_index(keys_arr) {
        return None;
    }
    let keys_len = (*keys_arr).length;
    let field_count = (*obj).field_count;
    let shape_fields = std::cmp::min(keys_len, field_count);
    if shape_fields == 0 || shape_fields > 32 {
        return None;
    }

    let keys_elements =
        (keys_arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let mut prefixes: Vec<String> = Vec::with_capacity(shape_fields as usize);
    for f in 0..shape_fields {
        let key_bits = (*keys_elements.add(f as usize)).to_bits();
        let key_tag = key_bits & 0xFFFF_0000_0000_0000;
        let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
            (key_bits & POINTER_MASK) as *const StringHeader
        } else {
            key_bits as *const StringHeader
        };
        let key_str = str_from_header(key_ptr)?;
        let needs_escape = key_str.bytes().any(|b| b == b'"' || b == b'\\' || b < 0x20);
        let mut prefix = String::with_capacity(key_str.len() + 4);
        prefix.push(if f == 0 { '{' } else { ',' });
        if needs_escape {
            write_escaped_string(&mut prefix, key_str);
        } else {
            prefix.push('"');
            prefix.push_str(key_str);
            prefix.push('"');
        }
        prefix.push(':');
        prefixes.push(prefix);
    }

    // Sample first element to decide whether every field slot is already
    // a primitive (number/bool/null/string). When true, per-element emit
    // can skip the undefined/closure pre-scan.
    let fields_ptr =
        (first_ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let mut primitive_only = true;
    for f in 0..shape_fields {
        let fb = (*fields_ptr.add(f as usize)).to_bits();
        if fb == TAG_UNDEFINED || (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            primitive_only = false;
            break;
        }
    }

    Some(ShapeTemplate {
        keys_arr,
        prefixes,
        shape_fields,
        primitive_only,
    })
}

/// Record field `f`'s property name (from the shape template's shared
/// `keys_arr`) as the pending `toJSON` key before recursing into that field
/// (#5909). Mirrors the key decode in `build_shape_prefix_template`.
#[inline]
unsafe fn set_to_json_key_for_template_field(template: &ShapeTemplate, f: usize) {
    let keys_elements = (template.keys_arr as *const u8)
        .add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64;
    let key_bits = (*keys_elements.add(f)).to_bits();
    let key_tag = key_bits & 0xFFFF_0000_0000_0000;
    let key_ptr = if key_tag == STRING_TAG || key_tag == POINTER_TAG {
        (key_bits & POINTER_MASK) as *const StringHeader
    } else {
        key_bits as *const StringHeader
    };
    set_to_json_key_str(str_from_header(key_ptr).unwrap_or(""));
}

/// Fast emission path for an object element that matches the cached shape
/// template. Returns `true` when the element was emitted via the template;
/// `false` when the element diverges (different shape, skippable field, or
/// has a `toJSON` that must produce the replacement value). On `false` the
/// buffer is unchanged — the caller is responsible for falling back.
pub(crate) unsafe fn try_emit_shape_element(
    elem_bits: u64,
    template: &ShapeTemplate,
    buf: &mut String,
    depth: u32,
) -> bool {
    let tag = elem_bits & 0xFFFF_0000_0000_0000;
    let elem_ptr = if tag == POINTER_TAG {
        (elem_bits & POINTER_MASK) as *const u8
    } else if is_raw_pointer(elem_bits) {
        elem_bits as *const u8
    } else {
        return false;
    };
    if gc_obj_type(elem_ptr) != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    let obj = elem_ptr as *const crate::ObjectHeader;
    if (*obj).keys_array != template.keys_arr {
        return false;
    }

    let fields_ptr =
        (elem_ptr as *const u8).add(std::mem::size_of::<crate::ObjectHeader>()) as *const f64;
    let shape_fields = template.shape_fields;
    let prefixes = template.prefixes.as_slice();

    // Primitive-only fast path (common case for JSON.parse output): skip
    // the undefined/closure pre-scan and trust that the sampled element 0
    // was representative. The emit loop handles stray POINTER_TAG values
    // via `stringify_value_depth`; a stray UNDEFINED is rare enough that
    // we save `buf.len()` pre-emit and roll back on detection.
    if template.primitive_only {
        let save_pos = buf.len();
        for f in 0..shape_fields as usize {
            let field_val = *fields_ptr.add(f);
            let fb = field_val.to_bits();
            // UNDEFINED desyncs comma placement → roll back and let the
            // slow object path emit this element correctly.
            if fb == TAG_UNDEFINED {
                buf.truncate(save_pos);
                return false;
            }
            buf.push_str(&prefixes[f]);
            let vtag = fb & 0xFFFF_0000_0000_0000;
            if fb == TAG_NULL {
                buf.push_str("null");
            } else if fb == TAG_TRUE {
                buf.push_str("true");
            } else if fb == TAG_FALSE {
                buf.push_str("false");
            } else if vtag == STRING_TAG {
                let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
                if let Some(s) = str_from_header(str_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == crate::value::SHORT_STRING_TAG {
                let jsval = JSValue::from_bits(fb);
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsval.short_string_to_buf(&mut scratch);
                if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
            } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
                set_to_json_key_for_template_field(template, f);
                stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
            } else {
                // A BigInt field reaches `serialize_bigint` via `write_number`,
                // which reads the pending `toJSON` key — record it first (#5909).
                if vtag == BIGINT_TAG {
                    set_to_json_key_for_template_field(template, f);
                }
                write_number(buf, field_val);
            }
        }
        buf.push('}');
        return true;
    }

    // General path: template contains (or may contain) pointer/undefined
    // fields. Pre-scan to honor JSON spec (skip undefined, skip closures,
    // respect toJSON).
    let mut has_pointer_fields = false;
    for f in 0..shape_fields as usize {
        let fb = (*fields_ptr.add(f)).to_bits();
        if fb == TAG_UNDEFINED {
            return false;
        }
        if (fb & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            has_pointer_fields = true;
            if is_closure_value(fb) || is_symbol_value(fb) {
                return false;
            }
        }
    }
    if has_pointer_fields {
        if let Some(to_json_val) = object_get_to_json(elem_ptr) {
            arm_to_json_result_guard(to_json_val);
            stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth + 1);
            SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
            return true;
        }
    }
    for f in 0..shape_fields as usize {
        buf.push_str(&prefixes[f]);
        let field_val = *fields_ptr.add(f);
        let fb = field_val.to_bits();
        let vtag = fb & 0xFFFF_0000_0000_0000;
        if fb == TAG_NULL {
            buf.push_str("null");
        } else if fb == TAG_TRUE {
            buf.push_str("true");
        } else if fb == TAG_FALSE {
            buf.push_str("false");
        } else if vtag == STRING_TAG {
            let str_ptr = (fb & POINTER_MASK) as *const StringHeader;
            if let Some(s) = str_from_header(str_ptr) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == crate::value::SHORT_STRING_TAG {
            let jsval = JSValue::from_bits(fb);
            let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            let n = jsval.short_string_to_buf(&mut scratch);
            if let Ok(s) = std::str::from_utf8(&scratch[..n]) {
                write_escaped_string(buf, s);
            } else {
                buf.push_str("null");
            }
        } else if vtag == POINTER_TAG || is_raw_pointer(fb) {
            set_to_json_key_for_template_field(template, f);
            stringify_value_depth(field_val, TYPE_UNKNOWN, buf, depth + 1);
        } else {
            // A BigInt field reaches `serialize_bigint` via `write_number`,
            // which reads the pending `toJSON` key — record it first (#5909).
            if vtag == BIGINT_TAG {
                set_to_json_key_for_template_field(template, f);
            }
            write_number(buf, field_val);
        }
    }
    buf.push('}');
    true
}

/// Depth-aware variant of stringify_array for recursive calls.
pub(crate) unsafe fn stringify_array_depth(ptr: *const u8, buf: &mut String, depth: u32) {
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
        stringify_value(to_json_val, TYPE_UNKNOWN, buf);
        SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
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
    // Root the array and re-derive the element base per access: a nested
    // `toJSON` / getter / any allocation inside the recursive serialization
    // below can trigger a GC that sweeps or moves this array while a hoisted
    // `elements` pointer still aims at the old storage (no conservative
    // stack scan in production; alloc-point minors can be moving under the
    // evacuation policy).
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let elem_at = |i: usize| -> f64 {
        let arr = arr_handle.get_raw_const_ptr::<crate::ArrayHeader>();
        *((arr as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const f64).add(i)
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
        buf.push('[');
        for i in 0..len {
            if i > 0 {
                buf.push(',');
            }
            // An element's `toJSON` key is its stringified index (#5909). Set
            // before the shape emit (which may run the element's own `toJSON`)
            // and the per-element fallback below.
            set_to_json_key_index(i as usize);
            // Re-derived per element: the previous element's serialization can
            // have run user code / allocated (and moved this array).
            let elem = elem_at(i as usize);
            let elem_bits = elem.to_bits();
            if !try_emit_shape_element(elem_bits, tmpl, buf, depth) {
                // Match the slow path: array descent does not bump depth.
                stringify_value_depth(elem, TYPE_UNKNOWN, buf, depth);
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
                    stringify_value_depth(to_json_val, TYPE_UNKNOWN, buf, depth);
                    SUPPRESS_NEXT_TO_JSON.with(|c| c.set(false));
                    continue;
                }
                stringify_value_depth(prim, TYPE_UNKNOWN, buf, depth);
                continue;
            }
            // #2089: a Date element → its toJSON() ISO string (or null),
            // before any object/array deref of the small cell.
            if crate::date::is_date_cell_addr(elem_ptr as usize) {
                let s_ptr = crate::date::js_date_to_json(elem);
                if let Some(s) = str_from_header(s_ptr) {
                    write_escaped_string(buf, s);
                } else {
                    buf.push_str("null");
                }
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
                crate::gc::GC_TYPE_OBJECT => stringify_object_inner(elem_ptr, buf, depth),
                crate::gc::GC_TYPE_ARRAY => stringify_array_depth(elem_ptr, buf, depth),
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
                        stringify_object_inner(elem_ptr, buf, depth);
                    } else {
                        let arr_elem = elem_ptr as *const crate::ArrayHeader;
                        let arr_len = (*arr_elem).length;
                        let arr_cap = (*arr_elem).capacity;
                        if arr_len <= arr_cap && arr_cap > 0 && arr_cap < 10000 {
                            stringify_array_depth(elem_ptr, buf, depth);
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
            let fields = (*obj).field_count as usize;
            return (fields * 200).max(256);
        }
    }
    4096
}

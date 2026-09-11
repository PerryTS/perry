//! toJSON resolution and primitive dispatch for the replacer walk.
use super::*;

/// Resolve `value.toJSON(key)` if `value` is an object with a callable
/// `toJSON` field, per spec `SerializeJSONProperty` step 2 (run BEFORE the
/// replacer). Mirrors the no-replacer path's `object_get_to_json`, which only
/// fires when the object actually has a closure-typed `toJSON` field. Returns
/// the (possibly substituted) value.
unsafe fn apply_to_json(value: f64) -> f64 {
    let bits = value.to_bits();
    // A BigInt is a primitive, not a POINTER_TAG value — `extract_pointer`
    // below never matches it, so without this check `BigInt.prototype.toJSON`
    // is silently skipped for a top-level/replacer-walked BigInt (test262
    // JSON/stringify/value-bigint-order). Mirror the no-replacer path's
    // `serialize_bigint`, which already applies this.
    if (bits & 0xFFFF_0000_0000_0000) == BIGINT_TAG {
        if let Some(converted) = crate::json::stringify::bigint_apply_to_json(value) {
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
                crate::json::stringify::array_get_to_json(ptr as *const crate::ArrayHeader)
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

/// Resolve `value.toJSON(key)` (spec `SerializeJSONProperty` step 2 — run
/// BEFORE the replacer). `key_f64` is the property key passed to `toJSON`.
#[inline]
pub(super) unsafe fn apply_to_json_keyed(value: f64, key_f64: f64) -> f64 {
    // SerializeJSONProperty step 2.b.i passes the property key to `toJSON`
    // (#5909, test262 JSON/stringify/value-tojson-arguments). The replacer walk
    // already carries the key here (empty String at the root, own key for a
    // member, stringified index for an element); record it so the shared
    // `object_get_to_json` / `array_get_to_json` / `bigint_apply_to_json` probes
    // hand it to `toJSON`.
    // Only objects and BigInt can have a toJSON hook. Primitive properties
    // still reach the replacer, but need no owned pending-key copy for toJSON.
    // Keep legacy raw-pointer classification identical to apply_to_json.
    let bits = value.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) != BIGINT_TAG && extract_pointer(bits).is_none() {
        return value;
    }
    set_to_json_key_value(key_f64);
    apply_to_json(value)
}

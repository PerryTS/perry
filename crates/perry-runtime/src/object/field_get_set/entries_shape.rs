//! `Object.entries` over the shape alone.
//!
//! Split out of `enumeration.rs` to keep it under the 2000-line size gate.

use super::enumeration::*;
use super::*;

/// [`js_object_entries`] over the shape alone.
pub(super) fn js_object_entries_shape(obj: *const ObjectHeader) -> *mut ArrayHeader {
    // #8149: a registered BUFFER receiver — node `Buffer`, `Uint8Array`,
    // `ArrayBuffer`, `SharedArrayBuffer` or `DataView`. Asked FIRST, above the
    // `is_valid_obj_ptr` guard: a `BufferHeader` is not an `ObjectHeader`, and
    // the generic walk below reads its payload bytes as `keys_array` — `[]`
    // when they are zero, SIGBUS in `js_array_length` when they are not.
    // See `registered_buffer_own_keys`.
    if let Some(result) = registered_buffer_enum(strip_nanbox_addr(obj), MapSetEnum::Entries) {
        return result;
    }
    let stripped = {
        let bits = obj as u64;
        let top16 = bits >> 48;
        if top16 == 0x7FFD || top16 >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ObjectHeader
        } else {
            obj
        }
    };
    // Map/Set receiver → no own enumerable properties; see the matching
    // guard in `js_object_keys` for the rationale.
    if crate::map::is_registered_map(stripped as usize)
        || crate::set::is_registered_set(stripped as usize)
    {
        return map_set_exotic_enum(stripped, MapSetEnum::Entries);
    }
    if let Some(addr) =
        crate::typedarray_props::typed_array_addr_from_value(f64::from_bits(obj as u64))
    {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                addr as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    if crate::typedarray::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                stripped as *const crate::typedarray::TypedArrayHeader,
            )
        };
    }
    // Arrays: emit [index, value] pairs for present elements, then named props.
    // `js_object_entries` has no `ArrayHeader` layout, so the generic object
    // path below would read an array's body as object fields and crash; handle
    // arrays explicitly (mirrors the `js_object_keys` / `js_object_values`
    // array branches).
    if !stripped.is_null() && (stripped as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        unsafe {
            let gc_header = (stripped as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
                let length = (*arr).length;
                if length > 100_000 {
                    return crate::array::js_array_alloc(0);
                }
                let elements = (arr as *const u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *const u64;
                let result = crate::array::js_array_alloc(length);
                for i in 0..length {
                    if std::ptr::read(elements.add(i as usize)) == crate::value::TAG_HOLE {
                        continue;
                    }
                    let pair = crate::array::js_array_alloc(2);
                    let s = i.to_string();
                    let key_box = crate::string::js_string_new_sso(s.as_ptr(), s.len() as u32);
                    crate::array::js_array_push_f64(pair, key_box);
                    let v = crate::array::js_array_get(arr, i);
                    crate::array::js_array_push_f64(pair, f64::from_bits(v.bits()));
                    crate::array::js_array_push_f64(
                        result,
                        crate::value::js_nanbox_pointer(pair as i64),
                    );
                }
                for name in crate::array::array_named_property_names(arr, true) {
                    if let Some(v) = crate::array::array_named_property_get_by_name(arr, &name) {
                        let pair = crate::array::js_array_alloc(2);
                        let key =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        crate::array::js_array_push(pair, JSValue::string_ptr(key));
                        crate::array::js_array_push_f64(pair, v);
                        crate::array::js_array_push_f64(
                            result,
                            crate::value::js_nanbox_pointer(pair as i64),
                        );
                    }
                }
                return result;
            }
        }
    }
    if obj.is_null() || !is_valid_obj_ptr(obj as *const u8) {
        // Issue #893 lineage: chalk's `Object.entries(ansiStyles)` passed a
        // value whose unboxed low-48 bits weren't a real heap pointer
        // (cross-module import where the default-export wrapper hasn't
        // finished initializing). Pre-fix the `crate::object::object_keys_array(obj)` deref
        // SIGSEGV'd at 0x14; now we return an empty array so the user's
        // `for (const [k, v] of Object.entries(undefined)) {}` no-ops the
        // way the spec's "abstract conversion to object" path would for
        // an unrecognized receiver. Real JS throws TypeError here; we
        // prefer the empty-array fallback because Perry doesn't have a
        // clean "throw at codegen-call boundaries" path for these
        // pointer-typed entry points and a segfault is strictly worse
        // for the caller.
        return crate::array::js_array_alloc(0);
    }
    unsafe {
        if let Some(result) = super::super::string_wrapper::enumerate(
            obj,
            super::super::string_wrapper::Enumeration::Entries,
        ) {
            return result;
        }
        if (*obj).class_id == NATIVE_MODULE_CLASS_ID {
            if let Some(result) = native_module_enum(obj, MapSetEnum::Entries) {
                return result;
            }
        }
        let keys = crate::object::object_keys_array(obj);
        // Iterate up to keys_len (the logical property count), not
        // field_count. Parser-built and dict-built objects with ≥9
        // fields cap field_count at the inline alloc_limit (8) and
        // store overflow values in OVERFLOW_FIELDS — for those,
        // field_count under-counts the actual property count by N-8.
        // Without this fix, `Object.entries(obj)` on a 50-key dict
        // returned only the first 8 entries (silent data loss).
        // Mirrors the same fix in `js_object_keys` and the
        // `actual_fields = keys_len` line in `json.rs::stringify_object`.
        let count = if !keys.is_null() {
            crate::array::js_array_length(keys) as usize
        } else {
            crate::object::object_live_slot_count(obj) as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        // #2438: emit pairs in OrdinaryOwnPropertyKeys order (array-index keys
        // first, ascending; then string keys in insertion order).
        let order = ecma_own_key_order(keys);
        let pos = |j: usize| -> u32 {
            match &order {
                Some(ord) => ord[j],
                None => j as u32,
            }
        };
        // Spec (EnumerableOwnProperties): the own key list is determined ONCE up
        // front, then `[[Get]]` is invoked per key. A getter that adds, removes,
        // or hides a future key during enumeration must not change the set of
        // entries reported (test262 entries/getter-adding-key,
        // getter-removing-future-key, getter-making-future-key-nonenumerable).
        //
        // Snapshot the own key *bytes* (not NaN-boxed pointers): a getter fired
        // by `js_object_get_field_by_name` can delete a future key and
        // allocate/GC before we visit it, and a key kept only inside this
        // Rust-heap `Vec` is not a stack-visible GC root — it could dangle.
        // Owning the bytes and rematerializing the string at read time sidesteps
        // that. Enumerability is likewise re-evaluated per key in the read phase
        // (an earlier getter can create a descriptor or flip a future key's
        // enumerability), so we deliberately do NOT filter it during the snapshot.
        let mut snapshot_keys: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..count {
            let i = pos(j);
            if keys.is_null() || i >= crate::array::js_array_length(keys) {
                continue;
            }
            let key_val = crate::array::js_array_get(keys, i);
            if instance_private_key_hidden(obj, key_val) {
                continue;
            }
            if let Some(bytes) = crate::string::js_string_key_bytes(key_val, &mut key_buf) {
                snapshot_keys.push(bytes.to_vec());
            }
        }

        for key_bytes in snapshot_keys {
            let key_str =
                crate::string::js_string_from_bytes(key_bytes.as_ptr(), key_bytes.len() as u32);
            if key_str.is_null() {
                continue;
            }
            // Spec EnumerableOwnProperties re-reads `[[GetOwnProperty]]` per key
            // and skips it when the descriptor is now undefined or no longer
            // enumerable — a getter earlier in the loop may have deleted or
            // hidden a key that was in the initial snapshot (test262
            // entries/getter-removing-future-key, getter-making-future-key-
            // nonenumerable).
            if !super::super::own_key_present(obj as *mut ObjectHeader, key_str) {
                continue;
            }
            if descriptor_marks_non_enumerable(obj, JSValue::string_ptr(key_str)) {
                continue;
            }
            // Create a pair array [key, value].
            let pair = crate::array::js_array_alloc(2);
            crate::array::js_array_push_f64(
                pair,
                f64::from_bits(JSValue::string_ptr(key_str).bits()),
            );

            // Read the value through the name-keyed `[[Get]]`, which fires an
            // own accessor's getter (the raw index-based field read returned the
            // empty data slot for accessor-defined properties — test262
            // entries/getter-adding-key expected the getter's "B").
            let value = js_object_get_field_by_name(obj as *const ObjectHeader, key_str);
            crate::array::js_array_push_f64(pair, f64::from_bits(value.bits()));

            // Push the pair to result (NaN-box the array pointer)
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            crate::array::js_array_push_f64(result, pair_boxed);
        }

        result
    }
}

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
                addr as *const crate::typedarray_hook_slots::TypedArrayHeader,
            )
        };
    }
    if crate::typedarray_hook_slots::lookup_typed_array_kind(stripped as usize).is_some() {
        return unsafe {
            crate::typedarray_props::typed_array_own_enumerable_entries(
                stripped as *const crate::typedarray_hook_slots::TypedArrayHeader,
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
            if crate::gc_type_is_array!((*gc_header).obj_type) {
                let arr = crate::array::clean_arr_ptr(stripped as *const crate::array::ArrayHeader);
                return array_own_enumerable(arr, ArrayEnum::Entries);
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
        let keys_view = crate::object::object_keys(obj);
        let keys = keys_view.arr();
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
            keys_view.count() as usize
        } else {
            crate::object::object_live_slot_count(obj) as usize
        };
        let result = crate::array::js_array_alloc(count as u32);

        // #2438: emit pairs in OrdinaryOwnPropertyKeys order (array-index keys
        // first, ascending; then string keys in insertion order).
        let order = ecma_own_key_order(keys_view);
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
        // The shape answers for every key at once: an own key is hidden
        // only when the receiver is a class instance or its shape has a
        // private entry (#11791), and no key of this snapshot becomes one.
        let hide_private = own_keys_may_hide(obj);
        let mut snapshot_keys: Vec<Vec<u8>> = Vec::with_capacity(count);
        let mut key_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for j in 0..count {
            let i = pos(j);
            if keys.is_null() || i >= keys_view.count() {
                continue;
            }
            let key_val = keys_view.get(i);
            if hide_private && own_slot_hidden(obj, i, key_val) {
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

/// What [`array_own_enumerable`] collects of each own enumerable property.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ArrayEnum {
    Keys,
    Values,
    Entries,
}

/// `Object.keys` / `Object.values` / `Object.entries` of a plain Array:
/// EnumerableOwnProperties in OrdinaryOwnPropertyKeys order. The present index
/// elements ascending (the store's, then the far ones a sparse Array keeps in
/// its named layout), then the enumerable named keys in insertion order; each
/// value by `Get`. No slot past the store is read, and the length of the Array
/// does not limit the answer. The receiver, the result and each pair under
/// construction are rooted: a getter and every allocation here can collect.
///
/// # Safety
/// `arr` is a live, forwarding-resolved plain Array.
pub(super) unsafe fn array_own_enumerable(
    arr: *const crate::array::ArrayHeader,
    mode: ArrayEnum,
) -> *mut crate::array::ArrayHeader {
    use crate::array::{clean_arr_ptr, ArrayHeader};
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_raw_const_ptr(arr);
    let result = scope.root_raw_mut_ptr(crate::array::js_array_alloc(0));
    let dense = (*arr).length.min((*arr).capacity);
    let (mut far, named): (Vec<String>, Vec<String>) =
        crate::array::array_named_property_enumeration_names(arr, true, dense)
            .into_iter()
            .partition(|name| crate::object::canonical_array_index(name).is_some());
    far.sort_by_key(|name| crate::object::canonical_array_index(name));
    let push = |value: f64| {
        let grown = result.with_mut_ptr(|out| crate::array::js_array_push_f64(out, value));
        result.set_raw_mut_ptr(grown);
    };
    let emit = |key: &str, value: f64| {
        if mode == ArrayEnum::Values {
            push(value);
            return;
        }
        if mode == ArrayEnum::Keys {
            let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            push(f64::from_bits(JSValue::string_ptr(key).bits()));
            return;
        }
        // One scope per pair: the handles of a pair die with it.
        let pair_scope = crate::gc::RuntimeHandleScope::new();
        let value_h = pair_scope.root_nanbox_f64(value);
        let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let key_h = pair_scope.root_nanbox_f64(f64::from_bits(JSValue::string_ptr(key).bits()));
        let pair = crate::array::js_array_alloc(2);
        let pair = crate::array::js_array_push_f64(pair, key_h.get_nanbox_f64());
        let pair = crate::array::js_array_push_f64(pair, value_h.get_nanbox_f64());
        push(crate::value::js_nanbox_pointer(pair as i64));
    };
    let mut index = 0u32;
    loop {
        let live = clean_arr_ptr(source.get_raw_const_ptr::<ArrayHeader>());
        if index >= dense.min((*live).length).min((*live).capacity) {
            break;
        }
        let slot = *(crate::array::array_elements_ptr(live) as *const u64).add(index as usize);
        if slot != crate::value::TAG_HOLE {
            // `Get` only when the value is wanted: a getter runs for
            // values / entries, never for keys.
            let value = if mode == ArrayEnum::Keys {
                0.0
            } else {
                f64::from_bits(crate::array::js_array_get(live, index).bits())
            };
            emit(&index.to_string(), value);
        }
        index += 1;
    }
    for name in far.iter().chain(named.iter()) {
        let live = clean_arr_ptr(source.get_raw_const_ptr::<ArrayHeader>());
        if let Some(value) = crate::array::array_named_property_get_by_name(live, name) {
            emit(name, value);
        } else if mode == ArrayEnum::Keys && crate::object::canonical_array_index(name).is_none() {
            emit(name, 0.0);
        }
    }
    result.get_raw_mut_ptr::<ArrayHeader>()
}

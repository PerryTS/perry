//! Unit tests.

use std::ptr;

use super::*;

extern "C" fn test_map_to_string(
    _closure: *const crate::closure::ClosureHeader,
    _element: f64,
    _index: f64,
) -> f64 {
    let str_ptr = crate::string::js_string_from_bytes(b"mapped".as_ptr(), 6);
    f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK))
}

fn gc_collection_count_for_tests() -> u64 {
    let mut collections = 0;
    crate::gc::js_gc_stats(&mut collections, ptr::null_mut(), ptr::null_mut());
    collections
}

fn assert_numeric_raw_values(arr: *mut ArrayHeader, expected: &[f64]) {
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_length(arr), expected.len() as u32);
    for (index, value) in expected.iter().enumerate() {
        assert_eq!(js_array_numeric_get_f64_unboxed(arr, index as u32), *value);
    }
}

fn int32_jsvalue_bits(value: i32) -> u64 {
    crate::value::JSValue::int32(value).bits()
}

fn assert_canonical_raw_slot(arr: *mut ArrayHeader, index: u32, expected: f64) {
    let raw_bits = js_array_get_f64_unchecked(arr, index).to_bits();
    assert_eq!(raw_bits, expected.to_bits());
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, index), expected);
}

unsafe fn raw_slot_bits(arr: *mut ArrayHeader, index: usize) -> u64 {
    let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    *elements.add(index)
}

fn string_key(name: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn boxed_pointer(ptr: *mut u8) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn string_value(ptr: *mut crate::StringHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

#[test]
fn flattenable_array_ptr_accepts_only_arrays_and_array_proxies() {
    let array = js_array_alloc(0);
    let array_value = boxed_pointer(array as *mut u8);
    assert_eq!(flattenable_array_ptr(array_value), array);

    let object = crate::object::js_object_alloc(0, 0);
    assert!(flattenable_array_ptr(boxed_pointer(object as *mut u8)).is_null());

    let closure = crate::closure::js_closure_alloc(ptr::null(), 0);
    assert!(flattenable_array_ptr(boxed_pointer(closure as *mut u8)).is_null());

    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy::js_proxy_new(array_value, boxed_pointer(handler as *mut u8));
    assert_eq!(flattenable_array_ptr(proxy), array);

    let nested_proxy = crate::proxy::js_proxy_new(proxy, boxed_pointer(handler as *mut u8));
    assert_eq!(flattenable_array_ptr(nested_proxy), array);
}

fn array_keys_contain(keys: *mut ArrayHeader, name: &[u8]) -> bool {
    let key = string_key(name);
    for i in 0..js_array_length(keys) {
        let stored = js_array_get(keys, i);
        if unsafe { crate::string::js_string_key_matches(stored, key) } {
            return true;
        }
    }
    false
}

#[test]
fn test_array_alloc_and_access() {
    let arr = js_array_alloc(5);

    // Initially empty
    assert_eq!(js_array_length(arr), 0);

    // Push some values
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 0), 1.0);
    assert_eq!(js_array_get_f64(arr, 1), 2.0);
    assert_eq!(js_array_get_f64(arr, 2), 3.0);

    // Out of bounds returns TAG_UNDEFINED (JS spec: arr[OOB] === undefined)
    assert_eq!(js_array_get_f64(arr, 5).to_bits(), 0x7FFC_0000_0000_0001u64);
}

#[test]
fn test_array_hole_is_not_own_property_but_undefined_value_is() {
    let mut holey = js_array_alloc(0);
    holey = js_array_push_hole(holey);
    assert_eq!(js_array_length(holey), 1);
    assert_eq!(
        js_array_get_f64(holey, 0).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    assert_eq!(
        crate::object::js_object_has_own(
            boxed_pointer(holey as *mut u8),
            string_value(string_key(b"0"))
        )
        .to_bits(),
        crate::value::TAG_FALSE
    );

    let mut explicit = js_array_alloc(0);
    explicit = js_array_push_f64(explicit, f64::from_bits(crate::value::TAG_UNDEFINED));
    assert_eq!(
        crate::object::js_object_has_own(
            boxed_pointer(explicit as *mut u8),
            string_value(string_key(b"0"))
        )
        .to_bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn test_array_exotic_named_indices_and_boundary_props() {
    let mut arr = js_array_alloc(0);
    let arr_obj = arr as *mut crate::object::ObjectHeader;

    let idx2 = string_key(b"2");
    arr = js_array_set_string_key(arr, idx2, 42.0);
    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 2), 42.0);

    let key0 = string_key(b"0");
    let key2 = string_key(b"2");
    assert_eq!(
        crate::object::js_object_has_own(boxed_pointer(arr as *mut u8), string_value(key0))
            .to_bits(),
        crate::value::TAG_FALSE
    );
    assert_eq!(
        crate::object::js_object_has_own(boxed_pointer(arr as *mut u8), string_value(key2))
            .to_bits(),
        crate::value::TAG_TRUE
    );

    let max_key = string_key(b"4294967295");
    arr = js_array_set_string_key(arr, max_key, 99.0);
    assert_eq!(js_array_length(arr), 3, "2^32-1 is not an array index");
    assert_eq!(
        crate::object::js_object_get_field_by_name(arr_obj, max_key).bits(),
        99.0f64.to_bits()
    );

    let foo = string_key(b"foo");
    crate::object::js_object_set_field_by_name(arr_obj, foo, 7.0);
    assert_eq!(
        crate::object::js_object_get_field_by_name(arr_obj, foo).bits(),
        7.0f64.to_bits()
    );

    let keys = crate::object::js_object_keys(arr_obj);
    assert!(array_keys_contain(keys, b"2"));
    assert!(array_keys_contain(keys, b"foo"));
    assert!(array_keys_contain(keys, b"4294967295"));
    assert!(!array_keys_contain(keys, b"0"));
}

#[test]
fn test_array_sparse_max_valid_index_boundary() {
    let mut arr = js_array_alloc(3);
    arr = js_array_push_f64(arr, 0.0);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);

    let max_index = u32::MAX - 1;
    arr = js_array_set_f64_extend(arr, max_index, 77.0);

    assert_eq!(js_array_length(arr), u32::MAX);
    assert_eq!(js_array_get_f64(arr, max_index), 77.0);
    assert_eq!(
        js_array_get_index_or_string(arr, max_index as f64).to_bits(),
        77.0f64.to_bits()
    );

    let key = string_key(b"4294967294");
    assert_eq!(
        crate::object::js_object_has_own(boxed_pointer(arr as *mut u8), string_value(key))
            .to_bits(),
        crate::value::TAG_TRUE
    );
}

/// Sequential growth (gap 0) must stay on the dense backing store past
/// MAX_DENSE_ARRAY_GROW_LENGTH — routing it to string-keyed sparse properties
/// is quadratic and hung the 10M-element 03_array_write benchmark for 6 hours
/// per CI run (v0.5.1129–v0.5.1150). Only far jumps past the current length
/// (the boundary test above) belong in sparse storage.
#[test]
fn test_array_sequential_growth_past_dense_threshold_stays_dense() {
    let mut arr = js_array_alloc(0);
    const N: u32 = 1_200_000; // past MAX_DENSE_ARRAY_GROW_LENGTH (1M)
    for i in 0..N {
        arr = js_array_set_f64_extend(arr, i, i as f64);
    }
    assert_eq!(js_array_length(arr), N);
    unsafe {
        assert!(
            (*arr).capacity >= N,
            "sequential fill fell off the dense path: capacity {} < length {}",
            (*arr).capacity,
            N
        );
    }
    assert_eq!(js_array_get_f64(arr, N - 1), (N - 1) as f64);
    assert_eq!(js_array_get_f64(arr, 1_000_001), 1_000_001.0);
}

#[test]
fn test_array_exotic_descriptors_and_global_prototype_identity() {
    let arr = js_array_alloc(0);
    let arr_box = boxed_pointer(arr as *mut u8);
    let length_desc = crate::object::js_object_get_own_property_descriptor(
        arr_box,
        string_value(string_key(b"length")),
    );
    let value_key = string_key(b"value");
    let writable_key = string_key(b"writable");
    let enumerable_key = string_key(b"enumerable");
    let length_desc_obj =
        crate::value::js_nanbox_get_pointer(length_desc) as *const crate::object::ObjectHeader;
    assert_eq!(
        crate::object::js_object_get_field_by_name(length_desc_obj, value_key).bits(),
        0.0f64.to_bits()
    );
    assert_eq!(
        crate::object::js_object_get_field_by_name(length_desc_obj, writable_key).bits(),
        crate::value::TAG_TRUE
    );
    assert_eq!(
        crate::object::js_object_get_field_by_name(length_desc_obj, enumerable_key).bits(),
        crate::value::TAG_FALSE
    );

    // This test reads the realm intrinsics (`Array`, `Array.prototype`,
    // `Array.prototype.constructor`) and compares their identities. It holds
    // those raw pointers as Rust locals across calls that allocate (string keys,
    // descriptor objects), so a GC mid-sequence can move/reclaim them — and the
    // libtest harness runs each test on its own thread, where the process-global
    // `GLOBAL_THIS_PTR` can be re-created (see `js_get_global_this`). Resolve and
    // validate the whole snapshot inside one iteration, re-reading every key, and
    // retry until a GC-quiet iteration yields a fully self-consistent view.
    let mut array_ctor = crate::value::JSValue::undefined();
    let mut proto = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut consistent = false;
    for _ in 0..256 {
        let global = crate::object::js_get_global_this();
        let global_ptr =
            crate::value::js_nanbox_get_pointer(global) as *const crate::object::ObjectHeader;
        array_ctor = crate::object::js_object_get_field_by_name(global_ptr, string_key(b"Array"));
        if !array_ctor.is_pointer() {
            std::thread::yield_now();
            continue;
        }
        let ctor_ptr =
            crate::value::js_nanbox_get_pointer(f64::from_bits(array_ctor.bits())) as usize;
        proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        if js_array_is_array(proto).to_bits() != crate::value::TAG_TRUE {
            std::thread::yield_now();
            continue;
        }
        let proto_obj =
            crate::value::js_nanbox_get_pointer(proto) as *const crate::object::ObjectHeader;
        let literal_to_string = crate::object::js_object_get_field_by_name(
            arr as *const crate::object::ObjectHeader,
            string_key(b"toString"),
        );
        let proto_to_string =
            crate::object::js_object_get_field_by_name(proto_obj, string_key(b"toString"));
        let constructor_desc = crate::object::js_object_get_own_property_descriptor(
            proto,
            string_value(string_key(b"constructor")),
        );
        let constructor_desc_obj = crate::value::js_nanbox_get_pointer(constructor_desc)
            as *const crate::object::ObjectHeader;
        let constructor_value =
            crate::object::js_object_get_field_by_name(constructor_desc_obj, string_key(b"value"));
        // `Array.prototype.toString === arr.toString` and
        // `Object.getOwnPropertyDescriptor(Array.prototype, 'constructor').value === Array`
        // must both hold against the *same* freshly-read intrinsics.
        if literal_to_string.bits() == proto_to_string.bits()
            && constructor_value.bits() == array_ctor.bits()
        {
            consistent = true;
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        consistent,
        "realm Array intrinsics did not present a self-consistent snapshot within 256 tries"
    );
    assert_eq!(js_array_is_array(proto).to_bits(), crate::value::TAG_TRUE);

    let array_from_call = unsafe {
        let args = [0.0, 1.0, 0.0, 1.0];
        crate::closure::js_native_call_value(
            f64::from_bits(array_ctor.bits()),
            args.as_ptr(),
            args.len(),
        )
    };
    let called_arr = crate::value::js_nanbox_get_pointer(array_from_call) as *const ArrayHeader;
    assert_eq!(js_array_length(called_arr), 4);
}

#[test]
fn test_array_from_f64() {
    let values = [10.0, 20.0, 30.0, 40.0, 50.0];
    let arr = js_array_from_f64(values.as_ptr(), 5);

    assert_eq!(js_array_length(arr), 5);
    assert_eq!(js_array_get_f64(arr, 0), 10.0);
    assert_eq!(js_array_get_f64(arr, 2), 30.0);
    assert_eq!(js_array_get_f64(arr, 4), 50.0);
}

#[test]
fn test_array_clone_prefers_buffer_registry_before_gc_header_probe() {
    let mut adjacent = None;
    for _ in 0..4 {
        let fake_prev = crate::buffer::buffer_alloc(8);
        let buf = crate::buffer::buffer_alloc(4);
        // Each slab slot is `GC_HEADER_SIZE` (the #5226 sentinel that precedes
        // every buffer pointer) plus the 8-byte-aligned header+capacity.
        let expected_next = fake_prev as usize
            + crate::gc::GC_HEADER_SIZE
            + ((std::mem::size_of::<crate::buffer::BufferHeader>() + 8 + 7) & !7);
        if buf as usize == expected_next {
            adjacent = Some((fake_prev, buf));
            break;
        }
    }
    let (fake_prev, buf) = adjacent.expect("expected adjacent small-buffer slab allocations");

    unsafe {
        *crate::buffer::buffer_data_mut(fake_prev) = crate::gc::GC_TYPE_STRING;
        (*buf).length = 4;
        std::ptr::copy_nonoverlapping(
            [1u8, 2, 3, 4].as_ptr(),
            crate::buffer::buffer_data_mut(buf),
            4,
        );
    }

    let cloned = js_array_clone(buf as *const ArrayHeader);
    assert_numeric_raw_values(cloned, &[1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn test_array_set() {
    let arr = js_array_alloc(3);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    js_array_set_f64(arr, 1, 99.0);
    assert_eq!(js_array_get_f64(arr, 1), 99.0);
}

#[test]
fn test_array_get_unchecked_basic() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 10.0);
    js_array_push_f64(arr, 20.0);
    js_array_push_f64(arr, 30.0);

    assert_eq!(js_array_get_f64_unchecked(arr, 0), 10.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 1), 20.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 2), 30.0);
}

#[test]
fn test_array_get_unchecked_out_of_bounds() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);

    // Out of bounds should return TAG_UNDEFINED (JS spec)
    assert_eq!(
        js_array_get_f64_unchecked(arr, 1).to_bits(),
        0x7FFC_0000_0000_0001u64
    );
    assert_eq!(
        js_array_get_f64_unchecked(arr, 100).to_bits(),
        0x7FFC_0000_0000_0001u64
    );
}

#[test]
fn test_array_get_f64_vs_unchecked_parity() {
    let arr = js_array_alloc(8);
    let values = [1.0, 2.5, -3.0, 0.0, 100.0, f64::INFINITY, f64::NEG_INFINITY];
    for &v in &values {
        js_array_push_f64(arr, v);
    }

    // Both functions should return identical results for plain arrays
    for i in 0..values.len() as u32 {
        let checked = js_array_get_f64(arr, i);
        let unchecked = js_array_get_f64_unchecked(arr, i);
        assert_eq!(
            checked.to_bits(),
            unchecked.to_bits(),
            "parity mismatch at index {}: checked={}, unchecked={}",
            i,
            checked,
            unchecked
        );
    }

    // Out of bounds parity — both return TAG_UNDEFINED
    let oob_checked = js_array_get_f64(arr, 100);
    let oob_unchecked = js_array_get_f64_unchecked(arr, 100);
    assert_eq!(oob_checked.to_bits(), 0x7FFC_0000_0000_0001u64);
    assert_eq!(oob_unchecked.to_bits(), 0x7FFC_0000_0000_0001u64);
}

#[test]
fn test_array_grow_capacity() {
    let mut arr = js_array_alloc(2);

    // Push well beyond initial capacity (push returns new ptr on grow)
    for i in 0..50 {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_length(arr), 50);

    // Verify all values preserved after growth
    for i in 0..50 {
        assert_eq!(
            js_array_get_f64(arr, i),
            i as f64,
            "value at index {} should be {}",
            i,
            i
        );
    }
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 50),
        Some(0),
        "numeric grow path should preserve pointer-free array layout"
    );
}

#[test]
fn test_array_push_f64_no_grow_fast_path() {
    let arr = js_array_alloc(4);
    let value = 42.5;
    let initial_capacity = unsafe { (*arr).capacity };

    let before = gc_collection_count_for_tests();
    let pushed = js_array_push_f64(arr, value);
    let after = gc_collection_count_for_tests();

    assert_eq!(pushed, arr);
    assert_eq!(after, before, "no-grow push must not trigger GC");
    assert_eq!(js_array_length(pushed), 1);
    assert_eq!(js_array_get_f64(pushed, 0), value);
    unsafe {
        assert_eq!((*pushed).capacity, initial_capacity);
    }

    let str_ptr = crate::string::js_string_from_bytes(b"fast-path".as_ptr(), 9);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));

    let before = gc_collection_count_for_tests();
    let pushed_again = js_array_push_f64(pushed, str_value);
    let after = gc_collection_count_for_tests();

    assert_eq!(pushed_again, pushed);
    assert_eq!(after, before, "tagged no-grow push must not trigger GC");
    assert_eq!(js_array_length(pushed_again), 2);
    assert_eq!(
        js_array_get_f64(pushed_again, 1).to_bits(),
        str_value.to_bits()
    );
}

#[test]
fn test_array_push_f64_grow_path_preserves_value_and_forwarding() {
    let mut arr = js_array_alloc(0);
    let initial = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        let pushed = js_array_push_f64(arr, i as f64);
        assert_eq!(pushed, arr);
        arr = pushed;
    }

    let str_ptr = crate::string::js_string_from_bytes(b"grow-path".as_ptr(), 9);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));

    let grown = js_array_push_f64(arr, str_value);

    assert_ne!(grown, arr, "push at capacity should grow the array");
    assert_eq!(js_array_length(grown), capacity + 1);
    assert_eq!(
        js_array_get_f64(grown, capacity).to_bits(),
        str_value.to_bits()
    );
    assert_eq!(
        js_array_length(initial),
        capacity + 1,
        "stale pre-grow pointer should follow the forwarding chain"
    );
    assert_eq!(
        js_array_get_f64(initial, capacity).to_bits(),
        str_value.to_bits()
    );
}

#[test]
fn test_numeric_array_layout_metadata_preserves_and_downgrades_on_writes() {
    let mut arr = js_array_alloc(4);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    arr = js_array_push_f64(arr, 1.25);
    arr = js_array_push_f64(arr, f64::NAN);
    arr = js_array_push_f64(arr, -0.0);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 3),
        Some(0)
    );

    let str_ptr = crate::string::js_string_from_bytes(b"not-number".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    arr = js_array_push_f64(arr, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 4),
        Some(1)
    );

    js_array_set_f64(arr, 0, 99.0);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        0,
        "numeric writes do not silently re-specialize a downgraded mixed array"
    );
}

#[test]
fn test_numeric_array_layout_mark_rejects_holes_and_accepts_dense_numbers() {
    let arr = js_array_alloc_with_length(2);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        js_array_mark_numeric_f64_layout(arr),
        0,
        "hole-filled arrays cannot be treated as dense numeric payloads"
    );

    js_array_set_f64(arr, 0, 3.5);
    js_array_set_f64(arr, 1, -0.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 2),
        Some(0)
    );

    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    js_array_set_f64(arr, 1, undefined);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 2),
        Some(0),
        "undefined downgrades numeric metadata but remains pointer-free for GC"
    );
}

// #6011: the raw-f64-or-holes invariant flag set by `new Array(n)` — the
// packed-f64 range-loop guard's walk-free fast path — must be maintained by
// the canonicalizing store helpers and downgraded by any non-numeric store.
#[test]
fn test_new_array_holes_flag_walk_free_guard_and_sound_downgrade() {
    unsafe {
        // `new Array(4)`: every slot TAG_HOLE, invariant marked at allocation.
        let arr = js_array_constructor_single(4.0);
        assert_eq!(js_array_length(arr), 4);
        assert!(
            rebuild_array_numeric_raw_f64_allow_holes(arr),
            "fresh `new Array(n)` passes the hole-tolerant verify"
        );
        assert_eq!(
            js_array_is_numeric_f64_layout(arr),
            0,
            "holes invariant must NOT satisfy the strict dense probe"
        );

        // Numeric stores keep the invariant and are canonicalized to raw f64
        // bits even though the dense RawF64 flag is not set: a verbatim
        // INT32-boxed slot would read as NaN payload through the guarded
        // loop's raw-f64 loads.
        let int32_value = f64::from_bits(crate::value::INT32_TAG | 7u64);
        js_array_set_f64(arr, 0, int32_value);
        let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
        assert_eq!(
            *elements, // slot 0
            7.0f64.to_bits(),
            "numeric store canonicalizes to raw f64 bits under the holes flag"
        );
        assert!(rebuild_array_numeric_raw_f64_allow_holes(arr));

        // A non-numeric store must drop the invariant so the guard re-walks
        // (and fails) instead of treating string bits as raw f64.
        let s = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
        let s_value =
            f64::from_bits(crate::value::STRING_TAG | (s as u64 & crate::value::POINTER_MASK));
        js_array_set_f64(arr, 1, s_value);
        assert!(
            !rebuild_array_numeric_raw_f64_allow_holes(arr),
            "non-numeric store downgrades the holes invariant"
        );
    }
}

// #6011: an unmarked hole-carrying array (internal alloc path) is verified by
// a walk; the walk records the invariant so re-entry is walk-free, and the
// strict dense probe must not clear that recorded invariant when it declines
// on a hole.
#[test]
fn test_holes_invariant_recorded_by_verify_walk_survives_strict_probe() {
    unsafe {
        let arr = js_array_alloc_with_length(3);
        js_array_set_f64(arr, 0, 1.5);
        assert!(
            rebuild_array_numeric_raw_f64_allow_holes(arr),
            "hole-tolerant verify accepts numeric + hole slots"
        );
        assert_eq!(
            js_array_is_numeric_f64_layout(arr),
            0,
            "strict probe still declines on the remaining holes"
        );
        assert!(
            rebuild_array_numeric_raw_f64_allow_holes(arr),
            "strict-probe decline must not clear the recorded holes invariant"
        );

        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        js_array_set_f64(arr, 2, undefined);
        assert!(
            !rebuild_array_numeric_raw_f64_allow_holes(arr),
            "undefined is not a hole: it must fail the hole-tolerant verify"
        );
    }
}

#[test]
fn test_numeric_array_mark_canonicalizes_int32_and_nan_inline() {
    let arr = js_array_alloc_with_length(3);
    let int32_value = f64::from_bits(crate::value::INT32_TAG | ((-17i32 as u32) as u64));
    let payload_nan = f64::from_bits(0x7FF8_0000_0000_1234);

    js_array_set_f64(arr, 0, int32_value);
    js_array_set_f64(arr, 1, payload_nan);
    js_array_set_f64(arr, 2, -0.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), -17.0);
    assert!(js_array_numeric_get_f64_unboxed(arr, 1).is_nan());
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, 2).to_bits(),
        (-0.0f64).to_bits()
    );
    unsafe {
        assert_eq!(raw_slot_bits(arr, 0), (-17.0f64).to_bits());
        assert_eq!(raw_slot_bits(arr, 1), f64::NAN.to_bits());
        assert_eq!(raw_slot_bits(arr, 2), (-0.0f64).to_bits());
    }
}

#[test]
fn test_numeric_array_raw_f64_payload_tracks_sets_and_downgrades() {
    let mut arr = js_array_alloc(2);
    arr = js_array_push_f64(arr, 1.5);
    arr = js_array_push_f64(arr, 2.5);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), 1.5);
    assert_eq!(js_array_numeric_set_f64_unboxed(arr, 1, 7.25), 1);
    assert_eq!(js_array_get_f64(arr, 1), 7.25);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 1), 7.25);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let str_ptr = crate::string::js_string_from_bytes(b"boxed".as_ptr(), 5);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    js_array_set_f64(arr, 1, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, 1).to_bits(),
        str_value.to_bits(),
        "unboxed helper falls back to boxed slots after downgrade"
    );
}

#[test]
fn pointer_only_array_allocation_clears_numeric_representation() {
    let arr = js_array_alloc_pointer_elements(2);
    assert_eq!(unsafe { super::header::array_numeric_layout(arr) }, None);
}

#[test]
fn test_numeric_array_sparse_extend_fills_holes_and_downgrades_raw_layout() {
    let mut arr = js_array_alloc(8);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let extended = js_array_set_f64_extend(arr, 5, 6.0);

    assert_eq!(extended, arr);
    assert_eq!(js_array_length(extended), 6);
    assert_eq!(js_array_is_numeric_f64_layout(extended), 0);
    assert_eq!(js_array_get_f64(extended, 0), 1.0);
    assert_eq!(js_array_get_f64(extended, 1), 2.0);
    assert_eq!(
        js_array_get_f64(extended, 3).to_bits(),
        crate::value::TAG_UNDEFINED
    );
    unsafe {
        assert_eq!(raw_slot_bits(extended, 3), crate::value::TAG_HOLE);
    }
    assert_eq!(js_array_get_f64(extended, 5), 6.0);
}

#[test]
fn test_numeric_array_raw_f64_payload_push_helper_preserves_and_downgrades() {
    let mut arr = js_array_alloc(2);

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    arr = js_array_numeric_push_f64_unboxed(arr, 1.0);
    arr = js_array_numeric_push_f64_unboxed(arr, 2.0);

    assert_eq!(js_array_length(arr), 2);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 0), 1.0);
    assert_eq!(js_array_numeric_get_f64_unboxed(arr, 1), 2.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let grown = js_array_numeric_push_f64_unboxed(arr, 3.0);
    assert_eq!(js_array_length(grown), 3);
    assert_eq!(js_array_numeric_get_f64_unboxed(grown, 2), 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(grown), 1);

    let str_ptr = crate::string::js_string_from_bytes(b"push-boxed".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    let mixed = js_array_numeric_push_f64_unboxed(grown, str_value);

    assert_eq!(js_array_get_f64(mixed, 3).to_bits(), str_value.to_bits());
    assert_eq!(js_array_is_numeric_f64_layout(mixed), 0);
}

#[test]
fn test_array_push_jsvalue_int32_canonicalizes_raw_f64_slot() {
    let int_bits = int32_jsvalue_bits(42);
    let arr = js_array_push_jsvalue(js_array_alloc(1), int_bits);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), int_bits);
    assert_canonical_raw_slot(arr, 0, 42.0);
}

#[test]
fn test_array_set_jsvalue_extend_int32_canonicalizes_dense_append() {
    let mut arr = js_array_alloc(2);
    arr = js_array_push_f64(arr, 1.0);

    let int_bits = int32_jsvalue_bits(-7);
    arr = js_array_set_jsvalue_extend(arr, 1, int_bits);

    assert_numeric_raw_values(arr, &[1.0, -7.0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 1).to_bits(), int_bits);
}

#[test]
fn test_numeric_raw_f64_helpers_canonicalize_int32_shaped_values() {
    let mut arr = js_array_alloc(2);
    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);

    let push_bits = int32_jsvalue_bits(9);
    arr = js_array_numeric_push_f64_unboxed(arr, f64::from_bits(push_bits));
    assert_eq!(js_array_length(arr), 1);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), push_bits);
    assert_canonical_raw_slot(arr, 0, 9.0);

    let set_bits = int32_jsvalue_bits(-11);
    assert_eq!(
        js_array_numeric_set_f64_unboxed(arr, 0, f64::from_bits(set_bits)),
        1
    );
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), set_bits);
    assert_canonical_raw_slot(arr, 0, -11.0);
}

#[test]
fn test_array_from_jsvalue_int32_rebuild_canonicalizes_raw_slots() {
    let elements = [int32_jsvalue_bits(3), int32_jsvalue_bits(-4)];
    let arr = js_array_from_jsvalue(elements.as_ptr(), elements.len() as u32);

    assert_numeric_raw_values(arr, &[3.0, -4.0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 0).to_bits(), elements[0]);
    assert_ne!(js_array_get_f64_unchecked(arr, 1).to_bits(), elements[1]);
}

/// #5552: `js_array_from_jsvalue` (the JSValue sibling of `js_array_from_values`,
/// used for mixed-type array construction) must demote a uniquely-owned
/// (refcount==1) heap string before it aliases an element slot — otherwise a
/// later in-place `js_string_append` on the source rewrites the stored element.
/// Codegen never emits this symbol today, so this can only be exercised at the
/// runtime level (no compiled-TS regression test can reach it).
#[test]
fn test_array_from_jsvalue_demotes_unique_string_against_inplace_append() {
    // A uniquely-owned heap string with spare capacity, so `js_string_append`
    // takes its in-place (refcount==1) fast path.
    let init = b"prefix_init";
    let s = crate::string::js_string_from_bytes_with_capacity(init.as_ptr(), init.len() as u32, 64);
    unsafe {
        (*s).refcount = 1;
    }
    let s_bits = crate::value::js_nanbox_string(s as i64).to_bits();

    // Construct a mixed-type array via the JSValue path.
    let elements = [s_bits, int32_jsvalue_bits(1)];
    let arr = js_array_from_jsvalue(elements.as_ptr(), elements.len() as u32);

    // Grow the source in place. With the demote, `s` is now shared (refcount==0)
    // so this allocates fresh and leaves the stored element untouched; without
    // it, the in-place append corrupts arr[0].
    let more = crate::string::js_string_from_bytes(b"_more".as_ptr(), 5);
    let grown = crate::string::js_string_append(s, more);

    let stored_bits = js_array_get_jsvalue(arr, 0);
    let stored_ptr = (stored_bits & crate::value::POINTER_MASK) as *const crate::StringHeader;
    let stored = crate::string::string_as_str(stored_ptr);
    assert_eq!(
        stored, "prefix_init",
        "string stored via js_array_from_jsvalue must not be corrupted by a later in-place append"
    );
    // Sanity: the source itself did grow (the append happened).
    let grown_str = crate::string::string_as_str(grown as *const crate::StringHeader);
    assert_eq!(grown_str, "prefix_init_more");
}

#[test]
fn test_nonnumeric_append_downgrades_raw_f64_and_preserves_payload() {
    let bool_bits = crate::value::JSValue::bool(true).bits();
    let arr = js_array_push_jsvalue(js_array_alloc(1), bool_bits);

    assert_eq!(js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(js_array_get_jsvalue(arr, 0), bool_bits);
    assert_eq!(js_array_get_f64_unchecked(arr, 0).to_bits(), bool_bits);
}

#[test]
fn test_numeric_array_layout_transfers_across_growth_forwarding() {
    let mut arr = js_array_alloc(0);
    let original = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let grown = js_array_push_f64(arr, capacity as f64);

    assert_ne!(grown, arr);
    assert_eq!(js_array_is_numeric_f64_layout(grown), 1);
    assert_eq!(
        js_array_is_numeric_f64_layout(original),
        1,
        "stale pointers should follow growth forwarding before checking metadata"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(grown as usize, (capacity + 1) as usize),
        Some(0)
    );
}

#[test]
fn test_numeric_array_raw_f64_payload_rebuilds_after_growth_forwarding() {
    let mut arr = js_array_alloc(0);
    let original = arr;
    let capacity = unsafe { (*arr).capacity };

    for i in 0..capacity {
        arr = js_array_push_f64(arr, i as f64);
    }

    assert_eq!(js_array_mark_numeric_f64_layout(arr), 1);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(arr, capacity - 1),
        (capacity - 1) as f64
    );

    let grown = js_array_push_f64(arr, capacity as f64);

    assert_ne!(grown, arr);
    assert_eq!(
        js_array_numeric_get_f64_unboxed(grown, capacity),
        capacity as f64
    );
    assert_eq!(
        js_array_numeric_get_f64_unboxed(original, capacity),
        capacity as f64,
        "stale forwarded handles rebuild the moved raw payload before reading"
    );
}

#[test]
fn test_numeric_array_layout_query_recovers_dense_numeric_metadata() {
    let mut arr = js_array_alloc(0);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);

    js_array_clear_numeric_layout(arr);

    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        1,
        "numeric layout metadata can be rebuilt from dense numeric slots"
    );
}

#[test]
fn test_array_get_f64_large_dense_array_preserves_values() {
    let arr = js_array_alloc_with_length(100_001);
    js_array_set_f64(arr, 100_000, 42.0);

    assert_eq!(js_array_get_f64(arr, 100_000), 42.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 100_000), 42.0);
}

#[test]
fn test_numeric_array_layout_bulk_rebuild_preserves_and_downgrades() {
    let values = [1.0, 2.0, 3.0, 4.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);

    assert_eq!(js_array_is_numeric_f64_layout(src), 1);

    let sliced = js_array_slice(src, 1, 3);
    assert_numeric_raw_values(sliced, &[2.0, 3.0]);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(sliced as usize, 2),
        Some(0)
    );

    let concatenated = js_array_concat(js_array_alloc(0), src);
    assert_numeric_raw_values(concatenated, &values);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(concatenated as usize, values.len()),
        Some(0)
    );

    let str_ptr = crate::string::js_string_from_bytes(b"bulk-mixed".as_ptr(), 10);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    js_array_fill(concatenated, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(concatenated), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(concatenated as usize, values.len()),
        Some(values.len())
    );
}

#[test]
fn test_array_slice_value_index_coercion() {
    let values = [1.0, 2.0, 3.0, 4.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

    assert_numeric_raw_values(js_array_slice_values(src, f64::NAN, undefined), &values);
    assert_numeric_raw_values(js_array_slice_values(src, f64::INFINITY, undefined), &[]);
    assert_numeric_raw_values(
        js_array_slice_values(src, f64::NEG_INFINITY, undefined),
        &values,
    );
    assert_numeric_raw_values(
        js_array_slice_values(src, 1.0, f64::INFINITY),
        &[2.0, 3.0, 4.0],
    );
    assert_numeric_raw_values(js_array_slice_values(src, 1.0, f64::NAN), &[]);
    assert_numeric_raw_values(js_array_slice_values(src, 1.9, 3.8), &[2.0, 3.0]);
    assert_numeric_raw_values(js_array_slice_values(src, 1.0, undefined), &[2.0, 3.0, 4.0]);

    let str_ptr = crate::string::js_string_from_bytes(b"2".as_ptr(), 1);
    let string_two = crate::value::js_nanbox_string(str_ptr as i64);
    assert_numeric_raw_values(
        js_array_slice_values(src, string_two, undefined),
        &[3.0, 4.0],
    );
}

#[test]
fn test_numeric_array_layout_length_and_delete_transitions() {
    let mut arr = js_array_alloc(4);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);
    arr = js_array_push_f64(arr, 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    js_array_set_length(arr, 2.0);

    assert_eq!(js_array_length(arr), 2);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        1,
        "truncation should preserve dense numeric layout for the reachable prefix"
    );

    js_array_set_length(arr, 4.0);

    assert_eq!(js_array_length(arr), 4);
    assert_eq!(
        js_array_is_numeric_f64_layout(arr),
        0,
        "extension creates holes and must downgrade numeric layout"
    );
    unsafe {
        assert_eq!(raw_slot_bits(arr, 2), crate::value::TAG_HOLE);
        assert_eq!(raw_slot_bits(arr, 3), crate::value::TAG_HOLE);
    }
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(arr as usize, 4),
        Some(0)
    );

    let mut dense = js_array_alloc(4);
    dense = js_array_push_f64(dense, 10.0);
    dense = js_array_push_f64(dense, 20.0);
    assert_eq!(js_array_is_numeric_f64_layout(dense), 1);

    assert_eq!(js_array_delete(dense, 0), 1);
    assert_eq!(
        js_array_is_numeric_f64_layout(dense),
        0,
        "delete creates a hole and downgrades dense numeric layout"
    );
    unsafe {
        assert_eq!(raw_slot_bits(dense, 0), crate::value::TAG_HOLE);
    }
}

#[test]
fn test_numeric_array_layout_immutable_helpers_preserve_or_downgrade() {
    let values = [10.0, 2.0, 30.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    assert_numeric_raw_values(src, &values);

    let reversed = js_array_to_reversed(src);
    assert_numeric_raw_values(reversed, &[30.0, 2.0, 10.0]);

    let sorted = js_array_to_sorted_default(src);
    assert_numeric_raw_values(sorted, &[10.0, 2.0, 30.0]);

    let numeric_replaced = js_array_with(src, 1.0, 99.0);
    assert_numeric_raw_values(numeric_replaced, &[10.0, 99.0, 30.0]);

    let insert = [7.0, 8.0];
    let spliced = js_array_to_spliced(src, 1.0, 1.0, insert.as_ptr(), insert.len() as u32);
    assert_numeric_raw_values(spliced, &[10.0, 7.0, 8.0, 30.0]);

    let str_ptr = crate::string::js_string_from_bytes(b"immutable-mixed".as_ptr(), 15);
    let str_value =
        f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK));
    let mixed = js_array_with(src, 1.0, str_value);

    assert_eq!(js_array_is_numeric_f64_layout(mixed), 0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(mixed as usize, values.len()),
        Some(1)
    );
}

#[test]
fn test_numeric_array_layout_map_fast_path_downgrades_mapped_pointers() {
    let mut arr = js_array_alloc(4);
    arr = js_array_push_f64(arr, 1.0);
    arr = js_array_push_f64(arr, 2.0);
    arr = js_array_push_f64(arr, 3.0);
    assert_eq!(js_array_is_numeric_f64_layout(arr), 1);

    let callback = crate::closure::js_closure_alloc(test_map_to_string as *const u8, 0);
    let mapped = js_array_map(arr, callback);

    assert_eq!(js_array_length(mapped), 3);
    assert_eq!(
        js_array_is_numeric_f64_layout(mapped),
        0,
        "small map() results use a layout-only fast path and must still downgrade"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(mapped as usize, 3),
        Some(3)
    );
}

#[test]
fn test_numeric_array_layout_entries_outer_downgrades_inner_pairs_preserve() {
    let values = [4.0, 5.0];
    let src = js_array_from_f64(values.as_ptr(), values.len() as u32);
    let entries = js_array_entries(src);

    assert_eq!(
        js_array_is_numeric_f64_layout(entries),
        0,
        "entries() outer array stores pair pointers, not raw numeric slots"
    );
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(entries as usize, values.len()),
        Some(values.len())
    );

    let pair_box = js_array_get_f64(entries, 0);
    let pair = (pair_box.to_bits() & crate::value::POINTER_MASK) as *mut ArrayHeader;
    assert_eq!(js_array_is_numeric_f64_layout(pair), 1);
    assert_eq!(js_array_numeric_get_f64_unboxed(pair, 0), 0.0);
    assert_eq!(js_array_numeric_get_f64_unboxed(pair, 1), 4.0);
    assert_eq!(
        crate::gc::test_layout_pointer_slot_count(pair as usize, 2),
        Some(0)
    );
}

#[test]
fn test_array_set_unchecked_basic() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);
    js_array_push_f64(arr, 3.0);

    js_array_set_f64_unchecked(arr, 1, 99.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 1), 99.0);
    // Other elements unchanged
    assert_eq!(js_array_get_f64_unchecked(arr, 0), 1.0);
    assert_eq!(js_array_get_f64_unchecked(arr, 2), 3.0);
}

#[test]
fn test_array_pop_and_push() {
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);

    let popped = js_array_pop_f64(arr);
    assert_eq!(popped, 3.0);
    assert_eq!(js_array_length(arr), 2);

    let arr = js_array_push_f64(arr, 4.0);
    assert_eq!(js_array_length(arr), 3);
    assert_eq!(js_array_get_f64(arr, 2), 4.0);
}

#[test]
fn test_array_indexOf() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 10.0);
    js_array_push_f64(arr, 20.0);
    js_array_push_f64(arr, 30.0);

    assert_eq!(js_array_indexOf_f64(arr, 10.0), 0);
    assert_eq!(js_array_indexOf_f64(arr, 20.0), 1);
    assert_eq!(js_array_indexOf_f64(arr, 30.0), 2);
    assert_eq!(js_array_indexOf_f64(arr, 99.0), -1);
}

#[test]
fn test_array_includes() {
    let arr = js_array_alloc(4);
    js_array_push_f64(arr, 1.0);
    js_array_push_f64(arr, 2.0);

    assert_eq!(js_array_includes_f64(arr, 1.0), 1);
    assert_eq!(js_array_includes_f64(arr, 2.0), 1);
    assert_eq!(js_array_includes_f64(arr, 3.0), 0);
}

#[test]
fn test_array_last_index_of() {
    let arr = js_array_alloc(8);
    for v in [1.0, 2.0, 3.0, 2.0, 1.0] {
        js_array_push_f64(arr, v);
    }
    // No fromIndex (has_from == 0) → search from the last element.
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 0.0, 0), 3);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 1.0, 0.0, 0), 4);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 9.0, 0.0, 0), -1);
    // Explicit fromIndex (has_from == 1), including the spec's clamping.
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 2.0, 1), 1);
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, -2.0, 1), 3); // 5 + (-2) = 3
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, -10.0, 1), -1); // < -length
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 100.0, 1), 3); // clamp to len-1
    assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 0.0, 1), -1); // only index 0
                                                                      // Empty array.
    let empty = js_array_alloc(1);
    assert_eq!(js_array_last_index_of_jsvalue(empty, 1.0, 0.0, 0), -1);
}

#[test]
fn test_array_from_f64_and_length() {
    let values = [5.0, 10.0, 15.0];
    let arr = js_array_from_f64(values.as_ptr(), 3);

    assert_eq!(js_array_length(arr), 3);
    for i in 0..3 {
        assert_eq!(js_array_get_f64(arr, i), values[i as usize]);
    }
}

#[test]
fn test_array_null_safety() {
    // Null array pointer should not crash
    assert!(js_array_get_f64(std::ptr::null(), 0).is_nan());
    assert!(js_array_get_f64_unchecked(std::ptr::null(), 0).is_nan());
    assert_eq!(js_array_length(std::ptr::null()), 0);
}

#[test]
fn test_array_length_rejects_nanboxed_non_pointers_before_registry_probe() {
    for bits in [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_FALSE,
        crate::value::TAG_TRUE,
        crate::value::TAG_HOLE,
        crate::value::INT32_TAG | 1,
    ] {
        assert_eq!(js_array_length(bits as *const ArrayHeader), 0);
    }
}

#[test]
fn test_array_splice_delete_middle() {
    // [1,2,3,4,5].splice(1, 2) -> deleted=[2,3], arr=[1,4,5]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let arr = js_array_push_f64(arr, 5.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 2, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 4.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 5.0);

    assert_eq!(js_array_length(deleted), 2);
    assert_eq!(js_array_get_f64(deleted, 0), 2.0);
    assert_eq!(js_array_get_f64(deleted, 1), 3.0);
}

#[test]
fn test_array_splice_insert() {
    // [1,2,5].splice(2, 0, 3, 4) -> deleted=[], arr=[1,2,3,4,5]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 5.0);
    let items = [3.0_f64, 4.0];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 2, 0, items.as_ptr(), 2, &mut out_arr);

    assert_eq!(js_array_length(deleted), 0);
    assert_eq!(js_array_length(out_arr), 5);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
    assert_eq!(js_array_get_f64(out_arr, 3), 4.0);
    assert_eq!(js_array_get_f64(out_arr, 4), 5.0);
}

#[test]
fn test_array_splice_replace() {
    // [1,2,3].splice(1, 1, 99) -> deleted=[2], arr=[1,99,3]
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let items = [99.0_f64];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 1, items.as_ptr(), 1, &mut out_arr);

    assert_eq!(js_array_length(deleted), 1);
    assert_eq!(js_array_get_f64(deleted, 0), 2.0);
    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 99.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
}

#[test]
fn test_array_splice_delete_to_end() {
    // [1,2,3,4].splice(2) -> deleted=[3,4], arr=[1,2]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 2, i32::MAX, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(out_arr), 2);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_length(deleted), 2);
    assert_eq!(js_array_get_f64(deleted, 0), 3.0);
    assert_eq!(js_array_get_f64(deleted, 1), 4.0);
}

#[test]
fn test_array_splice_delete_count_coerces_js_values() {
    assert_eq!(
        js_array_splice_delete_count(f64::from_bits(crate::value::TAG_UNDEFINED)),
        0
    );
    assert_eq!(
        js_array_splice_delete_count(f64::from_bits(crate::value::TAG_NULL)),
        0
    );
    assert_eq!(
        js_array_splice_delete_count(crate::value::js_nanbox_string(
            crate::string::js_string_from_bytes(b"2.8".as_ptr(), 3) as i64,
        )),
        2
    );
    assert_eq!(js_array_splice_delete_count(f64::INFINITY), i32::MAX);
    assert_eq!(js_array_splice_delete_count(f64::NAN), 0);
}

#[test]
fn test_array_splice_negative_start() {
    // [1,2,3,4].splice(-2, 1) -> deleted=[3], arr=[1,2,4]
    let arr = js_array_alloc(8);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let arr = js_array_push_f64(arr, 3.0);
    let arr = js_array_push_f64(arr, 4.0);
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, -2, 1, std::ptr::null(), 0, &mut out_arr);

    assert_eq!(js_array_length(deleted), 1);
    assert_eq!(js_array_get_f64(deleted, 0), 3.0);
    assert_eq!(js_array_length(out_arr), 3);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
    assert_eq!(js_array_get_f64(out_arr, 2), 4.0);
}

#[test]
fn test_array_splice_grow_realloc() {
    // Start with capacity 4, splice in 10 items to force reallocation
    let arr = js_array_alloc(4);
    let arr = js_array_push_f64(arr, 1.0);
    let arr = js_array_push_f64(arr, 2.0);
    let items = [
        10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0_f64,
    ];
    let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
    let deleted = js_array_splice(arr, 1, 0, items.as_ptr(), 10, &mut out_arr);

    assert_eq!(js_array_length(deleted), 0);
    assert_eq!(js_array_length(out_arr), 12);
    assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
    for i in 0..10 {
        assert_eq!(
            js_array_get_f64(out_arr, (i + 1) as u32),
            items[i],
            "mismatch at index {}",
            i + 1
        );
    }
    assert_eq!(js_array_get_f64(out_arr, 11), 2.0);
}

#[test]
fn join_routes_objects_and_nested_arrays_through_tostring() {
    // #800/#2135: a POINTER_TAG element that is an object/array (not a string)
    // must go through the spec ToString — a nested array joins recursively, a
    // plain object becomes "[object Object]" — instead of being mis-read as a
    // StringHeader (which produced corrupted/empty output).
    unsafe {
        let inner = js_array_push_f64(js_array_push_f64(js_array_alloc(2), 1.0), 2.0);
        let inner_v = f64::from_bits(crate::value::JSValue::pointer(inner as *const u8).bits());
        let obj = crate::object::js_object_alloc(0, 0);
        let obj_v = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
        let mut arr = js_array_alloc(2);
        arr = js_array_push_f64(arr, inner_v);
        arr = js_array_push_f64(arr, obj_v);
        let sep = crate::string::js_string_from_bytes(b";".as_ptr(), 1);
        let out = js_array_join(arr, sep);
        let len = (*out).byte_len as usize;
        let data = (out as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let s = std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap();
        assert_eq!(s, "1,2;[object Object]");
    }
}

#[test]
fn join_accepts_heap_string_tagged_elements() {
    unsafe {
        let left = crate::string::js_string_from_bytes(b"alpha".as_ptr(), 5);
        let right = crate::string::js_string_from_bytes(b"beta".as_ptr(), 4);
        let left_v =
            f64::from_bits(crate::value::STRING_TAG | (left as u64 & crate::value::POINTER_MASK));
        let right_v =
            f64::from_bits(crate::value::STRING_TAG | (right as u64 & crate::value::POINTER_MASK));

        let mut arr = js_array_alloc(2);
        arr = js_array_push_f64(arr, left_v);
        arr = js_array_push_f64(arr, right_v);

        let sep = crate::string::js_string_from_bytes(b"|".as_ptr(), 1);
        let out = js_array_join(arr, sep);
        let len = (*out).byte_len as usize;
        let data = (out as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let s = std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap();
        assert_eq!(s, "alpha|beta");
    }
}

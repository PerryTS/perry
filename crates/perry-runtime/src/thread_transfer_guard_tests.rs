//! #6185 (2026-07-09 GC audit §6): a non-transferable value crossing a
//! `perry/thread` boundary must surface a named `TypeError`, not silently
//! become `undefined`. These tests exercise the serialization-boundary
//! detection directly. The main-thread `js_throw` and promise-reject wiring
//! rides on top of this detection and needs the full JS runtime (setjmp
//! frame) to observe, so it is covered by the parity suite rather than here.
use super::*;

#[test]
fn unsupported_type_names_are_human_readable() {
    assert_eq!(unsupported_transfer_type_name(gc::GC_TYPE_MAP), "Map");
    assert_eq!(unsupported_transfer_type_name(gc::GC_TYPE_SET), "Set");
    assert_eq!(
        unsupported_transfer_type_name(gc::GC_TYPE_PROMISE),
        "Promise"
    );
    assert_eq!(unsupported_transfer_type_name(gc::GC_TYPE_ERROR), "Error");
    assert_eq!(
        unsupported_transfer_type_name(gc::GC_TYPE_TYPED_ARRAY),
        "TypedArray"
    );
    assert_eq!(unsupported_transfer_type_name(gc::GC_TYPE_BUFFER), "Buffer");
    assert_eq!(
        unsupported_transfer_type_name(gc::GC_TYPE_TEMPORAL),
        "Temporal value"
    );
    // A Symbol is POINTER_TAG'd but allocated with GC_TYPE_STRING.
    assert_eq!(unsupported_transfer_type_name(gc::GC_TYPE_STRING), "Symbol");
    // Any unrecognized type still yields a message, never a panic.
    assert_eq!(
        unsupported_transfer_type_name(250),
        "value of an unsupported type"
    );
}

#[test]
fn first_unsupported_transfer_type_finds_nested_markers() {
    // Top-level.
    assert_eq!(
        first_unsupported_transfer_type(&SerializedValue::Unsupported("Map")),
        Some("Map")
    );
    // Inside an array element.
    let arr = SerializedValue::Array(vec![
        SerializedValue::Inline(TAG_NULL),
        SerializedValue::Unsupported("Set"),
    ]);
    assert_eq!(first_unsupported_transfer_type(&arr), Some("Set"));
    // Inside an object field, nested in an array.
    let obj = SerializedValue::Object {
        class_id: 0,
        parent_class_id: 0,
        fields: vec![
            SerializedValue::Inline(TAG_TRUE),
            SerializedValue::Array(vec![SerializedValue::Unsupported("Promise")]),
        ],
        keys: None,
    };
    assert_eq!(first_unsupported_transfer_type(&obj), Some("Promise"));
    // Inside a closure capture.
    let clo = SerializedValue::Closure {
        func_ptr: 0,
        capture_count: 1,
        captures: vec![SerializedValue::Unsupported("Error")],
    };
    assert_eq!(first_unsupported_transfer_type(&clo), Some("Error"));
}

#[test]
fn transferable_trees_report_no_unsupported() {
    let tree = SerializedValue::Array(vec![
        SerializedValue::Inline(0x4045_0000_0000_0000), // a plain f64
        SerializedValue::String(b"ok".to_vec()),
        SerializedValue::Object {
            class_id: 3,
            parent_class_id: 0,
            fields: vec![
                SerializedValue::Inline(TAG_FALSE),
                SerializedValue::Date(1.0),
            ],
            keys: None,
        },
        SerializedValue::BigInt([0u64; BIGINT_LIMBS]),
    ]);
    assert_eq!(first_unsupported_transfer_type(&tree), None);
}

#[test]
fn serialize_map_yields_unsupported_marker() {
    // The concrete audit case: a real Map value serializes to a named
    // Unsupported marker instead of Inline(undefined).
    unsafe {
        let map = crate::map::js_map_alloc(4);
        let map_bits = POINTER_TAG | (map as u64 & POINTER_MASK);
        let sv = serialize_nanbox_for_thread(map_bits);
        assert!(
            matches!(sv, SerializedValue::Unsupported("Map")),
            "a Map must serialize to Unsupported(\"Map\"), got {sv:?}"
        );
    }
}

#[test]
fn serialize_supported_values_still_transfer() {
    unsafe {
        // Inline scalars round-trip their exact bits.
        for bits in [TAG_UNDEFINED, TAG_NULL, TAG_TRUE, TAG_FALSE] {
            assert!(matches!(
                serialize_nanbox_for_thread(bits),
                SerializedValue::Inline(b) if b == bits
            ));
        }
        let int_bits = INT32_TAG | 42u64;
        assert!(matches!(
            serialize_nanbox_for_thread(int_bits),
            SerializedValue::Inline(b) if b == int_bits
        ));
        let num_bits = 3.5f64.to_bits();
        assert!(matches!(
            serialize_nanbox_for_thread(num_bits),
            SerializedValue::Inline(b) if b == num_bits
        ));

        // A real string transfers as its UTF-8 bytes.
        let s = crate::string::js_string_from_bytes(b"hello".as_ptr(), 5);
        let s_bits = JSValue::string_ptr(s).bits();
        match serialize_nanbox_for_thread(s_bits) {
            SerializedValue::String(bytes) => assert_eq!(bytes, b"hello"),
            other => panic!("string must serialize to String, got {other:?}"),
        }

        // A real array of numbers transfers and round-trips.
        let arr = crate::array::js_array_alloc(3);
        for (i, v) in [10.0f64, 20.0, 30.0].iter().enumerate() {
            store_thread_array_slot(arr, i, v.to_bits());
        }
        let arr_bits = JSValue::pointer(arr as *const u8).bits();
        let sv = serialize_nanbox_for_thread(arr_bits);
        assert_eq!(first_unsupported_transfer_type(&sv), None);
        match &sv {
            SerializedValue::Array(elems) => {
                assert_eq!(elems.len(), 3);
                assert!(matches!(elems[0], SerializedValue::Inline(b) if b == 10.0f64.to_bits()));
            }
            other => panic!("array must serialize to Array, got {other:?}"),
        }
        // Round-trip back into this thread's arena.
        let back = deserialize_nanbox_on_current_thread(&sv);
        let back_arr = (back & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!((*back_arr).length, 3);
    }
}

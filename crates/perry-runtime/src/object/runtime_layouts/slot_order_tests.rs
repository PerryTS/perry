use super::*;
use crate::object::*;

runtime_atom!(FIRST, "slotorder_first", "736c6f746f726465725f6669727374");
runtime_atom!(
    SECOND,
    "slotorder_second",
    "736c6f746f726465725f7365636f6e64"
);
runtime_atom!(THIRD, "slotorder_third", "736c6f746f726465725f7468697264");
runtime_layout!(
    PERMUTED,
    0x7f000001,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [FIRST @ 2, SECOND @ 0, THIRD @ 1]
);

runtime_layout!(
    POSITIONAL,
    0x7f000002,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [FIRST, SECOND, THIRD]
);

unsafe fn string(value: crate::JSValue) -> String {
    let mut sso = [0; crate::value::SHORT_STRING_MAX_LEN];
    String::from_utf8(
        crate::string::js_string_key_bytes(value, &mut sso)
            .unwrap_or_else(|| panic!("expected string, got {:016x}", value.bits()))
            .to_vec(),
    )
    .unwrap()
}
unsafe fn names(array: *mut crate::ArrayHeader) -> Vec<String> {
    (0..crate::array::js_array_length(array))
        .map(|i| string(crate::array::js_array_get(array, i)))
        .collect()
}
unsafe fn assert_order(object: *mut ObjectHeader, expected: &[&str]) {
    let value = crate::js_nanbox_pointer(object as i64);
    assert_eq!(names(js_object_keys(object)), expected);
    let own = js_object_get_own_property_names(value);
    assert_eq!(
        names((own.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader),
        expected
    );
    assert_eq!(names(js_for_in_keys_value(value)), expected);
    let reflected = crate::proxy::js_reflect_own_keys(value);
    assert_eq!(
        names((reflected.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader),
        expected
    );
    let keys = crate::object::object_keys(object);
    assert_eq!(
        (0..keys.count())
            .filter_map(|i| {
                let key = keys.get(i);
                (!key.is_undefined()).then(|| string(key))
            })
            .collect::<Vec<_>>(),
        expected
    );
}

/// The emitted read's answer: the probe (when the key has its atom) and the
/// cacheless complete read.
unsafe fn assert_emitted_read(obj: *mut ObjectHeader, key: *const crate::StringHeader, value: f64) {
    if let Some(atom) = crate::string::existing_property_atom(key) {
        let probed = crate::object::layout_probe::perry_probe_get(
            crate::object::shapes::ordinary_dir_addr(),
            (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
            crate::value::STRING_TAG | atom as u64,
            crate::string::property_atom_hash(atom),
        );
        assert!(probed.to_bits() == crate::value::TAG_HOLE || probed == value);
    }
    assert_eq!(
        crate::object::js_object_get_field_slow(obj as i64, key),
        value
    );
}

#[test]
fn static_layout_slot_order_registration_reflection_values_and_transitions() {
    crate::gc::initialize_runtime_fixture();
    let desc = std::ptr::addr_of!(PERMUTED);
    assert_ne!(
        unsafe { layout_id(desc) },
        0,
        "non-positional descriptor must register"
    );
    assert_ne!(
        unsafe { layout_id(desc) },
        unsafe { layout_id(std::ptr::addr_of!(POSITIONAL)) },
        "slot mapping is part of shape identity"
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let object =
        scope.root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_layout(desc) });
    unsafe {
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 2, crate::JSValue::number(11.0));
            js_object_set_field(obj, 0, crate::JSValue::number(22.0));
            js_object_set_field(obj, 1, crate::JSValue::number(33.0));
            assert_order(
                obj,
                &["slotorder_first", "slotorder_second", "slotorder_third"],
            );
            assert_eq!(js_object_get_field_by_name_f64(obj, FIRST.string), 11.0);
            assert_eq!(js_object_get_field_by_name_f64(obj, SECOND.string), 22.0);
            assert_eq!(js_object_get_field_by_name_f64(obj, THIRD.string), 33.0);
            js_object_set_field_by_name(obj, SECOND.string, 44.0);
            assert_eq!(js_object_get_field(obj, 0).as_number(), 44.0);
            let value = crate::js_nanbox_pointer(obj as i64);
            crate::proxy::js_put_value_set(
                value,
                crate::js_nanbox_string(SECOND.string as i64),
                44.0,
                value,
                1,
            );
            assert_eq!(js_object_get_field(obj, 0).as_number(), 44.0);
            assert_eq!(js_object_get_field(obj, 2).as_number(), 11.0);
            let first = crate::runtime_state_key!(b"slotorder_first");
            let second = crate::runtime_state_key!(b"slotorder_second");
            assert_eq!(first.read_own(obj), Some(11.0));
            assert_eq!(first.read_own(obj), Some(11.0));
            assert_eq!(second.read_own(obj), Some(44.0));
            assert_eq!(second.read_own(obj), Some(44.0));
            assert!(second.write_existing(obj, 44.0));
            assert_eq!(js_object_get_field(obj, 0).as_number(), 44.0);
            assert_eq!(first.read_own(obj), Some(11.0));
            assert_emitted_read(obj, SECOND.string, 44.0);
            use crate::object::field_get_set::runtime_read_site as read;
            for _ in 0..3 { assert_eq!(read::get(obj, b"slotorder_second"), 44.0); }
            let answer = read::probe(obj, read::Key::Name(b"slotorder_second"));
            assert!(matches!(answer, Some(read::Answer::Data(bits)) if crate::JSValue::from_bits(bits).as_number() == 44.0));
        });
        let child = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
        crate::object::js_object_set_prototype_of(
            crate::js_nanbox_pointer(child.get_raw_mut_ptr::<ObjectHeader>() as i64),
            crate::js_nanbox_pointer(object.get_raw_mut_ptr::<ObjectHeader>() as i64),
        );
        child.with_mut_ptr(|obj| {
            for _ in 0..3 {
                assert_eq!(
                    crate::object::field_get_set::runtime_read_site::get(obj, b"slotorder_second"),
                    44.0
                );
            }
        });
        let json = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::json::js_json_stringify(crate::js_nanbox_pointer(obj as i64), 0)
        });
        assert_eq!(
            string(crate::JSValue::string_ptr(json)),
            "{\"slotorder_first\":11,\"slotorder_second\":44,\"slotorder_third\":33}"
        );
        let inspected = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::builtins::format_jsvalue(crate::js_nanbox_pointer(obj as i64), 0)
        });
        assert!(
            inspected.find("slotorder_first").unwrap()
                < inspected.find("slotorder_second").unwrap()
        );
        assert!(inspected.contains("slotorder_second: 44"), "{inspected}");
        let cloned = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::builtins::js_structured_clone(crate::js_nanbox_pointer(obj as i64))
        });
        let cloned = scope.root_nanbox_f64(cloned);
        let clone_obj = (cloned.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(
            clone_obj,
            &["slotorder_first", "slotorder_second", "slotorder_third"],
        );
        assert_eq!(
            js_object_get_field_by_name_f64(clone_obj, FIRST.string),
            11.0
        );
        assert_eq!(
            js_object_get_field_by_name_f64(clone_obj, SECOND.string),
            44.0
        );
        let assigned = scope.root_raw_mut_ptr(js_object_alloc_plain(0));
        let result = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::object::js_object_assign_one(
                crate::js_nanbox_pointer(assigned.get_raw_mut_ptr::<ObjectHeader>() as i64),
                crate::js_nanbox_pointer(obj as i64),
            )
        });
        let assigned_obj = (result.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(
            assigned_obj,
            &["slotorder_first", "slotorder_second", "slotorder_third"],
        );
        assert_eq!(
            js_object_get_field_by_name_f64(assigned_obj, FIRST.string),
            11.0
        );
        let spread = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::object::spread::js_object_spread_fresh_leading(
                f64::from_bits(crate::value::TAG_UNDEFINED),
                crate::js_nanbox_pointer(obj as i64),
            )
        });
        let spread = scope.root_nanbox_f64(spread);
        let spread_obj =
            (spread.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(
            spread_obj,
            &["slotorder_first", "slotorder_second", "slotorder_third"],
        );
        assert_eq!(
            js_object_get_field_by_name_f64(spread_obj, THIRD.string),
            33.0
        );
        let extra = scope.root_string_ptr(crate::string::js_string_from_bytes(
            b"slotorder_extra".as_ptr(),
            15,
        ));
        object.with_mut_ptr(|obj| js_object_set_field_by_name(obj, extra.get_raw_mut_ptr(), 55.0));
        object.with_mut_ptr(|obj| {
            assert_order(
                obj,
                &[
                    "slotorder_first",
                    "slotorder_second",
                    "slotorder_third",
                    "slotorder_extra",
                ],
            );
            assert_eq!(js_object_get_field_by_name_f64(obj, FIRST.string), 11.0);
            assert_eq!(
                js_object_get_field_by_name_f64(obj, extra.get_raw_mut_ptr()),
                55.0
            );
        });
        crate::gc::js_gc_collect();
        object.with_mut_ptr(|obj| {
            assert_order(
                obj,
                &[
                    "slotorder_first",
                    "slotorder_second",
                    "slotorder_third",
                    "slotorder_extra",
                ],
            );
            assert_eq!(js_object_delete_field(obj, SECOND.string), 1);
            assert_order(
                obj,
                &["slotorder_first", "slotorder_third", "slotorder_extra"],
            );
            assert_eq!(js_object_get_field_by_name_f64(obj, FIRST.string), 11.0);
            assert_eq!(js_object_get_field_by_name_f64(obj, THIRD.string), 33.0);
            assert_eq!(
                js_object_get_field_by_name_f64(obj, extra.get_raw_mut_ptr()),
                55.0
            );
        });
        object.with_mut_ptr(|obj| {
            js_object_set_field_by_name(obj, SECOND.string, 66.0);
            assert_order(
                obj,
                &[
                    "slotorder_first",
                    "slotorder_third",
                    "slotorder_extra",
                    "slotorder_second",
                ],
            );
            assert_eq!(js_object_get_field_by_name_f64(obj, SECOND.string), 66.0);
            assert_eq!(js_object_delete_field(obj, SECOND.string), 1);
        });
        crate::gc::js_gc_collect();
        object.with_mut_ptr(|obj| {
            assert_order(
                obj,
                &["slotorder_first", "slotorder_third", "slotorder_extra"],
            );
            assert_eq!(js_object_get_field(obj, 2).as_number(), 11.0);
            assert_eq!(js_object_get_field(obj, 1).as_number(), 33.0);
        });
    }
}

#[test]
fn static_layout_slot_order_pointer_slot_survives_full_gc() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(PERMUTED))
    });
    unsafe {
        let value = crate::string::js_string_from_bytes(b"mapped-pointer-value".as_ptr(), 20);
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 2, crate::JSValue::string_ptr(value));
            js_object_set_field(obj, 0, crate::JSValue::number(22.0));
            js_object_set_field(obj, 1, crate::JSValue::number(33.0));
        });
        crate::gc::js_gc_collect();
        object.with_mut_ptr(|obj| {
            assert_order(
                obj,
                &["slotorder_first", "slotorder_second", "slotorder_third"],
            );
            let pointer = js_object_get_field_by_name(obj, FIRST.string);
            assert_eq!(string(pointer), "mapped-pointer-value");
            assert_eq!(js_object_get_field_by_name_f64(obj, THIRD.string), 33.0);
            assert_emitted_read(obj, THIRD.string, 33.0);
            for _ in 0..3 {
                assert_eq!(
                    crate::object::field_get_set::runtime_read_site::get(obj, b"slotorder_third"),
                    33.0
                );
            }
        });
        let json = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::json::js_json_stringify(crate::js_nanbox_pointer(obj as i64), 0)
        });
        assert_eq!(
            string(crate::JSValue::string_ptr(json)),
            "{\"slotorder_first\":\"mapped-pointer-value\",\"slotorder_second\":22,\"slotorder_third\":33}"
        );
        crate::gc::js_gc_collect();
        object.with_mut_ptr(|obj| {
            assert_eq!(
                string(js_object_get_field_by_name(obj, FIRST.string)),
                "mapped-pointer-value"
            );
        });
    }
}

#[test]
fn static_layout_slot_order_rejects_slots_that_are_not_a_permutation() {
    use crate::object::layout_desc::{PerryLayoutDesc, PerryLayoutKey};
    crate::gc::initialize_runtime_fixture();
    // A hole at slot 1 would leave the next position's slot taken.
    static mut KEYS: [PerryLayoutKey; 3] = [
        PerryLayoutKey {
            atom: std::ptr::addr_of!(FIRST),
            slot: 2,
            attrs: 0,
            _pad: [0; 3],
        },
        PerryLayoutKey {
            atom: std::ptr::addr_of!(SECOND),
            slot: 0,
            attrs: 0,
            _pad: [0; 3],
        },
        PerryLayoutKey {
            atom: std::ptr::addr_of!(THIRD),
            slot: 4,
            attrs: 0,
            _pad: [0; 3],
        },
    ];
    let mut desc = PerryLayoutDesc {
        shape_id: 0,
        _layout_pad: 0,
        family: 0x7f000005,
        variant: 0,
        kind: 0,
        integrity: 0,
        slot_count: 5,
        layout_id: 0x80000089,
        rep: 0,
        proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
        keys: std::ptr::null(),
        key_count: 3,
        index_mask: 0,
        index: std::ptr::null(),
        class_id: 0,
        parent_class_id: 0,
        methods: std::ptr::null(),
    };
    unsafe {
        desc.keys = std::ptr::addr_of!(KEYS).cast();
        assert_eq!(
            crate::object::layout_desc::register_one(std::ptr::addr_of!(desc), 1),
            0
        );
    }
}

// Reuse main's atom and its exact StaticAtom kind/index encoding.
#[allow(non_snake_case)]
mod ATOM_SYMBOL_ITERATOR {
    pub const HASH: u64 = crate::codegen_abi::well_known_symbol_atom_hash(
        crate::codegen_abi::WELL_KNOWN_SYMBOL_ITERATOR,
    );
}
runtime_layout!(
    SYMBOL_PERMUTED,
    0x7f000004,
    0,
    crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    [FIRST @ 1, ATOM_SYMBOL_ITERATOR @ 0]
);

#[test]
fn static_layout_slot_order_symbol_reads_writes_reflection_and_delete() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(SYMBOL_PERMUTED))
    });
    unsafe {
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 1, crate::JSValue::number(11.0));
            js_object_set_field(obj, 0, crate::JSValue::number(77.0));
            let symbol = crate::object::object_keys(obj).get(1).bits();
            let value = crate::js_nanbox_pointer(obj as i64);
            assert_eq!(
                crate::symbol::js_object_get_symbol_property(value, f64::from_bits(symbol)),
                77.0
            );
            crate::proxy::js_put_value_set(value, f64::from_bits(symbol), 88.0, value, 1);
            assert_eq!(
                crate::symbol::js_object_get_symbol_property(value, f64::from_bits(symbol)),
                88.0
            );

            assert_eq!(js_object_get_field(obj, 0).as_number(), 88.0);
            assert_eq!(js_object_get_field(obj, 1).as_number(), 11.0);
            let reflected = crate::proxy::js_reflect_own_keys(value);
            let array =
                (reflected.to_bits() & crate::value::POINTER_MASK) as *mut crate::ArrayHeader;
            assert_eq!(crate::array::js_array_length(array), 2);
            assert_eq!(
                string(crate::array::js_array_get(array, 0)),
                "slotorder_first"
            );
            assert_eq!(crate::array::js_array_get(array, 1).bits(), symbol);
            assert!(crate::object::shaped_symbols::delete(
                obj as usize,
                (symbol & crate::value::POINTER_MASK) as usize
            ));
            assert_order(obj, &["slotorder_first"]);
            assert_eq!(js_object_get_field_by_name_f64(obj, FIRST.string), 11.0);
        });
        crate::gc::js_gc_collect();
        object.with_mut_ptr(|obj| {
            assert_eq!(js_object_get_field_by_name_f64(obj, FIRST.string), 11.0)
        });
    }
}

#[test]
fn static_layout_slot_order_holes_use_the_main_presence_readers() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(PERMUTED))
    });
    unsafe {
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 2, crate::JSValue::number(11.0));
            js_object_set_field(obj, 1, crate::JSValue::number(33.0));
            assert_eq!(js_object_delete_field(obj, SECOND.string), 1);
            assert_eq!(js_object_get_field(obj, 0).bits(), crate::value::TAG_HOLE);
            assert_order(obj, &["slotorder_first", "slotorder_third"]);
            assert!(js_object_get_field_by_name(obj, SECOND.string).is_undefined());
        });
    }
    unsafe {
        let json = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::json::js_json_stringify(crate::js_nanbox_pointer(obj as i64), 0)
        });
        assert_eq!(
            string(crate::JSValue::string_ptr(json)),
            "{\"slotorder_first\":11,\"slotorder_third\":33}"
        );
        let cloned = scope.root_nanbox_f64(object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::builtins::js_structured_clone(crate::js_nanbox_pointer(obj as i64))
        }));
        let copied = (cloned.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(copied, &["slotorder_first", "slotorder_third"]);
        assert_eq!(js_object_get_field_by_name_f64(copied, THIRD.string), 33.0);
        let spread = scope.root_nanbox_f64(object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::object::spread::js_object_spread_fresh_leading(
                f64::from_bits(crate::value::TAG_UNDEFINED),
                crate::js_nanbox_pointer(obj as i64),
            )
        }));
        let copied = (spread.get_nanbox_u64() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(copied, &["slotorder_first", "slotorder_third"]);
        assert_eq!(js_object_get_field_by_name_f64(copied, FIRST.string), 11.0);
        let target = scope.root_raw_mut_ptr(js_object_alloc_plain(0));
        let assigned = object.with_const_ptr(|obj: *const ObjectHeader| {
            crate::object::js_object_assign_one(
                crate::js_nanbox_pointer(target.get_raw_mut_ptr::<ObjectHeader>() as i64),
                crate::js_nanbox_pointer(obj as i64),
            )
        });
        let copied = (assigned.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader;
        assert_order(copied, &["slotorder_first", "slotorder_third"]);
        assert_eq!(js_object_get_field_by_name_f64(copied, THIRD.string), 33.0);
    }
}

#[test]
fn static_layout_slot_order_by_name_scans_read_storage_slots() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(PERMUTED))
    });
    unsafe {
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 2, crate::JSValue::number(11.0));
            js_object_set_field(obj, 0, crate::JSValue::number(22.0));
            js_object_set_field(obj, 1, crate::JSValue::number(33.0));
        });
        let value = crate::js_nanbox_pointer(object.get_raw_mut_ptr::<ObjectHeader>() as i64);
        for (name, expected) in [
            (b"slotorder_first".as_slice(), 11.0),
            (b"slotorder_second".as_slice(), 22.0),
            (b"slotorder_third".as_slice(), 33.0),
        ] {
            assert_eq!(
                crate::fs::options_field_value(value, name).map(|v| v.as_number()),
                Some(expected)
            );
        }
    }
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "positional lane on a layout whose key 0 keeps storage slot 2")]
fn static_layout_slot_order_positional_lane_invariant_fails_loudly() {
    crate::gc::initialize_runtime_fixture();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(unsafe {
        crate::object::layout_desc::perry_alloc_layout(std::ptr::addr_of!(PERMUTED))
    });
    unsafe {
        let keys = crate::object::object_keys(object.get_raw_mut_ptr::<ObjectHeader>());
        crate::object::key_attrs::debug_assert_positional(keys.arr(), keys.count());
    }
}

/// A runtime layout is born with every slot holding the hole marker
/// (`perry_alloc_layout`), so an unwritten member is a hole slot: the probe
/// SERVES it as an absent key (read, presence) instead of declining, and an
/// own store of a written member is one probe store.
#[test]
fn the_probe_serves_a_holey_runtime_layout() {
    crate::gc::initialize_runtime_fixture();
    crate::object::ensure_object_intrinsics();
    let desc = std::ptr::addr_of!(POSITIONAL);
    let scope = crate::gc::RuntimeHandleScope::new();
    let object =
        scope.root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_layout(desc) });
    unsafe {
        object.with_mut_ptr(|obj| {
            js_object_set_field(obj, 0, crate::JSValue::number(1.0));
            js_object_set_field(obj, 2, crate::JSValue::number(3.0));
            let dir = crate::object::shapes::ordinary_dir_addr();
            let atom = |s: *const crate::StringHeader| {
                let a = crate::string::existing_property_atom(s).expect("program atom");
                (
                    crate::value::STRING_TAG | a as u64,
                    crate::string::property_atom_hash(a),
                )
            };
            let biased = (obj as usize).wrapping_sub(perry_abi::RECEIVER_HANDLE_FLOOR) as i64;
            let value = crate::js_nanbox_pointer(obj as i64).to_bits();
            let (second, second_hash) = atom(SECOND.string);
            let (first, first_hash) = atom(FIRST.string);
            assert_eq!(
                crate::object::layout_probe::perry_probe_get(dir, biased, second, second_hash)
                    .to_bits(),
                crate::value::TAG_UNDEFINED,
                "a hole slot is an absent key the probe answers"
            );
            assert_eq!(
                crate::object::layout_probe::probe_has(value, second, false),
                0,
                "presence: the hole slot is absent along the whole chain"
            );
            assert_eq!(
                crate::object::layout_probe::probe_has(value, second, true),
                0
            );
            assert_eq!(
                crate::object::layout_probe::probe_has(value, first, true),
                1
            );
            assert_eq!(
                crate::object::layout_probe::perry_probe_get(dir, biased, first, first_hash),
                1.0
            );
            assert_eq!(
                crate::object::layout_probe::perry_probe_set(dir, value, first, first_hash, 5.0),
                1,
                "an own written member is one probe store"
            );
            assert_eq!(js_object_get_field(obj, 0).as_number(), 5.0);
            // The absent member's store adds a key, which enumerates last; a
            // later key is present, so its key position is not the end and
            // the probe leaves the add to the complete [[Set]]. (This layout
            // declares no optional key: the hole is an unfinished birth, whose
            // store fills its slot.)
            assert_eq!(
                crate::object::layout_probe::perry_probe_set(dir, value, second, second_hash, 7.0),
                0,
                "an out-of-order hole fill is the complete [[Set]]'s"
            );
            js_object_set_field_by_name(obj, SECOND.string, 7.0);
        });
        let obj = object.get_raw_mut_ptr::<ObjectHeader>();
        assert_eq!(js_object_get_field_by_name_f64(obj, SECOND.string), 7.0);
        assert_order(
            obj,
            &["slotorder_first", "slotorder_second", "slotorder_third"],
        );
        let value = crate::js_nanbox_pointer(obj as i64).to_bits();
        let a = crate::string::existing_property_atom(SECOND.string).expect("program atom");
        assert_eq!(
            crate::object::layout_probe::probe_has(
                value,
                crate::value::STRING_TAG | a as u64,
                true
            ),
            1
        );
    }
}

/// The store probe keeps the write barrier the deleted inline store IC
/// emitted: a pointer stored into an OLD object's slot remembers the slot's
/// page, so the next minor finds the young child.
#[test]
fn a_probe_store_of_a_pointer_into_an_old_object_keeps_its_write_barrier() {
    crate::gc::initialize_runtime_fixture();
    crate::object::ensure_object_intrinsics();
    let _copying_nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force_evacuation = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
    let _age = crate::gc::pin_tenuring_survivals_for_test(1);
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    let desc = std::ptr::addr_of!(POSITIONAL);
    let scope = crate::gc::RuntimeHandleScope::new();
    let object =
        scope.root_raw_mut_ptr(unsafe { crate::object::layout_desc::perry_alloc_layout(desc) });
    object.with_mut_ptr(|obj| js_object_set_field(obj, 0, crate::JSValue::number(1.0)));
    let _ = crate::gc::gc_collect_minor();
    let _ = crate::gc::gc_collect_minor();
    unsafe {
        let obj = object.get_raw_mut_ptr::<ObjectHeader>();
        assert!(
            crate::gc::never_move::lives_old(obj as usize),
            "the fixture's receiver must be old, or the barrier is untested"
        );
        let child = crate::object::object_alloc_plain(0);
        let child_value = crate::js_nanbox_pointer(child as i64);
        let a = crate::string::existing_property_atom(FIRST.string).expect("program atom");
        let (key, hash) = (
            crate::value::STRING_TAG | a as u64,
            crate::string::property_atom_hash(a),
        );
        let slot = (obj as usize) + std::mem::size_of::<ObjectHeader>();
        assert!(!crate::gc::old_slot_page_is_remembered_for_test(slot));
        assert_eq!(
            crate::object::layout_probe::perry_probe_set(
                crate::object::shapes::ordinary_dir_addr(),
                crate::js_nanbox_pointer(obj as i64).to_bits(),
                key,
                hash,
                child_value,
            ),
            1,
            "an own plain data slot is one probe store"
        );
        assert_eq!(js_object_get_field(obj, 0).bits(), child_value.to_bits());
        assert!(
            crate::gc::old_slot_page_is_remembered_for_test(slot),
            "a pointer store into an old object must remember its page"
        );
    }
}

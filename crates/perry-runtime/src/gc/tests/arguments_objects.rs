//! Arguments construction must preserve per-call state and trace shared keys.

use super::super::*;
use super::support::{ptr_bits, CopyingNurseryTestGuard, GcTestIsolationGuard};
use crate::object::*;
use crate::value::JSValue;

fn key(name: &str) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn arguments(values: &[f64], callee: f64, restricted: bool) -> *mut ObjectHeader {
    let mut array = crate::array::js_array_alloc(values.len() as u32);
    for &value in values {
        array = crate::array::js_array_push_f64(array, value);
    }
    js_arguments_object_alloc(
        crate::value::js_nanbox_pointer(array as i64),
        callee,
        restricted as i32,
    )
}

fn get(obj: *const ObjectHeader, name: &str) -> JSValue {
    js_object_get_field_by_name(obj, key(name))
}

fn register_scanners() {
    gc_register_mutable_root_scanner(scan_arguments_object_roots_mut);
    gc_register_mutable_root_scanner(descriptor_state::scan_descriptor_roots_mut);
}

#[test]
fn arguments_share_keys_but_keep_identity_values_and_descriptors_private() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    test_clear_arguments_object_roots();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let first = arguments(&[1.0, 2.0, 3.0], undefined, false);
    let second = arguments(&[4.0, 5.0, 6.0], undefined, false);
    assert_ne!(first, second);
    unsafe {
        assert_eq!(object_keys_array(first), object_keys_array(second));
    }
    // All-true indexed attributes are implicit, but reflection still exposes
    // the complete default descriptor. This also checks the optimization ran.
    assert!(get_property_attrs(first as usize, "0").is_none());
    let descriptor = unsafe { arguments_object_descriptor(first, key("0")) }.unwrap();
    let descriptor = crate::value::js_nanbox_get_pointer(descriptor) as *const ObjectHeader;
    assert_eq!(get(descriptor, "value").bits(), 1.0f64.to_bits());
    for name in ["writable", "enumerable", "configurable"] {
        assert_eq!(get(descriptor, name).bits(), crate::value::TAG_TRUE);
    }
    let length_attrs = get_property_attrs(first as usize, "length").unwrap();
    assert!(length_attrs.writable() && !length_attrs.enumerable() && length_attrs.configurable());
    let callee_attrs = get_property_attrs(first as usize, "callee").unwrap();
    assert!(callee_attrs.writable() && !callee_attrs.enumerable() && callee_attrs.configurable());
    assert_eq!(get(first, "length").bits(), 3.0f64.to_bits());

    js_object_set_field_by_name(first, key("0"), 8.0);
    assert_eq!(get(second, "0").bits(), 4.0f64.to_bits());
    set_property_attrs(
        first as usize,
        "0".into(),
        PropertyAttrs::new(false, false, false),
    );
    assert!(get_property_attrs(second as usize, "0").is_none());
    assert_eq!(js_object_delete_field(first, key("1")), 1);
    js_object_set_field_by_name(first, key("extra"), 9.0);
    assert!(get(first, "1").is_undefined());
    assert_eq!(get(second, "1").bits(), 5.0f64.to_bits());
    assert!(get(second, "extra").is_undefined());
    let third = arguments(&[10.0, 11.0, 12.0], undefined, false);
    assert_eq!(get(third, "1").bits(), 11.0f64.to_bits());
    assert!(get(third, "extra").is_undefined());
    unsafe {
        assert_eq!(object_keys_array(second), object_keys_array(third));
        assert_ne!(object_keys_array(first), object_keys_array(second));
    }
}

#[test]
fn arguments_bulk_construction_handles_empty_and_uncached_arities() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    for len in [0, 1, 3, 64, 65, 100] {
        let values: Vec<f64> = (0..len).map(|i| i as f64 + 0.5).collect();
        let args = arguments(&values, undefined, true);
        assert_eq!(get(args, "length").bits(), (len as f64).to_bits());
        for (i, value) in values.iter().enumerate() {
            assert_eq!(get(args, &i.to_string()).bits(), value.to_bits());
        }
        assert!(get(args, &len.to_string()).is_undefined());
        let attrs = get_property_attrs(args as usize, "callee").unwrap();
        assert!(!attrs.writable() && !attrs.enumerable() && !attrs.configurable());
        let accessor = get_accessor_descriptor(args as usize, "callee").unwrap();
        assert_eq!(accessor.get, accessor.set);
        assert_ne!(accessor.get, 0);
    }
}

#[test]
fn arguments_shared_keys_survive_moving_gc_without_a_live_arguments_owner() {
    let _guard = CopyingNurseryTestGuard::new(0);
    register_scanners();
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let before = arguments(&[1.0, 2.0, 3.0], undefined, false);
    let before_keys = unsafe { object_keys_array(before) };
    gc_collect_minor();
    let after = arguments(&[4.0, 5.0, 6.0], undefined, false);
    let after_keys = unsafe { object_keys_array(after) };
    assert_ne!(
        before_keys, after_keys,
        "the cached keys must actually evacuate"
    );
    assert_eq!(get(after, "0").bits(), 4.0f64.to_bits());
    assert_eq!(get(after, "length").bits(), 3.0f64.to_bits());
}

#[test]
fn arguments_values_callee_and_mapping_survive_moving_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    register_scanners();
    let child = js_object_alloc(0, 1);
    js_object_set_field(child, 0, JSValue::number(42.0));
    let child_value = crate::value::js_nanbox_pointer(child as i64);
    let text = crate::string::js_string_from_bytes(b"arguments payload".as_ptr(), 17);
    let text_value = crate::value::js_nanbox_string(text as i64);
    let args = arguments(&[child_value, text_value, 7.0], child_value, false);
    let mapped = crate::r#box::js_box_alloc(8.0);
    js_arguments_object_map_index(args, 2, mapped);
    js_shadow_slot_set(0, ptr_bits(args as usize));

    gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut ObjectHeader;
    assert_ne!(
        moved, args,
        "the escaping arguments object must actually evacuate"
    );
    assert!(is_arguments_object(moved));
    let moved_child = get(moved, "0").as_pointer::<ObjectHeader>();
    assert_ne!(
        moved_child, child,
        "indexed heap references must actually evacuate"
    );
    assert_eq!(
        js_object_get_field(moved_child, 0).bits(),
        42.0f64.to_bits()
    );
    assert_eq!(get(moved, "callee").bits(), get(moved, "0").bits());
    let moved_text = get(moved, "1");
    assert!(unsafe { crate::string::js_string_key_matches(moved_text, key("arguments payload")) });
    assert_eq!(get(moved, "2").bits(), 8.0f64.to_bits());
    js_object_set_field_by_name(moved, key("2"), 9.0);
    assert_eq!(crate::r#box::js_box_get(mapped), 9.0);
    assert_eq!(get(moved, "length").bits(), 3.0f64.to_bits());
    assert!(!get_property_attrs(moved as usize, "length")
        .unwrap()
        .enumerable());
}

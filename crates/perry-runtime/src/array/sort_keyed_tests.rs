use super::*;
use crate::object::{js_object_alloc_plain, js_object_set_field_by_name};

unsafe fn fixture(numbers: &[f64]) -> (*mut ArrayHeader, u64, Vec<u64>) {
    let key = crate::string::js_string_from_str("rank");
    let key_bits = crate::JSValue::string_ptr(key).bits();
    let mut array = js_array_alloc(numbers.len() as u32);
    let mut objects = vec![];
    for &number in numbers {
        let object = js_object_alloc_plain(1);
        js_object_set_field_by_name(object, key, number);
        let bits = crate::value::POINTER_TAG | object as u64;
        array = js_array_push_f64(array, f64::from_bits(bits));
        objects.push(bits);
    }
    (array, key_bits, objects)
}

#[test]
fn keyed_sort_admits_own_data_and_preserves_equal_key_identity() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        for descending in [false, true] {
            let numbers = [0.25, -0.0, 0.0, f64::INFINITY, 0.25, f64::NEG_INFINITY];
            let (array, key, objects) = fixture(&numbers);
            assert!(try_sort_keyed(array, key, descending));
            let mut order: Vec<usize> = (0..numbers.len()).collect();
            order.sort_by(|&a, &b| {
                if descending {
                    numbers[b].partial_cmp(&numbers[a]).unwrap()
                } else {
                    numbers[a].partial_cmp(&numbers[b]).unwrap()
                }
            });
            for (i, source) in order.into_iter().enumerate() {
                assert_eq!(*array_elements_ptr(array).add(i), objects[source]);
            }
        }
    }
}

#[test]
fn keyed_sort_refuses_missing_nan_and_coercing_keys_before_writing() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        for invalid in [
            f64::NAN,
            f64::from_bits(crate::value::TAG_UNDEFINED),
            f64::from_bits(crate::value::TAG_NULL),
        ] {
            let (array, key, objects) = fixture(&[3., invalid, 1.]);
            assert!(!try_sort_keyed(array, key, false));
            for (i, bits) in objects.iter().enumerate() {
                assert_eq!(*array_elements_ptr(array).add(i), *bits);
            }
        }
        let (array, _, objects) = fixture(&[3., 1., 2.]);
        let absent = crate::string::js_string_from_str("absent");
        assert!(!try_sort_keyed(
            array,
            crate::JSValue::string_ptr(absent).bits(),
            false
        ));
        for (i, bits) in objects.iter().enumerate() {
            assert_eq!(*array_elements_ptr(array).add(i), *bits);
        }
    }
}

#[test]
fn keyed_sort_refuses_overflow_shadowing_before_writing() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let (array, key, objects) = fixture(&[3., 1., 2.]);
        let object = (objects[1] & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
        let name = crate::string::js_string_from_str("rank");
        assert_eq!(crate::object::js_object_delete_field(object, name), 1);
        crate::object::js_object_set_field_by_name(object, name, 2.5);
        assert!(crate::object::overflow::has_overflow(object));
        assert!(!try_sort_keyed(array, key, false));
        for (i, bits) in objects.iter().enumerate() {
            assert_eq!(*array_elements_ptr(array).add(i), *bits);
        }
    }
}

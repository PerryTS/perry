use super::*;
use crate::array::header::array_numeric_layout;

fn array(values: &[f64]) -> *mut ArrayHeader {
    let a = js_array_alloc(values.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let a = scope.root_raw_mut_ptr(a);
    for &v in values {
        let live = js_array_push_f64(a.get_raw_mut_ptr(), v);
        a.set_raw_mut_ptr(live);
    }
    a.get_raw_mut_ptr()
}

#[test]
fn numeric_sort_admits_integer_domain_and_both_directions() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let values = [i32::MAX as f64, 2.0, -7.0, 0.0, 2.0, i32::MIN as f64];
    for descending in [false, true] {
        let a = array(&values);
        unsafe {
            assert!(try_sort_numeric(a, descending));
            assert_eq!((*a).ek_prefix, values.len() as u32);
            assert!(array_numeric_layout(a).is_some());
        }
        let mut expected = values;
        expected.sort_by(|a, b| {
            if descending {
                b.partial_cmp(a).unwrap()
            } else {
                a.partial_cmp(b).unwrap()
            }
        });
        for (i, expected) in expected.into_iter().enumerate() {
            assert_eq!(js_array_get_f64(a, i as u32).to_bits(), expected.to_bits());
        }
    }
}

#[test]
fn numeric_sort_refuses_unsupported_keys_without_writing() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for invalid in [
        f64::NAN,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.25,
        i32::MAX as f64 + 1.0,
        i32::MIN as f64 - 1.0,
    ] {
        let values = [3.0, invalid, 1.0];
        let a = array(&values);
        unsafe {
            assert!(!try_sort_numeric(a, false));
        }
        for (i, value) in values.into_iter().enumerate() {
            assert_eq!(js_array_get_f64(a, i as u32).to_bits(), value.to_bits());
        }
    }
}

#[test]
fn numeric_sort_refuses_holes_and_restricted_shapes() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let holey = js_array_alloc_with_length(4);
    unsafe {
        assert!(!try_sort_numeric(holey, false));
    }
    for flag in [
        crate::gc::OBJ_FLAG_FROZEN,
        crate::gc::OBJ_FLAG_SEALED,
        crate::gc::OBJ_FLAG_NO_EXTEND,
        crate::gc::GC_ARRAY_CUSTOM_PROTO,
        crate::gc::GC_ARRAY_ARGUMENTS_OBJECT,
    ] {
        let a = array(&[3.0, 1.0, 2.0]);
        unsafe {
            let header = (a as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*header)._reserved |= flag;
            assert!(!try_sort_numeric(a, false));
        }
        assert_eq!(js_array_get_f64(a, 0), 3.0);
    }
}

#[test]
fn numeric_sort_radix_matches_reference_for_duplicate_signed_keys() {
    let mut seed = 73u32;
    let mut keys: Vec<u32> = (0..8192)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed & 0x8000_ffff
        })
        .collect();
    let mut counts = [[0usize; 256]; 4];
    for &key in &keys {
        for (digit, count) in counts.iter_mut().enumerate() {
            count[((key >> (digit * 8)) & 255) as usize] += 1;
        }
    }
    let mut expected = keys.clone();
    expected.sort();
    radix_keys(&mut keys, &mut counts);
    assert_eq!(keys, expected);
}

#[test]
fn numeric_sort_admits_dense_numbers_without_a_raw_f64_brand() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let a = array(&[3.0, 1.0, i32::MAX as f64, 2.0]);
    unsafe {
        crate::array::header::clear_array_numeric_layout(a);
        note_array_slot(a, 2, crate::value::JSValue::int32(i32::MAX).bits());
        assert!(array_numeric_layout(a).is_none());
        assert!(try_sort_numeric(a, false));
        // The operation need not change a producer's conservative layout.
        assert!(array_numeric_layout(a).is_none());
    }
    for (i, value) in [1.0, 2.0, 3.0, i32::MAX as f64].into_iter().enumerate() {
        assert_eq!(js_array_get_f64(a, i as u32), value);
    }
}

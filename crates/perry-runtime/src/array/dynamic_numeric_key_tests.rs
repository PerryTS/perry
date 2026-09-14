//! #10190: the erased-receiver fallback must preserve the original numeric key.

use super::subclass::js_packed_arraylike_index_get;
use crate::value::js_dyn_index_get;
use crate::value::{JSValue, TAG_UNDEFINED};

fn receiver(state: u8) -> f64 {
    if state == 0 {
        let mut array = crate::array::js_array_alloc(3);
        for value in [11.0, 22.0, 33.0] {
            array = crate::array::js_array_push_f64(array, value);
        }
        return crate::value::js_nanbox_pointer(array as i64);
    }
    let bytes = b"[11,22,33]";
    let tape = crate::json_tape::build_tape(bytes).unwrap();
    let text = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let scope = crate::gc::RuntimeHandleScope::new();
    let array = scope
        .root_raw_mut_ptr(unsafe { crate::json_tape::alloc_lazy_array(&tape.entries, 0, 3, text) });
    if state == 2 {
        array.with_mut_ptr(|lazy: *mut crate::json_tape::LazyArrayHeader| unsafe {
            crate::json_tape::force_materialize_lazy(lazy)
        });
    }
    array.with_const_ptr(|raw: *const crate::json_tape::LazyArrayHeader| {
        let header = unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }.unwrap();
        assert_eq!(header.obj_type, crate::gc::GC_TYPE_LAZY_ARRAY);
        assert_eq!(unsafe { !(*raw).materialized.is_null() }, state == 2);
        crate::value::js_nanbox_pointer(raw as i64)
    })
}

#[test]
fn non_element_numeric_keys_do_not_truncate_on_regular_lazy_or_materialized_arrays() {
    for state in 0..3 {
        let scope = crate::gc::RuntimeHandleScope::new();
        let array = scope.root_nanbox_f64(receiver(state));
        for key in [
            0.5,
            -0.5,
            1.5,
            -1.0,
            2_147_483_648.0,
            4_294_967_295.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert_eq!(
                js_dyn_index_get(array.get_nanbox_f64(), key).to_bits(),
                TAG_UNDEFINED,
                "state={state}, key={key}"
            );
            assert_eq!(
                js_packed_arraylike_index_get(array.get_nanbox_f64(), key, std::ptr::null_mut())
                    .to_bits(),
                TAG_UNDEFINED,
                "packed state={state}, key={key}"
            );
        }
        for (key, expected) in [
            (0.0, 11.0),
            (-0.0, 11.0),
            (1.0, 22.0),
            (f64::from_bits(JSValue::int32(2).bits()), 33.0),
        ] {
            assert_eq!(js_dyn_index_get(array.get_nanbox_f64(), key), expected);
            assert_eq!(
                js_packed_arraylike_index_get(array.get_nanbox_f64(), key, std::ptr::null_mut()),
                expected
            );
        }
    }
}

#[test]
fn numeric_named_properties_remain_readable_through_the_dynamic_fallback() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let array = scope.root_nanbox_f64(receiver(0));
    for (key, name) in [
        (0.5, "0.5"),
        (-0.5, "-0.5"),
        (-1.0, "-1"),
        (4_294_967_295.0, "4294967295"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (1e21, "1e+21"),
    ] {
        let name = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let raw = JSValue::from_bits(array.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        crate::array::js_array_set_string_key(raw, name, 91.0);
        assert_eq!(
            js_dyn_index_get(array.get_nanbox_f64(), key),
            91.0,
            "key={key}"
        );
        assert_eq!(
            js_packed_arraylike_index_get(array.get_nanbox_f64(), key, std::ptr::null_mut()),
            91.0,
            "packed key={key}"
        );
    }
    assert_eq!(js_dyn_index_get(array.get_nanbox_f64(), 0.0), 11.0);
}

#[test]
fn boxed_integer_keys_keep_their_object_property_semantics() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let key = crate::string::js_string_from_bytes(b"2".as_ptr(), 1);
    object.with_mut_ptr(|o: *mut crate::object::ObjectHeader| {
        crate::object::js_object_set_field_by_name(o, key, 42.0)
    });
    let read = |key| {
        object.with_const_ptr(|o: *const crate::object::ObjectHeader| {
            js_dyn_index_get(crate::value::js_nanbox_pointer(o as i64), key)
        })
    };
    assert_eq!(read(2.0), 42.0);
    assert_eq!(read(f64::from_bits(JSValue::int32(2).bits())), 42.0);
}

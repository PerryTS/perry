//! #10175: valid inline storage has no arbitrary 10,000-field read cutoff.

use super::*;

#[test]
fn wide_inline_reads_use_the_published_slot_bound() {
    let scope = crate::gc::RuntimeHandleScope::new();
    for count in [9_999, 10_000, 10_001, 60_000] {
        let object = scope.root_raw_mut_ptr(js_object_alloc(0, count));
        assert_eq!(
            object.with_const_ptr(|o: *const ObjectHeader| unsafe { object_live_slot_count(o) }),
            count
        );
        for index in [0, 5, count / 2, count - 1] {
            object.with_mut_ptr(|o: *mut ObjectHeader| {
                js_object_set_field(o, index, JSValue::number(index as f64))
            });
            assert_eq!(
                object
                    .with_const_ptr(|o: *const ObjectHeader| js_object_get_field(o, index))
                    .as_number(),
                index as f64,
                "count={count}, index={index}"
            );
        }
        for index in [count, count + 1, u32::MAX] {
            assert!(
                object
                    .with_const_ptr(|o: *const ObjectHeader| js_object_get_field(o, index))
                    .is_undefined(),
                "out-of-bounds count={count}, index={index}"
            );
        }
    }
}

#[test]
fn parsed_wide_objects_keep_computed_reads_and_entries() {
    for count in [10_000, 10_001] {
        let mut input = String::from("{");
        for index in 0..count {
            if index != 0 {
                input.push(',');
            }
            input.push_str(&format!("\"k{index}\":{index}"));
        }
        input.push('}');
        let text = crate::string::js_string_from_bytes(input.as_ptr(), input.len() as u32);
        let parsed = unsafe { crate::json::js_json_parse(text) };
        assert!(parsed.is_pointer());
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_const_ptr(parsed.as_pointer::<ObjectHeader>());
        assert_eq!(
            object.with_const_ptr(|o: *const ObjectHeader| unsafe { object_live_slot_count(o) }),
            count,
            "the parser must exercise the wide inline representation"
        );
        for index in [0, 5, count / 2, count - 1] {
            let name = format!("k{index}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert_eq!(
                object
                    .with_const_ptr(|o: *const ObjectHeader| js_object_get_field_by_name(o, key))
                    .as_number(),
                index as f64
            );
        }
        let entries = scope
            .root_raw_mut_ptr(object.with_const_ptr(|o: *const ObjectHeader| js_object_entries(o)));
        assert_eq!(
            entries.with_const_ptr(|e: *const crate::array::ArrayHeader| {
                crate::array::js_array_length(e)
            }),
            count
        );
        for index in [0, 5, count / 2, count - 1] {
            let pair = entries.with_const_ptr(|e: *const crate::array::ArrayHeader| {
                crate::array::js_array_get(e, index)
            });
            assert_eq!(
                crate::array::js_array_get(pair.as_pointer(), 1).as_number(),
                index as f64
            );
        }
    }
}

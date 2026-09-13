//! #10175: valid inline storage has no arbitrary 10,000-field read cutoff.

use super::*;

#[test]
fn wide_inline_reads_use_the_published_slot_bound() {
    let scope = crate::gc::RuntimeHandleScope::new();
    for count in [9_999, 10_000, 10_001, 60_000] {
        let object = scope.root_raw_mut_ptr(js_object_alloc(0, count));
        assert_eq!(
            unsafe { object_live_slot_count(object.get_raw_const_ptr()) },
            count
        );
        for index in [0, 5, count / 2, count - 1] {
            js_object_set_field(
                object.get_raw_mut_ptr(),
                index,
                JSValue::number(index as f64),
            );
            assert_eq!(
                js_object_get_field(object.get_raw_const_ptr(), index).as_number(),
                index as f64,
                "count={count}, index={index}"
            );
        }
        for index in [count, count + 1, u32::MAX] {
            assert!(
                js_object_get_field(object.get_raw_const_ptr(), index).is_undefined(),
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
        let raw = || object.get_raw_const_ptr::<ObjectHeader>();
        assert_eq!(
            unsafe { object_live_slot_count(raw()) },
            count,
            "the parser must exercise the wide inline representation"
        );
        for index in [0, 5, count / 2, count - 1] {
            let name = format!("k{index}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert_eq!(
                js_object_get_field_by_name(raw(), key).as_number(),
                index as f64
            );
        }
        let entries = scope.root_raw_mut_ptr(js_object_entries(raw()));
        assert_eq!(
            crate::array::js_array_length(entries.get_raw_const_ptr()),
            count
        );
        for index in [0, 5, count / 2, count - 1] {
            let pair = crate::array::js_array_get(entries.get_raw_const_ptr(), index);
            assert_eq!(
                crate::array::js_array_get(pair.as_pointer(), 1).as_number(),
                index as f64
            );
        }
    }
}

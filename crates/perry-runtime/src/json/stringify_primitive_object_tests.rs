use super::*;

#[test]
fn primitive_object_leaf_preserves_wide_output_without_managed_scratch() {
    unsafe {
        let mut text = String::from("{");
        for i in 0..128 {
            if i != 0 {
                text.push(',');
            }
            let value = ["null", "true", "false", "1.25", "\"東京🙂\\n\"", "\"a\""][i % 6];
            write!(&mut text, "\"field_{i}\":{value}").unwrap();
        }
        text.push('}');
        let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = crate::json::test_json_parse_direct(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let input = scope.root_raw_const_ptr(value.as_pointer::<crate::ObjectHeader>());
        let obj = input.get_raw_const_ptr::<crate::ObjectHeader>();
        let keys = crate::object::object_keys_array(obj);
        assert_eq!((*keys).length, 128);
        assert!((*keys).length <= crate::object::object_live_slot_count(obj));
        let fields = (obj as *const u8)
            .add(std::mem::size_of::<crate::ObjectHeader>())
            .cast::<u64>();
        assert!((0..128).all(|i| field_is_primitive(*fields.add(i))));
        let bytes = crate::arena::arena_total_bytes();
        let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
        let mut collections = 0;
        crate::gc::js_gc_stats(&mut collections, std::ptr::null_mut(), std::ptr::null_mut());
        let mut output = String::with_capacity(text.len());
        for _ in 0..1000 {
            output.clear();
            emit_validated(obj, keys, None, &mut output);
            assert_eq!(output, text);
        }
        let mut after = 0;
        crate::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
        assert_eq!(after, collections);
        assert_eq!(crate::arena::arena_total_bytes(), bytes);
        assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
    }
}

#[test]
fn primitive_object_leaf_uses_existing_key_order_and_omits_undefined() {
    unsafe {
        let text = b"{\"20\":20,\"3\":3,\"z\":null,\"a\":\"quote\\\"\"}";
        let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = crate::json::test_json_parse_direct(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let input = scope.root_raw_const_ptr(value.as_pointer::<crate::ObjectHeader>());
        crate::object::js_object_set_field(
            input.get_raw_const_ptr::<crate::ObjectHeader>() as *mut crate::ObjectHeader,
            2,
            JSValue::from_bits(TAG_UNDEFINED),
        );
        let obj = input.get_raw_const_ptr::<crate::ObjectHeader>();
        let keys = crate::object::object_keys_array(obj);
        assert!((*keys).length <= crate::object::object_live_slot_count(obj));
        let order = crate::object::ecma_own_key_order(keys);
        let mut output = String::new();
        emit_validated(obj, keys, order.as_deref(), &mut output);
        assert_eq!(output, "{\"3\":3,\"20\":20,\"a\":\"quote\\\"\"}");
    }
}

#[test]
fn primitive_object_preflight_rejects_callback_and_hole_tags() {
    for bits in [
        POINTER_TAG | 0x12345678,
        BIGINT_TAG | 1,
        crate::value::TAG_HOLE,
        0x0000_0012_3456_7888,
    ] {
        assert!(!field_is_primitive(bits));
    }
    for bits in [
        TAG_UNDEFINED,
        TAG_NULL,
        TAG_TRUE,
        1.5f64.to_bits(),
        INT32_TAG | 7,
    ] {
        assert!(field_is_primitive(bits));
    }
}

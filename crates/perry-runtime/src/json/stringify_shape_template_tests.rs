use super::*;

#[test]
fn shape_template_declines_element_descriptors_before_output() {
    unsafe {
        let text = b"{\"id\":1,\"tags\":[1,2]}";
        let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = crate::json::test_json_parse_direct(source);
        let obj = value.as_pointer::<crate::ObjectHeader>() as *mut crate::ObjectHeader;
        let template = build_shape_prefix_template(value.bits()).unwrap();
        // The descriptor bit travels with this receiver, independently of
        // the shared keys array. Marking it must invalidate raw-slot emission
        // even when a template was already built for the same shape.
        crate::object::set_property_attrs(
            obj as usize,
            "id".into(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
        assert_eq!(
            crate::object::object_keys_array(obj),
            template.keys_arr.get()
        );
        let mut output = String::from("unchanged");
        let capacity = output.capacity();
        let mut data_record_global_proof = false;
        assert!(!try_emit_shape_element(
            value.bits(),
            &template,
            &mut output,
            0,
            None,
            &mut data_record_global_proof,
        ));
        assert_eq!(output, "unchanged");
        assert_eq!(output.capacity(), capacity);
        crate::object::clear_property_attrs(obj as usize, "id");
    }
}

#[test]
fn callback_free_record_does_not_publish_its_array_index_key() {
    unsafe {
        let text = b"{\"id\":1,\"tags\":[\"a\",2]}";
        let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let value = crate::json::test_json_parse_direct(source);
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_const_ptr(value.as_pointer::<crate::ObjectHeader>());
        invalidate_object_proto_tojson_state();
        assert!(object.with_const_ptr(|object: *const u8| {
            super::super::stringify_tojson_probe::to_json_definitely_absent(object)
        }));
        object.with_const_ptr(|object: *const crate::ObjectHeader| {
            let template = build_shape_prefix_template(make_pointer_bits(object.cast())).unwrap();
            assert!(template.data_record_candidate);
            set_to_json_key_str("sentinel");
            let mut output = String::new();
            let mut data_record_global_proof = false;
            assert!(try_emit_shape_element(
                make_pointer_bits(object.cast()),
                &template,
                &mut output,
                0,
                Some(42),
                &mut data_record_global_proof,
            ));
            assert_eq!(output.as_bytes(), text);
            assert_eq!(TO_JSON_KEY.with(|key| key.borrow().clone()), "sentinel");
        });
    }
}

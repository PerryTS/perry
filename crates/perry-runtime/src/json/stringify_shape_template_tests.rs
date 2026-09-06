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
        assert!(!try_emit_shape_element(
            value.bits(),
            &template,
            &mut output,
            0
        ));
        assert_eq!(output, "unchanged");
        assert_eq!(output.capacity(), capacity);
        crate::object::clear_property_attrs(obj as usize, "id");
    }
}

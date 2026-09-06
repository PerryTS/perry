use super::*;

fn collection_count() -> u64 {
    let mut count = 0;
    crate::gc::js_gc_stats(&mut count, std::ptr::null_mut(), std::ptr::null_mut());
    count
}

unsafe fn with_record(text: &str, test: impl FnOnce(*const crate::ObjectHeader, &ShapeTemplate)) {
    let source = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    let value = crate::json::test_json_parse_direct(source);
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_raw_const_ptr(value.as_pointer::<crate::ObjectHeader>());
    // Deliberately warm the potentially allocating first prototype lookup
    // while the input is rooted, before constructing a borrowed template.
    invalidate_object_proto_tojson_state();
    assert!(
        super::super::stringify_tojson_probe::to_json_definitely_absent(
            input.get_raw_const_ptr::<u8>()
        )
    );
    let obj = input.get_raw_const_ptr::<crate::ObjectHeader>();
    let template = super::super::stringify_shape_template::build_shape_prefix_template(
        make_pointer_bits(obj.cast()),
    )
    .unwrap();
    test(obj, &template);
}

#[test]
fn data_records_emit_without_managed_scratch_or_root_stack_changes() {
    unsafe {
        let text = "{\"id\":42,\"name\":\"東京🙂\",\"active\":true,\"score\":1.5,\"tags\":[\"line\\n\",null,false,3.25]}";
        with_record(text, |obj, template| {
            assert!(template.data_record_candidate);
            let bytes = crate::arena::arena_total_bytes();
            let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
            let collections = collection_count();
            let stack = STRINGIFY_STACK.with(|c| c.borrow().len());
            let mut output = String::with_capacity(256);
            for _ in 0..1000 {
                output.clear();
                assert!(try_emit(obj, template, &mut output, 0));
                assert_eq!(output, text);
            }
            assert_eq!(crate::arena::arena_total_bytes(), bytes);
            assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
            assert_eq!(collection_count(), collections);
            assert_eq!(STRINGIFY_STACK.with(|c| c.borrow().len()), stack);
        });
    }
}

#[test]
fn data_records_decline_complex_children_before_output() {
    unsafe {
        for text in [
            "{\"id\":1,\"tags\":[1,{\"x\":2}]}",
            "{\"id\":1,\"tags\":[1,[2]]}",
            "{\"id\":1,\"tags\":{\"x\":2}}",
            "{\"first\":[1,2,3],\"last\":[4,{\"x\":5}]}",
        ] {
            with_record(text, |obj, template| {
                let mut output = String::from("unchanged");
                let capacity = output.capacity();
                assert!(!try_emit(obj, template, &mut output, 0));
                assert_eq!(output, "unchanged");
                assert_eq!(output.capacity(), capacity);
            });
        }
    }
}

#[test]
fn data_records_decline_cold_prototype_lookup_without_allocating() {
    unsafe {
        with_record("{\"id\":1,\"tags\":[1,2]}", |obj, template| {
            let proto = CACHED_OBJECT_PROTO_BITS.with(|c| c.replace(0));
            let state = OBJECT_PROTO_TOJSON_STATE
                .with(|c| c.replace(super::super::stringify_tojson_probe::PROTO_TOJSON_DIRTY));
            let before = crate::arena::arena_total_bytes();
            let roots = crate::gc::RuntimeHandleScope::active_len_for_tests();
            let mut output = String::from("unchanged");
            let emitted = try_emit(obj, template, &mut output, 0);
            CACHED_OBJECT_PROTO_BITS.with(|c| c.set(proto));
            OBJECT_PROTO_TOJSON_STATE.with(|c| c.set(state));
            assert!(!emitted);
            assert_eq!(output, "unchanged");
            assert_eq!(crate::arena::arena_total_bytes(), before);
            assert_eq!(crate::gc::RuntimeHandleScope::active_len_for_tests(), roots);
        });
    }
}

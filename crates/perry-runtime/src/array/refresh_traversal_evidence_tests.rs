//! A lazy JSON array materialized whole by compiled code refreshing its local
//! head is traversal evidence, exactly once per array, and a plain array is
//! never evidence. Without this the element-shape loop clone (#10171) served
//! every scan loop through its preheader and traversal feedback never learned
//! that the tape was being wasted.

fn lazy_array_box(bytes: &[u8], len: u32) -> f64 {
    let tape = crate::json_tape::build_tape(bytes).unwrap();
    let text = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let lazy = unsafe { crate::json_tape::alloc_lazy_array(&tape.entries, 0, len, text) };
    let header = unsafe { crate::value::addr_class::try_read_gc_header(lazy as usize) }.unwrap();
    assert_eq!(
        header.obj_type,
        crate::gc::GC_TYPE_LAZY_ARRAY,
        "fixture must be a real lazy array or the assertions below are vacuous"
    );
    crate::value::js_nanbox_pointer(lazy as i64)
}

#[test]
fn compiled_materialization_of_a_lazy_array_counts_once_and_a_plain_array_never() {
    crate::json::traversal_feedback::reset_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();

    let mut plain = crate::array::js_array_alloc(2);
    plain = crate::array::js_array_push_f64(plain, 1.0);
    let plain_box = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(plain as i64));
    let _ = crate::array::header::js_array_refresh_local_head(plain_box.get_nanbox_f64());
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        0,
        "a plain array is not traversal evidence"
    );

    let lazy = scope.root_nanbox_f64(lazy_array_box(br#"[{"id":1},{"id":2},{"id":3}]"#, 3));
    let first = crate::array::header::js_array_refresh_local_head(lazy.get_nanbox_f64());
    assert_ne!(
        first.to_bits(),
        lazy.get_nanbox_f64().to_bits(),
        "the head was refreshed"
    );
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        2,
        "materializing a lazy array whole is one flip's worth of evidence"
    );

    let _ = crate::array::header::js_array_refresh_local_head(lazy.get_nanbox_f64());
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        2,
        "an already materialized lazy array is not evidence again"
    );
    crate::json::traversal_feedback::reset_for_tests();
}

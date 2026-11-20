//! Refreshing a compiled local never materializes a lazy JSON array (its own
//! read policy decides; the policy's scan flip is the traversal evidence), and
//! follows the materialized head once the policy installed one. A plain array
//! is never evidence.

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
fn refresh_follows_the_lazy_policy_and_is_never_evidence_itself() {
    crate::json::traversal_feedback::reset_for_tests();
    let scope = crate::gc::RuntimeHandleScope::new();

    let mut plain = crate::array::js_array_alloc(2);
    plain = crate::array::js_array_push_f64(plain, 1.0);
    let plain_box = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(plain as i64));
    let _ = crate::array::js_array_refresh_local_head(plain_box.get_nanbox_f64());
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        0,
        "a plain array is not traversal evidence"
    );

    let lazy = scope.root_nanbox_f64(lazy_array_box(br#"[{"id":1},{"id":2},{"id":3}]"#, 3));
    let first = crate::array::js_array_refresh_local_head(lazy.get_nanbox_f64());
    assert_eq!(
        first.to_bits(),
        lazy.get_nanbox_f64().to_bits(),
        "refreshing an unmaterialized lazy array keeps it lazy: its own read policy decides"
    );
    let raw = (lazy.get_nanbox_f64().to_bits() & crate::value::POINTER_MASK) as usize;
    assert!(
        unsafe { crate::json_tape::is_lazy_array(raw as *const crate::json_tape::LazyArrayHeader) },
        "a refresh must not materialize"
    );
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        0,
        "a refresh that materialized nothing is not evidence"
    );

    let materialized = unsafe {
        crate::json_hooks::force_materialize_lazy(raw as *mut crate::json_tape::LazyArrayHeader)
    };
    let after = crate::array::js_array_refresh_local_head(lazy.get_nanbox_f64());
    assert_eq!(
        after.to_bits(),
        crate::value::js_nanbox_pointer(materialized as i64).to_bits(),
        "once materialized, the refresh follows the installed head"
    );
    assert_eq!(
        crate::json::traversal_feedback::score_for_tests(),
        0,
        "following an installed head is not evidence again"
    );
    crate::json::traversal_feedback::reset_for_tests();
}

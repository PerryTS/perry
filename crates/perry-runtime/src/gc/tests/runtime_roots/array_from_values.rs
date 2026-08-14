use super::*;

/// #5391 follow-up: the outlined array-literal builder receives values in a
/// compiler stack buffer, then allocates the destination. The buffer is not a
/// GC root, so the runtime helper must root its inputs before that allocation.
#[test]
fn outlined_array_literal_roots_closure_elements_across_destination_allocation() {
    let _legacy_pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    register_runtime_handle_root_scanner_for_tests();

    extern "C" fn element_closure(_closure: *const crate::closure::ClosureHeader) -> f64 {
        37.0
    }

    let closure = crate::closure::js_closure_alloc(element_closure as *const u8, 0);
    let closure_before = closure as usize;
    assert!(crate::arena::pointer_in_nursery(closure_before));
    let values = [1.0, 2.0, crate::value::js_nanbox_pointer(closure as i64)];

    crate::array::test_force_array_from_values_gc();
    let array = crate::array::js_array_from_values(values.as_ptr(), values.len() as u32);

    let element = crate::array::js_array_get_f64_unchecked(array, 2);
    let closure_after = crate::value::js_nanbox_get_pointer(element) as usize;
    assert_ne!(
        closure_after, closure_before,
        "the closure must move so the test proves the stack-buffer window"
    );
    assert_eq!(
        crate::closure::js_closure_call0(closure_after as *const crate::closure::ClosureHeader),
        37.0
    );
}

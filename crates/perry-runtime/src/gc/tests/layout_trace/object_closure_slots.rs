use super::*;

extern "C" fn layout_mask_test_closure(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    0.0
}

#[test]
fn test_trace_closure_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::closure::js_closure_alloc(crate::fn_info!(layout_mask_test_closure, 0), 8);
    crate::closure::js_closure_set_capture_f64(numeric, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(numeric, 1, 2.0);
    crate::closure::js_closure_set_capture_ptr(numeric, 2, 7);
    assert_eq!(test_layout_pointer_slot_count(numeric as usize, 8), Some(0));
    assert_eq!(test_heap_child_slot_count(numeric as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (numeric as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 0);
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let mixed = crate::closure::js_closure_alloc(crate::fn_info!(layout_mask_test_closure, 0), 8);
    crate::closure::js_closure_set_capture_f64(mixed, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(
        mixed,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    crate::closure::js_closure_set_capture_ptr(mixed, 2, 7);
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 8), Some(1));

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

// Repsel Phase 4b.2 — an INT32-boxed numeric value reaching a raw-f64-masked
// object slot through the runtime store choke point must be canonicalized to
// raw f64 instead of permanently poisoning the object's typed layout.
//
// `layout_note_slot` treats any non-raw-f64 bit pattern landing in a raw-f64
// slot as a representation change and calls `layout_set_typed_unknown`, which
// evicts the `TypedLayoutDescriptor` one-way, per object. INT32 boxes genuinely
// reach object fields from FFI / native modules (sqlite row columns, `v8`
// deserialization), so one FFI integer used to cost that object its typed fast
// path forever. Codegen's guarded class-field store already canonicalized
// inline behind its plain-finite check; `runtime_store_jsvalue_slot` wrote the
// bits verbatim.

#[test]
fn test_int32_store_without_typed_descriptor_is_left_verbatim() {
    let _guard = GcTestIsolationGuard::new();
    let (obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = 0.0f64.to_bits();
    }
    assert!(
        !layout_has_typed_descriptor(obj as usize),
        "test setup: no descriptor installed"
    );

    let int32_bits = crate::value::INT32_TAG | 5u64;
    runtime_store_jsvalue_slot(obj as usize, fields as usize, 0, int32_bits);

    assert_eq!(
        unsafe { std::ptr::read(fields as *const u64) },
        int32_bits,
        "with no intact descriptor there is no raw-f64 contract to uphold — bits stay verbatim"
    );
}

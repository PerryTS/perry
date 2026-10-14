use super::*;

#[test]
fn builtin_constructor_header_fact_survives_real_evacuation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for declaration in crate::object::GLOBAL_THIS_BUILTIN_CONSTRUCTORS {
        let closure = crate::object::alloc_builtin_constructor(declaration.identity);
        let before = unsafe { (*closure).capture_count };
        assert_eq!(crate::closure::real_capture_count(before), 1);
        js_shadow_slot_set(0, ptr_bits(closure as usize));

        let trace = collect_minor_trace(GcTriggerKind::Direct);
        let after_addr = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
        let after = after_addr as *const crate::closure::ClosureHeader;
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_ne!(
            after_addr, closure as usize,
            "constructor must actually move"
        );
        assert!(trace.copying_nursery.copied_objects > 0);
        assert_eq!(unsafe { (*after).capture_count }, before);
        assert_eq!(
            crate::object::constructor_identity(f64::from_bits(js_shadow_slot_get(0))),
            Some(declaration.identity)
        );
        assert_eq!(
            crate::closure::js_closure_get_capture_bits(after, 0),
            crate::JSValue::int32(declaration.identity as i32).bits()
        );
    }
}

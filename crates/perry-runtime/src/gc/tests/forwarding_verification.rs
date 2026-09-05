use super::super::*;
use super::support::*;

// Solid's effect.sources array grows after promotion. Its owning computation
// retains the old array address, which remains a supported growth alias.
#[test]
fn copying_verifier_accepts_retained_array_growth_alias_in_old_field() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _verify_guard = VerifyEvacuationTestGuard::on();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (stub, _) = unsafe { alloc_old_test_array(1) };
    let (holder, field) = unsafe { alloc_old_test_object(1) };
    unsafe {
        layout_init_pointer_free(stub as *mut u8);
        layout_init_pointer_free(holder as *mut u8);
        crate::object::store_object_field_slot(holder, 0, ptr_bits(stub as usize));
    }
    let grown = crate::array::js_array_push_f64(stub, 42.0);
    assert_ne!(stub, grown);
    assert!(crate::arena::pointer_in_old_gen(stub as usize));
    assert!(crate::arena::pointer_in_old_gen(grown as usize));
    js_shadow_slot_set(0, ptr_bits(holder as usize));
    let young = young_leaf();
    js_shadow_slot_set(1, ptr_bits(young));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(trace.phase_us.contains_key("evacuation_verify"));
    assert_ne!(js_shadow_slot_get(1), ptr_bits(young));
    assert_eq!(unsafe { *field }, ptr_bits(stub as usize));
    assert_eq!(crate::array::js_array_get_f64(stub, 1), 42.0);
}

#[test]
fn copying_verifier_accepts_retained_growth_chains_across_root_formats() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (stub, _) = unsafe { alloc_old_test_array(1) };
    let next = crate::array::js_array_grow(stub, 2);
    let target = crate::array::js_array_grow(next, 4);
    assert_ne!(stub, next);
    assert_ne!(next, target);
    let valid_ptrs = build_valid_pointer_set();
    let verifier = EvacuationVerifier::copying_minor(&valid_ptrs);
    let bits = ptr_bits(stub as usize);

    // The ordinary rewrite and non-copying verifier still canonicalize every
    // hop, including old arrays moved during old-page evacuation.
    assert_eq!(
        try_rewrite_value(bits, &valid_ptrs),
        Some(ptr_bits(target as usize))
    );
    assert_eq!(
        EvacuationVerifier::all_forwarded(&valid_ptrs).stale_value(bits),
        Some(ptr_bits(target as usize))
    );
    let mut visitor = RuntimeRootVisitor::for_verify(verifier, "retained growth root");
    assert_eq!(visitor.visit_nanbox_bits(bits), None);
    assert_eq!(visitor.visit_heap_word_bits(stub as usize as u64), None);
    assert_eq!(
        visitor.visit_tagged_raw_addr(stub as usize, POINTER_TAG),
        None
    );
    assert_eq!(visitor.visit_metadata_raw_addr(stub as usize), None);
    verify_copy_only_scanner_bits(bits, verifier, "retained copy-only root");
    let mut context = verifier;
    perry_ffi_verify_root(
        f64::from_bits(bits),
        &mut context as *mut EvacuationVerifier<'_> as *mut c_void,
    );
}

#[test]
fn copying_verifier_rejects_from_space_hops_even_through_retained_stubs() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (stub, _) = unsafe { alloc_old_test_array(1) };
    let from_space = crate::array::js_array_alloc(1);
    let (target, _) = unsafe { alloc_old_test_array(1) };
    assert!(super::super::fromspace_scan::is_from_space(
        crate::arena::classify_heap_space(from_space as usize)
    ));
    unsafe {
        // Sabotage: array growth forbids an old -> young forwarding edge.
        // The verifier must still reject it if it somehow occurs, even when
        // the first hop is a retained old array and only the second is stale.
        set_forwarding_address(
            header_from_user_ptr(stub as *const u8),
            from_space as *mut u8,
        );
    }
    let valid_ptrs = build_valid_pointer_set();
    let verifier = EvacuationVerifier::copying_minor(&valid_ptrs);
    assert_eq!(
        verifier.stale_raw_addr(stub as usize),
        Some(from_space as usize),
        "a retained stub must not reference even an unforwarded young array"
    );
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(from_space as *const u8),
            target as *mut u8,
        );
    }
    for source in [stub, from_space] {
        let bits = ptr_bits(source as usize);
        assert_eq!(
            verifier.stale_raw_addr(source as usize),
            Some(target as usize)
        );
        assert_eq!(
            verifier.stale_value(source as usize as u64),
            Some(target as usize as u64)
        );
        assert_eq!(
            verifier.stale_nanboxed_value(bits),
            Some(ptr_bits(target as usize))
        );
        for format in 0..4 {
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut visitor = RuntimeRootVisitor::for_verify(verifier, "from-space control");
                match format {
                    0 => {
                        visitor.visit_nanbox_bits(bits);
                    }
                    1 => {
                        visitor.visit_heap_word_bits(source as usize as u64);
                    }
                    2 => {
                        visitor.visit_tagged_raw_addr(source as usize, POINTER_TAG);
                    }
                    _ => {
                        visitor.visit_metadata_raw_addr(source as usize);
                    }
                }
            }));
            assert!(
                failure.is_err(),
                "root format {format} must reject a stale hop"
            );
        }
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            verify_slot(&bits, verifier, "from-space heap control");
        }));
        assert!(failure.is_err());
    }
    // Leave a valid retained alias for subsequent tests' heap walks.
    unsafe {
        set_forwarding_address(header_from_user_ptr(stub as *const u8), target as *mut u8);
    }
}

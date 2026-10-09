//! Producer-contract witnesses. Conservative rescues are disabled and the
//! controls deliberately corrupt the slot's tag or leave its old address.
use super::super::*;
use super::support::*;

#[test]
fn numeric_address_bits_are_not_precise_roots() {
    let _guard = GcTestIsolationGuard::new();
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = young_leaf();
    let valid = build_valid_pointer_set();
    for kind in [
        MutableRootSlotKind::GlobalRoot,
        MutableRootSlotKind::NativeStack,
    ] {
        let mut bits = target as u64; // A legitimate subnormal JS number.
        let slot = MutableRootSlot {
            kind,
            ptr: &mut bits,
        };
        assert_eq!(slot.pointer_word(bits), None);
        mark_mutable_slot(slot, bits, &valid);
        assert_eq!(
            unsafe { (*header_from_user_ptr(target as *const u8)).gc_flags & GC_FLAG_MARKED },
            0
        );
        assert!(slot.pointer_word(ptr_bits(target)).is_some());
    }
    let mut visitor = RuntimeRootVisitor::for_mark(&valid);
    visitor.visit_nanbox_u64_slot(&mut (target as u64));
    assert_eq!(
        unsafe { (*header_from_user_ptr(target as *const u8)).gc_flags & GC_FLAG_MARKED },
        0
    );
    let mut raw = target;
    visitor.visit_usize_slot(&mut raw);
    assert_ne!(
        unsafe { (*header_from_user_ptr(target as *const u8)).gc_flags & GC_FLAG_MARKED },
        0
    );
    clear_marks();
    clear_mark_seeds();
}

fn global_liveness_witness(mistype: bool) {
    let _pacing = crate::gc::policy::force_legacy_gc_pacing();
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let dead = allocate_dead_malloc_churn_headers(8);
    let user = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(user);
    }
    let mut root = if mistype {
        user as u64
    } else {
        ptr_bits(user as usize)
    };
    js_gc_register_global_root(&mut root as *mut u64 as i64);
    GC_NEXT_MALLOC_TRIGGER.with(|v| v.set(malloc_object_count().saturating_sub(1)));
    gc_check_trigger();
    assert_eq!(
        complete_budgeted_gc_cycle().status,
        JS_GC_STEP_STATUS_COMPLETED
    );
    assert_eq!(
        tracked_malloc_headers_matching(&dead),
        0,
        "the control must actually sweep"
    );
    assert!(
        malloc_user_ptr_tracked((root & POINTER_MASK) as *mut u8),
        "rootcls liveness contract: the producer must encode its pointer"
    );
}

#[test]
fn mistyped_nonpointer_sabotage_fails_the_liveness_witness() {
    global_liveness_witness(false);
    let failure = std::panic::catch_unwind(|| global_liveness_witness(true));
    let payload = failure.expect_err("mistyped root must fail the survival assertion");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        message.contains("rootcls liveness contract"),
        "wrong failure: {message}"
    );
}

#[test]
fn stale_typed_root_sabotage_fails_evacuation_verification() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old = young_leaf();
    let moved = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let valid = build_valid_pointer_set();
    unsafe {
        set_forwarding_address(header_from_user_ptr(old as *const u8), moved as *mut u8);
    }
    let mut root = ptr_bits(old);
    js_gc_register_global_root(&mut root as *mut u64 as i64);
    let verifier = EvacuationVerifier::all_forwarded(&valid);
    let stale = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        verify_mutable_root_slots(verifier);
    }));
    assert!(
        stale.is_err(),
        "an omitted typed-root rewrite must be caught"
    );
    rewrite_mutable_root_slots(&valid, None);
    assert_eq!(root, ptr_bits(moved));
    verify_mutable_root_slots(verifier);
    unsafe {
        (*header_from_user_ptr(old as *const u8)).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

#[test]
fn immortal_leaf_header_is_authoritative_without_a_census_entry() {
    let _guard = GcTestIsolationGuard::new();
    let symbol = crate::symbol::well_known_symbol("iterator") as usize;
    let valid = build_valid_pointer_set();
    assert!(
        !valid.contains(&symbol),
        "the witness must be outside the census"
    );
    let flags = unsafe { (*header_from_user_ptr(symbol as *const u8)).gc_flags };
    assert_eq!(
        flags & (GC_FLAG_PINNED | GC_FLAG_MARKED),
        GC_FLAG_PINNED | GC_FLAG_MARKED
    );
    assert!(!mark_precise_root(PreciseRoot::GcPointer(symbol), &valid));
    assert!(!mark_precise_root(
        PreciseRoot::JSValue(ptr_bits(symbol)),
        &valid
    ));
}

#[test]
fn empty_pointer_payloads_are_not_root_edges() {
    let _guard = GcTestIsolationGuard::new();
    let valid = build_valid_pointer_set();
    assert!(!mark_precise_root(PreciseRoot::GcPointer(0), &valid));
    for tag in [POINTER_TAG, STRING_TAG, BIGINT_TAG] {
        assert_eq!(decode_nanboxed_root_word(tag), None);
        assert!(!mark_precise_root(PreciseRoot::JSValue(tag), &valid));
        let mut root = tag;
        let mut visitor = RuntimeRootVisitor::for_mark(&valid);
        visitor.visit_nanbox_u64_slot(&mut root);
        assert_eq!(root, tag);
    }
}

#[test]
fn source_encoding_controls_forwarding_and_verification() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old = young_leaf();
    let moved = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let valid = build_valid_pointer_set();
    unsafe {
        set_forwarding_address(header_from_user_ptr(old as *const u8), moved as *mut u8);
    }
    let verifier = EvacuationVerifier::all_forwarded(&valid);
    for kind in [
        MutableRootSlotKind::NativeStack,
        MutableRootSlotKind::GlobalRoot,
    ] {
        let mut numeric = old as u64;
        let slot = MutableRootSlot {
            kind,
            ptr: &mut numeric,
        };
        assert_eq!(slot.pointer_word(numeric), None);
        assert_eq!(try_rewrite_nanboxed_value(numeric, &valid), None);
        assert_eq!(verifier.stale_nanboxed_value(numeric), None);
        assert!(slot.pointer_word(ptr_bits(old)).is_some());
    }
    assert_eq!(
        try_rewrite_nanboxed_value(ptr_bits(old), &valid),
        Some(ptr_bits(moved))
    );
    assert_eq!(
        verifier.stale_nanboxed_value(ptr_bits(old)),
        Some(ptr_bits(moved))
    );
    let handle = POINTER_TAG | crate::value::addr_class::FETCH_HANDLE_BAND_START as u64;
    assert_eq!(decode_nanboxed_root_word(handle), None);
    assert_eq!(try_rewrite_nanboxed_value(handle, &valid), None);
    assert_eq!(verifier.stale_nanboxed_value(handle), None);
    unsafe {
        (*header_from_user_ptr(old as *const u8)).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

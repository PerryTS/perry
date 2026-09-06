//! Permanent old array-growth aliases must remain distinct from stale
//! evacuation references. No JSON calls are involved in these fixtures.
use super::super::*;
use super::support::*;

unsafe fn old_array() -> *mut crate::array::ArrayHeader {
    let array = crate::arena::arena_alloc_gc_old_born_tenured(24, 8, GC_TYPE_ARRAY)
        as *mut crate::array::ArrayHeader;
    (*array).length = 0;
    (*array).capacity = 2;
    let slots = (array as *mut u8).add(8) as *mut u64;
    slots.write(crate::value::TAG_HOLE);
    slots.add(1).write(crate::value::TAG_HOLE);
    layout_init_pointer_free(array.cast());
    crate::array::set_array_numeric_layout(array, crate::array::NumericArrayLayout::RawF64);
    array
}

unsafe fn assert_rejected_alias(address: usize) {
    let valid = build_valid_pointer_set();
    for bits in [address as u64, ptr_bits(address)] {
        assert!(
            try_rewrite_value(bits, &valid).is_some(),
            "the negative control must really forward"
        );
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            verify_slot(&bits, &valid, "unstable forwarding negative control");
        }));
        assert!(
            failure.is_err(),
            "unstable forwarding must still fail verification"
        );
    }
}

#[test]
fn array_growth_verifier_still_rejects_old_array_evacuation_stubs() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let source = old_array();
        let target = old_array();
        set_forwarding_address(
            header_from_user_ptr(source.cast()) as *mut GcHeader,
            target.cast(),
        );
        assert_rejected_alias(source as usize);
    }
}

#[test]
fn array_growth_verifier_still_rejects_evacuation_inside_growth_chain() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let source = old_array();
        let grown = crate::array::js_array_grow(source, 32);
        let moved = old_array();
        set_forwarding_address(
            header_from_user_ptr(grown.cast()) as *mut GcHeader,
            moved.cast(),
        );
        assert_rejected_alias(source as usize);
    }
}

#[test]
fn array_growth_verifier_still_rejects_unrewritten_young_growth_aliases() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let source = crate::array::js_array_alloc(2);
    let grown = crate::array::js_array_grow(source, 32);
    assert_ne!(source, grown);
    assert!(crate::arena::pointer_in_nursery(source as usize));
    unsafe {
        assert_rejected_alias(source as usize);
    }
}

#[test]
fn array_growth_verifier_agrees_across_registered_copy_only_and_shadow_roots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let source = old_array();
        let grown = crate::array::js_array_grow(source, 32);
        assert_ne!(source, grown);
        let valid = build_valid_pointer_set();
        let mut bits = ptr_bits(source as usize);
        let mut address = source as usize;
        let mut visitor = RuntimeRootVisitor::for_verify(&valid, "registered growth alias");
        visitor.visit_nanbox_u64_slot(&mut bits);
        visitor.visit_usize_slot(&mut address);
        verify_copy_only_scanner_bits(bits, &valid, "copy-only growth alias");
        js_shadow_slot_set(0, bits);
        verify_mutable_root_slots(&valid);
        assert_eq!(
            bits,
            ptr_bits(source as usize),
            "verification must not mutate the alias"
        );
        assert_eq!(address, source as usize);
    }
}

#[test]
fn array_growth_verifier_still_requires_canonical_metadata_keys() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let source = old_array();
        let grown = crate::array::js_array_grow(source, 32);
        let valid = build_valid_pointer_set();
        let mut key = source as usize;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RuntimeRootVisitor::for_verify(&valid, "metadata growth owner")
                .visit_metadata_usize_slot(&mut key);
        }));
        assert!(failure.is_err(), "metadata must still require canonical ownership");
        RuntimeRootVisitor::for_rewrite(&valid).visit_metadata_usize_slot(&mut key);
        assert_eq!(key, grown as usize);
        RuntimeRootVisitor::for_verify(&valid, "rewritten metadata owner")
            .visit_metadata_usize_slot(&mut key);
    }
}

#[test]
fn array_growth_verifier_accepts_permanent_old_aliases() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    unsafe {
        let original = old_array();
        let mut grown = original;
        for index in 0..70 {
            grown = crate::array::js_array_push(grown, crate::JSValue::number(index as f64));
        }
        assert_ne!(original, grown);
        assert!(crate::arena::pointer_in_old_gen(original as usize));
        assert!(crate::arena::pointer_in_old_gen(grown as usize));
        let valid = build_valid_pointer_set();
        for bits in [original as u64, ptr_bits(original as usize)] {
            assert!(
                try_rewrite_value(bits, &valid).is_some(),
                "exercise a real forwarding chain"
            );
            verify_slot(&bits, &valid, "permanent array growth alias");
        }
        assert_eq!(crate::array::js_array_get(grown, 69).as_number(), 69.0);
    }
}

#[test]
fn array_growth_verifier_keeps_old_owner_valid_while_unrelated_young_value_moves() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    let _verify = crate::gc::knob_overrides::VerifyEvacuationTestGuard::on();
    unsafe {
        let original = old_array();
        let (owner, _) = alloc_old_test_object(1);
        let pointer = crate::object::store_object_field_slot_layout_deferred(
            owner,
            0,
            ptr_bits(original as usize),
        );
        layout_finish_deferred_boxed_object(owner as usize, pointer);
        js_shadow_slot_set(0, ptr_bits(owner as usize));
        let young = young_leaf();
        js_shadow_slot_set(1, ptr_bits(young));
        for index in 0..70 {
            crate::array::js_array_push(original, crate::JSValue::number(index as f64));
        }
        let before = gc_collection_count();
        let _ = gc_collect_minor_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
        assert!(gc_collection_count() > before);
        assert_ne!(
            js_shadow_slot_get(1),
            ptr_bits(young),
            "the minor must actually move its young subject"
        );
        let owner = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
        let field = (owner as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *const u64;
        let alias = (*field & POINTER_MASK) as *const crate::array::ArrayHeader;
        assert_eq!(crate::array::js_array_length(alias), 70);
        assert_eq!(crate::array::js_array_get(alias, 69).as_number(), 69.0);
    }
}

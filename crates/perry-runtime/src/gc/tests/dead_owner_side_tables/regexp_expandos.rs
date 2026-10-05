//! RegExp user properties are ordinary slots, with no address-keyed expando state.
//! Copying collection must reclaim dead instances and their private data edges,
//! and preserve live properties through the ordinary object visitor.

use super::*;

/// A production-constructed RegExp, unrooted once the caller's scope ends.
fn construct_regexp(pattern: &str) -> usize {
    let scope = RuntimeHandleScope::new();
    let source = scope.root_string_ptr(crate::string::js_string_from_bytes(
        pattern.as_ptr(),
        pattern.len() as u32,
    ));
    let flags = scope.root_string_ptr(crate::string::js_string_from_bytes(b"g".as_ptr(), 1));
    let re = source.with_const_ptr(|source| {
        flags.with_const_ptr(|flags| crate::regex::js_regexp_new(source, flags))
    });
    assert!(
        crate::regex::regexp_data_of(crate::value::js_nanbox_pointer((re as usize) as i64))
            .is_some(),
        "test premise: the header identifies as a RegExp"
    );
    re as usize
}

#[test]
fn test_unrooted_regexp_instance_and_data_reclaimed_by_copying_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _scan = ConservativeScanDisabledGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let addr = construct_regexp("dies-before-the-full-trace");
    crate::object::js_object_set_field_by_name(
        addr as *mut crate::ObjectHeader,
        crate::string::intern_ascii_literal(b"tag"),
        f64::from_bits(crate::value::JSValue::int32(7).bits()),
    );
    assert!(!crate::object::exotic_expando::test_exotic_expando_entry_exists(addr));
    let data = crate::regex::regexp_data_of(crate::value::js_nanbox_pointer(addr as i64)).unwrap()
        as usize;
    // The construction cache holds the compiled program, not the header, but
    // evict it anyway so nothing the fixture made is reachable.
    crate::regex::perex_cache::clear_for_tests();

    // No roots: both cells must be collectible nursery allocations. A
    // persisting ordinary-object arena block is not evidence of liveness.
    assert!(crate::arena::pointer_in_nursery(addr));
    assert!(crate::arena::pointer_in_nursery(data));
    let cycles = copying_minor_cycles();
    gc_collect_minor();
    assert!(copying_minor_cycles() > cycles);

    let live = build_valid_pointer_set();
    assert!(
        !live.contains(&addr),
        "dead ordinary RegExp must be reclaimed"
    );
    assert!(
        !live.contains(&data),
        "evicted unrooted RegExpData must be reclaimed"
    );
}

#[test]
fn test_live_regexp_expando_survives_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let addr = construct_regexp("stays-live-across-the-full-trace");
    crate::object::js_object_set_field_by_name(
        addr as *mut crate::ObjectHeader,
        crate::string::intern_ascii_literal(b"tag"),
        f64::from_bits(crate::value::JSValue::int32(42).bits()),
    );
    js_shadow_slot_set(0, ptr_bits(addr));

    full_gc();

    // Full mark-sweep is non-moving: the rooted RegExp keeps its address.
    assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, addr);
    assert!(crate::regex::regexp_data_of(crate::value::js_nanbox_pointer((addr) as i64)).is_some());
    assert_eq!(
        Some(
            crate::object::js_object_get_field_by_name(
                addr as *const crate::ObjectHeader,
                crate::string::intern_ascii_literal(b"tag")
            )
            .bits()
        ),
        Some(crate::value::JSValue::int32(42).bits()),
        "a live RegExp's expando must survive a full GC"
    );
    js_shadow_slot_set(0, 0);
}

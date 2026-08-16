use super::super::*;
use super::support::*;

fn full_collect() {
    let trigger = GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
    };
    let _ = GcCycleState::new_full(trigger).run_to_completion();
}

fn alloc_proxy_endpoint() -> (*mut u8, f64) {
    let ptr = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(ptr);
    }
    (ptr, f64::from_bits(ptr_bits(ptr as usize)))
}

#[test]
fn full_trace_prunes_an_unobserved_proxy_and_its_registry_only_graph() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let reclaimed_before = crate::proxy::test_proxy_gc_reclaimed_total();
    let (target_ptr, target) = alloc_proxy_endpoint();
    let (handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy::js_proxy_new(target, handler);

    full_collect();

    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
    assert_eq!(
        crate::proxy::test_proxy_gc_reclaimed_total(),
        reclaimed_before + 1,
        "the prune counter proves the full-trace reclamation path ran"
    );
    assert!(!malloc_user_ptr_tracked(target_ptr));
    assert!(!malloc_user_ptr_tracked(handler_ptr));
}

#[test]
fn a_minor_trace_keeps_the_registry_strong_and_never_prunes_proxy_ids() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::proxy::scan_proxy_roots_mut);
    let (target_ptr, target) = alloc_proxy_endpoint();
    let (handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy::js_proxy_new(target, handler);

    let _ = gc_collect_minor();

    assert!(crate::proxy::test_proxy_slot_is_live(proxy));
    assert!(malloc_user_ptr_tracked(target_ptr));
    assert!(malloc_user_ptr_tracked(handler_ptr));

    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
}

#[test]
fn a_shadow_root_keeps_a_proxy_and_its_graph_until_the_next_full_trace() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (target_ptr, target) = alloc_proxy_endpoint();
    let (handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy::js_proxy_new(target, handler);
    js_shadow_slot_set(0, proxy.to_bits());

    full_collect();

    assert!(crate::proxy::test_proxy_slot_is_live(proxy));
    assert!(malloc_user_ptr_tracked(target_ptr));
    assert!(malloc_user_ptr_tracked(handler_ptr));

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
    assert!(!malloc_user_ptr_tracked(target_ptr));
    assert!(!malloc_user_ptr_tracked(handler_ptr));
}

#[test]
fn a_proxy_in_a_pointer_free_heap_range_is_still_observed() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (_target_ptr, target) = alloc_proxy_endpoint();
    let (_handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy::js_proxy_new(target, handler);
    let (owner, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = proxy.to_bits();
        layout_init_pointer_free(owner as *mut u8);
    }
    js_shadow_slot_set(0, ptr_bits(owner as usize));

    full_collect();

    assert!(
        crate::proxy::test_proxy_slot_is_live(proxy),
        "the full trace must inspect proxy-band values even in a range whose heap layout is pointer-free"
    );

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
}

#[test]
fn observing_an_outer_proxy_recursively_keeps_its_proxy_target_live() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (_inner_target_ptr, inner_target) = alloc_proxy_endpoint();
    let (_handler_ptr, handler) = alloc_proxy_endpoint();
    let inner = crate::proxy::js_proxy_new(inner_target, handler);
    let outer = crate::proxy::js_proxy_new(inner, handler);
    js_shadow_slot_set(0, outer.to_bits());

    full_collect();

    assert!(crate::proxy::test_proxy_slot_is_live(outer));
    assert!(crate::proxy::test_proxy_slot_is_live(inner));

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(outer));
    assert!(!crate::proxy::test_proxy_slot_is_live(inner));
}

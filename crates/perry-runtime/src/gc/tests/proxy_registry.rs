use super::super::*;
use super::support::*;

fn full_collect() {
    let trigger = GcTriggerSnapshot {
        kind: GcTriggerKind::Manual,
        steps_before: Some(GcStepSnapshot::current()),
        origin: None,
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
fn unreachable_proxy_cell_and_its_owned_graph_die_in_full_trace() {
    let _guard = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (target_ptr, target) = alloc_proxy_endpoint();
    let (handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy_hooks::proxy_new(target, handler);

    full_collect();

    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
    assert!(!malloc_user_ptr_tracked(target_ptr));
    assert!(!malloc_user_ptr_tracked(handler_ptr));
}

#[test]
fn a_minor_keeps_the_old_proxy_until_full_collection() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (target_ptr, target) = alloc_proxy_endpoint();
    let (handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy_hooks::proxy_new(target, handler);

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
    let proxy = crate::proxy_hooks::proxy_new(target, handler);
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
fn a_proxy_in_an_object_field_is_traced_as_a_heap_edge() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let (_target_ptr, target) = alloc_proxy_endpoint();
    let (_handler_ptr, handler) = alloc_proxy_endpoint();
    let proxy = crate::proxy_hooks::proxy_new(target, handler);
    let (owner, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = proxy.to_bits();
        layout_init_all_pointer_slots(owner as *mut u8);
    }
    js_shadow_slot_set(0, ptr_bits(owner as usize));

    full_collect();

    assert!(
        crate::proxy::test_proxy_slot_is_live(proxy),
        "the ordinary object field must retain the proxy GC cell"
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
    let inner = crate::proxy_hooks::proxy_new(inner_target, handler);
    let outer = crate::proxy_hooks::proxy_new(inner, handler);
    js_shadow_slot_set(0, outer.to_bits());

    full_collect();

    assert!(crate::proxy::test_proxy_slot_is_live(outer));
    assert!(crate::proxy::test_proxy_slot_is_live(inner));

    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(outer));
    assert!(!crate::proxy::test_proxy_slot_is_live(inner));
}

#[test]
fn proxy_target_and_handler_copy_without_moving_proxy_identity() {
    check_owned_proxy(false);
}
#[test]
#[ignore = "negative control: clear the proxy target edge"]
fn negative_control_objtables_proxies() {
    check_owned_proxy(true);
}
fn check_owned_proxy(erase: bool) {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = ConservativeScanDisabledGuard::new();
    crate::weakref::test_support::clear_weak_holders();
    register_runtime_handle_root_scanner_for_tests();
    let target = crate::object::js_object_alloc(0, 1);
    let handler = crate::object::js_object_alloc(0, 0);
    {
        crate::object::js_object_set_field_f64(target, 0, 73.0);
    }
    let proxy = crate::proxy_hooks::proxy_new(
        f64::from_bits(ptr_bits(target as usize)),
        f64::from_bits(ptr_bits(handler as usize)),
    );
    let cell = crate::proxy_hooks::canonical_address(proxy).unwrap();
    js_shadow_slot_set(0, proxy.to_bits());
    let _ = gc_collect_minor();
    assert_eq!(js_shadow_slot_get(0), proxy.to_bits());
    unsafe {
        let state = &mut *(cell as *mut crate::proxy::ProxyEntry);
        assert_ne!(
            (state.target.to_bits() & POINTER_MASK) as usize,
            target as usize
        );
        assert_ne!(
            (state.handler.to_bits() & POINTER_MASK) as usize,
            handler as usize
        );
        let weak = crate::weakref::js_weakref_new(state.target);
        js_shadow_slot_set(0, proxy.to_bits());
        let scope = RuntimeHandleScope::new();
        let weak = scope.root_nanbox_f64(f64::from_bits(ptr_bits(weak as usize)));
        if erase {
            state.target = 0.0;
        }
        full_collect();
        assert_ne!(
            crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits(),
            crate::value::TAG_UNDEFINED,
            "only the proxy's target field keeps this object live"
        );
        assert_eq!(
            crate::object::js_object_get_field_f64(
                (state.target.to_bits() & POINTER_MASK) as *const crate::ObjectHeader,
                0
            ),
            73.0
        );
        crate::proxy_hooks::proxy_revoke(proxy);
        full_collect();
        assert_eq!(
            crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert!(state.revoked);
        assert_eq!(state.target.to_bits(), 0);
        assert_eq!(state.handler.to_bits(), 0);
    }
    js_shadow_slot_set(0, 0);
    full_collect();
    assert!(!crate::proxy::test_proxy_slot_is_live(proxy));
}
#[test]
fn ordinary_allocation_is_not_a_proxy_cell() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let object = crate::object::js_object_alloc(0, 0);
    let ordinary = f64::from_bits(ptr_bits(object as usize));
    assert!(crate::proxy_hooks::canonical_address(ordinary).is_none());
    assert!(!crate::value::addr_class::is_proxy_cell(object as usize));
    let target = crate::object::js_object_alloc(0, 0);
    let handler = crate::object::js_object_alloc(0, 0);
    let proxy = crate::proxy_hooks::proxy_new(
        f64::from_bits(ptr_bits(target as usize)),
        f64::from_bits(ptr_bits(handler as usize)),
    );
    // A Proxy is boxed like every object; its header kind is the brand.
    assert_eq!(
        proxy.to_bits() & crate::value::TAG_MASK,
        crate::value::POINTER_TAG
    );
    assert_eq!(proxy.to_bits() & 7, 0);
    let cell = crate::proxy_hooks::canonical_address(proxy).unwrap();
    assert!(crate::value::addr_class::is_proxy_cell(cell));
}

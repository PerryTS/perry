//! The native-this alias (#4973 `http.Server.call(this, …)`, #10454
//! `http.ServerResponse.call(this, req)`) lives on its object: it follows the
//! object through moving collections and dies with it. Nothing roots an
//! aliased object on the alias's behalf — fastify's `inject` aliases one
//! light-my-request `Response` per request, and a table that rooted each one
//! kept every response (and its payload) alive for the life of the process.
use super::super::*;
use super::support::*;
use crate::gc::{RuntimeHandle, RuntimeHandleScope};
use crate::object::native_this_alias::{alias_handle_for_object, register_this_to_handle_alias};
use crate::object::ObjectHeader;

// Test-only ids in the native handle band; nothing dispatches them here.
const HANDLE: i64 = 0xe1726;

fn handle_value(id: i64) -> f64 {
    crate::value::js_nanbox_pointer(id)
}

/// The guards below clear the scanner registry; restore the object model's.
fn register_object_model_scanners() {
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(crate::object::scan_object_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::scan_transition_cache_roots_mut);
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
}

fn boxed(h: &RuntimeHandle<'_>) -> f64 {
    h.with_mut_ptr(|o: *mut ObjectHeader| crate::value::js_nanbox_pointer(o as i64))
}

#[test]
fn the_alias_follows_its_object_through_minor_and_full_collections() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _no_stack = ConservativeScanDisabledGuard::new();
    register_object_model_scanners();
    let scope = RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let sibling = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    register_this_to_handle_alias(boxed(&obj), handle_value(HANDLE), true);
    let before = boxed(&obj).to_bits();
    assert!(
        crate::arena::pointer_in_nursery((before & crate::value::POINTER_MASK) as usize),
        "premise: the aliased object is young"
    );
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        trace.copying_nursery.copied_objects > 0,
        "premise: the minor copied"
    );
    let after = boxed(&obj);
    assert_ne!(
        after.to_bits(),
        before,
        "premise: the minor moved the object"
    );
    let (handle, composite) = alias_handle_for_object(after).expect("alias moved with the object");
    assert_eq!(handle.to_bits(), handle_value(HANDLE).to_bits());
    assert!(composite);
    js_gc_collect();
    // The raw-i64 receiver form reads the same alias.
    let raw = f64::from_bits(boxed(&obj).to_bits() & crate::value::POINTER_MASK);
    assert_eq!(
        alias_handle_for_object(raw).map(|(h, _)| h.to_bits()),
        Some(handle.to_bits())
    );
    // A re-construction re-aliases the same object.
    register_this_to_handle_alias(boxed(&obj), handle_value(HANDLE + 1), false);
    assert_eq!(
        alias_handle_for_object(boxed(&obj)).map(|(h, c)| (h.to_bits(), c)),
        Some((handle_value(HANDLE + 1).to_bits(), false))
    );
    assert!(alias_handle_for_object(boxed(&sibling)).is_none());
}

/// The leak's regression test: a churn of aliased objects that nothing else
/// references must all be collectable. A WeakRef to every one of them must
/// clear after a full collection.
#[test]
fn an_unreferenced_aliased_object_is_collected() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _no_stack = ConservativeScanDisabledGuard::new();
    register_object_model_scanners();
    let scope = RuntimeHandleScope::new();
    const CHURN: usize = 64;
    let mut holders = Vec::with_capacity(CHURN);
    for i in 0..CHURN {
        // Each target is rooted only inside its own scope, across the alias
        // registration (which allocates the meta record and may move it).
        let holder = {
            let inner = RuntimeHandleScope::new();
            let target = inner.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            register_this_to_handle_alias(
                boxed(&target),
                handle_value(HANDLE + i as i64),
                i % 2 == 0,
            );
            assert!(alias_handle_for_object(boxed(&target)).is_some());
            crate::weakref::js_weakref_new(boxed(&target))
        };
        holders.push(scope.root_nanbox_f64(crate::value::js_nanbox_pointer(holder as i64)));
    }
    // Control: a rooted aliased object survives with its alias, so the
    // collections below do see this scope's roots.
    let control = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    register_this_to_handle_alias(boxed(&control), handle_value(HANDLE - 1), false);
    let control_weak = crate::weakref::js_weakref_new(boxed(&control));
    let control_weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(control_weak as i64));
    collect_minor_trace(GcTriggerKind::Direct);
    js_gc_collect();
    js_gc_collect();
    assert_eq!(
        crate::weakref::js_weakref_deref(control_weak.get_nanbox_f64()).to_bits(),
        boxed(&control).to_bits(),
        "premise: a rooted aliased object survives"
    );
    assert!(alias_handle_for_object(boxed(&control)).is_some());
    let cleared = holders
        .iter()
        .filter(|h| {
            crate::weakref::js_weakref_deref(h.get_nanbox_f64()).to_bits()
                == crate::value::TAG_UNDEFINED
        })
        .count();
    assert_eq!(
        cleared, CHURN,
        "every unreferenced aliased object must be collected"
    );
}

/// A response alias owns its ordinary payload target through a traced edge.
/// The stream record can be installed on either side of alias construction.
#[test]
fn an_alias_traces_and_rewrites_an_ordinary_target_in_both_stream_orders() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _no_stack = ConservativeScanDisabledGuard::new();
    register_object_model_scanners();
    let scope = RuntimeHandleScope::new();
    for stream_first in [false, true] {
        let receiver = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
        if stream_first {
            crate::node_stream::js_node_stream_writable_subclass_init(boxed(&receiver), undefined);
        }
        let weak = {
            let inner = RuntimeHandleScope::new();
            let target = inner.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
            register_this_to_handle_alias(boxed(&receiver), boxed(&target), true);
            let weak = crate::weakref::js_weakref_new(boxed(&target));
            weak
        };
        let weak = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(weak as i64));
        if !stream_first {
            crate::node_stream::js_node_stream_writable_subclass_init(boxed(&receiver), undefined);
        }
        let before = alias_handle_for_object(boxed(&receiver))
            .unwrap()
            .0
            .to_bits();
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert!(trace.copying_nursery.copied_objects > 0);
        let target = crate::weakref::js_weakref_deref(weak.get_nanbox_f64());
        assert_ne!(target.to_bits(), crate::value::TAG_UNDEFINED);
        assert_ne!(target.to_bits(), before, "the target moved");
        assert_eq!(
            alias_handle_for_object(boxed(&receiver)).map(|(v, c)| (v.to_bits(), c)),
            Some((target.to_bits(), true))
        );
        js_gc_collect();
        assert_eq!(
            alias_handle_for_object(boxed(&receiver))
                .unwrap()
                .0
                .to_bits(),
            crate::weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits()
        );
    }
}

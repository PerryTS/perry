//! #9539 — thenable assimilation must hand back the wrapper promise's
//! POST-collection address.
//!
//! `assimilate_via_then_property` / `js_assimilate_thenable` allocate a wrapper
//! Promise, hand the user's `then` a pair of resolving closures that capture it,
//! and then run that `then`. The body is arbitrary user code, and with the
//! moving nursery a loop safepoint inside it evacuates the young generation.
//! The wrapper survives — the closures' capture words are pointer-bearing and
//! get rewritten — but the bare `*mut Promise` Rust local used to build the
//! return value is not a GC root, so the pre-fix code returned the address the
//! promise had *before* the callback collected. `util.callbackify` then
//! classified that retired from-space word as a Promise, rooted it and attached
//! reactions to it, and the exit-time microtask checkpoint faulted.

use super::*;

/// A native stand-in for a user `then(resolve, reject)` whose body allocates
/// enough to collect — the `churn()` loop of the end-to-end fixture. It roots
/// `resolve` the way a compiled JS frame does (its locals are precise roots),
/// so the wrapper promise stays reachable through the closure capture and is
/// RELOCATED rather than freed.
extern "C" fn test_thenable_then_force_minor_gc(
    _closure: *const crate::closure::ClosureHeader,
    resolve: f64,
    _reject: f64,
) -> f64 {
    let scope = RuntimeHandleScope::new();
    let resolve_handle = scope.root_nanbox_f64(resolve);
    let _ = crate::gc::gc_collect_minor();
    let args = [1.0f64];
    unsafe {
        crate::closure::js_native_call_value(
            resolve_handle.get_nanbox_f64(),
            args.as_ptr(),
            args.len(),
        );
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn test_assimilated_thenable_wrapper_survives_then_callback_copied_minor_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    activate_malloc_registry_for_tests();
    register_runtime_handle_root_scanner_for_tests();
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);
    crate::closure::js_register_closure_arity(test_thenable_then_force_minor_gc as *const u8, 2);

    let scope = RuntimeHandleScope::new();
    let then_closure =
        crate::closure::js_closure_alloc(test_thenable_then_force_minor_gc as *const u8, 0);
    let then_handle = scope.root_raw_mut_ptr(then_closure);

    // `{ then(resolve, reject) { … } }` — an object literal (class_id 0), so
    // assimilation takes the `then`-as-data-property path.
    let thenable_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 1));
    let key_handle =
        scope.root_string_ptr(crate::string::js_string_from_bytes(b"then".as_ptr(), 4));
    let then_boxed =
        then_handle.with_mut_ptr::<u8, _>(|then| f64::from_bits(ptr_bits(then as usize)));
    thenable_handle.with_mut_ptr(|thenable| {
        key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
            crate::object::js_object_set_field_by_name(thenable, key, then_boxed);
        })
    });

    let before = gc_collection_count();
    let assimilated = crate::promise::js_assimilate_thenable(
        thenable_handle
            .with_mut_ptr::<u8, _>(|thenable| f64::from_bits(ptr_bits(thenable as usize))),
    );
    assert!(
        gc_collection_count() > before,
        "the thenable's `then` body must force a copying minor GC while the wrapper is live"
    );

    let bits = assimilated.to_bits();
    assert_eq!(
        bits & TAG_MASK,
        POINTER_TAG,
        "assimilation must return a heap pointer"
    );
    let promise = (bits & POINTER_MASK) as *mut crate::promise::Promise;
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(promise as *const u8)).obj_type,
            GC_TYPE_PROMISE,
            "the returned address must still be a live Promise after the callback's collection"
        );
        // The discriminating assertion: `resolve(1)` settled the promise at its
        // POST-collection address (the capture word was rewritten). Returning
        // the pre-collection address — the #9539 bug — hands back the retired
        // from-space copy, which is still Pending.
        assert_eq!(
            (*promise).state,
            crate::promise::PromiseState::Fulfilled,
            "assimilation must return the wrapper the resolving closure settled, \
             not its pre-collection address"
        );
        assert_eq!((*promise).value, 1.0);
    }
}

//! #7577: the generator-instance prototype wiring must survive a copying minor
//! that lands **inside** its own call.
//!
//! `js_generator_attach_prototype` and its closure-identity sibling are the
//! last thing generator construction does, and codegen drops the caller's root
//! immediately before the call:
//!
//! ```llvm
//! %r67 = bitcast i64 %r66 to double
//! store ptr addrspace(1) null, ptr %r28              ; the caller's root, dropped
//! %r68 = call double @js_generator_attach_prototype(double %r67, i32 0)
//! ```
//!
//! So the callee owns the only reference — and it allocates, twice
//! (`js_object_alloc`, and `object_set_static_prototype` via
//! `object_meta_ensure`). Pre-fix it bound the receiver's address at entry and
//! used it at the tail, producing two wrong answers at once: the
//! `[[Prototype]]` link went onto the **pre-move** address, and the function
//! **returned** that address to the caller as the generator object.
//!
//! Both tests here force the collection into that window deterministically —
//! `force_next_general_arena_alloc_slow` + `make_arena_trigger_due` make the
//! next arena block allocation collect, and the next one is the callee's own —
//! so neither depends on `PERRY_GC_ZEAL` or on the timing luck the #7577
//! reproducer needs. Each asserts its subject was live (the receiver actually
//! moved), per CLAUDE.md's "a gate must assert its subject was live": a run in
//! which nothing moved proves nothing, and says so rather than passing.

use super::*;

/// A function pointer to register in the generator-function registry. Never
/// called — only its address is used.
extern "C" fn fake_generator_body(_c: *const crate::closure::ClosureHeader, _a: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Build the generator intrinsic tower up front. It is lazily constructed on
/// the first `generator_prototype_ptr` call and costs dozens of allocations;
/// paying it here keeps the call under test down to its own two, so the
/// injected trigger lands where we intend.
fn warm_generator_intrinsics() {
    let _ = crate::object::js_generator_attach_prototype(
        f64::from_bits(crate::value::TAG_UNDEFINED),
        0,
    );
}

/// A shadow-rooted, freshly allocated object standing in for a generator
/// instance, as `(nanboxed, raw_addr)`.
fn rooted_instance() -> (f64, usize) {
    let obj = crate::object::js_object_alloc(0, 0);
    let addr = obj as usize;
    js_shadow_slot_set(0, ptr_bits(addr));
    (f64::from_bits(ptr_bits(addr)), addr)
}

/// Arm the next arena block allocation to collect.
fn arm_collection_on_next_block(trigger_guard: &GcTriggerThresholdTestGuard) {
    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
}

fn current_addr() -> usize {
    (js_shadow_slot_get(0) & POINTER_MASK) as usize
}

/// The two observable consequences of the pre-#7577 shape, asserted together:
/// the value handed back to the caller, and where the `[[Prototype]]` link
/// landed.
///
/// Deliberately NOT asserted: that `before` carries no recorded prototype. The
/// nursery recycles addresses between tests on this thread, and the collector's
/// `object_static_prototype_owner_moved` migrates an entry off the old address
/// when the owner moves — so that slot's contents are not a stable signal
/// either way. What is asserted is what the bug got wrong: the receiver moved
/// (subject live), the return value is the current address, and the LIVE object
/// owns the link.
fn assert_wiring_followed_the_move(returned: f64, before: usize, label: &str) {
    let after = current_addr();
    assert_ne!(
        after, before,
        "{label}: subject not live — no copying minor moved the receiver during \
         the call, so this test proved nothing. Check the arena trigger arming."
    );
    assert_eq!(
        crate::value::js_nanbox_get_pointer(returned) as usize,
        after,
        "{label}: must return the receiver's CURRENT address; returning the \
         pre-move one hands the caller a dangling generator object"
    );
    let recorded =
        crate::object::prototype_chain::object_static_prototype(after).unwrap_or_else(|| {
            panic!(
                "{label}: no `[[Prototype]]` recorded against the LIVE object. \
                 Recorded against the pre-move address instead, it is unreachable \
                 and `Object.getPrototypeOf(gen())` silently finds nothing."
            )
        });
    let proto_addr = (recorded & crate::value::POINTER_MASK) as usize;
    assert!(
        crate::value::addr_class::is_plausible_heap_addr(proto_addr),
        "{label}: the recorded `[[Prototype]]` is not a heap address ({proto_addr:#x})"
    );
}

/// SABOTAGE CHECK: bind `obj_ptr` at the top of `js_generator_attach_prototype`
/// again and use it at the tail (the pre-#7577 shape). Both the returned
/// address and the prototype link go to the dead object and this fails.
#[test]
fn attach_prototype_survives_a_copying_minor_inside_the_call() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Without this the `RuntimeHandleScope` inside the function under test is
    // decorative — the guard above took the thread's mutable-root scanners with
    // it, so the fix would look absent and the test would fail for the wrong
    // reason. See `register_runtime_handle_root_scanner_for_tests`.
    register_runtime_handle_root_scanner_for_tests();
    warm_generator_intrinsics();

    let (obj_value, before) = rooted_instance();
    arm_collection_on_next_block(&trigger_guard);

    let returned = crate::object::js_generator_attach_prototype(obj_value, 0);

    assert_wiring_followed_the_move(returned, before, "js_generator_attach_prototype");
}

/// The closure-identity path, whose extra allocation is
/// `generator_function_prototype_of` minting this function's `g.prototype`.
///
/// SABOTAGE CHECK: restore the entry-bound `obj_ptr` in
/// `js_generator_attach_closure_prototype` and this goes red.
#[test]
fn attach_closure_prototype_survives_a_copying_minor_inside_the_call() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();
    warm_generator_intrinsics();

    let func_ptr = fake_generator_body as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    crate::closure::js_register_closure_generator_function(func_ptr);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    assert!(!closure.is_null(), "closure allocation failed");

    let (obj_value, before) = rooted_instance();
    arm_collection_on_next_block(&trigger_guard);

    let returned = crate::object::js_generator_attach_closure_prototype(
        obj_value,
        closure as *const crate::closure::ClosureHeader,
    );

    assert_wiring_followed_the_move(returned, before, "js_generator_attach_closure_prototype");
}

//! #7577: the generator-instance prototype wiring must survive a copying minor
//! that lands **inside** its own call, at an ALLOCATION POINT.
//!
//! The two witnesses below run under the `PERRY_GC_MOVING_LOOP_POLLS=0` kill
//! switch, which is what `force_alloc_point_minor_pacing()` names. Under the
//! shipped default the callee's window does not exist at all — see
//! `the_shipped_default_defers_the_trigger_out_of_the_callees_window`, which
//! asserts that positively rather than leaving it as an argument.
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
///
/// **Must call the builder directly.** This used to go through
/// `js_generator_attach_prototype(TAG_UNDEFINED, 0)`, banking on that helper
/// reaching `generator_prototype_ptr` and lazily building the tower as a side
/// effect. It never did: `js_generator_attach_prototype` returns at its very
/// first line for any non-pointer `obj` (`!jv.is_pointer()`), so the "warm-up"
/// call was a no-op that touched nothing. That went unnoticed only because
/// `GENERATOR_FUNCTION_INTRINSIC_PTR` & co. were plain process-global statics
/// pre-#7723 — some earlier test in the same binary had almost always already
/// built the tower, so the *real* call under test found it cached regardless
/// of what this function did. #7723 converted those statics to
/// `per_test_global!` so each test starts from a guaranteed first-touch state
/// (its whole point, for `gc::tests::lazy_intrinsic_towers`), which took away
/// the accidental cross-test priming and exposed this helper as dead code:
/// with nothing pre-built, the real call now pays the dozens-of-allocations
/// tower build itself, inside `GcSuppressScope` (#7251's no-move window for
/// that build), which swallows the injected arena trigger for the rest of the
/// call and no copying minor ever runs — "subject not live" in the two
/// alloc-point tests, and the deferral witness never seeing its trigger armed
/// in the third. Call the builder directly so this function does what its
/// name and doc always claimed.
fn warm_generator_intrinsics() {
    crate::object::ensure_generator_intrinsics();
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

/// Pin the conservative native-stack scan OFF for the duration, restoring the
/// previous override on drop (including on a panicking assert, so a failure
/// here cannot leak the mode into the next test on this thread).
///
/// **REQUIRED SINCE #7682, together with the legacy-pacing guard beside it.**
/// Both tests below inject their collection at an ALLOCATION POINT — the
/// callee's own `js_object_alloc` — and #7682 changed that point twice over.
/// Each change alone is enough to make the injected collection stop relocating,
/// and the two need different levers:
///
///  1. **It no longer moves.** `gc_check_trigger` now always takes
///     `ManualGcScanGuard::force_full_scan`, which makes the copying minor
///     ineligible, because a NaN-boxed operand in an LLVM register at an
///     allocation point is named by neither root lowering. THIS guard is the
///     answer: `force_full_scan` is a no-op while an override is already
///     pinned, so pinning `Disabled` here leaves the minor eligible. It is the
///     same lever `gc_repsel_matrix.sh`'s `%E%` arms use to force relocation at
///     an allocation point.
///  2. **It no longer happens here at all.** With back-edge polls default-ON
///     the nursery trigger DEFERS to the next precise safepoint and returns
///     without collecting, so the callee's allocation runs no cycle whatsoever.
///     `force_alloc_point_minor_pacing()` is the answer to that one — polls
///     off, no deferral, the direct minor runs at the allocation point as
///     before.
///
///     It must be THAT guard and not `force_legacy_gc_pacing()`, which also
///     turns scavenge off. Scavenge is the disjunct that routes nursery
///     pressure to the direct arm in the first place: the neighbouring
///     `registered_root_scanners_block_budgeted_gc()` reduces to "any COPY-ONLY
///     scanner" under `gc_incremental_enabled()`, and this test's registry
///     holds only a mutable one. With scavenge off the trigger goes to the
///     budgeted stepper, which is non-moving by construction, and the symptom
///     is once again "subject not live" — a third way to reach the same
///     message, which is why the assertion names the arming rather than the
///     cause.
///
/// Diagnosing this needs both symptoms told apart, and they present
/// identically — the receiver simply does not move and the live-subject
/// assertion fires. That assertion is why these tests reported the change
/// instead of silently passing.
///
/// What the tests assert is unchanged and still worth asserting: a runtime
/// helper must not bind a receiver's ADDRESS across its own allocation. #7682
/// removes two routes to that hazard in the shipped default; it does not make
/// the helper correct, and `PERRY_CONSERVATIVE_STACK_SCAN=off` /
/// `PERRY_GC_MOVING_LOOP_POLLS=0` are supported configurations in which the
/// routes are open again.
struct AllocPointRelocationGuard(Option<crate::gc::roots::ConservativeStackScanMode>);

impl AllocPointRelocationGuard {
    fn new() -> Self {
        Self(crate::gc::roots::set_conservative_stack_scan_override(
            Some(crate::gc::roots::ConservativeStackScanMode::Disabled),
        ))
    }
}

impl Drop for AllocPointRelocationGuard {
    fn drop(&mut self) {
        crate::gc::roots::set_conservative_stack_scan_override(self.0);
    }
}

/// SABOTAGE CHECK: bind `obj_ptr` at the top of `js_generator_attach_prototype`
/// again and use it at the tail (the pre-#7577 shape). Both the returned
/// address and the prototype link go to the dead object and this fails.
#[test]
fn attach_prototype_survives_an_alloc_point_copying_minor_inside_the_call() {
    let _guard = CopyingNurseryTestGuard::new(4);
    // Polls off so the alloc-point trigger COLLECTS here instead of deferring,
    // scavenge on so it reaches the direct arm at all, scan off so it may MOVE.
    // See `AllocPointRelocationGuard` for all three.
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _relocation = AllocPointRelocationGuard::new();
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
fn attach_closure_prototype_survives_an_alloc_point_copying_minor_inside_the_call() {
    let _guard = CopyingNurseryTestGuard::new(4);
    // Polls off so the alloc-point trigger COLLECTS here instead of deferring,
    // scavenge on so it reaches the direct arm at all, scan off so it may MOVE.
    // See `AllocPointRelocationGuard` for all three.
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _relocation = AllocPointRelocationGuard::new();
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

/// The shipped default's answer to the same hazard, asserted rather than argued.
///
/// Under the default (back-edge polls ON since #7690) an allocation-point
/// trigger does not collect where it fires: it sets `GC_SAFEPOINT_PENDING` and
/// returns, and the copying minor runs later at a precise loop safepoint — by
/// which time the callee has returned and its `let` is gone. So the #7577
/// window is not merely survivable in the default configuration, it is
/// **closed**, and that is why the two witnesses above must pin the kill switch
/// to keep testing anything.
///
/// This is the honest shape for "add a default-pacing witness". A default-paced
/// test that tried to relocate the receiver INSIDE the call would be vacuous —
/// it would report "subject not live" forever, because there is no longer a
/// collection there to move anything. What is testable, and what this asserts,
/// is the routing that removed it: no collection, and a pending safepoint.
#[test]
fn the_shipped_default_defers_the_trigger_out_of_the_callees_window() {
    let _guard = CopyingNurseryTestGuard::new(4);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    // Polls ON + scavenge ON + nursery cap live: the shipped default since #7690.
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    register_runtime_handle_root_scanner_for_tests();
    warm_generator_intrinsics();
    GC_SAFEPOINT_PENDING.with(|p| p.set(false));

    let (obj_value, before) = rooted_instance();
    let collections_before = gc_collection_count();
    arm_collection_on_next_block(&trigger_guard);

    let returned = crate::object::js_generator_attach_prototype(obj_value, 0);

    assert_eq!(
        gc_collection_count(),
        collections_before,
        "the default must NOT collect at the allocation point — that is the \
         register-imprecise site #7682 closed"
    );
    assert!(
        GC_SAFEPOINT_PENDING.with(std::cell::Cell::get),
        "LIVE SUBJECT: the trigger must actually have been due and deferred. \
         Without this the test also passes when nothing was armed at all."
    );
    assert_eq!(
        current_addr(),
        before,
        "nothing may move inside the callee under the default"
    );
    assert_eq!(
        crate::value::js_nanbox_get_pointer(returned) as usize,
        before,
        "and the receiver handed back is still the one that went in"
    );

    GC_SAFEPOINT_PENDING.with(|p| p.set(false));
}

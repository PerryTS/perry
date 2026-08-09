//! #7274 — `fs.readdir`'s options object must survive the allocation that the
//! `withFileTypes` lookup performs.
//!
//! `fs/dirent.rs::options_with_file_types` decoded a raw `*const ObjectHeader`
//! out of the NaN-boxed `options` argument, THEN called
//! `js_string_from_bytes(b"withFileTypes")` — a collection point — and THEN
//! dereferenced the address it computed before the collection. `options_value`
//! is a plain Rust `f64` local, so nothing kept the object alive and nothing
//! rewrote the pointer: a minor landing on that allocation either swept the
//! object outright or evacuated it and left the local naming retired
//! from-space, which the very next allocation recycles.
//!
//! `{ withFileTypes: true }` is a fresh object literal at every call site, so
//! it is a nursery object — precisely the generation the minor triggered by the
//! next allocation relocates.
//!
//! ## Why this lives in a Rust unit test rather than a `.ts` witness
//!
//! `scripts/gc_root_dominance_check.py` reads emitted LLVM IR and this
//! function is hand-written Rust the compiler never sees, so the static gate is
//! structurally blind to it (CLAUDE.md, "a runtime-side cache of a raw heap
//! pointer is a GC root, and the static checker cannot see it"). A `.ts`
//! reproducer would additionally need a real filesystem, a real `readdir`, and
//! GC pressure landing inside one 3-instruction window. Driving the collection
//! from the test is deterministic and needs no knob.
//!
//! ## Which configuration this bites in — stated precisely
//!
//! `gc_check_trigger`'s alloc-point arm engages
//! `ManualGcScanGuard::force_full_scan(NurseryChurnSlackValve)` (unconditional
//! since #7682). When that guard *engages*, the conservative native-stack scan
//! both retains the raw local and makes the copying minor ineligible
//! (`CopiedMinorFallbackReason::ConservativeStack`), so the pre-fix code
//! survives — the address neither dies nor moves.
//!
//! It does not always engage. `force_full_scan` is a no-op whenever
//! `CONSERVATIVE_STACK_SCAN_OVERRIDE` is already set, and
//! `conservative_stack_scan_mode` lets an explicit
//! `PERRY_CONSERVATIVE_STACK_SCAN` env value beat any pin. So
//! `PERRY_CONSERVATIVE_STACK_SCAN=off` — the arm every issue in this family
//! reproduces on — removes the valve and the alloc-point minor evacuates.
//!
//! These tests run in exactly that configuration and say so out loud rather
//! than inheriting it: `CopyingNurseryTestGuard` pins the mode to `Auto`, which
//! `conservative_stack_scan_decision_for` resolves to `SkipDisabled` — the same
//! decision `PERRY_CONSERVATIVE_STACK_SCAN=off` produces. So the honest claim
//! is not "this crashed a shipped default build"; it is "the valve masking it
//! is a bounded valve #7148 documents as such, and with the valve off the
//! read lands on retired from-space, deterministically."
//!
//! ## What makes it able to fail
//!
//! The options object is held by NOTHING except the function under test: the
//! test never installs it in a shadow slot and it is not reachable from any
//! registered root. So the only thing that can keep it alive and correct
//! across the key allocation is the `RuntimeHandleScope` inside
//! `options_with_file_types` itself.
//!
//! And the collection is asserted to have MOVED something — a separately
//! rooted sentinel allocated in the same nursery must come back at a different
//! address. Without that, a cycle that collected nothing would let this file
//! pass while proving nothing (CLAUDE.md, "a gate must assert its subject was
//! live"). Pacing is left at the shipped default deliberately: pinning
//! `force_legacy_gc_pacing` (scavenge off, polls off) routes the trigger to the
//! budgeted stepper, which is non-moving by construction — under that guard the
//! sentinel never moves and the file certifies nothing. That was observed, not
//! assumed.
//!
//! SABOTAGE RECORD: reverting `options_with_file_types` to its pre-fix shape
//! (decode, then allocate, then dereference) fails
//! `readdir_options_object_survives_the_with_file_types_key_allocation` and
//! leaves the negative test green — the negative one is a regression guard, not
//! a discriminator, and is labelled as such below.

use super::*;

/// Build `{ withFileTypes: true }` in the nursery and return it NaN-boxed.
///
/// Runs under suppressed triggers: `js_object_set_field_by_name` allocates the
/// keys array, and a collection there would move `obj` — which this function
/// holds as a bare local — out from under the setup, failing the test for a
/// reason that has nothing to do with the subject.
fn with_file_types_options_object(_trigger_guard: &GcTriggerThresholdTestGuard) -> (f64, usize) {
    let obj = crate::object::js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"withFileTypes".as_ptr(), 13);
    crate::object::js_object_set_field_by_name(obj, key, f64::from_bits(crate::value::TAG_TRUE));
    assert!(
        crate::arena::pointer_in_nursery(obj as usize),
        "the options object must be a movable nursery object or the test \
         exercises nothing"
    );
    (f64::from_bits(ptr_bits(obj as usize)), obj as usize)
}

#[test]
fn readdir_options_object_survives_the_with_file_types_key_allocation() {
    // This test needs nursery pressure to reach the DIRECT allocation-point
    // minor: it asserts both that a bounded assist ran and — as its liveness
    // witness — that the minor EVACUATED. The default moving-loop pacing routes
    // that pressure into the safepoint deferral instead, and a Rust unit test
    // has no loop back-edge poll to drain it, so no collection happens at all.
    // Legacy pacing is not the answer either: it hands the work to the budgeted
    // stepper, which is deliberately non-moving, and the evacuation witness then
    // correctly refuses to certify an empty run. `force_alloc_point_minor_pacing`
    // is the combination this test was written against and the only one in which
    // both halves hold. The moving default's rooting coverage for these helpers
    // is the gap suite's `test_gap_gc_*_rooting.ts` cases plus the zeal +
    // from-space-protect runs, not this vehicle.
    let _alloc_point_pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let (options_value, options_addr_before) = with_file_types_options_object(&trigger_guard);

    // The liveness witness. Rooted, so the collector must keep it AND rewrite
    // it; if its address is unchanged afterwards the cycle did not evacuate and
    // this test would be certifying an empty run.
    let sentinel_scope = RuntimeHandleScope::new();
    let sentinel = sentinel_scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let sentinel_before = sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize;

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();

    // SUBJECT. The `js_string_from_bytes` inside this call is the collection
    // point; everything the function needs afterwards has to come back out of
    // its own handle.
    let with_file_types = unsafe { crate::fs::options_with_file_types(options_value) };

    drain_scheduled_minor_gc(before, "withFileTypes key allocation");

    let sentinel_after = sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize;
    assert_ne!(
        sentinel_after, sentinel_before,
        "the minor did not evacuate — no object moved, so nothing here was \
         exercised and a green result would be meaningless"
    );
    assert!(
        with_file_types,
        "options_with_file_types read `withFileTypes` through the address it \
         computed BEFORE the key allocation (object was at {options_addr_before:#x}); \
         the collection at that allocation moved/reclaimed the object, so the \
         read landed on retired from-space"
    );
}

/// The negative half: an options object WITHOUT the field must still read
/// `false` across the same collection, rather than picking up a truthy word
/// from whatever was recycled into the from-space bytes.
///
/// NOT A DISCRIMINATOR — it stays green under the pre-fix code, because a
/// stale read yields not-truthy just as readily as a correct one. It is here as
/// a regression guard on the `None`/false arms of the rewritten decode
/// (`options_object_ptr` returning `None`, the handle-band floor), which the
/// positive test never reaches. Recorded rather than deleted so nobody later
/// reads its passing as evidence about the rooting.
#[test]
fn readdir_options_without_the_field_stays_false_across_the_key_allocation() {
    // This test needs nursery pressure to reach the DIRECT allocation-point
    // minor: it asserts both that a bounded assist ran and — as its liveness
    // witness — that the minor EVACUATED. The default moving-loop pacing routes
    // that pressure into the safepoint deferral instead, and a Rust unit test
    // has no loop back-edge poll to drain it, so no collection happens at all.
    // Legacy pacing is not the answer either: it hands the work to the budgeted
    // stepper, which is deliberately non-moving, and the evacuation witness then
    // correctly refuses to certify an empty run. `force_alloc_point_minor_pacing`
    // is the combination this test was written against and the only one in which
    // both halves hold. The moving default's rooting coverage for these helpers
    // is the gap suite's `test_gap_gc_*_rooting.ts` cases plus the zeal +
    // from-space-protect runs, not this vehicle.
    let _alloc_point_pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    register_runtime_handle_root_scanner_for_tests();

    let obj = crate::object::js_object_alloc(0, 1);
    let key = crate::string::js_string_from_bytes(b"encoding".as_ptr(), 8);
    let encoding = crate::string::js_string_from_bytes(b"utf8".as_ptr(), 4);
    crate::object::js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(string_bits(encoding as usize)),
    );
    let options_value = f64::from_bits(ptr_bits(obj as usize));

    let sentinel_scope = RuntimeHandleScope::new();
    let sentinel = sentinel_scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let sentinel_before = sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize;

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();

    let with_file_types = unsafe { crate::fs::options_with_file_types(options_value) };

    drain_scheduled_minor_gc(before, "withFileTypes key allocation");

    assert_ne!(
        sentinel.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize,
        sentinel_before,
        "the minor did not evacuate — nothing here was exercised"
    );
    assert!(
        !with_file_types,
        "an options object with no `withFileTypes` field must read false"
    );
}

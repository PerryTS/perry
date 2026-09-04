//! #9717 — a budgeted (classifier-mode) full trace must keep alive an array a
//! live heap slot reaches only through an array-growth forwarding stub.
//!
//! Array growth leaves a PERMANENT forwarding stub at the pre-grow address and
//! never rewrites the references pointing at it (#6228 / #233), so a live field
//! — hono's `SmartRouter.#routes`, in the reported case — can keep naming the
//! stub. A SYNCHRONOUS full trace is fine: its census (`record_arena_header`)
//! admits every arena object, stubs included, so `mark_field_into_worklist`
//! marks the stub and `trace_one_worklist_header` follows it to the live array.
//!
//! A BUDGETED full trace (what the idle-time reducer runs) resolves membership
//! through `classifier_valid_object_start` instead, and that gate rejected
//! FORWARDED headers by design (a dead metadata key's recycled bytes can set
//! the bit, #8040). So the field -> stub edge was dropped: the stub was never
//! marked, the FORWARDED-follow never ran, and the array reachable ONLY through
//! it was swept — the private-field array that "is still an array, but empty"
//! ten seconds into a loaded server.
//!
//! The classifier is documented as a census SUPERSET; for stubs it was not.
//! These tests plant the exact edge and assert the budgeted trace keeps the
//! target alive. Each first asserts the PREMISE — the pre-#9717 gate rejects
//! the stub — so a green run says the recovery works, not that nothing was
//! tried.

use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

/// Grow a fresh array past its inline capacity and return `(stub, grown, first
/// child)`: `stub` is the pre-grow head, now a forwarding stub; `grown` is the
/// current head; `first_child` is the value at index 0, reachable only through
/// the array.
fn grow_array_leaving_stub() -> (usize, usize, usize) {
    let stub = crate::array::js_array_alloc(0);
    let mut current = stub;
    let mut first_child = 0usize;
    for i in 0..64 {
        let child = young_leaf();
        if i == 0 {
            first_child = child;
        }
        current = crate::array::js_array_push_f64(current, f64::from_bits(ptr_bits(child)));
    }
    assert_ne!(
        stub, current,
        "setup must grow the array and leave a forwarding stub"
    );
    unsafe {
        let stub_hdr = header_from_user_ptr(stub as *const u8) as *mut GcHeader;
        assert_ne!(
            (*stub_hdr).gc_flags & GC_FLAG_FORWARDED,
            0,
            "the pre-grow head must be a forwarding stub"
        );
        // THE PREMISE: the census-superset gate a budgeted classifier used
        // rejects this stub, so a trace consulting only it drops the edge.
        assert!(
            crate::gc::barrier::plausible_arena_user_ptr_header(stub_hdr).is_none(),
            "a forwarded stub must fail the pre-#9717 gate, or recovery proves nothing"
        );
    }
    (stub as usize, current as usize, first_child)
}

#[test]
fn a_budgeted_full_cycle_keeps_an_array_a_live_field_reaches_through_a_growth_stub() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    reset_global_roots();
    let _root_reset = ShadowAndGlobalRootResetGuard;

    let (stub, _grown, first_child) = grow_array_leaving_stub();

    // The "#routes field": a heap holder whose slot points DIRECTLY at the
    // stub (references are never rewritten), rooted so the trace reaches it.
    let holder = crate::array::js_array_alloc(1);
    let holder = crate::array::js_array_push_f64(holder, f64::from_bits(ptr_bits(stub)));
    js_shadow_slot_set(0, ptr_bits(holder as usize));

    let recoveries_before = crate::gc::forwarded_stub_membership_recoveries();

    // Drive a budgeted FULL cycle — the idle reclaimer's path — to completion.
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.collection_kind, GcCollectionKind::Full.ffi_code());
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    assert!(
        crate::gc::forwarded_stub_membership_recoveries() > recoveries_before,
        "#9717: the budgeted trace must recover the growth stub the live field \
         points at; without it the field -> stub edge is dropped and the array \
         reachable only through it is swept"
    );

    // The array survives with its contents: resolve holder[0] -> stub -> grown
    // and read the first element back through the stub (clean_arr_ptr follows
    // the forward). A swept target would read as an empty/undefined array here.
    let holder_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let stub_bits =
        crate::array::js_array_get_f64(holder_after as *const crate::array::ArrayHeader, 0)
            .to_bits();
    let stub_after = (stub_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    let child_bits = crate::array::js_array_get_f64(stub_after, 0).to_bits();
    let child_after = (child_bits & POINTER_MASK) as usize;
    assert_eq!(
        child_after, first_child,
        "the array element reachable only through the field -> stub edge must \
         survive the budgeted full cycle unchanged"
    );
}

/// Negative control: a SYNCHRONOUS full mark-sweep already handles the same
/// edge through the exact census (`record_arena_header`), so it needs no
/// recovery. This keeps the assertion above a statement about the BUDGETED
/// path specifically, not about stubs in general.
#[test]
fn a_synchronous_full_cycle_keeps_the_same_array_without_needing_recovery() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_global_roots();
    let _root_reset = ShadowAndGlobalRootResetGuard;

    let (stub, _grown, first_child) = grow_array_leaving_stub();
    let holder = crate::array::js_array_alloc(1);
    let holder = crate::array::js_array_push_f64(holder, f64::from_bits(ptr_bits(stub)));
    js_shadow_slot_set(0, ptr_bits(holder as usize));

    let recoveries_before = crate::gc::forwarded_stub_membership_recoveries();
    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Manual));
    assert_eq!(
        crate::gc::forwarded_stub_membership_recoveries(),
        recoveries_before,
        "the synchronous census admits stubs directly; the classifier recovery \
         path must not run on this path"
    );

    let holder_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let stub_bits =
        crate::array::js_array_get_f64(holder_after as *const crate::array::ArrayHeader, 0)
            .to_bits();
    let stub_after = (stub_bits & POINTER_MASK) as *const crate::array::ArrayHeader;
    let child_bits = crate::array::js_array_get_f64(stub_after, 0).to_bits();
    assert_eq!(
        (child_bits & POINTER_MASK) as usize,
        first_child,
        "a synchronous full cycle keeps the stub-reached array (unchanged behaviour)"
    );
}

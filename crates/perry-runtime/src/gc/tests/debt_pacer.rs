use super::super::*;
use super::support::*;

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

fn live_test_string(bytes: &'static [u8]) -> usize {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as usize
}

fn budgeted_step_until_phase(target: GcCyclePhase) -> JsGcStepResult {
    let mut status = JsGcStepResult::default();
    for _ in 0..500_000 {
        let current = js_gc_step_status(&mut status);
        if current == JS_GC_STEP_STATUS_ACTIVE && status.phase == target.ffi_code() {
            return status;
        }
        let stepped = js_gc_step_work_units(1, &mut status);
        if stepped == JS_GC_STEP_STATUS_ACTIVE && status.phase == target.ffi_code() {
            return status;
        }
        assert_ne!(
            stepped, JS_GC_STEP_STATUS_COMPLETED,
            "budgeted cycle completed before reaching phase {target:?}"
        );
    }
    panic!("budgeted cycle did not reach phase {target:?}");
}

#[test]
fn arena_threshold_debt_starts_bounded_assist_without_monolithic_collection() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"arena_debt_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    let before = gc_collection_count();
    gc_check_trigger();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(status.active, 1);
    assert_eq!(status.completed, 0);
    assert!(status.arena_debt_bytes > 0);
    assert_eq!(
        gc_collection_count(),
        before,
        "arena pressure should not complete a synchronous collection"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(gc_collection_count() > before);
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"arena_debt_live");
    }
    assert!(
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get()) > crate::arena::arena_total_bytes(),
        "completed arena debt cycle should rebaseline the heap goal"
    );
}

#[test]
fn malloc_threshold_debt_reclaims_dead_churn_after_host_drain() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len()
    );
    let malloc_count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_count.saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(status.trigger_kind, GcTriggerKind::MallocCount.ffi_code());
    assert!(status.malloc_debt_objects > 0);
    assert_eq!(
        gc_collection_count(),
        before,
        "malloc pressure should be assisted, not synchronously collected"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive the completed debt cycle"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "dead malloc churn should be reclaimed once debt is drained"
    );

    let survivors_after = malloc_object_count();
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    assert_eq!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
        survivors_after + malloc_step_after
    );
}

#[test]
fn active_cycle_gc_check_trigger_calls_pay_bounded_assist_work() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"active_assist_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 8) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    let before = gc_collection_count();
    gc_check_trigger();
    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);

    gc_check_trigger();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(
        gc_collection_count(),
        before,
        "active-cycle assists must not start a nested synchronous collection"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(gc_collection_count() > before);
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"active_assist_live");
    }
}

/// #6180: allocation-side mutator assists must drive the *entire* budgeted
/// cycle to completion — through `AtomicFinalize`, `Sweep`, and `Reclaim` —
/// using only the slice of work performed from `gc_check_trigger` (the
/// allocator), never a host safepoint (`js_gc_step_work_units`).
///
/// Before #6180 the assist path bailed at the first non-mark phase, so a pure
/// compute loop that never reached the event pump would start a cycle, advance
/// it to `AtomicFinalize`, and park there forever: the incremental mark barrier
/// stayed enabled and nothing was ever swept, so resident memory grew without
/// bound. This proves the parking hole is closed — dead malloc churn is
/// reclaimed and the live root survives, entirely from allocation-side assists.
///
/// Host-driven incremental sweep/reclaim *slicing* (partial-progress pauses) is
/// covered separately in `incremental_sweep_reclaim.rs`.
#[test]
fn allocation_assists_complete_finalize_sweep_and_reclaim() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len()
    );
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_object_count().saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    // Drive the active budgeted cycle using ONLY allocation-side assists: every
    // `gc_check_trigger()` performs one bounded assist step, and we observe the
    // phase after each. We never call `js_gc_step_work_units` (the host path).
    let mut status = JsGcStepResult::default();
    let mut reached_finalize = false;
    let mut reached_sweep = false;
    let mut reached_reclaim = false;
    let mut completed = false;
    for _ in 0..500_000 {
        gc_check_trigger();
        js_gc_step_status(&mut status);
        if status.phase == GcCyclePhase::AtomicFinalize.ffi_code() {
            reached_finalize = true;
        } else if status.phase == GcCyclePhase::Sweep.ffi_code() {
            reached_sweep = true;
        } else if status.phase == GcCyclePhase::Reclaim.ffi_code() {
            reached_reclaim = true;
        }
        if gc_collection_count() > before {
            completed = true;
            break;
        }
    }

    assert!(
        completed,
        "allocation-side assists alone must drive the budgeted cycle to completion (#6180 parking hole)"
    );
    assert!(
        reached_finalize && reached_sweep && reached_reclaim,
        "assists must advance through atomic finalize, sweep, and reclaim \
         (finalize={reached_finalize} sweep={reached_sweep} reclaim={reached_reclaim})"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "assist-driven sweep must reclaim dead malloc churn"
    );
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root must survive the assist-driven cycle"
    );
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        live_after, live_malloc as usize,
        "live root must remain reachable via the shadow slot after assists drain the cycle"
    );
}
fn noop_copy_only_root_scanner(_visit: &mut dyn FnMut(f64)) {}

/// Regression: the direct synchronous minor — taken whenever synchronous-only
/// root scanners block the budgeted stepper, i.e. in every compiled program —
/// must re-baseline the arming trigger on completion, exactly as the budgeted
/// finisher (`gc_finish_budgeted_cycle`) does.
///
/// The bug: this arm merely emitted the outcome, leaving `GC_NEXT_TRIGGER_BYTES`
/// at the value that armed the collection. The non-moving minor reclaims dead
/// objects into per-block free lists without lowering `arena_total`, so a
/// workload holding a large live set above the trigger kept
/// `gc_budgeted_due_trigger` reporting the trigger as due and re-armed a whole-
/// arena mark/sweep on every fresh block — O(n^2), a ~100% CPU stall with a
/// bounded live set that never made progress.
#[test]
fn direct_arena_minor_rebaselines_trigger_above_live_set() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    // A registered copy-only scanner makes the budgeted stepper ineligible, so
    // gc_check_trigger takes the direct synchronous-minor arm.
    let _scanners = ScopedRootScannerRegistryGuard::new();
    gc_register_root_scanner(noop_copy_only_root_scanner);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"direct_minor_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }

    // Arm the arena trigger (sets GC_NEXT_TRIGGER_BYTES = 0) so it is due.
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();

    gc_check_trigger();

    // The direct arm runs a synchronous collection to completion (unlike the
    // budgeted stepper, which would only arm an assist here)...
    assert!(
        gc_collection_count() > before,
        "a registered synchronous-only scanner should drive gc_check_trigger \
         through the direct synchronous minor, not the budgeted stepper"
    );
    // ...and re-baselines the arena trigger above the retained live set, so the
    // next allocation does not immediately re-arm another whole-arena minor.
    let next_trigger = GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get());
    let arena_total = crate::arena::arena_total_bytes();
    assert!(
        next_trigger > arena_total,
        "direct minor must rebaseline the arena trigger above arena_total \
         (next_trigger={next_trigger}, arena_total={arena_total}); leaving it at \
         the arming value re-triggers a full minor on every block"
    );
}

/// Companion to the arena case for the `MallocCount` arm: the direct minor must
/// dispatch to `gc_finish_malloc_trigger_collection`, which sweeps malloc (its
/// `debug_assert!(outcome.malloc_swept)` is exercised here) and re-baselines
/// `GC_NEXT_MALLOC_TRIGGER` to `survivors + step`. Without the re-baseline the
/// malloc trigger stays at the arming value and every tracked allocation
/// re-arms a full synchronous minor.
#[test]
fn direct_malloc_minor_rebaselines_trigger_above_survivors() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    let _scanners = ScopedRootScannerRegistryGuard::new();
    gc_register_root_scanner(noop_copy_only_root_scanner);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len()
    );

    // Arm the malloc-count trigger so the direct minor takes the MallocCount arm.
    let malloc_count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_count.saturating_sub(1)));

    let before = gc_collection_count();
    gc_check_trigger();

    // The direct arm runs a synchronous collection to completion...
    assert!(
        gc_collection_count() > before,
        "a registered synchronous-only scanner should drive the MallocCount \
         trigger through the direct synchronous minor, not the budgeted stepper"
    );
    // ...and re-baselines the malloc trigger to survivors + step (the same
    // formula the budgeted finisher applies), leaving it strictly above the
    // surviving count so the next allocation does not immediately re-arm.
    let survivors_after = malloc_object_count();
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get());
    assert_eq!(
        next_malloc_trigger,
        survivors_after + malloc_step_after,
        "direct malloc minor must rebaseline GC_NEXT_MALLOC_TRIGGER to \
         survivors + step (next={next_malloc_trigger}, survivors={survivors_after}, \
         step={malloc_step_after})"
    );
    assert!(next_malloc_trigger > survivors_after);
}

/// Debt-proportional assist pacing: the per-assist work budget must grow
/// linearly with measured debt (and be exactly the base when no debt).
#[test]
fn mutator_assist_work_units_scale_with_debt() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    // Suppressed triggers (usize::MAX) → both debts read zero → base budget.
    assert_eq!(
        gc_mutator_assist_scaled_work_units(),
        GC_MUTATOR_ASSIST_WORK_UNITS
    );

    // Arena debt scales at GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT bytes per unit.
    let total = crate::arena::arena_total_bytes();
    let arena_debt = (2 * 1024 * 1024).min(total);
    GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(total - arena_debt));
    let expected = GC_MUTATOR_ASSIST_WORK_UNITS
        + (arena_debt as u64 / GC_ASSIST_DEBT_BYTES_PER_WORK_UNIT) as usize;
    assert_eq!(gc_mutator_assist_scaled_work_units(), expected);

    // Malloc debt converts 1:1 (one mark/sweep unit per outstanding object).
    let malloc_count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(malloc_count.saturating_sub(7)));
    let malloc_debt = malloc_count - malloc_count.saturating_sub(7);
    assert_eq!(
        gc_mutator_assist_scaled_work_units(),
        expected + malloc_debt
    );
}

/// The measured #6180 Stage-2 failure mode: with a FIXED 256-unit assist
/// budget, a large-debt cycle crawls — a 10M-allocation benchmark completed
/// ZERO collections while the synchronous default completed 7, and RSS grew
/// 6-22× unbounded. With debt-scaled assists the same shape must finish in a
/// bounded number of allocation-side calls: the heap below needs hundreds of
/// thousands of work units, which 300 fixed-256 assists (76.8k units) cannot
/// supply but 300 debt-scaled assists comfortably can.
#[test]
fn debt_scaled_assists_cannot_be_outrun_by_allocation() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"debt_pacing_live");
    js_shadow_slot_set(0, string_bits(live));

    // A heap big enough that fixed-256 assists could not finish the cycle
    // within this test's call budget (see doc comment).
    for _ in 0..150_000 {
        let _ = young_leaf();
    }

    // Simulate the collector having fallen far behind: several MB of debt.
    let total = crate::arena::arena_total_bytes();
    let debt = (total / 2).max(1);
    GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(total - debt));
    assert!(
        gc_mutator_assist_scaled_work_units() > GC_MUTATOR_ASSIST_WORK_UNITS * 10,
        "large debt must scale the assist budget well past the base"
    );

    // Drive the cycle with allocation-side assists ONLY (never a host step),
    // while the mutator keeps allocating between calls.
    let before = gc_collection_count();
    let mut calls = 0usize;
    while gc_collection_count() == before {
        for _ in 0..8 {
            let _ = young_leaf();
        }
        gc_check_trigger();
        calls += 1;
        assert!(
            calls <= 300,
            "debt-scaled assists must complete the cycle before allocation \
             outruns collection (still incomplete after {calls} calls)"
        );
    }

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"debt_pacing_live");
    }
}

/// Allocate-black lifecycle: runtime-path allocations made while a budgeted
/// cycle is in flight are born MARKED (from the FIRST build slice, not just
/// the barrier window), and birth flags reset once the cycle completes.
#[test]
fn budgeted_cycle_allocations_are_born_marked_for_the_whole_cycle() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"birth_flags_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    // Start the budgeted cycle; it parks in BuildValidPointerSet after one
    // bounded assist — BEFORE the mark barrier enables.
    gc_check_trigger();
    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);

    let mid_cycle = young_leaf();
    let header = unsafe { header_from_user_ptr(mid_cycle as *const u8) };
    assert!(
        unsafe { (*header).gc_flags } & GC_FLAG_MARKED != 0,
        "allocation during an active budgeted cycle must be born marked \
         (raw-installed runtime buffers would otherwise be swept live)"
    );

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    let post_cycle = young_leaf();
    let post_header = unsafe { header_from_user_ptr(post_cycle as *const u8) };
    assert_eq!(
        unsafe { (*post_header).gc_flags } & GC_FLAG_MARKED,
        0,
        "birth flags must reset once the cycle completes"
    );
}

/// Manual `gc()` landing while a budgeted cycle is parked mid-phase must
/// drain that cycle to completion FIRST: two cycles share GC_FLAG_MARKED,
/// the mark-seed queue, and the barrier TLS, so the synchronous full's sweep
/// would erase the parked cycle's marks and its later sweep would free live
/// objects (measured as the #6224 stress SIGSEGV).
#[test]
fn manual_gc_drains_parked_budgeted_cycle_first() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"drain_first_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    gc_check_trigger();
    let mut status = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_status(&mut status),
        JS_GC_STEP_STATUS_ACTIVE,
        "budgeted cycle should be parked mid-phase before the manual gc"
    );

    let before = gc_collection_count();
    js_gc_collect();

    assert!(
        !gc_budgeted_cycle_active(),
        "manual gc must leave no parked budgeted cycle behind"
    );
    assert!(
        gc_collection_count() >= before + 2,
        "both the drained budgeted cycle and the manual full must complete"
    );
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"drain_first_live");
    }
}

/// Final-root-remark (#6180 Stage 2): a pointer whose ONLY reference appears
/// in a shadow-stack slot AFTER the budgeted cycle's one-shot RootScan has
/// completed is invisible to the original scan — the atomic-finalize remark
/// must re-scan roots and keep it alive. The object is allocated BEFORE the
/// cycle starts (so whole-cycle allocate-black does not protect it) and is
/// referenced by nothing until it is planted in the slot mid-cycle.
#[test]
fn atomic_finalize_remark_rescues_pointer_hidden_in_shadow_slot_after_root_scan() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"remark_anchor_live");
    js_shadow_slot_set(0, string_bits(live));

    // The victim: unreferenced at cycle start, NOT born during the cycle.
    let hidden = live_test_string(b"remark_hidden_victim");

    for _ in 0..(GC_MUTATOR_ASSIST_WORK_UNITS * 4) {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();
    gc_check_trigger();

    // Advance the budgeted cycle PAST RootScan, then plant the only
    // reference — the exact hide-in-a-local shape.
    let status = budgeted_step_until_phase(GcCyclePhase::MarkPropagation);
    assert_eq!(status.phase, GcCyclePhase::MarkPropagation.ffi_code());
    // RAW slot write, deliberately bypassing js_shadow_slot_set: that setter
    // fires runtime_write_barrier_root_nanbox, whose incremental-mark shading
    // would keep `hidden` alive on its own and stop this test from isolating
    // FinalRootRemark (CodeRabbit, PR #6235). The hide-in-a-local hazard is
    // exactly a migration that crosses NO barriered store.
    SHADOW.with(|cell| unsafe {
        let st = &mut *cell.get();
        let slot = st.frame_top + 1;
        st.stack[slot] = string_bits(hidden);
        st.active[slot] = true;
    });

    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    let hidden_after = (js_shadow_slot_get(1) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(hidden_after, b"remark_hidden_victim");
    }
    // Deterministic deadness probe: a swept arena object's slot is pushed
    // onto the free list — the victim must not be there.
    let on_free_list =
        ARENA_FREE_LIST.with(|fl| fl.borrow().iter().any(|&(ptr, _)| ptr as usize == hidden));
    assert!(
        !on_free_list,
        "remark must keep the shadow-slot-only pointer off the sweep free list"
    );
}

/// #6228: array growth installs a PERMANENT forwarding stub at the old
/// address; a stale pre-growth pointer held as the ONLY reference (the
/// deforestation pass manufactures exactly this for direct calls) must keep
/// the live post-growth array alive — the tracer hops the forwarding edge
/// and MARKS the target instead of treating the stub as zero-children.
#[test]
fn forwarded_array_stub_propagates_liveness_to_grown_array() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let arr = crate::array::js_array_alloc(4);
    let pre_growth = arr as usize;
    // Push past capacity: js_array_grow reallocates and installs the
    // forwarding stub at `pre_growth`.
    for i in 0..64 {
        crate::array::js_array_push(
            pre_growth as *mut crate::array::ArrayHeader,
            crate::JSValue::number(i as f64),
        );
    }
    let header = unsafe { header_from_user_ptr(pre_growth as *const u8) };
    assert!(
        unsafe { (*header).gc_flags } & GC_FLAG_FORWARDED != 0,
        "growth past capacity must leave a forwarding stub at the old address"
    );

    // The stale pre-growth pointer is the ONLY root.
    js_shadow_slot_set(0, ptr_bits(pre_growth));

    trigger_guard.make_arena_trigger_due();
    gc_check_trigger();
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    // Reads through the stale pointer must still see the full array.
    let live = pre_growth as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(live), 64);
    assert_eq!(crate::array::js_array_get_f64_unchecked(live, 63), 63.0);
}

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

fn make_arena_pressure(trigger_guard: &GcTriggerThresholdTestGuard, live_bytes: &'static [u8]) {
    let live = live_test_string(live_bytes);
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..6_000 {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();
}

fn complete_host_safepoint_cycle() -> JsGcStepResult {
    for _ in 0..500_000 {
        let result = gc_runtime_safepoint();
        match result.status {
            JS_GC_STEP_STATUS_ACTIVE => continue,
            JS_GC_STEP_STATUS_COMPLETED => return result,
            other => panic!("host safepoint cycle stopped before completion: status {other}"),
        }
    }
    panic!("host safepoint cycle did not complete within step limit");
}

struct SuppressGcGuard;

impl SuppressGcGuard {
    fn enter() -> Self {
        gc_suppress();
        Self
    }
}

impl Drop for SuppressGcGuard {
    fn drop(&mut self) {
        gc_unsuppress();
    }
}

struct UnsafeZoneGuard;

impl UnsafeZoneGuard {
    fn enter() -> Self {
        js_gc_enter_unsafe_zone();
        Self
    }
}

impl Drop for UnsafeZoneGuard {
    fn drop(&mut self) {
        js_gc_exit_unsafe_zone();
    }
}

struct RootLockGuard;

impl RootLockGuard {
    fn enter() -> Self {
        super::super::roots::enter_gc_root_lock();
        Self
    }
}

impl Drop for RootLockGuard {
    fn drop(&mut self) {
        super::super::roots::exit_gc_root_lock();
    }
}

#[test]
fn no_pressure_runtime_safepoint_reports_idle_without_starting_cycle() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let before = gc_collection_count();
    let result = gc_runtime_safepoint();

    assert_eq!(result.status, JS_GC_STEP_STATUS_IDLE);
    assert_eq!(result.active, 0);
    assert_eq!(result.completed, 0);
    assert_eq!(gc_collection_count(), before);
}

#[test]
fn arena_pressure_runtime_safepoint_starts_bounded_normal_work() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_live");

    let before = gc_collection_count();
    let result = gc_runtime_safepoint();

    assert_eq!(result.status, JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(result.active, 1);
    assert_eq!(result.completed, 0);
    assert_eq!(result.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(result.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert!(result.arena_debt_bytes > 0);
    assert_eq!(
        gc_collection_count(),
        before,
        "one scheduler safepoint should not complete a monolithic collection"
    );
}

#[test]
fn repeated_runtime_safepoints_complete_cycle_rebaseline_debt_and_preserve_roots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_drain_live");

    let first = gc_runtime_safepoint();
    assert_eq!(first.status, JS_GC_STEP_STATUS_ACTIVE);

    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_eq!(completed.active, 0);
    assert_eq!(completed.completed, 1);
    assert_eq!(completed.arena_debt_bytes, 0);
    assert!(
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get()) > crate::arena::arena_total_bytes(),
        "completed safepoint cycle should rebaseline the arena trigger"
    );

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"host_safepoint_drain_live");
    }
}

#[test]
fn microtask_runner_tail_pays_bounded_safepoint_under_pressure() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_microtask_live");

    let before = gc_collection_count();
    let _ran = crate::promise::js_promise_run_microtasks();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    assert_eq!(gc_collection_count(), before);

    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
}

#[test]
fn stdlib_pump_and_perry_poll_pay_debt_through_shared_scheduler_surfaces() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_stdlib_live");

    crate::stdlib_pump::js_run_stdlib_pump();

    let mut status = JsGcStepResult::default();
    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    make_arena_pressure(&trigger_guard, b"host_safepoint_poll_live");
    let _microtasks = crate::event_pump::perry_poll();

    assert_eq!(js_gc_step_status(&mut status), JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(status.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
}

#[test]
fn js_gc_safepoint_null_and_output_pointer_are_safe() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    assert_eq!(
        js_gc_safepoint(std::ptr::null_mut()),
        JS_GC_STEP_STATUS_IDLE
    );

    make_arena_pressure(&trigger_guard, b"host_safepoint_ffi_live");
    let mut result = JsGcStepResult::default();
    let status = js_gc_safepoint(&mut result);

    assert_eq!(status, result.status);
    assert_eq!(result.status, JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(result.active, 1);
    assert_eq!(result.completed, 0);
    assert!(result.arena_debt_bytes > 0);
    assert_eq!(result.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
}

#[test]
fn unsafe_suppressed_and_root_locked_safepoints_skip_without_collecting() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    make_arena_pressure(&trigger_guard, b"host_safepoint_unsafe_live");
    let before = gc_collection_count();
    {
        let _unsafe_zone = UnsafeZoneGuard::enter();
        let result = gc_runtime_safepoint();
        assert_eq!(result.status, JS_GC_STEP_STATUS_SKIPPED);
        assert_eq!(result.active, 0);
        assert_eq!(gc_collection_count(), before);
    }

    {
        let _suppressed = SuppressGcGuard::enter();
        let result = gc_runtime_safepoint();
        assert_eq!(result.status, JS_GC_STEP_STATUS_SKIPPED);
        assert_eq!(result.active, 0);
        assert_eq!(gc_collection_count(), before);
    }

    {
        let _root_lock = RootLockGuard::enter();
        let result = gc_runtime_safepoint();
        assert_eq!(result.status, JS_GC_STEP_STATUS_SKIPPED);
        assert_eq!(result.active, 0);
        assert_eq!(gc_collection_count(), before);
    }
}

#[test]
fn host_safepoint_trace_reports_normal_incremental_budgeted_steps() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_trace_live");

    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);

    let event = take_test_last_gc_trace_json().expect("host safepoint completion should trace");
    assert_eq!(
        event["pause_budget"]["kind"].as_str(),
        Some("normal_incremental")
    );
    assert_eq!(
        event["pause_budget"]["class"].as_str(),
        Some("ordinary_budgeted")
    );

    let steps = event["pause_steps"]
        .as_array()
        .expect("host safepoint trace should include pause_steps");
    assert!(
        !steps.is_empty(),
        "host safepoint cycle should report ordinary pause steps"
    );
    for (index, step) in steps.iter().enumerate() {
        assert_eq!(
            step["budget"]["kind"].as_str(),
            Some("normal_incremental"),
            "pause_steps[{index}] should use the host safepoint budget kind"
        );
        assert_eq!(
            step["budget"]["class"].as_str(),
            Some("ordinary_budgeted"),
            "pause_steps[{index}] should stay ordinary budgeted"
        );
        assert_eq!(
            step["budget"]["ordinary_budgeted"].as_bool(),
            Some(true),
            "pause_steps[{index}] should count as ordinary budgeted work"
        );
        assert!(
            step["budget"]["within_soft_pause_target"]
                .as_bool()
                .is_some(),
            "pause_steps[{index}] should report soft pause target status"
        );
    }
}

/// #7909: a budgeted cycle started at a host safepoint for NURSERY pressure
/// locks the moving minor out for as long as it stays active — and it can stay
/// active forever.
///
/// # The bug this documents
///
/// `gc_runtime_safepoint()` starts a budgeted cycle as soon as any trigger is
/// due, including the young-generation scavenge cap. That cycle is
/// `low_pause_non_moving` by construction, so it cannot evacuate and cannot
/// lower the quantity `young_scavenge_cap_due()` tests. Meanwhile
/// `gc_safepoint_moving_minor` rejects every safepoint at its `budgeted` entry
/// guard. If the host's step cadence is too slow to finish the cycle, the two
/// facts compose into a stall: the trigger stays due, the collector that could
/// clear it never runs, and the mutator pays the SATB mark barrier for the rest
/// of the process with nothing collected.
///
/// Measured on `gc-handoff/apps/asyncpipe.ts` (`PERRY_GC_DIAG=1`, the
/// `[gc-incremental]` line this branch adds): **1 cycle started, 15 steps, 0
/// completions, still active at exit, mark barrier armed 37 ms of a 127 ms
/// program, zero collections** — 11.8 % of the program's instructions.
///
/// This test pins the **lockout half only**, on a trigger the budgeted cycle
/// CAN discharge (whole-arena bytes). That composition is intended: a cycle
/// that will finish owns the collector while it runs. What was the defect is
/// the *other* trigger — the young-gen scavenge cap, which no budgeted cycle
/// can lower — and that arm is closed by
/// `a_nursery_cap_only_trigger_is_deferred_to_the_collector_that_can_discharge_it`
/// below. Keep both: this one states what the lockout costs, that one states
/// when it is allowed to be paid.
#[test]
fn an_active_budgeted_cycle_locks_out_the_moving_minor_and_keeps_the_barrier_armed() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    make_arena_pressure(&trigger_guard, b"host_safepoint_7909_live");

    // ★ Live-subject half #1: the pressure is real. Without this every
    // assertion below is equally satisfied by a fixture with nothing due —
    // CLAUDE.md, the fourth way a gate cannot fail.
    assert!(
        crate::arena::arena_total_bytes() >= GC_NEXT_TRIGGER_BYTES.with(|t| t.get()),
        "fixture must present a due arena trigger"
    );

    let starts_before = super::super::instruments::incremental_cycle_starts();
    let blocked_before = super::super::instruments::moving_safepoints_blocked_by_budgeted();
    let armed_before = super::super::instruments::mark_barrier_armed_us();

    let started = gc_runtime_safepoint();

    // ★ Live-subject half #2: a cycle really was started by this call.
    assert_eq!(started.status, JS_GC_STEP_STATUS_ACTIVE);
    assert_eq!(
        super::super::instruments::incremental_cycle_starts(),
        starts_before + 1,
        "the instrument must count the cycle the host safepoint just started"
    );
    assert!(gc_budgeted_cycle_active());

    // The composition: with that cycle active, the precise safepoint is
    // rejected, and rejected for the `budgeted` reason specifically.
    assert!(
        !super::super::gc_safepoint_moving_minor(),
        "an active budgeted cycle must lock the moving minor out"
    );
    assert_eq!(
        super::super::instruments::moving_safepoints_blocked_by_budgeted(),
        blocked_before + 1,
        "the block must be attributed to the budgeted cycle, not to a transient guard"
    );

    // And the barrier is armed for the whole time the cycle is open. `>=` not
    // `>`: the window is measured in microseconds and a fast machine can open
    // and read it inside one tick. What must hold is that the instrument is
    // running at all, which the arm-event count states exactly.
    assert!(
        super::super::instruments::mark_barrier_arm_events() > 0,
        "an active incremental cycle must have armed the SATB mark barrier"
    );
    assert!(super::super::instruments::mark_barrier_armed_us() >= armed_before);

    // Drive it to completion so the shared thread state is left clean, and take
    // the opportunity to pin the other end of the instrument.
    let completions_before = super::super::instruments::incremental_completions();
    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert_eq!(
        super::super::instruments::incremental_completions(),
        completions_before + 1,
        "the instrument must count the completion too, or `starts > completions` \
         could never be read as a stall"
    );
    assert!(!gc_budgeted_cycle_active());
}

/// ★ #7909: a host safepoint must NOT start a budgeted cycle whose only due
/// trigger is the young-generation scavenge cap.
///
/// # The defect this closes
///
/// `young_scavenge_cap_due()` tests `copying_from_space_in_use_bytes()`. A
/// budgeted cycle is `low_pause_non_moving` by construction, sweeps in place,
/// and therefore **cannot lower that quantity** — so the trigger survives the
/// cycle it started. Meanwhile the cycle blocks `gc_safepoint_moving_minor` at
/// its `budgeted` entry guard, which is the one collector that *can* lower it.
/// If the host cadence cannot finish the cycle (2048 work units per microtask
/// drain; `gc-handoff/apps/asyncpipe.ts` reaches ~15 drains after the cap goes
/// due) the composition is permanent and completely silent: `cycle_starts=1
/// steps=14 completions=0 active_at_exit=true`, the SATB mark barrier armed for
/// 22.9 ms of a 127 ms program, and **zero** `[gc]` lines because the trace is
/// written by the completion path.
///
/// # Why this is a test and not a knob
///
/// The defect is currently *unreached* on the shipped corpus — #7933/#7939 took
/// `asyncpipe`'s young survival to ~25‰, so the 16 MB cap never goes due there.
/// That is a property of one week's allocation profile, not of the collector:
/// `PERRY_GC_SCAVENGE_NURSERY_MB=4` reproduces the original signature exactly on
/// today's `main`. Closing the issue on those numbers would delete the knowledge
/// and let the next allocation-rate change silently re-expose it.
///
/// # Why the control phase is not optional
///
/// "No cycle was started" is satisfied by a fixture where nothing was due at
/// all, which is the failure mode this repo keeps paying for. So phase 1 asserts
/// the cap is due *and* that neither other trigger is, and phase 2 then makes
/// the whole-arena trigger due **on the same thread, with the same heap** and
/// asserts a cycle DOES start. Only the trigger differs between the two phases,
/// so the pair discriminates "declines this trigger" from "declines everything".
#[test]
fn a_nursery_cap_only_trigger_is_deferred_to_the_collector_that_can_discharge_it() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    // Real young-gen occupancy, then a cap that the occupancy clears. The
    // override is per-thread and next to its reader (support.rs's rule); moving
    // `PERRY_GC_SCAVENGE_NURSERY_MB` would be a process-wide environment write.
    let live = live_test_string(b"nursery_cap_7909_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..6_000 {
        let _ = young_leaf();
    }
    let _cap = super::super::policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);

    // ── phase 1: cap-only pressure ───────────────────────────────────────────
    // ★ Live subject: the cap is genuinely due...
    assert!(
        crate::arena::copying_from_space_in_use_bytes() > 0,
        "fixture must have young-gen occupancy for the cap to test"
    );
    assert!(
        super::super::policy::young_scavenge_cap_due(),
        "fixture must present a due young-gen scavenge cap"
    );
    // ...and it is the ONLY thing due, so the refusal below can only be about it.
    assert!(
        crate::arena::arena_total_bytes() < super::super::policy::next_arena_trigger_base(),
        "the whole-arena trigger must NOT be due in phase 1"
    );
    assert!(
        malloc_object_count() < GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
        "the malloc trigger must NOT be due in phase 1"
    );

    let starts_before = super::super::instruments::incremental_cycle_starts();
    let deferrals_before = super::super::instruments::budgeted_step_nursery_cap_deferrals();

    let declined = gc_runtime_safepoint();

    assert_eq!(
        declined.status, JS_GC_STEP_STATUS_IDLE,
        "a cap-only host safepoint must report idle, not an active cycle"
    );
    assert_eq!(
        super::super::instruments::incremental_cycle_starts(),
        starts_before,
        "no budgeted cycle may be started for a trigger it cannot discharge"
    );
    assert!(
        !gc_budgeted_cycle_active(),
        "no budgeted cycle may be left active by a cap-only safepoint"
    );
    // ★ The refusal is attributed, not merely absent: without this the assertion
    // above is also satisfied by every other reason a start can be skipped
    // (reentrant, start-blocked, nothing due).
    assert_eq!(
        super::super::instruments::budgeted_step_nursery_cap_deferrals(),
        deferrals_before + 1,
        "the refusal must be counted as the nursery-cap deferral specifically"
    );
    // The deferral hands the pressure to the precise safepoint, exactly as the
    // alloc-point arm does — otherwise a compute loop would never poll for it.
    assert!(
        GC_SAFEPOINT_PENDING.with(|pending| pending.get()),
        "declining the cycle must arm the precise safepoint instead"
    );

    // ★ The point of the whole fix: the collector that CAN discharge the cap is
    // reachable. Under the defect this returns false, for the `budgeted` reason.
    let blocked_before = super::super::instruments::moving_safepoints_blocked_by_budgeted();
    assert!(
        super::super::gc_safepoint_moving_minor(),
        "the moving minor must not be locked out by a cycle that was never started"
    );
    assert_eq!(
        super::super::instruments::moving_safepoints_blocked_by_budgeted(),
        blocked_before,
        "no safepoint may be rejected for `budgeted` when no cycle is active"
    );

    // ── phase 2 (control): the SAME fixture, a discharge-able trigger ────────
    // Same thread, same heap, same guards — only the due trigger differs. A
    // cycle must start here, or phase 1 proves nothing.
    trigger_guard.make_arena_trigger_due();
    assert!(
        crate::arena::arena_total_bytes() >= super::super::policy::next_arena_trigger_base(),
        "control phase must present a due whole-arena trigger"
    );

    let started = gc_runtime_safepoint();
    assert_eq!(
        started.status, JS_GC_STEP_STATUS_ACTIVE,
        "a whole-arena trigger IS discharge-able by a budgeted cycle and must still start one"
    );
    assert_eq!(
        super::super::instruments::incremental_cycle_starts(),
        starts_before + 1,
        "the control must start exactly one cycle"
    );
    assert_eq!(
        super::super::instruments::budgeted_step_nursery_cap_deferrals(),
        deferrals_before + 1,
        "the control must NOT be counted as a nursery-cap deferral"
    );

    // Leave the shared thread state clean.
    let completed = complete_host_safepoint_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    assert!(!gc_budgeted_cycle_active());
}

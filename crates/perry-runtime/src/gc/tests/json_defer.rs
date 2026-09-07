use super::*;
use crate::gc::tests::support::{
    register_runtime_handle_root_scanner_for_tests, CopyingNurseryTestGuard,
    GcTriggerThresholdTestGuard,
};
use crate::gc::*;

struct StateGuard {
    state: Deferral,
    pending: bool,
    old_pending: bool,
    old_baseline: usize,
    parse_pending: bool,
    bumped: bool,
    armed: bool,
    pre_suppress: usize,
}

impl StateGuard {
    fn new() -> Self {
        let guard = Self {
            state: JSON_DEFERRAL.with(|s| s.replace(Deferral::Available)),
            pending: GC_SAFEPOINT_PENDING.with(Cell::get),
            old_pending: GC_OLD_RECLAIM_PENDING.with(|c| c.replace(false)),
            old_baseline: GC_LAST_OLD_RECLAIM_IN_USE_BYTES
                .with(|c| c.replace(crate::arena::old_gen_in_use_bytes())),
            parse_pending: GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.replace(false)),
            bumped: GC_TRIGGER_BUMPED.with(Cell::get),
            armed: GC_TRIGGER_ARMED.with(|c| c.replace(true)),
            pre_suppress: GC_PRE_SUPPRESS_BYTES.with(Cell::get),
        };
        set_safepoint_pending(false);
        register_runtime_handle_root_scanner_for_tests();
        gc_register_mutable_root_scanner(json_parse_mutable_root_scanner);
        guard
    }
}

impl Drop for StateGuard {
    fn drop(&mut self) {
        JSON_DEFERRAL.with(|s| s.set(self.state));
        set_safepoint_pending(self.pending);
        GC_OLD_RECLAIM_PENDING.with(|c| c.set(self.old_pending));
        GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|c| c.set(self.old_baseline));
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|c| c.set(self.parse_pending));
        GC_TRIGGER_BUMPED.with(|c| c.set(self.bumped));
        GC_TRIGGER_ARMED.with(|c| c.set(self.armed));
        GC_PRE_SUPPRESS_BYTES.with(|c| c.set(self.pre_suppress));
    }
}

unsafe fn parse_records() -> crate::JSValue {
    let text = format!(
        "{{\"records\":[{}]}}",
        (0..1_000)
            .map(|i| format!(
                r#"{{"id":{i},"text":"child value {i} {}"}}"#,
                "x".repeat(300)
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let source = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    crate::json::js_json_parse(source)
}

unsafe fn first_child(value: crate::JSValue) -> crate::JSValue {
    let records = crate::object::js_object_get_field(value.as_pointer(), 0);
    crate::array::js_array_get(records.as_pointer(), 0)
}

#[test]
fn json_deferral_small_heap_allowance_scales_without_an_absolute_floor() {
    assert_eq!(extra_bytes_for_budget(None), 8 * 1024 * 1024);
    for mb in [1, 8, 32, 64, 256, 4096] {
        let budget = mb * 1024 * 1024;
        let allowance = extra_bytes_for_budget(Some(budget));
        assert!(allowance > 0 && allowance <= budget / 32);
        assert!(allowance <= 8 * 1024 * 1024);
    }
    assert_eq!(extra_bytes_for_budget(Some(0)), 0);
}

#[test]
fn json_deferral_declines_tiny_inputs_and_documents_larger_than_the_allowance() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _schedule = schedule::ScheduleGuard::off();
    let _moving = knob_overrides::ForcedEvacuationTestGuard::on();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let scope = RuntimeHandleScope::new();
    for source in [
        String::from("{}"),
        format!("{{\"text\":\"{}\"}}", "x".repeat(MAX_EXTRA_BYTES + 1)),
    ] {
        unsafe {
            let text = crate::js_string_from_bytes(source.as_ptr(), source.len() as u32);
            let value = crate::json::js_json_parse(text);
            let root = scope.root_nanbox_u64(value.bits());
            assert_eq!(JSON_DEFERRAL.with(Cell::get), Deferral::Available);
            let before = gc_total_collection_count();
            // Both routes must remain eligible for the ordinary first-poll
            // collection, and the returned object must survive actual motion.
            set_safepoint_pending(true);
            js_gc_loop_safepoint();
            assert!(gc_total_collection_count() > before);
            assert_ne!(root.get_nanbox_u64(), value.bits());
        }
    }
}

#[test]
fn json_deferral_returns_then_discards_siblings_before_actual_moving_collection() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _schedule = schedule::ScheduleGuard::off();
    let _moving = knob_overrides::ForcedEvacuationTestGuard::on();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let _trace = policy::TestGcTraceCaptureGuard::force_enabled();
    let scope = RuntimeHandleScope::new();
    unsafe {
        let before = gc_total_collection_count();
        let value = parse_records();
        assert_eq!(
            gc_total_collection_count(),
            before,
            "parse completion must return first"
        );
        assert!(
            matches!(JSON_DEFERRAL.with(Cell::get), Deferral::Waiting { .. }),
            "the actual JSON boundary must grant the allowance"
        );
        let parent = scope.root_nanbox_u64(value.bits());
        let child = scope.root_nanbox_u64(first_child(value).bits());
        let original = child.get_nanbox_u64();
        for _ in 0..MAX_SAFEPOINTS {
            js_gc_loop_safepoint();
            assert_eq!(gc_total_collection_count(), before);
            assert!(GC_SAFEPOINT_PENDING.with(Cell::get), "debt must stay armed");
        }
        parent.set_nanbox_u64(crate::JSValue::undefined().bits());
        // The one retained child stays live, but the other 999 records can die
        // before the collector ever sees them. The third poll MUST collect.
        reset_scan_fallback_counters();
        js_gc_loop_safepoint();
        assert!(gc_total_collection_count() > before);
        assert_eq!(safepoint_drain_count(SafepointDrainKind::NurseryMinor), 1);
        let trace = take_test_last_gc_trace_json().expect("the deferred collection must emit");
        let copied = trace["copying_nursery"]["copied_objects"].as_u64().unwrap();
        assert!(
            copied > 0 && copied < 100,
            "must copy the child, not 999 siblings: {copied}"
        );
        assert_ne!(
            child.get_nanbox_u64(),
            original,
            "retained child must actually move"
        );
        let child = crate::JSValue::from_bits(child.get_nanbox_u64());
        assert_eq!(
            crate::object::js_object_get_field(child.as_pointer(), 0).as_number(),
            0.0
        );
        assert_eq!(JSON_DEFERRAL.with(Cell::get), Deferral::Available);
    }
}

#[test]
fn json_deferral_repeated_parses_cannot_renew_bytes_or_poll_allowance() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _schedule = schedule::ScheduleGuard::off();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    unsafe {
        let _ = parse_records();
    }
    js_gc_loop_safepoint();
    let original = JSON_DEFERRAL.with(Cell::get);
    let before = gc_total_collection_count();
    for _ in 0..3 {
        unsafe {
            let _ = parse_records();
        }
        assert_eq!(JSON_DEFERRAL.with(Cell::get), original);
    }
    assert_eq!(gc_total_collection_count(), before);
    js_gc_loop_safepoint();
    js_gc_loop_safepoint();
    assert!(gc_total_collection_count() > before);
}

#[test]
fn json_deferral_byte_limit_forces_collection_before_poll_allowance_expires() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _schedule = schedule::ScheduleGuard::off();
    let _moving = knob_overrides::ForcedEvacuationTestGuard::on();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let scope = RuntimeHandleScope::new();
    let value = unsafe { parse_records() };
    let child = scope.root_nanbox_u64(unsafe { first_child(value) }.bits());
    let original = child.get_nanbox_u64();
    let before = gc_total_collection_count();
    // Real occupied space, not a mocked pressure reading. Suppressed windows
    // may cross the allowance; the very next precise poll must pay the debt.
    {
        let _suppressed = GcSuppressScope::new();
        let text = vec![b'x'; MAX_EXTRA_BYTES];
        crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    }
    assert!(matches!(
        JSON_DEFERRAL.with(Cell::get),
        Deferral::Waiting { polls_left: 2, .. }
    ));
    js_gc_loop_safepoint();
    assert!(gc_total_collection_count() > before);
    assert_ne!(child.get_nanbox_u64(), original);
}

#[test]
fn json_deferral_seeded_collection_bypasses_the_grace_period() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    let _schedule = schedule::ScheduleGuard::set(17, u64::MAX);
    let scope = RuntimeHandleScope::new();
    let value = unsafe { parse_records() };
    let root = scope.root_nanbox_u64(value.bits());
    let before = gc_total_collection_count();
    assert!(matches!(
        JSON_DEFERRAL.with(Cell::get),
        Deferral::Waiting { .. }
    ));
    js_gc_loop_safepoint();
    assert!(gc_total_collection_count() > before);
    assert_ne!(root.get_nanbox_u64(), value.bits());
}

#[test]
fn json_deferral_memory_warning_cancels_even_inside_an_unsafe_window() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _schedule = schedule::ScheduleGuard::off();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    unsafe {
        let _ = parse_records();
    }
    let before = gc_total_collection_count();
    {
        let _suppressed = GcSuppressScope::new();
        assert_eq!(js_gc_memory_pressure(2), 1);
    }
    assert_eq!(JSON_DEFERRAL.with(Cell::get), Deferral::Spent);
    assert!(!note_completed_parse(crate::arena::arena_in_use_bytes()));
    js_gc_loop_safepoint();
    assert!(gc_total_collection_count() > before);
}

#[test]
fn json_deferral_old_pressure_and_explicit_collection_are_not_delayed() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let _pacing = policy::force_moving_gc_pacing();
    let _cap = policy::ScavengeNurseryCapTestGuard::due_at_bytes(1);
    unsafe {
        let _ = parse_records();
    }
    let before = gc_total_collection_count();
    GC_OLD_RECLAIM_PENDING.with(|p| p.set(true));
    gc_check_trigger();
    assert!(
        gc_total_collection_count() > before,
        "old-pressure allocation fallback must run"
    );
    unsafe {
        let _ = parse_records();
    }
    let before = gc_total_collection_count();
    assert!(matches!(
        JSON_DEFERRAL.with(Cell::get),
        Deferral::Waiting { .. }
    ));
    gc_collect_minor();
    assert!(
        gc_total_collection_count() > before,
        "explicit collection must run"
    );
}

#[test]
fn json_deferral_completion_hook_never_services_post_parse_pressure() {
    let _isolation = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _state = StateGuard::new();
    let text = vec![b'x'; gc_tiny_parse_in_use_trigger_dyn_bytes()];
    {
        let _suppressed = GcSuppressScope::new();
        crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
    }
    // The tiny-pressure arm used to call gc_check_trigger here, which would
    // synchronously service this real old-gen debt before returning JSON.
    gc_suppress();
    gc_unsuppress();
    GC_OLD_RECLAIM_PENDING.with(|p| p.set(true));
    let before = gc_total_collection_count();
    gc_bump_json_malloc_trigger_deferred();
    assert_eq!(gc_total_collection_count(), before);
    assert!(GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(Cell::get));
    gc_check_trigger();
    assert!(
        gc_total_collection_count() > before,
        "debt must remain serviceable"
    );
}

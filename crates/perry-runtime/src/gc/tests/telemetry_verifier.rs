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

fn start_budgeted_cycle() {
    let mut result = JsGcStepResult::default();
    assert_eq!(
        js_gc_step_work_units(1, &mut result),
        JS_GC_STEP_STATUS_ACTIVE
    );
    assert_eq!(result.collection_kind, GcCollectionKind::Minor.ffi_code());
    assert_eq!(result.trigger_kind, GcTriggerKind::ArenaBytes.ffi_code());
}

fn complete_budgeted_cycle_trace() -> serde_json::Value {
    let completed = complete_budgeted_gc_cycle();
    assert_eq!(completed.status, JS_GC_STEP_STATUS_COMPLETED);
    take_test_last_gc_trace_json().expect("budgeted GC completion should emit test trace JSON")
}

fn verify_ordinary_pause_budget(event: &serde_json::Value) -> Result<(), String> {
    let soft_target = event["pause_budget"]["soft_pause_target_us"]
        .as_u64()
        .ok_or_else(|| "missing pause_budget.soft_pause_target_us".to_string())?;
    let steps = event["pause_steps"]
        .as_array()
        .ok_or_else(|| "missing pause_steps".to_string())?;
    if steps.is_empty() {
        return Err("ordinary cycle emitted no pause_steps".to_string());
    }
    for (index, step) in steps.iter().enumerate() {
        let include = step["budget"]["ordinary_pause_stats_include"]
            .as_bool()
            .unwrap_or(false);
        if !include {
            continue;
        }
        let elapsed = step["elapsed_pause_us"]
            .as_u64()
            .ok_or_else(|| format!("pause_steps[{index}] missing elapsed_pause_us"))?;
        if elapsed > soft_target {
            return Err(format!(
                "pause_steps[{index}] elapsed {elapsed}us exceeded soft target {soft_target}us"
            ));
        }
        if step["budget"]["within_soft_pause_target"].as_bool() != Some(true) {
            return Err(format!(
                "pause_steps[{index}] did not self-report within_soft_pause_target"
            ));
        }
    }
    Ok(())
}

fn assert_budgeted_ordinary_trace(event: &serde_json::Value, expected_kind: &str) {
    assert_eq!(
        event["progress_contract"]["kind"].as_str(),
        Some(expected_kind)
    );
    assert_eq!(
        event["progress_contract"]["class"].as_str(),
        Some("ordinary_budgeted")
    );
    assert_eq!(event["pause_budget"]["kind"].as_str(), Some(expected_kind));
    assert_eq!(
        event["pause_budget"]["class"].as_str(),
        Some("ordinary_budgeted")
    );
    assert_eq!(
        event["pause_budget"]["ordinary_pause_stats_include"].as_bool(),
        Some(true)
    );
    verify_ordinary_pause_budget(event).expect("ordinary pause steps should stay in budget");
}

fn assert_phase_progression_present(event: &serde_json::Value) {
    let phases = event["phase_progression"]
        .as_array()
        .expect("phase_progression should be an array");
    assert!(
        phases
            .iter()
            .any(|phase| phase.as_str() == Some("build_valid_pointer_set")),
        "phase_progression should include build_valid_pointer_set"
    );
    assert!(
        phases
            .iter()
            .any(|phase| phase.as_str() == Some("root_scan")),
        "phase_progression should include root_scan"
    );
    assert!(
        phases
            .iter()
            .any(|phase| phase.as_str() == Some("complete")),
        "phase_progression should include complete"
    );
}

#[test]
fn allocation_heavy_arena_debt_reports_budgeted_steps_and_debt() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"telemetry_arena_live");
    js_shadow_slot_set(0, string_bits(live));
    for _ in 0..512 {
        let _ = young_leaf();
    }
    trigger_guard.make_arena_trigger_due();

    start_budgeted_cycle();
    let event = complete_budgeted_cycle_trace();

    assert_budgeted_ordinary_trace(&event, "normal_incremental");
    assert_phase_progression_present(&event);
    assert!(
        event["debt"]["start"]["arena_debt_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "arena trigger should report arena debt at cycle start"
    );
    assert!(
        event["debt"]["max_observed"]["arena_debt_bytes"]
            .as_u64()
            .unwrap_or(0)
            >= event["debt"]["start"]["arena_debt_bytes"]
                .as_u64()
                .unwrap_or(0)
    );

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"telemetry_arena_live");
    }
}

#[test]
fn dirty_store_workload_reports_remembered_set_and_ordinary_pauses() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();
    let _ = take_write_barrier_trace_counters();

    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    js_shadow_slot_set(0, ptr_bits(old_obj as usize));
    let child = live_test_string(b"telemetry_dirty_child");
    runtime_store_jsvalue_slot(old_obj as usize, fields as usize, 0, string_bits(child));
    trigger_guard.make_arena_trigger_due();

    start_budgeted_cycle();
    let event = complete_budgeted_cycle_trace();

    assert_budgeted_ordinary_trace(&event, "normal_incremental");
    assert!(
        event["write_barrier"]["calls"].as_u64().unwrap_or(0) > 0,
        "trace should include write-barrier calls from dirty store workload"
    );
    assert!(
        event["remembered_set"]["dirty_slots_scanned"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "remembered-set scan should visit the dirty old-to-young slot"
    );
    unsafe {
        assert_eq!(*fields, string_bits(child));
    }
}

#[test]
fn root_heavy_workload_reports_root_sources_and_budgeted_progression() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let roots = 64_u32;
    let _guard = CopyingNurseryTestGuard::new(roots);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let first_live = live_test_string(b"telemetry_root_0");
    js_shadow_slot_set(0, string_bits(first_live));
    for slot in 1..roots {
        let root = young_leaf();
        js_shadow_slot_set(slot, string_bits(root));
    }
    trigger_guard.make_arena_trigger_due();

    start_budgeted_cycle();
    let event = complete_budgeted_cycle_trace();

    assert_budgeted_ordinary_trace(&event, "normal_incremental");
    assert_phase_progression_present(&event);
    assert!(
        event["root_sources"]["compiled_shadow"]["slots_scanned"]
            .as_u64()
            .unwrap_or(0)
            >= u64::from(roots),
        "root-source telemetry should include the installed shadow roots"
    );
    assert!(
        event["root_sources"]["compiled_shadow"]["pointer_roots"]
            .as_u64()
            .unwrap_or(0)
            >= u64::from(roots),
        "shadow-root telemetry should classify the roots as pointers"
    );

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"telemetry_root_0");
    }
}

#[test]
fn emergency_full_trace_is_excluded_from_ordinary_pause_stats() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_old_reclaim_pressure();

    let live = live_test_string(b"telemetry_emergency_live");
    js_shadow_slot_set(0, string_bits(live));
    let event = test_gc_collect_emergency_full_trace_json();

    assert_eq!(event["collection_kind"].as_str(), Some("full"));
    assert_eq!(event["trigger"]["kind"].as_str(), Some("emergency"));
    assert_eq!(
        event["progress_contract"]["kind"].as_str(),
        Some("emergency_full")
    );
    assert_eq!(event["pause_budget"]["class"].as_str(), Some("emergency"));
    assert_eq!(
        event["pause_budget"]["ordinary_pause_stats_include"].as_bool(),
        Some(false)
    );
    assert_eq!(
        event["pause_budget"]["ordinary_budgeted"].as_bool(),
        Some(false)
    );

    let steps = event["pause_steps"]
        .as_array()
        .expect("emergency full trace should include pause steps");
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|step| {
        step["budget"]["class"].as_str() == Some("emergency")
            && step["budget"]["ordinary_pause_stats_include"].as_bool() == Some(false)
    }));

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"telemetry_emergency_live");
    }

    drop(trigger_guard);
}

#[test]
fn verifier_rejects_over_budget_ordinary_step() {
    let event = serde_json::json!({
        "pause_budget": {
            "soft_pause_target_us": 10,
        },
        "pause_steps": [
            {
                "elapsed_pause_us": 11,
                "budget": {
                    "ordinary_pause_stats_include": true,
                    "within_soft_pause_target": false,
                },
            },
        ],
    });

    assert!(
        verify_ordinary_pause_budget(&event).is_err(),
        "synthetic over-budget ordinary step should fail verifier"
    );
}

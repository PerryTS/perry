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

/// Verify the ordinary-pause contract of a budgeted cycle's trace.
///
/// # Why this does not assert elapsed microseconds (#7956)
///
/// It used to: every included step had to satisfy
/// `elapsed_pause_us <= soft_pause_target_us`. That assertion failed ~2 runs in
/// 100 of `cargo test --release -p perry-runtime` on a loaded host — and it
/// failed for a reason no change to the GC could fix, because the quantity it
/// bounds is not one the code under test controls:
///
///   * `GcPauseBudget`'s own definition is "hard work-unit limit plus a **soft
///     pause target for telemetry**". `pause_us` is an annotation on the
///     trace, not a guarantee the stepper offers;
///   * `GcCycle::step` runs a phase for `budget.work_units` and measures
///     `elapsed` *afterwards*. No code path anywhere consults the clock to
///     decide when a step ends, so `elapsed <= pause_us` is not a
///     postcondition the collector can establish — it is a property of the
///     host;
///   * these fixtures drive the cycle with `js_gc_step_work_units(1, …)`, the
///     smallest step that exists. When one work unit takes 4.9 ms there is no
///     smaller step the pacer could have chosen, so the failure carries no
///     information about pacing at all.
///
/// The second arm was worse than uninformative: `within_soft_pause_target` is
/// computed in `pause_step_json` as `elapsed_us <= target`, so
/// "did not self-report within_soft_pause_target" could only fire when the
/// first check had already fired. Two assertions, one bit — the presence-check
/// shape, not a proof.
///
/// What replaces it is the part of the contract that IS deterministic: an
/// ordinary budgeted step is bounded in **work units** (the unit the trace
/// itself names as `budget_unit`), it is never labelled unbounded, its
/// self-report agrees with the numbers printed beside it, and the cycle-level
/// pause aggregate is the max of the per-step figures it summarises. Elapsed
/// microseconds stay in the trace and in these messages as a diagnostic; they
/// are no longer a verdict.
fn verify_ordinary_pause_budget(event: &serde_json::Value) -> Result<(), String> {
    let steps = event["pause_steps"]
        .as_array()
        .ok_or_else(|| "missing pause_steps".to_string())?;
    if steps.is_empty() {
        return Err("ordinary cycle emitted no pause_steps".to_string());
    }

    let mut included = 0usize;
    let mut max_elapsed = 0u64;
    for (index, step) in steps.iter().enumerate() {
        let elapsed = step["elapsed_pause_us"]
            .as_u64()
            .ok_or_else(|| format!("pause_steps[{index}] missing elapsed_pause_us"))?;
        max_elapsed = max_elapsed.max(elapsed);

        if !step["budget"]["ordinary_pause_stats_include"]
            .as_bool()
            .unwrap_or(false)
        {
            continue;
        }
        included += 1;

        // An ordinary step must be BOUNDED. A `null` work budget is how the
        // trace spells "this path is intentionally unbounded", and a step
        // carrying that label while counted in ordinary pause stats is the
        // real pacing defect the elapsed check was reaching for.
        let work_budget = step["budget"]["configured_work_budget"]
            .as_u64()
            .ok_or_else(|| {
                format!(
                    "pause_steps[{index}] is counted in ordinary pause stats but \
                     carries no configured_work_budget — an ordinary step must \
                     never run unbounded (elapsed {elapsed}us)"
                )
            })?;
        let applied = step["applied_work_units"].as_u64().ok_or_else(|| {
            format!("pause_steps[{index}] missing applied_work_units (elapsed {elapsed}us)")
        })?;
        if applied > work_budget {
            return Err(format!(
                "pause_steps[{index}] applied {applied} work units over a \
                 configured budget of {work_budget} (elapsed {elapsed}us)"
            ));
        }

        // The self-report must agree with the numbers reported beside it. This
        // is a coherence check on the telemetry, not a claim about the host:
        // it fires when the flag is computed against the wrong step or the
        // wrong progress kind's budget, and never because the box was busy.
        let soft_target = step["budget"]["soft_pause_target_us"]
            .as_u64()
            .ok_or_else(|| format!("pause_steps[{index}] missing soft_pause_target_us"))?;
        let self_report = step["budget"]["within_soft_pause_target"].as_bool();
        if self_report != Some(elapsed <= soft_target) {
            return Err(format!(
                "pause_steps[{index}] self-reported within_soft_pause_target = \
                 {self_report:?}, but elapsed {elapsed}us against a soft target \
                 of {soft_target}us says {}",
                elapsed <= soft_target
            ));
        }
    }

    if included == 0 {
        return Err(
            "ordinary cycle emitted no step counted in ordinary pause stats — \
             the pause budget under test never ran"
                .to_string(),
        );
    }

    // The cycle-level aggregate must summarise the very steps printed in the
    // same event (#7025's shape: a counter that sums something other than what
    // it names). Deterministic — both sides come out of `record_pause_step`.
    let reported_max = event["pause_budget"]["max_observed_step_pause_us"]
        .as_u64()
        .ok_or_else(|| "missing pause_budget.max_observed_step_pause_us".to_string())?;
    if reported_max != max_elapsed {
        return Err(format!(
            "pause_budget.max_observed_step_pause_us = {reported_max}us but the \
             maximum elapsed_pause_us over the {} reported steps is {max_elapsed}us",
            steps.len()
        ));
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
    let malloc_trim = &event["allocator_maintenance"]["malloc_trim"];
    // #6180 RSS floor: budgeted cycles now RUN allocator trim (previously
    // skipped with reason ordinary_budgeted). The outcome is platform-
    // dependent: executed on glibc, unsupported elsewhere — but never the
    // old budgeted skip.
    assert_ne!(malloc_trim["reason"].as_str(), Some("ordinary_budgeted"));
    assert!(matches!(
        malloc_trim["status"].as_str(),
        Some("executed") | Some("unsupported")
    ));
    assert_eq!(malloc_trim["progress_kind"].as_str(), Some(expected_kind));
    assert_eq!(malloc_trim["class"].as_str(), Some("ordinary_budgeted"));
    assert_eq!(
        malloc_trim["ordinary_pause_stats_include"].as_bool(),
        Some(false)
    );
    for (index, step) in event["pause_steps"]
        .as_array()
        .expect("ordinary trace should include pause steps")
        .iter()
        .enumerate()
    {
        assert!(
            step.get("allocator_maintenance").is_none(),
            "pause_steps[{index}] should not include allocator maintenance"
        );
    }
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
    assert!(
        event["root_sources"]["compiled_native"].is_object(),
        "native stack-map roots need their own source bucket"
    );
    assert!(
        event["root_sources"]["native_stack_maps"]["frames_visited"].is_number(),
        "native stack-map telemetry should expose unwinder work"
    );
    assert!(
        event["root_sources"]["native_stack_maps"]["fp_walks"].is_number()
            && event["root_sources"]["native_stack_maps"]["fallback_walks"].is_number(),
        "native stack-map telemetry should expose which walker ran"
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
    let malloc_trim = &event["allocator_maintenance"]["malloc_trim"];
    assert_eq!(
        malloc_trim["progress_kind"].as_str(),
        Some("emergency_full")
    );
    assert_eq!(malloc_trim["class"].as_str(), Some("emergency"));
    assert_eq!(
        malloc_trim["ordinary_pause_stats_include"].as_bool(),
        Some(false)
    );
    if cfg!(any(target_env = "gnu", target_os = "macos")) {
        // glibc malloc_trim / darwin malloc_zone_pressure_relief (#6180).
        assert_eq!(malloc_trim["status"].as_str(), Some("executed"));
        assert_eq!(
            malloc_trim["reason"].as_str(),
            Some("explicit_or_emergency")
        );
    } else {
        assert_eq!(malloc_trim["status"].as_str(), Some("unsupported"));
        assert_eq!(malloc_trim["reason"].as_str(), Some("not_supported"));
    }
    assert!(malloc_trim["elapsed_us"].as_u64().is_some());

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"telemetry_emergency_live");
    }

    drop(trigger_guard);
}

/// A well-formed ordinary trace, as a base for the sabotage cases below.
///
/// Deliberately over the soft pause target (`elapsed_pause_us` 4936 against a
/// 2000 us target — the figure from #7956's real failure), with a self-report
/// that says so. A slow host is not a defect, and this asserts that directly:
/// the shape that used to fail 2 runs in 100 must now VERIFY.
fn coherent_ordinary_trace() -> serde_json::Value {
    serde_json::json!({
        "pause_budget": {
            "soft_pause_target_us": 2000,
            "configured_work_budget": 64,
            "max_observed_step_pause_us": 4936,
        },
        "pause_steps": [
            {
                "applied_work_units": 1,
                "elapsed_pause_us": 4936,
                "budget": {
                    "configured_work_budget": 64,
                    "soft_pause_target_us": 2000,
                    "ordinary_pause_stats_include": true,
                    "within_soft_pause_target": false,
                },
            },
            {
                "applied_work_units": 1,
                "elapsed_pause_us": 12,
                "budget": {
                    "configured_work_budget": 64,
                    "soft_pause_target_us": 2000,
                    "ordinary_pause_stats_include": true,
                    "within_soft_pause_target": true,
                },
            },
        ],
    })
}

/// #7956: an over-target step on a loaded host is REPORTED, not failed. Kept as
/// a test rather than a comment so the decision cannot be reverted silently.
#[test]
fn verifier_accepts_a_slow_but_coherent_ordinary_step() {
    assert_eq!(
        verify_ordinary_pause_budget(&coherent_ordinary_trace()),
        Ok(())
    );
}

/// The work-unit budget is the HARD limit (`GcPauseBudget`'s own wording), so
/// a step that applied more work than it was granted is a real defect.
#[test]
fn verifier_rejects_a_step_over_its_work_budget() {
    let mut event = coherent_ordinary_trace();
    event["pause_steps"][1]["applied_work_units"] = serde_json::json!(65);
    assert!(
        verify_ordinary_pause_budget(&event)
            .unwrap_err()
            .contains("applied 65 work units"),
        "a step exceeding its configured work budget must fail the verifier"
    );
}

/// `null` is how the trace spells "unbounded". An ordinary budgeted step
/// carrying that label is the pacing defect the old elapsed check was aimed at.
#[test]
fn verifier_rejects_an_unbounded_ordinary_step() {
    let mut event = coherent_ordinary_trace();
    event["pause_steps"][0]["budget"]["configured_work_budget"] = serde_json::Value::Null;
    assert!(
        verify_ordinary_pause_budget(&event)
            .unwrap_err()
            .contains("must never run unbounded"),
        "an ordinary step with no work budget must fail the verifier"
    );
}

/// The self-report must track the numbers printed beside it — a flag computed
/// against the wrong step or the wrong progress kind's budget is a telemetry
/// bug that no timing threshold would catch.
#[test]
fn verifier_rejects_an_incoherent_pause_self_report() {
    let mut event = coherent_ordinary_trace();
    event["pause_steps"][0]["budget"]["within_soft_pause_target"] = serde_json::json!(true);
    assert!(
        verify_ordinary_pause_budget(&event)
            .unwrap_err()
            .contains("self-reported within_soft_pause_target"),
        "a self-report contradicting elapsed vs target must fail the verifier"
    );
}

/// #7025's shape: an aggregate that summarises something other than what it
/// names.
#[test]
fn verifier_rejects_a_pause_aggregate_that_misses_its_own_steps() {
    let mut event = coherent_ordinary_trace();
    event["pause_budget"]["max_observed_step_pause_us"] = serde_json::json!(12);
    assert!(
        verify_ordinary_pause_budget(&event)
            .unwrap_err()
            .contains("max_observed_step_pause_us"),
        "the cycle-level pause max must equal the max over the reported steps"
    );
}

/// The subject-was-live check: a trace whose every step is excluded from
/// ordinary pause stats proves nothing about the ordinary pause budget, so the
/// verifier must not report success for it.
#[test]
fn verifier_rejects_a_trace_with_no_ordinary_step() {
    let mut event = coherent_ordinary_trace();
    for index in 0..2 {
        event["pause_steps"][index]["budget"]["ordinary_pause_stats_include"] =
            serde_json::json!(false);
    }
    assert!(
        verify_ordinary_pause_budget(&event)
            .unwrap_err()
            .contains("never ran"),
        "a verifier that passes when its subject never ran is not a gate"
    );
}

// #6187: the always-on pause ring must track last/max/window coherently.
#[test]
fn test_pause_ring_records_max_and_window() {
    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        let baseline_count = stats.collection_count;
        stats.record_collection(10, 100);
        stats.record_collection(0, 900);
        stats.record_collection(5, 300);
        assert_eq!(stats.collection_count, baseline_count + 3);
        assert_eq!(stats.last_pause_us, 300);
        assert!(stats.max_pause_us >= 900);
        assert!(stats.recent_len >= 3);
    });
    // Overflow the ring: cursor wraps, len saturates at the window size.
    GC_STATS.with(|stats| {
        let mut stats = stats.borrow_mut();
        for i in 0..(GC_RECENT_PAUSE_WINDOW as u64 + 5) {
            stats.record_collection(0, i + 1);
        }
        assert_eq!(stats.recent_len as usize, GC_RECENT_PAUSE_WINDOW);
        assert!((stats.recent_cursor as usize) < GC_RECENT_PAUSE_WINDOW);
        assert_eq!(stats.last_pause_us, GC_RECENT_PAUSE_WINDOW as u64 + 5);
    });
    let mut max_us = 0u64;
    let mut recent_max = 0u64;
    let mut recent_avg = 0u64;
    let mut count = 0u64;
    js_gc_pause_stats(&mut max_us, &mut recent_max, &mut recent_avg, &mut count);
    assert_eq!(count as usize, GC_RECENT_PAUSE_WINDOW);
    assert!(max_us >= 900);
    assert!(recent_max >= GC_RECENT_PAUSE_WINDOW as u64);
    assert!(recent_avg > 0 && recent_avg <= recent_max);
}

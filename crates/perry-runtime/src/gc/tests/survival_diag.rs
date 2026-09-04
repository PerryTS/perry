//! `[gc-survival]` / `[gc-trigger]` / `[gc-full]` / `[gc-budgeted]` /
//! `[gc-charge]` (gc/survival_diag.rs, gc/diag_sites.rs): the attribution
//! instruments are validated against heaps and cycles of KNOWN shape before
//! they are pointed at anything real. Every assertion here can fail on the
//! instrument: a lost drain propagation charges elements to the drain phase,
//! a missed worklist mirror misaligns the origin vector, an unconsumed site
//! label mislabels the next full, an uncounted step leaves `steps` short.

use super::super::*;
use super::support::*;

/// A young array holding `N` young strings, rooted from ONE shadow-stack slot.
/// The elements are reachable only through the array, so their origin is the
/// array's — which is exactly the claim the parallel origin vector makes.
#[test]
fn survival_rows_charge_transitive_reach_to_the_originating_root() {
    let _diag = crate::gc::telemetry::GcDiagTestGuard::force_on();
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    const N: usize = 40;
    let mut arr = crate::array::js_array_alloc(N as u32);
    for _ in 0..N {
        let child = young_leaf();
        arr = crate::array::js_array_push_f64(arr, f64::from_bits(string_bits(child)));
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let moved =
        trace.copying_nursery.copied_objects as u64 + trace.copying_nursery.promoted_objects as u64;
    assert!(
        moved > N as u64,
        "subject must be live: the minor moved {moved} objects, expected at least {}",
        N + 1
    );

    let rows = super::super::survival_diag::test_last_report();
    assert!(
        !rows.is_empty(),
        "the diag was forced on, so the minor must have reported rows"
    );
    let attributed: u64 = rows.iter().map(|r| r.2).sum();
    assert_eq!(
        attributed, moved,
        "every moved object is attributed exactly once (origin vector aligned with the worklist)"
    );
    const SHADOW: &str = "mutable_root_slots/shadow_stack";
    let strings_via_shadow: u64 = rows
        .iter()
        .filter(|(o, t, ..)| o == SHADOW && *t == GC_TYPE_STRING)
        .map(|r| r.2)
        .sum();
    let arrays_via_shadow: u64 = rows
        .iter()
        .filter(|(o, t, ..)| o == SHADOW && *t == GC_TYPE_ARRAY)
        .map(|r| r.2)
        .sum();
    assert!(
        arrays_via_shadow >= 1,
        "the rooted array is charged to the shadow-stack root: rows={rows:?}"
    );
    assert!(
        strings_via_shadow >= N as u64,
        "the {N} elements reach the collector only through the array, so they are charged to \
         the array's origin, not to the drain: rows={rows:?}"
    );
    assert!(
        rows.iter().all(|(o, ..)| !o.contains("worklist_drain")),
        "transitive reach must never be charged to the drain phase: rows={rows:?}"
    );
}

#[test]
fn full_site_label_is_consumed_once_and_counted_per_site() {
    use super::super::diag_sites::*;
    let _diag = crate::gc::telemetry::GcDiagTestGuard::force_on();
    set_full_site("survival_diag_test_a");
    assert_eq!(take_full_site(), "survival_diag_test_a");
    assert_eq!(
        take_full_site(),
        "sync",
        "a label is consumed by the first full after it; the next full must not inherit it"
    );
    let before = test_full_site_count("survival_diag_test_b");
    full_started("survival_diag_test_b", GcTriggerKind::Manual);
    full_started("survival_diag_test_b", GcTriggerKind::OldGenBytes);
    assert_eq!(test_full_site_count("survival_diag_test_b"), before + 2);
    assert_eq!(test_full_site_count("survival_diag_test_never"), 0);
}

#[test]
fn budgeted_accounting_counts_steps_and_root_scan_time() {
    use super::super::diag_sites::*;
    let _diag = crate::gc::telemetry::GcDiagTestGuard::force_on();
    budgeted_started(
        GcTriggerKind::OldGenBytes,
        GcCollectionKind::Full,
        GcProgressKind::MutatorAssist,
    );
    // Phase codes follow `GcCyclePhase::ffi_code`: 2 = root scan, 6 = sweep.
    budgeted_step_done(2, 300, 16);
    budgeted_step_done(2, 200, 16);
    budgeted_step_done(6, 50, 16);
    budgeted_completed(4096);
    let (steps, step_us, units, root_us) =
        test_last_budgeted().expect("a completed cycle publishes its accounting");
    assert_eq!(steps, 3);
    assert_eq!(step_us, 550);
    assert_eq!(units, 48);
    assert_eq!(
        root_us, 500,
        "root-scan time is the sum of the steps taken in phase 2"
    );
}

#[test]
fn charge_probe_attributes_only_under_the_diag() {
    use super::super::diag_sites::*;
    {
        // Diag OFF: a probe is inert and records nothing.
        let probe = ChargeProbe::begin();
        probe.end(7, ChargeKind::Assist);
    }
    let _diag = crate::gc::telemetry::GcDiagTestGuard::force_on();
    report_charges("survival_diag_test_reset");
    assert!(
        test_charge_rows().is_empty(),
        "report_charges drains the table"
    );
    let probe = ChargeProbe::begin();
    probe.end(7, ChargeKind::SyncFull);
    let probe = ChargeProbe::begin();
    probe.end(3, ChargeKind::Assist);
    let rows = test_charge_rows();
    let calls: u64 = rows.iter().map(|r| r.0).sum();
    let units: u64 = rows.iter().map(|r| r.1).sum();
    let fulls: u64 = rows.iter().map(|r| r.3).sum();
    assert_eq!(calls, 2, "two probes ended under the diag: rows={rows:?}");
    assert_eq!(units, 10);
    assert_eq!(fulls, 1);
    report_charges("survival_diag_test_done");
}

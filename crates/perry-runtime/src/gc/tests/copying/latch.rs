//! The young-pin latch that lets the copying minor skip its eligibility
//! preflight (#7645).
//!
//! Two things need proving here, and the second is the one that matters.
//!
//! 1. **The optimisation happens.** A cycle with no young pin reports
//!    `preflight_skipped` and still copies objects. A gate that passes having
//!    run zero copying minors proves nothing (#7024/#7025), so every assertion
//!    below is paired with a liveness assertion on the same trace.
//! 2. **The guard it replaced still refuses.** A pin planted through
//!    `gc::pin_object` — the sanctioned path — still forces
//!    `PinnedYoungRoot`/`PinnedYoungDirtySlot`/`PinnedYoungTransitive`, and it
//!    does so *because the latch was armed*. Deleting the arming in
//!    `pin_object` turns `young_pin_via_pin_object_restores_the_walk` (and the
//!    three `test_copying_minor_falls_back_for_pinned_*` cases in
//!    `survival_and_malloc.rs`) red, which is what makes them worth having.
//!
//! Plus a third, cheaper line of defence, exercised by
//! [`raw_young_pin_that_bypasses_pin_object_aborts_the_copier`]: the collector
//! itself checks the pinned bit at the instant it is about to relocate an
//! object on a preflight-skipped cycle. That is the moment an incomplete latch
//! becomes a use-after-move, and the child process below plants exactly the
//! bug the static gate exists to prevent and asserts the collector dies rather
//! than corrupts.

use super::super::super::*;
use super::super::support::*;

/// Env var that unlocks the abort-child body. Belt and braces on top of
/// `#[ignore]`, so even `cargo test -- --include-ignored` cannot abort a
/// normal run.
const SABOTAGE_ENV: &str = "PERRY_TEST_PIN_LATCH_SABOTAGE";

fn preflight_skipped(trace: &GcCycleTrace) -> bool {
    trace.copying_nursery.preflight_skipped
}

/// A clean young graph: the walk is skipped, and the collector still ran.
#[test]
fn no_pin_ever_means_the_preflight_walks_are_skipped() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let skips_before = crate::gc::copied_minor_preflight_skips();
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        preflight_skipped(&trace),
        "with no young pin ever created, both preflight walks are provably no-ops"
    );
    assert_eq!(
        crate::gc::copied_minor_preflight_skips(),
        skips_before + 1,
        "the process-wide skip counter must move with the trace flag"
    );
    // Liveness: the subject actually ran. Without this the assertions above
    // would pass on a cycle that collected nothing.
    assert!(
        trace.copying_nursery.copied_objects > 0 || trace.copying_nursery.promoted_objects > 0,
        "copying minor must have moved something: {:?}/{:?}",
        trace.copying_nursery.copied_objects,
        trace.copying_nursery.promoted_objects
    );
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor, child, "the root must have been relocated");
}

/// The sanctioned pin path arms the latch, the walk comes back, and the walk
/// still refuses to move the pinned object.
///
/// **This is the test that goes red if the arming in `gc::pin_object` is
/// deleted**: without it the latch stays clear, the walk is skipped, and the
/// fallback never happens.
#[test]
fn young_pin_via_pin_object_restores_the_walk() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    // Baseline on the same heap shape: without the pin this cycle skips.
    let clean = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&clean, true, CopiedMinorFallbackReason::None, false);
    assert!(preflight_skipped(&clean));
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    unsafe {
        crate::gc::pin_object(header_from_user_ptr(survivor as *const u8));
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(
        !preflight_skipped(&trace),
        "a young pin must re-arm the latch and bring the walks back"
    );
    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungRoot,
        false,
    );
    assert_eq!(
        (js_shadow_slot_get(0) & POINTER_MASK) as usize,
        survivor,
        "a pinned young object must not move"
    );
    unsafe {
        crate::gc::unpin_object(header_from_user_ptr(survivor as *const u8));
    }
}

/// The latch is monotone: unpinning does not bring the fast path back.
/// Documented behaviour, not an accident — see `gc/pin.rs` on why a
/// decrementing counter was rejected.
#[test]
fn the_latch_is_monotone_across_an_unpin() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    unsafe {
        let header = header_from_user_ptr(child as *const u8);
        crate::gc::pin_object(header);
        crate::gc::unpin_object(header);
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        !preflight_skipped(&trace),
        "the latch stays armed after an unpin — one completeness proof, not two"
    );
}

/// A `Longlived` pin does not arm the latch.
///
/// This is what keeps `string/format.rs`'s `SMALL_INT_CACHE` free: it pins
/// every cached small-integer string, and stringifying `0` is common enough
/// that arming on it would disable the optimisation for essentially every
/// program. `CopyingNurseryPreflight::check_ptr_with_reason` never trips on
/// `Longlived`, so those pins genuinely cannot constrain the copying minor.
#[test]
fn a_longlived_pin_does_not_arm_the_latch() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let bytes = b"longlived_pin_probe";
    let longlived =
        crate::string::js_string_from_bytes_longlived(bytes.as_ptr(), bytes.len() as u32);
    assert!(
        matches!(
            crate::arena::classify_heap_space(unsafe {
                header_from_user_ptr(longlived as *const u8)
            } as usize),
            crate::arena::HeapSpace::Longlived
        ),
        "probe precondition: js_string_from_bytes_longlived must land in Longlived"
    );
    unsafe {
        crate::gc::pin_object(header_from_user_ptr(longlived as *const u8));
    }

    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        preflight_skipped(&trace),
        "a Longlived pin must not arm the young-pin latch"
    );
    unsafe {
        crate::gc::unpin_object(header_from_user_ptr(longlived as *const u8));
    }
}

/// A malloc-space pin does not arm the latch — `spawn`'s cross-thread promise
/// is deliberately allocated there (`thread.rs`) precisely because the copying
/// minor never relocates it.
#[test]
fn a_malloc_pin_does_not_arm_the_latch() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let malloced = crate::gc::gc_malloc(64, GC_TYPE_OBJECT);
    unsafe {
        let header = header_from_user_ptr(malloced as *const u8);
        assert_eq!(
            (*header).gc_flags & GC_FLAG_ARENA,
            0,
            "probe precondition: gc_malloc must not carry GC_FLAG_ARENA"
        );
        crate::gc::pin_object(header);
    }

    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        preflight_skipped(&trace),
        "a malloc-space pin must not arm the young-pin latch"
    );
    unsafe {
        crate::gc::unpin_object(header_from_user_ptr(malloced as *const u8));
    }
}

/// SABOTAGE. Plant the exact bug `scripts/gc_pin_sites.py` exists to prevent —
/// a young pin created by a raw flag write that never tells the latch — and
/// require the collector to abort rather than relocate it.
///
/// The body runs in a child process because it ends in `std::process::abort`,
/// which no in-process harness can catch. A green run here means the second
/// line of defence was exercised and fired; if the child exits cleanly the
/// guard is inert and this test fails.
#[test]
#[cfg(unix)]
fn raw_young_pin_that_bypasses_pin_object_aborts_the_copier() {
    use std::os::unix::process::ExitStatusExt;

    if std::env::var_os(SABOTAGE_ENV).is_some() {
        // We are the child; the #[ignore]d body below does the work.
        return;
    }
    let exe = std::env::current_exe().expect("test binary path");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "gc::tests::copying::latch::pin_latch_sabotage_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SABOTAGE_ENV, "1")
        .output()
        .expect("spawn sabotage child");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.signal(),
        Some(libc::SIGABRT),
        "the copier must abort when it meets a pinned young object on a \
         preflight-skipped cycle. status={:?} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    assert!(
        stderr.contains("[gc-pin-latch] FATAL"),
        "the abort must name the invariant it caught. stderr={stderr}"
    );
}

/// Child body for [`raw_young_pin_that_bypasses_pin_object_aborts_the_copier`].
/// Never runs in a normal `cargo test`: it is `#[ignore]`d *and* gated on
/// [`SABOTAGE_ENV`].
#[test]
#[ignore = "aborts the process on purpose; driven by raw_young_pin_that_bypasses_pin_object_aborts_the_copier"]
fn pin_latch_sabotage_child() {
    if std::env::var_os(SABOTAGE_ENV).is_none() {
        return;
    }
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));
    unsafe {
        // The bug, planted verbatim: a young pin that never arms the latch.
        // Allowlisted in scripts/gc_pin_sites.py by this binding's name — it
        // must stay a raw write or the sabotage tests nothing.
        let sabotage_plant_7645 = header_from_user_ptr(child as *const u8);
        (*sabotage_plant_7645).gc_flags |= GC_FLAG_PINNED;
    }
    let _ = collect_minor_trace(GcTriggerKind::Direct);
    panic!("copying minor relocated a pinned young object without aborting");
}

/// #7650 follow-up: every `pin_object_non_young` call site must really be
/// non-young, because that variant deliberately skips the space classifier and
/// therefore leaves the latch disarmed.
///
/// The variant exists for a LINK reason, not a GC one — `pin_object` reaches
/// `arena::classify_heap_space`, and that edge kept a reference chain alive
/// that `-Wl,-dead_strip` had been removing, breaking five `perry-ext-*` crates
/// with `Undefined symbols: _js_blob_new, _js_fetch_with_options, …` (#7650,
/// bisected). The full workspace test is tag/nightly-only, so a per-PR run
/// cannot see it. The safety property is therefore asserted here rather than at
/// the call, and it is only as good as this list: **add a case when you add a
/// caller.**
///
/// Callers as of this change:
/// * `string/format.rs` — the interned format buffer, allocated long-lived.
/// * `thread.rs` (x2) — the spawn promise and its handle, malloc-resident, so
///   they carry no `GC_FLAG_ARENA` and the predicate short-circuits.
#[test]
fn pin_object_non_young_call_sites_are_never_young() {
    let _guard = CopyingNurseryTestGuard::new(1);

    unsafe {
        // `string/format.rs` pins a LONG-LIVED allocation.
        let longlived = crate::string::js_string_from_bytes_longlived(b"x".as_ptr(), 1);
        let ll_header = header_from_user_ptr(longlived as *const u8) as *mut GcHeader;
        assert!(
            !crate::gc::pin::pin_constrains_copying_minor_for_tests(ll_header),
            "format.rs pins a long-lived string; if that allocation ever moved \
             to Eden, pin_object_non_young there would become memory corruption"
        );

        // `thread.rs` pins MALLOC-resident promise state: no GC_FLAG_ARENA, so
        // the predicate short-circuits before any space classification.
        let mut synthetic = std::ptr::read(ll_header);
        synthetic.gc_flags &= !crate::gc::GC_FLAG_ARENA;
        assert!(
            !crate::gc::pin::pin_constrains_copying_minor_for_tests(&mut synthetic),
            "a header without GC_FLAG_ARENA is malloc space and can never be \
             Eden/FromSurvivor"
        );

        // Control: a plain nursery object IS young, so the predicate the two
        // assertions above rely on is not vacuously false for everything.
        let young = young_leaf();
        let y_header = header_from_user_ptr(young as *const u8) as *mut GcHeader;
        assert!(
            crate::gc::pin::pin_constrains_copying_minor_for_tests(y_header),
            "control: a nursery object must be reported young, or this test \
             proves nothing about the two assertions above"
        );
    }
}

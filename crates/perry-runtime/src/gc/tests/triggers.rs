use super::super::*;
#[allow(unused_imports)]
use super::support::*;

struct GcBumpTriggerTestGuard {
    next_arena_trigger: usize,
    arena_step: usize,
    next_malloc_trigger: usize,
    malloc_step: usize,
    trigger_bumped: bool,
    pre_suppress_bytes: usize,
    suppressed_parse_pending: bool,
}

impl GcBumpTriggerTestGuard {
    fn new(next_arena_trigger: usize, arena_step: usize) -> Self {
        let previous = Self {
            next_arena_trigger: GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                let previous = trigger.get();
                trigger.set(next_arena_trigger);
                previous
            }),
            arena_step: GC_STEP_BYTES.with(|step| {
                let previous = step.get();
                step.set(arena_step);
                previous
            }),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
                let previous = trigger.get();
                trigger.set(usize::MAX);
                previous
            }),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|step| step.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|bumped| {
                let previous = bumped.get();
                bumped.set(false);
                previous
            }),
            pre_suppress_bytes: GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.get()),
            suppressed_parse_pending: GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| {
                let previous = pending.get();
                pending.set(false);
                previous
            }),
        };
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(0));
        previous
    }

    fn set_pre_suppress(bytes: usize) {
        GC_PRE_SUPPRESS_BYTES.with(|pre| pre.set(bytes));
    }

    fn next_arena_trigger() -> usize {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get())
    }

    fn trigger_bumped() -> bool {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.get())
    }

    fn reset_cycle_bump() {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(false));
    }
}

impl Drop for GcBumpTriggerTestGuard {
    fn drop(&mut self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
        GC_STEP_BYTES.with(|step| step.set(self.arena_step));
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
        GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(self.trigger_bumped));
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(self.pre_suppress_bytes));
        GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING
            .with(|pending| pending.set(self.suppressed_parse_pending));
    }
}

fn reset_old_reclaim_pressure() {
    let old_in_use = crate::arena::old_gen_in_use_bytes();
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(old_in_use));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(false));
}

#[test]
fn test_gc_bump_tiny_parse_caps_arena_trigger_at_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(
        !GcBumpTriggerTestGuard::trigger_bumped(),
        "tiny parses must not consume the medium/large per-cycle bump"
    );
}

#[test]
fn test_gc_bump_repeated_tiny_parses_cannot_exceed_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(
        GC_TRIGGER_ABSOLUTE_CEILING - (2 * 1024 * 1024),
        GC_THRESHOLD_INITIAL_BYTES,
    );

    let first_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(first_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );

    let later_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(later_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
}

#[test]
fn test_gc_bump_one_block_parse_uses_tiny_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + GC_SUPPRESSED_TINY_PARSE_BYTES;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now - GC_SUPPRESSED_TINY_PARSE_BYTES);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_medium_parse_allows_one_arena_bump_per_gc_cycle() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let first_bytes_now = 2 * GC_SUPPRESSED_TINY_PARSE_BYTES;
    let first_expected = first_bytes_now + GC_THRESHOLD_INITIAL_BYTES;

    GcBumpTriggerTestGuard::set_pre_suppress(0);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(GcBumpTriggerTestGuard::next_arena_trigger(), first_expected);
    assert!(GcBumpTriggerTestGuard::trigger_bumped());

    let later_bytes_now = first_expected + (16 * 1024 * 1024);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        first_expected,
        "second medium/large bump in the same cycle must be ignored"
    );

    GcBumpTriggerTestGuard::reset_cycle_bump();
    let second_expected = later_bytes_now + GC_THRESHOLD_INITIAL_BYTES;
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        second_expected
    );
    assert!(GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_never_lowers_existing_arena_trigger() {
    let existing_trigger = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    let _guard = GcBumpTriggerTestGuard::new(existing_trigger, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (16 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        existing_trigger
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_pending_tiny_parse_boundary_collection_completes_and_rebaselines() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let _copying_guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _bump_guard = GcBumpTriggerTestGuard::new(usize::MAX, GC_THRESHOLD_INITIAL_BYTES);
    reset_old_reclaim_pressure();

    let live = crate::string::js_string_from_bytes(b"parse-boundary-live".as_ptr(), 19) as usize;
    js_shadow_slot_set(0, string_bits(live));

    while crate::arena::arena_in_use_bytes()
        < GC_SUPPRESSED_TINY_PARSE_IN_USE_TRIGGER_BYTES + (2 * 1024 * 1024)
    {
        let _ = crate::arena::arena_alloc_gc(8 * 1024, 8, GC_TYPE_STRING);
    }
    let pre_in_use = crate::arena::arena_in_use_bytes();
    let before = gc_collection_count();

    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
    gc_collect_pending_suppressed_parse();

    assert_eq!(gc_collection_count(), before + 1);
    assert!(
        crate::arena::arena_in_use_bytes() < pre_in_use / 8,
        "parse-boundary collection should reclaim dead tiny-parse churn"
    );
    assert!(
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get()) > crate::arena::arena_total_bytes(),
        "completed boundary collection should rebaseline the arena trigger"
    );

    let event =
        take_test_last_gc_trace_json().expect("parse-boundary collection should emit trace JSON");
    assert_eq!(event["collection_kind"].as_str(), Some("minor"));
    assert_eq!(event["trigger"]["kind"].as_str(), Some("arena_bytes"));

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::StringHeader;
    unsafe {
        assert_string_bytes(live_after, b"parse-boundary-live");
    }
}

#[test]
fn test_pending_parse_boundary_prioritizes_old_reclaim_pressure() {
    let _trace_guard = TestGcTraceCaptureGuard::force_enabled();
    let _copying_guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _bump_guard = GcBumpTriggerTestGuard::new(usize::MAX, GC_THRESHOLD_INITIAL_BYTES);

    let live_old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    js_shadow_slot_set(0, ptr_bits(live_old));
    GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.set(0));
    GC_OLD_RECLAIM_PENDING.with(|pending| pending.set(true));

    let mut dead_bytes = 0usize;
    let dead_chunks = (GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES / (1024 * 1024)) + 2;
    for _ in 0..dead_chunks {
        let dead = crate::arena::arena_alloc_gc_old(1024 * 1024, 8, GC_TYPE_STRING) as usize;
        let (_header, total) = old_test_header_and_size(dead);
        dead_bytes = dead_bytes.saturating_add(total);
    }
    let old_before = crate::arena::old_gen_in_use_bytes();
    let before = gc_collection_count();

    GC_SUPPRESSED_TINY_PARSE_COLLECTION_PENDING.with(|pending| pending.set(true));
    gc_collect_pending_suppressed_parse();

    assert_eq!(gc_collection_count(), before + 1);
    assert!(
        crate::arena::old_gen_in_use_bytes() < old_before / 4,
        "parse-boundary old reclaim should reclaim dead old JSON pressure"
    );
    assert!(
        GC_LAST_OLD_RECLAIM_IN_USE_BYTES.with(|bytes| bytes.get())
            <= crate::arena::old_gen_in_use_bytes(),
        "completed old reclaim should rebaseline old-generation pressure"
    );
    assert!(dead_bytes >= GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES);

    let event =
        take_test_last_gc_trace_json().expect("parse-boundary old reclaim should emit trace JSON");
    assert_eq!(event["collection_kind"].as_str(), Some("full"));
    assert_eq!(event["trigger"]["kind"].as_str(), Some("old_gen_bytes"));

    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(live_after, live_old);
    assert!(crate::arena::pointer_in_old_gen(live_after));
}

#[test]
fn test_old_reclaim_pressure_uses_threshold_and_growth() {
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
        GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
    ));
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
}

#[test]
fn test_copying_minor_promotion_handoff_uses_predicted_old_pressure() {
    assert!(!copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES - 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        0,
    ));
    assert!(copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
        0,
    ));
    assert!(copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        20 * 1024 * 1024,
        8 * 1024 * 1024,
    ));
    assert!(!copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        20 * 1024 * 1024,
        20 * 1024 * 1024,
    ));
}

//! #10698: `gc_check_trigger`'s "nothing is due" watermark.
//!
//! Every fast-path answer in the unit suite is already re-derived by the full
//! ladder (`trigger_watermark::verify_nothing_due`), so a watermark that
//! outlives an input change fails whichever test drove it. These tests cover
//! what that check cannot: that the fast path is actually TAKEN on the path it
//! was built for (a green verifier over zero hits proves nothing), that each
//! bound is the ladder's own threshold, that each class of input change
//! retires it, and that the verifier itself can fail.

use super::super::policy::{
    force_moving_gc_pacing, gc_budgeted_due_trigger, BudgetedGcTrigger,
    ScavengeNurseryCapTestGuard, GC_NEXT_MALLOC_TRIGGER, GC_NEXT_TRIGGER_BYTES,
};
use super::super::trigger_watermark::{
    nothing_due, publish_trigger_watermark, published_trigger_watermark,
    trigger_watermark_fast_path_hits, TriggerWatermark,
};
use super::super::*;
use super::support::*;

/// Evaluate the ladder with nothing due and return what it published.
fn publish() -> (usize, usize) {
    gc_check_trigger();
    published_trigger_watermark()
        .expect("fixture: an evaluation that finds nothing due must publish a watermark")
}

/// The steady state the watermark exists for: `gc_malloc` after `gc_malloc`
/// with nothing due. After the first evaluation publishes, every later
/// trigger check must be answered by the fast path — and, by the verifier on
/// that path, answered identically to the ladder.
#[test]
fn repeated_gc_malloc_is_answered_by_the_fast_path() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = crate::arena::js_inline_arena_state();
    publish();

    let before = trigger_watermark_fast_path_hits();
    const CALLS: u64 = 1_000;
    for _ in 0..CALLS {
        std::hint::black_box(gc_malloc(16, GC_TYPE_STRING));
    }
    let hits = trigger_watermark_fast_path_hits() - before;
    assert_eq!(
        hits, CALLS,
        "every gc_malloc with nothing due must take the fast path; something on \
         the gc_malloc path is retiring the watermark it just published"
    );
}

/// The malloc bound is the malloc-count arm's threshold: the fast path
/// declines exactly when the registry reaches it, and the ladder then agrees
/// that the arm is due.
#[test]
fn malloc_limit_is_the_malloc_arm_threshold() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = crate::arena::js_inline_arena_state();
    let count = malloc_object_count();
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(count + 1));

    let (malloc_limit, _) = publish();
    assert_eq!(malloc_limit, count + 1);
    assert!(nothing_due(), "one object below the threshold is not due");

    // `gc_malloc` checks BEFORE it registers, so this call is answered at
    // `count` and leaves the registry at the threshold.
    std::hint::black_box(gc_malloc(16, GC_TYPE_STRING));
    assert_eq!(malloc_object_count(), count + 1);
    assert!(
        !nothing_due(),
        "a registry at the threshold must fall through to the ladder"
    );
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::MallocCount),
        "and the ladder must agree the arm is due"
    );
}

/// The young bound is the scavenge cap less the sealed young bytes, expressed
/// as the inline allocator's offset: the fast path declines exactly when the
/// young generation reaches the cap.
#[test]
fn young_offset_limit_is_the_cap_less_the_sealed_bytes() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let inline = crate::arena::js_inline_arena_state();
    let filler = [b'y'; 64];
    crate::string::js_string_from_bytes(filler.as_ptr(), filler.len() as u32);

    let in_use = crate::arena::copying_from_space_in_use_bytes();
    // SAFETY: this thread's inline state, read only.
    let offset = unsafe { (*inline).offset };
    let sealed = in_use - offset;
    let headroom = 4096;
    let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(in_use + headroom);

    let (_, young_offset_limit) = publish();
    assert_eq!(
        young_offset_limit,
        in_use + headroom - sealed,
        "the bound must be the cap re-expressed as a current-block offset"
    );
    assert!(nothing_due());

    // Fill the headroom through the runtime Eden path, which advances the
    // inline offset without retiring anything -- unless the block fills, and
    // a block switch is supposed to retire it, so re-evaluate then.
    while crate::arena::copying_from_space_in_use_bytes() < in_use + headroom {
        if published_trigger_watermark().is_none() {
            publish();
        }
        assert!(
            nothing_due(),
            "below the cap the fast path must still answer"
        );
        crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING);
    }
    assert!(
        !nothing_due(),
        "a young generation at the cap must fall through to the ladder"
    );
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::YoungScavengeCap)
    );
}

/// Each class of input change retires the watermark: a `TriggerInput`
/// write, a heap-generation advance, an Eden block switch and a root lock.
#[test]
fn every_class_of_input_change_retires_the_watermark() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = crate::arena::js_inline_arena_state();

    publish();
    // Even a write of the value already there: the type does not compare.
    GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(trigger.get()));
    assert_eq!(published_trigger_watermark(), None, "TriggerInput write");

    publish();
    drop(heap_generation::HeapChange::begin(
        heap_generation::HeapChangeKind::Sweep,
    ));
    assert_eq!(published_trigger_watermark(), None, "heap change");

    publish();
    // SAFETY: this thread's nursery arena; re-selecting the current block is
    // the smallest block switch there is.
    unsafe {
        let arena = &mut *crate::arena::hot_arena();
        let current = arena.current;
        arena.set_current(current);
    }
    assert_eq!(published_trigger_watermark(), None, "Eden block switch");

    publish();
    super::super::roots::enter_gc_root_lock();
    assert_eq!(published_trigger_watermark(), None, "root lock");
    // Under the lock the check must reach the ladder's deferral, not the
    // fast path, and must not publish.
    let hits = trigger_watermark_fast_path_hits();
    gc_check_trigger();
    assert_eq!(trigger_watermark_fast_path_hits(), hits);
    assert_eq!(published_trigger_watermark(), None);
    super::super::roots::exit_gc_root_lock();
}

/// The verifier is what makes the rest of the suite a check on this
/// mechanism, so it must be able to fail: a watermark published while a
/// trigger is due has to trip it on the very next check.
#[test]
#[should_panic(expected = "#10698")]
fn a_watermark_the_ladder_disagrees_with_trips_the_verifier() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = crate::arena::js_inline_arena_state();
    triggers.make_arena_trigger_due();
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::ArenaBytes),
        "fixture: the arena arm must be due"
    );
    publish_trigger_watermark(TriggerWatermark::for_test(usize::MAX, usize::MAX));
    gc_check_trigger();
}

/// Fixed young-leaf bytes reduce the small-allocation headroom even without
/// another leaf birth to invalidate the cached trigger answer.
#[test]
fn small_allocation_crosses_the_leaf_window_before_the_copying_cap() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let inline = crate::arena::js_inline_arena_state();
    let leaf = crate::arena::arena_alloc_gc(LARGE_OBJECT_THRESHOLD_BYTES + 8, 8, GC_TYPE_STRING);
    std::hint::black_box(leaf);
    let leaves = crate::arena::young_large_in_use_bytes();
    assert!(
        leaves > 0,
        "fixture must exercise dedicated young ownership"
    );
    assert_eq!(crate::arena::copying_active_survivor_in_use_bytes(), 0);
    let small = crate::arena::copying_from_space_in_use_bytes();
    let cap = small + leaves + 4096;
    let _cap = ScavengeNurseryCapTestGuard::due_at_bytes(cap);
    let sealed = small - unsafe { (*inline).offset };
    let (_, limit) = publish();
    assert_eq!(limit, cap - leaves - sealed);
    let before = trigger_watermark_fast_path_hits();
    while crate::arena::copying_from_space_in_use_bytes() + leaves < cap {
        assert!(nothing_due());
        gc_check_trigger(); // The existing verifier checks the full ladder too.
        crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING);
    }
    assert!(trigger_watermark_fast_path_hits() > before);
    assert!(crate::arena::copying_from_space_in_use_bytes() < cap);
    assert_eq!(crate::arena::young_large_in_use_bytes(), leaves);
    assert!(
        !nothing_due(),
        "the residual window must reach the full ladder"
    );
    assert_eq!(
        gc_budgeted_due_trigger(),
        Some(BudgetedGcTrigger::YoungScavengeCap)
    );
}

/// The Supabase `--version` shape: a 3.17 MB survivor cohort under a
/// 14.45 MB cap, from-space 0.2 MB short of the cap, one 64 KB leaf. The
/// copying arm is not due, so a leaf service runs; it can only retire or
/// promote the leaf. The leaf arm must therefore be false once the leaves are
/// gone, or every later leaf birth repeats a census that retires nothing
/// (measured: four in a row, 48 M instructions each, 0 bytes retired).
#[test]
fn the_leaf_arm_does_not_count_the_survivor_space_twice() {
    use crate::gc::policy::leaf_window_due;
    let cap = 14_445_182;
    let survivor = 3_173_624;
    let small = cap - 200_000;
    let leaf = 64 * 1024;
    assert!(small < cap, "fixture: the copying arm is not due");
    assert!(
        !leaf_window_due(cap, small, survivor, leaf).0,
        "Eden plus the leaf is below the Eden window: nothing for a leaf service to do"
    );
    // After a service that retired or promoted every leaf, a reading just
    // under the copying cap is never due through the leaf arm.
    for survivor in [0, 1 << 20, survivor, cap / 2] {
        let small = cap - 2;
        let (due, limit) = leaf_window_due(cap, small, survivor, 1);
        assert!(
            !due,
            "survivor {survivor}: the leaf arm fires below the copying cap"
        );
        assert_eq!(due, small >= limit, "the bound and the answer agree");
    }
    // Leaves that do fill Eden's window still make it due.
    let window = crate::gc::tenuring::eden_allocation_window_bytes(cap, survivor);
    let eden = small - survivor;
    assert!(leaf_window_due(cap, small, survivor, window - eden).0);
}

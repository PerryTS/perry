use super::super::*;
use super::support::*;

fn seed_one_pooled_block() {
    let layout = std::alloc::Layout::from_size_align(crate::arena::BLOCK_SIZE, 16).unwrap();
    let raw = unsafe { std::alloc::alloc(layout) };
    assert!(!raw.is_null(), "test block allocation must succeed");
    assert!(
        crate::arena::block_pool_put(raw, crate::arena::BLOCK_SIZE),
        "a fresh current-thread pool must accept one block"
    );
}

#[test]
fn critical_pressure_drains_the_current_thread_block_pool() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    seed_one_pooled_block();
    let drained_before = crate::arena::block_pool_explicit_drained_bytes_for_test();
    assert!(
        crate::arena::block_pool_bytes_for_test() >= crate::arena::BLOCK_SIZE,
        "LIVE SUBJECT: the pool contains retained capacity before pressure"
    );

    assert_eq!(
        js_gc_memory_pressure(2),
        2,
        "safe critical pressure collects"
    );
    assert_eq!(
        crate::arena::block_pool_bytes_for_test(),
        0,
        "critical pressure must return every current-thread pooled block"
    );
    assert!(
        crate::arena::block_pool_explicit_drained_bytes_for_test()
            >= drained_before.saturating_add(crate::arena::BLOCK_SIZE),
        "drain telemetry must count the retained mapping actually released"
    );
}

#[test]
fn deferred_critical_pressure_drains_after_the_owed_full_cycle() {
    let _isolation = GcTestIsolationGuard::new();
    let _pacing = crate::gc::policy::force_moving_gc_pacing();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_shadow_stack();

    seed_one_pooled_block();
    let drained_before = crate::arena::block_pool_explicit_drained_bytes_for_test();
    let frame = js_shadow_frame_push(1);
    assert_eq!(
        js_gc_memory_pressure(2),
        1,
        "a live generated frame defers critical pressure"
    );
    assert!(
        crate::arena::block_pool_bytes_for_test() >= crate::arena::BLOCK_SIZE,
        "the pool must remain owned until the full cycle actually completes"
    );
    js_shadow_frame_pop(frame);

    js_gc_loop_safepoint();
    assert_eq!(
        crate::arena::block_pool_bytes_for_test(),
        0,
        "the deferred full-cycle publication must consume the sticky drain"
    );
    assert!(
        crate::arena::block_pool_explicit_drained_bytes_for_test()
            >= drained_before.saturating_add(crate::arena::BLOCK_SIZE),
        "honest drain telemetry must include the seeded retained block"
    );
}

#[test]
fn blocked_critical_pressure_keeps_the_full_cycle_and_drain_sticky() {
    let _isolation = GcTestIsolationGuard::new();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    seed_one_pooled_block();

    let previous_flags = GC_FLAGS.with(|flags| {
        let previous = flags.get();
        flags.set(previous | GC_FLAG_SUPPRESSED);
        previous
    });
    let result = js_gc_memory_pressure(2);
    GC_FLAGS.with(|flags| flags.set(previous_flags));

    assert_eq!(
        result, 1,
        "suppressed allocation bookkeeping defers pressure"
    );
    assert!(
        GC_OLD_RECLAIM_PENDING.with(std::cell::Cell::get),
        "critical pressure must retain the full-cycle debt across every guard"
    );
    assert!(crate::arena::block_pool_bytes_for_test() >= crate::arena::BLOCK_SIZE);

    gc_check_trigger();
    assert_eq!(
        crate::arena::block_pool_bytes_for_test(),
        0,
        "the allocation-point full-cycle backstop must consume the drain"
    );
}

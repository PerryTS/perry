//! Promise side-table GC-death cleanup (2026-07-09 GC audit, wave 2 batch A).
//!
//! `PROMISE_SETTLE_LISTENERS`, `PROMISE_OVERFLOW_REACTIONS`, and
//! `PROMISE_ALL_STATES` key entries by promise address but root the parked
//! closures/result machinery strongly, pruning only at settle — so an
//! abandoned never-settling promise leaked everything it captured, forever.
//! They now mirror the `PROMISE_CONTEXTS` reference lifecycle: the
//! `PromiseCleanup` finalize arm drops a dead promise's entries in sweeping
//! collections, and the copied-minor from-space pass drops/rekeys entries via
//! the shared `copied_minor_promise_key_fate` classifier.

use super::*;

/// A DEAD (unrooted) pending promise's entries in all three side tables must
/// be dropped by the copied-minor from-space cleanup.
#[test]
fn test_dead_promise_side_table_entries_cleared_by_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let p_addr = {
        let p = crate::promise::js_promise_new();
        assert!(crate::arena::pointer_in_nursery(p as usize));
        crate::promise::scanners::test_park_promise_side_table_entries(p);
        p as usize
    };
    // Not rooted: dead at the first minor.
    js_shadow_slot_set(0, 0);

    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(p_addr),
        (1, 1, 1),
        "test premise: one entry parked in each table"
    );

    let _ = gc_collect_minor();

    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(p_addr),
        (0, 0, 0),
        "dead promise's settle-listener / overflow-reaction / Promise.all-state \
         entries must be dropped (they can never fire and strongly root their \
         payloads)"
    );
}

/// A LIVE (rooted) pending promise must KEEP its entries across a copied
/// minor, rekeyed to the promise's post-move address by the registered
/// promise root scanner.
#[test]
fn test_live_promise_side_table_entries_rekeyed_by_copied_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    // The guard clears the thread's mutable-scanner registry for isolation;
    // this test is ABOUT key rewriting, so re-register the promise scanner.
    gc_register_mutable_root_scanner(promise_mutable_root_scanner);

    let p = crate::promise::js_promise_new();
    let p_addr = p as usize;
    crate::promise::scanners::test_park_promise_side_table_entries(p);
    js_shadow_slot_set(0, ptr_bits(p_addr));

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        moved, p_addr,
        "test premise: the promise must actually move"
    );
    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(moved),
        (1, 1, 1),
        "live promise's entries must survive, keyed by the moved address"
    );
    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(p_addr),
        (0, 0, 0),
        "no entry may linger under the stale pre-move key"
    );
}

/// The `GcFinalizeHookKind::PromiseCleanup` arm (sweeping collections /
/// malloc'd promises) routes through `clear_promise_context_for_gc`, which
/// must drop the dead promise's entries in all three tables.
#[test]
fn test_promise_cleanup_finalize_arm_clears_side_tables() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let p = crate::promise::js_promise_new();
    crate::promise::scanners::test_park_promise_side_table_entries(p);
    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(p as usize),
        (1, 1, 1),
    );

    crate::promise::clear_promise_context_for_gc(p);

    assert_eq!(
        crate::promise::scanners::test_promise_side_table_counts_for(p as usize),
        (0, 0, 0),
        "the finalize arm must purge every side table keyed by the dead promise"
    );
}

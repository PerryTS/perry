//! Owner-reachability checks for Promise reaction and combinator state.
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
    // Observe the private list weakly: reading an old owner address after a
    // copying reset is invalid now that there is no address table to inspect.
    let bag = unsafe { (*(*(p_addr as *mut crate::promise::Promise)).meta).owned_state };
    let weak = crate::weakref::js_weakref_new(f64::from_bits(ptr_bits(bag as usize)));
    js_shadow_slot_set(0, ptr_bits(weak as usize));
    let _ = gc_collect_minor();
    assert_eq!(
        crate::weakref::js_weakref_deref(f64::from_bits(js_shadow_slot_get(0))).to_bits(),
        crate::value::TAG_UNDEFINED,
        "an abandoned Promise must not independently root its reaction/combinator bag"
    );
}

/// A LIVE (rooted) pending promise must KEEP its entries across a copied
/// minor, traced through the promise's metadata edge.
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
}

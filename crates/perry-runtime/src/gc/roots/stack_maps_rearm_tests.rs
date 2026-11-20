//! The published-index flag survives a re-arm.
//!
//! Every agent runs `js_gc_init`, which re-arms the lazy stack-map build for a
//! newly initialized image. Re-arming used to clear the "index published" flag,
//! so a collection another thread was running at that moment aborted in
//! `visit_stack_map_root_slots` (worker threads starting while another thread
//! collected). Re-arming now only owes a rebuild.
use super::*;

#[test]
fn a_rearm_owes_a_rebuild_without_unpublishing_the_index() {
    let Some(_isolated) = crate::test_support::isolated_process_guard() else {
        return;
    };
    ensure_built();
    assert!(stack_maps_initialized());
    let before = STACK_MAPS.read().generation;
    // Another agent's `js_gc_init` (lazy by default: the build is left owed).
    initialize();
    assert!(
        stack_maps_initialized(),
        "a re-arm must not make concurrent scans on other threads see an unbuilt index"
    );
    ensure_built();
    assert!(!STACK_MAPS_REBUILD_OWED.load(Ordering::Acquire));
    assert!(
        STACK_MAPS.read().generation > before,
        "the owed rebuild ran"
    );
    // Nothing owed: no rebuild.
    let built = STACK_MAPS.read().generation;
    ensure_built();
    assert_eq!(STACK_MAPS.read().generation, built);
}

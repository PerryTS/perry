//! #11523: a `CannotCollect` helper's root-registry lock must not collect on
//! release.
//!
//! The typed-feedback registry is the one `GcRootRegistryGuard` reachable from
//! helpers `perry-codegen` classifies `CannotCollect` (so emits as
//! `"gc-leaf-function"`, relocating nothing across the call). These tests plant
//! a GC request inside that locked region — the shape of a future edit adding
//! an allocation there — and show the release catches it instead of flushing
//! it into a collection.

use super::super::*;
use super::support::*;
use crate::typed_feedback::leaf_lock_test_hooks as hooks;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// What an allocation's trigger check does once a collection is due: request
/// one. Under a root lock that request is deferred to the lock's release.
fn plant_collection_request() {
    assert!(
        gc_root_lock_held(),
        "plant must run inside the locked region"
    );
    assert_eq!(gc_collect_minor(), 0, "under a root lock the minor defers");
    assert!(
        deferred_gc_request_pending(),
        "the plant must defer a request"
    );
}

struct PlantGuard;

impl PlantGuard {
    fn plant() -> Self {
        let _ = take_deferred_gc_request();
        hooks::plant_in_locked_region(Some(plant_collection_request));
        Self
    }
}

impl Drop for PlantGuard {
    fn drop(&mut self) {
        hooks::plant_in_locked_region(None);
        let _ = take_deferred_gc_request();
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Drive `helper` with a request planted in its locked region and assert the
/// release caught it (panicked) without collecting and without losing it.
fn assert_planted_request_is_caught_not_flushed(helper: impl FnOnce()) {
    let _nursery = CopyingNurseryTestGuard::new(1);
    let plant = PlantGuard::plant();
    let before = gc_collection_count();

    let result = catch_unwind(AssertUnwindSafe(helper));

    let message = result
        .err()
        .map(|payload| panic_message(&*payload))
        .expect("a request deferred inside a CannotCollect helper's lock must be caught");
    assert!(message.contains("#11523"), "unexpected panic: {message}");
    assert_eq!(
        gc_collection_count(),
        before,
        "the release of a CannotCollect helper's lock ran a collection"
    );
    assert!(
        deferred_gc_request_pending(),
        "the request must stay pending for a real safepoint, not be dropped"
    );
    assert!(
        !gc_root_lock_held(),
        "the lock depth must still unwind to 0"
    );
    drop(plant);
}

#[test]
fn record_guard_pass_catches_a_request_deferred_in_its_locked_region() {
    assert_planted_request_is_caught_not_flushed(|| hooks::record_guard_pass(11_523));
}

#[test]
fn layout_invalidation_catches_a_request_deferred_in_its_locked_region() {
    // `js_gc_note_slot_layout` / `js_closure_set_capture_bits` reach this
    // through `layout_mark_unknown`.
    assert_planted_request_is_caught_not_flushed(|| {
        hooks::invalidate_representation_change(0x1_0000)
    });
}

/// Control: the same plant under an ordinary registry lock DOES collect on
/// release. This is the hazard, and it proves the plant is live — without it
/// the two tests above could pass on a plant that never requested anything.
#[test]
fn the_same_plant_under_an_ordinary_lock_collects_on_release() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    let plant = PlantGuard::plant();
    let ordinary = std::sync::Mutex::new(());
    let before = gc_collection_count();

    {
        let _guard = lock_gc_root_registry(&ordinary);
        plant_collection_request();
    }

    assert!(
        !deferred_gc_request_pending(),
        "an ordinary release flushes"
    );
    assert!(
        gc_collection_count() > before,
        "the flushed request must have run a collection"
    );
    drop(plant);
}

/// A clean region stays silent, and a request inherited from an enclosing
/// ordinary lock is not blamed on the non-collecting region nested inside it:
/// the outer release still flushes it.
#[test]
fn clean_and_inherited_requests_are_not_violations() {
    let _nursery = CopyingNurseryTestGuard::new(1);
    let _ = take_deferred_gc_request();
    hooks::record_guard_pass(11_523);
    assert!(!deferred_gc_request_pending());

    let outer = std::sync::Mutex::new(());
    let before = gc_collection_count();
    {
        let _outer = lock_gc_root_registry(&outer);
        assert_eq!(gc_collect_minor(), 0);
        hooks::record_guard_pass(11_523);
        assert!(
            deferred_gc_request_pending(),
            "inner release must not flush"
        );
        assert_eq!(gc_collection_count(), before);
    }
    assert!(!deferred_gc_request_pending());
    assert!(gc_collection_count() > before);
}

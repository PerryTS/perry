//! Error side-table GC integration (2026-07-02 audit, GC deep set).
//!
//! All seven address-keyed error side tables (`ERROR_MESSAGE_*` +
//! `ERROR_USER_PROPS` in `node_submodules::diagnostics`) used to be GC-blind
//! for a MOVABLE type: a moved error lost its `err.code`/user props, a fresh
//! error at a recycled address inherited a dead error's entries, and an
//! object-valued user prop was collectable while reachable. Covered by the
//! `ErrorSideTables` move/finalize hooks + the user-props mutable-root
//! scanner.

use super::super::*;
use super::support::*;

fn error_bits(err: usize) -> u64 {
    ptr_bits(err)
}

/// A rooted error that MOVES in a copied-minor must keep its user props —
/// the side-table entry is rekeyed to the new address by the move hook.
#[test]
fn test_error_user_props_survive_copied_minor_move() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let err = crate::error::js_error_new() as usize;
    assert!(crate::arena::pointer_in_nursery(err));
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "code",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    js_shadow_slot_set(0, error_bits(err));

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, err, "test premise: the error must actually move");
    let prop = crate::node_submodules::diagnostics::error_user_prop(moved, "code");
    assert_eq!(
        prop.map(f64::to_bits),
        Some(crate::value::TAG_TRUE),
        "user prop must be readable at the error's post-move address \
         (the side tables were keyed by the stale pre-move address)"
    );
    assert!(
        crate::node_submodules::diagnostics::error_user_prop(err, "code").is_none(),
        "the stale pre-move key must be gone (a recycled address would \
         inherit it otherwise)"
    );
}

/// A DEAD error's side-table entries must be dropped by the copied-minor
/// from-space finalize so a fresh error at the recycled address doesn't
/// inherit them.
#[test]
fn test_dead_error_side_table_entries_cleared() {
    let _guard = CopyingNurseryTestGuard::new(1);

    let err = crate::error::js_error_new() as usize;
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "inherited",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    // Not rooted: dead at the first minor.
    js_shadow_slot_set(0, 0);

    let _ = gc_collect_minor();

    assert!(
        crate::node_submodules::diagnostics::error_user_prop(err, "inherited").is_none(),
        "dead error's user-prop entry must be cleared, not left for a \
         fresh error at the recycled address to inherit"
    );
}

/// An OBJECT-valued user prop must keep its referent alive across a
/// copied-minor and must read back at the referent's moved address.
#[test]
fn test_object_valued_user_prop_is_a_gc_root_and_rewrites() {
    let _guard = CopyingNurseryTestGuard::new(1);
    // The guard clears the thread's mutable-scanner registry for isolation;
    // this test is ABOUT the scanner, so re-register it.
    gc_register_mutable_root_scanner(
        crate::node_submodules::diagnostics_gc::scan_error_user_props_roots_mut,
    );

    let err = crate::error::js_error_new() as usize;
    js_shadow_slot_set(0, error_bits(err));

    // The prop's object is reachable ONLY through the side table.
    let cause = crate::object::js_object_alloc(0, 0);
    crate::node_submodules::diagnostics::set_error_user_prop(
        err,
        "cause",
        f64::from_bits(ptr_bits(cause as usize)),
    );

    let _ = gc_collect_minor();

    let moved_err = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let prop = crate::node_submodules::diagnostics::error_user_prop(moved_err, "cause")
        .expect("prop must survive");
    let prop_addr = (prop.to_bits() & POINTER_MASK) as usize;
    assert_ne!(
        prop_addr, cause as usize,
        "the object referent must have been evacuated (and the stored \
         bits rewritten) — identical address means the scanner did not \
         visit the slot"
    );
    unsafe {
        let header = (prop_addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        assert_eq!(
            (*header).obj_type,
            crate::gc::GC_TYPE_OBJECT,
            "rewritten prop bits must point at the live moved object"
        );
    }
}

//! The relocation funnel: what follows an object when its storage moves.
//!
//! Split out of `gc/layout.rs` (#10362), which sits on the repo's 2000-line
//! cap, and narrowed to the contract its callers have always satisfied.
//!
//! # The contract
//!
//! Four paths replace an object's storage: the copying nursery's `move_young`,
//! the two old-generation evacuations in `gc/oldgen.rs`, and `js_array_grow`.
//! **Every one of them copies the source header's `_reserved` into the
//! destination before calling** — the copying minor through
//! `reserved_with_copied_survival_age`, which rewrites only the age bits.
//!
//! So every layout fact the header carries has already arrived at the
//! destination by construction: the layout state, `GC_LAYOUT_ALL_POINTERS`,
//! the raw-f64 / holes flags, `GC_ARRAY_ELEMENT_SHAPE`. What cannot ride a header is a record keyed
//! by the object's ADDRESS, and that is all this funnel moves:
//!
//! * the residual static-prototype owner registry (#9304), gated by its
//!   process-global latch (#7733/#7737) — for EVERY kind that can own an
//!   entry, not only the layout kinds (see below);
//! * the element-shape proof record (#7480), gated by the header bit that is
//!   authoritative for it;
//!
//! # The prototype registry is not layout metadata
//!
//! Its population is every receiver `object::prototype_chain::
//! meta_capable_object` turns away — a Map, Set, Error, Promise, Date, RegExp,
//! Temporal cell, lazy JSON array or closure as much as an array — and most of
//! those kinds have no layout slots at all. The rekey used to sit in the array
//! arm of the record path, below the layout-kind return, and in the ordinary
//! object's move hook; every other movable owner kept its entry under the
//! address it had just left, and the dead-owner prune then dropped it. So it
//! runs first, keyed on `prototype_chain::residual_prototype_owner_type`, the
//! same population predicate the collector's value visit uses
//! (`gc/layout_slot_visit.rs`).
//!
//! Until #10362 the funnel re-derived the header half too, once per relocated
//! object (4.2% of #10362's retained-graph run). The gates below answer the
//! same questions from the header word and two flags the caller has already
//! brought into cache. GC payload kinds travel in the header copy.

use super::*;

/// Rekey residual prototypes and element-shape proofs after relocation.
///
/// # Safety
///
/// `old_user` and `new_user` are user pointers of live allocations, and the
/// caller has already made the destination header a copy of the source's (see
/// the module docs). The precondition is asserted in test and debug builds.
///
/// `inline(always)`: every relocation of every object runs this and all it
/// keeps inline is gates — each record move is a cold out-of-line call — but
/// three gates are enough for the heuristic to outline it from `move_young`,
/// and then the call costs more than the gates.
#[inline(always)]
pub(crate) unsafe fn layout_transfer(old_user: *mut u8, new_user: *mut u8) {
    if old_user.is_null() || new_user.is_null() || old_user == new_user {
        return;
    }
    if (old_user as usize) < GC_HEADER_SIZE + 0x1000 {
        return;
    }
    // Before the layout-kind return, because the registry's owners are not the
    // layout kinds (module docs). The latch first: it is one byte load, false
    // for any process that never re-prototyped a non-object, and the move is
    // out of line.
    let relocating_header = header_from_user_ptr(old_user as *const u8);
    if crate::object::prototype_chain::object_static_prototypes_maybe_nonempty()
        && crate::object::prototype_chain::residual_prototype_owner_type(
            (*relocating_header).obj_type,
        )
        // #10362: the per-OWNER half, same bit and same proof as the trace
        // path's. `_reserved` rides every relocation by construction — all four
        // callers copy it before calling here and
        // `assert_relocation_copied_the_header` enforces that — so the bit is
        // already correct at this point and needs no transfer of its own.
        && crate::object::prototype_chain::residual_entry_possible_for(relocating_header)
    {
        transfer_residual_prototype(old_user as usize, new_user as usize);
    }
    // Kinds with no layout metadata at all (strings, meta records, RegExps)
    // have no layout record to move. The destination carries the same
    // `obj_type`, so one classification answers for both.
    let Some(old_header) = layout_header_for_user(old_user as usize) else {
        return;
    };
    assert_relocation_copied_the_header(old_header, new_user);

    let reserved = (*old_header)._reserved;
    let is_array = (*old_header).obj_type == GC_TYPE_ARRAY;
    if is_array && reserved & GC_ARRAY_ELEMENT_SHAPE != 0 {
        crate::array::transfer_element_shape(old_user as usize, new_user as usize);
    }
}

/// The residual prototype registry's rekey. Cold and out of line so the funnel
/// stays small enough to inline into every relocation site: it is reached only
/// once something in the process has been re-prototyped.
#[cold]
#[inline(never)]
fn transfer_residual_prototype(old_user: usize, new_user: usize) {
    crate::object::prototype_chain::object_static_prototype_owner_moved(old_user, new_user);
}

/// The funnel's precondition: the destination header is the source's copy.
///
/// Checked in test and debug builds — including `cargo test --release`, which
/// is how the GC suites run — so a future relocation path that allocates a
/// destination without copying `_reserved` fails loudly here instead of
/// silently losing a layout state, an `ALL_POINTERS` bit or an element-shape
/// proof at the first collection.
#[inline]
unsafe fn assert_relocation_copied_the_header(old_header: *mut GcHeader, new_user: *mut u8) {
    #[cfg(any(test, debug_assertions))]
    {
        let new_header = header_from_user_ptr(new_user as *const u8);
        assert_eq!(
            (*new_header).obj_type,
            (*old_header).obj_type,
            "layout_transfer: a relocation must not change the object type"
        );
        assert_eq!(
            (*new_header)._reserved & !GC_COPY_SURVIVAL_AGE_MASK,
            (*old_header)._reserved & !GC_COPY_SURVIVAL_AGE_MASK,
            "layout_transfer: the caller must copy `_reserved` into the destination before \
             relocating (only the copied-survival age may differ) — every header-carried \
             layout fact rides that copy"
        );
    }
    #[cfg(not(any(test, debug_assertions)))]
    {
        let _ = (old_header, new_user);
    }
}

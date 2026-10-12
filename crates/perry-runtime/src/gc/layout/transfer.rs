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
//! * the element-shape proof record (#7480), gated by the header bit that is
//!   authoritative for it. Prototypes already travel with their owner bags.
//!
//! Until #10362 the funnel re-derived the header half too, once per relocated
//! object (4.2% of #10362's retained-graph run). The gates below answer the
//! same questions from the header word and two flags the caller has already
//! brought into cache. GC payload kinds travel in the header copy.

use super::*;

/// Rekey element-shape proofs after relocation.
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

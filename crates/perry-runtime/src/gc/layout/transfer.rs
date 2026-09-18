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
//! the raw-f64 / holes flags, `GC_ARRAY_ELEMENT_SHAPE` and
//! `GC_OBJ_TYPED_LAYOUT_INTACT`. What cannot ride a header is a record keyed
//! by the object's ADDRESS, and that is all this funnel moves:
//!
//! * the residual static-prototype owner registry (#9304), gated by its
//!   process-global latch (#7733/#7737) — for EVERY kind that can own an
//!   entry, not only the layout kinds (see below);
//! * the element-shape proof record (#7480), gated by the header bit that is
//!   authoritative for it;
//! * the per-object `TYPED_LAYOUTS` and `LAYOUT_SLOT_MASKS` entries, gated by
//!   #7510's emptiness flag and address filter.
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
//! Until #10362 the funnel re-derived the header half too — rewriting bits
//! that were already equal, and re-resolving the intact bit through a
//! ShapeId-keyed `SHAPE_LAYOUTS` probe — once per relocated object. Measured
//! on #10362's retained-graph workload that was 160 instructions per moved
//! array and 245 per moved object, 518M instructions (4.2% of the run), of
//! which zero reached a side-table record: both per-object maps held one key.
//! The gates below answer the same questions from the header word and two
//! flags the caller has already brought into cache.
//!
//! # Why the intact bit is not re-derived
//!
//! `GC_OBJ_TYPED_LAYOUT_INTACT` asks whether a canonical typed descriptor is
//! reachable for this object. Its inputs are the receiver's stamped ShapeId
//! (copied verbatim with the payload), `SHAPE_LAYOUTS`, the process-global
//! registered typed-shape registry (#8405) and the per-object map — and a
//! relocation changes none of them. Re-asking at move time could therefore
//! only apply a LAZY downgrade, and only to the objects that happen to move.
//!
//! The state that downgrade cleared — intact while no descriptor is reachable
//! — is legal and handled. `shape_install_shared` poisons a shape's shared
//! entry to `None` and deliberately leaves "any still-INTACT siblings" to fall
//! back; #8115 clears the bit at the first contradicting store; the trace path
//! resolves no mask, sets `GC_LAYOUT_UNKNOWN` and scans every slot; the query
//! helpers answer "no descriptor". An unmoved sibling in exactly that state
//! keeps its bit today, so an argument that needed the move to clear it would
//! already be broken for every object that does not move.
//! `gc/tests/layout_trace/typed_shape.rs` pins the pair across a real copying
//! minor: the moved object and its unmoved peer must answer identically, and
//! the child behind the poisoned shape must survive the cycle.

use super::*;
use crate::gc::layout_tables::per_object_layouts_may_hold_either;

/// Move the address-keyed layout records of a relocated object.
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
    // Two gates, both answered from words already in registers or in the one
    // hot thread-local slot #7510 keeps them in. Each is the same question the
    // record mover behind it asks first, hoisted so the common case — no
    // record anywhere near either address — never leaves this function.
    let per_object = per_object_layouts_may_hold_either(old_user as usize, new_user as usize);
    let element_shape = is_array && reserved & GC_ARRAY_ELEMENT_SHAPE != 0;
    if per_object || element_shape {
        transfer_address_keyed_records(
            old_user as usize,
            new_user as usize,
            header_from_user_ptr(new_user as *const u8),
            is_array,
        );
    }

    // The source is a dead evacuation original or a growth forwarding stub the
    // moment we return. Drop its claim to a descriptor rather than leave the
    // bit readable at an address whose records now belong to the destination.
    header_clear_typed_layout_intact(old_header);
}

/// The residual prototype registry's rekey. Cold and out of line so the funnel
/// stays small enough to inline into every relocation site: it is reached only
/// once something in the process has been re-prototyped.
#[cold]
#[inline(never)]
fn transfer_residual_prototype(old_user: usize, new_user: usize) {
    crate::object::prototype_chain::object_static_prototype_owner_moved(old_user, new_user);
}

/// The layout record moves themselves. Cold: on a workload holding no
/// per-object layout record and no element-shape proof — the steady state of
/// every monomorphic program — it is never reached.
#[cold]
#[inline(never)]
unsafe fn transfer_address_keyed_records(
    old_user: usize,
    new_user: usize,
    new_header: *mut GcHeader,
    is_array: bool,
) {
    if is_array {
        // #7480: the proof record is keyed by the array's address while the
        // header bit is what a read consults. `transfer_element_shape` decides
        // from both headers and fails closed — it clears the destination bit
        // when no record follows the move.
        crate::array::transfer_element_shape(old_user, new_user);
    }
    // #7510's two per-object maps. Both re-test the gate above for their own
    // address pair, so calling them when only a sibling gate fired costs one
    // predictable branch each.
    //
    // Re-setting the intact bit for a moved per-object descriptor is parity
    // with the pre-#10362 funnel rather than a fact the copy lost: a source
    // whose descriptor existed while its own bit was clear had the bit SET by
    // the move. Keeping that leaves the lazy downgrade (module docs) as the
    // single behavioural difference of #10362.
    if transfer_per_object_descriptor(old_user, new_user) {
        header_set_typed_layout_intact(new_header);
    }
    transfer_per_object_slot_mask(old_user, new_user);
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

//! The **per-object** halves of the GC slot-layout metadata, and the emptiness
//! flag that keeps them off the hot path (#7510).
//!
//! Two address-keyed thread-locals live here:
//!
//! - [`LAYOUT_SLOT_MASKS`] — which slots of an object hold pointers.
//! - [`TYPED_LAYOUTS`] — the object's canonical `TypedLayoutDescriptor`.
//!
//! Both predate #6893, which moved the *common* case — an object whose live
//! layout still matches its shape — into the shape-keyed `SHAPE_LAYOUTS` map
//! in [`super::layout`]. What is left in these two maps is the residue:
//! objects that **diverged** from their shape, objects with no `keys_array`,
//! and ambiguous shapes. On a monomorphic workload that residue is empty for
//! the entire run.
//!
//! Empty is not the same as free, though. Every allocation
//! (`layout_init_pointer_free`), every typed-shape install, every object death
//! (`layout_clear_for_ptr`) and every relocation (`layout_transfer`) probed
//! both maps to clear whatever a previous tenant of a recycled address might
//! have left. Two `RefCell` round-trips plus two hashes, per object, to remove
//! nothing: `layout_forget_object` was 14.5% of self time on the
//! object-construction profile in #7510, nearly twice the allocator it was
//! bookkeeping for.
//!
//! [`PER_OBJECT_LAYOUTS_NONEMPTY`] answers "is there anything in either map at
//! all" in a single load, and every mutating path in this module maintains it.
//! Callers outside get the guarded accessors, not the maps.
//!
//! Split out of `layout.rs` to stay under the repo's 2000-line-per-file cap
//! (`scripts/check_file_size.sh`).

use super::hot_tls::{hot_layout_slot_masks, hot_per_object_layouts_nonempty, hot_typed_layouts};
use super::layout::{LayoutSlotMask, TypedLayoutDescriptor};
use std::cell::{Cell, RefCell};

thread_local! {
    pub(in crate::gc) static LAYOUT_SLOT_MASKS: RefCell<crate::fast_hash::PtrHashMap<usize, LayoutSlotMask>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    pub(in crate::gc) static TYPED_LAYOUTS: RefCell<crate::fast_hash::PtrHashMap<usize, TypedLayoutDescriptor>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// #7510: "either per-object side table above **may** hold an entry".
    ///
    /// INVARIANT: `false` ⟹ both [`LAYOUT_SLOT_MASKS`] and [`TYPED_LAYOUTS`]
    /// are empty. Only an insert can break that emptiness, and every insert
    /// routes through [`typed_layouts_insert`] / [`slot_masks_insert`], which
    /// arm the flag; the removal paths re-test both maps and clear it again
    /// once they are empty. A stale `true` therefore costs exactly the
    /// pre-#7510 probe and nothing else — the flag is an accelerator, never an
    /// authority, and no caller may treat it as one.
    pub(in crate::gc) static PER_OBJECT_LAYOUTS_NONEMPTY: Cell<bool> = const { Cell::new(false) };
}

/// True when either per-object side table may hold an entry. `false` is a
/// proof of emptiness (see [`PER_OBJECT_LAYOUTS_NONEMPTY`]); `true` is only a
/// hint, so every caller still has to handle a miss.
#[inline(always)]
pub(in crate::gc) fn per_object_layouts_maybe_nonempty() -> bool {
    hot_per_object_layouts_nonempty().get()
}

/// Arm the flag. Called by anything that inserts into either map — including
/// the one insert site that holds its own `borrow_mut` and so cannot go
/// through the wrappers below.
#[inline(always)]
pub(in crate::gc) fn mark_per_object_layouts_nonempty() {
    hot_per_object_layouts_nonempty().set(true);
}

/// Re-establish the flag after a removal: clear it once *both* maps are empty
/// again. Only reached in the rare "something diverged" regime, so the two
/// extra borrows never land on the monomorphic hot path.
#[inline]
pub(in crate::gc) fn refresh_per_object_layouts_flag() {
    if hot_layout_slot_masks().borrow().is_empty() && hot_typed_layouts().borrow().is_empty() {
        hot_per_object_layouts_nonempty().set(false);
    }
}

/// The one way to add a per-object typed descriptor.
#[inline]
pub(in crate::gc) fn typed_layouts_insert(user_ptr: usize, descriptor: TypedLayoutDescriptor) {
    mark_per_object_layouts_nonempty();
    hot_typed_layouts()
        .borrow_mut()
        .insert(user_ptr, descriptor);
}

/// The one way to add a per-object pointer mask.
#[inline]
pub(in crate::gc) fn slot_masks_insert(user_ptr: usize, mask: LayoutSlotMask) {
    mark_per_object_layouts_nonempty();
    hot_layout_slot_masks().borrow_mut().insert(user_ptr, mask);
}

/// Drop `user_ptr`'s per-object typed descriptor (only).
#[inline]
pub(in crate::gc) fn typed_layouts_remove(user_ptr: usize) {
    if !per_object_layouts_maybe_nonempty() {
        return;
    }
    hot_typed_layouts().borrow_mut().remove(&user_ptr);
    refresh_per_object_layouts_flag();
}

/// Drop `user_ptr`'s per-object pointer mask (only).
#[inline]
pub(in crate::gc) fn slot_masks_remove(user_ptr: usize) {
    if !per_object_layouts_maybe_nonempty() {
        return;
    }
    hot_layout_slot_masks().borrow_mut().remove(&user_ptr);
    refresh_per_object_layouts_flag();
}

/// Run `f` against `user_ptr`'s per-object typed descriptor, if it has one.
/// The borrow is confined to `f` because the callers that act on the answer
/// (`layout_set_typed_unknown`) take the same map mutably.
#[inline]
pub(in crate::gc) fn with_per_object_descriptor<R>(
    user_ptr: usize,
    f: impl FnOnce(&TypedLayoutDescriptor) -> R,
) -> Option<R> {
    if !per_object_layouts_maybe_nonempty() {
        return None;
    }
    hot_typed_layouts().borrow().get(&user_ptr).map(f)
}

/// `user_ptr`'s per-object pointer mask, if it has one. The trace path calls
/// this once per `SIDE_MASK` object it visits, so the emptiness proof is worth
/// as much here as it is on the mutator side.
#[inline]
pub(in crate::gc) fn per_object_slot_mask(user_ptr: usize) -> Option<LayoutSlotMask> {
    if !per_object_layouts_maybe_nonempty() {
        return None;
    }
    hot_layout_slot_masks().borrow().get(&user_ptr).cloned()
}

/// Move `old_user`'s per-object typed descriptor to `new_user` (relocation),
/// clearing anything the destination address inherited from a previous tenant.
/// Returns whether a descriptor actually made the move — `layout_transfer`
/// uses that to decide the destination's intact bit.
///
/// With both maps provably empty there is nothing to move, and every relocated
/// object would otherwise pay a `RefCell` round-trip plus two hashes during
/// evacuation. The shape-keyed half is unaffected: it needs no move at all.
#[inline]
pub(in crate::gc) fn transfer_per_object_descriptor(old_user: usize, new_user: usize) -> bool {
    if !per_object_layouts_maybe_nonempty() {
        return false;
    }
    let mut typed = hot_typed_layouts().borrow_mut();
    typed.remove(&new_user);
    match typed.remove(&old_user) {
        Some(layout) => {
            typed.insert(new_user, layout);
            true
        }
        None => false,
    }
}

/// Move `old_user`'s per-object pointer mask to `new_user` (relocation).
#[inline]
pub(in crate::gc) fn transfer_per_object_slot_mask(old_user: usize, new_user: usize) {
    if !per_object_layouts_maybe_nonempty() {
        return;
    }
    let mut masks = hot_layout_slot_masks().borrow_mut();
    masks.remove(&new_user);
    if let Some(mask) = masks.remove(&old_user) {
        masks.insert(new_user, mask);
    }
}

/// Drop any per-object layout record keyed by `user_ptr`.
///
/// Both maps are probed on **every** object allocation
/// (`layout_init_pointer_free`), on every typed-shape install, and again on
/// object death, to clear whatever a previous tenant of a recycled address
/// left behind. Since #6893 there is usually nothing to clear, so the whole
/// call is pure cost — see the module docs.
///
/// [`PER_OBJECT_LAYOUTS_NONEMPTY`]'s `false` state is a proof of emptiness, so
/// the early return cannot skip a live record. The slow half below is the
/// pre-#7510 path unchanged, and re-arms the flag on the way out.
#[inline]
pub(in crate::gc) fn layout_forget_object(user_ptr: usize) {
    if !per_object_layouts_maybe_nonempty() {
        return;
    }
    // One `borrow_mut` per map, not a `borrow` to test emptiness followed by a
    // second `borrow_mut` to remove: `RefCell`'s flag traffic is a measurable
    // share of a function this hot (#7469).
    let masks_empty = {
        let mut masks = hot_layout_slot_masks().borrow_mut();
        if !masks.is_empty() {
            masks.remove(&user_ptr);
        }
        masks.is_empty()
    };
    let typed_empty = {
        let mut typed = hot_typed_layouts().borrow_mut();
        if !typed.is_empty() {
            typed.remove(&user_ptr);
        }
        typed.is_empty()
    };
    if masks_empty && typed_empty {
        hot_per_object_layouts_nonempty().set(false);
    }
}

#[cfg(test)]
pub(in crate::gc) fn test_per_object_tables_are_empty() -> bool {
    hot_layout_slot_masks().borrow().is_empty() && hot_typed_layouts().borrow().is_empty()
}

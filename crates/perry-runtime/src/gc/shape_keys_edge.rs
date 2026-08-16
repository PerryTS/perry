//! #8112 — the shape table's shared keys edge, seen from old→young
//! verification.
//!
//! `object/shapes.rs` owns the edge itself; this module owns the one question
//! `verify_old_young_parent_slots_covered` has to ask about it.
//!
//! A receiver's ordered-keys edge is the `keys` word of its boxed
//! `ShapeDescriptor`, and every sibling of the shape enumerates that same word.
//! So it is not a slot any ONE parent owns, and per-parent coverage — "is this
//! parent's page in the remembered set?" — is the wrong question to ask of it:
//!
//! * one sibling's rewrite changes the edge of every other, including old
//!   receivers a minor never visits and never gets a chance to remember;
//! * the word lives outside the GC heap, so no page a barrier can dirty
//!   contains it;
//! * and even a per-parent entry that DOES get recorded (the promoted-object
//!   rebuild still records one whenever it walks a carrier, and that entry is
//!   useful — it re-enters the owner next cycle) cannot speak for the parents
//!   it is not attached to. When its own parent dies, the surviving old
//!   carriers are left with an edge nothing describes.
//!
//! Recording is therefore left alone and only the VERIFIER skips the word.
//! What actually covers it is the shape table's own root scanner under the
//! `old_carrier` gate (`shapes::scan_shape_table_rekey_mut`). Skipping without
//! that gate would be a missing-edge bug; the gate without the skip is the
//! `slot_page_ever_dirty=false` abort `PERRY_GC_VERIFY_EVACUATION` produced
//! while this issue was being built.

use super::{GcHeader, GC_HEADER_SIZE, GC_TYPE_OBJECT};

/// Is `slot` the shared shape-table `keys` word of `parent_header`'s receiver?
///
/// The cheap term comes first and is false for every ordinary slot: an object
/// field, an array element and a closure capture all live inside the GC heap,
/// while a descriptor record is a `Box` on the Rust heap. Only then does this
/// pay for a shape probe.
#[inline]
pub(super) unsafe fn slot_is_shared_shape_keys_word(
    parent_header: *mut GcHeader,
    slot: *mut u64,
) -> bool {
    if parent_header.is_null() || slot.is_null() || (*parent_header).obj_type != GC_TYPE_OBJECT {
        return false;
    }
    if crate::arena::classify_heap_generation(slot as usize)
        != crate::arena::HeapGeneration::Unknown
    {
        return false;
    }
    let obj = (parent_header as *mut u8).add(GC_HEADER_SIZE) as *const crate::object::ObjectHeader;
    let shape_id = crate::object::shapes::object_shape_stamp(obj);
    crate::object::shapes::shape_id_owns_keys_slot(shape_id, slot)
}

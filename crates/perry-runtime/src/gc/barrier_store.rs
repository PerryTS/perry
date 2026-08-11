//! The runtime's exact slot-store helpers — the choke point every dynamic
//! object-field and array-element write funnels through, plus the slot-form
//! write-barrier wrappers they compose with. Split out of `barrier.rs` to
//! keep it under the repo's 2000-line cap (#7630); pure move except for the
//! `use` lines.

use super::barrier::{
    incremental_mark_barrier_value, malloc_gc_parent_addr, write_barrier_slot_decoded,
    write_barriers_enabled,
};
use super::*;

pub(crate) fn runtime_write_barrier_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, false);
}

/// Canonicalize an **INT32-boxed** numeric store into a raw-f64-masked slot of
/// an intact typed-shape descriptor (`0x7FFE…` → the plain IEEE bits of the same
/// number). The object twin of `canonicalize_array_numeric_store_bits`
/// (`array/header.rs`), and needed for the same reason.
///
/// `layout_note_slot` treats any non-raw-f64 bit pattern landing in a raw-f64
/// slot as a representation change and calls `layout_set_typed_unknown`, which
/// evicts the object's `TypedLayoutDescriptor` **permanently and one-way**.
/// INT32 boxes genuinely reach object fields from FFI / native modules (sqlite
/// row columns, `v8` deserialization, …), and unlike codegen's guarded class-
/// field store — which canonicalizes inline behind a plain-finite check — this
/// runtime choke point wrote the bits verbatim. One FFI integer therefore cost
/// the object its typed fast path forever.
///
/// There is no observable behavior change: an INT32 box and its f64 are `===`
/// and print identically. `value_bits_to_number` supplies the class-ref
/// exclusion (a `ClassRef` shares INT32_TAG and must keep its tag), so a class
/// value still downgrades the descriptor rather than being stripped to a bare
/// number.
///
/// Ordered tag-first so the (hot) non-INT32 store never pays the thread-local
/// descriptor probe.
#[inline]
fn canonicalize_typed_slot_store_bits(
    parent_user: usize,
    slot_index: usize,
    value_bits: u64,
) -> u64 {
    if value_bits & TAG_MASK != crate::value::INT32_TAG {
        return value_bits;
    }
    if !crate::gc::layout_slot_is_raw_f64_typed(parent_user, slot_index) {
        return value_bits;
    }
    match crate::array::value_bits_to_number(value_bits) {
        Some(number) => number.to_bits(),
        None => value_bits,
    }
}

/// #7630: `runtime_store_jsvalue_slot` minus the per-slot layout note, for a
/// caller that OWNS the object's whole construction and settles its layout
/// state once at the end (`layout_finish_deferred_boxed_object`). The JSON
/// materialiser is the caller: per record it performed ~13 `layout_note_slot`
/// calls whose only net effect was to build a per-object side-table pointer
/// mask — the profile's top cost family. Everything else is kept bit-for-bit:
/// the typed-slot canonicalization, the string addref demote, and the write
/// barrier (whose SATB shade must never be dropped — the #7602 lesson).
/// Returns whether the stored bits carry a heap pointer, so the caller can
/// accumulate the one fact the elided notes were computing.
#[inline]
pub(crate) fn runtime_store_jsvalue_slot_layout_deferred(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) -> bool {
    let value_bits = canonicalize_typed_slot_store_bits(parent_user, slot_index, value_bits);
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    if value_bits & TAG_MASK == STRING_TAG {
        crate::string::js_string_addref((value_bits & POINTER_MASK) as *mut crate::StringHeader);
    }
    runtime_write_barrier_slot(parent_user, slot_addr, value_bits);
    super::layout::layout_pointer_bearing_bits(value_bits)
}

#[inline]
pub(crate) fn runtime_store_jsvalue_slot(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) {
    let value_bits = canonicalize_typed_slot_store_bits(parent_user, slot_index, value_bits);
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    // A heap string stored into an object field / array element is now aliased
    // from the heap, so a later `js_string_append` must NOT mutate its buffer
    // in place while this slot still references it. Demote it from "uniquely
    // owned" (refcount==1) to shared (refcount==0). This realizes the
    // documented `js_string_addref` contract ("stored into an array/object" —
    // see string/alloc.rs): codegen wires the local-copy alias case (`let y =
    // x`) but never the heap-store case, so refcount=1 strings leaked into
    // object/array slots and were corrupted by the in-place append fast path.
    // Concretely: code that snapshots a string into a heap slot
    // (`slot = newState`) and later grows the same buffer via `+=` would have
    // the in-place append silently rewrite the stored slot, so a later equality
    // check against the snapshot wrongly saw the two as identical. Every dynamic
    // object-field and array-element write funnels through here, so this is the
    // single complete choke point.
    if value_bits & TAG_MASK == STRING_TAG {
        crate::string::js_string_addref((value_bits & POINTER_MASK) as *mut crate::StringHeader);
    }
    layout_note_slot(parent_user, slot_index, value_bits);
    runtime_write_barrier_slot(parent_user, slot_addr, value_bits);
}

pub(crate) fn runtime_write_barrier_external_slot(
    parent_addr: usize,
    slot_addr: usize,
    child_bits: u64,
) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, true);
}

pub(crate) fn runtime_write_barrier_gc_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    let parent_is_malloc_gc = matches!(
        crate::arena::classify_heap_generation(parent_addr),
        crate::arena::HeapGeneration::Unknown
    ) && malloc_gc_parent_addr(parent_addr);
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, parent_is_malloc_gc);
}

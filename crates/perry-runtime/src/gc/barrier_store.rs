//! The runtime's exact slot-store helpers — the choke point every dynamic
//! object-field and array-element write funnels through, plus the slot-form
//! write-barrier wrappers they compose with. Split out of `barrier.rs` to
//! keep it under the repo's 2000-line cap (#7630); pure move except for the
//! `use` lines.

use super::barrier::{
    bump_write_barrier_trace_counter, decode_heap_addr, incremental_mark_barrier_value,
    malloc_gc_parent_addr, write_barrier_decoded_parent, write_barrier_slot_decoded,
    write_barriers_enabled,
};
use super::barrier_arming::barrier_remembering_armed;
use super::telemetry::BarrierTraceCounter;
use super::*;

/// Loop form of [`runtime_write_barrier_slot`] for one old-gen parent and a
/// contiguous slot run — see [`super::barrier::replay_old_parent_slot_range`].
pub(crate) fn replay_old_parent_slot_range_barriers(
    parent_addr: usize,
    slots: *mut u64,
    count: usize,
) {
    super::barrier::replay_old_parent_slot_range(parent_addr, slots, count);
}

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

// --- slot-form barrier entry points (moved from `barrier/mod.rs`, #2000-line cap) ---

/// Gen-GC Phase C1: slot-aware write barrier. Called by
/// codegen-emitted store sites unless `PERRY_WRITE_BARRIERS=0`/
/// `off`/`false` disabled barrier emission at compile time.
///
/// Decode the parent + child as raw addresses. If parent's
/// GcHeader sits in the old-gen arena AND child's NaN-boxed
/// pointer (any of POINTER / STRING / BIGINT / SHORT_STRING)
/// resolves to a heap address inside the nursery, dirty the page
/// containing the written slot. A zero slot address falls back to
/// dirtying every occupied page in the parent object.
///
/// Hot-path constraints: this fires on EVERY heap store in
/// compiled code by default. Must be cheap:
/// generation checks use arena page side metadata rather than
/// scanning every arena block.
#[no_mangle]
pub extern "C" fn js_write_barrier_slot(parent: u64, slot_addr: u64, child: u64) {
    write_barrier_slot_inner(parent, slot_addr as usize, child, false);
}

/// [`js_write_barrier_slot`] for a parent the caller has ALREADY validated:
/// `parent_user` is the raw (untagged) user pointer of a live, non-forwarded
/// GC object whose header the emitted code dereferenced a few instructions
/// earlier (`emit_parent_may_need_remembering_check` reads `gc_flags`).
///
/// Skips `decode_heap_addr(parent)` — a tag dispatch, an alignment/floor
/// test and a page-generation classification that `write_barrier_decoded_parent`
/// repeats one call later — and nothing else. For every parent that meets the
/// contract the two entries decide identically: a classified arena parent
/// reaches the same `barrier_parent_needs_remembering`, and an unregistered
/// (`gc_malloc`) parent, which `decode_heap_addr` would have turned into a
/// skip, is refused there instead because an inline slot never qualifies as
/// external. On a 5k-entity ECS frame the buckets' `push` stores were
/// classifying each old parent twice per command.
#[no_mangle]
pub extern "C" fn js_write_barrier_slot_validated_parent(
    parent_user: u64,
    slot_addr: u64,
    child: u64,
) {
    let Some(child_addr) = barrier_child_prologue(child) else {
        return;
    };
    if !barrier_remembering_active() {
        return;
    }
    if parent_user == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    // Leaf exit for the common repeated store into one old page — see
    // `inline_slot_store_on_cached_dirty_page`.
    if super::barrier::inline_slot_store_on_cached_dirty_page(
        parent_user as usize,
        slot_addr as usize,
    ) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageCacheHits);
        return;
    }
    write_barrier_decoded_parent(parent_user as usize, slot_addr as usize, child_addr, false);
}

pub(super) fn write_barrier_slot_inner(
    parent: u64,
    slot_addr: usize,
    child: u64,
    external_slot: bool,
) {
    // Decode child first: primitive stores are the overwhelmingly common
    // case (every numeric array/field store) and need NEITHER the
    // incremental-mark probe (nothing to mark) NOR the remembered set (no
    // old→young edge) — so they must not pay the incremental barrier's
    // unconditional thread-local access, which dominated tight numeric store
    // loops (#6011: `ema[i] = <f64>` spent more time in this preamble than
    // in the store itself).
    let Some(child_addr) = barrier_child_prologue(child) else {
        return;
    };
    if !barrier_remembering_active() {
        return;
    }
    // Decode the parent — must be a NaN-boxed heap pointer.
    let parent_addr = decode_heap_addr(parent);
    if parent_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    write_barrier_decoded_parent(parent_addr, slot_addr, child_addr, external_slot);
}

/// The never-skippable half of the barrier: decode the stored child and shade
/// it for any in-progress incremental cycle. Returns the child's heap address,
/// or `None` when the store published no heap pointer at all (every numeric
/// array/field store — the #6011 fast path, which must stay the cheapest exit).
#[inline]
pub(super) fn barrier_child_prologue(child: u64) -> Option<usize> {
    let child_addr = decode_heap_addr(child);
    bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);
    if child_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
        return None;
    }
    incremental_mark_barrier_value(child);
    Some(child_addr)
}

/// #7187: should this barrier call do remembered-set work at all?
///
/// Placed **after** [`barrier_child_prologue`] and **before** the parent
/// decode, in every entry point. Both halves of that placement are
/// load-bearing:
///
///   * After the prologue, so the #6011 fast path (any number stored into any
///     slot — the overwhelmingly common store) pays literally nothing new, and
///     so SATB/insertion shading for an in-progress incremental cycle is never
///     skipped. An incremental cycle implies a collection has run implies
///     armed, so this could not bite today; writing the order down keeps a
///     later refactor from hoisting the check above the shading.
///   * Before the parent decode, so the unarmed window also skips
///     `decode_heap_addr`'s raw-pointer arm — itself a
///     `classify_heap_generation` on the bare-`u64` entry point.
///
/// Cost once armed: one relaxed load of a `static` (`adrp`/`ldr`) plus a
/// perfectly-predicted, permanently-taken branch.
#[inline]
pub(super) fn barrier_remembering_active() -> bool {
    if barrier_remembering_armed() {
        return true;
    }
    bump_write_barrier_trace_counter(BarrierTraceCounter::UnarmedSkips);
    false
}

//! Repsel #7480: the per-array **homogeneous element-shape invariant** —
//! "every element of this array is an object of class `C`".
//!
//! ## Why this layer exists
//!
//! `keep[j].v` measures 6.2× vs node on the pure shape (#7480). The cost is
//! two *stacked* inline guard diamonds per access: the element-read tier
//! proves `keep[j]` is an in-bounds slot of a real array, then the field-read
//! precheck re-proves that the value it produced is an object of the right
//! shape. Both candidate fixes — extending the #5093 versioned-loop clone
//! (hoist ONE whole-array guard into the preheader) and full element
//! `Ptr<Shape>` representation — need the *same* missing fact: an O(1),
//! array-level answer to "are all the elements the same shape?". The only
//! array-level invariants that existed before this module were the numeric
//! ones (`GC_ARRAY_RAW_F64_LAYOUT` / `_HOLES`).
//!
//! **This module is the invariant only.** Nothing consumes it yet, by
//! design: #6377's lesson is that every added proof un-gates latent fast
//! paths its own microbench never exercises, so the proof lands and is
//! tested on its own before a consumer reads it. No emitted code changes.
//!
//! ## Storage — deliberately the same shape as Phase 4a's dense bit
//!
//! 4a's `GC_ARRAY_RAW_F64_LAYOUT` is a bit in `GcHeader._reserved` plus a
//! rebuild-on-demand scan, and it works because the collector copies the
//! whole `_reserved` word when it moves an object — the invariant survives a
//! copying minor with no side-table walk and no per-move bookkeeping. This
//! module mirrors that, point for point:
//!
//! | | 4a (`RAW_F64_LAYOUT`) | here (`ELEMENT_SHAPE`) |
//! |---|---|---|
//! | fast proof | `_reserved` bit 7 | `_reserved` bit 11 |
//! | rides a move | yes (`_reserved` is copied) | yes (same word) |
//! | self-heals by rescan | `ensure_array_numeric_raw_f64` | [`ensure_element_shape`] |
//! | move fixup | `transfer_array_numeric_layout` | [`transfer_element_shape`] |
//! | clear funnel | `clear_array_numeric_layout` | [`clear_element_shape`] |
//!
//! The one thing 4a does not need is a *payload*: "raw f64" is the whole
//! fact, so the bit is the whole record. A shape id does not fit in a bit,
//! so it lives in `ELEMENT_SHAPES`, keyed by the array's user address and
//! moved by [`transfer_element_shape`] from inside `layout_transfer` —
//! exactly how `TYPED_LAYOUTS` is moved. **The bit stays the authority.** A
//! fresh allocation's `_reserved` is zero, so a stale record left at a
//! recycled address is unreachable, and a missing record fails closed.
//!
//! The two are mutually exclusive by construction: an element-shape array's
//! slots are NaN-boxed pointers, which no raw-f64 layout admits, so
//! `set_array_numeric_layout` clears this bit and vice versa.
//!
//! ## The record holds no heap pointer
//!
//! [`ElementShapeRecord`] is four plain integers. It is **not** a cache of a
//! raw heap pointer, so it is deliberately NOT registered with
//! `gc_register_mutable_root_scanner` — there is nothing in it for the
//! collector to mark or rewrite. The `class_id` is a registry index; the key
//! is only ever compared, never dereferenced. Dead keys are dropped by
//! [`prune_dead_element_shape_owners`] on the same collection hook that
//! prunes `ARRAY_NAMED_PROPS` — a footprint concern only, since the bit on a
//! recycled allocation's fresh `_reserved` is zero.
//!
//! ## Two counters, two different questions
//!
//! * [`element_shape_epoch`] — monotone, bumped on **every** clear or
//!   invalidation, of any array. This is the word a consumer's hoisted guard
//!   re-reads: "has anything that could retire any element-shape proof
//!   happened since the preheader?" One relaxed load, one compare, no
//!   side-table probe, no rescan. Deliberately coarse — an unrelated array's
//!   clear deopts a running loop, which errs in the safe direction.
//! * `CLASS_SHAPE_GENERATION` — bumped only when a *class* stops being a
//!   reliable shape (prototype surgery). A record installed under an older
//!   generation is retired lazily on its next query, so one prototype write
//!   retires every outstanding record at O(1) without enumerating arrays.
//!
//! Both follow the crate's convention for invalidation counters (`AtomicU64`
//! starting at 1, `PROP_PLAN_EPOCH` / `PERRY_IC_EPOCH`), not a per-thread
//! `Cell`: the class registry a generation bump answers to is process-wide.
//!
//! ## Verified length: the structural half of the invalidation matrix
//!
//! The record pins the `length` it was verified against, and a query
//! requires the array's current `length` to still match. Every operation
//! that changes `length` outside the store funnels — `pop`, `shift`,
//! `splice`, `length = n`, sparse extend, codegen's inline append — is
//! therefore invalidated *automatically* on the next query, with no call
//! site of its own. That is what keeps the invalidation matrix small enough
//! to enumerate.

use super::header::{
    array_elements_ptr, array_gc_header, array_object_flags, clean_arr_ptr, clean_arr_ptr_mut,
};
use super::ArrayHeader;
use crate::gc::GC_ARRAY_ELEMENT_SHAPE;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Same ceiling the raw-f64 rebuild walk uses — an array claiming a length
/// past this is corrupt or pathological and gets no proof.
const MAX_VERIFIED_LEN: usize = 16_000_000;

/// The side-table half of the invariant. Four integers; **no heap pointer**
/// (see the module docs on why this is not a GC root).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ElementShapeRecord {
    /// `ObjectHeader::class_id` shared by every element in `[0, length)`.
    class_id: u32,
    /// The `length` this record was verified against. A query requires the
    /// array's current `length` to still equal it, so every length-changing
    /// mutation invalidates the proof without needing its own call site.
    verified_len: u32,
    /// Bumped each time THIS array's invariant is cleared, so a consumer
    /// holding a proof can tell "still the same proof" from "re-established
    /// with the same class after a break".
    epoch: u32,
    /// `CLASS_SHAPE_GENERATION` at install time. A prototype write bumps the
    /// global and retires every record at once.
    generation: u64,
}

/// What a query hands back. Deliberately not the raw record: `generation` is
/// a validity input, not a consumer-visible fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ElementShapeProof {
    pub(crate) class_id: u32,
    pub(crate) verified_len: u32,
    pub(crate) epoch: u32,
}

thread_local! {
    /// Address-keyed element-shape records. `PtrHashMap` for the same reason
    /// `ARRAY_NAMED_PROPS` uses it (#6386): the key is already a
    /// well-distributed address and SipHash dominates the probe.
    ///
    /// Thread-local because arenas are: an array address is only meaningful
    /// on the thread that allocated it.
    static ELEMENT_SHAPES: RefCell<crate::fast_hash::PtrHashMap<usize, ElementShapeRecord>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Bumped on every clear/invalidation. See the module docs.
static ELEMENT_SHAPE_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Bumped only when a class stops being a reliable shape.
static CLASS_SHAPE_GENERATION: AtomicU64 = AtomicU64::new(1);

/// The consumer-facing "nothing has been retired" word. See the module docs.
#[inline]
pub(crate) fn element_shape_epoch() -> u64 {
    ELEMENT_SHAPE_EPOCH.load(Ordering::Relaxed)
}

#[inline]
fn bump_epoch() {
    ELEMENT_SHAPE_EPOCH.fetch_add(1, Ordering::Relaxed);
}

#[inline]
fn class_shape_generation() -> u64 {
    CLASS_SHAPE_GENERATION.load(Ordering::Relaxed)
}

/// Retire **every** outstanding element-shape record at O(1).
///
/// Called when a class stops being a reliable shape — a method written onto
/// a prototype object, a `[[Prototype]]` swap. Records are not enumerated:
/// each carries the generation it was installed under and fails its next
/// query, which clears the bit. An array that is still homogeneous
/// self-heals on the next [`ensure_element_shape`].
pub(crate) fn invalidate_all_element_shapes() {
    CLASS_SHAPE_GENERATION.fetch_add(1, Ordering::Relaxed);
    bump_epoch();
}

/// The class id an element must have to keep the invariant, or `None` if the
/// value cannot participate at all.
///
/// Strict on purpose. `POINTER_TAG` alone is not enough: `RegExpHeader` is
/// also tagged `GC_TYPE_OBJECT` (see the aliasing caution on `ObjectMeta`),
/// and reading `class_id` off a non-`ObjectHeader` payload yields garbage
/// that would then be *compared equal* across two unrelated arrays.
/// Requiring `object_type == OBJECT_TYPE_REGULAR` and a nonzero class id
/// keeps every accepted value a genuine shaped instance.
#[inline]
pub(crate) fn element_class_of_bits(value_bits: u64) -> Option<u32> {
    if value_bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (value_bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(addr) {
        return None;
    }
    unsafe {
        let header =
            (addr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let obj = addr as *const crate::object::ObjectHeader;
        if (*obj).object_type != crate::error::OBJECT_TYPE_REGULAR {
            return None;
        }
        let class_id = (*obj).class_id;
        if class_id == 0 {
            return None;
        }
        Some(class_id)
    }
}

#[inline]
unsafe fn header_has_bit(header: *const crate::gc::GcHeader) -> bool {
    (*header)._reserved & GC_ARRAY_ELEMENT_SHAPE != 0
}

#[inline]
unsafe fn set_bit(header: *mut crate::gc::GcHeader) {
    (*header)._reserved |= GC_ARRAY_ELEMENT_SHAPE;
}

#[inline]
unsafe fn clear_bit(header: *mut crate::gc::GcHeader) {
    (*header)._reserved &= !GC_ARRAY_ELEMENT_SHAPE;
}

/// An array carrying per-index descriptors (accessors, custom attrs) can
/// hand back a value its element slot does not hold, so no element proof is
/// admissible — the same stand-down the raw-f64 element accessors make on
/// `OBJ_FLAG_ARRAY_DESCRIPTORS`.
#[inline]
unsafe fn array_admits_element_proof(arr: *const ArrayHeader) -> bool {
    array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS == 0
}

#[inline]
fn record_for(key: usize) -> Option<ElementShapeRecord> {
    ELEMENT_SHAPES.with(|m| m.borrow().get(&key).copied())
}

/// Drop the invariant for `arr` and bump the global epoch.
///
/// Idempotent and cheap when nothing was set: the bit test short-circuits
/// before the side-table borrow, so the overwhelming majority of arrays
/// (which never carry the invariant) pay one predictable-not-taken branch.
#[inline]
pub(crate) unsafe fn clear_element_shape(arr: *const ArrayHeader) {
    let Some(header) = array_gc_header(arr) else {
        return;
    };
    if !header_has_bit(header) {
        return;
    }
    clear_bit(header);
    let key = arr as usize;
    ELEMENT_SHAPES.with(|m| {
        // Keep the record, with a bumped per-array epoch, so a later
        // re-establishment is distinguishable from the proof just retired.
        // The bit — not the record — is the authority, so a retained record
        // under a cleared bit proves nothing.
        if let Some(record) = m.borrow_mut().get_mut(&key) {
            record.epoch = record.epoch.wrapping_add(1);
        }
    });
    bump_epoch();
}

/// Address-keyed sibling of [`clear_element_shape`], for the `layout_*`
/// family and other callers that hold a `usize`.
#[inline]
pub(crate) fn clear_element_shape_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    unsafe { clear_element_shape(user_ptr as *const ArrayHeader) }
}

/// Forget everything about an address, bit included. Used when an allocation
/// dies and its address may be recycled (`layout_clear_for_ptr`).
pub(crate) fn forget_element_shape(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    unsafe { clear_element_shape(user_ptr as *const ArrayHeader) };
    ELEMENT_SHAPES.with(|m| {
        m.borrow_mut().remove(&user_ptr);
    });
}

/// Install the invariant for `arr` at `class_id`, verified over
/// `[0, verified_len)`.
unsafe fn install(arr: *mut ArrayHeader, class_id: u32, verified_len: u32) {
    let Some(header) = array_gc_header(arr) else {
        return;
    };
    let key = arr as usize;
    let generation = class_shape_generation();
    ELEMENT_SHAPES.with(|m| {
        let mut map = m.borrow_mut();
        let epoch = map.get(&key).map_or(1, |r| r.epoch);
        map.insert(
            key,
            ElementShapeRecord {
                class_id,
                verified_len,
                epoch,
                generation,
            },
        );
    });
    set_bit(header);
}

/// The O(1) query: does `arr` still carry a homogeneous element-shape proof?
///
/// Self-healing in the invalidating direction — a record that lost its
/// generation, or whose `verified_len` no longer matches the array's
/// `length`, clears the bit here rather than lingering as a lie. Never
/// *establishes* anything; that is [`ensure_element_shape`].
#[inline]
pub(crate) unsafe fn element_shape_proof(arr: *const ArrayHeader) -> Option<ElementShapeProof> {
    let arr = clean_arr_ptr(arr);
    let header = array_gc_header(arr)?;
    if !header_has_bit(header) {
        return None;
    }
    let Some(record) = record_for(arr as usize) else {
        // Fail closed: the bit rode a move `layout_transfer` did not fix up,
        // or the record was pruned. Clearing keeps the halves in agreement.
        clear_bit(header);
        bump_epoch();
        return None;
    };
    if record.generation != class_shape_generation()
        || record.verified_len != (*arr).length
        || !array_admits_element_proof(arr)
    {
        clear_element_shape(arr);
        return None;
    }
    Some(ElementShapeProof {
        class_id: record.class_id,
        verified_len: record.verified_len,
        epoch: record.epoch,
    })
}

/// Establish the invariant by scanning, if it holds — the self-healing entry
/// point, and the direct analogue of `ensure_array_numeric_raw_f64`.
///
/// Returns the existing proof without scanning when the bit is already
/// valid, so a consumer may call this unconditionally in a preheader and pay
/// O(n) only on the first visit (or after a break).
pub(crate) unsafe fn ensure_element_shape(arr: *mut ArrayHeader) -> Option<ElementShapeProof> {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return None;
    }
    if let Some(proof) = element_shape_proof(arr) {
        return Some(proof);
    }
    array_gc_header(arr)?;
    if !array_admits_element_proof(arr) {
        return None;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    // An empty array has no element shape to prove: the fact would be
    // vacuous and a consumer guarding on it would licence reads that cannot
    // happen. Decline rather than install a class-less proof.
    if length == 0 || length > capacity || length > MAX_VERIFIED_LEN {
        return None;
    }
    let elements = array_elements_ptr(arr);
    let class_id = element_class_of_bits(*elements)?;
    for i in 1..length {
        if element_class_of_bits(*elements.add(i)) != Some(class_id) {
            return None;
        }
    }
    install(arr, class_id, length as u32);
    element_shape_proof(arr)
}

/// The single element-store hook, called from `gc::layout_note_slot` — the
/// one funnel **both** the runtime's element-store helpers and codegen's
/// inline element stores already pass through.
///
/// Routing it there rather than through `array::note_array_slot` is what
/// makes the invariant hold against emitted code: `array_store_needs_layout_note`
/// elides the note only when the array is *statically proven numeric and
/// pointer-free*, which an element-shape array can never be, so every
/// codegen-inlined store into a shape-proven array reaches this hook. The
/// call sits before `layout_note_slot`'s `GC_LAYOUT_UNKNOWN` early return
/// (an all-pointer array is marked unknown on its first generic write) and
/// costs a `GC_TYPE_ARRAY` compare plus a bit test on a header word that
/// line is about to read anyway.
///
/// Three cases, each with its own named test:
///
/// * **establish** — no proof yet, this store writes element 0 of an empty
///   array, and the value is a shaped object. That is the
///   `const rows = []; rows.push(new C(…))` construction shape, which is
///   exactly the form the compile-time collector already admits (#7034
///   E1/E2). Growth beyond element 0 then rides the *keep* case.
/// * **keep** — a proof exists and the value's class matches. A contiguous
///   append additionally extends `verified_len`.
/// * **clear** — anything else: a different class, a non-pointer, a
///   `TAG_HOLE` (so `delete arr[i]` and `arr.length = n`, which both funnel
///   `TAG_HOLE` stores through here, are covered with no call site of their
///   own), or a store that would leave a gap.
///
/// `arr` must already be forwarding-resolved; `layout_note_slot` chases the
/// chain before calling.
#[inline]
pub(crate) unsafe fn note_element_store(arr: *mut ArrayHeader, index: usize, value_bits: u64) {
    let Some(header) = array_gc_header(arr) else {
        return;
    };
    if !header_has_bit(header) {
        // Establish only from empty. Adopting a longer array here would
        // claim a prefix was verified when it never was.
        if index == 0 && (*arr).length == 0 && array_admits_element_proof(arr) {
            if let Some(class_id) = element_class_of_bits(value_bits) {
                install(arr, class_id, 1);
            }
        }
        return;
    }
    let Some(record) = record_for(arr as usize) else {
        clear_element_shape(arr);
        return;
    };
    // A store past the end of the verified prefix leaves a gap of holes.
    if index > record.verified_len as usize {
        clear_element_shape(arr);
        return;
    }
    if element_class_of_bits(value_bits) != Some(record.class_id) {
        clear_element_shape(arr);
        return;
    }
    if index == record.verified_len as usize {
        // Contiguous append. Callers bump `length` immediately after the
        // store, so the record leads it for exactly the store's duration.
        install(arr, record.class_id, record.verified_len.saturating_add(1));
    }
}

/// Move the record when the array's storage moves — growth forwarding, a
/// copying minor, an old-gen defrag. Called from `layout_transfer`, the one
/// funnel every relocation already goes through.
///
/// `_reserved` (and with it the bit) is copied verbatim by both the
/// collector and `js_array_grow`, so the bit is normally already correct on
/// `new_user`; this sets it explicitly anyway so the two halves cannot
/// disagree if a future move path forgets the word copy.
pub(crate) fn transfer_element_shape(old_user: usize, new_user: usize) {
    if old_user == 0 || new_user == 0 || old_user == new_user {
        return;
    }
    let moved = ELEMENT_SHAPES.with(|m| {
        let mut map = m.borrow_mut();
        map.remove(&new_user);
        match map.remove(&old_user) {
            Some(record) => {
                map.insert(new_user, record);
                true
            }
            None => false,
        }
    });
    unsafe {
        let Some(new_header) = array_gc_header(new_user as *const ArrayHeader) else {
            return;
        };
        let had_bit = array_gc_header(old_user as *const ArrayHeader)
            .is_some_and(|old_header| header_has_bit(old_header));
        if moved && had_bit {
            set_bit(new_header);
        } else {
            // Fail closed. The epoch is deliberately NOT bumped for a plain
            // move: no proof was retired, there was never one to retire.
            clear_bit(new_header);
        }
    }
}

/// Drop records whose array owners are provably dead, on the same collection
/// hook that prunes `ARRAY_NAMED_PROPS`. Footprint only — a stale record can
/// never be *read*, because a recycled allocation's fresh `_reserved` is
/// zero.
pub(crate) fn prune_dead_element_shape_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    ELEMENT_SHAPES.with(|m| {
        m.borrow_mut().retain(|owner, _| !is_dead_owner(*owner));
    });
}

// ---------------------------------------------------------------------------
// FFI surface — the codegen-observable query. No emitted code calls these
// yet (#7480 ships the invariant without a consumer); they are the contract
// the #5093 versioned-loop clone will guard on.
// ---------------------------------------------------------------------------

/// Establish-or-confirm, returning the proven `class_id` or `0`. The
/// preheader entry point: O(n) on first visit, O(1) afterwards.
#[no_mangle]
pub extern "C" fn js_array_ensure_element_shape(arr: *mut ArrayHeader) -> i32 {
    unsafe { ensure_element_shape(arr).map_or(0, |p| p.class_id as i32) }
}

/// The O(1) query with no scan: the proven `class_id`, or `0`.
#[no_mangle]
pub extern "C" fn js_array_element_shape_class(arr: *const ArrayHeader) -> i32 {
    unsafe { element_shape_proof(arr).map_or(0, |p| p.class_id as i32) }
}

/// The per-array proof identity a consumer pins alongside the class id.
#[no_mangle]
pub extern "C" fn js_array_element_shape_version(arr: *const ArrayHeader) -> i64 {
    unsafe { element_shape_proof(arr).map_or(-1, |p| i64::from(p.epoch)) }
}

/// The global "nothing was retired" word. One relaxed load; see module docs.
#[no_mangle]
pub extern "C" fn js_array_element_shape_epoch() -> i64 {
    element_shape_epoch() as i64
}

/// Re-validate a hoisted guard without rescanning: the array still proves
/// `class_id`, and it is still the same proof (`epoch`).
#[no_mangle]
pub extern "C" fn js_array_element_shape_check(
    arr: *const ArrayHeader,
    class_id: i32,
    epoch: i64,
) -> i32 {
    unsafe {
        match element_shape_proof(arr) {
            Some(p) if p.class_id as i32 == class_id && i64::from(p.epoch) == epoch => 1,
            _ => 0,
        }
    }
}

// `#[no_mangle]` exports for generated code, so release/LTO builds cannot
// internalize and strip them in the window before a consumer exists.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_ENSURE_ELEMENT_SHAPE: extern "C" fn(*mut ArrayHeader) -> i32 =
    js_array_ensure_element_shape;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_ELEMENT_SHAPE_CLASS: extern "C" fn(*const ArrayHeader) -> i32 =
    js_array_element_shape_class;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_ELEMENT_SHAPE_VERSION: extern "C" fn(*const ArrayHeader) -> i64 =
    js_array_element_shape_version;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_ELEMENT_SHAPE_EPOCH: extern "C" fn() -> i64 = js_array_element_shape_epoch;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_ELEMENT_SHAPE_CHECK: extern "C" fn(*const ArrayHeader, i32, i64) -> i32 =
    js_array_element_shape_check;

#[cfg(test)]
pub(crate) fn test_element_shape_record_exists(owner: usize) -> bool {
    ELEMENT_SHAPES.with(|m| m.borrow().contains_key(&owner))
}

#[cfg(test)]
pub(crate) fn test_clear_element_shape_table() {
    ELEMENT_SHAPES.with(|m| m.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) unsafe fn test_element_shape_bit_set(arr: *const ArrayHeader) -> bool {
    array_gc_header(arr).is_some_and(|header| header_has_bit(header))
}

#[cfg(test)]
#[path = "element_shape_tests.rs"]
mod tests;

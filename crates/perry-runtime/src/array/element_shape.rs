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
//! A third counter, `ELEMENT_SHAPE_PROOF_SEQ`, is not an invalidation signal
//! but an identity source: every *established* proof takes the next value and
//! carries it unchanged while it is kept or extended. Because identities are
//! never reused and are never read back from the table, a record that
//! outlives its array — left by a fail-closed transfer, or by a death whose
//! prune has not run yet — cannot donate its identity to whatever is
//! established at that recycled address next.
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
    /// This proof's identity, drawn from `ELEMENT_SHAPE_PROOF_SEQ` when the
    /// proof is *established*, and carried unchanged while it is merely kept
    /// or extended. Values are never reused, so a consumer that pinned
    /// `(class_id, epoch)` can never be fooled by the same array being
    /// re-proven with the same class after a break — nor by a *different*
    /// array being established at a recycled address.
    epoch: u64,
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
    pub(crate) epoch: u64,
}

crate::perry_thread_local! {
    /// Address-keyed element-shape records. `PtrHashMap` for the same reason
    /// `ARRAY_NAMED_PROPS` uses it (#6386): the key is already a
    /// well-distributed address and SipHash dominates the probe.
    ///
    /// Thread-local because arenas are: an array address is only meaningful
    /// on the thread that allocated it.
    static ELEMENT_SHAPES: RefCell<crate::fast_hash::PtrHashMap<usize, ElementShapeRecord>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

// #7946: all three counters are `per_test_global!`, so a test build gives each
// libtest thread its own instance and a product build gets the plain `static`
// back, byte for byte.
//
// The records they validate (`ELEMENT_SHAPES`) are already thread-local, so a
// process-wide generation was never *reachable* by a well-formed cross-thread
// read — it only reached other tests. And it did: `CLASS_SHAPE_GENERATION` is
// bumped by `invalidate_all_element_shapes()`, i.e. by ANY test anywhere in the
// crate that writes a prototype method or swaps a `[[Prototype]]`
// (`object::class_registry::prototype_methods`, `object::prototype_chain`).
// None of those take `ELEMENT_SHAPE_TEST_LOCK`, and asking them to would be the
// opt-in-defence the `per_test_global!` module docs exist to argue against.
// The symptom was `js_array_element_shape_class` returning **0** for an array
// this thread had just proven, 8 runs in 100 (`gc::tests::layout_trace::
// element_shape`, both cases).
per_test_global! {
    /// Bumped on every clear/invalidation. See the module docs.
    static ELEMENT_SHAPE_EPOCH: AtomicU64 = AtomicU64::new(1);

    /// Bumped only when a class stops being a reliable shape.
    static CLASS_SHAPE_GENERATION: AtomicU64 = AtomicU64::new(1);

    /// Hands out a fresh identity to every proof that is *established*. Values
    /// are never reused, which is what makes address recycling safe: a record
    /// that survives at an address its array no longer owns cannot donate its
    /// identity to whatever is established there next, because establishing
    /// always takes a new number rather than reading the old one.
    static ELEMENT_SHAPE_PROOF_SEQ: AtomicU64 = AtomicU64::new(1);
}

/// A shared serialization guard for the element-shape test files.
///
/// **Since #7946 this is belt-and-braces, not the defence.** The counters above
/// are `per_test_global!` and `ELEMENT_SHAPES` was already thread-local, so a
/// concurrent test cannot reach this test's state at all — which is the point,
/// because the lock never covered the mutators that actually broke these tests
/// (a prototype write anywhere in the crate). It is kept only so a case that
/// spans several mutations still reads as ordered, and it is deliberately
/// poison-tolerant (#7492): a panicking test must not cascade into every other
/// one.
#[cfg(test)]
pub(crate) static ELEMENT_SHAPE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take [`ELEMENT_SHAPE_TEST_LOCK`], recovering from poisoning.
///
/// Declare this guard **first** in a test, before any `…TestGuard`, so unwind
/// order drops the state-restoring guards while this one is still held.
#[cfg(test)]
pub(crate) fn test_serialize() -> std::sync::MutexGuard<'static, ()> {
    ELEMENT_SHAPE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
    unsafe {
        // The canonical validated header read: it rejects the handle bands and
        // implausible magnitudes BEFORE touching `addr - GC_HEADER_SIZE`, and
        // it also rejects small-buffer slab addresses, which are heap-plausible
        // but carry no `GcHeader` at all.
        let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
            return None;
        };
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
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
    // Drop the record outright. Keeping a retired one buys nothing —
    // identities come from `ELEMENT_SHAPE_PROOF_SEQ`, never from whatever
    // happens to sit at the address — and a record with no bit is a
    // liability: it is the thing a later establishment could read.
    ELEMENT_SHAPES.with(|m| {
        m.borrow_mut().remove(&key);
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

/// **Establish** the invariant for `arr` at `class_id`, verified over
/// `[0, verified_len)`, under a brand-new proof identity.
///
/// The identity is drawn from `ELEMENT_SHAPE_PROOF_SEQ` rather than read back
/// from whatever record happens to sit at this address. That is the whole
/// defence against address recycling: a survivor record — left by a
/// fail-closed transfer, a dead array whose prune has not run yet — can never
/// donate its identity to the array established here next.
unsafe fn establish(arr: *mut ArrayHeader, class_id: u32, verified_len: u32) {
    let Some(header) = array_gc_header(arr) else {
        return;
    };
    let record = ElementShapeRecord {
        class_id,
        verified_len,
        epoch: ELEMENT_SHAPE_PROOF_SEQ.fetch_add(1, Ordering::Relaxed),
        generation: class_shape_generation(),
    };
    ELEMENT_SHAPES.with(|m| {
        m.borrow_mut().insert(arr as usize, record);
    });
    set_bit(header);
}

/// **Keep** an existing proof while extending its verified prefix. The
/// identity is carried unchanged — a consumer that pinned it stays valid,
/// which is the point: appending a matching element does not retire anything.
unsafe fn extend_verified_len(arr: *mut ArrayHeader, record: ElementShapeRecord, new_len: u32) {
    if array_gc_header(arr).is_none() {
        return;
    }
    ELEMENT_SHAPES.with(|m| {
        m.borrow_mut().insert(
            arr as usize,
            ElementShapeRecord {
                verified_len: new_len,
                ..record
            },
        );
    });
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
    establish(arr, class_id, length as u32);
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
                establish(arr, class_id, 1);
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
        extend_verified_len(arr, record, record.verified_len.saturating_add(1));
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
    unsafe {
        let Some(new_header) = array_gc_header(new_user as *const ArrayHeader) else {
            return;
        };
        // Decide BEFORE touching the table. Moving the record first and then
        // discovering the source had no bit would leave an ORPHAN record at
        // `new_user`: the bit is the authority so no proof is readable, but a
        // later `install` at that address inherits the orphan's `epoch` and
        // silently continues a *different* array's proof identity — which is
        // exactly the versioning a consumer guards on.
        let had_bit = array_gc_header(old_user as *const ArrayHeader)
            .is_some_and(|old_header| header_has_bit(old_header));
        let moved = ELEMENT_SHAPES.with(|m| {
            let mut map = m.borrow_mut();
            map.remove(&new_user);
            if !had_bit {
                // The source proved nothing; drop its record too rather than
                // leave it addressable under a new key.
                map.remove(&old_user);
                return false;
            }
            match map.remove(&old_user) {
                Some(record) => {
                    map.insert(new_user, record);
                    true
                }
                None => false,
            }
        });
        if moved {
            set_bit(new_header);
        } else {
            // Fail closed, with no record left behind at `new_user`. The
            // epoch is deliberately NOT bumped for a plain move: no proof was
            // retired, there was never one to retire.
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
    unsafe { element_shape_proof(arr).map_or(-1, |p| p.epoch as i64) }
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
            Some(p) if p.class_id as i32 == class_id && p.epoch as i64 == epoch => 1,
            _ => 0,
        }
    }
}

// NOTE — exactly ONE `keepalive-anchors` `#[used]` static, deliberately.
// `keepalive-anchors` is a DEFAULT feature, so an anchor pins its symbol into
// every shipped binary: anchoring all five would be the dead-strip defeat the
// hello-size campaign traced its regression to. #5093's element-shape
// versioned-loop clone emits a call to exactly one of them
// (`js_array_ensure_element_shape`, from the loop preheader), so that one —
// and only that one — is anchored. The other four stay unanchored and
// dead-strippable until something emits a call to them.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_ARRAY_ENSURE_ELEMENT_SHAPE: extern "C" fn(*mut ArrayHeader) -> i32 =
    js_array_ensure_element_shape;

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

/// The end-to-end revocation matrix: every mutator family driven through its
/// real FFI entry point. Separate from `tests` because it asserts a different
/// thing — not that the funnels work, but that the mutators *reach* them.
#[cfg(test)]
#[path = "element_shape_matrix_tests.rs"]
mod matrix_tests;

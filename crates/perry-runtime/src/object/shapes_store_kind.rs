//! Charter step 3 — the two per-object STORE facts are shape facts.
//!
//! A store `o.k = v` that hits a published word for ShapeId `S` writes a slot
//! with no `[[Set]]` walk. Two facts decide whether that is allowed, and until
//! this module they lived only on the object, so every hit re-read them:
//!
//! * **F-A, receiver kind**: may the receiver's own slots be written as plain
//!   data? Admitted: a class instance (`class_id` not 0 / `u32::MAX-1` native
//!   module / `u32::MAX`), or a class-less receiver a birth site marked
//!   `OBJ_FLAG_PLAIN_ORDINARY` that is not a typed-array prototype. Not
//!   admitted: every other class-less receiver (`URL`, `Object.prototype`, a
//!   typed-array prototype, a runtime-born record) and native-module receivers.
//! * **F-B, the Array-subclass packed-numeric proof**: an owner store
//!   must retire it first, so a proof-carrying receiver may not take a raw
//!   store.
//!
//! Both are now [`ShapeObjectKind`] values of the ordinary LAYOUT:
//!
//! | kind | F-A | F-B |
//! |---|---|---|
//! | `Ordinary` | admitted | clear |
//! | `OrdinaryUnmarked` | not admitted | clear |
//! | `OrdinaryNumericProof` | admitted | set |
//!
//! so `kind == Ordinary` is the whole store admission, and a word published
//! only for an `Ordinary` shape needs no per-object test on its hit path.
//!
//! The rules that keep the kind true (DESIGN R1-R6):
//!
//! * a mint with the receiver in hand takes its ordinary-family kind from the
//!   receiver ([`mint_kind`]), never from lineage — so no lineage can carry a
//!   stale F-A, or a proof, across a structural change;
//! * a caller-supplied id is stamped only if its kind is the receiver's
//!   ([`receiver_ordinary_kind`] at every explicit-id stamp);
//! * a writer of an input moves the shape ([`restamp_object_store_kind`]);
//! * the proof is a transition ([`stamp_numeric_proof_twin`] /
//!   [`restore_unproven_twin`]), and any other stamp of a proof-carrying
//!   receiver retires the proof first ([`retire_proof_before_stamp`]).
//!
//! F-A still has per-object inputs for R1. F-B is only the ShapeId kind;
//! its `ObjectMeta` payload stores the numeric bound and unproven twin id.
//! [`store_facts_agree`] checks receiver inputs against the shape at every
//! stamp in debug builds and with `shape-fact-audit` in release builds.

use super::{object_shape_stamp, shape_descriptor_by_id, shape_record_by_id, ShapeObjectKind};
use crate::object::ObjectHeader;

#[inline(always)]
unsafe fn gc_header(obj: *const ObjectHeader) -> *mut crate::gc::GcHeader {
    crate::object::object_ops::gc_header_for(obj)
}

/// F-A from the per-object record: the receiver's own slots may be written as
/// plain data.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_admits_plain_store(obj: *const ObjectHeader) -> bool {
    let class_id = (*obj).class_id;
    // `class_id + 2 > 2` (unsigned): a class id other than 0, u32::MAX-1
    // (native module) and u32::MAX (never allocated).
    if class_id.wrapping_add(2) > 2 {
        return true;
    }
    class_id == 0
        && (*gc_header(obj))._reserved
            & (crate::gc::OBJ_FLAG_PLAIN_ORDINARY | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            == crate::gc::OBJ_FLAG_PLAIN_ORDINARY
}

/// F-B from the receiver's authoritative ShapeId.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_carries_numeric_proof(obj: *const ObjectHeader) -> bool {
    super::shape_object_kind_by_id(object_shape_stamp(obj))
        == Some(ShapeObjectKind::OrdinaryNumericProof)
}

/// R1: the ordinary-family kind a NEW shape of `obj` must have. Never the
/// proof kind — only [`stamp_numeric_proof_twin`] enters that.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` `ObjectHeader`.
#[inline]
pub(crate) unsafe fn receiver_ordinary_kind(obj: *const ObjectHeader) -> ShapeObjectKind {
    if receiver_admits_plain_store(obj) {
        ShapeObjectKind::Ordinary
    } else {
        ShapeObjectKind::OrdinaryUnmarked
    }
}

/// R2: the kind a mint for `obj` publishes, given the kind its lineage (or
/// caller) supplies. An ordinary-family kind is re-derived from the receiver;
/// every other kind (class object, dictionary, function) carries over.
///
/// # Safety
/// `obj` is null or a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn mint_kind(
    lineage: ShapeObjectKind,
    obj: *const ObjectHeader,
) -> ShapeObjectKind {
    if lineage.is_ordinary_layout() && !obj.is_null() && crate::object::object_is_shaped(obj) {
        receiver_ordinary_kind(obj)
    } else {
        lineage
    }
}

/// The store admission, from the shape alone: `S` is a shape a plain-data
/// store may be served on. What every word publisher and every region that
/// stores asks (charter step 3; the 4b lane's store regions pack with it).
#[inline]
pub(crate) fn shape_admits_plain_store(shape_id: u32) -> bool {
    super::shape_object_kind_by_id(shape_id) == Some(ShapeObjectKind::Ordinary)
}

/// R6, the invariant: `obj`'s shape kind agrees with its per-object record.
/// A receiver with no ordinary-family shape must not carry the proof.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` `ObjectHeader`.
pub(crate) unsafe fn store_facts_agree(obj: *const ObjectHeader) -> bool {
    let Some(kind) = super::shape_object_kind_by_id(object_shape_stamp(obj)) else {
        return true;
    };
    let proof = receiver_carries_numeric_proof(obj);
    if !kind.is_ordinary_layout() {
        return !proof;
    }
    let expected = if proof {
        if !receiver_admits_plain_store(obj) {
            return false;
        }
        ShapeObjectKind::OrdinaryNumericProof
    } else {
        receiver_ordinary_kind(obj)
    };
    kind == expected
}

/// Assert R6 for `obj` (debug builds, and every build with
/// `shape-fact-audit`). Every stamp funnel and every input writer calls it.
///
/// # Safety
/// `obj` is a live `GC_TYPE_OBJECT` `ObjectHeader`.
#[inline]
pub(crate) unsafe fn check_store_facts(obj: *const ObjectHeader) {
    if cfg!(any(debug_assertions, feature = "shape-fact-audit")) {
        audit_check(obj);
    }
}

#[cold]
#[inline(never)]
unsafe fn audit_check(obj: *const ObjectHeader) {
    if obj.is_null() || !crate::object::object_is_shaped(obj) {
        return;
    }
    audit::note_check();
    if !store_facts_agree(obj) {
        report_disagreement(obj);
    }
}

#[cold]
#[inline(never)]
unsafe fn report_disagreement(obj: *const ObjectHeader) -> ! {
    let id = object_shape_stamp(obj);
    panic!(
        "shape store facts disagree: obj={:#x} shape={:#x} kind={:?} class_id={:#x} reserved={:#06x}",
        obj as usize,
        id,
        super::shape_object_kind_by_id(id),
        (*obj).class_id,
        (*gc_header(obj))._reserved
    );
}

/// Stamp a TWIN of the receiver's current shape — the same identity facts,
/// another kind — without the structural-stamp side effects: a twin changes
/// no key, attribute or prototype, so nothing that inherits from `obj` can
/// observe it (no prototype-validity bump). The old-carrier note is kept.
///
/// # Safety
/// `obj` is a live shaped `ObjectHeader`; `id` names a live descriptor.
unsafe fn stamp_twin(obj: *mut ObjectHeader, id: u32) {
    (*obj).parent_class_id = id;
    if !crate::arena::pointer_in_nursery(obj as usize) {
        let record = shape_record_by_id(id);
        super::note_old_generation_carrier(record);
        if let Some(record) = record {
            super::note_shape_carrier_candidate(record.keys());
        }
    }
}

/// Mint (or find) the twin of `shape_id` with `kind`. Never reads the keys
/// array (see [`super::shape_descriptor_kind_twin`]), so it is safe on the
/// proof-retire path inside the owner store funnel.
fn twin_of(shape_id: u32, kind: ShapeObjectKind) -> Option<u32> {
    if super::shape_object_kind_by_id(shape_id)? != kind {
        audit::note_twin_mint();
    }
    super::shape_descriptor_kind_twin(shape_id, kind)
}

/// R4: after a write to an F-A input (`class_id`, `OBJ_FLAG_PLAIN_ORDINARY`,
/// `OBJ_FLAG_TYPED_ARRAY_PROTO`), move `obj` to the twin whose kind the
/// receiver now derives. A no-op for an unstamped receiver (its first mint
/// derives the kind), a non-ordinary kind, or a kind that already agrees.
///
/// # Safety
/// `obj` is null or a live `ObjectHeader`.
pub(crate) unsafe fn restamp_object_store_kind(obj: *mut ObjectHeader) {
    if obj.is_null() || !crate::object::object_is_shaped(obj) {
        return;
    }
    let id = object_shape_stamp(obj);
    let Some(kind) = super::shape_object_kind_by_id(id) else {
        return;
    };
    if !kind.is_ordinary_layout() {
        return;
    }
    let target = receiver_ordinary_kind(obj);
    if kind == target
        || (kind == ShapeObjectKind::OrdinaryNumericProof && target == ShapeObjectKind::Ordinary)
    {
        check_store_facts(obj);
        return;
    }
    if kind == ShapeObjectKind::OrdinaryNumericProof {
        // A proven receiver lost F-A (a post-birth class-id rewrite): the
        // proof goes with it.
        crate::array::clear_packed_subclass_numeric_proof(obj);
    }
    audit::note_restamp();
    let current = object_shape_stamp(obj);
    if let Some(twin) = twin_of(current, target) {
        stamp_twin(obj, twin);
    }
    check_store_facts(obj);
}

/// `mark_object_plain_ordinary` (R4): set the birth mark, then move the shape.
///
/// # Safety
/// `obj` is null or a live `GC_TYPE_OBJECT` `ObjectHeader`.
pub(crate) unsafe fn mark_plain_ordinary(obj: *mut ObjectHeader) {
    if obj.is_null() {
        return;
    }
    (*gc_header(obj))._reserved |= crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
    restamp_object_store_kind(obj);
}

/// Set the plain-ordinary birth mark on a newborn BEFORE its first stamp, so
/// the birth mint derives `Ordinary` directly and no twin is ever minted.
///
/// # Safety
/// `obj` is a live, unpublished `GC_TYPE_OBJECT` newborn.
#[inline]
pub(crate) unsafe fn premark_plain_ordinary(obj: *mut ObjectHeader) {
    (*gc_header(obj))._reserved |= crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
}

/// R5, publish: move a receiver whose proof payload was just written to the
/// proof twin of its current (`Ordinary`) shape.
/// Returns the unproven shape the payload must name, or `None` when the
/// receiver cannot carry the proof (its shape is not `Ordinary`).
///
/// # Safety
/// `obj` is a live shaped `GC_TYPE_OBJECT` `ObjectHeader`.
pub(crate) unsafe fn stamp_numeric_proof_twin(obj: *mut ObjectHeader) -> Option<u32> {
    let unproven = object_shape_stamp(obj);
    if super::shape_object_kind_by_id(unproven) != Some(ShapeObjectKind::Ordinary) {
        return None;
    }
    let proven = twin_of(unproven, ShapeObjectKind::OrdinaryNumericProof)?;
    stamp_twin(obj, proven);
    audit::note_proof_publish();
    check_store_facts(obj);
    Some(unproven)
}

/// R5, retire: the payload was just cleared; move the
/// receiver back to its unproven twin. `unproven` is the payload's shape — a
/// hit is one slab probe; a pruned or mismatched id re-mints the twin.
///
/// # Safety
/// `obj` is a live shaped `GC_TYPE_OBJECT` `ObjectHeader` whose proof payload
/// is clear.
pub(crate) unsafe fn restore_unproven_twin(obj: *mut ObjectHeader, unproven: u32) {
    let current = object_shape_stamp(obj);
    let Some(cur) = shape_descriptor_by_id(current) else {
        return;
    };
    if cur.object_kind != ShapeObjectKind::OrdinaryNumericProof {
        // Already moved by the stamp that retired the proof.
        check_store_facts(obj);
        return;
    }
    audit::note_proof_retire();
    let target = receiver_ordinary_kind(obj);
    let direct = shape_descriptor_by_id(unproven).is_some_and(|u| {
        u.object_kind == target
            && u.keys == cur.keys
            && u.logical_key_count == cur.logical_key_count
            && u.live_inline_slot_count == cur.live_inline_slot_count
            && u.semantic_generation == cur.semantic_generation
            && u.hole_count == cur.hole_count
            && u.proto_id == cur.proto_id
            && u.summary == cur.summary
    });
    let id = if direct {
        Some(unproven)
    } else {
        twin_of(current, target)
    };
    if let Some(id) = id {
        stamp_twin(obj, id);
    }
    check_store_facts(obj);
}

/// R5: called by every stamp of `new_id` on `obj` before the header store. A
/// receiver carrying the proof loses it unless `new_id` is its proof shape:
/// every structural change retires the proof itself.
///
/// # Safety
/// `obj` is a live `ObjectHeader`.
#[inline]
pub(crate) unsafe fn retire_proof_before_stamp(obj: *mut ObjectHeader, new_id: u32) {
    if (*gc_header(obj)).obj_type != crate::gc::GC_TYPE_OBJECT
        || !receiver_carries_numeric_proof(obj)
    {
        return;
    }
    retire_proof_before_stamp_slow(obj, new_id);
}

#[cold]
#[inline(never)]
unsafe fn retire_proof_before_stamp_slow(obj: *mut ObjectHeader, new_id: u32) {
    if (*gc_header(obj)).obj_type != crate::gc::GC_TYPE_OBJECT {
        return;
    }
    if super::shape_object_kind_by_id(new_id) == Some(ShapeObjectKind::OrdinaryNumericProof) {
        return;
    }
    let _ = crate::array::drop_packed_subclass_numeric_proof_record(obj);
}

/// The audit counters (`shape-fact-audit`): how often each transition and
/// check ran, reported at exit when `PERRY_SHAPE_FACT_AUDIT` is set. Also
/// the census of how often the changed paths run in real code.
pub(crate) mod audit {
    #[cfg(feature = "shape-fact-audit")]
    use std::sync::atomic::{AtomicU64, Ordering};

    #[cfg(feature = "shape-fact-audit")]
    pub(crate) static COUNTERS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    #[cfg(feature = "shape-fact-audit")]
    const NAMES: [&str; 8] = [
        "checks",
        "twin_mints",
        "fa_restamps",
        "proof_publish",
        "proof_retire",
        "heap_objects",
        "explicit_id_declines",
        "admission_reads",
    ];

    #[inline(always)]
    fn bump(_i: usize) {
        #[cfg(feature = "shape-fact-audit")]
        {
            arm();
            COUNTERS[_i].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[cfg(feature = "shape-fact-audit")]
    fn arm() {
        static ARMED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        ARMED.get_or_init(|| {
            if std::env::var_os("PERRY_SHAPE_FACT_AUDIT").is_some() {
                extern "C" fn report() {
                    let mut line = String::from("[shape-fact-audit]");
                    for (i, c) in COUNTERS.iter().enumerate() {
                        line.push_str(&format!(" {}={}", NAMES[i], c.load(Ordering::Relaxed)));
                    }
                    eprintln!("{line}");
                }
                unsafe { libc::atexit(report) };
            }
        });
    }

    #[inline(always)]
    pub(crate) fn note_check() {
        bump(0);
    }
    #[inline(always)]
    pub(crate) fn note_twin_mint() {
        bump(1);
    }
    #[inline(always)]
    pub(crate) fn note_restamp() {
        bump(2);
    }
    #[inline(always)]
    pub(crate) fn note_proof_publish() {
        bump(3);
    }
    #[inline(always)]
    pub(crate) fn note_proof_retire() {
        bump(4);
    }
    #[inline(always)]
    #[cfg_attr(not(feature = "shape-fact-audit"), allow(dead_code))]
    pub(crate) fn note_heap_object() {
        bump(5);
    }
    #[inline(always)]
    pub(crate) fn note_explicit_decline() {
        bump(6);
    }
    #[inline(always)]
    pub(crate) fn note_admission_read() {
        bump(7);
    }
}

/// `shape-fact-audit`: check R6 on EVERY live `GC_TYPE_OBJECT` in the arena.
/// Called at the sweep start of a synchronous full collection (marks final,
/// nothing swept), where "marked" is exactly "live".
pub(crate) fn audit_heap_at_full_sweep_start() {
    #[cfg(feature = "shape-fact-audit")]
    crate::gc::for_each_live_object_at_sweep_start(|obj| unsafe {
        audit::note_heap_object();
        if !store_facts_agree(obj) {
            report_disagreement(obj);
        }
    });
}

#[cfg(test)]
#[path = "shapes_store_kind_tests.rs"]
mod tests;

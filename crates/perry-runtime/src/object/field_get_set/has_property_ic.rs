//! Presence inline cache for `"k" in o` with a **constant** key.
//!
//! `in` has no cache slot today: every `"k" in o` is a bare `js_in_operator`
//! call that re-derives the receiver's keys array from its ShapeId (a shape
//! slab probe) and re-scans it, ~950 instructions for a hit on a plain object.
//! The answer it recomputes is a property of the *shape*, not of the object:
//! two objects with the same ShapeId have the same keys array, so "shape S has
//! own key K" is stable for as long as S is stamped on the receiver.
//!
//! So the site caches exactly that — one ShapeId — and the emitted guard
//! answers `true` when the receiver still carries it. Everything else calls
//! [`js_in_operator_presence_ic`], which computes the real answer and may arm
//! the site.
//!
//! # Why only positives, and why no prototype epoch is needed
//!
//! The cached claim is about an **own** key, so it does not mention the
//! prototype chain: `Object.setPrototypeOf`, a late `Proto.x = 1`, a
//! `delete Proto.x` — none of them can make an own key stop existing, so none
//! of them can invalidate a positive. A *negative* would be a claim about the
//! whole chain, and there is no prototype-chain epoch in this runtime to key
//! one on (`prototype_chain` records per-object replacements; nothing counts
//! chain mutations globally), so negatives are not cached at all — `"zz" in o`
//! keeps calling the runtime every time.
//!
//! # What invalidates a positive
//!
//! Only losing the key, and every way of losing it moves the receiver off the
//! cached ShapeId or off the guard:
//!
//! * A compacting `delete` rebuilds the keys array and publishes a new
//!   ShapeId — the stamp at header offset 4 no longer matches.
//! * A tombstoning `delete` (#9064) deliberately KEEPS the ShapeId and leaves
//!   `TAG_HOLE` in the slot, but it sets `OBJ_FLAG_STABLE_TOMBSTONES` on the
//!   receiver in the same step; the emitted guard rejects that bit, so such a
//!   receiver never takes the inline answer again.
//! * Adding a key is a transition to a different ShapeId, which can only turn
//!   a hit into a miss (`true` stays `true` for the cached key anyway).
//! * A moved receiver reads its own header, so evacuation is invisible here;
//!   a forwarded one is rejected by the guard's `GC_FLAG_FORWARDED` test.
//!
//! ShapeIds are allocated monotonically from a process-global counter and are
//! never reused (`shapes::SHAPE_ID_NEXT`), so a stale stamp can only miss. The
//! id range (`0x8000_0000..0xC000_0000`) is disjoint from every class id, so
//! the guard's compare against a header word that still holds
//! `parent_class_id` — an object that was never shape-stamped — cannot alias
//! an armed id either.
//!
//! The cache holds two integers and never a pointer, so it is not a GC root
//! (contrast the caches enumerated by `scripts/gc_runtime_root_holders.py`).

use super::*;

/// The words the emitted site reads. Word 0 is the only one the inline guard
/// touches; the arena hands out zeroed memory, and `shape == 0` is "unarmed".
#[repr(C)]
pub struct InPresenceCache {
    /// Armed ShapeId, widened to the guard's compare width. 0 until the site
    /// primes.
    shape: u64,
    /// Arming attempts spent at this site. Bounds the work a site that can
    /// never arm (an inherited hit, a proxy receiver) or one that thrashes
    /// between shapes pays on every call.
    attempts: u64,
}

/// Arming attempts a site gets before it stops trying. A monomorphic own-key
/// site spends exactly one; a site whose `true` comes from the prototype chain
/// spends this many and then costs one load and one compare per call forever.
const IN_PRESENCE_ATTEMPT_BUDGET: u64 = 8;

/// `"k" in o` for a site that owns a presence-cache slot.
///
/// Answers exactly what [`js_in_operator`] answers — including its TypeError
/// on a non-object right operand — and then, for a `true` that came from an
/// own string key on an ordinary receiver, records the receiver's ShapeId so
/// the site's inline guard can answer the next one without a call.
///
/// # Safety
/// `slot` is the address of the site's `@perry_ic_N` global (or null): a live,
/// pointer-sized location holding null or a cache from the IC arena.
#[no_mangle]
pub unsafe extern "C" fn js_in_operator_presence_ic(
    obj: f64,
    key: f64,
    slot: *mut *mut InPresenceCache,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    let answer = js_in_operator(obj, key);
    if answer.to_bits() != TAG_TRUE {
        // Absent, or a `false` from a trap: nothing positive to record, and a
        // negative is not cacheable (see the module header).
        return answer;
    }
    // Resolved with word 0 already holding [`IN_PRESENCE_UNARMED`]: a `true`
    // from the prototype chain resolves the slot (the attempt budget lives in
    // the cache) but cannot arm it, and a zero word 0 is not "unarmed" to the
    // emitted guard — see the constant.
    let cache = crate::object::pic_slot_resolve_init(slot, |c| {
        (*c).shape = IN_PRESENCE_UNARMED;
    });
    if cache.is_null() {
        return answer;
    }
    if (*cache).attempts >= IN_PRESENCE_ATTEMPT_BUDGET {
        return answer;
    }
    (*cache).attempts += 1;
    if let Some(shape) = armable_own_key_shape(obj, key) {
        (*cache).shape = u64::from(shape);
    }
    answer
}

/// Word 0 of a presence cache that has been resolved but not armed.
///
/// The emitted guard (`perry-codegen/src/expr/in_presence_ic.rs`) compares the
/// receiver's zero-extended `+4` word against word 0 and has no "is this site
/// armed?" test of its own, on the assumption that no stamped receiver's
/// ShapeId can be 0. That is true, and beside the point: a receiver that was
/// never shape-stamped carries `parent_class_id` at `+4`, which is 0 for an
/// anonymous object literal. So at a site whose cache had been resolved by a
/// prototype-chain `true` (which resolves, spends an attempt, and cannot arm),
/// `"k" in {}` read word 0 == 0 == the receiver's word and answered `true`.
/// The same zero-sentinel flaw was taken off the property-read tower in
/// #10833; `1 << 32` is above every zero-extended `u32`, so it matches nothing.
pub(crate) const IN_PRESENCE_UNARMED: u64 = 1 << 32;

/// The receiver's ShapeId, when a site may answer `true` for `key` from it
/// alone: an ordinary, shape-stamped, descriptor-free, tombstone-free heap
/// object whose keys array holds `key` as an own entry.
///
/// Every rejection here is a receiver whose `in` answer is decided by
/// something the ShapeId does not capture — a proxy trap, a native-module
/// dispatch table, a RegExp expando, an accessor side table, an internal
/// runtime key, or a hole left by a tombstoning delete.
unsafe fn armable_own_key_shape(obj: f64, key: f64) -> Option<u32> {
    let obj_val = JSValue::from_bits(obj.to_bits());
    let key_val = JSValue::from_bits(key.to_bits());
    if !obj_val.is_pointer() || !key_val.is_any_string() {
        return None;
    }
    let addr = (obj_val.bits() & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(addr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_HAS_DESCRIPTORS | crate::gc::OBJ_FLAG_STABLE_TOMBSTONES)
            != 0
    {
        return None;
    }
    // RegExp cells are OBJECT-typed but answer `in` through the exotic expando
    // registry, exactly as `js_object_has_property`'s own fast path documents.
    if super::super::exotic_expando::exotic_expando_kind(addr).is_some() {
        return None;
    }
    let obj_ptr = addr as *const ObjectHeader;
    // Native-module namespaces (console, fs, …) expose VIRTUAL keys that never
    // live in `keys_array`, and their answer is the vtable's, not the shape's.
    if (*obj_ptr).class_id == NATIVE_MODULE_CLASS_ID {
        return None;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let key_bytes = crate::string::js_string_key_bytes(key_val, &mut sso)?;
    // A compiler-private storage key is invisible to [[HasProperty]] even
    // though it sits in `keys_array`.
    if super::is_internal_runtime_key_bytes(key_bytes) {
        return None;
    }
    let shape = super::super::shapes::object_shape_stamp(obj_ptr);
    if shape == 0 {
        return None;
    }
    let keys = crate::object::object_keys_array(obj_ptr);
    match crate::value::addr_class::try_read_gc_header(keys as usize) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {}
        _ => return None,
    }
    let key_count = crate::array::js_array_length(keys);
    super::super::keys_lookup::keys_find_slot_by_bytes(keys, key_count, key_bytes)?;
    Some(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emitted guard compares a zero-extended `i32` header word against
    /// word 0, so an armed id must be unrepresentable as a class id — else an
    /// object that was never shape-stamped could alias one.
    #[test]
    fn armed_ids_cannot_alias_a_class_id() {
        assert!(!super::super::super::shapes::is_shape_id(0));
        assert!(!super::super::super::shapes::is_shape_id(1));
        assert!(!super::super::super::shapes::is_shape_id(0x7FFF_FFFF));
        assert!(!super::super::super::shapes::is_shape_id(0xFFFF_0000));
        assert!(super::super::super::shapes::is_shape_id(0x8000_0000));
    }

    /// Word 0 is what the emitted guard loads; word 1 must not move under it.
    #[test]
    fn cache_layout_matches_the_emitted_guard() {
        assert_eq!(std::mem::size_of::<InPresenceCache>(), 16);
        assert_eq!(std::mem::offset_of!(InPresenceCache, shape), 0);
        assert_eq!(std::mem::offset_of!(InPresenceCache, attempts), 8);
    }

    /// A site that can never arm must stop paying for the attempt.
    #[test]
    fn the_attempt_budget_is_small_and_nonzero() {
        assert!(IN_PRESENCE_ATTEMPT_BUDGET > 0 && IN_PRESENCE_ATTEMPT_BUDGET <= 16);
    }

    /// A `true` the site cannot arm on (here: the key sits under a descriptor)
    /// resolves the cache to count the attempt, and used to leave word 0 at
    /// its zeroed birth value — which the emitted guard then matched against
    /// any receiver whose `+4` word is 0, i.e. every unstamped anonymous
    /// object literal: `"k" in {}` answered `true`. Word 0 must be resolved
    /// holding a value no zero-extended `u32` can equal.
    #[test]
    fn a_resolved_but_unarmed_site_cannot_be_matched_by_an_unstamped_receiver() {
        let _lock = crate::gc::global_side_table_test_lock();
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 4));
        let key = scope.root_string_ptr(crate::string::js_string_from_bytes(b"k".as_ptr(), 1));
        obj.with_mut_ptr(|o| {
            key.with_const_ptr(|k| crate::object::js_object_set_field_by_name(o, k, 1.0))
        });
        // A descriptor makes the receiver unarmable while `in` still says true.
        obj.with_mut_ptr(|o: *mut ObjectHeader| {
            super::super::super::descriptor_state::set_property_attrs(
                o as usize,
                "k".to_string(),
                crate::object::PropertyAttrs::new(false, true, false),
            );
        });
        let mut slot: *mut InPresenceCache = std::ptr::null_mut();
        let answer = obj.with_mut_ptr(|o: *mut ObjectHeader| {
            key.with_const_ptr(|k: *const crate::StringHeader| unsafe {
                js_in_operator_presence_ic(
                    f64::from_bits(crate::value::js_nanbox_pointer(o as i64).to_bits()),
                    f64::from_bits(crate::value::js_nanbox_string(k as i64).to_bits()),
                    &mut slot,
                )
            })
        });
        assert_eq!(answer.to_bits(), 0x7FFC_0000_0000_0004, "test premise: `in` is true");
        assert!(!slot.is_null(), "test premise: the attempt resolved the cache");
        let word0 = unsafe { (*slot).shape };
        assert_eq!(word0, IN_PRESENCE_UNARMED);
        assert!(
            word0 > u64::from(u32::MAX),
            "an unarmed word must be unmatchable by any zero-extended +4 word, got {word0:#x}"
        );
        assert_ne!(word0, 0, "0 is what an unstamped `{{}}` carries at +4");
    }
}

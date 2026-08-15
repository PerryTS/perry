//! #8113: the live inline-slot bound, and the `ObjectHeader` ABI revision.
//!
//! `ObjectHeader` used to carry a `field_count: u32` word. It was derivable
//! from the object's immutable ShapeId descriptor, and removing it together
//! with the equally derivable `object_type` word took the header from 32 bytes
//! to 24 (a two-slot object from 56 to 48). These four items are what took its
//! place; they live in their own module because `object/mod.rs` is at the
//! repository's 2000-line cap.

use super::shapes;
use super::ObjectHeader;
use super::INLINE_SLOT_FLOOR;

/// Revision of the [`ObjectHeader`] ABI, paired with
/// `perry_ffi::OBJECT_HEADER_ABI_REVISION`.
///
/// `perry-ffi` is published to crates.io, and a wrapper compiled against an old
/// mirror linked against a new runtime reads the wrong header offsets with no
/// compile error. Bump this and the perry-ffi constant together on ANY change
/// to the header's size, field set, or field offsets; perry-ffi's
/// `object_header_abi_revision_matches_the_pinned_layout` (now actually run in
/// CI, see `test.yml`) fails otherwise.
///
/// * 1 — `{object_type, class_id, parent_class_id, field_count, keys_array, meta}`.
/// * 2 — `{class_id, parent_class_id, keys_array, meta}` (#8113).
#[no_mangle]
pub extern "C" fn perry_object_header_abi_revision() -> u32 {
    2
}

/// Direct-mapped `ShapeId -> live_inline_slot_count` memo.
///
/// The bound used to be a single `u32` load off the header. It is now a
/// shape-table probe — a TLS resolution, a `RefCell` borrow, a SipHash and a
/// bucket walk — on a path that includes every by-index field write. This memo
/// puts a plain array index in front of that.
///
/// # Why it needs no invalidation
///
/// Two facts, both load-bearing:
///
/// * **ShapeIds are never reused.** `shapes::SHAPE_ID_NEXT` is a monotonic
///   process-global counter and exhaustion fail-STOPS (`shape_id_exhausted_abort`),
///   so an id names one fact set for the life of the process.
/// * **`live_inline_slot_count` is part of the exact facts an id is minted
///   for.** `shape_descriptor_ensure_with_generation` dedupes on those facts, so
///   two different bounds get two different ids. The only field ever mutated in
///   place on a published descriptor is `keys` (rewritten by the evacuator), and
///   this memo does not hold it.
///
/// `prune_dead_shape_keys` can REMOVE an id, which would leave a stale entry —
/// but its documented contract is that "a descriptor removed here cannot be
/// named by a live object", so a stale entry is only reachable through a dead
/// receiver. (Keyless descriptors, whose `keys` is 0, are never pruned: the
/// dead-owner predicate classifies address 0 as not-in-any-heap-space.)
///
/// The memo is per-thread because descriptor tables are per-agent: a
/// process-global id can name different local facts in two agents
/// (`install_external_shape_id`).
const LIVE_SLOT_MEMO_WAYS: usize = 64;

thread_local! {
    static LIVE_SLOT_MEMO: [std::cell::Cell<(u32, u32)>; LIVE_SLOT_MEMO_WAYS] =
        [const { std::cell::Cell::new((0, 0)) }; LIVE_SLOT_MEMO_WAYS];
}

/// The authoritative live inline-slot bound (#8113: the replacement for the
/// deleted `ObjectHeader::field_count` word).
///
/// Zero for a receiver with no published descriptor. That is deliberately
/// fail-CLOSED: a bound of 0 rejects field writes instead of admitting an
/// unbounded one, and every runtime allocator publishes a descriptor before its
/// header escapes, so the zero case is a raw/synthetic fixture, not a live
/// object.
#[inline]
pub unsafe fn object_live_slot_count(obj: *const ObjectHeader) -> u32 {
    let shape_id = shapes::object_shape_stamp(obj);
    if shape_id == 0 {
        return 0;
    }
    let way = (shape_id as usize) & (LIVE_SLOT_MEMO_WAYS - 1);
    LIVE_SLOT_MEMO.with(|memo| {
        let entry = &memo[way];
        let (cached_id, cached_count) = entry.get();
        if cached_id == shape_id {
            return cached_count;
        }
        let count = shapes::shape_descriptor_by_id(shape_id)
            .map(|descriptor| descriptor.live_inline_slot_count)
            .unwrap_or(0);
        // A missing descriptor is NOT cached: it is the fail-closed answer for
        // a stale/foreign id, and caching it would make a later legitimate
        // install of that id invisible.
        if count != 0 {
            entry.set((shape_id, count));
        }
        count
    })
}

/// Test hook: drop every memo entry. The memo needs no invalidation in
/// production (see [`LIVE_SLOT_MEMO`]), but a test that plants a synthetic id,
/// drops its descriptor and re-mints under the same id must be able to say so.
#[cfg(test)]
pub(crate) fn test_clear_live_slot_memo() {
    LIVE_SLOT_MEMO.with(|memo| {
        for entry in memo.iter() {
            entry.set((0, 0));
        }
    });
}

/// C-ABI accessor for [`object_live_slot_count`], for out-of-runtime consumers
/// (`perry-ext-*`) that mirror `ObjectHeader` through `perry-ffi` and used to
/// read the deleted `field_count` word directly (#8113).
///
/// # Safety
/// `obj` must be a live `GC_TYPE_OBJECT` allocation or null.
#[no_mangle]
pub unsafe extern "C" fn js_object_live_slot_count(obj: *const ObjectHeader) -> u32 {
    if obj.is_null() {
        return 0;
    }
    object_live_slot_count(obj)
}

/// The OOB bound every by-index field write is checked against:
/// `max(live_inline_slot_count, INLINE_SLOT_FLOOR)`. Every allocator reserves
/// at least `INLINE_SLOT_FLOOR` physical slots (`object/alloc.rs`), and
/// `live_inline_slot_count` is a fixed point of the same expression — the
/// by-name append path only ever bumps it for a slot it placed inline — so this
/// can never exceed the physical slot count.
#[inline]
pub unsafe fn object_inline_alloc_limit(obj: *const ObjectHeader) -> u32 {
    std::cmp::max(object_live_slot_count(obj), INLINE_SLOT_FLOOR as u32)
}

/// Publish a new authoritative live-inline-slot bound.
///
/// #8113 MINT-THEN-STAMP. There is no longer a header word to fall back on, so
/// this must never leave the receiver without a descriptor, not even
/// transiently: `shape_descriptor_ensure_*` inserts into a `HashMap` and can
/// therefore collect, and a collection landing in a stamp-cleared window would
/// see a live bound of 0 and stop tracing the object's payload entirely.
///
/// The successor descriptor is minted while the PREDECESSOR is still stamped
/// (so a collection during the mint sees the old, still-correct bound — the
/// newly exposed slot has not been written yet), and publication is the single
/// `parent_class_id` store, which cannot collect.
///
/// Callers growing the traced range must invoke this before publishing the
/// pointer-bearing field value (#7154): mint → stamp → value-slot store.
#[inline]
pub(crate) unsafe fn set_object_live_slot_count(obj: *mut ObjectHeader, field_count: u32) {
    shapes::publish_object_live_slot_count(obj, field_count);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #8113: the memo must be keyed by ShapeId, and an entry must be REPLACED
    /// when a different id maps to the same way.
    ///
    /// Sabotage-shaped, and the premise is the load-bearing part: two arbitrary
    /// shapes get consecutive ids and therefore different ways, so alternating
    /// between them proves nothing. This mints enough shapes to FIND a pair
    /// that collides, asserts it found one, and only then alternates. Replacing
    /// the `cached_id == shape_id` test with `cached_id != 0` turns it red.
    #[test]
    fn the_live_slot_memo_is_keyed_by_shape_id_not_by_way() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            // Distinct widths so a mixed-up answer is observable, and enough
            // shapes that two of them must share one of the 64 ways.
            let mut minted: Vec<(*mut ObjectHeader, u32, u32)> = Vec::new();
            for width in 1u32..=(LIVE_SLOT_MEMO_WAYS as u32 + 8) {
                let mut packed = Vec::new();
                for i in 0..width {
                    packed.extend_from_slice(format!("m8113w{width}_{i}").as_bytes());
                    packed.push(0);
                }
                let obj = crate::object::js_object_alloc_with_shape(
                    0x8113_2000 + width,
                    width,
                    packed.as_ptr(),
                    packed.len() as u32,
                );
                let id = (*obj).parent_class_id;
                assert!(shapes::is_shape_id(id));
                minted.push((obj, id, width));
            }

            let mut collision: Option<((*mut ObjectHeader, u32), (*mut ObjectHeader, u32))> = None;
            'outer: for i in 0..minted.len() {
                for j in (i + 1)..minted.len() {
                    let (a, ida, wa) = minted[i];
                    let (b, idb, wb) = minted[j];
                    if ida != idb
                        && wa != wb
                        && (ida as usize) & (LIVE_SLOT_MEMO_WAYS - 1)
                            == (idb as usize) & (LIVE_SLOT_MEMO_WAYS - 1)
                    {
                        collision = Some(((a, wa), (b, wb)));
                        break 'outer;
                    }
                }
            }
            let ((a, wa), (b, wb)) = collision.expect(
                "test premise: two distinct shapes with different widths must share a memo way",
            );

            // Alternate. A memo that returns whatever is in the way, without
            // checking the id, hands one object the other's bound.
            for _ in 0..4 {
                assert_eq!(object_live_slot_count(a), wa);
                assert_eq!(object_live_slot_count(b), wb);
            }
        }
    }

    /// The bound must FOLLOW a re-stamp: growing an object past its birth width
    /// mints a successor ShapeId, and the memo is keyed by that id, so the new
    /// bound must be visible immediately.
    #[test]
    fn the_live_slot_memo_follows_a_reshape() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 1);
            let before_id = (*obj).parent_class_id;
            assert_eq!(object_live_slot_count(obj), 1);

            let key = crate::string::js_string_from_bytes(b"m8113_grow".as_ptr(), 10);
            crate::object::js_object_set_field_by_name(obj, key, 7.0);
            let after_id = (*obj).parent_class_id;
            assert_ne!(
                before_id, after_id,
                "test premise: the append re-stamps the receiver"
            );
            assert_eq!(
                object_live_slot_count(obj),
                shapes::shape_descriptor_by_id(after_id)
                    .expect("successor descriptor")
                    .live_inline_slot_count,
                "the memo must follow the successor ShapeId, not hold the birth bound"
            );
        }
    }
}

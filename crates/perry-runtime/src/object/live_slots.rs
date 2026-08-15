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

/// The authoritative live inline-slot bound (#8113: the replacement for the
/// deleted `ObjectHeader::field_count` word).
///
/// Zero for a receiver with no published descriptor. That is deliberately
/// fail-CLOSED: a bound of 0 rejects field writes instead of admitting an
/// unbounded one, and every runtime allocator publishes a descriptor before its
/// header escapes, so the zero case is a raw/synthetic fixture, not a live
/// object.
///
/// # A ShapeId -> count memo in front of this measured NULL (#8113)
///
/// The bound used to be one `u32` load off the header and is now a shape-table
/// probe, so a 64-way direct-mapped `ShapeId -> count` cache looked like the
/// obvious recovery. It was built, sabotage-tested, and measured on the
/// 19-program corpus against the same baseline: `retain` +4.26% vs +3.26%
/// WITHOUT it, `retain_wide` +4.46% vs +2.89%, `retain_wide1` +4.18% vs +2.61%,
/// `deeplist` +8.69% vs +8.20% — worse on four of the five rows that pay the
/// bound at all, better only on `shapes`. The memo pays its own TLS resolution
/// and a closure, which is most of what `state()` + a small `HashMap<u32, _>`
/// probe costs. It was deleted rather than left in as an unmeasured
/// configuration.
#[inline]
pub unsafe fn object_live_slot_count(obj: *const ObjectHeader) -> u32 {
    shapes::object_shape_descriptor(obj)
        .map(|descriptor| descriptor.live_inline_slot_count)
        .unwrap_or(0)
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

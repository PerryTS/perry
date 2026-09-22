//! `ObjectMeta::flags` — the per-object bit word, and what each bit means.
//!
//! Split out of `object/mod.rs` for the 2000-line cap. The constants and the
//! bit map belong together: the map is what a reader needs in order to add a
//! bit, and the "last free bit" warning below is only useful next to the
//! constants it constrains.

pub(crate) const OBJECT_META_FLAG_PROTO_DIVERGED: u64 = 1;
pub(crate) const OBJECT_META_FLAG_USER_PROTO_OVERRIDE: u64 = 1 << 3;
pub(crate) const OBJECT_META_FLAG_CLASS_EVALUATION_PROTO: u64 = 1 << 4;
/// This object is used as somebody's `[[Prototype]]`, so a STRUCTURAL mutation
/// of it (key add, delete, descriptor install, attribute change,
/// `setPrototypeOf`) is invisible to everything that inherits from it and must
/// bump `object::proto_validity`.
///
/// Set by the `[[Prototype]]` install funnel
/// (`prototype_chain::object_set_static_prototype_impl`, all four link kinds)
/// and by `class_prototype_object_root_store`. The inherited-read cache
/// REFUSES to record a hop that is not already marked, so an install site this
/// list misses costs a cache hit and can never produce a stale value — the
/// fail-safe polarity, chosen deliberately after the header-bit version of
/// this flag turned out to be erasable by unrelated GC layout transitions.
pub(crate) const OBJECT_META_FLAG_IS_PROTOTYPE: u64 = 1 << 5;
/// This object answers property reads by something OTHER than its shape — it
/// is `process.env` or an `arguments` object — so a shape-keyed cache must
/// refuse it however well its ShapeId matches.
///
/// A per-object summary of two address-keyed registries, set inside each
/// registry's single writer, in the same breath as the insert. It replaces two
/// probes that cost 14.0 and 5.0 instructions on every cached read and that an
/// EMITTED read sequence could not have called at all.
pub(crate) const OBJECT_META_FLAG_EXOTIC_READ_RECEIVER: u64 = 1 << 6;
//
// `ObjectMeta::flags` bit map (u64), verified against #8690's comment in
// `array/subclass.rs` and every reader in the tree:
//   bit 0      prototype-semantic divergence      (OBJECT_META_FLAG_PROTO_DIVERGED)
//   bit 1      packed-numeric payload valid       (#8690)
//   bit 2      packed-numeric u32 entity proof    (#8690)
//   bit 3      user-origin prototype signal       (OBJECT_META_FLAG_USER_PROTO_OVERRIDE)
//   bit 4      class-evaluation prototype         (post-dates #8690's comment)
//   bit 5      this object is a prototype         (here)
//   bit 6      exotic read receiver               (here)
//   bit 7      *** THE LAST FREE BIT IN THIS WORD ***
//   bits 8..31 verified prefix bound              (#8690)
//   bits 32..63 exact semantic ShapeId            (#8690)
//
// `PACKED_NUMERIC_META_MASK` clears only bits 1, 2 and 8..63, so bits 0 and
// 3..7 survive a packed-proof retirement. The two `flags = 0` stores in
// `meta_accessors.rs` are fresh-record initialisation — both return an
// existing record before reaching them — so a mark is never reset under a
// live object. One consumer treats the word as a whole:
// `typed_feedback::guards` declines on `flags != 0`, so marking an object
// takes it off that fast path. That is a decline, not a wrong answer, and it
// applies only to prototypes and to the two exotic receivers.
//
// BIT 7 IS THE LAST FREE BIT IN THIS WORD. `GcHeader::_reserved` has none at
// all (see `gc::OBJ_FLAG_RESERVED_BIT_MAP_SEE_DOC`), so after bit 7 a new
// per-object fact needs a new word, not a new flag — and #10826 returning
// `OBJ_FLAG_STABLE_TOMBSTONES` is the only bit expected back from either.
//
// Note also that bit 4 is absent from #8690's own bit map in
// `array/subclass.rs`: it was added later and that comment was not updated.
// The same documentation failure one layer down from the one that put a mark
// on `_reserved` bit 13. If you add a bit here, update BOTH maps.

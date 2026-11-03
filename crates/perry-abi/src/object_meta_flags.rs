//! `ObjectMeta::flags` — the per-object bit word, and what each bit means.
//!
//! Split out of `object/mod.rs` for the 2000-line cap. The constants and the
//! bit map belong together: the map is what a reader needs in order to add a
//! bit, and the "last free bit" warning below is only useful next to the
//! constants it constrains.

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
pub const OBJECT_META_FLAG_IS_PROTOTYPE: u64 = 1 << 5;
/// This object answers property reads by something OTHER than its shape — it
/// is `process.env` or an `arguments` object — so a shape-keyed cache must
/// refuse it however well its ShapeId matches.
///
/// A per-object summary set by the single writer of each of those two facts,
/// in the same breath as it records them: the `process.env` registry insert,
/// and the store of `ObjectMeta::arguments` (#11506; it was an address-keyed
/// registry too until then). It replaces two probes that cost 14.0 and 5.0
/// instructions on every cached read and that an EMITTED read sequence could
/// not have called at all.
pub const OBJECT_META_FLAG_EXOTIC_READ_RECEIVER: u64 = 1 << 6;
/// Ordinary storage and prototype links, with native forwarding only after
/// an ordinary read yields undefined. Projected into the shape kind.
pub const OBJECT_META_FLAG_NATIVE_ALIAS: u64 = 1;
/// #11915: this object is a SEALED intrinsic of the realm
/// (`builtin_proto::seal_realm`, `seal_runtime_prototype`): a prototype, a
/// builtin namespace's member holder, or the realm global; its family is in
/// bits 8..13 ([`OBJECT_META_SEALED_FAMILY_SHIFT`]). Every mutation funnel
/// reads it (`builtin_proto::note_write`): a builtin family withdraws its
/// `Pristine` byte, a runtime prototype (`builtin_proto::RUNTIME_FAMILY`)
/// sets its own shadow bit.
///
/// Bits 8..13 are the packed-numeric payload's on an Array-subclass
/// instance, which a sealed prototype never is. The encoding is fail-safe:
/// a cleared family field reads as family 0, `Object`, which withdraws every
/// family.
pub const OBJECT_META_FLAG_SEALED_INTRINSIC: u64 = 1 << 3;
/// Shift of the sealed intrinsic's family (6 bits): a `BuiltinProto`
/// (0..16), a builtin namespace (16..31, `builtin_namespace`), the realm
/// global (31), or a runtime prototype (32, `builtin_proto::RUNTIME_FAMILY`).
pub const OBJECT_META_SEALED_FAMILY_SHIFT: u32 = 8;
/// Mask of the sealed intrinsic's family after the shift.
pub const OBJECT_META_SEALED_FAMILY_MASK: u64 = 0x3F;
//
// `ObjectMeta::flags` bit map (u64), verified against #8690's comment in
// `array/subclass.rs` and every reader in the tree:
//   bit 0      native fallback after ordinary undefined
//   bit 1      packed-numeric payload valid       (#8690)
//   bit 2      packed-numeric u32 entity proof    (#8690)
//   bit 3      sealed intrinsic prototype         (here, #11915)
//   bit 4      free (was the class-evaluation bit)  how a prototype was linked
//                                                   is not a fact: its shape is
//   bit 5      this object is a prototype         (here)
//   bit 6      exotic read receiver               (here)
//   bit 7      *** THE LAST FREE BIT IN THIS WORD ***
//   bits 8..31 verified prefix bound              (#8690)
//              — on a sealed intrinsic (bit 3), bits 8..13 are its
//              family instead (#11915; never both)
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

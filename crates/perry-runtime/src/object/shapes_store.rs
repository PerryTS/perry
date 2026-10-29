//! Storage for the agent-local shape descriptor table (#9706).
//!
//! Two structures, both owned by [`super::ShapeTable`]:
//!
//! * [`ShapeSlab`] — the by-id store. A ShapeId is a process-global monotonic
//!   counter (`SHAPE_ID_BASE + n`), so `n` indexes a chunked slab directly: no
//!   hash, no per-record heap allocation, and a record address that never
//!   moves for the record's lifetime — the property the collector relies on
//!   when it enumerates a descriptor's `keys` word as a rewritable slot
//!   (#8112) and retains that address across budgeted resumptions. Chunks
//!   (32 records) hang off a two-level page directory, are allocated lazily
//!   (a worker's ids interleave with the main thread's), and an all-dead chunk
//!   is released by [`ShapeSlab::release_empty_chunks`] at the same cadence as
//!   the reverse-index shrink (once per major collection).
//!
//! * [`IdList`] — the value of the per-keys-address family index
//!   (`ShapeTableInner::families`). One entry per keys array names every
//!   descriptor id currently indexed under that address. Exact-facts interning
//!   walks the family and compares the remaining facts against the slab
//!   record, which is what lets the table drop the second, facts-keyed reverse
//!   map it used to carry: a family is small by construction — a SHARED keys
//!   array is immutable, so its descriptors differ only in the birth bound or
//!   a semantic generation, and an OWNED array retires its growth history
//!   eagerly (`retire_owned_shape_siblings`).
//!
//! Measured on the compiled claude-code TUI at idle (`PERRY_GC_CENSUS`), the
//! previous layout — a `PtrHashMap<u32, Box<ShapeDescriptor>>` beside two
//! `Vec<u32>`-valued reverse maps — cost ~330 bytes per live descriptor:
//! a 56-byte record in a 64-byte allocator bin, a 16-byte map entry at 25%
//! load after `shrink_to(2 * len)`, a 57-byte facts-map bucket, and a 33-byte
//! keys-map bucket, plus a 16-byte `Vec` buffer per reverse entry. A packed
//! 32-byte slab record with one 24-byte family bucket per keys array is the
//! same information at a fraction of the bytes.

use super::{
    ShapeDescriptor, ShapeObjectKind, EXOTIC_SHAPE_ID_BASE, SHAPE_ID_BASE, SITE_SHAPE_ID_END,
};
use crate::codegen_abi::{LayoutRow, LayoutWord, PublishedShape, ShapeId};
use std::cell::UnsafeCell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PublishError {
    Capacity,
    Commit,
    InvalidWord,
    IdentityConflict,
    RowConflict,
}

#[path = "shapes_store/extras.rs"]
mod extras;
pub(super) use extras::ShapeExtras;

/// Static body identity of a ConstFn slot. This is image metadata, never a
/// closure pointer or a GC edge. Entries are sorted by slot in a shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConstFnSlotInfo {
    pub slot: u8,
    pub info: u64,
}

const _: () = assert!(std::mem::align_of::<ShapeExtras>() >= 2);
// Birth/store facts and weak integrity edges share the same record extension.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<ShapeExtras>() == 72);

/// A brand list is strictly ascending: sorted with no duplicate, so equal
/// sets are equal slices.
pub(crate) fn brands_are_sorted(brands: &[u64]) -> bool {
    brands.windows(2).all(|pair| pair[0] < pair[1])
}

/// The exact mask of a sorted body list, or `None` for a duplicate, absent
/// body, or slot outside the 32 inline representation lanes.
pub(crate) fn constfn_mask(infos: &[ConstFnSlotInfo]) -> Option<u32> {
    let mut mask = 0u32;
    let mut previous = None;
    for entry in infos {
        if entry.slot >= crate::object::field_rep::REP_SLOTS as u8
            || entry.info == 0
            || previous.is_some_and(|slot| entry.slot <= slot)
        {
            return None;
        }
        mask |= 1 << entry.slot;
        previous = Some(entry.slot);
    }
    Some(mask)
}

pub(super) const RECORD_FLAG_PRESENT: u8 = 1 << 0;
pub(super) const RECORD_FLAG_FACTS_INDEXED: u8 = 1 << 1;
pub(super) const RECORD_FLAG_OLD_CARRIER: u8 = 1 << 2;
pub(super) const RECORD_FLAG_OLD_CARRIER_SEEN: u8 = 1 << 3;
pub(super) const RECORD_FLAG_CACHE_CARRIER: u8 = 1 << 4;
// Bit 5 was `RECORD_FLAG_KIND_CLASS` until the object kind became a 2-bit
// field in `flags_and_kind` (#10868), a flag byte having no room for a third
// value.
/// #10905: a keyless birth shape an allocation consulted during the current
/// full-collection epoch (`shapes_birth_width`). Keeps the record, and so the
/// width it learned, through the next synchronous full prune; the epoch
/// rotation clears it.
pub(super) const RECORD_FLAG_BIRTH_OWNER: u8 = 1 << 5;
pub(super) const RECORD_FLAG_CARRIED_SEEN: u8 = 1 << 6;
pub(super) const RECORD_FLAG_EXTERNAL_CARRIER: u8 = 1 << 7;

#[path = "shapes_store/flat.rs"]
mod flat;
#[path = "shapes_store/kind.rs"]
mod kind;

/// The table-owned record of one ShapeId. `keys` is first and 8-aligned: it
/// is the word the collector marks through and rewrites in place.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ShapeRecord {
    /// Raw ArrayHeader address in Perry's fixed-width heap-word ABI (0 for a
    /// keyless shape).
    pub(super) keys: u64,
    pub(super) semantic_generation: u64,
    /// The receiver's [[Prototype]] identity (`shapes::object_proto_id`). An
    /// identity fact like every other field here: two objects share a ShapeId
    /// only if they share their prototype, so anything a site learns about a
    /// ShapeId's inherited behaviour is keyed by the shape itself.
    pub(super) proto_id: u64,
    pub(super) logical_key_count: u32,
    pub(super) live_inline_slot_count: u32,
    pub(super) hole_count: u32,
    /// Low 8 bits: the `RECORD_FLAG_*` set. Bits 8-11: `ShapeObjectKind`.
    /// Bits 12-14 and 30: births a keyless birth shape served while tracking its
    /// width (#10905). Bit 15: a weak-collection brand; bit 31 distinguishes WeakSet.
    /// Bit 22: immutable key prefix proves symbol absence (derived, not identity).
    /// Bit 23: immutable integrity facts are present in the cold extension.
    /// attribute SUMMARY bits 16-21 (`key_attrs::SUMMARY_*`), an identity fact.
    /// Bits 24-29: the inline width a keyless birth shape's descendants grow
    /// to (#10905), stored minus one. The two #10905 fields are learned facts of the record,
    /// never identity.
    ///
    /// This word replaces the old `flags: u8` plus `_pad: [u8; 3]`. It is the
    /// same four bytes in the same place, so the record stays 32 bytes and
    /// 8-aligned (asserted below) and the slab geometry is unchanged — the
    /// kind field is free, it lives in padding that was already paid for.
    flags_and_kind: u32,
    /// POSBOUND: how many leading key positions ARE inline slots of every
    /// receiver carrying this shape — `min(logical_key_count,
    /// live_inline_slot_count)` when the shape answers by position
    /// ([`Self::positional_by_facts`]), 0 otherwise. A function of the
    /// record's facts, rewritten by [`Self::refresh_positional`] wherever an
    /// input changes, read by [`Self::position_bound`].
    ///
    /// ONE field so the megamorphic read confirm (`js_object_read_confirm`)
    /// answers "is the guess a position of this shape" with one compare —
    /// `guess < position_bound` — instead of a flag test and a `min`.
    /// Offsets: bound 40, special mask 44, rep 48, extras 56, prototype cell 64.
    position_bound: u32,
    /// For a `REP_SPECIAL` lane, one means ConstFn and zero reserves the
    /// NoPointer interpretation for P5. This uses the old padding at 44;
    /// with no special lanes it is zero and old shape identity is unchanged.
    special_constfn_mask: u32,
    /// Charter step 5: the per-slot field representation, two bits per inline
    /// slot 0..32 (`field_rep`). An identity fact under
    /// [`field_rep::identity`](crate::object::field_rep::identity), folded into the
    /// facts key only when nonzero.
    pub(super) rep: u64,
    /// Zero, a low-bit-tagged scalar rollback parent, or an aligned, owned
    /// [`ShapeExtras`] address. Ordinary reverse edges need no allocation.
    /// Fixed width keeps the slab layout identical on ILP32/LP64.
    extras: u64,
    /// Borrowed stable identity-word address; zero for DEFAULT/null/per-object.
    /// GC rewrites the word, not this derived address; no new identity fact.
    pub(super) proto_cell: u64,
    /// Low byte: element properties described by this immutable key
    /// prefix: 0 plain, 1 indexed/length descriptors, 2 a per-object mutable
    /// key list. Bits 8-31: the canonical trie node of `keys` plus one, 0 for
    /// none (an owned list, no keys, or a holder that never learned it). The
    /// node is the list's identity in this agent (CKI S1), carried here so
    /// a receiver's keys reach the trie with no address lookup; it is a fact
    /// of the key list, never of shape identity (`facts_match` compares the
    /// `keys` word). Offset 72; codegen never reads it.
    keys_node_and_element_kind: u32,
    /// The class family layout (its ShapeId, `layout_desc`) this record is a
    /// class evaluation's copy of: the family's keys, slots, holes, kind,
    /// attributes and lanes at the identity `MIXED | class | serial` naming
    /// that evaluation's prototype (`shapes::shape_is_evaluation_layout_of`),
    /// or 0. Written once, by the prototype transition that reached the
    /// record from the family layout, after checking those facts; a function
    /// of the two records' immutable facts, never identity. Offset 76, in
    /// what was the high half of `element_kind`.
    pub(super) family_desc_index_plus_one: u32,
    /// The key index of this record's key list for the unknown-receiver
    /// probe (`object/layout_probe.rs`): 0 until the first probe derives it
    /// from the record's immutable key list, then `PROBE_*` bits plus the
    /// address of an owned position table (0 for a list short enough to
    /// scan). A function of the list alone; never invalidated, released with
    /// the record. Offset 80.
    pub(crate) probe: u64,
    /// The coloured keys this record's receivers hold as inline data at
    /// their colour slot (OBJECT_MODEL §3.7 case 2c; bit = the key's colour
    /// bit, `perry-codegen` `layout_families`). Derived with `probe`; a read
    /// site of a coloured key tests its bit and loads the slot. Offset 88.
    pub(crate) colour_mask: [u64; 2],
    /// The coloured keys a store may write at their colour slot: the read
    /// bit, plus a writable `Any`- or `F64`-lane key of a store-admitted
    /// layout with no integrity level (never a `Special` lane). A store into
    /// an `F64` lane must be a plain double: the site tests the lane (`rep`,
    /// offset 48) unless it is. Offset 104.
    pub(crate) colour_store_mask: [u64; 2],
    pub(crate) shape_id: ShapeId,
    pub(crate) layout_word: LayoutWord,
}

// Derived once from the immutable key prefix on slab publication. Owned
// mutable lists carry no absence proof. This is not shape identity.
const RECORD_KEYS_NO_SYMBOLS: u32 = 1 << 22;
/// Immutable integrity exists only in the record-owned cold extension.
const RECORD_HAS_INTEGRITY: u32 = 1 << 23;
const RECORD_WEAK_COLLECTION: u32 = 1 << 15;
const RECORD_WEAK_SET: u32 = 1 << 31;
const RECORD_KIND_SHIFT: u32 = 8;
const RECORD_KIND_MASK: u32 = 0xF << RECORD_KIND_SHIFT;
/// The frequently decoded kind is one contiguous field.
/// `kind_codes_round_trip` pins the encoding independently of record width.
const RECORD_KIND_MAX_CODE: u32 = 12;
const _: () = assert!(RECORD_KIND_MAX_CODE <= 15);
/// Charter step 3: the summary of the attributes the shape's keys carry —
/// what the chain store check and every per-key reader ask FIRST, so a shape
/// whose keys are all default answers without touching its keys. Derived
/// from `(keys, logical_key_count)` for a shape that publishes keys, which is
/// why folding it into identity costs no precision; a dictionary receiver's
/// shape publishes no keys and carries its private list's conservative
/// summary here instead.
const RECORD_SUMMARY_SHIFT: u32 = 16;
const RECORD_SUMMARY_MASK: u32 = 0x3F << RECORD_SUMMARY_SHIFT;
const _: () = assert!(crate::object::key_attrs::SUMMARY_KEY_BITS == 0x3F);
const _: () = assert!(RECORD_KIND_MASK & RECORD_SUMMARY_MASK == 0);
const _: () = assert!(RECORD_KIND_MASK & 0xFF == 0);

/// #10905: births served while tracking, low bits 12-14 and high bit 30.
/// This count is decoded while tracking a keyless birth shape; move its
/// high bit rather than making every shape-kind decode merge two fields.
const RECORD_BIRTHS_SHIFT: u32 = 12;
const RECORD_BIRTHS_HIGH: u32 = 1 << 30;
const RECORD_BIRTHS_MASK: u32 = (7 << RECORD_BIRTHS_SHIFT) | RECORD_BIRTHS_HIGH;
const RECORD_BIRTHS_MAX: u32 = 15;
/// #10905 (`shapes_birth_width`): the learned descendant width, bits 24-29
/// (maximum 64), stored minus one: a learned width is always above the
/// inline floor, so 0 still reads as "nothing learned".
const RECORD_WIDTH_SHIFT: u32 = 24;
const RECORD_WIDTH_MASK: u32 = 0x3F << RECORD_WIDTH_SHIFT;
const _: () = assert!(
    super::shapes_birth_width::LEARNED_WIDTH_MAX <= (RECORD_WIDTH_MASK >> RECORD_WIDTH_SHIFT) + 1
);
const _: () = assert!(crate::object::INLINE_SLOT_FLOOR >= 1);
// The fields of `flags_and_kind` are pairwise disjoint.
const _: () = {
    let fields = [
        0xFF,
        RECORD_KIND_MASK,
        RECORD_BIRTHS_MASK,
        RECORD_WEAK_COLLECTION,
        RECORD_WEAK_SET,
        RECORD_KEYS_NO_SYMBOLS,
        RECORD_HAS_INTEGRITY,
        RECORD_SUMMARY_MASK,
        RECORD_WIDTH_MASK,
    ];
    let mut i = 0;
    while i < fields.len() {
        let mut j = i + 1;
        while j < fields.len() {
            assert!(fields[i] & fields[j] == 0);
            j += 1;
        }
        i += 1;
    }
};
const _: () = assert!(super::shapes_birth_width::TRACKING_BIRTHS <= RECORD_BIRTHS_MAX);

const _: () = assert!(std::mem::size_of::<ShapeRecord>() == crate::codegen_abi::SHAPE_RECORD_SIZE);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, probe) == 80);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, family_desc_index_plus_one) == 76);
const _: () = assert!(
    std::mem::offset_of!(ShapeRecord, colour_mask) == crate::codegen_abi::SHAPE_RECORD_COLOUR_MASK
);
const _: () = assert!(
    std::mem::offset_of!(ShapeRecord, colour_store_mask)
        == crate::codegen_abi::SHAPE_RECORD_COLOUR_STORE_MASK
);
const _: () =
    assert!(std::mem::offset_of!(ShapeRecord, rep) == crate::codegen_abi::SHAPE_RECORD_REP);
const _: () = assert!(crate::object::field_rep::REP_SLOTS == crate::codegen_abi::SHAPE_REP_SLOTS);
const _: () = assert!(std::mem::align_of::<ShapeRecord>() == 8);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, position_bound) == 40);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, special_constfn_mask) == 44);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, rep) == 48);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, extras) == 56);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, proto_cell) == 64);

impl ShapeRecord {
    const EMPTY: ShapeRecord = ShapeRecord {
        keys: 0,
        semantic_generation: 0,
        proto_id: 0,
        logical_key_count: 0,
        live_inline_slot_count: 0,
        hole_count: 0,
        flags_and_kind: 0,
        position_bound: 0,
        special_constfn_mask: 0,
        rep: 0,
        extras: 0,
        proto_cell: 0,
        keys_node_and_element_kind: 0,
        family_desc_index_plus_one: 0,
        probe: 0,
        colour_mask: [0; 2],
        colour_store_mask: [0; 2],
        shape_id: ShapeId(0),
        layout_word: LayoutWord(0),
    };

    #[inline]
    pub(super) fn present(&self) -> bool {
        self.flags() & RECORD_FLAG_PRESENT != 0
    }

    /// The `RECORD_FLAG_*` byte. Storage state only, never identity.
    #[inline]
    pub(super) fn flags(&self) -> u8 {
        (self.flags_and_kind & 0xFF) as u8
    }

    #[inline]
    pub(super) fn has(&self, flag: u8) -> bool {
        self.flags() & flag != 0
    }

    #[inline]
    pub(super) fn set(&mut self, flag: u8, on: bool) {
        if on {
            self.flags_and_kind |= u32::from(flag);
        } else {
            self.flags_and_kind &= !u32::from(flag);
        }
    }

    /// A runtime table or process-lifetime generated-code global may reinstall
    /// this id even while no object currently carries it.
    #[inline]
    pub(super) fn cache_carrier(&self) -> bool {
        self.has(RECORD_FLAG_CACHE_CARRIER | RECORD_FLAG_EXTERNAL_CARRIER)
    }

    /// The attribute summary byte (see [`RECORD_SUMMARY_SHIFT`]).
    #[inline]
    pub(super) fn summary(&self) -> u8 {
        ((self.flags_and_kind & RECORD_SUMMARY_MASK) >> RECORD_SUMMARY_SHIFT) as u8
    }

    /// The same record carrying attribute summary `summary`.
    #[inline(always)]
    pub(super) fn with_summary(mut self, summary: u8) -> ShapeRecord {
        debug_assert_eq!(summary & !crate::object::key_attrs::SUMMARY_KEY_BITS, 0);
        self.flags_and_kind = (self.flags_and_kind & !RECORD_SUMMARY_MASK)
            | ((u32::from(summary) << RECORD_SUMMARY_SHIFT) & RECORD_SUMMARY_MASK);
        // The summary is an input of the positional bit (an accessor key).
        self.refresh_positional();
        self
    }

    /// The inline width this keyless birth shape's descendants grow to
    /// (#10905), or 0 when nothing was learned.
    #[inline]
    pub(super) fn descendant_width(&self) -> u32 {
        match (self.flags_and_kind & RECORD_WIDTH_MASK) >> RECORD_WIDTH_SHIFT {
            0 => 0,
            stored => stored + 1,
        }
    }

    /// Raise [`ShapeRecord::descendant_width`] to `width` (monotone,
    /// saturating at the field's maximum, 64). A width of 1 or less teaches
    /// nothing (it is below the inline floor).
    #[inline]
    pub(super) fn note_descendant_width(&mut self, width: u32) {
        let width = width.min((RECORD_WIDTH_MASK >> RECORD_WIDTH_SHIFT) + 1);
        if width > 1 && width > self.descendant_width() {
            self.flags_and_kind =
                (self.flags_and_kind & !RECORD_WIDTH_MASK) | ((width - 1) << RECORD_WIDTH_SHIFT);
        }
    }

    /// Births this keyless birth shape served while tracking (#10905).
    #[inline]
    pub(super) fn tracked_births(&self) -> u32 {
        ((self.flags_and_kind >> RECORD_BIRTHS_SHIFT) & 7) | ((self.flags_and_kind >> 27) & 8)
    }

    #[inline]
    pub(super) fn set_tracked_births(&mut self, births: u32) {
        let births = births.min(RECORD_BIRTHS_MAX);
        let bits = ((births & 7) << RECORD_BIRTHS_SHIFT) | ((births & 8) << 27);
        self.flags_and_kind = (self.flags_and_kind & !RECORD_BIRTHS_MASK) | bits;
    }

    /// A fresh, facts-indexed record with every liveness bit clear.
    pub(super) fn new(
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
    ) -> ShapeRecord {
        let flags = RECORD_FLAG_PRESENT | RECORD_FLAG_FACTS_INDEXED;
        // The kind is a FIELD, not a flag: it has three values, and a record
        // that reported the wrong one would be a wrong identity match,
        // because `facts_match` compares the full enum.
        let kind_code = object_kind.code() as u32;
        debug_assert!(
            kind_code <= RECORD_KIND_MAX_CODE,
            "kind has no packed decoder"
        );
        let kind_bits = kind_code << RECORD_KIND_SHIFT;
        debug_assert!(kind_bits & !RECORD_KIND_MASK == 0, "kind does not fit");
        let mut record = ShapeRecord {
            keys,
            semantic_generation,
            proto_id: 0,
            logical_key_count,
            live_inline_slot_count,
            hole_count,
            flags_and_kind: u32::from(flags) | kind_bits,
            position_bound: 0,
            special_constfn_mask: 0,
            rep: 0,
            extras: 0,
            proto_cell: 0,
            keys_node_and_element_kind: 0,
            family_desc_index_plus_one: 0,
            probe: 0,
            colour_mask: [0; 2],
            colour_store_mask: [0; 2],
            shape_id: ShapeId(0),
            layout_word: LayoutWord(0),
        };
        record.refresh_positional();
        record
    }

    /// How many leading keys of this shape's list sit AT their own inline
    /// slot: logical key position `i < bound` IS inline slot `i` of every
    /// receiver carrying the shape. 0 when the shape cannot answer by position
    /// at all.
    ///
    /// It is a FACT OF THE RECORD, stored in the `position_bound` field
    /// (POSBOUND) and read here with one load: the megamorphic read confirm
    /// compares a site's slot guess against it on every latched read.
    /// [`Self::position_bound_by_facts`] is its definition; every write of one
    /// of its inputs is followed by [`Self::refresh_positional`]
    /// (construction, `with_summary`, slab insert, the in-place
    /// stable-tombstone update), and debug builds assert the stored bound
    /// against the definition on every read.
    #[cfg(test)]
    #[inline]
    pub(crate) fn position_bound(&self) -> u32 {
        debug_assert_eq!(
            self.position_bound,
            self.position_bound_by_facts(),
            "the position bound disagrees with the record's facts: {self:?}"
        );
        self.position_bound
    }

    /// The definition of POSBOUND: `min(logical_key_count,
    /// live_inline_slot_count)` for a shape that answers by position, else 0.
    /// A key past the key count is another list's (canonical backings are
    /// shared by a growth chain), and one past the live inline count is
    /// spilled.
    #[inline]
    /// Published records own live key arrays; record-only unit tests also
    /// use synthetic addresses, so there the header read is checked.
    unsafe fn keys_have_slots(&self) -> bool {
        let keys = self.keys as usize as *const crate::ArrayHeader;
        #[cfg(test)]
        return crate::object::key_attrs::keys_have_slots_checked(keys, self.logical_key_count);
        #[cfg(not(test))]
        return crate::object::key_attrs::keys_have_slots(keys, self.logical_key_count);
    }

    pub(super) fn position_bound_by_facts(&self) -> u32 {
        // Keys that keep their own storage slots are not positional.
        if !self.positional_by_facts() || unsafe { self.keys_have_slots() } {
            return 0;
        }
        self.logical_key_count.min(self.live_inline_slot_count)
    }

    /// The definition of the positional bit. The conjuncts are
    /// `js_shape_ordinary_inline_slot_for_key`'s:
    ///
    /// * an ordinary LAYOUT (`Ordinary`, and the store facts F-A/F-B that
    ///   share its layout) — a class shape's slots are its class layout, and a
    ///   DICTIONARY shape keeps its id across layout changes, so a dictionary
    ///   receiver must never be matched by position;
    /// * generation 0 — a descriptor/prototype mutation minted this layout;
    /// * no tombstones — the answer is only claimed for hole-free lists;
    /// * no ACCESSOR key in the attribute summary — an accessor key's slot
    ///   holds its accessor pair, not a value;
    /// * no key whose slot may hold the hole (`SUMMARY_HOLES`): such a key
    ///   is absent while its slot holds it (`key_view`);
    /// * a keys array at all.
    ///
    /// The field representation (`rep`) is deliberately NOT an input: an
    /// `F64` slot holds a JS Number as raw IEEE bits outside the tag band,
    /// which is itself a valid NaN-boxed value, so key position `i` is inline
    /// slot `i` whatever the slot's representation. A shape minted with a
    /// non-`Any` rep is its own record and gets its own bound from these
    /// facts at construction and slab insert, like every other shape, and
    /// deprecating a lane in place (`deprecate_rep_slot`) leaves the bound
    /// as it is, correctly.
    #[inline]
    pub(super) fn positional_by_facts(&self) -> bool {
        self.object_kind().is_ordinary_layout()
            && super::complete_layout_generation(self.semantic_generation)
            && self.hole_count == 0
            && self.keys != 0
            && self.summary()
                & (crate::object::key_attrs::SUMMARY_ACCESSOR
                    | crate::object::key_attrs::SUMMARY_HOLES)
                == 0
    }

    /// Rewrite POSBOUND from the record's facts.
    #[inline]
    pub(super) fn refresh_positional(&mut self) {
        self.position_bound = self.position_bound_by_facts();
    }

    /// The stored POSBOUND without the debug agreement assert, for the
    /// agreement test.
    #[cfg(test)]
    pub(super) fn stored_position_bound(&self) -> u32 {
        self.position_bound
    }

    /// The stored POSBOUND for the megamorphic read confirm, which must stay
    /// a GC leaf with no formatting path: the agreement is asserted by
    /// [`Self::position_bound`] everywhere else and by the census test.
    #[inline(always)]
    pub(crate) fn position_bound_raw(&self) -> u32 {
        self.position_bound
    }

    /// The same record carrying field representation `rep` (`field_rep`).
    #[cfg(test)]
    #[inline]
    pub(super) fn with_rep(mut self, rep: u64) -> ShapeRecord {
        debug_assert!(crate::object::field_rep::is_valid(rep), "reserved rep lane");
        self.rep = rep;
        self
    }

    /// ConstFn slots; a `REP_SPECIAL` slot outside this mask is the reserved
    /// NoPointer representation. GC must visit ConstFn slots.
    #[inline]
    pub(crate) fn special_constfn_mask(&self) -> u32 {
        self.special_constfn_mask
    }

    #[inline]
    pub(crate) fn constfn_infos(&self) -> &[ConstFnSlotInfo] {
        if !self.has_boxed_extras() {
            &[]
        } else {
            // SAFETY: the live slab record owns this allocation; copies of
            // the record borrow it and a rekey transfers its ownership.
            unsafe { &(*(self.extras as usize as *const ShapeExtras)).constfn_infos }
        }
    }

    /// The private brands every receiver of this shape carries (#11791).
    #[inline]
    pub(crate) fn brands(&self) -> &[u64] {
        if !self.has_boxed_extras() {
            &[]
        } else {
            // SAFETY: the live slab record owns this allocation.
            unsafe { &(*(self.extras as usize as *const ShapeExtras)).brands }
        }
    }

    #[inline]
    pub(crate) fn deprecation_targets(&self) -> (u32, u32) {
        if !self.has_boxed_extras() {
            (0, 0)
        } else {
            // SAFETY: the extension is owned by this live record.
            let extras = unsafe { &*(self.extras as usize as *const ShapeExtras) };
            (
                extras
                    .to_nopointer
                    .load(std::sync::atomic::Ordering::Acquire),
                extras.to_any.load(std::sync::atomic::Ordering::Acquire),
            )
        }
    }

    /// A ConstFn lineage learned a different function or a nonfunction. It
    /// keeps its old body invariant for existing carriers, but migrates them
    /// to an Any successor on miss. NoPointer can use the same target later.
    pub(crate) fn deprecate_special_to_any(&self, slot: u32) -> bool {
        assert!(slot < crate::object::field_rep::REP_SLOTS);
        let bit = 1u32 << slot;
        assert_ne!(self.special_constfn_mask & bit, 0);
        assert!(self.has_boxed_extras());
        // SAFETY: the record owns the extension for its entire live span.
        let extras = unsafe { &*(self.extras as usize as *const ShapeExtras) };
        extras
            .to_any
            .fetch_or(bit, std::sync::atomic::Ordering::AcqRel)
            & bit
            == 0
    }

    /// Called only when a record is retired, not when it is copied or rekeyed.
    /// The returned slab record owns the pointer until this call consumes it.
    pub(super) unsafe fn release_extras(self) {
        if self.has_boxed_extras() {
            drop(Box::from_raw(self.extras as usize as *mut ShapeExtras));
        }
    }

    /// The same record for a receiver whose [[Prototype]] identity is
    /// `proto_id` (see [`ShapeRecord::proto_id`]).
    #[inline(always)]
    pub(super) fn with_proto_id(mut self, proto_id: u64) -> ShapeRecord {
        self.proto_id = proto_id;
        self.proto_cell = 0;
        self
    }

    /// [`ShapeRecord::facts_match`] including the prototype identity — the
    /// test every production interning path uses.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub(super) fn facts_match_proto(
        &self,
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
        proto_id: u64,
        summary: u8,
        rep: u64,
    ) -> bool {
        self.facts_match_proto_with_special(
            keys,
            logical_key_count,
            live_inline_slot_count,
            semantic_generation,
            object_kind,
            hole_count,
            proto_id,
            summary,
            rep,
            &[],
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    #[inline(always)]
    pub(super) fn facts_match_proto_with_special(
        &self,
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
        proto_id: u64,
        summary: u8,
        rep: u64,
        infos: &[ConstFnSlotInfo],
        brands: &[u64],
    ) -> bool {
        let Some(mask) = constfn_mask(infos) else {
            return false;
        };
        self.proto_id == proto_id
            && self.summary() == summary
            && crate::object::field_rep::identity_with_special(self.rep)
                == crate::object::field_rep::identity_with_special(rep)
            && self.special_constfn_mask == mask
            // Both masks came from validated immutable lists: zero iff empty.
            // There is no body identity to fetch from the cold extension then.
            && (mask == 0 || self.constfn_infos() == infos)
            && brand_lists_equal(self.brands(), brands)
            && self.facts_match(
                keys,
                logical_key_count,
                live_inline_slot_count,
                semantic_generation,
                object_kind,
                hole_count,
            )
    }

    /// Exact-facts identity test (#8067): keys edge, both counts, generation,
    /// kind, tombstones. Liveness bits and the facts-indexed bit are storage
    /// state, never identity.
    #[inline(always)]
    pub(super) fn facts_match(
        &self,
        keys: u64,
        logical_key_count: u32,
        live_inline_slot_count: u32,
        semantic_generation: u64,
        object_kind: ShapeObjectKind,
        hole_count: u32,
    ) -> bool {
        self.keys == keys
            && self.logical_key_count == logical_key_count
            && self.live_inline_slot_count == live_inline_slot_count
            && self.semantic_generation == semantic_generation
            && self.hole_count == hole_count
            && self.object_kind() == object_kind
    }

    /// The 64-bit fold of the six identity facts, with `keys` supplied by
    /// the caller: the collector rewrites a record's `keys` in place, so the
    /// address the record was INDEXED under (its family key) is what the
    /// exact-facts accelerator must be probed with until the metadata scan
    /// re-indexes it.
    #[inline]
    pub(super) fn facts_key_with_keys(&self, keys: u64) -> u64 {
        integrity_facts_key(
            facts_key_proto_with_special(
                keys,
                self.logical_key_count,
                self.live_inline_slot_count,
                self.semantic_generation,
                self.object_kind(),
                self.hole_count,
                self.proto_id,
                self.summary(),
                self.rep,
                self.constfn_infos(),
                self.brands(),
            ),
            self.integrity_flags(),
        )
    }

    /// Copy the record out as the by-value [`ShapeDescriptor`] the rest of the
    /// runtime consumes. `record` is the slab address of THIS record, which is
    /// what `keys_slot()` and the tombstone fast paths hand back to the table.
    #[inline]
    pub(super) fn lift(&self, record: *mut ShapeRecord) -> ShapeDescriptor {
        ShapeDescriptor {
            keys: self.keys,
            record: record as usize,
            old_carrier: self.has(RECORD_FLAG_OLD_CARRIER),
            cache_carrier: self.cache_carrier(),
            logical_key_count: self.logical_key_count,
            live_inline_slot_count: self.live_inline_slot_count,
            semantic_generation: self.semantic_generation,
            integrity_flags: self.integrity_flags(),
            proto_id: self.proto_id,
            object_kind: self.object_kind(),
            hole_count: self.hole_count,
            summary: self.summary(),
            rep: self.rep,
            special_constfn_mask: self.special_constfn_mask,
            // A descriptor borrows only extension metadata. The scalar
            // reverse edge stays on its record and is never a pointer.
            extras: if self.has_boxed_extras() {
                self.extras
            } else {
                0
            },
        }
    }
}

/// FNV-1a fold of the six identity facts into the single word the
/// exact-facts accelerator is keyed by. Every field reaches the accumulator
/// (fold, never overwrite — the property `PtrHasher` lacks and the reason the
/// old `ShapeFacts` map could not use it); a 64-bit collision between two
/// live shapes is resolved by the per-hit `facts_match` on the record, so a
/// collision only costs a second record read, never a wrong answer.
/// [`facts_key`] for a record at the DEFAULT prototype identity (0).
#[cfg(test)]
#[inline]
pub(super) fn facts_key(
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
) -> u64 {
    facts_key_proto(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        0,
        0,
        0,
    )
}

/// The identity facts, the prototype identity, the attribute summary and the
/// field representation included.
#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn facts_key_proto(
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
) -> u64 {
    facts_key_proto_with_special(
        keys,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation,
        object_kind,
        hole_count,
        proto_id,
        summary,
        rep,
        &[],
        &[],
    )
}

/// The extension record of a shape with ConstFn slots or private brands, out
/// of line: almost every mint has neither.
#[cold]
#[inline(never)]
fn new_extras(infos: &[ConstFnSlotInfo], brands: &[u64]) -> u64 {
    let extras = Box::new(ShapeExtras {
        constfn_infos: infos.into(),
        brands: brands.into(),
        to_nopointer: std::sync::atomic::AtomicU32::new(0),
        to_any: std::sync::atomic::AtomicU32::new(0),
        rollback_parent: std::sync::atomic::AtomicU32::new(0),
        created_birth_shape: std::sync::atomic::AtomicU32::new(0),
        field_define_target: 0,
        integrity_targets: [0; 3],
        integrity_flags: 0,
    });
    Box::into_raw(extras) as usize as u64
}

/// Brand-list identity. Nearly every shape has none, and slice equality calls
/// `memcmp` even for two empty lists, which the shape intern's hit path paid
/// on every lookup (#11791).
#[inline(always)]
pub(crate) fn brand_lists_equal(a: &[u64], b: &[u64]) -> bool {
    a.len() == b.len() && (a.is_empty() || a == b)
}

/// Extended exact-facts hash. Old shapes take the wrapper above and get the
/// exact old fold; ConstFn adds a domain-separated ordered body list. Address
/// values are process-local identities, never serialized as static seed keys.
#[allow(clippy::too_many_arguments)]
#[inline(always)]
pub(super) fn facts_key_proto_with_special(
    keys: u64,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    semantic_generation: u64,
    object_kind: ShapeObjectKind,
    hole_count: u32,
    proto_id: u64,
    summary: u8,
    rep: u64,
    infos: &[ConstFnSlotInfo],
    brands: &[u64],
) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let fold = |acc: u64, word: u64| (acc ^ word).wrapping_mul(FNV_PRIME);
    let mut h = fold(FNV_OFFSET_BASIS, keys);
    h = fold(h, u64::from(logical_key_count));
    h = fold(h, u64::from(live_inline_slot_count));
    h = fold(h, semantic_generation);
    h = fold(h, u64::from(hole_count));
    // The DISCRIMINANT, not `== Class`: folding a bool would give Ordinary
    // and Dictionary the same hash contribution. `facts_match` re-checks the
    // full enum on every hit, so that was never a wrong answer — but it is a
    // silent hash-quality loss, and the two kinds differ in every consumer.
    h = fold(h, object_kind.code());
    h = fold(h, proto_id);
    // Folded only when nonzero, so every attribute-free shape keeps the key
    // it had before the summary existed.
    if summary != 0 {
        h = fold(h, 0x5_0000 | u64::from(summary));
    }
    // The same rule for the field representation: an all-`Any` shape keeps
    // the key it had before the word existed. The deprecated state is not
    // identity, so it is masked here as it is in `facts_match_proto`.
    let rep = crate::object::field_rep::identity_with_special(rep);
    if rep != 0 {
        h = fold(h, 0x6_0000);
        h = fold(h, rep);
    }
    if !infos.is_empty() {
        h = fold(h, 0x7_4346_4e);
        for entry in infos {
            h = fold(h, u64::from(entry.slot));
            h = fold(h, entry.info);
        }
    }
    // Brands (#11791) are domain-separated the same way; a brandless shape
    // keeps the key it had before brands existed.
    if !brands.is_empty() {
        h = fold_brands(h, brands);
    }
    // Final avalanche: FNV keeps most of its entropy in the high bits and
    // hashbrown's probe sequence starts from the LOW bits.
    h ^ (h >> 32)
}

/// The brand half of [`facts_key_proto_with_special`], out of line: almost
/// no shape carries a brand, and the hash runs on every intern.
#[cold]
#[inline(never)]
fn fold_brands(mut h: u64, brands: &[u64]) -> u64 {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let fold = |acc: u64, word: u64| (acc ^ word).wrapping_mul(FNV_PRIME);
    h = fold(h, 0x8_4252_4e44);
    for &brand in brands {
        h = fold(h, brand);
    }
    h
}

/// Records per chunk. Ids are minted far faster than they survive — the
/// compiled claude-code TUI mints ~1.05 M ShapeIds during startup and keeps
/// ~44 k, scattered over the whole range — so a chunk is deliberately SMALL
/// (32 records, 1 KB): an all-dead chunk is released whole, and the smaller
/// the chunk the less of a survivor's neighbourhood it drags along. Measured
/// on that TUI, 256-record chunks held 7.15 MB for those 44 k records and
/// 32-record chunks 4.0 MB.
const CHUNK_SHIFT: usize = 5;
const CHUNK_LEN: usize = 1 << CHUNK_SHIFT;
const CHUNK_MASK: usize = CHUNK_LEN - 1;

/// Chunk pointers per directory page. The directory is two-level so its
/// size follows the LIVE id range, not the minted one: a long-running server
/// minting a billion ids over its life would otherwise carry a flat
/// `Vec<Option<Chunk>>` of 250 MB at 32 records per chunk. A page is 8 KB and
/// covers 32 K ids; a page whose chunks have all been released is dropped.
const PAGE_SHIFT: usize = 10;
const PAGE_LEN: usize = 1 << PAGE_SHIFT;
const PAGE_MASK: usize = PAGE_LEN - 1;

/// One lazily allocated run of `CHUNK_LEN` consecutive ids. The cells give
/// the table interior mutability through a shared slab reference: the
/// collector writes liveness bits and the `keys` word through raw record
/// pointers while other code holds only copies (`ShapeDescriptor`).
type ChunkCells = [UnsafeCell<*mut ShapeRecord>; CHUNK_LEN];

/// One directory page: `PAGE_LEN` chunk slots.
type PageSlots = [Slot<ChunkCells>; PAGE_LEN];

/// A directory or page entry: an allocation this slab owns, or the SHARED
/// all-empty one of its level ([`EMPTY_CHUNK`], [`EMPTY_PAGE`]) — never null.
/// An absent run therefore reads exactly like a present run of absent
/// records (`ShapeRecord::EMPTY`: not present, position bound 0, every lane
/// `Any`), so a reader walks page → chunk → record with no null test at
/// either level ([`ShapeSlab::agent_record`], [`ShapeSlab::ordinary_record_in`]).
/// Nothing is ever written
/// through a shared empty: every writer asks [`Slot::is_shared`] first and
/// allocates. The slab frees what it owns ([`ShapeSlab::free_dir`]).
#[repr(transparent)]
struct Slot<T>(std::ptr::NonNull<T>);

impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Slot<T> {}

/// A shared all-empty allocation. Never written (see [`Slot`]).
#[repr(transparent)]
pub struct SharedEmpty<T>(T);
// SAFETY: nothing ever writes a shared empty; every reader only loads.
unsafe impl<T> Sync for SharedEmpty<T> {}

static EMPTY_CHUNK: SharedEmpty<ChunkCells> =
    SharedEmpty([const { UnsafeCell::new(std::ptr::null_mut()) }; CHUNK_LEN]);
static EMPTY_RECORD: SharedEmpty<ShapeRecord> = SharedEmpty(ShapeRecord::EMPTY);
static EMPTY_PAGE: SharedEmpty<PageSlots> = SharedEmpty(
    // SAFETY: the address of a static is never null.
    [Slot(unsafe {
        std::ptr::NonNull::new_unchecked(std::ptr::addr_of!(EMPTY_CHUNK.0) as *mut ChunkCells)
    }); PAGE_LEN],
);
trait Level: Sized + 'static {
    fn shared() -> std::ptr::NonNull<Self>;
    fn fresh() -> Box<Self>;
}

impl Level for ChunkCells {
    fn shared() -> std::ptr::NonNull<Self> {
        std::ptr::NonNull::from(&EMPTY_CHUNK.0)
    }
    fn fresh() -> Box<Self> {
        let mut v: Vec<UnsafeCell<*mut ShapeRecord>> = Vec::with_capacity(CHUNK_LEN);
        v.resize_with(CHUNK_LEN, || UnsafeCell::new(std::ptr::null_mut()));
        // Exact length by construction; the conversion moves the allocation.
        v.into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| unreachable!("chunk vector has CHUNK_LEN cells"))
    }
}

impl Level for PageSlots {
    fn shared() -> std::ptr::NonNull<Self> {
        std::ptr::NonNull::from(&EMPTY_PAGE.0)
    }
    fn fresh() -> Box<Self> {
        let mut v: Vec<Slot<ChunkCells>> = Vec::with_capacity(PAGE_LEN);
        v.resize_with(PAGE_LEN, Slot::empty);
        v.into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| unreachable!("page vector has PAGE_LEN slots"))
    }
}

impl<T: Level> Slot<T> {
    #[inline]
    fn empty() -> Self {
        Slot(T::shared())
    }
    #[inline]
    fn is_shared(self) -> bool {
        self.0 == T::shared()
    }
    /// The owned allocation, or `None` for the shared empty.
    #[inline]
    fn owned(&self) -> Option<&T> {
        // SAFETY: an owned slot points at a live allocation of this slab.
        (!self.is_shared()).then(|| unsafe { self.0.as_ref() })
    }
    #[inline]
    fn owned_mut(&mut self) -> Option<&mut T> {
        // SAFETY: as `owned`; `&mut self` is the slab's exclusive borrow.
        (!self.is_shared()).then(|| unsafe { self.0.as_mut() })
    }
    /// The owned allocation, allocating it first if this is the shared empty.
    #[inline]
    fn owned_or_alloc(&mut self) -> &mut T {
        if self.is_shared() {
            self.0 = std::ptr::NonNull::from(Box::leak(T::fresh()));
        }
        // SAFETY: owned now.
        unsafe { self.0.as_mut() }
    }
    /// Free an owned allocation (not its children) and become the shared
    /// empty.
    fn release(&mut self) {
        if !self.is_shared() {
            // SAFETY: allocated by `owned_or_alloc`, freed once: the slot is
            // the shared empty afterwards.
            drop(unsafe { Box::from_raw(self.0.as_ptr()) });
            self.0 = T::shared();
        }
    }
}

type Page = Slot<PageSlots>;

/// Band-relative starts: the dictionary band, the exotic band, and the end of
/// the ShapeId range, each relative to `SHAPE_ID_BASE`.
const DICT_REL: u32 = SITE_SHAPE_ID_END - SHAPE_ID_BASE;
const EXOTIC_REL: u32 = EXOTIC_SHAPE_ID_BASE - SHAPE_ID_BASE;
const END_REL: u32 = super::SHAPE_ID_END - SHAPE_ID_BASE;

/// The band boundaries are multiples of `1 << BAND_SHIFT` (relative to
/// `SHAPE_ID_BASE`): the ordinary band is segments 0-5, the dictionary band
/// segment 6, the exotic band segment 7, and segment 8 on is not a ShapeId.
const BAND_SHIFT: u32 = 27;
const _: () = assert!(DICT_REL == 6 << BAND_SHIFT);
const _: () = assert!(EXOTIC_REL == 7 << BAND_SHIFT);
const _: () = assert!(END_REL == 8 << BAND_SHIFT);
/// Each band's first id, relative to `SHAPE_ID_BASE`, by band.
const BAND_BASE_REL: [u32; 4] = [0, DICT_REL, EXOTIC_REL, END_REL];

/// The band of `rel = id - SHAPE_ID_BASE` (wrapping): 0 ordinary, 1
/// dictionary, 2 exotic receivers, and 3 "not a ShapeId" (an id below
/// `SHAPE_ID_BASE` wraps past `END_REL`), whose directory is always empty.
/// A shift, a saturating subtract and a min: no branch, because every
/// lookup starts here.
#[inline(always)]
fn band_of(rel: u32) -> usize {
    (rel >> BAND_SHIFT).saturating_sub(5).min(3) as usize
}

/// Where `id` lives: `(band, index within that band's directory)`. Total:
/// band 3's index is meaningless and its directory empty.
#[inline(always)]
fn locate(id: u32) -> (usize, usize) {
    // A header word may carry the shadow bit; the record is the layout's.
    let rel = id.wrapping_sub(SHAPE_ID_BASE);
    let band = band_of(rel);
    (band, rel.wrapping_sub(BAND_BASE_REL[band]) as usize)
}

/// One band's directory as published for [`ShapeSlab::agent_record`]: the
/// element pointer and length of that band's `Vec<Page>`, and the band's
/// first id relative to `SHAPE_ID_BASE` (a constant, kept beside the pair so
/// the lookup reads it from the same line). 32 bytes, so the band indexes
/// the directory with a shift. The ordinary band's entry is also what the
/// megamorphic read confirm reads through its address
/// ([`ShapeSlab::ordinary_dir_addr`]).
#[repr(C, align(64))]
pub(crate) struct BandDir {
    pages: std::cell::Cell<*const Page>,
    #[cfg(target_pointer_width = "32")]
    _pages_high: u32,
    len: std::cell::Cell<usize>,
    #[cfg(target_pointer_width = "32")]
    _len_high: u32,
    base_rel: u32,
    #[cfg(target_pointer_width = "32")]
    _base_rel_pad: u32,
    /// The ordinary band's records at fixed addresses (`flat.rs`): record
    /// `rel` is `flat_base + rel` for every `rel < flat_ids`; null and 0 for
    /// every other band, every target without the reservation, and a slab
    /// that publishes nothing. Offsets 24 and 32 (`FLAT_BASE_OFFSET`).
    flat_base: std::cell::Cell<*mut ShapeRecord>,
    #[cfg(target_pointer_width = "32")]
    _flat_base_high: u32,
    flat_ids: std::cell::Cell<usize>,
    #[cfg(target_pointer_width = "32")]
    _flat_ids_high: u32,
    /// This agent's `Object.prototype` cell (`array::prototype_addr`), for
    /// the probe's prototype walk; null when not published. Offset 40.
    pub(crate) proto_cell: std::cell::Cell<*const usize>,
    #[cfg(target_pointer_width = "32")]
    _proto_cell_high: u32,
    /// This agent's identity-word directory (`shapes_prototype`), for the
    /// probe's class-holder hop; null when not published. Offset 48.
    pub(crate) word_dir: std::cell::Cell<*const u8>,
    #[cfg(target_pointer_width = "32")]
    _word_dir_high: u32,
}

/// `BandDir.flat_base`'s offset; `flat_ids` follows at +8.
pub(crate) const FLAT_BASE_OFFSET: usize = crate::codegen_abi::SHAPE_DIR_FLAT_BASE;
const _: () = assert!(std::mem::offset_of!(BandDir, flat_base) == FLAT_BASE_OFFSET);
const _: () = assert!(std::mem::offset_of!(BandDir, flat_ids) == FLAT_BASE_OFFSET + 8);
const _: () = assert!(std::mem::offset_of!(BandDir, proto_cell) == FLAT_BASE_OFFSET + 16);
const _: () = assert!(std::mem::offset_of!(BandDir, word_dir) == FLAT_BASE_OFFSET + 24);

impl BandDir {
    const fn empty(band: usize) -> BandDir {
        BandDir {
            pages: std::cell::Cell::new(std::ptr::null()),
            #[cfg(target_pointer_width = "32")]
            _pages_high: 0,
            len: std::cell::Cell::new(0),
            #[cfg(target_pointer_width = "32")]
            _len_high: 0,
            base_rel: BAND_BASE_REL[band],
            #[cfg(target_pointer_width = "32")]
            _base_rel_pad: 0,
            flat_base: std::cell::Cell::new(std::ptr::null_mut()),
            #[cfg(target_pointer_width = "32")]
            _flat_base_high: 0,
            flat_ids: std::cell::Cell::new(0),
            #[cfg(target_pointer_width = "32")]
            _flat_ids_high: 0,
            proto_cell: std::cell::Cell::new(std::ptr::null()),
            #[cfg(target_pointer_width = "32")]
            _proto_cell_high: 0,
            word_dir: std::cell::Cell::new(std::ptr::null()),
            #[cfg(target_pointer_width = "32")]
            _word_dir_high: 0,
        }
    }
}

/// THIS agent's shape directory: the page pointers and page count of each
/// band of its slab (the `RuntimeState` shape table's), republished by the
/// slab after every change to a band's page vector and reset to all-empty
/// when the slab is dropped. Band 3 ("not a ShapeId") is never written.
///
/// It is what makes the by-id lookup free of the runtime state: a const
/// `#[thread_local]` with no destructor is valid from the thread's first
/// instruction, so [`ShapeSlab::agent_record`] reads an always-valid
/// directory with one thread-pointer-relative load — no `state()` fetch, no
/// lazy-init branch. An agent that has not built its runtime state yet has
/// no shapes, and its all-empty directory says exactly that. Per agent by
/// construction: every agent is its own thread with its own directory.
#[thread_local]
static AGENT_SHAPE_DIR: [BandDir; 4] = [
    BandDir::empty(0),
    BandDir::empty(1),
    BandDir::empty(2),
    BandDir::empty(3),
];

/// This agent's commit view of its Region (top vector of live-page leaves,
/// its length, the page shift), published beside [`AGENT_SHAPE_DIR`] so a
/// held-word read proves its row is committed without reaching the slab.
#[thread_local]
static AGENT_COMMIT: (
    std::cell::Cell<*const ()>,
    std::cell::Cell<usize>,
    std::cell::Cell<u32>,
) = (
    std::cell::Cell::new(std::ptr::null()),
    std::cell::Cell::new(0),
    std::cell::Cell::new(0),
);

/// An ordinary-band directory of no pages, never written: the value of the
/// agent's shape-directory pointer slot until the agent publishes its own
/// (`agent_ptrs::PERRY_AGENT_PTRS`), and what a `length` read site passes
/// (`perry-codegen` `generic_dispatch.rs`). Every id indexes past its length
/// and reads the shared empty record, so a reader never needs a null test for
/// the directory itself.
#[no_mangle]
pub static PERRY_EMPTY_SHAPE_DIR: SharedEmpty<BandDir> = SharedEmpty(BandDir::empty(0));

/// The by-id descriptor store. See the module docs.
/// Two page directories: ordinary ShapeIds index from `SHAPE_ID_BASE`, and the
/// dictionary band (`shapes::SITE_SHAPE_ID_END`) from its own base. One
/// directory indexed from `SHAPE_ID_BASE` would grow to ~24,577 page slots the
/// moment the first dictionary id is minted; measured on `ts.transpileModule`,
/// that one ~196 KB allocation moved the GC arena's pages relative to the
/// page-class table window and cost +2.3% instructions (1.65 M vs 0.20 M
/// registered-page misses in `classify_heap_generation`).
pub(crate) struct ShapeSlab {
    pages: Vec<Page>,
    dict_pages: Vec<Page>,
    /// The exotic-receiver band (`shapes::EXOTIC_SHAPE_ID_BASE`).
    exotic_pages: Vec<Page>,
    /// The ordinary band's chunks live here (`flat.rs`).
    region: flat::Region,
    static_rows: u32,
    /// Next free dynamic row inside this agent's current chunk (the static
    /// row count before the first claim). Also the high-water bound readers use.
    next_row: u32,
    /// One past the highest row this agent ever issued: the bound readers use
    /// (`flat_ids`). Chunks need not arrive in increasing order.
    row_bound: u32,
    /// Starts of the chunks this agent owns; returned to the process
    /// allocator when the slab drops.
    chunks: OwnedChunks,
    /// End (exclusive) of the current chunk; reaching it claims another.
    chunk_end: u32,
    /// Present records.
    len: usize,
    /// This is the agent's slab (the runtime state's shape table), so it
    /// publishes its directories into [`AGENT_SHAPE_DIR`]. A slab a test
    /// builds on its own does not.
    agent: bool,
    /// The agent's identity -> [[Prototype]] words (`shapes_prototype`).
    pub(super) protos: super::shapes_prototype::ProtoWords,
}

impl Drop for ShapeSlab {
    fn drop(&mut self) {
        if self.agent {
            for band in &AGENT_SHAPE_DIR[..3] {
                band.pages.set(std::ptr::null());
                band.len.set(0);
                band.flat_base.set(std::ptr::null_mut());
                band.flat_ids.set(0);
                band.proto_cell.set(std::ptr::null());
                band.word_dir.set(std::ptr::null());
            }
            AGENT_COMMIT.0.set(std::ptr::null());
            AGENT_COMMIT.1.set(0);
            self.protos.unpublish();
        }
        // Retired records release on removal; live records release at agent
        // teardown. Rekeys transfer one pointer, never duplicate ownership.
        self.for_each(|_, ptr| unsafe {
            (*ptr).release_extras();
            crate::object::layout_probe::release_probe((*ptr).probe);
            (*ptr).probe = 0;
        });
        for band in [0u8, 1, 2] {
            Self::free_dir(self.dir_mut(band), band);
        }
        // The Region unmaps with the slab; its chunks go back to the process.
        CHUNKS.release(self.chunks.inline(), &self.chunks.more);
    }
}

#[path = "shapes_store/slab.rs"]
mod slab;

/// MEASUREMENT that this structure is judged on, and the test's instrument.
///
/// Two counters on the id-list mutation path, kept unconditionally because the
/// rig falsifier and the unit guard both read them and a `cfg(test)` counter
/// can only prove the test's own arithmetic. Deliberately a THREE-field struct
/// in one `Cell`: a thread-local `Cell<T>` get/set copies `T` on every
/// operation, and this path runs millions of times per turn, so the width of
/// this type is itself a cost.
#[derive(Default, Clone, Copy)]
pub(crate) struct IdListOpStats {
    /// Removals that found their id.
    pub(crate) removals: u64,
    /// Elements shifted by a removal. Bytes = this x 4. Swap-remove moves
    /// none; `Vec::remove` moves the whole tail past the removed position.
    pub(crate) elems_moved: u64,
    /// Entries touched by a linear membership or position scan. The other
    /// half of the same defect: the index removes this too.
    pub(crate) positions_scanned: u64,
}

crate::perry_thread_local! {
    pub(crate) static ID_LIST_OP_STATS: std::cell::Cell<IdListOpStats> =
        const {
            std::cell::Cell::new(IdListOpStats {
                removals: 0,
                elems_moved: 0,
                positions_scanned: 0,
            })
        };
}

#[inline]
fn note_scan(entries: usize) {
    ID_LIST_OP_STATS.with(|c| {
        let mut st = c.get();
        st.positions_scanned += entries as u64;
        c.set(st);
    });
}

#[inline]
fn note_removal(elems_moved: usize) {
    ID_LIST_OP_STATS.with(|c| {
        let mut st = c.get();
        st.removals += 1;
        st.elems_moved += elems_moved as u64;
        c.set(st);
    });
}

/// One `[gc-idlist]` line per copying minor under `PERRY_GC_DIAG=1`,
/// cumulative. `elems_moved` is the rig falsifier for this change.
pub(crate) fn id_list_report() {
    if !crate::gc::gc_diag_enabled() {
        return;
    }
    let st = ID_LIST_OP_STATS.with(std::cell::Cell::get);
    if st.removals == 0 {
        return;
    }
    eprintln!(
        "[gc-idlist] removals={} elems_moved={} bytes_moved={} positions_scanned={}",
        st.removals,
        st.elems_moved,
        st.elems_moved * 4,
        st.positions_scanned,
    );
}

/// A spilled id list: the ids, plus an `id -> index` map built once the list
/// is large enough for a linear scan to cost more than a hash probe.
///
/// The index is what makes `remove_unordered`, `contains` and `position` O(1)
/// on the lists that actually get long. Below [`SPILL_INDEX_MIN`] it stays
/// empty and every operation is the linear scan it always was, because for a
/// handful of entries the scan is a single cache line and the map is not.
#[derive(Clone, Debug, Default)]
pub(super) struct SpillList {
    ids: Vec<LayoutWord>,
    /// Empty while `ids.len() < SPILL_INDEX_MIN`; complete above it.
    ///
    /// `PtrHasher` (#8125) is the right hasher here for the same reason it is
    /// on the maps around it: shape ids come from a monotonic counter, so the
    /// key is a small dense integer and the avalanche step is what keeps every
    /// one of them off bucket 0.
    pos: crate::fast_hash::PtrHashMap<u32, u32>,
}

/// Where the index starts paying. Measured shape of the problem: `families`
/// lists reach 514,030 entries on a claude-code reply while `by_facts` lists
/// are length 1, so anything in the low tens is far below the case that hurts
/// and far above the case where the map would be pure overhead.
const SPILL_INDEX_MIN: usize = 32;

impl SpillList {
    #[inline]
    fn indexed(&self) -> bool {
        !self.pos.is_empty()
    }

    /// Build the index if the list has just crossed the threshold. Called
    /// after every growth, so the map exists from the first entry past it.
    #[inline]
    fn maybe_build_index(&mut self) {
        if self.pos.is_empty() && self.ids.len() >= SPILL_INDEX_MIN {
            self.pos.reserve(self.ids.len());
            for (i, &id) in self.ids.iter().enumerate() {
                self.pos.insert(id.0, i as u32);
            }
        }
    }

    /// Position of `id`, O(1) when indexed and a counted linear scan below the
    /// threshold.
    #[inline]
    fn position(&self, id: LayoutWord) -> Option<usize> {
        if self.indexed() {
            return self.pos.get(&id.0).map(|&i| i as usize);
        }
        note_scan(self.ids.len());
        self.ids.iter().position(|&x| x == id)
    }

    #[inline]
    fn push(&mut self, id: LayoutWord) {
        let i = self.ids.len();
        self.ids.push(id);
        if self.indexed() {
            self.pos.insert(id.0, i as u32);
        } else {
            self.maybe_build_index();
        }
    }

    /// ORDER-PRESERVING removal, for a list whose order is load-bearing.
    /// O(n) in the tail by construction — that is what "preserve the order"
    /// costs — and it reindexes the shifted suffix.
    fn remove_ordered(&mut self, id: LayoutWord) -> Option<usize> {
        let pos = self.position(id)?;
        let moved = self.ids.len() - 1 - pos;
        note_removal(moved);
        self.ids.remove(pos);
        if self.indexed() {
            self.pos.remove(&id.0);
            for (i, &other) in self.ids.iter().enumerate().skip(pos) {
                self.pos.insert(other.0, i as u32);
            }
        }
        Some(pos)
    }

    /// UNORDERED removal: the last element takes the removed one's slot.
    /// Moves ONE element regardless of position, which is the whole point —
    /// the measured removals sit at position ~0.31 of a list up to 514,030
    /// long, so `Vec::remove` was shifting essentially the entire list every
    /// time.
    ///
    /// What this does NOT claim: that the memmove explains the bimodal turn
    /// CPU. On perrymaster one draw moved 335 GB and was as fast as a draw
    /// that moved 16 GB, so bytes moved is necessary but not sufficient for
    /// the slow mode. This removes work that is unambiguously wasted; how much
    /// TIME it removes is the A/B's to say.
    fn remove_unordered(&mut self, id: LayoutWord) -> Option<usize> {
        let pos = self.position(id)?;
        note_removal(if pos + 1 == self.ids.len() { 0 } else { 1 });
        let last = self.ids.len() - 1;
        self.ids.swap_remove(pos);
        if self.indexed() {
            self.pos.remove(&id.0);
            if pos != last {
                // The element that was last now lives at `pos`.
                self.pos.insert(self.ids[pos].0, pos as u32);
            }
        }
        Some(pos)
    }

    #[inline]
    fn replace(&mut self, old: LayoutWord, new: LayoutWord) -> bool {
        let Some(pos) = self.position(old) else {
            return false;
        };
        self.ids[pos] = new;
        if self.indexed() {
            self.pos.remove(&old.0);
            self.pos.insert(new.0, pos as u32);
        }
        true
    }
}

/// A compact list of descriptor ids: up to three inline, then a spilled
/// `Vec` with an `id -> index` map (see [`SpillList`]). Sized so a family-index
/// bucket is `(u64, IdList)` = 24 bytes.
///
/// # Order
/// Order is meaningful **for `by_facts` only**: [`IdList::push_front`] is how
/// an installed process-global id becomes the canonical answer for exact-facts
/// interning ahead of an equivalent local id (`install_external_shape_id`), and
/// that list is read first-wins. `families` is NOT order-sensitive: its only
/// order-touching reader is the "one descriptor stands for the family" choice
/// in the two rekey walks, which breaks on the first carrier and otherwise
/// takes any present member — and the chosen descriptor feeds exactly one
/// expression, `old_carrier || cache_carrier`, whose value is the same for
/// every carrier and the same for every non-carrier. The outcome is a function
/// of the SET, not of the order.
///
/// That asymmetry is why removal comes in two flavours:
/// [`IdList::remove_ordered`] for `by_facts` and [`IdList::remove_unordered`]
/// for `families`. **The caller declares the contract**, because the caller is
/// the one that knows whether its order is load-bearing; a single `remove` that
/// guessed would be the bug.
#[derive(Clone, Debug)]
pub(super) enum IdList {
    Inline { len: u8, ids: [LayoutWord; 3] },
    // The `Box` is the point: an inline `SpillList` is far wider and would make
    // every bucket pay for it; the spill is the rare case, so its extra
    // indirection is cheaper than those bytes on every family.
    Spill(Box<SpillList>),
}

const _: () = assert!(std::mem::size_of::<IdList>() == 16);

impl Default for IdList {
    fn default() -> Self {
        IdList::Inline {
            len: 0,
            ids: [LayoutWord(0); 3],
        }
    }
}

impl IdList {
    #[inline]
    pub(super) fn as_slice(&self) -> &[LayoutWord] {
        match self {
            IdList::Inline { len, ids } => &ids[..*len as usize],
            IdList::Spill(v) => v.ids.as_slice(),
        }
    }

    #[inline]
    pub(super) fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub(super) fn contains(&self, id: LayoutWord) -> bool {
        match self {
            IdList::Inline { len, ids } => ids[..*len as usize].contains(&id),
            IdList::Spill(v) => v.position(id).is_some(),
        }
    }

    fn spill(&mut self) -> &mut SpillList {
        if let IdList::Inline { len, ids } = self {
            let v = SpillList {
                ids: ids[..*len as usize].to_vec(),
                pos: crate::fast_hash::new_ptr_hash_map(),
            };
            *self = IdList::Spill(Box::new(v));
        }
        match self {
            IdList::Spill(v) => v,
            IdList::Inline { .. } => unreachable!(),
        }
    }

    /// Append `id` unless already present.
    pub(super) fn push_back(&mut self, id: LayoutWord) {
        if self.contains(id) {
            return;
        }
        self.append_unchecked(id);
    }

    /// Append an id the caller knows is not in this list.
    ///
    /// `alloc_shape_id` hands out a strictly increasing counter that is never
    /// reused (it parks at `SHAPE_ID_END` rather than wrapping), so an id that
    /// was allocated after this list was built cannot be in it, in this family
    /// or in any other. The membership scan in [`push_back`] is therefore dead
    /// work at the two interning sites, and it is not O(1) dead work: a family
    /// holds every descriptor ever created for one keys array, so the scan is
    /// linear in the history of that keys array and interning the *n*-th
    /// descriptor for it costs O(n) — quadratic over a render that keeps
    /// bumping a shape's semantic generation. `IdList::contains` was 6.2 % of
    /// main-thread leaf samples on a claude-code streamed reply, 95 % of it
    /// under `ShapeTableInner::family_push_back`.
    ///
    /// The spill index now removes that scan for the callers that cannot use
    /// this entry point, which is why `contains` is O(1) above
    /// [`SPILL_INDEX_MIN`]. This function stays because skipping the probe
    /// entirely is still cheaper than performing it.
    ///
    /// Callers that re-file an EXISTING id (the metadata rekey when a keys
    /// array moves) must keep using [`push_back`]: those ids can already be in
    /// the destination list.
    pub(super) fn append_unchecked(&mut self, id: LayoutWord) {
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids[*len as usize] = id;
                *len += 1;
            }
            _ => self.spill().push(id),
        }
    }

    /// Prepend `id` unless already present. Order-preserving by definition, so
    /// it stays O(n) on a spilled list; only `by_facts` and the external-id
    /// install use it, and neither is on a hot path.
    pub(super) fn push_front(&mut self, id: LayoutWord) {
        if self.contains(id) {
            return;
        }
        match self {
            IdList::Inline { len, ids } if (*len as usize) < ids.len() => {
                ids.copy_within(0..*len as usize, 1);
                ids[0] = id;
                *len += 1;
            }
            _ => {
                let v = self.spill();
                v.ids.insert(0, id);
                if v.indexed() {
                    v.pos.clear();
                }
                v.maybe_build_index();
            }
        }
    }

    /// [`Self::push_front`] for an id the caller knows is in no list: a
    /// freshly published word (rows are never reissued, see
    /// [`Self::append_unchecked`]). An empty list is the common case.
    pub(super) fn push_front_fresh(&mut self, id: LayoutWord) {
        if let IdList::Inline { len, ids } = self {
            if *len == 0 {
                ids[0] = id;
                *len = 1;
                return;
            }
        }
        self.push_front(id);
    }

    /// Drop `id` if present, PRESERVING the order of what remains; returns
    /// whether it was there. For a list whose order is load-bearing —
    /// `by_facts`, where the first entry is the canonical answer.
    pub(super) fn remove_ordered(&mut self, id: LayoutWord) -> bool {
        match self {
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_ordered(id).is_some(),
        }
    }

    /// Drop `id` if present, WITHOUT preserving order; returns whether it was
    /// there. For `families`, whose readers are set-valued (see the type doc).
    ///
    /// This is the change: on a spilled list it moves ONE element instead of
    /// the whole tail.
    pub(super) fn remove_unordered(&mut self, id: LayoutWord) -> bool {
        match self {
            // Three entries: the inline shift is a single register move and
            // there is nothing to gain from disturbing the order.
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_unordered(id).is_some(),
        }
    }

    #[inline]
    fn remove_inline(len: &mut u8, ids: &mut [LayoutWord; 3], id: LayoutWord) -> bool {
        let n = *len as usize;
        note_scan(n);
        let Some(pos) = ids[..n].iter().position(|&x| x == id) else {
            return false;
        };
        note_removal(n - 1 - pos);
        ids.copy_within(pos + 1..n, pos);
        ids[n - 1] = LayoutWord(0);
        *len -= 1;
        true
    }

    /// Replace `old` with `new` in place (keeps its position); returns
    /// whether `old` was present.
    pub(super) fn replace(&mut self, old: LayoutWord, new: LayoutWord) -> bool {
        match self {
            IdList::Inline { len, ids } => {
                let n = *len as usize;
                note_scan(n);
                match ids[..n].iter().position(|&x| x == old) {
                    Some(pos) => {
                        ids[pos] = new;
                        true
                    }
                    None => false,
                }
            }
            IdList::Spill(v) => v.replace(old, new),
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            IdList::Inline { .. } => 0,
            IdList::Spill(v) => {
                std::mem::size_of::<SpillList>()
                    + v.ids.capacity() * 4
                    // The index is the structure's memory cost and is reported
                    // rather than hidden: it exists only above SPILL_INDEX_MIN.
                    + v.pos.capacity() * (std::mem::size_of::<(u32, u32)>() + 1)
            }
        }
    }
}

#[cfg(test)]
#[path = "shapes_store_tests.rs"]
mod tests;

#[path = "shapes_store_special.rs"]
mod special;

#[path = "shapes_store_symbol_presence.rs"]
mod symbol_presence;

#[path = "shapes_store/transitions.rs"]
mod transitions;
pub(super) use transitions::integrity_facts_key;

#[cfg(test)]
#[path = "shapes_store/dense_tests.rs"]
mod dense_tests;

/// Dynamic rows per chunk: one 4 KiB page of 128-byte records.
pub(crate) const DYNAMIC_CHUNK_ROWS: u32 = 4096 / 128;
/// First dynamic row of the process: one fixed value, independent of any
/// image's static row count S (an image needs S <= this), so a dynamic
/// LayoutWord names one shape in the whole process whatever image an agent
/// runs. ILP32/Wasm Regions hold only 1 << 16 rows.
pub(crate) const DYNAMIC_BASE_ROWS: u32 = if cfg!(target_pointer_width = "64") {
    1 << 20
} else {
    1 << 14
};

/// The process-wide chunk space. Allocator state, not a cache: `next` hands
/// out never-used chunks (one atomic per 32 rows); `free` records the chunks
/// of dead agents but is never handed out (see `claim`): a word keeps naming
/// one shape for the life of the process, or the claim fails cleanly.
pub(super) struct ChunkAllocator {
    next: std::sync::atomic::AtomicU32,
    free: std::sync::Mutex<Vec<u32>>,
}
impl ChunkAllocator {
    pub(super) const fn new() -> Self {
        Self {
            next: std::sync::atomic::AtomicU32::new(0),
            free: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Claim a chunk; returns its first row. `capacity` is the Region's rows.
    pub(super) fn claim(&self, capacity: usize) -> Result<u32, PublishError> {
        use std::sync::atomic::Ordering::Relaxed;
        let fresh = self.next.try_update(Relaxed, Relaxed, |k| {
            let end =
                u64::from(DYNAMIC_BASE_ROWS) + (u64::from(k) + 1) * u64::from(DYNAMIC_CHUNK_ROWS);
            (end <= capacity as u64).then_some(k + 1)
        });
        // A freed chunk is NEVER handed out again. Process-shared memos (the
        // runtime's read-site memos, compiled PIC tokens) key on the layout
        // word, so a reused chunk would give a dead agent's word to a new
        // shape and a stale (word, slot) proof could answer for it: a wrong
        // answer is worse than the clean fatal error raised when fresh chunk
        // space is exhausted. Freed chunks stay on the list for bookkeeping;
        // sound reuse (a memo epoch or a proof that no memo names the chunk)
        // is later work.
        match fresh {
            Ok(k) => Ok(DYNAMIC_BASE_ROWS + k * DYNAMIC_CHUNK_ROWS),
            Err(_) => Err(PublishError::Capacity),
        }
    }

    pub(super) fn release(&self, first: &[u32], more: &[u32]) {
        if !first.is_empty() {
            let mut free = self.free.lock().unwrap_or_else(|e| e.into_inner());
            free.extend_from_slice(first);
            free.extend_from_slice(more);
        }
    }

    #[cfg(test)]
    pub(super) fn free_len(&self) -> usize {
        self.free.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}
pub(super) static CHUNKS: ChunkAllocator = ChunkAllocator::new();

/// The chunks one agent owns. The first few live inline, so a typical agent
/// (one or two chunks) allocates nothing for the list.
struct OwnedChunks {
    n: usize,
    first: [u32; 4],
    more: Vec<u32>,
}
impl OwnedChunks {
    const fn new() -> Self {
        Self {
            n: 0,
            first: [0; 4],
            more: Vec::new(),
        }
    }
    fn push(&mut self, start: u32) {
        if self.n < self.first.len() {
            self.first[self.n] = start;
            self.n += 1;
        } else {
            self.more.push(start);
        }
    }
    fn inline(&self) -> &[u32] {
        &self.first[..self.n]
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        self.n + self.more.len()
    }
    #[cfg(test)]
    fn clear(&mut self) {
        self.n = 0;
        self.more.clear();
    }
}

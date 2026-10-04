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
    ShapeDescriptor, ShapeObjectKind, DICTIONARY_SHAPE_ID_BASE, EXOTIC_SHAPE_ID_BASE, SHAPE_ID_BASE,
};
use std::cell::UnsafeCell;

/// Static body identity of a ConstFn slot. This is image metadata, never a
/// closure pointer or a GC edge. Entries are sorted by slot in a shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConstFnSlotInfo {
    pub slot: u8,
    pub info: u64,
}

/// Optional, record-owned extension. It has no keyed lookup: a method-site
/// hit reads only the receiver's shape and current closure slot. The learned
/// masks are not shape identity, so record copies/rekeys transfer this box.
///
/// `brands` is the sorted list of private brands (#11791) every receiver of
/// the shape carries: one scalar per class evaluation whose private methods
/// or fields were installed on it (`private_brand_id`). An identity fact like
/// `constfn_infos`; scalars, so nothing here is a GC edge.
#[derive(Debug)]
pub(super) struct ShapeExtras {
    pub(super) constfn_infos: Box<[ConstFnSlotInfo]>,
    pub(super) brands: Box<[u64]>,
    pub(super) to_nopointer: std::sync::atomic::AtomicU32,
    pub(super) to_any: std::sync::atomic::AtomicU32,
}

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

/// The table-owned record of one ShapeId. `keys` is first and 8-aligned: it
/// is the word the collector marks through and rewrites in place.
#[repr(C)]
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
    /// Low 8 bits: the `RECORD_FLAG_*` set. Bits 8-10: the `ShapeObjectKind`
    /// discriminant (codes 0-6; the store facts F-A/F-B are kinds 5 and 6).
    /// Bits 11-14: the births a keyless birth shape served while tracking its
    /// width (#10905). Bit 15: reserved (it held the answerable-by-position
    /// bit, which is now [`Self::position_bound`]'s zero). Bits 16-23: the
    /// attribute SUMMARY byte (`key_attrs::SUMMARY_*`), an identity fact.
    /// Bits 24-31: the inline width a keyless birth shape's descendants grow
    /// to (#10905). The two #10905 fields are learned facts of the record,
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
    /// Offset 40; the special-lane mask uses the former padding at 44, and
    /// `rep` follows at 48. The owned extension pointer at 56 makes the
    /// record 64 bytes; ordinary shapes keep that pointer null.
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
    /// Null for an ordinary shape. Otherwise a record-owned [`ShapeExtras`]
    /// address, stored at fixed width so the slab layout agrees on ILP32/LP64.
    extras: u64,
}

const RECORD_KIND_SHIFT: u32 = 8;
const RECORD_KIND_MASK: u32 = 0b111 << RECORD_KIND_SHIFT;
/// The largest `ShapeObjectKind::code()` (`OrdinaryNumericProof`, 6). Code 7
/// is the field's last free value. `kind_codes_round_trip` pins every kind.
const RECORD_KIND_MAX_CODE: u32 = 6;
const _: () = assert!(RECORD_KIND_MAX_CODE <= RECORD_KIND_MASK >> RECORD_KIND_SHIFT);
/// Charter step 3: the summary of the attributes the shape's keys carry —
/// what the chain store check and every per-key reader ask FIRST, so a shape
/// whose keys are all default answers without touching its keys. Derived
/// from `(keys, logical_key_count)` for a shape that publishes keys, which is
/// why folding it into identity costs no precision; a dictionary receiver's
/// shape publishes no keys and carries its private list's conservative
/// summary here instead.
const RECORD_SUMMARY_SHIFT: u32 = 16;
const RECORD_SUMMARY_MASK: u32 = 0xFF << RECORD_SUMMARY_SHIFT;
const _: () = assert!(RECORD_KIND_MASK & RECORD_SUMMARY_MASK == 0);
const _: () = assert!(RECORD_KIND_MASK & 0xFF == 0);

/// #10905 (`shapes_birth_width`): births served while tracking, bits 11-14.
/// Four bits hold every count the tracker stores (it stops at
/// `TRACKING_BIRTHS`, asserted below).
const RECORD_BIRTHS_SHIFT: u32 = 11;
const RECORD_BIRTHS_MASK: u32 = 0xF << RECORD_BIRTHS_SHIFT;
/// #10905 (`shapes_birth_width`): the learned descendant width, bits 24-31.
const RECORD_WIDTH_SHIFT: u32 = 24;
const RECORD_WIDTH_MASK: u32 = 0xFF << RECORD_WIDTH_SHIFT;
// The fields of `flags_and_kind` are pairwise disjoint.
const _: () = {
    let fields = [
        0xFF,
        RECORD_KIND_MASK,
        RECORD_BIRTHS_MASK,
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
const _: () = assert!(
    super::shapes_birth_width::TRACKING_BIRTHS <= RECORD_BIRTHS_MASK >> RECORD_BIRTHS_SHIFT
);

const _: () = assert!(std::mem::size_of::<ShapeRecord>() == 64);
const _: () = assert!(std::mem::align_of::<ShapeRecord>() == 8);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, position_bound) == 40);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, special_constfn_mask) == 44);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, rep) == 48);
const _: () = assert!(std::mem::offset_of!(ShapeRecord, extras) == 56);

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
        self.flags_and_kind = (self.flags_and_kind & !RECORD_SUMMARY_MASK)
            | (u32::from(summary) << RECORD_SUMMARY_SHIFT);
        // The summary is an input of the positional bit (an accessor key).
        self.refresh_positional();
        self
    }

    /// The inline width this keyless birth shape's descendants grow to
    /// (#10905), or 0 when nothing was learned.
    #[inline]
    pub(super) fn descendant_width(&self) -> u32 {
        (self.flags_and_kind & RECORD_WIDTH_MASK) >> RECORD_WIDTH_SHIFT
    }

    /// Raise [`ShapeRecord::descendant_width`] to `width` (monotone,
    /// saturating at the byte).
    #[inline]
    pub(super) fn note_descendant_width(&mut self, width: u32) {
        let width = width.min(RECORD_WIDTH_MASK >> RECORD_WIDTH_SHIFT);
        if width > self.descendant_width() {
            self.flags_and_kind =
                (self.flags_and_kind & !RECORD_WIDTH_MASK) | (width << RECORD_WIDTH_SHIFT);
        }
    }

    /// Births this keyless birth shape served while tracking (#10905).
    #[inline]
    pub(super) fn tracked_births(&self) -> u32 {
        (self.flags_and_kind & RECORD_BIRTHS_MASK) >> RECORD_BIRTHS_SHIFT
    }

    #[inline]
    pub(super) fn set_tracked_births(&mut self, births: u32) {
        let births = births.min(RECORD_BIRTHS_MASK >> RECORD_BIRTHS_SHIFT);
        self.flags_and_kind =
            (self.flags_and_kind & !RECORD_BIRTHS_MASK) | (births << RECORD_BIRTHS_SHIFT);
    }

    #[inline]
    pub(super) fn object_kind(&self) -> ShapeObjectKind {
        match (self.flags_and_kind & RECORD_KIND_MASK) >> RECORD_KIND_SHIFT {
            1 => ShapeObjectKind::Class,
            2 => ShapeObjectKind::Dictionary,
            3 => ShapeObjectKind::Function,
            4 => ShapeObjectKind::FunctionDictionary,
            5 => ShapeObjectKind::OrdinaryUnmarked,
            6 => ShapeObjectKind::OrdinaryNumericProof,
            _ => ShapeObjectKind::Ordinary,
        }
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
        let kind_bits = (object_kind.code() as u32) << RECORD_KIND_SHIFT;
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
    pub(super) fn position_bound_by_facts(&self) -> u32 {
        if !self.positional_by_facts() {
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
            && self.semantic_generation == 0
            && self.hole_count == 0
            && self.keys != 0
            && self.summary() & crate::object::key_attrs::SUMMARY_ACCESSOR == 0
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

    /// Install a SPECIAL rep with its exact ConstFn body identities. A `11`
    /// lane absent from `infos` is reserved for optional NoPointer. The old
    /// `with_rep` keeps rejecting `11`, so no legacy mint silently omits the
    /// identity fact. Called only after the interner misses.
    #[inline(always)]
    pub(super) fn with_special_facts(
        mut self,
        rep: u64,
        infos: &[ConstFnSlotInfo],
        brands: &[u64],
    ) -> ShapeRecord {
        assert_eq!(self.extras, 0, "special facts replace a fresh record only");
        let mask = constfn_mask(infos).expect("invalid ConstFn slot list");
        assert!(crate::object::field_rep::is_valid_with_special(rep, mask));
        assert_eq!(mask & !crate::object::field_rep::special_lane_slots(rep), 0);
        assert!(
            brands_are_sorted(brands),
            "a brand list is sorted and unique"
        );
        self.rep = rep;
        self.special_constfn_mask = mask;
        if !infos.is_empty() || !brands.is_empty() {
            self.extras = new_extras(infos, brands);
        }
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
        if self.extras == 0 {
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
        if self.extras == 0 {
            &[]
        } else {
            // SAFETY: the live slab record owns this allocation.
            unsafe { &(*(self.extras as usize as *const ShapeExtras)).brands }
        }
    }

    #[inline]
    pub(crate) fn deprecation_targets(&self) -> (u32, u32) {
        if self.extras == 0 {
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
        assert_ne!(self.extras, 0);
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
        if self.extras != 0 {
            drop(Box::from_raw(self.extras as usize as *mut ShapeExtras));
        }
    }

    /// The same record for a receiver whose [[Prototype]] identity is
    /// `proto_id` (see [`ShapeRecord::proto_id`]).
    #[inline(always)]
    pub(super) fn with_proto_id(mut self, proto_id: u64) -> ShapeRecord {
        self.proto_id = proto_id;
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
            && self.constfn_infos() == infos
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
            proto_id: self.proto_id,
            object_kind: self.object_kind(),
            hole_count: self.hole_count,
            summary: self.summary(),
            rep: self.rep,
            special_constfn_mask: self.special_constfn_mask,
            extras: self.extras,
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
type ChunkCells = [UnsafeCell<ShapeRecord>; CHUNK_LEN];

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
    SharedEmpty([const { UnsafeCell::new(ShapeRecord::EMPTY) }; CHUNK_LEN]);
static EMPTY_PAGE: SharedEmpty<PageSlots> = SharedEmpty(
    // SAFETY: the address of a static is never null.
    [Slot(unsafe {
        std::ptr::NonNull::new_unchecked(std::ptr::addr_of!(EMPTY_CHUNK.0) as *mut ChunkCells)
    }); PAGE_LEN],
);
/// The directory entry a lookup reads for a page index past its band's
/// directory: the shared empty page. Lets [`ShapeSlab::agent_record`] turn
/// the bounds test into a select instead of an early return.
static EMPTY_PAGE_ENTRY: SharedEmpty<Page> = SharedEmpty(Slot(
    // SAFETY: the address of a static is never null.
    unsafe { std::ptr::NonNull::new_unchecked(std::ptr::addr_of!(EMPTY_PAGE.0) as *mut PageSlots) },
));

trait Level: Sized + 'static {
    fn shared() -> std::ptr::NonNull<Self>;
    fn fresh() -> Box<Self>;
}

impl Level for ChunkCells {
    fn shared() -> std::ptr::NonNull<Self> {
        std::ptr::NonNull::from(&EMPTY_CHUNK.0)
    }
    fn fresh() -> Box<Self> {
        let mut v: Vec<UnsafeCell<ShapeRecord>> = Vec::with_capacity(CHUNK_LEN);
        v.resize_with(CHUNK_LEN, || UnsafeCell::new(ShapeRecord::EMPTY));
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
const DICT_REL: u32 = DICTIONARY_SHAPE_ID_BASE - SHAPE_ID_BASE;
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
#[repr(C, align(32))]
pub(crate) struct BandDir {
    pages: std::cell::Cell<*const Page>,
    len: std::cell::Cell<usize>,
    base_rel: u32,
}

impl BandDir {
    const fn empty(band: usize) -> BandDir {
        BandDir {
            pages: std::cell::Cell::new(std::ptr::null()),
            len: std::cell::Cell::new(0),
            base_rel: BAND_BASE_REL[band],
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
/// dictionary band (`shapes::DICTIONARY_SHAPE_ID_BASE`) from its own base. One
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
            }
            self.protos.unpublish();
        }
        // Retired records release on removal; live records release at agent
        // teardown. Rekeys transfer one pointer, never duplicate ownership.
        self.for_each(|_, ptr| unsafe { (*ptr).release_extras() });
        for band in [0u8, 1, 2] {
            Self::free_dir(self.dir_mut(band));
        }
    }
}

impl ShapeSlab {
    /// A slab that publishes nothing (tests).
    pub(super) fn new() -> Self {
        ShapeSlab {
            pages: Vec::new(),
            dict_pages: Vec::new(),
            exotic_pages: Vec::new(),
            len: 0,
            agent: false,
            protos: Default::default(),
        }
    }

    /// The agent's slab: the one [`Self::agent_record`] reads. One per
    /// agent (the runtime state's shape table).
    pub(super) fn new_agent() -> Self {
        let mut slab = Self::new();
        slab.agent = true;
        slab.publish_dir();
        slab.protos.make_agent();
        slab
    }

    #[inline]
    fn id_of(band: u8, index: usize) -> u32 {
        match band {
            0 => SHAPE_ID_BASE + index as u32,
            1 => DICTIONARY_SHAPE_ID_BASE + index as u32,
            _ => EXOTIC_SHAPE_ID_BASE + index as u32,
        }
    }

    #[inline]
    fn dir(&self, band: u8) -> &Vec<Page> {
        match band {
            0 => &self.pages,
            1 => &self.dict_pages,
            _ => &self.exotic_pages,
        }
    }

    #[inline]
    fn dir_mut(&mut self, band: u8) -> &mut Vec<Page> {
        match band {
            0 => &mut self.pages,
            1 => &mut self.dict_pages,
            _ => &mut self.exotic_pages,
        }
    }

    /// `(page, chunk within page, record within chunk)` of a slab index.
    #[inline(always)]
    fn split(index: usize) -> (usize, usize, usize) {
        (
            index >> (CHUNK_SHIFT + PAGE_SHIFT),
            (index >> CHUNK_SHIFT) & PAGE_MASK,
            index & CHUNK_MASK,
        )
    }

    /// The cell `index` names in a directory of `len` pages at `pages`: two
    /// dependent loads and no test — a page past the directory is the shared
    /// empty page, and an absent page or chunk is the shared empty one.
    ///
    /// # Safety
    /// `pages` holds `len` live entries (or `len` is 0).
    #[inline(always)]
    unsafe fn walk(pages: *const Page, len: usize, index: usize) -> *mut ShapeRecord {
        let (page, chunk, slot) = Self::split(index);
        let entry: *const Page = if page < len {
            pages.add(page)
        } else {
            &EMPTY_PAGE_ENTRY.0
        };
        let page = (*entry).0.as_ref();
        let chunk = page[chunk].0.as_ref();
        chunk[slot].get()
    }

    /// Present records.
    #[inline]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The record for `id`, or `None` when the id names no descriptor in this
    /// slab. The pointer stays valid until the record is removed; a removal
    /// only ever happens through the table's own retirement paths.
    #[inline]
    pub(super) fn record_ptr(&self, id: u32) -> Option<*mut ShapeRecord> {
        let (band, index) = locate(id);
        let cell = match band {
            // SAFETY: a band's `Vec` holds `len()` live entries.
            0..=2 => unsafe {
                let dir = self.dir(band as u8);
                Self::walk(dir.as_ptr(), dir.len(), index)
            },
            _ => return None,
        };
        // The published directory is this slab's, entry for entry.
        debug_assert!(!self.agent || cell == Self::agent_record(id));
        // SAFETY: `walk` returns a cell of a live chunk of this slab or of the
        // shared empty chunk; reads are serialized by the single-threaded
        // agent discipline every other shape-table access already relies on.
        if unsafe { (*cell).present() } {
            Some(cell)
        } else {
            None
        }
    }

    /// A copy of the record for `id`.
    #[inline]
    pub(super) fn get(&self, id: u32) -> Option<ShapeRecord> {
        // SAFETY: `record_ptr` only returns a cell of a live chunk.
        self.record_ptr(id).map(|p| unsafe { *p })
    }

    /// Lift `id` to the by-value descriptor.
    #[inline]
    pub(super) fn lift(&self, id: u32) -> Option<ShapeDescriptor> {
        // SAFETY: as in `get`.
        self.record_ptr(id).map(|p| unsafe { (*p).lift(p) })
    }

    /// Install `record` under `id`, allocating the page and chunk on first
    /// touch. Returns the record it replaced, if the id was already present.
    pub(super) fn insert(&mut self, id: u32, mut record: ShapeRecord) -> Option<ShapeRecord> {
        let (band, index) = locate(id);
        assert!(band < 3, "ShapeSlab::insert: id outside the ShapeId range");
        // The id's identity kind is a fact the prototype readers trust
        // without reading this record (`shapes::SHAPE_ID_KIND_SHIFT`): a
        // plain or null kind names exactly that identity. (The other kinds
        // send a reader to this record.)
        let kind = super::shape_word_kind(id);
        assert!(
            !matches!(kind, super::SHAPE_ID_KIND_PLAIN | super::SHAPE_ID_KIND_NULL)
                || kind == super::proto_id_kind(record.proto_id),
            "ShapeSlab::insert: identity {:#x} under ShapeId {id:#x} of kind {kind}",
            record.proto_id
        );
        let band = band as u8;
        record.set(RECORD_FLAG_PRESENT, true);
        let (page, chunk, slot) = Self::split(index);
        let dir = self.dir_mut(band);
        if page >= dir.len() {
            dir.resize_with(page + 1, Slot::empty);
            self.publish_dir();
        }
        let dir = self.dir_mut(band);
        let page = dir[page].owned_or_alloc();
        let chunk = page[chunk].owned_or_alloc();
        let cell = chunk[slot].get_mut();
        let previous = cell.present().then_some(*cell);
        // A retire-and-reinsert edits facts on a removed copy: the positional
        // bit follows them.
        record.refresh_positional();
        *cell = record;
        if previous.is_none() {
            self.len += 1;
        }
        previous
    }

    /// Clear the record under `id`, returning it if it was present.
    pub(super) fn remove(&mut self, id: u32) -> Option<ShapeRecord> {
        let (band, index) = locate(id);
        if band >= 3 {
            return None;
        }
        let (page, chunk, slot) = Self::split(index);
        let chunk = self.dir_mut(band as u8).get_mut(page)?.owned_mut()?[chunk].owned_mut()?;
        let cell = chunk[slot].get_mut();
        if !cell.present() {
            return None;
        }
        let previous = *cell;
        *cell = ShapeRecord::EMPTY;
        self.len -= 1;
        Some(previous)
    }

    /// Visit every present record in id order. The callback may write
    /// through the record pointer; it must not insert or remove.
    pub(super) fn for_each(&self, mut f: impl FnMut(u32, *mut ShapeRecord)) {
        for dict in [0u8, 1, 2] {
            for (page_index, page) in self.dir(dict).iter().enumerate() {
                let Some(page) = page.owned() else {
                    continue;
                };
                for (chunk_index, chunk) in page.iter().enumerate() {
                    let Some(chunk) = chunk.owned() else {
                        continue;
                    };
                    let base = ((page_index << PAGE_SHIFT) | chunk_index) << CHUNK_SHIFT;
                    for (slot, cell) in chunk.iter().enumerate() {
                        let p = cell.get();
                        // SAFETY: live chunk, single-threaded agent.
                        if unsafe { (*p).present() } {
                            f(Self::id_of(dict, base | slot), p);
                        }
                    }
                }
            }
        }
    }

    /// Every present id, in id order.
    #[cfg(test)]
    pub(super) fn ids(&self) -> Vec<u32> {
        let mut ids = Vec::with_capacity(self.len);
        self.for_each(|id, _| ids.push(id));
        ids
    }

    /// Free chunks that hold no present record, and pages that hold no
    /// chunk. Called once per major collection, after dead-key pruning:
    /// retirement is monotonic in id order for the common workload, so the
    /// oldest chunks empty first.
    pub(super) fn release_empty_chunks(&mut self) {
        for dict in [0u8, 1, 2] {
            let dir = self.dir_mut(dict);
            for page in dir.iter_mut() {
                let Some(chunks) = page.owned_mut() else {
                    continue;
                };
                let mut live_chunks = 0usize;
                for chunk in chunks.iter_mut() {
                    let empty = chunk
                        .owned()
                        .is_some_and(|c| c.iter().all(|cell| !unsafe { (*cell.get()).present() }));
                    if empty {
                        chunk.release();
                    }
                    if !chunk.is_shared() {
                        live_chunks += 1;
                    }
                }
                if live_chunks == 0 {
                    page.release();
                }
            }
            while dir.last().is_some_and(|p| p.is_shared()) {
                dir.pop();
            }
            dir.shrink_to_fit();
        }
        self.publish_dir();
    }

    /// Publish every band's page vector into [`AGENT_SHAPE_DIR`], if this is
    /// the agent's slab. Called after every change to a page vector (a
    /// resize or shrink moves its buffer); a page or chunk allocated or
    /// released in place is visible through the published buffer already.
    fn publish_dir(&self) {
        if !self.agent {
            return;
        }
        for (band, dir) in [&self.pages, &self.dict_pages, &self.exotic_pages]
            .into_iter()
            .enumerate()
        {
            AGENT_SHAPE_DIR[band].pages.set(dir.as_ptr());
            AGENT_SHAPE_DIR[band].len.set(dir.len());
        }
        // Emitted read sites hand the ordinary entry's address to the miss
        // front from the agent's pointer block; publish it with the directory.
        crate::agent_ptrs::publish(
            crate::agent_ptrs::AGENT_PTR_SHAPE_DIR,
            Self::ordinary_dir_addr(),
        );
    }

    /// The address of THIS thread's ordinary-band directory
    /// (`AGENT_SHAPE_DIR[0]`), as an opaque pointer for
    /// [`Self::ordinary_record_in`]. Stable for the thread's life (a
    /// const-initialised `#[thread_local]` with no destructor), so the agent
    /// publishes it into its `PERRY_AGENT_PTRS` slot and emitted code hands it
    /// to the megamorphic read confirm, which then reads no thread-local at
    /// all (a runtime thread-local access is a `__tls_get_addr` call on ELF
    /// and a TLV thunk call on Darwin, which would give the stub a frame).
    #[inline]
    pub(crate) fn ordinary_dir_addr() -> *const u8 {
        &AGENT_SHAPE_DIR[0] as *const BandDir as *const u8
    }

    /// The record of ShapeId `id` in the ordinary directory at `dir` (this
    /// thread's [`Self::ordinary_dir_addr`], or `PERRY_EMPTY_SHAPE_DIR`), or
    /// `None` past its pages: the ordinary band of [`Self::agent_record`], for
    /// the megamorphic read, with no band select and no thread-local access.
    /// Any other id (dictionary, exotic, not a ShapeId) indexes past the
    /// ordinary directory, whose pages stop below the dictionary band. Inside
    /// the pages the record may be absent (the shared `ShapeRecord::EMPTY`):
    /// its position bound is 0, the confirm's answer for it anyway.
    ///
    /// The page bound is a branch here, not [`Self::walk`]'s select: the
    /// megamorphic read's ids are the receivers' own, all inside the pages,
    /// so the branch is predicted and costs less than the select's address
    /// arithmetic (lead_mega1: 138.4 instructions/read with it, 139.9 with the
    /// select).
    ///
    /// # Safety
    /// `dir` is this thread's [`Self::ordinary_dir_addr`] or
    /// `PERRY_EMPTY_SHAPE_DIR`; never null.
    #[inline(always)]
    pub(super) unsafe fn ordinary_record_in<'a>(
        dir: *const u8,
        id: u32,
    ) -> Option<&'a ShapeRecord> {
        let dir = &*(dir as *const BandDir);
        debug_assert_eq!(dir.base_rel, 0);
        let (page, chunk, slot) = Self::split(id.wrapping_sub(SHAPE_ID_BASE) as usize);
        if page >= dir.len.get() {
            return None;
        }
        // SAFETY: the published pair is the ordinary band's page vector,
        // current as of its last change; nothing here can change it. An
        // absent page or chunk is the shared empty one (`Slot`), never null.
        let page = (*dir.pages.get().add(page)).0.as_ref();
        let chunk = page[chunk].0.as_ref();
        Some(&*chunk[slot].get())
    }

    /// The record `id` names in THIS agent's slab — never null. An id that
    /// names no record here (never minted, retired, not a ShapeId at all, or
    /// read before the agent built its runtime state) reads the shared
    /// `ShapeRecord::EMPTY`: not present, position bound 0, every lane `Any`.
    /// A caller whose answer for "no record" is exactly that reads the fields
    /// with no presence test; any other caller tests `present()`.
    ///
    /// The whole lookup: one thread-local load of the band's directory, the
    /// band select, and two dependent loads (page, chunk). No `state()`.
    ///
    /// The pointer must not be written unless the record is present (the
    /// shared empty is never written), and stays valid until the record is
    /// removed.
    #[inline(always)]
    pub(super) fn agent_record(id: u32) -> *mut ShapeRecord {
        let rel = id.wrapping_sub(SHAPE_ID_BASE);
        let band = band_of(rel);
        debug_assert!(band < AGENT_SHAPE_DIR.len());
        // SAFETY: `band_of` is at most 3; the published pair is the agent slab's page vector,
        // current as of its last change; nothing between here and the read
        // changes it.
        unsafe {
            let dir = AGENT_SHAPE_DIR.get_unchecked(band);
            let index = rel.wrapping_sub(dir.base_rel) as usize;
            Self::walk(dir.pages.get(), dir.len.get(), index)
        }
    }

    /// [`Self::agent_record`], or `None` when the record is absent.
    #[inline(always)]
    pub(super) fn agent_record_present(id: u32) -> Option<*mut ShapeRecord> {
        let record = Self::agent_record(id);
        // SAFETY: `agent_record` never returns null or a dead cell.
        unsafe { (*record).present() }.then_some(record)
    }

    #[cfg(test)]
    pub(super) fn clear(&mut self) {
        for band in [0u8, 1, 2] {
            Self::free_dir(self.dir_mut(band));
        }
        self.publish_dir();
        self.len = 0;
        self.protos.reset();
    }

    /// Bytes held: the page directory, every allocated page and every
    /// allocated chunk.
    pub(super) fn estimated_bytes(&self) -> usize {
        let mut pages = 0usize;
        let mut chunks = 0usize;
        for page in self
            .pages
            .iter()
            .chain(self.dict_pages.iter())
            .chain(self.exotic_pages.iter())
            .filter_map(Slot::owned)
        {
            pages += 1;
            chunks += page.iter().filter(|c| !c.is_shared()).count();
        }
        (self.pages.capacity() + self.dict_pages.capacity() + self.exotic_pages.capacity())
            * std::mem::size_of::<Page>()
            + pages * PAGE_LEN * std::mem::size_of::<Slot<ChunkCells>>()
            + chunks * CHUNK_LEN * std::mem::size_of::<ShapeRecord>()
    }

    /// Allocated chunks (diagnostics).
    #[cfg(test)]
    pub(super) fn chunk_count(&self) -> usize {
        self.pages
            .iter()
            .chain(self.dict_pages.iter())
            .chain(self.exotic_pages.iter())
            .filter_map(Slot::owned)
            .map(|page| page.iter().filter(|c| !c.is_shared()).count())
            .sum()
    }

    /// Free every page and chunk a directory owns, and empty it.
    fn free_dir(dir: &mut Vec<Page>) {
        for page in dir.iter_mut() {
            if let Some(chunks) = page.owned_mut() {
                for chunk in chunks.iter_mut() {
                    chunk.release();
                }
            }
            page.release();
        }
        dir.clear();
    }
}

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
    ids: Vec<u32>,
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
                self.pos.insert(id, i as u32);
            }
        }
    }

    /// Position of `id`, O(1) when indexed and a counted linear scan below the
    /// threshold.
    #[inline]
    fn position(&self, id: u32) -> Option<usize> {
        if self.indexed() {
            return self.pos.get(&id).map(|&i| i as usize);
        }
        note_scan(self.ids.len());
        self.ids.iter().position(|&x| x == id)
    }

    #[inline]
    fn push(&mut self, id: u32) {
        let i = self.ids.len();
        self.ids.push(id);
        if self.indexed() {
            self.pos.insert(id, i as u32);
        } else {
            self.maybe_build_index();
        }
    }

    /// ORDER-PRESERVING removal, for a list whose order is load-bearing.
    /// O(n) in the tail by construction — that is what "preserve the order"
    /// costs — and it reindexes the shifted suffix.
    fn remove_ordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        let moved = self.ids.len() - 1 - pos;
        note_removal(moved);
        self.ids.remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            for (i, &other) in self.ids.iter().enumerate().skip(pos) {
                self.pos.insert(other, i as u32);
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
    fn remove_unordered(&mut self, id: u32) -> Option<usize> {
        let pos = self.position(id)?;
        note_removal(if pos + 1 == self.ids.len() { 0 } else { 1 });
        let last = self.ids.len() - 1;
        self.ids.swap_remove(pos);
        if self.indexed() {
            self.pos.remove(&id);
            if pos != last {
                // The element that was last now lives at `pos`.
                self.pos.insert(self.ids[pos], pos as u32);
            }
        }
        Some(pos)
    }

    #[inline]
    fn replace(&mut self, old: u32, new: u32) -> bool {
        let Some(pos) = self.position(old) else {
            return false;
        };
        self.ids[pos] = new;
        if self.indexed() {
            self.pos.remove(&old);
            self.pos.insert(new, pos as u32);
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
    Inline { len: u8, ids: [u32; 3] },
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
            ids: [0; 3],
        }
    }
}

impl IdList {
    #[inline]
    pub(super) fn as_slice(&self) -> &[u32] {
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
    pub(super) fn contains(&self, id: u32) -> bool {
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
    pub(super) fn push_back(&mut self, id: u32) {
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
    pub(super) fn append_unchecked(&mut self, id: u32) {
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
    pub(super) fn push_front(&mut self, id: u32) {
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

    /// Drop `id` if present, PRESERVING the order of what remains; returns
    /// whether it was there. For a list whose order is load-bearing —
    /// `by_facts`, where the first entry is the canonical answer.
    pub(super) fn remove_ordered(&mut self, id: u32) -> bool {
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
    pub(super) fn remove_unordered(&mut self, id: u32) -> bool {
        match self {
            // Three entries: the inline shift is a single register move and
            // there is nothing to gain from disturbing the order.
            IdList::Inline { len, ids } => Self::remove_inline(len, ids, id),
            IdList::Spill(v) => v.remove_unordered(id).is_some(),
        }
    }

    #[inline]
    fn remove_inline(len: &mut u8, ids: &mut [u32; 3], id: u32) -> bool {
        let n = *len as usize;
        note_scan(n);
        let Some(pos) = ids[..n].iter().position(|&x| x == id) else {
            return false;
        };
        note_removal(n - 1 - pos);
        ids.copy_within(pos + 1..n, pos);
        ids[n - 1] = 0;
        *len -= 1;
        true
    }

    /// Replace `old` with `new` in place (keeps its position); returns
    /// whether `old` was present.
    pub(super) fn replace(&mut self, old: u32, new: u32) -> bool {
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

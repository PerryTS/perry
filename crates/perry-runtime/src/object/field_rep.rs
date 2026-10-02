//! Charter step 5: the per-slot field representation of a shape.
//!
//! Every inline slot 0..[`REP_SLOTS`] of a shape has a representation, two
//! bits of the record's `rep` word (`shapes_store::ShapeRecord::rep`):
//!
//! | bits | name | invariant for every object carrying the shape |
//! |---|---|---|
//! | `00` | [`REP_ANY`] | the slot holds a NaN-boxed value |
//! | `01` | [`REP_F64`] | the slot holds a JS Number as raw IEEE bits: never in the tag band, any NaN canonical, an INT32 box stored as a double |
//! | `10` | [`REP_F64_DEPRECATED`] | the same invariant as `F64` for the objects that still carry the shape; the lineage has generalized the slot. A learned fact, never identity |
//! | `11` | [`REP_SPECIAL`] | a shape-owned side fact selects ConstFn (the slot is a closure of one static body) or optional NoPointer (the slot has no GC pointer) |
//!
//! A slot at or past [`REP_SLOTS`], and every spill slot, is `Any`.
//!
//! The word is an IDENTITY fact of the shape: two shapes with the same keys
//! and a different rep are different ShapeIds. [`identity`] is the part that
//! identity sees (deprecated reads as `F64`), and it is folded into the facts
//! key only when nonzero, so an all-`Any` shape keeps the key it had before
//! the word existed.

/// Slots with a representation: the same limit 4b's region word has.
pub(crate) const REP_SLOTS: u32 = 32;

pub(crate) const REP_ANY: u64 = 0b00;
pub(crate) const REP_F64: u64 = 0b01;
pub(crate) const REP_F64_DEPRECATED: u64 = 0b10;
pub(crate) const REP_SPECIAL: u64 = 0b11;

/// The low bit of every 2-bit lane.
const LANE_LOW: u64 = 0x5555_5555_5555_5555;
/// The high bit of every 2-bit lane.
const LANE_HIGH: u64 = 0xAAAA_AAAA_AAAA_AAAA;

/// The representation of `slot` in `rep`.
#[inline]
pub(crate) fn slot_rep(rep: u64, slot: u32) -> u64 {
    if slot >= REP_SLOTS {
        return REP_ANY;
    }
    (rep >> (2 * slot)) & 0b11
}

/// `rep` with `slot` set to `value` (a `REP_*` constant).
#[inline]
pub(crate) fn with_slot_rep(rep: u64, slot: u32, value: u64) -> u64 {
    debug_assert!(slot < REP_SLOTS && value <= 0b11);
    let shift = 2 * slot;
    (rep & !(0b11 << shift)) | (value << shift)
}

/// The lanes of every slot below `slot` (the whole word past it): the part
/// of a predecessor's rep a key-add at `slot` carries.
#[inline]
pub(crate) fn lanes_below(slot: u32) -> u64 {
    if slot >= REP_SLOTS {
        u64::MAX
    } else {
        (1u64 << (2 * slot)) - 1
    }
}

/// Does no lane carry the reserved `11`?
#[inline]
pub(crate) fn is_valid(rep: u64) -> bool {
    rep & (rep >> 1) & LANE_LOW == 0
}

/// One bit per `11` lane. The shape's `special_constfn_mask` names the
/// ConstFn subset; the complement is reserved for NoPointer if P5 is accepted.
/// Existing callers of [`is_valid`] still reject every special lane.
#[inline]
pub(crate) fn special_lane_slots(rep: u64) -> u32 {
    let mut lanes = rep & (rep >> 1) & LANE_LOW;
    let mut slots = 0u32;
    while lanes != 0 {
        let bit = lanes.trailing_zeros();
        slots |= 1 << (bit / 2);
        lanes &= lanes - 1;
    }
    slots
}

/// The extended representation is valid only when every ConstFn bit names a
/// `11` lane. A `11` lane without that bit is the *reserved* NoPointer state;
/// no current producer requests it. ConstFn metadata coverage is checked by
/// the shape interner, which owns that metadata.
#[inline]
pub(crate) fn is_valid_with_special(rep: u64, special_constfn_mask: u32) -> bool {
    special_constfn_mask & !special_lane_slots(rep) == 0
}

/// The part of `rep` that is shape identity: every deprecated lane (`10`)
/// reads as `F64` (`01`). The deprecated state is a learned fact of a record,
/// like the #10905 width fields, and two records that differ only in it are
/// the same identity.
#[inline]
pub(crate) fn identity(rep: u64) -> u64 {
    debug_assert!(is_valid(rep), "reserved rep lane in {rep:#x}");
    identity_with_special(rep)
}

/// Like [`identity`], preserving `11` as an identity code. The mask and the
/// static body identities of ConstFn lanes are folded separately by the shape
/// interner. This keeps every pre-SPECIAL F64 hash byte-identical.
#[inline]
pub(crate) fn identity_with_special(rep: u64) -> u64 {
    let deprecated = rep & LANE_HIGH & !(rep << 1);
    (rep & !deprecated) | (deprecated >> 1)
}

/// The slots whose lane is exactly `F64` (`01`), one bit per slot: the
/// lanes a class birth shape declares (T1) and the allocator birth-fills.
#[inline]
pub(crate) fn f64_lane_slots(rep: u64) -> u32 {
    let lanes = rep & LANE_LOW & !(rep >> 1);
    let mut slots = 0u32;
    let mut rest = lanes;
    while rest != 0 {
        let bit = rest.trailing_zeros();
        slots |= 1 << (bit / 2);
        rest &= rest - 1;
    }
    slots
}

/// Does any lane carry the deprecated `10`?
#[inline]
pub(crate) fn has_deprecated(rep: u64) -> bool {
    rep & LANE_HIGH & !(rep << 1) != 0
}

/// The representation every object carrying `rep` converges to: each
/// deprecated lane (`10`) becomes `Any`, every `F64` lane stays. This is the
/// rep of the shape a generalized lineage moves to (DESIGN §1.5).
#[inline]
pub(crate) fn normalized(rep: u64) -> u64 {
    debug_assert!(is_valid(rep), "reserved rep lane in {rep:#x}");
    rep & LANE_LOW
}

/// Normalize learned transitions without changing the identity of a carried
/// shape. `to_any` wins if a lineage first learned F64→NoPointer and later
/// observed a pointer. A special ConstFn or NoPointer lane can only go to Any.
/// With both masks zero this agrees with [`normalized`] for every old rep.
#[inline]
pub(crate) fn normalized_with_special(rep: u64, to_nopointer: u32, to_any: u32) -> u64 {
    let mut normalized = rep;
    for slot in 0..REP_SLOTS {
        let bit = 1 << slot;
        let lane = slot_rep(rep, slot);
        let next = if to_any & bit != 0 {
            REP_ANY
        } else if lane == REP_F64_DEPRECATED {
            if to_nopointer & bit != 0 {
                REP_SPECIAL
            } else {
                REP_ANY
            }
        } else {
            lane
        };
        if next != lane {
            normalized = with_slot_rep(normalized, slot, next);
        }
    }
    normalized
}

/// A structural publisher that has no static body list may keep old numeric
/// lanes but must conservatively drop ConstFn (and optional NoPointer) lanes.
/// Its result is valid for the legacy rep-only interner.
#[inline]
pub(crate) fn normalized_without_special(rep: u64) -> u64 {
    let mut ordinary = rep;
    let mut special = special_lane_slots(rep);
    while special != 0 {
        let slot = special.trailing_zeros();
        special &= special - 1;
        ordinary = with_slot_rep(ordinary, slot, REP_ANY);
    }
    normalized(ordinary)
}

/// Slots the collector may skip. ConstFn is pointer-bearing, while the
/// optional NoPointer half of `11` is not. This must not be used as a Number
/// fact by the type guard: NoPointer also admits booleans/null/undefined.
#[inline]
pub(crate) fn non_pointer_slot_bits(rep: u64, special_constfn_mask: u32) -> u32 {
    let numeric = non_any_slot_bits(rep);
    numeric | (special_lane_slots(rep) & !special_constfn_mask)
}

/// One bit per slot whose lane is `F64` or deprecated `F64`: a Number fact
/// used by the type guard. The collector also skips the optional NoPointer
/// subset of SPECIAL, via [`non_pointer_slot_bits`].
#[inline]
pub(crate) fn non_any_slot_bits(rep: u64) -> u32 {
    let mut x = (rep | (rep >> 1)) & LANE_LOW;
    x = (x | (x >> 1)) & 0x3333_3333_3333_3333;
    x = (x | (x >> 2)) & 0x0F0F_0F0F_0F0F_0F0F;
    x = (x | (x >> 4)) & 0x00FF_00FF_00FF_00FF;
    x = (x | (x >> 8)) & 0x0000_FFFF_0000_FFFF;
    x = (x | (x >> 16)) & 0x0000_0000_FFFF_FFFF;
    (x as u32) & !special_lane_slots(rep)
}

/// The bits an `F64` slot stores for the JS value `value_bits`, or `None`
/// when the value is not a JS Number (the store generalizes the slot, T4).
///
/// A Number is any double outside the tag band (NaN collapsed to the one
/// canonical NaN, ±Infinity kept) or an INT32 box (stored as its double).
/// `value_bits_to_number` is the one classifier: it also refuses a class
/// reference, which shares the INT32 tag and must keep it.
#[inline]
pub(crate) fn f64_slot_bits(value_bits: u64) -> Option<u64> {
    crate::array::value_bits_to_number(value_bits).map(f64::to_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_maps_deprecated_to_f64_and_keeps_everything_else() {
        assert_eq!(identity(0), 0);
        assert_eq!(identity(REP_F64), REP_F64);
        assert_eq!(identity(REP_F64_DEPRECATED), REP_F64);
        let mixed = with_slot_rep(with_slot_rep(0, 3, REP_F64), 31, REP_F64_DEPRECATED);
        let expected = with_slot_rep(with_slot_rep(0, 3, REP_F64), 31, REP_F64);
        assert_eq!(identity(mixed), expected);
        assert_eq!(identity(expected), expected);
    }

    #[test]
    fn slots_past_the_word_are_any() {
        let all_f64 = LANE_LOW;
        assert_eq!(slot_rep(all_f64, 0), REP_F64);
        assert_eq!(slot_rep(all_f64, REP_SLOTS - 1), REP_F64);
        assert_eq!(slot_rep(all_f64, REP_SLOTS), REP_ANY);
        assert_eq!(slot_rep(all_f64, 1000), REP_ANY);
    }

    #[test]
    fn normalizing_turns_only_deprecated_lanes_into_any() {
        let rep = with_slot_rep(with_slot_rep(0, 1, REP_F64), 5, REP_F64_DEPRECATED);
        assert!(has_deprecated(rep));
        assert_eq!(normalized(rep), with_slot_rep(0, 1, REP_F64));
        assert!(!has_deprecated(normalized(rep)));
        assert!(!has_deprecated(LANE_LOW));
    }

    #[test]
    fn a_number_is_stored_as_its_canonical_double_and_a_tag_is_refused() {
        let int32 = crate::value::INT32_TAG | 7;
        assert_eq!(f64_slot_bits(int32), Some(7.0f64.to_bits()));
        assert_eq!(f64_slot_bits(1.5f64.to_bits()), Some(1.5f64.to_bits()));
        assert_eq!(
            f64_slot_bits(f64::INFINITY.to_bits()),
            Some(f64::INFINITY.to_bits())
        );
        let negative_payload_nan = 0xFFF8_0000_0000_0001u64;
        assert_eq!(
            f64_slot_bits(negative_payload_nan),
            Some(0x7FF8_0000_0000_0000)
        );
        assert_eq!(f64_slot_bits(crate::value::TAG_UNDEFINED), None);
        assert_eq!(f64_slot_bits(crate::value::TAG_NULL), None);
    }

    #[test]
    fn non_any_slot_bits_has_one_bit_per_non_any_lane() {
        assert_eq!(non_any_slot_bits(0), 0);
        assert_eq!(non_any_slot_bits(LANE_LOW), u32::MAX);
        assert_eq!(non_any_slot_bits(LANE_HIGH), u32::MAX);
        let rep = with_slot_rep(with_slot_rep(0, 1, REP_F64), 31, REP_F64_DEPRECATED);
        assert_eq!(non_any_slot_bits(rep), (1 << 1) | (1 << 31));
    }

    #[test]
    fn the_reserved_lane_is_rejected() {
        assert!(is_valid(LANE_LOW));
        assert!(is_valid(LANE_HIGH));
        assert!(!is_valid(with_slot_rep(0, 7, REP_SPECIAL)));
    }

    #[test]
    fn special_lanes_preserve_old_f64_identity_and_separate_gc_facts() {
        let old = with_slot_rep(0, 2, REP_F64_DEPRECATED);
        assert_eq!(identity_with_special(old), identity(old));
        assert_eq!(normalized_with_special(old, 0, 0), normalized(old));
        let rep = with_slot_rep(with_slot_rep(old, 5, REP_SPECIAL), 8, REP_SPECIAL);
        let constfn = 1 << 5;
        assert_eq!(special_lane_slots(rep), constfn | (1 << 8));
        assert!(is_valid_with_special(rep, constfn));
        assert!(!is_valid_with_special(rep, constfn | (1 << 7)));
        assert_eq!(non_any_slot_bits(rep), 1 << 2);
        assert_eq!(non_pointer_slot_bits(rep, constfn), (1 << 2) | (1 << 8));
        assert_eq!(slot_rep(identity_with_special(rep), 5), REP_SPECIAL);
        assert_eq!(normalized_without_special(rep), 0);
    }

    #[test]
    fn learned_target_can_escalate_from_nopointer_to_any() {
        let old = with_slot_rep(0, 3, REP_F64_DEPRECATED);
        assert_eq!(
            slot_rep(normalized_with_special(old, 1 << 3, 0), 3),
            REP_SPECIAL
        );
        assert_eq!(
            slot_rep(normalized_with_special(old, 1 << 3, 1 << 3), 3),
            REP_ANY
        );
        let special = with_slot_rep(0, 3, REP_SPECIAL);
        assert_eq!(
            slot_rep(normalized_with_special(special, 0, 1 << 3), 3),
            REP_ANY
        );
    }
}

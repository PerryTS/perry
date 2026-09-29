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
//! | `11` | [`REP_RESERVED`] | kept free for a later `I32` representation; nothing produces it |
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
#[cfg(test)] // nothing produces it (the store check rejects it in debug)
pub(crate) const REP_RESERVED: u64 = 0b11;

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

/// Does no lane carry the reserved `11`?
#[inline]
pub(crate) fn is_valid(rep: u64) -> bool {
    rep & (rep >> 1) & LANE_LOW == 0
}

/// The part of `rep` that is shape identity: every deprecated lane (`10`)
/// reads as `F64` (`01`). The deprecated state is a learned fact of a record,
/// like the #10905 width fields, and two records that differ only in it are
/// the same identity.
#[inline]
pub(crate) fn identity(rep: u64) -> u64 {
    debug_assert!(is_valid(rep), "reserved rep lane in {rep:#x}");
    let deprecated = rep & LANE_HIGH & !(rep << 1);
    (rep & !deprecated) | (deprecated >> 1)
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
    fn the_reserved_lane_is_rejected() {
        assert!(is_valid(LANE_LOW));
        assert!(is_valid(LANE_HIGH));
        assert!(!is_valid(with_slot_rep(0, 7, REP_RESERVED)));
    }
}

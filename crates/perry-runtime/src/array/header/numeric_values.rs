#[inline]
pub(crate) fn value_bits_are_numeric(value_bits: u64) -> bool {
    value_bits_to_number(value_bits).is_some()
}

#[inline]
pub(crate) fn value_bits_to_number(value_bits: u64) -> Option<f64> {
    // An I32 slot word (`layout_slot`): this also classifies raw slot words,
    // and an at-rest Array's slots hold them. No JS value is one (P3 B4b);
    // every other internal word is refused by the tag-band check below.
    if crate::layout_slot::is_i32_word(value_bits) {
        return Some((value_bits as u32 as i32) as f64);
    }
    let upper = value_bits >> 48;
    if (0x7FF9..=0x7FFF).contains(&upper) || upper == (crate::value::HANDLE_TAG >> 48) {
        return None;
    }
    Some(canonical_raw_f64(f64::from_bits(value_bits)))
}

/// Collapse ANY NaN to the single canonical quiet NaN.
///
/// #10779: Perry's tag band is the positive qNaN range `0x7FF8..=0x7FFF`, so a
/// genuine IEEE-754 `f64` NaN whose high mantissa nibble is >= 8 is
/// bit-indistinguishable from a NaN-boxed string / pointer / int32 / singleton.
/// The band cannot be moved — every 64-bit pattern with `exp == 0x7FF` and a
/// non-zero mantissa is a NaN some program may legitimately store — so the
/// invariant has to be established at the SOURCE: the only NaN allowed to enter
/// a NaN-boxed slot is this one.
///
/// Collapsing EVERY NaN (not only the ones already inside the band) is load
/// bearing, and two hardware behaviours are why:
///
/// * a **signalling** NaN quiets under arithmetic by setting mantissa bit 51,
///   so `0x7FF7_0000_FFFF_FFFF * 1` becomes `0x7FFF_…` — a forged
///   `StringHeader*`. Every positive sNaN in `0x7FF1..=0x7FF7` maps into the
///   band this way.
/// * `fneg` / `fabs` clear the sign bit, so a NEGATIVE payload NaN such as
///   `0xFFFE_0000_1234_5678` becomes `0x7FFE_…` — a forged int32 — under `-x`
///   or `Math.abs(x)`.
///
/// With every source canonicalised the only NaN in circulation is
/// `0x7FF8_0000_0000_0000`; quieting it is a no-op and negating it gives
/// `0xFFF8_…`, both outside the band. The property then holds inductively,
/// which is exactly the contract
/// `perry-codegen::type_analysis::expr_produces_canonical_raw_f64` documents.
#[inline]
pub(crate) fn canonical_raw_f64(value: f64) -> f64 {
    if value.is_nan() {
        f64::NAN
    } else {
        value
    }
}

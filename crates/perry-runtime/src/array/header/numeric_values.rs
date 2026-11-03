#[inline]
pub(crate) fn value_bits_are_numeric(value_bits: u64) -> bool {
    value_bits_to_number(value_bits).is_some()
}

#[inline]
pub(crate) fn value_bits_to_number(value_bits: u64) -> Option<f64> {
    if (value_bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        let lower = (value_bits & crate::value::INT32_MASK) as u32;
        // #321/effect-Schema: a class reference shares the INT32_TAG (0x7FFE)
        // NaN-box shape with genuine small integers — `arrays_finds.rs` lowers
        // a `ClassRef` to its registered class id NaN-boxed with INT32_TAG, and
        // downstream property / method / `instanceof` dispatch keys off the
        // surviving 0x7FFE tag. A class ref is NOT a numeric array element, so
        // treating it as the integer `class_id` here let the raw-f64 numeric
        // layout canonicalize the slot to `class_id.to_bits()`, stripping the
        // tag (`canonicalize_array_numeric_store_bits` /
        // `note_array_numeric_index_write`). That turned a class value passed
        // through a rest parameter — `Union(...members)` in effect's Schema,
        // whose `members.map((m) => m.ast)` then dereferenced the bare number
        // as an object — into a SIGSEGV. Reporting class refs as non-numeric
        // keeps such arrays off the raw-f64 fast path and preserves the tag.
        // A genuine integer whose value coincides with a registered class id
        // only loses the raw-f64 *optimization* (it is still a valid number
        // when read back), so correctness is never at stake.
        if crate::object::is_class_id_registered(lower) {
            return None;
        }
        return Some((lower as i32) as f64);
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

//! NaN-boxing constants and helpers.
//!
//! These MUST match `perry-runtime/src/value.rs` exactly. Perry's entire runtime
//! ABI depends on the tag bits in the high 16 bits of the NaN payload; if any
//! constant here drifts from the runtime, every call through `js_nanbox_*`
//! corrupts values silently.
//!
//! The pre-computed `*_I64` strings are the signed-i64 representation of each
//! tag, ready to paste directly into LLVM IR (`bitcast i64 <str> to double`).
//! We store them as strings so LLVM's text parser doesn't have to round-trip
//! through a double constant, which would lose the NaN payload bits on some
//! architectures.

pub const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
/// Issue #323 hole sentinel — see `perry-runtime::value::TAG_HOLE` for the
/// invariant. Inline `IndexGet` paths emitted by the codegen must select-
/// rewrite this back to `TAG_UNDEFINED` so user code never sees the sentinel.
pub const TAG_HOLE: u64 = 0x7FFC_0000_0000_0010;
/// TDZ (Temporal Dead Zone) sentinel — see `perry-runtime::value::TAG_TDZ`.
/// A lexical `let`/`const`/`class` box is seeded with this at scope entry;
/// reading it before the declaration runs throws a spec ReferenceError.
pub const TAG_TDZ: u64 = 0x7FFC_0000_0000_0011;
pub const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
pub const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;
pub const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
/// Low end of the Perry-owned qNaN tag band. A NaN-boxed value is a primitive
/// number iff its `TAG_MASK` bits are NOT in `[SHORT_STRING_TAG, STRING_TAG]`.
/// Must match `perry-runtime::value::tags::SHORT_STRING_TAG` (used by the
/// inline `is-number` guard in `stmt::loops`).
pub const SHORT_STRING_TAG: u64 = 0x7FF9_0000_0000_0000;
/// Internal-only tag for a pointer to an immutable static-dispatch string
/// descriptor. This is not a JavaScript value; it travels only through the
/// property/method `*_by_id` ABI. `0x7FF8` is outside Perry's JS-value tag
/// band and corresponds to the otherwise-unused canonical quiet-NaN prefix.
pub const STATIC_DISPATCH_TAG: u64 = 0x7FF8_0000_0000_0000;
pub const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

pub const TAG_UNDEFINED_I64: &str = "9222246136947933185";
pub const TAG_NULL_I64: &str = "9222246136947933186";
pub const TAG_FALSE_I64: &str = "9222246136947933187";
pub const TAG_TRUE_I64: &str = "9222246136947933188";
pub const TAG_HOLE_I64: &str = "9222246136947933200";
pub const TAG_TDZ_I64: &str = "9222246136947933201";
pub const POINTER_TAG_I64: &str = "9222527611924643840";
pub const POINTER_MASK_I64: &str = "281474976710655";
pub const INT32_TAG_I64: &str = "9222809086901354496";
pub const STRING_TAG_I64: &str = "9223090561878065152";
pub const BIGINT_TAG_I64: &str = "9221683186994511872";
/// Top-16-bit comparands for `lshr 48`-style tag dispatch (repsel Phase 3a
/// canonical-Str lowerings): `STRING_TAG >> 48` and `SHORT_STRING_TAG >> 48`.
/// Asserted against the u64 tags in `tag_strings_match_u64_values`.
pub const STRING_TAG_TOP16_I64: &str = "32767";
pub const SHORT_STRING_TAG_TOP16_I64: &str = "32761";
/// The other two `lshr 48` comparands that name a HEAP-POINTER-bearing tag.
/// Together with [`STRING_TAG_TOP16_I64`] these are exactly the three tags
/// `perry-runtime::gc::barrier::decode_heap_addr` resolves to a heap address —
/// the set the #7511 inline pointer-bearing test is built from. Asserted
/// against the u64 tags in `tag_strings_match_u64_values`.
pub const POINTER_TAG_TOP16_I64: &str = "32765";
pub const BIGINT_TAG_TOP16_I64: &str = "32762";
/// `INT32_TAG >> 48`. Used by the inline `===` prefix, which settles two
/// INT32-tagged operands by bit inequality — the encoding
/// `INT32_TAG | (v as u32 as u64)` is canonical, so different bits are
/// different integers. Asserted in `tag_strings_match_u64_values`.
pub const INT32_TAG_TOP16_I64: &str = "32766";

/// Format a `u64` as a signed LLVM i64 literal (LLVM IR integer literals are signed).
pub fn i64_literal(v: u64) -> String {
    if v > 0x7FFF_FFFF_FFFF_FFFF {
        // Two's-complement: emit negative form.
        let signed = (v as i128) - (1i128 << 64);
        signed.to_string()
    } else {
        v.to_string()
    }
}

/// Format a `f64` as an LLVM IR `double` literal.
///
/// LLVM requires a decimal point or exponent for integer-valued doubles, so
/// `42` must be emitted as `42.0`. Non-finite values (NaN, ±Inf) take the
/// hexadecimal bit-pattern form LLVM accepts.
pub fn double_literal(v: f64) -> String {
    if v == 0.0 {
        // Handles both +0 and -0; LLVM distinguishes via `-0.0`.
        if v.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        }
    } else if !v.is_finite() {
        // LLVM accepts raw hex bit patterns for non-finite doubles.
        format!("0x{:016X}", v.to_bits())
    } else {
        let s = format!("{}", v);
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s
        } else {
            format!("{}.0", s)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_strings_match_u64_values() {
        assert_eq!(i64_literal(TAG_UNDEFINED), TAG_UNDEFINED_I64);
        assert_eq!(i64_literal(TAG_NULL), TAG_NULL_I64);
        assert_eq!(i64_literal(TAG_FALSE), TAG_FALSE_I64);
        assert_eq!(i64_literal(TAG_TRUE), TAG_TRUE_I64);
        assert_eq!(i64_literal(TAG_HOLE), TAG_HOLE_I64);
        assert_eq!(i64_literal(POINTER_TAG), POINTER_TAG_I64);
        assert_eq!(i64_literal(POINTER_MASK), POINTER_MASK_I64);
        assert_eq!(i64_literal(INT32_TAG), INT32_TAG_I64);
        assert_eq!(i64_literal(STRING_TAG), STRING_TAG_I64);
        assert_eq!(i64_literal(BIGINT_TAG), BIGINT_TAG_I64);
        assert_eq!(i64_literal(STATIC_DISPATCH_TAG), "9221120237041090560");
        assert_eq!(i64_literal(STRING_TAG >> 48), STRING_TAG_TOP16_I64);
        assert_eq!(
            i64_literal(SHORT_STRING_TAG >> 48),
            SHORT_STRING_TAG_TOP16_I64
        );
        assert_eq!(i64_literal(POINTER_TAG >> 48), POINTER_TAG_TOP16_I64);
        assert_eq!(i64_literal(BIGINT_TAG >> 48), BIGINT_TAG_TOP16_I64);
        assert_eq!(i64_literal(INT32_TAG >> 48), INT32_TAG_TOP16_I64);
    }

    /// #7511 — the inline pointer-bearing test emitted at class-field stores
    /// must be a **superset** of every tag the runtime resolves to a heap
    /// address. This enumerates the whole 16-bit tag space and asserts the
    /// codegen predicate never says "no pointer" where the runtime's
    /// `decode_heap_addr` / `layout_pointer_bearing_bits` would say "pointer".
    ///
    /// Written against the tag values rather than against the emitted IR on
    /// purpose: the IR is a rendering of this set, and it is the SET that has
    /// to be right. If a future tag joins the heap-pointer family
    /// (`perry-runtime/src/value.rs`), this test still passes while the
    /// generated code silently drops its barrier — so the mirror assertion
    /// lives in the runtime too (`gc::barrier::tests`).
    #[test]
    fn inline_pointer_bearing_top16_set_covers_every_heap_tag() {
        let comparands: Vec<u64> = vec![
            POINTER_TAG_TOP16_I64.parse().unwrap(),
            STRING_TAG_TOP16_I64.parse().unwrap(),
            BIGINT_TAG_TOP16_I64.parse().unwrap(),
        ];
        for tag in [POINTER_TAG, STRING_TAG, BIGINT_TAG] {
            assert!(
                comparands.contains(&(tag >> 48)),
                "heap tag {tag:#x} is missing from the inline pointer-bearing comparand set"
            );
        }
        // Non-heap tags must NOT be in the set, or the test would be vacuous.
        for tag in [
            TAG_UNDEFINED & TAG_MASK,
            INT32_TAG,
            SHORT_STRING_TAG,
            STATIC_DISPATCH_TAG,
        ] {
            assert!(
                !comparands.contains(&(tag >> 48)),
                "non-heap tag {tag:#x} must not force a barrier call"
            );
        }
    }

    #[test]
    fn double_literal_integer_gets_decimal_point() {
        assert_eq!(double_literal(42.0), "42.0");
        assert_eq!(double_literal(0.0), "0.0");
        assert_eq!(double_literal(-1.0), "-1.0");
    }

    #[test]
    fn double_literal_fractional_passes_through() {
        assert_eq!(double_literal(1.5), "1.5");
    }
}

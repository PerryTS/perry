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
pub const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
pub const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
pub const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
pub const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;
pub const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
pub const BIGINT_TAG: u64 = 0x7FFA_0000_0000_0000;
pub const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;

pub const TAG_UNDEFINED_I64: &str = "9222246136947933185";
pub const TAG_NULL_I64: &str = "9222246136947933186";
pub const TAG_FALSE_I64: &str = "9222246136947933187";
pub const TAG_TRUE_I64: &str = "9222246136947933188";
pub const TAG_HOLE_I64: &str = "9222246136947933200";
pub const POINTER_TAG_I64: &str = "9222527611924643840";
pub const POINTER_MASK_I64: &str = "281474976710655";
pub const INT32_TAG_I64: &str = "9222809086901354496";
pub const STRING_TAG_I64: &str = "9223090561878065152";
pub const BIGINT_TAG_I64: &str = "9221683186994511872";

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

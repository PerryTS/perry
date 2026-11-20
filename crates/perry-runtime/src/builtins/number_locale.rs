//! Default Number.prototype.toLocaleString formatting.
use crate::string::alloc_runtime_string;

/// `(n).toLocaleString()` for numeric receivers (#600). Formats with
/// thousands separators in en-US — `(12345).toLocaleString() === "12,345"`.
/// The HIR lowers `n.toLocaleString()` on any receiver to
/// `Expr::DateToLocaleString(n)` (the variant predates numeric
/// support), so the LLVM codegen routes statically-Number receivers
/// here and Date receivers to `js_date_to_locale_string` below. Decimal
/// part follows JS spec: trailing zeros after the decimal stripped,
/// leading sign preserved.
///
/// The non-finite and signed-zero spellings are ECMA-402's, not Rust's
/// (#9452): the operation is defined as "format with a default
/// `Intl.NumberFormat`", whose number pattern renders the infinities with
/// U+221E ("∞" / "-∞") and keeps the sign of negative zero ("-0"). `NaN`
/// stays the literal "NaN" in both. Before #9452 this returned Rust's
/// `Display` spelling "Infinity"/"-Infinity" and dropped the sign of `-0`,
/// so `x.toLocaleString()` disagreed with `x.toLocaleString("en-US")` —
/// which since #9448 goes through the real formatter (see
/// `intl::number_format`, whose sign test this now mirrors).
#[no_mangle]
pub extern "C" fn js_number_to_locale_string(n: f64) -> *mut crate::StringHeader {
    if n.is_nan() {
        return alloc_runtime_string("NaN");
    }
    if n.is_infinite() {
        return alloc_runtime_string(if n > 0.0 { "\u{221e}" } else { "-\u{221e}" });
    }
    // `-0.0 < 0.0` is false, so the sign of a negative zero has to be read
    // off the bit pattern — the same test `intl::number_format` applies.
    let negative = n < 0.0 || (n == 0.0 && n.is_sign_negative());
    let abs = n.abs();
    // Split into integer and decimal parts. JS's default
    // `Number.prototype.toLocaleString()` shows up to 3 fraction digits
    // (Intl.NumberFormat default `maximumFractionDigits`), trailing
    // zeros stripped.
    let frac = abs - abs.trunc();
    // Format integer part with comma every 3 digits (en-US). #6127: derive the
    // digits from the f64's shortest round-trip (Rust `{}` is always positional,
    // never scientific) rather than an exact `as u64` cast — the cast both
    // overflows past ~1.8e19 and, in [2^53, 2^64), prints more digits than the
    // shortest decimal V8 groups (`2**60` → `…847,000`, not `…846,976`).
    let int_str = format!("{}", abs.trunc());
    let mut grouped = String::new();
    let bytes = int_str.as_bytes();
    let len = bytes.len();
    for (i, &b) in bytes.iter().enumerate() {
        let from_end = len - i;
        grouped.push(b as char);
        if from_end > 1 && from_end % 3 == 1 {
            grouped.push(',');
        }
    }
    let mut out = if negative {
        format!("-{}", grouped)
    } else {
        grouped
    };
    if frac != 0.0 {
        // 3 fraction digits, trailing zeros stripped.
        let frac_str = format!("{:.3}", frac);
        // frac_str = "0.xxx" — keep "xxx", strip trailing zeros.
        let frac_digits = frac_str.trim_start_matches("0.").trim_end_matches('0');
        if !frac_digits.is_empty() {
            out.push('.');
            out.push_str(frac_digits);
        }
    }
    alloc_runtime_string(&out)
}

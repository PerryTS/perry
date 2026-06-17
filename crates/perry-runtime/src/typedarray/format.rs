//! Node-style display formatting for typed arrays (`console.log` / `join`).

use super::bigint::bigint_slot_bits;
use super::{
    clean_ta_ptr, load_at, name_for_kind, TypedArrayHeader, KIND_BIGINT64, KIND_BIGUINT64,
    KIND_FLOAT16, KIND_FLOAT32, KIND_FLOAT64,
};

/// Format a typed array Node-style: `Int32Array(N) [ a, b, c ]`. Used by
/// `format_jsvalue` in builtins.rs.
pub fn format_typed_array(ta: *const TypedArrayHeader) -> String {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return "TypedArray(0) []".to_string();
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as usize;
        let name = name_for_kind(kind);
        if len == 0 {
            return format!("{}(0) []", name);
        }
        let mut s = format!("{}({}) [", name, len);
        for i in 0..len {
            if i == 0 {
                s.push(' ');
            } else {
                s.push_str(", ");
            }
            let v = load_at(ta, i);
            s.push_str(&format_typed_value(kind, v, true));
        }
        s.push_str(" ]");
        s
    }
}

/// Format a single typed-array element. `bigint_suffix` controls whether a
/// `BigInt64`/`BigUint64` element renders with the trailing `n` (true for the
/// `console.log` inspect form `BigInt64Array(1) [ 5n ]`, false for `join`,
/// which calls plain `ToString` on each element → `"5"`).
pub(super) fn format_typed_value(kind: u8, v: f64, bigint_suffix: bool) -> String {
    match kind {
        KIND_BIGINT64 => {
            let n = bigint_slot_bits(v) as i64;
            if bigint_suffix {
                format!("{n}n")
            } else {
                format!("{n}")
            }
        }
        KIND_BIGUINT64 => {
            let n = bigint_slot_bits(v);
            if bigint_suffix {
                format!("{n}n")
            } else {
                format!("{n}")
            }
        }
        KIND_FLOAT16 | KIND_FLOAT32 | KIND_FLOAT64 => {
            // Match Node: integer-valued floats render with no decimal,
            // others render via Rust's default Debug for f64.
            if v.is_nan() {
                "NaN".to_string()
            } else if v.is_infinite() {
                if v > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if v == v.trunc() && v.abs() < 1e16 {
                format!("{}", v as i64)
            } else {
                // Use Rust's default short formatting.
                let s = format!("{}", v);
                s
            }
        }
        _ => {
            // Integer types
            format!("{}", v as i64)
        }
    }
}

//! `util.toUSVString(value)` (#2514) — coerce `value` to a string, then replace
//! each lone surrogate code unit with the Unicode replacement character U+FFFD.
//!
//! Perry stores lone surrogates (U+D800..U+DFFF) as WTF-8 bytes. Decoding those
//! bytes back to a Rust `String` via `from_utf8_lossy` substitutes U+FFFD for
//! each ill-formed sequence — which is exactly the USV-string scrubbing Node
//! performs. So `ToString(value)` followed by a lossy round-trip yields Node's
//! `toUSVString` result. Unlike `stripVTControlCharacters`, this coerces rather
//! than throwing on non-string input (`toUSVString(123) === "123"`).

use crate::url::{create_string_f64, string_from_header};

/// `util.toUSVString(value)` → string with lone surrogates replaced by U+FFFD.
#[no_mangle]
pub extern "C" fn js_util_to_usv_string(value: f64) -> f64 {
    // ToString(value): reuse the runtime's canonical coercion.
    let str_ptr = crate::value::js_jsvalue_to_string(value);
    // string_from_header decodes WTF-8 → String via from_utf8_lossy, mapping
    // every lone surrogate to U+FFFD; re-intern the scrubbed bytes.
    let scrubbed = string_from_header(str_ptr);
    create_string_f64(&scrubbed)
}

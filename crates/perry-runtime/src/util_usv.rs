//! `util.toUSVString(value)` (#2514) — coerce `value` to a string, then replace
//! each lone surrogate code unit with the Unicode replacement character U+FFFD.
//!
//! This is `ToString(value)` followed by the same lone-surrogate scrubbing that
//! `String.prototype.toWellFormed` performs: Perry stores lone surrogates
//! (U+D800..U+DFFF) as 3-byte WTF-8 sequences, and `js_string_to_well_formed`
//! replaces each *whole* sequence with one U+FFFD (a byte-wise `from_utf8_lossy`
//! would instead emit three). Unlike `stripVTControlCharacters`, `toUSVString`
//! coerces rather than throwing on non-string input (`toUSVString(123) === "123"`).

/// `util.toUSVString(value)` → string with lone surrogates replaced by U+FFFD.
#[no_mangle]
pub extern "C" fn js_util_to_usv_string(value: f64) -> f64 {
    // ToString(value): reuse the runtime's canonical coercion (-> *mut StringHeader).
    let str_ptr = crate::value::js_jsvalue_to_string(value);
    // Replace each lone surrogate with a single U+FFFD (WTF-8-aware).
    let well_formed = crate::string::js_string_to_well_formed(str_ptr);
    f64::from_bits(crate::value::JSValue::string_ptr(well_formed).bits())
}

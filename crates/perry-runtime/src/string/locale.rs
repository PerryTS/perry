//! Locale-aware String methods: `toLocaleLowerCase` / `toLocaleUpperCase`
//! and the `locales`-arg validation shared with `localeCompare`.
//!
//! Perry does not ship a full ICU/`Intl` collator, so locale-sensitive
//! *collation* ordering (e.g. German vs. Swedish placement of `ä`) is still
//! deferred (tracked by the umbrella Intl work). What this module DOES match
//! against Node:
//!
//!   * BCP 47 language-tag validation — an invalid `locales` argument throws a
//!     `RangeError: Invalid language tag: <tag>`, just like V8.
//!   * High-impact locale-specific casing: the Turkish/Azeri (`tr`/`az`)
//!     dotted/dotless `I` rules, where `"I".toLocaleLowerCase("tr") === "ı"`
//!     and `"i".toLocaleUpperCase("tr") === "İ"`. Every other locale (and the
//!     no-arg form) falls back to the language-neutral Unicode casing already
//!     implemented by `to_lowercase` / `to_uppercase`.
//!
//! Closes #2781.

use super::*;
use crate::value::JSValue;

#[cold]
fn throw_invalid_language_tag(tag: &str) -> ! {
    let message = format!("Invalid language tag: {tag}");
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Read a single locale tag JSValue (string / short-string) into an owned
/// `String`. Returns `None` for non-string values (numbers, objects, etc.),
/// which the caller coerces per spec (`ToString` would stringify them, but the
/// realistic inputs are strings or arrays of strings).
fn jsvalue_to_locale_string(v: JSValue) -> Option<String> {
    if v.is_string() {
        let ptr = v.as_string_ptr();
        if !is_valid_string_ptr(ptr) {
            return Some(String::new());
        }
        return Some(string_as_str(ptr).to_string());
    }
    if v.is_short_string() {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = v.short_string_to_buf(&mut scratch);
        return Some(String::from_utf8_lossy(&scratch[..n]).into_owned());
    }
    None
}

/// Validate a single BCP 47 language tag well enough to match V8's
/// `RangeError` surface for the common invalid inputs (e.g. `not_a_locale`).
///
/// This is intentionally a *structural* check, not the full RFC 5646 grammar:
/// a tag is a sequence of `-`-separated subtags, each 1..=8 ASCII
/// alphanumerics, and the primary subtag must be alphabetic. Underscores —
/// which `not_a_locale` uses — are rejected, matching Node. Returns the
/// lowercased primary language subtag on success.
fn validate_language_tag(tag: &str) -> Result<String, ()> {
    if tag.is_empty() {
        return Err(());
    }
    let mut subtags = tag.split('-');
    let primary = subtags.next().ok_or(())?;
    // Primary subtag: 1..=8 ASCII letters (`i`/`x` private-use single letters
    // are technically allowed as grandfathered/private tags, but the realistic
    // locale inputs are language codes, so require alphabetic here).
    if primary.is_empty() || primary.len() > 8 || !primary.bytes().all(|b| b.is_ascii_alphabetic())
    {
        return Err(());
    }
    for sub in subtags {
        if sub.is_empty() || sub.len() > 8 || !sub.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(());
        }
    }
    Ok(primary.to_ascii_lowercase())
}

/// Resolve the `locales` argument to the primary (first) language subtag,
/// validating every candidate tag. Throws `RangeError` for any malformed tag,
/// matching Node. `undefined`/`null`/missing yields `None` (host default
/// locale). An array yields the FIRST element's primary subtag (BestAvailable
/// is approximated as "first listed").
fn resolve_primary_locale(locales: f64) -> Option<String> {
    let v = JSValue::from_bits(locales.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    // Array of tags: validate each, return the first's primary subtag.
    if v.is_pointer() {
        let ptr = v.as_pointer::<u8>();
        let addr = ptr as usize;
        let is_array = !ptr.is_null()
            && addr >= crate::gc::GC_HEADER_SIZE + 0x1000
            && unsafe {
                let gc = (ptr.sub(crate::gc::GC_HEADER_SIZE)) as *const crate::gc::GcHeader;
                (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
            };
        if is_array {
            let arr = ptr as *const crate::array::ArrayHeader;
            let len = crate::array::js_array_length(arr);
            let mut first: Option<String> = None;
            for i in 0..len {
                let elem = crate::array::js_array_get(arr, i);
                if let Some(tag) = jsvalue_to_locale_string(elem) {
                    match validate_language_tag(&tag) {
                        Ok(primary) => {
                            if first.is_none() {
                                first = Some(primary);
                            }
                        }
                        Err(()) => throw_invalid_language_tag(&tag),
                    }
                }
            }
            return first;
        }
    }
    // Single string tag.
    if let Some(tag) = jsvalue_to_locale_string(v) {
        return match validate_language_tag(&tag) {
            Ok(primary) => Some(primary),
            Err(()) => throw_invalid_language_tag(&tag),
        };
    }
    None
}

/// Returns true if the primary language subtag uses Turkic dotted/dotless `I`
/// casing rules (Turkish `tr` / Azeri `az`).
fn is_turkic(primary: &Option<String>) -> bool {
    matches!(primary.as_deref(), Some("tr") | Some("az"))
}

/// Turkic-aware lowercasing: `I` → `ı` (dotless), `İ` (U+0130) → `i`. All other
/// characters use the language-neutral Unicode lowercase mapping.
fn turkic_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'I' => out.push('\u{0131}'), // LATIN SMALL LETTER DOTLESS I
            '\u{0130}' => out.push('i'), // İ → i
            other => out.extend(other.to_lowercase()),
        }
    }
    out
}

/// Turkic-aware uppercasing: `i` → `İ` (U+0130, dotted), `ı` (U+0131) → `I`.
fn turkic_upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            'i' => out.push('\u{0130}'), // i → İ (LATIN CAPITAL LETTER I WITH DOT ABOVE)
            '\u{0131}' => out.push('I'), // ı → I
            other => out.extend(other.to_uppercase()),
        }
    }
    out
}

/// `String.prototype.toLocaleLowerCase(locales)` — validates `locales` and
/// applies Turkic special casing when requested, else language-neutral.
#[no_mangle]
pub extern "C" fn js_string_to_locale_lower_case(
    s: *const StringHeader,
    locales: f64,
) -> *mut StringHeader {
    let primary = resolve_primary_locale(locales);
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let lower = if is_turkic(&primary) {
        turkic_lower(str_data)
    } else {
        str_data.to_lowercase()
    };
    js_string_from_str(&lower)
}

/// `String.prototype.toLocaleUpperCase(locales)` — see `..lower_case`.
#[no_mangle]
pub extern "C" fn js_string_to_locale_upper_case(
    s: *const StringHeader,
    locales: f64,
) -> *mut StringHeader {
    let primary = resolve_primary_locale(locales);
    if !is_valid_string_ptr(s) {
        return js_string_from_bytes(ptr::null(), 0);
    }
    let str_data = string_as_str(s);
    let upper = if is_turkic(&primary) {
        turkic_upper(str_data)
    } else {
        str_data.to_uppercase()
    };
    js_string_from_str(&upper)
}

/// Validate the `locales` argument of `localeCompare` for its side effect
/// (throwing `RangeError` on an invalid tag). Returns nothing — the actual
/// comparison still routes through the existing (locale-neutral) collation in
/// `compare.rs`, since full ICU ordering is deferred.
#[no_mangle]
pub extern "C" fn js_string_validate_locales(locales: f64) {
    let _ = resolve_primary_locale(locales);
}

// `#[used]` keepalive anchors: these `#[no_mangle]` entry points are reached
// only from generated `.o`, so the whole-program auto-optimize bitcode rebuild
// would otherwise dead-strip them (see project_auto_optimize_keepalive_3320).
#[used]
static KEEP_LOCALE_LOWER: extern "C" fn(*const StringHeader, f64) -> *mut StringHeader =
    js_string_to_locale_lower_case;
#[used]
static KEEP_LOCALE_UPPER: extern "C" fn(*const StringHeader, f64) -> *mut StringHeader =
    js_string_to_locale_upper_case;
#[used]
static KEEP_VALIDATE_LOCALES: extern "C" fn(f64) = js_string_validate_locales;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_well_formed_tags() {
        assert_eq!(validate_language_tag("tr"), Ok("tr".to_string()));
        assert_eq!(validate_language_tag("en-US"), Ok("en".to_string()));
        assert_eq!(validate_language_tag("az-Latn-AZ"), Ok("az".to_string()));
        assert_eq!(validate_language_tag("DE"), Ok("de".to_string()));
    }

    #[test]
    fn rejects_malformed_tags() {
        assert!(validate_language_tag("not_a_locale").is_err());
        assert!(validate_language_tag("").is_err());
        assert!(validate_language_tag("e-").is_err());
        assert!(validate_language_tag("toolongsubtag").is_err());
        assert!(validate_language_tag("123").is_err());
    }

    #[test]
    fn turkic_casing_rules() {
        assert_eq!(turkic_lower("I"), "\u{0131}");
        assert_eq!(turkic_lower("\u{0130}"), "i");
        assert_eq!(turkic_upper("i"), "\u{0130}");
        assert_eq!(turkic_upper("\u{0131}"), "I");
        // Non-Turkic letters keep neutral casing.
        assert_eq!(turkic_lower("ABC"), "abc");
        assert_eq!(turkic_upper("abc"), "ABC");
    }
}

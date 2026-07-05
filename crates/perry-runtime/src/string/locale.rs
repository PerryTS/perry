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
//!   * Language-sensitive casing from SpecialCasing.txt:
//!     - Turkish/Azeri (`tr`/`az`) dotted/dotless `I` — `"I".toLocaleLowerCase("tr")
//!       === "ı"`, `"i".toLocaleUpperCase("tr") === "İ"`, plus the conditional
//!       `After_I` / `Before_Dot` COMBINING DOT ABOVE handling.
//!     - Lithuanian (`lt`) — the `More_Above` explicit-dot insertion when
//!       lowercasing `I`/`J`/`Į`, the precomposed `Ì`/`Í`/`Ĩ` decompositions, and
//!       the `After_Soft_Dotted` dot removal when uppercasing.
//!     Every other locale (and the no-arg form) falls back to the
//!     language-neutral Unicode casing (`to_lowercase` / `to_uppercase`).
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

/// Returns true if the primary language subtag is Lithuanian (`lt`).
fn is_lithuanian(primary: &Option<String>) -> bool {
    matches!(primary.as_deref(), Some("lt"))
}

/// Canonical Combining Class of `ch`. The full UCD table lives in
/// `unicode-normalization` (already a dependency, gated behind
/// `string-normalize`); when that feature is off we fall back to a tiny table
/// covering the marks the SpecialCasing conditions actually inspect (dot above
/// / grave = 230, dot below / oblique-stroke = 220). Everything unlisted is 0,
/// which is the correct default for the base letters these rules operate on.
#[inline]
fn ccc(ch: char) -> u8 {
    #[cfg(feature = "string-normalize")]
    {
        unicode_normalization::char::canonical_combining_class(ch)
    }
    #[cfg(not(feature = "string-normalize"))]
    {
        match ch {
            // Above (230)
            '\u{0300}'..='\u{0314}'
            | '\u{033D}'..='\u{0344}'
            | '\u{0346}'
            | '\u{034A}'..='\u{034C}'
            | '\u{0350}'..='\u{0352}'
            | '\u{0357}'
            | '\u{035B}'
            | '\u{0363}'..='\u{036F}'
            | '\u{1D185}'..='\u{1D189}' => 230,
            // Below (220)
            '\u{0316}'..='\u{0319}'
            | '\u{031C}'..='\u{0320}'
            | '\u{0323}'..='\u{0333}'
            | '\u{0339}'..='\u{033C}'
            | '\u{0347}'..='\u{0349}'
            | '\u{034D}'..='\u{034E}'
            | '\u{0353}'..='\u{0356}'
            | '\u{101FD}' => 220,
            _ => 0,
        }
    }
}

/// `More_Above` (SpecialCasing.txt): starting *after* `chars[i]`, the next
/// character of combining class 0 or 230 is class 230 (i.e. there is a following
/// Above mark before any starter or lower/higher-blocking mark). Intervening
/// marks of other classes (e.g. Below = 220) are skipped.
fn more_above(chars: &[char], i: usize) -> bool {
    for &c in &chars[i + 1..] {
        match ccc(c) {
            230 => return true,
            0 => return false,
            _ => continue,
        }
    }
    false
}

/// `Before_Dot` (SpecialCasing.txt, Turkic): starting *after* `chars[i]`, the
/// next character of combining class 0 or 230 is `COMBINING DOT ABOVE`
/// (U+0307). Its negation is `Not_Before_Dot`.
fn before_dot(chars: &[char], i: usize) -> bool {
    for &c in &chars[i + 1..] {
        if c == '\u{0307}' {
            return true;
        }
        if ccc(c) == 0 || ccc(c) == 230 {
            return false;
        }
    }
    false
}

/// Soft_Dotted code points (Unicode PropList.txt) — the 46 characters the
/// Lithuanian `After_Soft_Dotted` uppercasing condition inspects. Small and
/// stable enough to inline rather than pull in the whole property table.
fn is_soft_dotted(ch: char) -> bool {
    matches!(
        ch,
        '\u{0069}'
            | '\u{006A}'
            | '\u{012F}'
            | '\u{0249}'
            | '\u{0268}'
            | '\u{029D}'
            | '\u{02B2}'
            | '\u{03F3}'
            | '\u{0456}'
            | '\u{0458}'
            | '\u{1D62}'
            | '\u{1D96}'
            | '\u{1DA4}'
            | '\u{1DA8}'
            | '\u{1E2D}'
            | '\u{1ECB}'
            | '\u{2071}'
            | '\u{2148}'
            | '\u{2149}'
            | '\u{2C7C}'
            | '\u{1D422}'
            | '\u{1D423}'
            | '\u{1D456}'
            | '\u{1D457}'
            | '\u{1D48A}'
            | '\u{1D48B}'
            | '\u{1D4BE}'
            | '\u{1D4BF}'
            | '\u{1D4F2}'
            | '\u{1D4F3}'
            | '\u{1D526}'
            | '\u{1D527}'
            | '\u{1D55A}'
            | '\u{1D55B}'
            | '\u{1D58E}'
            | '\u{1D58F}'
            | '\u{1D5C2}'
            | '\u{1D5C3}'
            | '\u{1D5F6}'
            | '\u{1D5F7}'
            | '\u{1D62A}'
            | '\u{1D62B}'
            | '\u{1D65E}'
            | '\u{1D65F}'
            | '\u{1D692}'
            | '\u{1D693}'
    )
}

/// Turkic-aware lowercasing (Turkish/Azeri SpecialCasing.txt conditionals):
///   * `İ` (U+0130) → `i` (unconditional).
///   * `I` → `ı` (dotless) when `Not_Before_Dot`, else → `i` (default).
///   * `COMBINING DOT ABOVE` (U+0307) is removed when `After_I` (the last
///     preceding class-0-or-230 character was `I`).
/// All other characters use the language-neutral Unicode lowercase mapping.
fn turkic_lower(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    // `After_I`: the last preceding class-0-or-230 character was `I` (U+0049).
    let mut after_i = false;
    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '\u{0130}' => out.push('i'), // İ → i
            'I' => {
                if before_dot(&chars, i) {
                    out.push('i'); // I → i, its following dot is removed below
                } else {
                    out.push('\u{0131}'); // I → ı (dotless)
                }
            }
            '\u{0307}' if after_i => { /* COMBINING DOT ABOVE removed After_I */ }
            other => out.extend(other.to_lowercase()),
        }
        // Update the After_I state on class-0-or-230 boundaries.
        match ccc(ch) {
            0 | 230 => after_i = ch == 'I',
            _ => {}
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

/// Lithuanian lowercasing (SpecialCasing.txt `lt` conditionals): capital `I`
/// (U+0049), `J` (U+004A) and `Į` (U+012E) gain a `COMBINING DOT ABOVE`
/// (U+0307) after their lowercase form when `More_Above`. Other characters use
/// the language-neutral mapping.
fn lithuanian_lower(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            // Unconditional precomposed `lt` mappings (SpecialCasing.txt): the
            // accent-above forms decompose to `i` + COMBINING DOT ABOVE + accent.
            '\u{00CC}' => out.push_str("\u{0069}\u{0307}\u{0300}"), // Ì
            '\u{00CD}' => out.push_str("\u{0069}\u{0307}\u{0301}"), // Í
            '\u{0128}' => out.push_str("\u{0069}\u{0307}\u{0303}"), // Ĩ
            'I' | 'J' | '\u{012E}' if more_above(&chars, i) => {
                out.extend(ch.to_lowercase());
                out.push('\u{0307}'); // inserted COMBINING DOT ABOVE
            }
            other => out.extend(other.to_lowercase()),
        }
    }
    out
}

/// Lithuanian uppercasing (SpecialCasing.txt `lt` conditional): a `COMBINING
/// DOT ABOVE` (U+0307) is removed when `After_Soft_Dotted` (the last preceding
/// class-0-or-230 character has the Soft_Dotted property). Other characters use
/// the language-neutral uppercase mapping.
fn lithuanian_upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    // `After_Soft_Dotted`: last preceding class-0-or-230 char was Soft_Dotted.
    let mut after_soft_dotted = false;
    for ch in s.chars() {
        match ch {
            '\u{0307}' if after_soft_dotted => { /* removed */ }
            other => out.extend(other.to_uppercase()),
        }
        match ccc(ch) {
            0 | 230 => after_soft_dotted = is_soft_dotted(ch),
            _ => {}
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
    } else if is_lithuanian(&primary) {
        lithuanian_lower(str_data)
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
    } else if is_lithuanian(&primary) {
        lithuanian_upper(str_data)
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

    #[test]
    fn turkic_special_casing_dot_above() {
        // COMBINING DOT ABOVE removed after I (I → i).
        assert_eq!(turkic_lower("I\u{0307}"), "i");
        // Dot below (ccc 220) doesn't reset After_I.
        assert_eq!(turkic_lower("I\u{0323}\u{0307}"), "i\u{0323}");
        assert_eq!(turkic_lower("I\u{101FD}\u{0307}"), "i\u{101FD}");
        // A class-0 char resets → I becomes dotless, dot survives.
        assert_eq!(turkic_lower("IA\u{0307}"), "\u{0131}a\u{0307}");
        // A class-230 char (grave / musical doit) blocks → I dotless, dot survives.
        assert_eq!(
            turkic_lower("I\u{0300}\u{0307}"),
            "\u{0131}\u{0300}\u{0307}"
        );
        assert_eq!(
            turkic_lower("I\u{1D185}\u{0307}"),
            "\u{0131}\u{1D185}\u{0307}"
        );
    }

    #[test]
    fn lithuanian_lower_more_above() {
        // Capital I/J/Į followed by an Above mark gain a COMBINING DOT ABOVE.
        assert_eq!(lithuanian_lower("I\u{0300}"), "i\u{0307}\u{0300}");
        assert_eq!(lithuanian_lower("J\u{0300}"), "j\u{0307}\u{0300}");
        assert_eq!(
            lithuanian_lower("\u{012E}\u{0300}"),
            "\u{012F}\u{0307}\u{0300}"
        );
        assert_eq!(lithuanian_lower("I\u{1D185}"), "i\u{0307}\u{1D185}");
        // Without an Above mark following: neutral lowercase.
        assert_eq!(lithuanian_lower("I"), "i");
        // A class-0 char after I suppresses the dot.
        assert_eq!(lithuanian_lower("IA\u{0300}"), "ia\u{0300}");
        // Precomposed accented capitals decompose to i + dot above + accent.
        assert_eq!(lithuanian_lower("\u{00CC}"), "\u{0069}\u{0307}\u{0300}");
        assert_eq!(lithuanian_lower("\u{00CD}"), "\u{0069}\u{0307}\u{0301}");
        assert_eq!(lithuanian_lower("\u{0128}"), "\u{0069}\u{0307}\u{0303}");
    }

    #[test]
    fn lithuanian_upper_after_soft_dotted() {
        // Dot above removed when preceded by Soft_Dotted (i/j and math variants).
        assert_eq!(lithuanian_upper("i\u{0307}"), "I");
        assert_eq!(lithuanian_upper("j\u{0307}"), "J");
        // Below mark between soft-dotted and dot doesn't reset.
        assert_eq!(lithuanian_upper("i\u{0323}\u{0307}"), "I\u{0323}");
        // Capital I/J are NOT soft-dotted → dot survives.
        assert_eq!(lithuanian_upper("I\u{0307}"), "I\u{0307}");
        assert_eq!(lithuanian_upper("J\u{0307}"), "J\u{0307}");
    }
}

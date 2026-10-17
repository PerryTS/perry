//! Locale-aware String methods: `toLocaleLowerCase` / `toLocaleUpperCase`
//! and the `locales` / `options` resolution of `localeCompare`.
//!
//! Perry does not ship a full ICU/`Intl` collator, so locale-sensitive
//! *collation* ordering (e.g. German vs. Swedish placement of `ä`) is still
//! deferred (tracked by the umbrella Intl work). What this module DOES match
//! against Node:
//!
//!   * `CanonicalizeLocaleList(locales)` — an invalid `locales` argument throws a
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
//!
//! GC rule for every entry here (#12539): reading `locales` / `options` runs
//! user code and can move strings, so the entries resolve them FIRST and only
//! then borrow string payloads. A string argument the caller already passed as
//! an address is rooted across the read and re-read from its handle; nothing
//! borrowed from a GC string lives across a locale or option read.

use super::*;

/// Resolve the `locales` argument of `toLocaleLowerCase` / `toLocaleUpperCase`
/// to the language subtag of the first requested locale (ECMA-402 TransformCase
/// steps 1-4, with LookupMatchingLocaleByPrefix approximated as "first listed").
///
/// `requestedLocales = ? CanonicalizeLocaleList(locales)` is the same machinery
/// `Intl.getCanonicalLocales` and the Collator validation use: a String is one
/// tag, an Array or array-like is read through `length` and `Get`, an object
/// entry goes through `ToString`, a non-String non-Object entry is a TypeError
/// and a malformed tag a RangeError. Reading the list therefore runs user code
/// (getters, `toString`), so it can collect: callers must not hold a raw string
/// address across this call (#12539). `undefined` yields `None` (host default).
fn resolve_primary_locale(locales: f64) -> Option<String> {
    crate::intl::canonicalize_locale_list(locales)
        .into_iter()
        .next()
        .map(|tag| primary_language_subtag(&tag))
}

/// The lowercased language subtag of a canonical BCP 47 tag (`"az-Latn-AZ"` ->
/// `"az"`).
fn primary_language_subtag(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

use super::normalize_hooked::ccc;

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

/// ECMA-402 TransformCase(S, locales, targetCase). `locales` is resolved first,
/// with `s` rooted across it (resolution runs user code, see
/// [`resolve_primary_locale`]); the payload is borrowed only from the address
/// re-read from the handle afterwards.
fn transform_case(
    s: *const StringHeader,
    locales: f64,
    transform: fn(&str, Option<&str>) -> String,
) -> *mut StringHeader {
    if !is_valid_string_ptr(s) {
        // Still observe CanonicalizeLocaleList's throws for an invalid receiver.
        let _ = resolve_primary_locale(locales);
        return js_string_from_bytes(ptr::null(), 0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let (primary, s) = s_handle.across_const::<StringHeader, _>(|| resolve_primary_locale(locales));
    let out = transform(string_as_str(s), primary.as_deref());
    js_string_from_str(&out)
}

fn lower_for_locale(s: &str, primary: Option<&str>) -> String {
    match primary {
        Some("tr") | Some("az") => turkic_lower(s),
        Some("lt") => lithuanian_lower(s),
        _ => s.to_lowercase(),
    }
}

fn upper_for_locale(s: &str, primary: Option<&str>) -> String {
    match primary {
        Some("tr") | Some("az") => turkic_upper(s),
        Some("lt") => lithuanian_upper(s),
        _ => s.to_uppercase(),
    }
}

/// `String.prototype.toLocaleLowerCase(locales)` — resolves `locales` and
/// applies Turkic / Lithuanian special casing when requested, else
/// language-neutral.
#[no_mangle]
pub extern "C" fn js_string_to_locale_lower_case(
    s: *const StringHeader,
    locales: f64,
) -> *mut StringHeader {
    transform_case(s, locales, lower_for_locale)
}

/// `String.prototype.toLocaleUpperCase(locales)` — see `..lower_case`.
#[no_mangle]
pub extern "C" fn js_string_to_locale_upper_case(
    s: *const StringHeader,
    locales: f64,
) -> *mut StringHeader {
    transform_case(s, locales, upper_for_locale)
}

/// Resolve the `locales` / `options` arguments of `String.prototype.localeCompare`
/// exactly as `Construct(%Collator%, « locales, options »)` would (ECMA-402
/// §22.1.3.10): CanonicalizeLocaleList, then every InitializeCollator GetOption
/// read, each option read exactly once and in spec order. Returns 1 when the
/// resolved collation is numeric (`options.numeric`), else 0; the comparison
/// itself ([`super::js_string_locale_compare_resolved`]) takes that flag and
/// reads no JS value, so no user code runs while it holds the two strings.
/// Ordering stays locale-neutral otherwise (full ICU deferred, #5906).
#[no_mangle]
pub extern "C" fn js_string_resolve_collator_args(locales: f64, options: f64) -> i32 {
    crate::intl::resolve_locale_compare(locales, options) as i32
}

// `#[used]` keepalive anchors: these `#[no_mangle]` entry points are reached
// only from generated `.o`, so the whole-program auto-optimize bitcode rebuild
// would otherwise dead-strip them (see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_LOCALE_LOWER: extern "C" fn(*const StringHeader, f64) -> *mut StringHeader =
    js_string_to_locale_lower_case;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_LOCALE_UPPER: extern "C" fn(*const StringHeader, f64) -> *mut StringHeader =
    js_string_to_locale_upper_case;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_RESOLVE_COLLATOR_ARGS: extern "C" fn(f64, f64) -> i32 = js_string_resolve_collator_args;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::JSValue;

    #[test]
    fn primary_subtag_of_canonical_tags() {
        assert_eq!(primary_language_subtag("tr"), "tr");
        assert_eq!(primary_language_subtag("en-US"), "en");
        assert_eq!(primary_language_subtag("az-Latn-AZ"), "az");
        assert_eq!(primary_language_subtag("DE"), "de");
    }

    #[test]
    fn locale_casing_dispatch() {
        assert_eq!(upper_for_locale("i", Some("tr")), "\u{0130}");
        assert_eq!(lower_for_locale("I", Some("az")), "\u{0131}");
        assert_eq!(
            lower_for_locale("I\u{0300}", Some("lt")),
            "i\u{0307}\u{0300}"
        );
        assert_eq!(upper_for_locale("i", Some("en")), "I");
        assert_eq!(lower_for_locale("I", None), "i");
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

    extern "C" fn locale_tr_getter(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::gc::gc_collect_minor();
        let value = js_string_from_bytes(b"tr".as_ptr(), 2);
        f64::from_bits(JSValue::string_ptr(value).bits())
    }

    extern "C" fn locale_en_getter(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
    ) -> f64 {
        crate::gc::gc_collect_minor();
        let value = js_string_from_bytes(b"en".as_ptr(), 2);
        f64::from_bits(JSValue::string_ptr(value).bits())
    }

    #[test]
    fn moving_locale_case_conversion_roots_strings_and_reloads_getter_array() {
        let _gc = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _force = crate::gc::knob_overrides::ForcedEvacuationTestGuard::on();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        crate::gc::gc_register_mutable_root_scanner(
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);

        let scope = crate::gc::RuntimeHandleScope::new();
        let lower = scope.root_string_ptr(js_string_from_str("I-long-string-for-moving"));
        let mut array = crate::array::js_array_alloc(2);
        array = crate::array::js_array_push(array, JSValue::undefined());
        array = crate::array::js_array_push(array, JSValue::undefined());
        let locales = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(array as i64));

        let tr_getter = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(locale_tr_getter, 0),
            0,
        ) as i64);
        let en_getter = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(locale_en_getter, 0),
            0,
        ) as i64);
        for (key, getter) in [(b"0".as_slice(), tr_getter), (b"1".as_slice(), en_getter)] {
            let key = js_string_from_bytes(key.as_ptr(), key.len() as u32);
            crate::object::js_object_define_getter(
                locales.get_nanbox_f64(),
                f64::from_bits(JSValue::string_ptr(key).bits()),
                getter,
            );
        }

        let locales_before = crate::value::js_nanbox_get_pointer(locales.get_nanbox_f64()) as usize;
        let lower_before = lower.get_raw_const_ptr::<StringHeader>() as usize;
        let lower_result = scope.root_string_ptr(js_string_to_locale_lower_case(
            lower.get_raw_const_ptr::<StringHeader>(),
            locales.get_nanbox_f64(),
        ));
        assert_ne!(
            lower_before,
            lower.get_raw_const_ptr::<StringHeader>() as usize,
            "the getter's forced moving minor must relocate the receiver"
        );
        assert_eq!(
            string_as_str(lower_result.get_raw_const_ptr()),
            "ı-long-string-for-moving"
        );
        assert_ne!(
            locales_before,
            crate::value::js_nanbox_get_pointer(locales.get_nanbox_f64()) as usize,
            "the getter's forced moving minor must relocate the locale array"
        );

        // Allocate this receiver after the first collection so the second
        // getter also runs while it is young and movable.
        let upper = scope.root_string_ptr(js_string_from_str("i-long-string-for-moving"));
        let en_array = crate::array::js_array_alloc(1);
        let en_array = crate::array::js_array_push(en_array, JSValue::undefined());
        let en_locales = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(en_array as i64));
        let en_getter = crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
            crate::fn_info!(locale_en_getter, 0),
            0,
        ) as i64);
        let en_key = js_string_from_bytes(b"0".as_ptr(), 1);
        crate::object::js_object_define_getter(
            en_locales.get_nanbox_f64(),
            f64::from_bits(JSValue::string_ptr(en_key).bits()),
            en_getter,
        );
        let en_array_before =
            crate::value::js_nanbox_get_pointer(en_locales.get_nanbox_f64()) as usize;
        let upper_before = upper.get_raw_const_ptr::<StringHeader>() as usize;
        let upper_result = scope.root_string_ptr(js_string_to_locale_upper_case(
            upper.get_raw_const_ptr::<StringHeader>(),
            en_locales.get_nanbox_f64(),
        ));
        assert_ne!(
            upper_before,
            upper.get_raw_const_ptr::<StringHeader>() as usize,
            "the getter's forced moving minor must relocate the receiver"
        );
        assert_eq!(
            string_as_str(upper_result.get_raw_const_ptr()),
            "I-LONG-STRING-FOR-MOVING"
        );
        assert_ne!(
            en_array_before,
            crate::value::js_nanbox_get_pointer(en_locales.get_nanbox_f64()) as usize,
            "the English index getter's forced moving minor must relocate its locale array"
        );
    }
}

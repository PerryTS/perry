//! `Intl.Locale` — the BCP-47 locale object of ECMA-402.
//!
//! A focused but spec-shaped implementation: the constructor parses a
//! `unicode_locale_id` (language / script / region / variants + the `-u-`
//! Unicode extension keywords), applies the options-bag overrides
//! (`language`/`script`/`region`/`calendar`/`collation`/`hourCycle`/
//! `caseFirst`/`numeric`/`numberingSystem`), and exposes the canonical string
//! plus the eleven accessor properties (`baseName`, `language`, `script`,
//! `region`, `calendar`, `caseFirst`, `collation`, `hourCycle`, `numeric`,
//! `numberingSystem`) as *getters on `Intl.Locale.prototype`* — the real
//! descriptor shape, not own data properties. `toString`/`maximize`/`minimize`
//! are prototype methods.
//!
//! `maximize`/`minimize` use a curated likely-subtags table (full CLDR
//! likely-subtags data needs `icu_locale` + its data pack, which is out of
//! scope here); they are correct for the common languages and fall back to the
//! identity transform for the long tail.

use std::collections::BTreeMap;

use super::{
    bool_value, captured_intl_object, get_field, get_string_field, install_bound_instance_function,
    install_function, object_ptr_from_value, set_builtin_attrs, set_field, set_internal_field,
    set_proto_to_string_tag, string_from_string_value, string_value, throw_range_error,
    throw_type_error, undefined, value_to_string, KEY_KIND,
};
use crate::closure::ClosureHeader;
use crate::object::{js_object_alloc, ObjectHeader, PropertyAttrs};
use crate::value::{js_is_truthy, js_nanbox_pointer, JSValue};

const KIND_LOCALE: &str = "Locale";

/// Internal slot holding the canonical locale id (non-enumerable) — read by
/// `toString` / `maximize` / `minimize`.
const KEY_FULL: &str = "__localeFull";

// The value-bearing properties are stored under their public names as
// non-enumerable own data props (so live `loc.language` dispatch works — these
// native objects do not consult the prototype accessor chain for lookup). The
// matching accessor getters live on `Intl.Locale.prototype` for reflection.
const KEY_BASENAME: &str = "baseName";
const KEY_LANGUAGE: &str = "language";
const KEY_SCRIPT: &str = "script";
const KEY_REGION: &str = "region";
const KEY_CALENDAR: &str = "calendar";
const KEY_CASEFIRST: &str = "caseFirst";
const KEY_COLLATION: &str = "collation";
const KEY_HOURCYCLE: &str = "hourCycle";
const KEY_NUMERIC: &str = "numeric";
const KEY_NUMBERINGSYSTEM: &str = "numberingSystem";
const KEY_FIRSTDAYOFWEEK: &str = "firstDayOfWeek";

// ---- parsing ---------------------------------------------------------------

#[derive(Default, Clone)]
struct ParsedLocale {
    language: String,
    script: Option<String>,
    region: Option<String>,
    variants: Vec<String>,
    attributes: Vec<String>,
    keywords: BTreeMap<String, String>,
    /// Non-`u` singleton extensions (`-t-`, `-x-`, …) preserved verbatim as
    /// `(singleton, joined-subtags)` for round-tripping through `toString`.
    other_ext: Vec<(char, String)>,
}

fn is_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}
fn is_digit(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}
fn is_alnum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn valid_language_subtag(s: &str) -> bool {
    is_alpha(s) && (s.len() == 2 || s.len() == 3 || (5..=8).contains(&s.len()))
}
fn valid_script_subtag(s: &str) -> bool {
    is_alpha(s) && s.len() == 4
}
fn valid_region_subtag(s: &str) -> bool {
    (is_alpha(s) && s.len() == 2) || (is_digit(s) && s.len() == 3)
}
fn valid_variant_subtag(s: &str) -> bool {
    (is_alnum(s) && (5..=8).contains(&s.len()))
        || (s.len() == 4 && s.as_bytes()[0].is_ascii_digit() && is_alnum(s))
}
/// A Unicode extension `type` value: one or more `alphanum{3,8}` segments.
fn valid_unicode_type(s: &str) -> bool {
    !s.is_empty()
        && s.split('-')
            .all(|seg| is_alnum(seg) && (3..=8).contains(&seg.len()))
}

fn title_case(s: &str) -> String {
    let mut out = s.to_ascii_lowercase();
    if let Some(first) = out.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    out
}

/// Parse a `unicode_locale_id`. Returns `None` for any structural violation
/// (the caller raises `RangeError`).
fn parse_language_tag(tag: &str) -> Option<ParsedLocale> {
    if tag.is_empty() {
        return None;
    }
    let tokens: Vec<&str> = tag.split('-').collect();
    if tokens
        .iter()
        .any(|t| t.is_empty() || t.len() > 8 || !is_alnum(t))
    {
        return None;
    }
    let mut p = ParsedLocale::default();
    let mut i = 0;

    // language (required)
    if !valid_language_subtag(tokens[i]) {
        return None;
    }
    p.language = tokens[i].to_ascii_lowercase();
    i += 1;

    // script (optional, alpha-4)
    if i < tokens.len() && valid_script_subtag(tokens[i]) {
        p.script = Some(title_case(tokens[i]));
        i += 1;
    }
    // region (optional)
    if i < tokens.len() && valid_region_subtag(tokens[i]) {
        p.region = Some(tokens[i].to_ascii_uppercase());
        i += 1;
    }
    // variants
    while i < tokens.len() && valid_variant_subtag(tokens[i]) {
        let v = tokens[i].to_ascii_lowercase();
        if p.variants.contains(&v) {
            return None; // duplicate variant
        }
        p.variants.push(v);
        i += 1;
    }

    // extensions / private use
    let mut seen_singletons: Vec<char> = Vec::new();
    while i < tokens.len() {
        if tokens[i].len() != 1 {
            return None; // leftover non-singleton => structurally invalid
        }
        let singleton = tokens[i].to_ascii_lowercase().chars().next().unwrap();
        if seen_singletons.contains(&singleton) {
            return None; // duplicate singleton
        }
        seen_singletons.push(singleton);
        i += 1;

        if singleton == 'u' {
            if !parse_unicode_extension(&tokens, &mut i, &mut p) {
                return None;
            }
        } else if singleton == 'x' {
            // private use: one or more alphanum{1,8} subtags, terminates the tag.
            let start = i;
            let mut buf = String::new();
            while i < tokens.len() {
                if !is_alnum(tokens[i]) {
                    return None;
                }
                if !buf.is_empty() {
                    buf.push('-');
                }
                buf.push_str(&tokens[i].to_ascii_lowercase());
                i += 1;
            }
            if i == start {
                return None;
            }
            p.other_ext.push((singleton, buf));
        } else {
            // other singleton (-t-, -a-..-s-): subtags are alphanum{2,8}.
            let start = i;
            let mut buf = String::new();
            while i < tokens.len() && tokens[i].len() >= 2 && is_alnum(tokens[i]) {
                if !buf.is_empty() {
                    buf.push('-');
                }
                buf.push_str(&tokens[i].to_ascii_lowercase());
                i += 1;
            }
            if i == start {
                return None;
            }
            p.other_ext.push((singleton, buf));
        }
    }
    Some(p)
}

/// Parse the body of a `-u-` Unicode extension into `attributes` + `keywords`,
/// advancing `*i` to the next singleton (or end). Returns `false` on a malformed
/// (empty) extension.
fn parse_unicode_extension(tokens: &[&str], i: &mut usize, p: &mut ParsedLocale) -> bool {
    let start = *i;
    let mut cur_key: Option<String> = None;
    let mut cur_vals: Vec<String> = Vec::new();
    while *i < tokens.len() && tokens[*i].len() >= 2 {
        let tok = tokens[*i];
        if !is_alnum(tok) {
            return false;
        }
        if tok.len() == 2 {
            // new keyword key — flush the previous one.
            if let Some(key) = cur_key.take() {
                insert_keyword(p, key, std::mem::take(&mut cur_vals));
            }
            cur_key = Some(tok.to_ascii_lowercase());
        } else if cur_key.is_none() {
            p.attributes.push(tok.to_ascii_lowercase());
        } else {
            cur_vals.push(tok.to_ascii_lowercase());
        }
        *i += 1;
    }
    if let Some(key) = cur_key.take() {
        insert_keyword(p, key, std::mem::take(&mut cur_vals));
    }
    *i != start
}

/// Insert a keyword, applying UTS-35 value canonicalization: an empty value or
/// the literal `"true"` collapses to the boolean form (stored as `""`).
fn insert_keyword(p: &mut ParsedLocale, key: String, vals: Vec<String>) {
    let mut value = canonicalize_keyword_value(&key, &vals.join("-"));
    if value == "true" {
        value.clear();
    }
    p.keywords.entry(key).or_insert(value);
}

/// Apply UTS-35 § 3.2.1 value canonicalization for the `-u-` keyword `key`
/// (e.g. the deprecated calendar aliases `islamicc` → `islamic-civil`).
fn canonicalize_keyword_value(key: &str, value: &str) -> String {
    match (key, value) {
        ("ca", "islamicc") => "islamic-civil".to_string(),
        ("ca", "ethiopic-amete-alem") => "ethioaa".to_string(),
        ("ca", "gregorian") => "gregory".to_string(),
        ("ms", "imperial") => "uksystem".to_string(),
        ("tz", "aqams") => "nzakl".to_string(),
        _ => value.to_string(),
    }
}

/// Canonicalize a parsed locale's base subtags per the CLDR language/variant
/// aliases that diverge from plain RFC 5646 canonicalization (e.g. `mo` → `ro`).
fn canonicalize_aliases(p: &mut ParsedLocale) {
    if let Some(canon) = language_alias(&p.language) {
        p.language = canon.to_string();
    }
    // Variant-keyed language aliases (CLDR `languageAlias` with a variant).
    if p.language == "hy" {
        if let Some(pos) = p.variants.iter().position(|v| v == "arevmda") {
            p.variants.remove(pos);
            p.language = "hyw".to_string();
        } else if let Some(pos) = p.variants.iter().position(|v| v == "arevela") {
            p.variants.remove(pos);
        }
    }
}

/// CLDR `languageAlias` replacements (deprecated/legacy codes → preferred).
fn language_alias(lang: &str) -> Option<&'static str> {
    Some(match lang {
        "mo" => "ro",
        "aar" => "aa",
        "heb" => "he",
        "ces" => "cs",
        "deu" => "de",
        "eng" => "en",
        "fra" | "fre" => "fr",
        "spa" => "es",
        "rus" => "ru",
        "zho" | "chi" => "zh",
        "jpn" => "ja",
        "in" => "id",
        "iw" => "he",
        "ji" => "yi",
        "tl" => "fil",
        _ => return None,
    })
}

// ---- canonical serialization ----------------------------------------------

fn base_name(p: &ParsedLocale) -> String {
    let mut s = p.language.clone();
    if let Some(sc) = &p.script {
        s.push('-');
        s.push_str(sc);
    }
    if let Some(r) = &p.region {
        s.push('-');
        s.push_str(r);
    }
    let mut variants = p.variants.clone();
    variants.sort();
    for v in variants {
        s.push('-');
        s.push_str(&v);
    }
    s
}

fn full_string(p: &ParsedLocale) -> String {
    let mut s = base_name(p);
    if !p.attributes.is_empty() || !p.keywords.is_empty() {
        s.push_str("-u");
        let mut attrs = p.attributes.clone();
        attrs.sort();
        for a in attrs {
            s.push('-');
            s.push_str(&a);
        }
        for (k, v) in &p.keywords {
            s.push('-');
            s.push_str(k);
            if !v.is_empty() {
                s.push('-');
                s.push_str(v);
            }
        }
    }
    // Other extensions, sorted by singleton with private-use (`x`) last.
    let mut others = p.other_ext.clone();
    others.sort_by_key(|(c, _)| if *c == 'x' { '{' } else { *c });
    for (c, content) in others {
        s.push('-');
        s.push(c);
        s.push('-');
        s.push_str(&content);
    }
    s
}

// ---- options ---------------------------------------------------------------

/// Read an Intl.Locale option per `GetOption(options, key, "string", …)`: only
/// `undefined` (or a missing key) is absent — `null`, numbers, booleans and
/// objects are coerced through `ToString` (so e.g. `{ script: null }` yields the
/// structurally-valid script subtag `"null"`).
fn get_opt_string(options: Option<*mut ObjectHeader>, key: &str) -> Option<String> {
    let obj = options?;
    let value = get_field(obj, key);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        None
    } else if js.is_any_string() {
        string_from_string_value(value)
    } else {
        Some(value_to_string(value))
    }
}

/// Set a `-u-` keyword from an option, validating the value as a Unicode type.
fn apply_type_keyword(
    p: &mut ParsedLocale,
    options: Option<*mut ObjectHeader>,
    opt_name: &str,
    key: &str,
) {
    if let Some(raw) = get_opt_string(options, opt_name) {
        let value = raw.to_ascii_lowercase();
        if !valid_unicode_type(&value) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property {opt_name}"
            ));
        }
        let value = canonicalize_keyword_value(key, &value);
        let canonical = if value == "true" {
            String::new()
        } else {
            value
        };
        p.keywords.insert(key.to_string(), canonical);
    }
}

fn apply_enum_keyword(
    p: &mut ParsedLocale,
    options: Option<*mut ObjectHeader>,
    opt_name: &str,
    key: &str,
    allowed: &[&str],
) {
    if let Some(raw) = get_opt_string(options, opt_name) {
        // `GetOption` validates the coerced string against the allowed set
        // case-sensitively, so `"Upper"`/`"H12"` are rejected even though the
        // canonical keyword values are lowercase.
        if !allowed.contains(&raw.as_str()) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property {opt_name}"
            ));
        }
        p.keywords.insert(key.to_string(), raw);
    }
}

fn apply_options(p: &mut ParsedLocale, options: Option<*mut ObjectHeader>) {
    // Base-subtag overrides.
    if let Some(raw) = get_opt_string(options, "language") {
        let value = raw.to_ascii_lowercase();
        if !valid_language_subtag(&value) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property language"
            ));
        }
        p.language = value;
    }
    if let Some(raw) = get_opt_string(options, "script") {
        if !valid_script_subtag(&raw) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property script"
            ));
        }
        p.script = Some(title_case(&raw));
    }
    if let Some(raw) = get_opt_string(options, "region") {
        if !valid_region_subtag(&raw) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property region"
            ));
        }
        p.region = Some(raw.to_ascii_uppercase());
    }

    // Unicode-extension keyword overrides.
    apply_type_keyword(p, options, "calendar", "ca");
    apply_type_keyword(p, options, "collation", "co");
    apply_enum_keyword(p, options, "hourCycle", "hc", &["h11", "h12", "h23", "h24"]);
    apply_enum_keyword(p, options, "caseFirst", "kf", &["upper", "lower", "false"]);
    apply_type_keyword(p, options, "numberingSystem", "nu");

    // `numeric` is a Boolean option mapped to the `kn` keyword.
    if let Some(obj) = options {
        let value = get_field(obj, "numeric");
        let js = JSValue::from_bits(value.to_bits());
        if !js.is_undefined() {
            let kn = if js_is_truthy(value) != 0 {
                ""
            } else {
                "false"
            };
            p.keywords.insert("kn".to_string(), kn.to_string());
        }
    }

    // `firstDayOfWeek` is coerced to a String (so `null`/numbers/booleans pass
    // through `ToString`), mapped through `WeekdayToString`, validated as a
    // Unicode type sequence, then stored as the `fw` keyword.
    if let Some(raw) = get_opt_string(options, "firstDayOfWeek") {
        let normalized = weekday_to_string(&raw.to_ascii_lowercase());
        if !valid_unicode_type(&normalized) {
            throw_range_error(&format!(
                "Value {raw} out of range for Intl.Locale options property firstDayOfWeek"
            ));
        }
        let canonical = if normalized == "true" {
            String::new()
        } else {
            normalized
        };
        p.keywords.insert("fw".to_string(), canonical);
    }
}

/// UTS-35 `WeekdayToString`: map a numeric or named weekday to its lowercase
/// `fw` keyword value; anything else passes through unchanged.
fn weekday_to_string(s: &str) -> String {
    match s {
        "mon" | "1" => "mon",
        "tue" | "2" => "tue",
        "wed" | "3" => "wed",
        "thu" | "4" => "thu",
        "fri" | "5" => "fri",
        "sat" | "6" => "sat",
        "sun" | "7" | "0" => "sun",
        other => other,
    }
    .to_string()
}

/// Inverse of [`weekday_to_string`] for the named days: `fw` keyword → ISO
/// weekday number (1 = Monday … 7 = Sunday). `None` for non-weekday values.
fn weekday_name_to_num(s: &str) -> Option<u8> {
    Some(match s {
        "mon" => 1,
        "tue" => 2,
        "wed" => 3,
        "thu" => 4,
        "fri" => 5,
        "sat" => 6,
        "sun" => 7,
        _ => return None,
    })
}

// ---- instance construction -------------------------------------------------

fn make_locale_instance(proto_bits: u64, p: &ParsedLocale) -> f64 {
    let obj = js_object_alloc(0, 12);
    set_internal_field(obj, KEY_KIND, string_value(KIND_LOCALE));
    set_internal_field(obj, KEY_FULL, string_value(&full_string(p)));
    set_internal_field(obj, KEY_BASENAME, string_value(&base_name(p)));
    set_internal_field(obj, KEY_LANGUAGE, string_value(&p.language));
    if let Some(sc) = &p.script {
        set_internal_field(obj, KEY_SCRIPT, string_value(sc));
    }
    if let Some(r) = &p.region {
        set_internal_field(obj, KEY_REGION, string_value(r));
    }
    if let Some(v) = p.keywords.get("ca") {
        set_internal_field(obj, KEY_CALENDAR, string_value(v));
    }
    if let Some(v) = p.keywords.get("kf") {
        set_internal_field(obj, KEY_CASEFIRST, string_value(v));
    }
    if let Some(v) = p.keywords.get("co") {
        set_internal_field(obj, KEY_COLLATION, string_value(v));
    }
    if let Some(v) = p.keywords.get("hc") {
        set_internal_field(obj, KEY_HOURCYCLE, string_value(v));
    }
    if let Some(v) = p.keywords.get("nu") {
        set_internal_field(obj, KEY_NUMBERINGSYSTEM, string_value(v));
    }
    let numeric = p.keywords.get("kn").map(|v| v != "false").unwrap_or(false);
    set_internal_field(obj, KEY_NUMERIC, bool_value(numeric));
    if let Some(fw) = p.keywords.get("fw").filter(|v| !v.is_empty()) {
        set_internal_field(obj, KEY_FIRSTDAYOFWEEK, string_value(fw));
    }

    // These native objects resolve methods from own properties, not the static
    // prototype chain, so install bound `toString`/`maximize`/`minimize` (and
    // the `Intl.Locale-info` getters) on the instance (mirroring the other
    // `Intl.*` constructors).
    install_bound_instance_function(obj, "toString", locale_bound_to_string as *const u8, 0);
    install_bound_instance_function(obj, "maximize", locale_bound_maximize as *const u8, 0);
    install_bound_instance_function(obj, "minimize", locale_bound_minimize as *const u8, 0);
    install_bound_instance_function(
        obj,
        "getCalendars",
        locale_bound_get_calendars as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getCollations",
        locale_bound_get_collations as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getHourCycles",
        locale_bound_get_hour_cycles as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getNumberingSystems",
        locale_bound_get_numbering_systems as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getTimeZones",
        locale_bound_get_time_zones as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getTextInfo",
        locale_bound_get_text_info as *const u8,
        0,
    );
    install_bound_instance_function(
        obj,
        "getWeekInfo",
        locale_bound_get_week_info as *const u8,
        0,
    );

    if JSValue::from_bits(proto_bits).is_pointer() {
        crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto_bits);
    }
    js_nanbox_pointer(obj as i64)
}

extern "C" fn locale_bound_to_string(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "toString", KIND_LOCALE);
    string_value(&get_string_field(obj, KEY_FULL).unwrap_or_default())
}

extern "C" fn locale_bound_maximize(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "maximize", KIND_LOCALE);
    transform_instance(obj, likely_subtags::maximize)
}

extern "C" fn locale_bound_minimize(closure: *const ClosureHeader) -> f64 {
    let obj = captured_intl_object(closure, "minimize", KIND_LOCALE);
    transform_instance(obj, likely_subtags::minimize)
}

extern "C" fn locale_bound_get_calendars(closure: *const ClosureHeader) -> f64 {
    calendars_of(captured_intl_object(closure, "getCalendars", KIND_LOCALE))
}
extern "C" fn locale_bound_get_collations(closure: *const ClosureHeader) -> f64 {
    collations_of(captured_intl_object(closure, "getCollations", KIND_LOCALE))
}
extern "C" fn locale_bound_get_hour_cycles(closure: *const ClosureHeader) -> f64 {
    hour_cycles_of(captured_intl_object(closure, "getHourCycles", KIND_LOCALE))
}
extern "C" fn locale_bound_get_numbering_systems(closure: *const ClosureHeader) -> f64 {
    numbering_systems_of(captured_intl_object(
        closure,
        "getNumberingSystems",
        KIND_LOCALE,
    ))
}
extern "C" fn locale_bound_get_time_zones(closure: *const ClosureHeader) -> f64 {
    time_zones_of(captured_intl_object(closure, "getTimeZones", KIND_LOCALE))
}
extern "C" fn locale_bound_get_text_info(closure: *const ClosureHeader) -> f64 {
    text_info_of(captured_intl_object(closure, "getTextInfo", KIND_LOCALE))
}
extern "C" fn locale_bound_get_week_info(closure: *const ClosureHeader) -> f64 {
    week_info_of(captured_intl_object(closure, "getWeekInfo", KIND_LOCALE))
}

/// Apply a likely-subtags transform to a live instance, returning a fresh
/// `Intl.Locale` that inherits the receiver's prototype.
fn transform_instance(obj: *const ObjectHeader, transform: fn(&mut ParsedLocale)) -> f64 {
    let proto = crate::object::prototype_chain::object_static_prototype(obj as usize).unwrap_or(0);
    let mut p = parsed_from_instance(obj);
    transform(&mut p);
    make_locale_instance(proto, &p)
}

extern "C" fn locale_constructor_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    super::require_new_target("Locale");
    let tag_value = super::rest_arg(rest, 0);
    let options_value = super::rest_arg(rest, 1);
    let tag_js = JSValue::from_bits(tag_value.to_bits());

    let tag = if tag_js.is_any_string() {
        string_from_string_value(tag_value).unwrap_or_default()
    } else if tag_js.is_pointer() && unsafe { crate::symbol::js_is_symbol(tag_value) } == 0 {
        // An Object: reuse an existing Locale's canonical id, else ToString it.
        match object_ptr_from_value(tag_value) {
            Some(obj) if get_string_field(obj, KEY_KIND).as_deref() == Some(KIND_LOCALE) => {
                get_string_field(obj, KEY_FULL).unwrap_or_default()
            }
            _ => value_to_string(tag_value),
        }
    } else {
        throw_type_error("Intl.Locale: tag must be a String or an Intl.Locale instance");
    };

    let Some(mut parsed) = parse_language_tag(&tag) else {
        throw_range_error(&format!("Incorrect locale information provided: {tag}"));
    };
    canonicalize_aliases(&mut parsed);

    let options = object_ptr_from_value(options_value);
    if options.is_none() && !JSValue::from_bits(options_value.to_bits()).is_undefined() {
        // CoerceOptionsToObject: a non-undefined non-object (e.g. null) is a TypeError.
        if JSValue::from_bits(options_value.to_bits()).is_null() {
            throw_type_error("Intl.Locale options must be an object");
        }
    }
    apply_options(&mut parsed, options);

    let proto = super::constructor_target_prototype(closure);
    make_locale_instance(proto.to_bits(), &parsed)
}

// ---- prototype methods & getters ------------------------------------------

fn locale_this(method: &str) -> *mut ObjectHeader {
    let this = crate::object::js_implicit_this_get();
    let Some(obj) = object_ptr_from_value(this) else {
        throw_type_error(&format!(
            "Intl.Locale.prototype.{method} called on incompatible receiver"
        ));
    };
    if get_string_field(obj, KEY_KIND).as_deref() != Some(KIND_LOCALE) {
        throw_type_error(&format!(
            "Intl.Locale.prototype.{method} called on incompatible receiver"
        ));
    }
    obj
}

fn field_or_undefined(obj: *const ObjectHeader, key: &str) -> f64 {
    let raw = get_field(obj, key);
    if JSValue::from_bits(raw.to_bits()).is_undefined() {
        undefined()
    } else {
        raw
    }
}

extern "C" fn locale_to_string_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = locale_this("toString");
    string_value(&get_string_field(obj, KEY_FULL).unwrap_or_default())
}

extern "C" fn locale_maximize_thunk(_closure: *const ClosureHeader) -> f64 {
    transform_instance(locale_this("maximize"), likely_subtags::maximize)
}

extern "C" fn locale_minimize_thunk(_closure: *const ClosureHeader) -> f64 {
    transform_instance(locale_this("minimize"), likely_subtags::minimize)
}

extern "C" fn locale_get_calendars_thunk(_closure: *const ClosureHeader) -> f64 {
    calendars_of(locale_this("getCalendars"))
}
extern "C" fn locale_get_collations_thunk(_closure: *const ClosureHeader) -> f64 {
    collations_of(locale_this("getCollations"))
}
extern "C" fn locale_get_hour_cycles_thunk(_closure: *const ClosureHeader) -> f64 {
    hour_cycles_of(locale_this("getHourCycles"))
}
extern "C" fn locale_get_numbering_systems_thunk(_closure: *const ClosureHeader) -> f64 {
    numbering_systems_of(locale_this("getNumberingSystems"))
}
extern "C" fn locale_get_time_zones_thunk(_closure: *const ClosureHeader) -> f64 {
    time_zones_of(locale_this("getTimeZones"))
}
extern "C" fn locale_get_text_info_thunk(_closure: *const ClosureHeader) -> f64 {
    text_info_of(locale_this("getTextInfo"))
}
extern "C" fn locale_get_week_info_thunk(_closure: *const ClosureHeader) -> f64 {
    week_info_of(locale_this("getWeekInfo"))
}

// ---- Intl.Locale-info computations -----------------------------------------

/// Build a JS `Array` of strings.
fn string_array_value(items: &[String]) -> f64 {
    let mut arr = crate::array::js_array_alloc(items.len() as u32);
    for s in items {
        arr = crate::array::js_array_push_f64(arr, string_value(s));
    }
    js_nanbox_pointer(arr as i64)
}

/// `CalendarsOfLocale`: the requested `ca` keyword if present, else the default.
fn calendars_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let list = match p.keywords.get("ca").filter(|v| !v.is_empty()) {
        Some(ca) => vec![ca.clone()],
        None => vec!["gregory".to_string()],
    };
    string_array_value(&list)
}

/// `CollationsOfLocale`: the `co` keyword (excluding `standard`/`search`) if
/// present, else the default list.
fn collations_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let list = match p
        .keywords
        .get("co")
        .filter(|v| !v.is_empty() && v.as_str() != "standard" && v.as_str() != "search")
    {
        Some(co) => vec![co.clone()],
        None => vec!["emoji".to_string(), "eor".to_string()],
    };
    string_array_value(&list)
}

/// `HourCyclesOfLocale`: the `hc` keyword if it names a valid cycle, else `h12`.
fn hour_cycles_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let list = match p
        .keywords
        .get("hc")
        .filter(|v| matches!(v.as_str(), "h11" | "h12" | "h23" | "h24"))
    {
        Some(hc) => vec![hc.clone()],
        None => vec!["h12".to_string()],
    };
    string_array_value(&list)
}

/// `NumberingSystemsOfLocale`: the `nu` keyword if present, else `latn`.
fn numbering_systems_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let list = match p.keywords.get("nu").filter(|v| !v.is_empty()) {
        Some(nu) => vec![nu.clone()],
        None => vec!["latn".to_string()],
    };
    string_array_value(&list)
}

/// `TimeZonesOfLocale`: `undefined` when the tag carries no region subtag, else
/// the (sorted) zones in common use for that region.
fn time_zones_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let Some(region) = p.region.as_deref() else {
        return undefined();
    };
    let zones: Vec<String> = info::time_zones_for_region(region)
        .into_iter()
        .map(str::to_string)
        .collect();
    string_array_value(&zones)
}

/// `getTextInfo`: an Object `{ direction }`, where direction follows the
/// (maximized) script's writing direction.
fn text_info_of(obj: *const ObjectHeader) -> f64 {
    let mut p = parsed_from_instance(obj);
    likely_subtags::maximize(&mut p);
    let rtl = p
        .script
        .as_deref()
        .map(info::is_rtl_script)
        .unwrap_or(false);
    let result = js_object_alloc(0, 1);
    set_field(
        result,
        "direction",
        string_value(if rtl { "rtl" } else { "ltr" }),
    );
    js_nanbox_pointer(result as i64)
}

/// `getWeekInfo`: an Object `{ firstDay, weekend }` with ISO weekday numbers.
fn week_info_of(obj: *const ObjectHeader) -> f64 {
    let p = parsed_from_instance(obj);
    let region = {
        let mut m = p.clone();
        likely_subtags::maximize(&mut m);
        m.region.unwrap_or_default()
    };
    let first_day = p
        .keywords
        .get("fw")
        .and_then(|fw| weekday_name_to_num(fw))
        .unwrap_or_else(|| info::first_day_of_week(&region));
    let weekend = info::weekend(&region);

    let result = js_object_alloc(0, 2);
    set_field(result, "firstDay", first_day as f64);
    let mut arr = crate::array::js_array_alloc(weekend.len() as u32);
    for d in weekend {
        arr = crate::array::js_array_push_f64(arr, d as f64);
    }
    set_field(result, "weekend", js_nanbox_pointer(arr as i64));
    js_nanbox_pointer(result as i64)
}

/// Reconstruct a [`ParsedLocale`] from a live instance by re-parsing its stored
/// canonical id — used by `maximize`/`minimize` to derive a fresh instance.
fn parsed_from_instance(obj: *const ObjectHeader) -> ParsedLocale {
    let full = get_string_field(obj, KEY_FULL).unwrap_or_default();
    parse_language_tag(&full).unwrap_or_default()
}

extern "C" fn getter_base_name(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("baseName"), KEY_BASENAME)
}
extern "C" fn getter_language(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("language"), KEY_LANGUAGE)
}
extern "C" fn getter_script(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("script"), KEY_SCRIPT)
}
extern "C" fn getter_region(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("region"), KEY_REGION)
}
extern "C" fn getter_calendar(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("calendar"), KEY_CALENDAR)
}
extern "C" fn getter_case_first(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("caseFirst"), KEY_CASEFIRST)
}
extern "C" fn getter_collation(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("collation"), KEY_COLLATION)
}
extern "C" fn getter_hour_cycle(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("hourCycle"), KEY_HOURCYCLE)
}
extern "C" fn getter_numbering_system(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("numberingSystem"), KEY_NUMBERINGSYSTEM)
}
extern "C" fn getter_first_day_of_week(_c: *const ClosureHeader) -> f64 {
    field_or_undefined(locale_this("firstDayOfWeek"), KEY_FIRSTDAYOFWEEK)
}
extern "C" fn getter_numeric(_c: *const ClosureHeader) -> f64 {
    let obj = locale_this("numeric");
    let value = get_field(obj, KEY_NUMERIC);
    if value.to_bits() == crate::value::TAG_TRUE {
        bool_value(true)
    } else {
        bool_value(false)
    }
}

fn install_getter(proto: *mut ObjectHeader, name: &str, thunk: *const u8) {
    unsafe {
        crate::closure::js_register_closure_arity(thunk, 0);
        let closure = crate::closure::js_closure_alloc(thunk, 0);
        if closure.is_null() {
            return;
        }
        crate::object::set_bound_native_closure_name(closure, &format!("get {name}"));
        crate::object::set_builtin_closure_length(closure as usize, 0);
        let getter_bits = js_nanbox_pointer(closure as i64).to_bits();
        crate::object::install_builtin_getter(proto, name, getter_bits);
    }
}

pub(super) fn install_locale(ns_obj: *mut ObjectHeader) {
    let ctor_ptr = locale_constructor_thunk as *const u8;
    let ctor = crate::closure::js_closure_alloc(ctor_ptr, 0);
    if ctor.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(ctor_ptr, 0);
    crate::object::set_bound_native_closure_name(ctor, "Locale");
    crate::object::set_builtin_closure_length(ctor as usize, 1);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );

    let ctor_value = js_nanbox_pointer(ctor as i64);
    let proto = js_object_alloc(0, 16);
    set_field(proto, "constructor", ctor_value);
    crate::object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );

    install_function(
        proto,
        "toString",
        locale_to_string_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "maximize",
        locale_maximize_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "minimize",
        locale_minimize_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getCalendars",
        locale_get_calendars_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getCollations",
        locale_get_collations_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getHourCycles",
        locale_get_hour_cycles_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getNumberingSystems",
        locale_get_numbering_systems_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getTimeZones",
        locale_get_time_zones_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getTextInfo",
        locale_get_text_info_thunk as *const u8,
        0,
        0,
        false,
    );
    install_function(
        proto,
        "getWeekInfo",
        locale_get_week_info_thunk as *const u8,
        0,
        0,
        false,
    );

    install_getter(proto, "baseName", getter_base_name as *const u8);
    install_getter(proto, "language", getter_language as *const u8);
    install_getter(proto, "script", getter_script as *const u8);
    install_getter(proto, "region", getter_region as *const u8);
    install_getter(proto, "calendar", getter_calendar as *const u8);
    install_getter(proto, "caseFirst", getter_case_first as *const u8);
    install_getter(proto, "collation", getter_collation as *const u8);
    install_getter(proto, "hourCycle", getter_hour_cycle as *const u8);
    install_getter(proto, "numeric", getter_numeric as *const u8);
    install_getter(
        proto,
        "firstDayOfWeek",
        getter_first_day_of_week as *const u8,
    );
    install_getter(
        proto,
        "numberingSystem",
        getter_numbering_system as *const u8,
    );

    set_proto_to_string_tag(proto, "Intl.Locale");

    let proto_value = js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(ctor as usize, "prototype", proto_value);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        PropertyAttrs::new(false, false, false),
    );

    set_field(ns_obj, "Locale", ctor_value);
    set_builtin_attrs(ns_obj, "Locale", PropertyAttrs::new(true, false, true));
}

mod info;
mod likely_subtags;

//! `Intl.Locale` — BCP-47 / UTS #35 locale objects, backed by the data-free
//! `icu_locale_core` parser. Split out of `intl.rs` to keep that file under the
//! file-size gate; this module is compiled only with the `intl-locale` feature.

use super::*;

const KIND_LOCALE: &str = "Locale";

// ---- Intl.Locale -----------------------------------------------------------
//
// Backed by `icu_locale_core` (the data-free BCP-47 / UTS #35 parser behind the
// `intl-locale` feature). The constructor resolves a canonical
// `unicode_locale_id` from the tag + options; each getter re-parses the stored
// canonical string on demand. `maximize`/`minimize` need CLDR likely-subtags
// data (`icu_locale`) and are best-effort no-ops here.
#[cfg(feature = "intl-locale")]
const KEY_LOCALE_TAG: &str = KEY_LOCALE;

#[cfg(feature = "intl-locale")]
thread_local! {
    /// NaN-box bits of `Intl.Locale.prototype`, stashed when the constructor is
    /// installed. The constructor thunk can't recover it from its own closure
    /// argument (`new` may hand the thunk a rebound closure that doesn't carry
    /// the `prototype` dynamic prop), so instances read it from here to link
    /// `[[Prototype]]`.
    static LOCALE_PROTO_BITS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Re-parse the canonical locale string stored on a validated `Intl.Locale`
/// receiver. The stored string is always a normalized `Locale`, so the parse
/// cannot fail in practice; fall back to `und` defensively.
#[cfg(feature = "intl-locale")]
fn locale_of(method: &str) -> icu_locale_core::Locale {
    let obj = this_intl_object(method, KIND_LOCALE);
    let tag = get_string_field(obj, KEY_LOCALE_TAG).unwrap_or_else(|| "und".to_string());
    icu_locale_core::Locale::try_from_str(&tag).unwrap_or(icu_locale_core::Locale::UNKNOWN)
}

/// Look up a unicode-extension keyword (e.g. `ca`, `co`) on a parsed locale and
/// render its value as a string, or `None` when the key is absent.
#[cfg(feature = "intl-locale")]
fn locale_keyword(loc: &icu_locale_core::Locale, key: &str) -> Option<String> {
    let key = icu_locale_core::extensions::unicode::Key::try_from_str(key).ok()?;
    loc.extensions
        .unicode
        .keywords
        .get(&key)
        .map(|v| v.to_string())
}

#[cfg(feature = "intl-locale")]
fn to_boolean(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}

/// ApplyOptionsToTag + ApplyUnicodeExtensionToTag: read each option (in the
/// spec-mandated order so `constructor-getter-order` observes the right
/// sequence), validate it structurally via `icu_locale_core`, and fold it into
/// `loc`. A structurally invalid value raises `RangeError`.
#[cfg(feature = "intl-locale")]
fn apply_locale_options(loc: &mut icu_locale_core::Locale, options: f64) {
    use icu_locale_core::extensions::unicode::{Key, Value};
    use icu_locale_core::subtags::{Language, Region, Script, Variant, Variants};

    if let Some(s) = get_option_string(options, "language") {
        match Language::try_from_str(&s) {
            Ok(v) => loc.id.language = v,
            Err(_) => throw_invalid_language_tag(&s),
        }
    }
    if let Some(s) = get_option_string(options, "script") {
        match Script::try_from_str(&s) {
            Ok(v) => loc.id.script = Some(v),
            Err(_) => throw_invalid_language_tag(&s),
        }
    }
    if let Some(s) = get_option_string(options, "region") {
        match Region::try_from_str(&s) {
            Ok(v) => loc.id.region = Some(v),
            Err(_) => throw_invalid_language_tag(&s),
        }
    }
    if let Some(s) = get_option_string(options, "variants") {
        let mut variants = Variants::new();
        for part in s.split('-') {
            match Variant::try_from_str(part) {
                // `push` returns false when the variant is already present —
                // duplicate variants are a RangeError (reject-duplicate-variants).
                Ok(v) if variants.push(v) => {}
                _ => throw_invalid_language_tag(&s),
            }
        }
        loc.id.variants = variants;
    }

    // Unicode extension keywords, read in spec order: ca, co, hc, kf, kn, nu.
    set_keyword_option(loc, options, "calendar", "ca");
    set_keyword_option(loc, options, "collation", "co");
    set_enum_keyword_option(
        loc,
        options,
        "hourCycle",
        "hc",
        &["h11", "h12", "h23", "h24"],
    );
    set_enum_keyword_option(
        loc,
        options,
        "caseFirst",
        "kf",
        &["upper", "lower", "false"],
    );

    // `numeric` is a boolean option: true → `-u-kn` (empty value, the canonical
    // form), false → `-u-kn-false`. Absent leaves the keyword untouched.
    let numeric_value = get_option_value(options, "numeric");
    if !JSValue::from_bits(numeric_value.to_bits()).is_undefined() {
        let value = if to_boolean(numeric_value) {
            Value::new_empty()
        } else {
            Value::try_from_str("false").unwrap_or_else(|_| Value::new_empty())
        };
        loc.extensions
            .unicode
            .keywords
            .set(Key::try_from_str("kn").unwrap(), value);
    }

    set_keyword_option(loc, options, "numberingSystem", "nu");
}

/// Read a free-form (`type`-shaped) keyword option, validate it as a UTS #35
/// extension value, and set it on `loc`. Invalid → RangeError.
#[cfg(feature = "intl-locale")]
fn set_keyword_option(
    loc: &mut icu_locale_core::Locale,
    options: f64,
    option_name: &str,
    key: &str,
) {
    use icu_locale_core::extensions::unicode::{Key, Value};
    if let Some(s) = get_option_string(options, option_name) {
        // ECMA-402 requires the value to match the UTS #35 `type` nonterminal:
        // one or more `alphanum{3,8}` subtags. `icu_locale_core`'s `Value` parser
        // is laxer (it accepts 2-char subtags and the empty value), so validate
        // the grammar explicitly first — invalid → RangeError.
        if !is_unicode_type_value(&s) {
            throw_invalid_language_tag(&s);
        }
        match Value::try_from_str(&s.to_ascii_lowercase()) {
            Ok(value) if !value.is_empty() => {
                loc.extensions
                    .unicode
                    .keywords
                    .set(Key::try_from_str(key).unwrap(), value);
            }
            _ => throw_invalid_language_tag(&s),
        }
    }
}

/// UTS #35 `type` grammar: one or more `-`-separated `alphanum{3,8}` subtags.
#[cfg(feature = "intl-locale")]
fn is_unicode_type_value(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|sub| {
            (3..=8).contains(&sub.len()) && sub.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

/// Read a keyword option restricted to a fixed value set (`GetOption` with a
/// `values` list): a value outside the set is a RangeError.
#[cfg(feature = "intl-locale")]
fn set_enum_keyword_option(
    loc: &mut icu_locale_core::Locale,
    options: f64,
    option_name: &str,
    key: &str,
    allowed: &[&str],
) {
    use icu_locale_core::extensions::unicode::{Key, Value};
    if let Some(s) = get_option_string(options, option_name) {
        if !allowed.contains(&s.as_str()) {
            throw_invalid_language_tag(&s);
        }
        if let Ok(value) = Value::try_from_str(&s) {
            loc.extensions
                .unicode
                .keywords
                .set(Key::try_from_str(key).unwrap(), value);
        }
    }
}

/// Resolve the canonical locale string for `new Intl.Locale(tag, options)`.
/// `tag` must be a String or an Object (a `Locale` instance reuses its stored
/// tag; any other object is `ToString`-coerced) — otherwise a TypeError. A
/// structurally invalid tag is a RangeError.
#[cfg(feature = "intl-locale")]
fn build_locale_string(tag_arg: f64, options: f64) -> String {
    let tag_js = JSValue::from_bits(tag_arg.to_bits());
    let base = if tag_js.is_any_string() {
        string_from_string_value(tag_arg).unwrap_or_default()
    } else if let Some(obj) = object_ptr_from_value(tag_arg) {
        if get_string_field(obj, KEY_KIND).as_deref() == Some(KIND_LOCALE) {
            get_string_field(obj, KEY_LOCALE_TAG).unwrap_or_default()
        } else {
            // ToString an ordinary object (triggers a user `toString`, which the
            // getter-order test observes as "tag toString").
            value_to_string(tag_arg)
        }
    } else {
        throw_type_error("Intl.Locale: tag must be a string or a Locale object");
    };

    let mut loc = match icu_locale_core::Locale::try_from_str(&base) {
        Ok(loc) => loc,
        Err(_) => throw_invalid_language_tag(&base),
    };
    apply_locale_options(&mut loc, options);
    loc.to_string()
}

/// True when `func_value` is the `Intl.Locale` constructor closure. Lets
/// `ensure_function_prototype_object` return the pre-populated `prototype`
/// (with the accessor getters / methods) instead of synthesizing a fresh empty
/// one on `new Intl.Locale()` — the same gate Temporal constructors use.
#[cfg(feature = "intl-locale")]
pub(crate) fn is_locale_ctor(func_value: f64) -> bool {
    let jv = JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let closure = jv.as_pointer::<ClosureHeader>();
    if closure.is_null() {
        return false;
    }
    let (tag, fp) = unsafe { ((*closure).type_tag, (*closure).func_ptr) };
    tag == crate::closure::CLOSURE_MAGIC && fp as *const u8 == locale_constructor_thunk as *const u8
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_constructor_thunk(_closure: *const ClosureHeader, rest: f64) -> f64 {
    let tag = rest_arg(rest, 0);
    let options = rest_arg(rest, 1);
    let locale_str = build_locale_string(tag, options);
    let obj = js_object_alloc(0, 4);
    set_internal_field(obj, KEY_KIND, string_value(KIND_LOCALE));
    set_internal_field(obj, KEY_LOCALE_TAG, string_value(&locale_str));
    let proto_bits = LOCALE_PROTO_BITS.with(|c| c.get());
    if proto_bits != 0 {
        crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto_bits);
    }
    js_nanbox_pointer(obj as i64)
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_to_string_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("toString", KIND_LOCALE);
    string_value(&get_string_field(obj, KEY_LOCALE_TAG).unwrap_or_else(|| "und".to_string()))
}

/// `maximize`/`minimize` need CLDR likely-subtags data (`icu_locale`), which is
/// out of scope for the data-free `icu_locale_core`. Return a fresh `Locale`
/// wrapping the same tag so callers that chain `.toString()` still work.
#[cfg(feature = "intl-locale")]
fn locale_clone_self(method: &str, closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object(method, KIND_LOCALE);
    let tag = get_string_field(obj, KEY_LOCALE_TAG).unwrap_or_else(|| "und".to_string());
    let out = js_object_alloc(0, 4);
    set_internal_field(out, KEY_KIND, string_value(KIND_LOCALE));
    set_internal_field(out, KEY_LOCALE_TAG, string_value(&tag));
    let proto_bits = LOCALE_PROTO_BITS.with(|c| c.get());
    if proto_bits != 0 {
        crate::object::prototype_chain::object_set_static_prototype(out as usize, proto_bits);
    }
    let _ = closure;
    js_nanbox_pointer(out as i64)
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_maximize_thunk(closure: *const ClosureHeader) -> f64 {
    locale_clone_self("maximize", closure)
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_minimize_thunk(closure: *const ClosureHeader) -> f64 {
    locale_clone_self("minimize", closure)
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_base_name(_closure: *const ClosureHeader) -> f64 {
    string_value(&locale_of("baseName").id.to_string())
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_language(_closure: *const ClosureHeader) -> f64 {
    string_value(locale_of("language").id.language.as_str())
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_script(_closure: *const ClosureHeader) -> f64 {
    match locale_of("script").id.script {
        Some(s) => string_value(s.as_str()),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_region(_closure: *const ClosureHeader) -> f64 {
    match locale_of("region").id.region {
        Some(r) => string_value(r.as_str()),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_variants(_closure: *const ClosureHeader) -> f64 {
    let loc = locale_of("variants");
    if loc.id.variants.is_empty() {
        return undefined();
    }
    let joined = loc
        .id
        .variants
        .iter()
        .map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join("-");
    string_value(&joined)
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_calendar(_closure: *const ClosureHeader) -> f64 {
    match locale_keyword(&locale_of("calendar"), "ca") {
        Some(v) => string_value(&v),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_case_first(_closure: *const ClosureHeader) -> f64 {
    match locale_keyword(&locale_of("caseFirst"), "kf") {
        Some(v) => string_value(&v),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_collation(_closure: *const ClosureHeader) -> f64 {
    match locale_keyword(&locale_of("collation"), "co") {
        Some(v) => string_value(&v),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_hour_cycle(_closure: *const ClosureHeader) -> f64 {
    match locale_keyword(&locale_of("hourCycle"), "hc") {
        Some(v) => string_value(&v),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_numbering_system(_closure: *const ClosureHeader) -> f64 {
    match locale_keyword(&locale_of("numberingSystem"), "nu") {
        Some(v) => string_value(&v),
        None => undefined(),
    }
}

#[cfg(feature = "intl-locale")]
extern "C" fn locale_get_numeric(_closure: *const ClosureHeader) -> f64 {
    // `-u-kn` (empty value) and `-u-kn-true` both mean numeric collation is on;
    // `-u-kn-false` or an absent key mean off.
    let on = locale_keyword(&locale_of("numeric"), "kn")
        .map(|v| v.is_empty() || v == "true")
        .unwrap_or(false);
    bool_value(on)
}

/// Install a built-in accessor getter (no setter) on a prototype as a
/// `{ enumerable: false, configurable: true }` accessor property. Mirrors
/// `temporal_proto::install_getter`: `install_builtin_getter` adds the key and
/// the reflection descriptor (so `getOwnPropertyDescriptor` / `getOwnPropertyNames`
/// see it), then `set_accessor_descriptor` flips the `ACCESSORS_IN_USE` hot-path
/// gate so a plain `loc.<name>` value read actually dispatches the getter.
#[cfg(feature = "intl-locale")]
fn install_proto_getter(proto: *mut ObjectHeader, name: &str, getter: *const u8) {
    let closure = crate::closure::js_closure_alloc(getter, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(getter, 0);
    crate::object::set_bound_native_closure_name(closure, &format!("get {name}"));
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    crate::object::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        PropertyAttrs::new(false, false, true),
    );
    let get_bits = js_nanbox_pointer(closure as i64).to_bits();
    unsafe {
        crate::object::install_builtin_getter(proto, name, get_bits);
    }
    crate::object::set_accessor_descriptor(
        proto as usize,
        name.to_string(),
        crate::object::AccessorDescriptor {
            get: get_bits,
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        proto as usize,
        name.to_string(),
        PropertyAttrs::new(true, false, true),
    );
}

#[cfg(feature = "intl-locale")]
pub(crate) fn install_locale_constructor(ns_obj: *mut ObjectHeader) {
    let name = "Locale";
    let ctor = crate::closure::js_closure_alloc(locale_constructor_thunk as *const u8, 0);
    if ctor.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(locale_constructor_thunk as *const u8, 0);
    crate::object::set_bound_native_closure_name(ctor, name);
    // `Intl.Locale.length === 1` (the tag parameter).
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
    set_builtin_attrs(proto, "constructor", PropertyAttrs::new(true, false, true));

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

    install_proto_getter(proto, "baseName", locale_get_base_name as *const u8);
    install_proto_getter(proto, "calendar", locale_get_calendar as *const u8);
    install_proto_getter(proto, "caseFirst", locale_get_case_first as *const u8);
    install_proto_getter(proto, "collation", locale_get_collation as *const u8);
    install_proto_getter(proto, "hourCycle", locale_get_hour_cycle as *const u8);
    install_proto_getter(proto, "language", locale_get_language as *const u8);
    install_proto_getter(
        proto,
        "numberingSystem",
        locale_get_numbering_system as *const u8,
    );
    install_proto_getter(proto, "numeric", locale_get_numeric as *const u8);
    install_proto_getter(proto, "region", locale_get_region as *const u8);
    install_proto_getter(proto, "script", locale_get_script as *const u8);
    install_proto_getter(proto, "variants", locale_get_variants as *const u8);

    set_proto_to_string_tag(proto, "Intl.Locale");
    let proto_value = js_nanbox_pointer(proto as i64);
    LOCALE_PROTO_BITS.with(|c| c.set(proto_value.to_bits()));
    crate::closure::closure_set_dynamic_prop(ctor as usize, "prototype", proto_value);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        PropertyAttrs::new(false, false, false),
    );

    set_field(ns_obj, name, ctor_value);
    set_builtin_attrs(ns_obj, name, PropertyAttrs::new(true, false, true));
}

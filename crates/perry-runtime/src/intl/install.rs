//! Shared `Intl.*` constructor/prototype installation.
//!
//! `install_constructor` builds the constructor closure, its prototype (plain
//! methods, accessor getters, `Symbol.toStringTag`), the static
//! `supportedLocalesOf`, and registers the pair on the `Intl` namespace. Split
//! out of `intl.rs` to keep that namespace module under the file-size gate; the
//! private helpers it leans on (`set_field`, `install_function`, …) stay in the
//! parent and are reachable here as a descendant module.

use crate::object::{js_object_alloc, ObjectHeader, PropertyAttrs};
use crate::value::js_nanbox_pointer;

use super::{
    install_function, set_builtin_attrs, set_field, set_proto_to_string_tag,
    supported_locales_of_thunk,
};

/// `ctor` is the constructor body's info (a rest body, `with_rest(0)`);
/// each method's info records its declared `arity`, each getter's 0.
pub(super) fn install_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    ctor: *const crate::closure::JsFunctionInfo,
    ctor_length: u32,
    methods: &[(&str, *const crate::closure::JsFunctionInfo, u32)],
    getters: &[(&str, *const crate::closure::JsFunctionInfo)],
) {
    let ctor = crate::closure::js_closure_alloc(ctor, 0);
    if ctor.is_null() {
        return;
    }
    crate::object::set_bound_native_closure_name(ctor, name);
    crate::object::set_builtin_closure_length(ctor as usize, ctor_length);
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
    // Generous inline capacity so installing methods plus an accessor getter and
    // the toStringTag symbol never bumps the live inline-slot count past the
    // physical slot count (which would expose an overflow slot —
    // keys_array.rs #4099).
    let proto = crate::realm_decl! { birth js_object_alloc(0, 16); realm { Intl.Collator.prototype, members = { constructor; resolvedOptions; compare = accessor(get = "Intl.Collator.prototype.compare.<get>"); [Symbol.toStringTag] = "Intl.Collator"; }; Intl.DateTimeFormat.prototype, members = { constructor; formatToParts; formatRange; formatRangeToParts; resolvedOptions; format = accessor(get = "Intl.DateTimeFormat.prototype.format.<get>"); [Symbol.toStringTag] = "Intl.DateTimeFormat"; }; Intl.DisplayNames.prototype, members = { constructor; of; resolvedOptions; [Symbol.toStringTag] = "Intl.DisplayNames"; }; Intl.DurationFormat.prototype, members = { constructor; format; formatToParts; resolvedOptions; [Symbol.toStringTag] = "Intl.DurationFormat"; }; Intl.ListFormat.prototype, members = { constructor; format; formatToParts; resolvedOptions; [Symbol.toStringTag] = "Intl.ListFormat"; }; Intl.Locale.prototype, members = { constructor; toString; maximize; minimize; getCalendars; getCollations; getHourCycles; getNumberingSystems; getTimeZones; getTextInfo; getWeekInfo; baseName = accessor(get = "Intl.Locale.prototype.baseName.<get>"); language = accessor(get = "Intl.Locale.prototype.language.<get>"); script = accessor(get = "Intl.Locale.prototype.script.<get>"); region = accessor(get = "Intl.Locale.prototype.region.<get>"); calendar = accessor(get = "Intl.Locale.prototype.calendar.<get>"); caseFirst = accessor(get = "Intl.Locale.prototype.caseFirst.<get>"); collation = accessor(get = "Intl.Locale.prototype.collation.<get>"); hourCycle = accessor(get = "Intl.Locale.prototype.hourCycle.<get>"); numeric = accessor(get = "Intl.Locale.prototype.numeric.<get>"); firstDayOfWeek = accessor(get = "Intl.Locale.prototype.firstDayOfWeek.<get>"); numberingSystem = accessor(get = "Intl.Locale.prototype.numberingSystem.<get>"); [Symbol.toStringTag] = "Intl.Locale"; }; Intl.NumberFormat.prototype, members = { constructor; formatToParts; formatRange; formatRangeToParts; resolvedOptions; format = accessor(get = "Intl.NumberFormat.prototype.format.<get>"); [Symbol.toStringTag] = "Intl.NumberFormat"; }; Intl.PluralRules.prototype, members = { constructor; select; selectRange; resolvedOptions; [Symbol.toStringTag] = "Intl.PluralRules"; }; Intl.RelativeTimeFormat.prototype, members = { constructor; format; formatToParts; resolvedOptions; [Symbol.toStringTag] = "Intl.RelativeTimeFormat"; }; Intl.Segmenter.prototype, members = { constructor; segment; resolvedOptions; [Symbol.toStringTag] = "Intl.Segmenter"; }; } };
    set_field(proto, "constructor", ctor_value);
    set_builtin_attrs(proto, "constructor", PropertyAttrs::new(true, false, true));
    for (method, ptr, arity) in methods.iter().copied() {
        install_function(proto, method, ptr, arity);
    }
    // Accessor properties (e.g. `get Intl.NumberFormat.prototype.format`): a
    // getter-only descriptor on the prototype so reflection
    // (`Object.getOwnPropertyDescriptor(proto, key).get`) sees a function whose
    // name is `"get <key>"` and length 0. Instances still carry an own bound
    // method for the hot dispatch path (native objects resolve from own props).
    for (getter_name, ptr) in getters.iter().copied() {
        let closure = crate::closure::js_closure_alloc(ptr, 0);
        if closure.is_null() {
            continue;
        }
        crate::object::set_bound_native_closure_name(closure, &format!("get {getter_name}"));
        crate::object::set_builtin_closure_length(closure as usize, 0);
        // The `get Intl.X.prototype.format` accessor is a built-in
        // non-constructor function — no `[[Construct]]`, no own `prototype`
        // property (`format/builtin.js` reads the getter and asserts both).
        crate::object::set_builtin_closure_non_constructable(closure as usize);
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
        let getter_bits = js_nanbox_pointer(closure as i64).to_bits();
        unsafe {
            crate::object::install_builtin_getter(proto, getter_name, getter_bits);
        }
    }
    set_proto_to_string_tag(proto, &format!("Intl.{name}"));
    let proto_value = js_nanbox_pointer(proto as i64);
    crate::closure::closure_set_dynamic_prop(ctor as usize, "prototype", proto_value);
    crate::object::set_builtin_property_attrs(
        ctor as usize,
        "prototype".to_string(),
        PropertyAttrs::new(false, false, false),
    );

    // `supportedLocalesOf(locales, options)` — `.length` is 1, but it reads a
    // second `options` argument, so register it rest-style (all args collected)
    // and pull both positionally.
    let supported = install_function(
        ctor as *mut ObjectHeader,
        "supportedLocalesOf",
        crate::fn_info!(#[realm(realm_decl! { Intl.Collator.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.DateTimeFormat.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.DisplayNames.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.DurationFormat.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.ListFormat.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.NumberFormat.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.PluralRules.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.RelativeTimeFormat.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); Intl.Segmenter.supportedLocalesOf, spec = {length = 1}, emit = metadata(realm_intl_collator_supportedlocalesof_metadata); })] supported_locales_of_thunk, 1; with_rest(0)),
        realm_intl_collator_supportedlocalesof_metadata!(length),
    );
    crate::closure::closure_set_dynamic_prop(ctor as usize, "supportedLocalesOf", supported);

    set_field(ns_obj, name, ctor_value);
    set_builtin_attrs(ns_obj, name, PropertyAttrs::new(true, false, true));
}

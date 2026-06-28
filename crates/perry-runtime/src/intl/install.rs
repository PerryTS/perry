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

pub(super) fn install_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    ctor_ptr: *const u8,
    ctor_length: u32,
    methods: &[(&str, *const u8, u32)],
    getters: &[(&str, *const u8)],
) {
    let ctor = crate::closure::js_closure_alloc(ctor_ptr, 0);
    if ctor.is_null() {
        return;
    }
    crate::closure::js_register_closure_rest(ctor_ptr, 0);
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
    // the toStringTag symbol never bumps `field_count` past the physical slot
    // count (which would expose an overflow slot — keys_array.rs #4099).
    let proto = js_object_alloc(0, 16);
    set_field(proto, "constructor", ctor_value);
    set_builtin_attrs(proto, "constructor", PropertyAttrs::new(true, false, true));
    for (method, ptr, arity) in methods.iter().copied() {
        install_function(proto, method, ptr, arity, arity, false);
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
        crate::closure::js_register_closure_arity(ptr, 0);
        crate::object::set_bound_native_closure_name(closure, &format!("get {getter_name}"));
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
        supported_locales_of_thunk as *const u8,
        0,
        1,
        true,
    );
    crate::closure::closure_set_dynamic_prop(ctor as usize, "supportedLocalesOf", supported);

    set_field(ns_obj, name, ctor_value);
    set_builtin_attrs(ns_obj, name, PropertyAttrs::new(true, false, true));
}

//! Ordered JSON and Reflect births, reached only through their feature hooks.
use super::bigint_promise::{
    json_is_raw_json_thunk, json_parse_thunk, json_raw_json_thunk, json_stringify_thunk,
    reflect_apply_thunk, reflect_construct_thunk, reflect_define_property_thunk,
    reflect_delete_property_thunk, reflect_get_own_property_descriptor_thunk,
    reflect_get_prototype_of_thunk, reflect_get_thunk, reflect_has_thunk,
    reflect_is_extensible_thunk, reflect_own_keys_thunk, reflect_prevent_extensions_thunk,
    reflect_set_prototype_of_thunk, reflect_set_thunk,
};
use super::builtin_namespace::{self, Member};
use crate::object::ObjectHeader;

// Preserve the existing own-key order and optional-argument function metadata.
const JSON_MEMBERS: &[Member] = &[
    Member::method(
        "parse",
        crate::fn_info!(#[realm(JSON.parse, births = [Array, Boolean, Number, Object, String])] json_parse_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "stringify",
        crate::fn_info!(#[realm(JSON.stringify)] json_stringify_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "rawJSON",
        crate::fn_info!(#[realm(JSON.rawJSON)] json_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "isRawJSON",
        crate::fn_info!(#[realm(JSON.isRawJSON)] json_is_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::tag("JSON"),
];
crate::realm_decl! { JSON, provider = namespace(JSON_MEMBERS), births = [Array, Boolean, Number, Object, String]; }
#[cold]
#[inline(never)]
pub(crate) fn create_json_namespace() -> *mut ObjectHeader {
    builtin_namespace::create(JSON_MEMBERS)
}

const REFLECT_MEMBERS: &[Member] = &[
    Member::method(
        "defineProperty",
        crate::fn_info!(#[realm(Reflect.defineProperty)] reflect_define_property_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "deleteProperty",
        crate::fn_info!(#[realm(Reflect.deleteProperty)] reflect_delete_property_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "apply",
        crate::fn_info!(#[realm(Reflect.apply)] reflect_apply_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "construct",
        crate::fn_info!(#[realm(Reflect.construct)] reflect_construct_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "get",
        crate::fn_info!(#[realm(Reflect.get)] reflect_get_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "getOwnPropertyDescriptor",
        crate::fn_info!(#[realm(Reflect.getOwnPropertyDescriptor)] reflect_get_own_property_descriptor_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "getPrototypeOf",
        crate::fn_info!(#[realm(Reflect.getPrototypeOf)] reflect_get_prototype_of_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "has",
        crate::fn_info!(#[realm(Reflect.has)] reflect_has_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "isExtensible",
        crate::fn_info!(#[realm(Reflect.isExtensible)] reflect_is_extensible_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "ownKeys",
        crate::fn_info!(#[realm(Reflect.ownKeys)] reflect_own_keys_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "preventExtensions",
        crate::fn_info!(#[realm(Reflect.preventExtensions)] reflect_prevent_extensions_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "set",
        crate::fn_info!(#[realm(Reflect.set)] reflect_set_thunk, 4; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "setPrototypeOf",
        crate::fn_info!(#[realm(Reflect.setPrototypeOf)] reflect_set_prototype_of_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::tag("Reflect"),
];
crate::realm_decl! { Reflect, provider = namespace(REFLECT_MEMBERS); }
#[cold]
#[inline(never)]
pub(crate) fn create_reflect_namespace() -> *mut ObjectHeader {
    builtin_namespace::create(REFLECT_MEMBERS)
}

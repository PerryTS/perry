//! Ordered JSON and Reflect births, reached only through their feature hooks.
use super::builtin_namespace::{self, Member};
use crate::object::ObjectHeader;

// Preserve the existing own-key order and optional-argument function metadata.
const JSON_MEMBERS: &[Member] = &[
    Member::method(
        "parse",
        crate::fn_info!(#[realm(JSON.parse, births = [Array, Boolean, Number, Object, String])] export crate::object::global_this::bigint_promise::json_parse_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "stringify",
        crate::fn_info!(#[realm(JSON.stringify)] export crate::object::global_this::bigint_promise::json_stringify_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "rawJSON",
        crate::fn_info!(#[realm(JSON.rawJSON)] export crate::object::global_this::bigint_promise::json_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "isRawJSON",
        crate::fn_info!(#[realm(JSON.isRawJSON)] export crate::object::global_this::bigint_promise::json_is_raw_json_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
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
        crate::fn_info!(#[realm(Reflect.defineProperty)] export crate::object::global_this::bigint_promise::reflect_define_property_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "deleteProperty",
        crate::fn_info!(#[realm(Reflect.deleteProperty)] export crate::object::global_this::bigint_promise::reflect_delete_property_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "apply",
        crate::fn_info!(#[realm(Reflect.apply)] export crate::object::global_this::bigint_promise::reflect_apply_thunk, 3; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "construct",
        crate::fn_info!(#[realm(Reflect.construct)] export crate::object::global_this::bigint_promise::reflect_construct_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "get",
        crate::fn_info!(#[realm(Reflect.get)] export crate::object::global_this::bigint_promise::reflect_get_thunk, 3; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "getOwnPropertyDescriptor",
        crate::fn_info!(#[realm(Reflect.getOwnPropertyDescriptor)] export crate::object::global_this::bigint_promise::reflect_get_own_property_descriptor_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "getPrototypeOf",
        crate::fn_info!(#[realm(Reflect.getPrototypeOf)] export crate::object::global_this::bigint_promise::reflect_get_prototype_of_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "has",
        crate::fn_info!(#[realm(Reflect.has)] export crate::object::global_this::bigint_promise::reflect_has_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
        2,
    ),
    Member::method(
        "isExtensible",
        crate::fn_info!(#[realm(Reflect.isExtensible)] export crate::object::global_this::bigint_promise::reflect_is_extensible_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "ownKeys",
        crate::fn_info!(#[realm(Reflect.ownKeys)] export crate::object::global_this::bigint_promise::reflect_own_keys_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "preventExtensions",
        crate::fn_info!(#[realm(Reflect.preventExtensions)] export crate::object::global_this::bigint_promise::reflect_prevent_extensions_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
        1,
    ),
    Member::method(
        "set",
        crate::fn_info!(#[realm(Reflect.set)] export crate::object::global_this::bigint_promise::reflect_set_thunk, 4; with_declared(3), with_flags(crate::closure::FN_BUILTIN)),
        3,
    ),
    Member::method(
        "setPrototypeOf",
        crate::fn_info!(#[realm(Reflect.setPrototypeOf)] export crate::object::global_this::bigint_promise::reflect_set_prototype_of_thunk, 2; with_declared(2), with_flags(crate::closure::FN_BUILTIN)),
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

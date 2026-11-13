//! Built-in families whose members a crate outside the runtime provides
//! (declared `external true` in `global_this_tables.rs`, always `lazy`).
//!
//! The realm declares the family and binds its constructor; its prototype is
//! born on first observation (`materialize_family`'s birth arm). At that birth the runtime
//! calls the one registered provider hook with the family identity and the
//! prototype; the provider installs the members with the generic define
//! primitives below. A program that links no provider gets the declared
//! family with `constructor` and its tag only, and carries none of the
//! provider's code.
//!
//! The provider reaches this module through the stable C ABI only: a stdlib
//! archive carries its own copy of this crate's Rust items, so a direct Rust
//! call from it would set that copy's hook and allocate in that copy, never
//! the program's runtime.

use crate::closure::JsFunctionInfo;
use crate::object::ObjectHeader;

/// The provider's member install for one family prototype (the family is a
/// `BuiltinConstructorIdentity` discriminant).
pub type BirthFn = extern "C" fn(u8, *mut ObjectHeader);
/// The provider's `new F(arg)` for a constructible external family.
pub type ConstructFn = extern "C" fn(u8, f64) -> f64;

static BIRTH: crate::feature_hooks::Hook<BirthFn> = crate::feature_hooks::Hook::empty();
static CONSTRUCT: crate::feature_hooks::Hook<ConstructFn> = crate::feature_hooks::Hook::empty();

/// Registers the provider of the external families (one per program).
#[no_mangle]
pub extern "C" fn js_runtime_provide_external_families(birth: BirthFn, construct: ConstructFn) {
    BIRTH.set(birth);
    CONSTRUCT.set(construct);
}

/// The family birth step for an `external` family: the provider's members.
/// External families are `lazy`, so this runs inside `materialize_family`'s birth arm,
/// which already holds the no-move scope the provider's allocations need.
pub(crate) fn birth(identity: crate::object::BuiltinConstructorIdentity, proto: *mut ObjectHeader) {
    if let Some(birth) = BIRTH.peek() {
        birth(identity as u8, proto);
    }
}

/// `new F(arg)` by the provider; `None` when no provider is linked.
pub(crate) fn construct(
    identity: crate::object::BuiltinConstructorIdentity,
    args: &[f64],
) -> Option<f64> {
    let construct = CONSTRUCT.peek()?;
    let arg = args
        .first()
        .copied()
        .unwrap_or(f64::from_bits(crate::value::TAG_UNDEFINED));
    Some(construct(identity as u8, arg))
}

/// # Safety
/// `ptr`/`len` are UTF-8.
unsafe fn utf8<'a>(ptr: *const u8, len: usize) -> &'a str {
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
}

/// A built-in method on `target` (an object or a function): a
/// non-constructor function named `name` with length `arity`, defined as an
/// own data property with `attr_bits` (`PropertyAttrs` bits). Returns it.
///
/// # Safety
/// `target` is live; `name_ptr`/`name_len` UTF-8; `info` static.
#[no_mangle]
pub unsafe extern "C" fn js_builtin_define_method(
    target: *mut ObjectHeader,
    name_ptr: *const u8,
    name_len: usize,
    info: *const JsFunctionInfo,
    arity: u32,
    attr_bits: u8,
) -> f64 {
    let name = utf8(name_ptr, name_len);
    let closure = crate::closure::js_closure_alloc(info, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::object::native_module::set_bound_native_closure_metadata(closure, name, arity);
    crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    crate::object::descriptor_state::define_builtin_data_property_bytes(
        target,
        key,
        value,
        name.as_bytes(),
        crate::object::PropertyAttrs { bits: attr_bits },
    );
    value
}

/// A built-in getter `name` on `target` (`get <name>`, no setter),
/// configurable, enumerable when the interface says so.
///
/// # Safety
/// As [`js_builtin_define_method`].
#[no_mangle]
pub unsafe extern "C" fn js_builtin_define_getter(
    target: *mut ObjectHeader,
    name_ptr: *const u8,
    name_len: usize,
    info: *const JsFunctionInfo,
    enumerable: bool,
) {
    let name = utf8(name_ptr, name_len);
    crate::event_target::prototype::accessor(target, name, info, None, enumerable);
}

/// `target[Symbol.<symbol_name>] = value` as a method alias (`{ writable,
/// configurable }`): the same function object as a named member.
///
/// # Safety
/// `target` is live; `symbol_ptr`/`symbol_len` UTF-8.
#[no_mangle]
pub unsafe extern "C" fn js_builtin_define_symbol_method(
    target: *mut ObjectHeader,
    symbol_ptr: *const u8,
    symbol_len: usize,
    value: f64,
) {
    let symbol = utf8(symbol_ptr, symbol_len);
    crate::object::install_builtin_well_known_symbol_method(target, symbol, value);
}

/// Unit tests stand in for a provider: the `ReadableStream` family gets a
/// `from` static (the stdlib's `ReadableStream.from` install, in miniature).
#[cfg(test)]
pub(crate) fn provide_from_for_tests(info: *const JsFunctionInfo) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INFO: AtomicUsize = AtomicUsize::new(0);
    INFO.store(info as usize, Ordering::Release);
    extern "C" fn birth(family: u8, proto: *mut ObjectHeader) {
        if family != crate::object::BuiltinConstructorIdentity::ReadableStream as u8 {
            return;
        }
        let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let ctor = crate::object::js_object_get_field_by_name(proto, key);
        let ctor = crate::value::js_nanbox_get_pointer(f64::from_bits(ctor.bits()));
        let info = INFO.load(Ordering::Acquire) as *const JsFunctionInfo;
        let attrs = crate::object::PropertyAttrs::new(true, true, true).bits;
        unsafe {
            js_builtin_define_method(
                ctor as *mut ObjectHeader,
                b"from".as_ptr(),
                4,
                info,
                1,
                attrs,
            )
        };
    }
    extern "C" fn construct(_: u8, _: f64) -> f64 {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
    js_runtime_provide_external_families(birth, construct);
}

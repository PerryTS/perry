//! Primitive-wrapper prototype-method thunks (#4100, part of the #3662 cluster).
//!
//! `Number.prototype.valueOf` & friends are reachable as plain values (e.g.
//! `Number.prototype.valueOf.call(x)`, `Reflect.apply`, method extraction).
//! The fast path `n.valueOf()` is lowered directly by codegen and never touches
//! these thunks, so they only fire on the reflective path — which previously
//! resolved to `global_this_builtin_noop_thunk` (Number/Boolean) or fell back
//! to `Object.prototype.toString` (Symbol/BigInt, whose prototypes had no own
//! methods). Both produced `"[object Object]"`/`"[object Symbol]"` instead of
//! the spec behaviour.
//!
//! Per spec these methods must perform a `this` brand check and throw a
//! `TypeError` when called on an incompatible receiver. The thunks below read
//! the `IMPLICIT_THIS` receiver (set by the `.call`/`.apply` dispatch),
//! brand-check it against the matching primitive (or its boxed wrapper), throw
//! on mismatch, and otherwise re-dispatch to the canonical per-type logic via
//! `js_native_call_method` — which also returns the correct value, fixing the
//! wrong-value reflective `toString` (`Symbol("x").toString()` → `"Symbol(x)"`,
//! `(5n).toString(2)` → `"101"`).
//!
//! Installed onto each wrapper's `.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

/// Throw `TypeError: Method <proto>.<method> called on incompatible receiver`.
/// Mirrors the collection-thunk wording; Test262's brand-check tests assert
/// only the error *type*, so the exact message is informational. Never returns.
fn throw_incompatible_receiver(proto: &str, method: &str) -> ! {
    let msg = format!("Method {proto}.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

fn receiver_bits() -> f64 {
    f64::from_bits(IMPLICIT_THIS.with(|c| c.get()))
}

#[inline]
fn number_receiver_or_throw(method: &str) -> f64 {
    let this = receiver_bits();
    let jsv = crate::value::JSValue::from_bits(this.to_bits());
    if jsv.is_number()
        || jsv.is_int32()
        || crate::builtins::boxed_primitive_to_string_tag(this) == Some("Number")
    {
        this
    } else {
        throw_incompatible_receiver("Number.prototype", method)
    }
}

#[inline]
fn boolean_receiver_or_throw(method: &str) -> f64 {
    let this = receiver_bits();
    let jsv = crate::value::JSValue::from_bits(this.to_bits());
    if jsv.is_bool() || crate::builtins::boxed_primitive_to_string_tag(this) == Some("Boolean") {
        this
    } else {
        throw_incompatible_receiver("Boolean.prototype", method)
    }
}

#[inline]
fn symbol_receiver_or_throw(method: &str) -> f64 {
    let this = receiver_bits();
    let is_symbol = unsafe { crate::symbol::js_is_symbol(this) != 0 };
    if is_symbol || crate::builtins::boxed_primitive_to_string_tag(this) == Some("Symbol") {
        this
    } else {
        throw_incompatible_receiver("Symbol.prototype", method)
    }
}

#[inline]
fn bigint_receiver_or_throw(method: &str) -> f64 {
    let this = receiver_bits();
    let jsv = crate::value::JSValue::from_bits(this.to_bits());
    if jsv.is_bigint() || crate::builtins::boxed_primitive_to_string_tag(this) == Some("BigInt") {
        this
    } else {
        throw_incompatible_receiver("BigInt.prototype", method)
    }
}

/// Re-dispatch the brand-checked receiver to the canonical method logic, which
/// also produces the correct value for the wrong-value reflective `toString`.
fn redispatch(this: f64, method: &str, args: &[f64]) -> f64 {
    unsafe {
        super::js_native_call_method(
            this,
            method.as_ptr() as *const i8,
            method.len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

pub(super) extern "C" fn number_proto_value_of_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = number_receiver_or_throw("valueOf");
    redispatch(this, "valueOf", &[])
}

pub(super) extern "C" fn number_proto_to_locale_string_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = number_receiver_or_throw("toLocaleString");
    redispatch(this, "toLocaleString", &[])
}

pub(super) extern "C" fn boolean_proto_to_string_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = boolean_receiver_or_throw("toString");
    redispatch(this, "toString", &[])
}

pub(super) extern "C" fn boolean_proto_value_of_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = boolean_receiver_or_throw("valueOf");
    redispatch(this, "valueOf", &[])
}

pub(super) extern "C" fn symbol_proto_to_string_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = symbol_receiver_or_throw("toString");
    redispatch(this, "toString", &[])
}

pub(super) extern "C" fn symbol_proto_value_of_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = symbol_receiver_or_throw("valueOf");
    redispatch(this, "valueOf", &[])
}

pub(super) extern "C" fn bigint_proto_to_string_thunk(
    _c: *const crate::closure::ClosureHeader,
    radix: f64,
) -> f64 {
    let this = bigint_receiver_or_throw("toString");
    // Forward an explicit radix; a missing arg arrives as `undefined`, which the
    // canonical BigInt `toString` arm treats as decimal.
    redispatch(this, "toString", &[radix])
}

pub(super) extern "C" fn bigint_proto_value_of_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let this = bigint_receiver_or_throw("valueOf");
    redispatch(this, "valueOf", &[])
}

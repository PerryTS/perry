//! Primitive wrapper prototype-method thunks.
//!
//! These methods are observable as function values
//! (`Number.prototype.valueOf.call(x)`, `Symbol.prototype.toString.call(x)`)
//! and must brand-check their `this` receiver instead of falling through to
//! Object defaults.

use super::*;

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_00D0;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_00D1;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_00D2;
const CLASS_ID_BOXED_BIGINT: u32 = 0xFFFF_00D3;
const CLASS_ID_BOXED_SYMBOL: u32 = 0xFFFF_00D4;

pub(super) fn install_primitive_proto_methods(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) -> bool {
    use super::global_this::install_proto_method as ipm;

    match builtin_name {
        "Number" => {
            ipm(
                proto_obj,
                "toExponential",
                number_proto_to_exponential_thunk as *const u8,
                1,
            );
            ipm(
                proto_obj,
                "toFixed",
                number_proto_to_fixed_thunk as *const u8,
                1,
            );
            ipm(
                proto_obj,
                "toLocaleString",
                number_proto_to_locale_string_thunk as *const u8,
                0,
            );
            ipm(
                proto_obj,
                "toPrecision",
                number_proto_to_precision_thunk as *const u8,
                1,
            );
            ipm(
                proto_obj,
                "toString",
                number_proto_to_string_thunk as *const u8,
                1,
            );
            ipm(
                proto_obj,
                "valueOf",
                number_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Boolean" => {
            ipm(
                proto_obj,
                "toString",
                boolean_proto_to_string_thunk as *const u8,
                0,
            );
            ipm(
                proto_obj,
                "valueOf",
                boolean_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "Symbol" => {
            ipm(
                proto_obj,
                "toString",
                symbol_proto_to_string_thunk as *const u8,
                0,
            );
            ipm(
                proto_obj,
                "valueOf",
                symbol_proto_value_of_thunk as *const u8,
                0,
            );
        }
        "BigInt" => {
            ipm(
                proto_obj,
                "toString",
                bigint_proto_to_string_thunk as *const u8,
                1,
            );
            ipm(
                proto_obj,
                "valueOf",
                bigint_proto_value_of_thunk as *const u8,
                0,
            );
            // `.length` is 0 despite the optional `(locales?, options?)`
            // params (both are optional, so neither counts per the built-in
            // function `.length` rule) — installed rest-based like
            // `Date.prototype.toLocaleString` so the fixed call arity stays 0
            // while `locales`/`options` still arrive via `rest`.
            super::global_this::install_proto_method_rest_with_length(
                proto_obj,
                "toLocaleString",
                bigint_proto_to_locale_string_thunk as *const u8,
                0,
                0,
            );
        }
        _ => return false,
    }
    true
}

pub(crate) fn primitive_proto_method_value(builtin_name: &str, method_name: &str) -> Option<f64> {
    let (func_ptr, arity) = match (builtin_name, method_name) {
        ("Number", "toExponential") => (number_proto_to_exponential_thunk as *const u8, 1),
        ("Number", "toFixed") => (number_proto_to_fixed_thunk as *const u8, 1),
        ("Number", "toLocaleString") => (number_proto_to_locale_string_thunk as *const u8, 0),
        ("Number", "toPrecision") => (number_proto_to_precision_thunk as *const u8, 1),
        ("Number", "toString") => (number_proto_to_string_thunk as *const u8, 1),
        ("Number", "valueOf") => (number_proto_value_of_thunk as *const u8, 0),
        ("Boolean", "toString") => (boolean_proto_to_string_thunk as *const u8, 0),
        ("Boolean", "valueOf") => (boolean_proto_value_of_thunk as *const u8, 0),
        ("Symbol", "toString") => (symbol_proto_to_string_thunk as *const u8, 0),
        ("Symbol", "valueOf") => (symbol_proto_value_of_thunk as *const u8, 0),
        ("BigInt", "toString") => (bigint_proto_to_string_thunk as *const u8, 1),
        ("BigInt", "valueOf") => (bigint_proto_value_of_thunk as *const u8, 0),
        _ => return None,
    };
    Some(primitive_proto_method_closure_value(
        method_name,
        func_ptr,
        arity,
    ))
}

fn primitive_proto_method_closure_value(method_name: &str, func_ptr: *const u8, arity: u32) -> f64 {
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::native_module::set_bound_native_closure_name(closure, method_name);
    super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::PropertyAttrs::new(false, false, true),
    );
    crate::value::js_nanbox_pointer(closure as i64)
}

fn receiver_value() -> f64 {
    f64::from_bits(IMPLICIT_THIS.with(|c| c.get()))
}

fn throw_incompatible_receiver(proto: &str, method: &str) -> ! {
    let msg = format!("{proto}.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn boxed_payload(receiver: f64, expected_class_id: u32) -> Option<f64> {
    let (class_id, payload) = crate::builtins::boxed_primitive_payload(receiver)?;
    (class_id == expected_class_id).then_some(payload)
}

fn number_receiver_or_throw(method: &str) -> f64 {
    let receiver = receiver_value();
    if let Some(payload) = boxed_payload(receiver, CLASS_ID_BOXED_NUMBER) {
        return payload;
    }
    let jv = crate::value::JSValue::from_bits(receiver.to_bits());
    if jv.is_int32() {
        return jv.as_int32() as f64;
    }
    if jv.is_number() {
        return receiver;
    }
    // ECMA-262 21.1.3: `Number.prototype` is itself a Number object whose
    // [[NumberData]] is +0. A method invoked with `this === Number.prototype`
    // (e.g. `Number.prototype == 0` coercing via `valueOf`, or
    // `Number.prototype.toString()`) must therefore see +0, not throw an
    // incompatible-receiver TypeError (test262 prototype/S15.7.3.1_A3.js,
    // S15.7.4_A1.js).
    if receiver.to_bits() == super::global_this::builtin_prototype_value("Number").to_bits() {
        return 0.0;
    }
    throw_incompatible_receiver("Number.prototype", method)
}

fn boolean_receiver_or_throw(method: &str) -> f64 {
    let receiver = receiver_value();
    if let Some(payload) = boxed_payload(receiver, CLASS_ID_BOXED_BOOLEAN) {
        return payload;
    }
    let jv = crate::value::JSValue::from_bits(receiver.to_bits());
    if jv.is_bool() {
        return receiver;
    }
    // ECMA-262 20.3.3: `Boolean.prototype` is itself a Boolean object whose
    // [[BooleanData]] is `false`. A method invoked with `this === Boolean.prototype`
    // (`Boolean.prototype.valueOf()`, or `Boolean.prototype == false` coercing via
    // valueOf) sees `false` rather than throwing (test262 prototype/valueOf/A1, S15.6.3.1_A1).
    if receiver.to_bits() == super::global_this::builtin_prototype_value("Boolean").to_bits() {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    throw_incompatible_receiver("Boolean.prototype", method)
}

fn symbol_receiver_or_throw(method: &str) -> f64 {
    let receiver = receiver_value();
    if let Some(payload) = boxed_payload(receiver, CLASS_ID_BOXED_SYMBOL) {
        return payload;
    }
    if unsafe { crate::symbol::js_is_symbol(receiver) } != 0 {
        return receiver;
    }
    throw_incompatible_receiver("Symbol.prototype", method)
}

fn bigint_receiver_or_throw(method: &str) -> f64 {
    let receiver = receiver_value();
    if let Some(payload) = boxed_payload(receiver, CLASS_ID_BOXED_BIGINT) {
        return payload;
    }
    if crate::value::JSValue::from_bits(receiver.to_bits()).is_bigint() {
        return receiver;
    }
    throw_incompatible_receiver("BigInt.prototype", method)
}

/// ECMA-262 `thisStringValue(value)`: accept a String primitive or a boxed
/// `String` object; throw `TypeError` for anything else, including `null`,
/// `undefined`, numbers, booleans, symbols, and plain objects.
fn string_receiver_or_throw(method: &str) -> f64 {
    let receiver = receiver_value();
    let jv = crate::value::JSValue::from_bits(receiver.to_bits());
    // String primitive (heap or SSO inline).
    if jv.is_string() || jv.is_short_string() {
        return receiver;
    }
    // Boxed String wrapper (`new String("x")`): return the underlying string.
    if let Some(payload) = boxed_payload(receiver, CLASS_ID_BOXED_STRING) {
        return payload;
    }
    // `String.prototype` itself is a String exotic object with [[StringData]] = "".
    if receiver.to_bits() == super::global_this::builtin_prototype_value("String").to_bits() {
        let empty = crate::string::js_string_from_bytes(std::ptr::null(), 0);
        return f64::from_bits(crate::value::JSValue::string_ptr(empty).bits());
    }
    throw_incompatible_receiver("String.prototype", method)
}

fn string_value(ptr: *mut crate::string::StringHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

pub(super) extern "C" fn number_proto_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    number_receiver_or_throw("valueOf")
}

pub(super) extern "C" fn number_proto_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    radix: f64,
) -> f64 {
    string_value(crate::value::js_jsvalue_to_string_radix(
        number_receiver_or_throw("toString"),
        radix,
    ))
}

pub(super) extern "C" fn number_proto_to_fixed_thunk(
    _closure: *const crate::closure::ClosureHeader,
    decimals: f64,
) -> f64 {
    string_value(crate::string::js_number_to_fixed(
        number_receiver_or_throw("toFixed"),
        decimals,
    ))
}

pub(super) extern "C" fn number_proto_to_precision_thunk(
    _closure: *const crate::closure::ClosureHeader,
    precision: f64,
) -> f64 {
    string_value(crate::string::js_number_to_precision(
        number_receiver_or_throw("toPrecision"),
        precision,
    ))
}

pub(super) extern "C" fn number_proto_to_exponential_thunk(
    _closure: *const crate::closure::ClosureHeader,
    decimals: f64,
) -> f64 {
    string_value(crate::string::js_number_to_exponential(
        number_receiver_or_throw("toExponential"),
        decimals,
    ))
}

pub(super) extern "C" fn number_proto_to_locale_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let n = number_receiver_or_throw("toLocaleString");
    let s = crate::date::js_number_to_locale_string(n);
    string_value(s)
}

pub(super) extern "C" fn boolean_proto_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    boolean_receiver_or_throw("valueOf")
}

pub(super) extern "C" fn boolean_proto_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let value = boolean_receiver_or_throw("toString");
    let bytes = if crate::value::JSValue::from_bits(value.to_bits()).as_bool() {
        b"true".as_slice()
    } else {
        b"false".as_slice()
    };
    let s = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    string_value(s)
}

pub(super) extern "C" fn symbol_proto_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    symbol_receiver_or_throw("valueOf")
}

pub(super) extern "C" fn symbol_proto_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let symbol = symbol_receiver_or_throw("toString");
    let s =
        unsafe { crate::symbol::js_symbol_to_string(symbol) } as *mut crate::string::StringHeader;
    string_value(s)
}

pub(super) extern "C" fn bigint_proto_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    bigint_receiver_or_throw("valueOf")
}

pub(super) extern "C" fn bigint_proto_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    radix: f64,
) -> f64 {
    string_value(crate::value::js_jsvalue_to_string_radix(
        bigint_receiver_or_throw("toString"),
        radix,
    ))
}

pub(super) extern "C" fn bigint_proto_to_locale_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let value = bigint_receiver_or_throw("toLocaleString");
    let args = super::global_this::global_this_rest_array_values(rest);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let locales = args.first().copied().unwrap_or(undef);
    let options = args.get(1).copied().unwrap_or(undef);
    string_value(crate::intl::bigint_to_locale_string(
        value, locales, options,
    ))
}

/// `String.prototype.toString()` — brand-checked: returns the underlying string
/// primitive for String primitives and boxed `String` objects; throws a
/// `TypeError` for any other receiver.
pub(super) extern "C" fn string_proto_to_string_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    string_receiver_or_throw("toString")
}

/// `String.prototype.valueOf()` — brand-checked, identical semantics to
/// `toString()`: returns the [[StringData]] of the receiver.
pub(super) extern "C" fn string_proto_value_of_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    string_receiver_or_throw("valueOf")
}

//! #11896: built-in constructors reached as VALUES (`const B = BigInt; B(3)`,
//! `[1].map(BigInt)`, `Map.groupBy` read off the constructor, `x instanceof
//! WeakRef`) answer like the direct forms do.

use super::*;
use crate::closure::{call_value, plain_call_receiver};
use crate::value::{js_nanbox_string, JSValue};

fn builtin(name: &str) -> f64 {
    js_get_global_this_builtin_value(name.as_ptr(), name.len())
}

fn static_member(ctor: f64, name: &str) -> f64 {
    unsafe { crate::value::js_dynamic_object_get_property(ctor, name.as_ptr().cast(), name.len()) }
}

fn string(s: &str) -> f64 {
    js_nanbox_string(crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64)
}

fn call(func: f64, args: &[f64]) -> f64 {
    unsafe { call_value(func, plain_call_receiver(), args) }
}

fn string_of(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    assert!(
        jv.is_any_string(),
        "expected a string, got {:#x}",
        value.to_bits()
    );
    crate::string::with_string_value_bytes(value, |bytes| {
        String::from_utf8_lossy(bytes).into_owned()
    })
    .expect("a string value has bytes")
}

#[test]
fn bigint_value_call_coerces_like_the_direct_form() {
    let big = builtin("BigInt");
    for (arg, expect) in [(3.0, "3"), (0.0, "0"), (-7.0, "-7")] {
        let got = call(big, &[arg]);
        assert!(
            JSValue::from_bits(got.to_bits()).is_bigint(),
            "BigInt({arg})"
        );
        let text =
            crate::bigint::js_bigint_to_string(JSValue::from_bits(got.to_bits()).as_bigint_ptr());
        assert_eq!(
            string_of(js_nanbox_string(text as i64)),
            expect,
            "BigInt({arg}) through the value"
        );
    }
    let from_string = call(big, &[string("12345678901234567890")]);
    assert!(JSValue::from_bits(from_string.to_bits()).is_bigint());
}

#[test]
fn symbol_value_call_makes_a_fresh_symbol_with_its_description() {
    let sym = builtin("Symbol");
    let a = call(sym, &[string("x")]);
    let b = call(sym, &[string("x")]);
    assert_eq!(
        unsafe { crate::symbol::js_is_symbol(a) },
        1,
        "Symbol('x') is a symbol"
    );
    assert_ne!(a.to_bits(), b.to_bits(), "every call is a fresh symbol");
    let none = call(sym, &[f64::from_bits(crate::value::TAG_UNDEFINED)]);
    assert_eq!(
        unsafe { crate::symbol::js_is_symbol(none) },
        1,
        "Symbol() is a symbol"
    );
}

#[test]
fn bigint_and_symbol_values_stay_recognised_as_constructors() {
    for name in ["BigInt", "Symbol"] {
        assert_eq!(
            class_registry::identify_global_builtin_constructor(builtin(name)),
            Some(name),
            "{name} must keep its constructor identity now that it has its own call thunk"
        );
    }
}

#[test]
fn map_group_by_and_regexp_escape_are_statics_of_their_constructors() {
    let group_by = static_member(builtin("Map"), "groupBy");
    assert!(
        JSValue::from_bits(group_by.to_bits()).is_pointer(),
        "Map.groupBy must be a function value"
    );
    let items = crate::array::js_array_alloc(0);
    let items = crate::value::js_nanbox_pointer(items as i64);
    let cb = static_member(builtin("Object"), "keys");
    // A non-callable callback is a TypeError, so the thunk reaches the real
    // `Map.groupBy`: calling with a valid callback and no items yields a Map.
    let map = call(group_by, &[items, cb]);
    assert!(
        crate::map::is_registered_map(crate::value::js_nanbox_get_pointer(map) as usize),
        "Map.groupBy(items, cb) through the value returns a Map"
    );

    let escape = static_member(builtin("RegExp"), "escape");
    assert!(
        JSValue::from_bits(escape.to_bits()).is_pointer(),
        "RegExp.escape must be a function value"
    );
    assert_eq!(string_of(call(escape, &[string("a.b")])), "\\x61\\.b");
    assert_eq!(string_of(call(escape, &[string("1+")])), "\\x31\\+");
}

#[test]
fn weakref_and_finalization_registry_ids_are_their_own() {
    use crate::weakref::{CLASS_ID_FINALIZATION_REGISTRY, CLASS_ID_WEAKREF};
    // perry-codegen/src/expr/instance_misc1.rs names these two ids for
    // `x instanceof WeakRef` / `FinalizationRegistry`.
    assert_eq!(CLASS_ID_WEAKREF, 0xFFFF_0064);
    assert_eq!(CLASS_ID_FINALIZATION_REGISTRY, 0xFFFF_0065);
    assert_eq!(
        instanceof::global_builtin_constructor_class_id("WeakRef"),
        CLASS_ID_WEAKREF
    );
    assert_eq!(
        instanceof::global_builtin_constructor_class_id("FinalizationRegistry"),
        CLASS_ID_FINALIZATION_REGISTRY
    );
    // They once equalled the fetch probe ids of Request (0x29) / Headers (0x2A).
    for fetch_id in [
        0xFFFF_0028u32,
        0xFFFF_0029,
        0xFFFF_002A,
        0xFFFF_002B,
        0xFFFF_002C,
    ] {
        assert_ne!(CLASS_ID_WEAKREF, fetch_id);
        assert_ne!(CLASS_ID_FINALIZATION_REGISTRY, fetch_id);
    }

    let target = crate::object::js_object_alloc(0, 0);
    let target = f64::from_bits(JSValue::pointer(target as *const u8).bits());
    let weak = crate::weakref::js_weakref_new(target);
    let weak = f64::from_bits(JSValue::pointer(weak as *const u8).bits());
    let truthy = |v: f64| v.to_bits() == 0x7FFC_0000_0000_0004;
    assert!(
        truthy(js_instanceof(weak, CLASS_ID_WEAKREF)),
        "a WeakRef is an instanceof WeakRef"
    );
    assert!(!truthy(js_instanceof(weak, CLASS_ID_FINALIZATION_REGISTRY)));
    assert!(
        !truthy(js_instanceof(weak, 0xFFFF_0029)),
        "a WeakRef is not a Request"
    );
    assert!(
        !truthy(js_instanceof(target, CLASS_ID_WEAKREF)),
        "a plain object is not a WeakRef"
    );
}

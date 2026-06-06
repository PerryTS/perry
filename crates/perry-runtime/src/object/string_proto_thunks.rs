//! `String.prototype` generic-`this` method thunks.
//!
//! These methods are observable as function *values*
//! (`String.prototype.charAt.call(receiver, i)`, `__obj.charAt = String.
//! prototype.charAt; __obj.charAt(i)`) and per ECMA-262 §22.1.3 each begins
//! with `RequireObjectCoercible(this)` followed by `ToString(this)` — so a
//! boxed `new Boolean(false)` receiver coerces to `"false"`, a `{ toString }`
//! object coerces via its method, and a `null`/`undefined` receiver throws a
//! `TypeError`.
//!
//! Without a real thunk these were installed as the shared
//! `global_this_builtin_noop_thunk`; a reflective `.call(receiver)` then
//! re-dispatched *by name on the receiver's own type* (a boolean has no
//! `charAt`), throwing `(boolean).charAt is not a function`. The direct, typed
//! `"abc".charAt(1)` fast path (codegen `lower_string_method.rs`) and the
//! any-typed-string dispatch arm in `native_call_method.rs` are unaffected —
//! they never read `String.prototype`.

use super::*;

/// Methods routed through real thunks. Kept in lock-step with the HIR
/// `.call`/`.apply` fold exclusion (`is_string_prototype_generic_method`) so the
/// reflective path actually reaches these thunks instead of being rewritten to
/// `receiver.<method>()`.
pub(super) fn install_string_proto_methods(
    builtin_name: &str,
    proto_obj: *mut ObjectHeader,
) -> bool {
    if builtin_name != "String" {
        return false;
    }
    use super::global_this::install_proto_method as ipm;
    ipm(proto_obj, "at", string_proto_at_thunk as *const u8, 1);
    ipm(
        proto_obj,
        "charAt",
        string_proto_char_at_thunk as *const u8,
        1,
    );
    ipm(
        proto_obj,
        "charCodeAt",
        string_proto_char_code_at_thunk as *const u8,
        1,
    );
    ipm(
        proto_obj,
        "codePointAt",
        string_proto_code_point_at_thunk as *const u8,
        1,
    );
    install_string_iterator_symbol(proto_obj);
    true
}

/// Install `String.prototype[Symbol.iterator]` as a real, reflectable function
/// value (`{ writable: true, enumerable: false, configurable: true }`, `name`
/// `"[Symbol.iterator]"`, `length` 0). The thunk applies RequireObjectCoercible
/// + ToString to `this` and returns a codepoint-aware String iterator. Mirrors
/// the Map/Set iterator exposure (#4576).
fn install_string_iterator_symbol(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let iter = crate::symbol::well_known_symbol("iterator");
    if iter.is_null() {
        return;
    }
    let func_ptr = string_proto_symbol_iterator_thunk as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    super::native_module::set_bound_native_closure_name(closure, "[Symbol.iterator]");
    super::native_module::set_builtin_closure_length(closure as usize, 0);
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
    let method_value = crate::value::js_nanbox_pointer(closure as i64);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto_obj as i64),
            f64::from_bits(crate::value::JSValue::pointer(iter as *const u8).bits()),
            method_value,
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto_obj as usize,
        iter as usize,
        super::PropertyAttrs::new(true, false, true),
    );
}

fn throw_string_proto_nullish(method: &str) -> ! {
    let msg = format!("String.prototype.{method} called on null or undefined");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `RequireObjectCoercible(this)` + `ToString(this)` for the generic-`this`
/// String.prototype methods. `this` is the IMPLICIT_THIS receiver bound by
/// `.call`/`.apply`/property dispatch. `null`/`undefined` throw `TypeError`;
/// everything else (a primitive string, a boxed `String`/`Boolean`/`Number`
/// object, a `{ toString }` object) coerces via the shared `js_string_coerce`,
/// which runs user `toString`/`valueOf`.
fn string_this_or_throw(method: &str) -> *mut crate::string::StringHeader {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let jv = crate::value::JSValue::from_bits(this.to_bits());
    if jv.is_undefined() || jv.is_null() {
        throw_string_proto_nullish(method);
    }
    // ToString(Symbol) is a TypeError (abstract `ToString`, ECMA-262 §7.1.17) —
    // `String.prototype.codePointAt.call(Symbol(), 1)` must throw, not stringify
    // to `"Symbol(...)"`. Symbols are NaN-boxed pointers, so guard before the
    // generic `js_string_coerce` (which would render the description).
    if unsafe { crate::symbol::js_is_symbol(this) } != 0 {
        crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
    }
    crate::builtins::js_string_coerce(this)
}

pub(super) extern "C" fn string_proto_char_at_thunk(
    _c: *const crate::closure::ClosureHeader,
    pos: f64,
) -> f64 {
    let s = string_this_or_throw("charAt");
    let idx = crate::string::js_string_index_to_i32(pos);
    let r = crate::string::js_string_char_at(s, idx);
    f64::from_bits(crate::value::JSValue::string_ptr(r).bits())
}

pub(super) extern "C" fn string_proto_char_code_at_thunk(
    _c: *const crate::closure::ClosureHeader,
    pos: f64,
) -> f64 {
    let s = string_this_or_throw("charCodeAt");
    let idx = crate::string::js_string_index_to_i32(pos);
    crate::string::js_string_char_code_at(s, idx)
}

pub(super) extern "C" fn string_proto_code_point_at_thunk(
    _c: *const crate::closure::ClosureHeader,
    pos: f64,
) -> f64 {
    let s = string_this_or_throw("codePointAt");
    let idx = crate::string::js_string_index_to_i32(pos);
    crate::string::js_string_code_point_at(s, idx)
}

pub(super) extern "C" fn string_proto_at_thunk(
    _c: *const crate::closure::ClosureHeader,
    index: f64,
) -> f64 {
    let s = string_this_or_throw("at");
    let idx = crate::string::js_string_index_to_i32(index);
    crate::string::js_string_at(s, idx)
}

pub(super) extern "C" fn string_proto_symbol_iterator_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let s = string_this_or_throw("[Symbol.iterator]");
    crate::string::string_values_iter(s)
}

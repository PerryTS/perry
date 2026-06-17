//! Minimal ECMAScript ObjectEnvironmentRecord helpers for `with`.
//!
//! The compiler lowers `with (obj) { ident }` reads/writes into explicit
//! probes of the captured object value. These helpers provide the runtime
//! pieces that are genuinely dynamic: prototype-chain HasProperty,
//! `Symbol.unscopables`, property reads, and strict PutValue rechecks.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{js_is_truthy, js_nanbox_pointer, js_nanbox_string, JSValue};

fn throw_type_error(message: &'static [u8]) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[inline]
fn key_as_value(key: *const StringHeader) -> f64 {
    js_nanbox_string(key as i64)
}

/// `ToObject(bindings)` as the `with` object environment record: null /
/// undefined throw a TypeError; a primitive (number / string / boolean /
/// bigint / symbol) is boxed into its wrapper object; an object passes through.
/// Returns the (possibly boxed) value as a NaN-boxed-pointer f64 so callers can
/// use it for both `HasProperty` probes and field access on a single, stable
/// object. Without the boxing, `with (2)` / `with ("str")` threw instead of
/// probing the wrapper's own properties (and falling back to the outer scope).
#[inline]
fn to_object_bindings(bindings: f64) -> f64 {
    let value = JSValue::from_bits(bindings.to_bits());
    if value.is_null() || value.is_undefined() {
        throw_type_error(b"Cannot convert undefined or null to object");
    }
    if value.is_pointer() {
        return bindings;
    }
    crate::object::alloc::js_object_coerce(bindings)
}

#[inline]
fn object_ptr(bindings: f64) -> *mut ObjectHeader {
    let coerced = to_object_bindings(bindings);
    let coerced_val = JSValue::from_bits(coerced.to_bits());
    if !coerced_val.is_pointer() {
        throw_type_error(b"with object environment requires an object");
    }
    let ptr = coerced_val.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        throw_type_error(b"with object environment requires an object");
    }
    ptr
}

#[inline]
fn has_property(bindings: f64, key: *const StringHeader) -> bool {
    !key.is_null() && js_is_truthy(js_object_has_property(bindings, key_as_value(key))) != 0
}

#[no_mangle]
pub extern "C" fn js_with_has_binding(bindings: f64, key: *const StringHeader) -> i32 {
    if key.is_null() {
        return 0;
    }
    let bindings = to_object_bindings(bindings);
    if !has_property(bindings, key) {
        return 0;
    }

    let unscopables_symbol = crate::symbol::well_known_symbol("unscopables");
    let unscopables_symbol_value = js_nanbox_pointer(unscopables_symbol as i64);
    let unscopables =
        unsafe { crate::symbol::js_object_get_symbol_property(bindings, unscopables_symbol_value) };
    let unscopables_value = JSValue::from_bits(unscopables.to_bits());
    if unscopables_value.is_pointer() {
        let unscopables_ptr = unscopables_value.as_pointer::<ObjectHeader>();
        let blocked = js_object_get_field_by_name_f64(unscopables_ptr, key);
        if js_is_truthy(blocked) != 0 {
            return 0;
        }
    }

    1
}

#[no_mangle]
pub extern "C" fn js_with_get_binding(bindings: f64, key: *const StringHeader) -> f64 {
    let ptr = object_ptr(bindings);
    js_object_get_field_by_name_f64(ptr as *const ObjectHeader, key)
}

#[no_mangle]
pub extern "C" fn js_with_set_binding(
    bindings: f64,
    key: *const StringHeader,
    value: f64,
    strict: i32,
) -> f64 {
    let coerced = to_object_bindings(bindings);
    let ptr = object_ptr(coerced);
    if strict != 0 && !has_property(coerced, key) {
        crate::error::js_throw_reference_error_unresolvable_assignment(key_as_value(key));
    }
    js_object_set_field_by_name(ptr, key, value);
    value
}

#[no_mangle]
pub extern "C" fn js_with_delete_binding(bindings: f64, key: *const StringHeader) -> i32 {
    let ptr = object_ptr(bindings);
    js_object_delete_field(ptr, key)
}

/// Sentinel for a sloppy implicit global created as a `with`-set FALLBACK
/// (`with (o) { foo = 42; }` where `o` may or may not own `foo`). The local
/// starts as this sentinel; the fallback store replaces it only when the
/// with-env did NOT take the write. A later bare read of the name routes
/// through `js_with_implicit_read`, which throws ReferenceError while the
/// sentinel is still in place (test262 with/12.10-0-7 vs S13.2.2_A19).
#[no_mangle]
pub extern "C" fn js_with_implicit_unset() -> f64 {
    f64::from_bits(crate::value::TAG_HOLE)
}

// #1561-style force-keep: only generated IR calls these.
#[used]
static KEEP_JS_WITH_IMPLICIT_UNSET: extern "C" fn() -> f64 = js_with_implicit_unset;
#[used]
static KEEP_JS_WITH_IMPLICIT_READ: extern "C" fn(f64, f64) -> f64 = js_with_implicit_read;

/// `name` arrives as a NaN-boxed string (codegen lowers `Expr::String` args
/// to boxed doubles).
#[no_mangle]
pub extern "C" fn js_with_implicit_read(value: f64, name: f64) -> f64 {
    if value.to_bits() == crate::value::TAG_HOLE {
        let name_ptr = crate::value::js_get_string_pointer_unified(name) as *const StringHeader;
        let name_str = if name_ptr.is_null() {
            "<ident>".to_string()
        } else {
            unsafe {
                let ptr = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let len = (*name_ptr).byte_len as usize;
                String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
            }
        };
        let msg = format!("{} is not defined", name_str);
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_referenceerror_new(msg_str);
        crate::exception::js_throw(js_nanbox_pointer(err as i64));
    }
    value
}

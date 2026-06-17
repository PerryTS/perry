//! Shared Node-style argument validators (#2013, #3146).
//!
//! Mirrors node's `lib/internal/validators.js` (`validateString`,
//! `validateNumber`, `validateInteger`, `validateInt32`, `validateUint32`,
//! `validateBoolean`, `validateObject`, `validateFunction`, `validateBuffer`,
//! `validateOneOf`, `validatePort`, …) so every stdlib entry point can throw
//! the same typed error (`TypeError [ERR_INVALID_ARG_TYPE]`, `RangeError
//! [ERR_OUT_OF_RANGE]`, `RangeError [ERR_SOCKET_BAD_PORT]`,
//! `TypeError [ERR_INVALID_ARG_VALUE]`) with node's exact message shape.
//!
//! The error-object plumbing (message side table carrying `.code`, the
//! `Received …` clause renderer) predates this module and lives in
//! [`crate::fs::validate`]; this module is the argument-validation surface
//! built on top of it. New stdlib validation should call these helpers
//! rather than hand-formatting messages so the wording stays centralized.

use crate::fs::validate::{
    describe_received, format_received_number, is_numeric, throw_range_error_named,
    throw_type_error_with_code,
};
use crate::value::JSValue;

/// `Number.MIN_SAFE_INTEGER` / `Number.MAX_SAFE_INTEGER` — the default
/// bounds of node's `validateInteger`.
pub const MIN_SAFE_INTEGER: f64 = -9_007_199_254_740_991.0;
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[inline]
fn jv(value: f64) -> JSValue {
    JSValue::from_bits(value.to_bits())
}

/// True when `value` NaN-boxes a callable closure. Mirrors
/// `fs::stream::extract_closure_ptr`'s probe but uses only the public
/// `crate::closure` surface (the `fs::stream` module is private).
fn is_closure_value(value: f64) -> bool {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if (0x7FF8..=0x7FFF).contains(&top16) {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return false;
    };
    raw >= 0x1000 && crate::closure::is_closure_ptr(raw)
}

/// Numeric payload of a JS number value (plain double or INT32-tagged).
/// Callers must have checked [`is_numeric`] first.
pub fn number_value(value: f64) -> f64 {
    let v = jv(value);
    if v.is_int32() {
        v.as_int32() as f64
    } else {
        v.as_number()
    }
}

/// node `determineSpecificType` puts `argument` in the message for plain
/// names and `property` for dotted ones (`options.level`).
fn name_kind(name: &str) -> &'static str {
    if name.contains('.') {
        "property"
    } else {
        "argument"
    }
}

/// Throw `TypeError [ERR_INVALID_ARG_TYPE]` with node's message shape:
/// `The "<name>" argument must be <expected>. Received <actual>`.
///
/// `expected` is the already-formatted clause — `"of type string"`,
/// `"an instance of Buffer, TypedArray, or DataView"`, `"one of type string
/// or object"` — matching how node renders its `expected` array.
pub fn throw_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" {} must be {}. Received {}",
        name,
        name_kind(name),
        expected,
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

/// Render the `Received …` clause of an `ERR_OUT_OF_RANGE` message. node
/// adds `_` numeric separators once the magnitude passes 2^32
/// (`addNumericalSeparator` in internal/errors.js).
pub fn format_range_received(n: f64) -> String {
    let base = format_received_number(n);
    if n.is_finite() && n.fract() == 0.0 && n.abs() > 4_294_967_296.0 {
        let (sign, digits) = base
            .strip_prefix('-')
            .map_or(("", base.as_str()), |rest| ("-", rest));
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        let len = digits.len();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (len - i) % 3 == 0 {
                out.push('_');
            }
            out.push(ch);
        }
        format!("{sign}{out}")
    } else {
        base
    }
}

/// Throw `RangeError [ERR_OUT_OF_RANGE]`: `The value of "<name>" is out of
/// range. It must be <range>. Received <received>`.
pub fn throw_out_of_range(name: &str, range: &str, received: &str) -> ! {
    let message = format!(
        "The value of \"{}\" is out of range. It must be {}. Received {}",
        name, range, received
    );
    throw_range_error_named(&message, "ERR_OUT_OF_RANGE")
}

/// Throw `TypeError [ERR_INVALID_ARG_VALUE]`: `The <argument|property>
/// '<name>' <reason>. Received <received>`.
pub fn throw_invalid_arg_value(name: &str, reason: &str, received: &str) -> ! {
    let message = format!(
        "The {} '{}' {}. Received {}",
        name_kind(name),
        name,
        reason,
        received
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

/// node `validateString(value, name)`.
pub fn validate_string(value: f64, name: &str) {
    if !jv(value).is_any_string() {
        throw_invalid_arg_type(name, "of type string", value);
    }
}

/// node `validateNumber(value, name[, min[, max]])`.
pub fn validate_number(value: f64, name: &str, min: Option<f64>, max: Option<f64>) -> f64 {
    if !is_numeric(jv(value)) {
        throw_invalid_arg_type(name, "of type number", value);
    }
    let n = number_value(value);
    let below = min.is_some_and(|m| !(n >= m));
    let above = max.is_some_and(|m| !(n <= m));
    if below || above {
        let range = match (min, max) {
            (Some(lo), Some(hi)) => format!(
                ">= {} && <= {}",
                format_received_number(lo),
                format_received_number(hi)
            ),
            (Some(lo), None) => format!(">= {}", format_received_number(lo)),
            (None, Some(hi)) => format!("<= {}", format_received_number(hi)),
            (None, None) => unreachable!(),
        };
        throw_out_of_range(name, &range, &format_range_received(n));
    }
    n
}

/// node `validateInteger(value, name[, min[, max]])` — defaults to the safe
/// integer range. Returns the validated value.
pub fn validate_integer(value: f64, name: &str, min: f64, max: f64) -> f64 {
    if !is_numeric(jv(value)) {
        throw_invalid_arg_type(name, "of type number", value);
    }
    let n = number_value(value);
    if !n.is_finite() || n.fract() != 0.0 {
        throw_out_of_range(name, "an integer", &format_range_received(n));
    }
    if n < min || n > max {
        let range = format!(
            ">= {} && <= {}",
            format_received_number(min),
            format_received_number(max)
        );
        throw_out_of_range(name, &range, &format_range_received(n));
    }
    n
}

/// node `validateInt32(value, name[, min[, max]])`.
pub fn validate_int32(value: f64, name: &str, min: i32, max: i32) -> i32 {
    validate_integer(value, name, min as f64, max as f64) as i32
}

/// node `validateUint32(value, name)`.
pub fn validate_uint32(value: f64, name: &str) -> u32 {
    validate_integer(value, name, 0.0, u32::MAX as f64) as u32
}

/// node `validateBoolean(value, name)`.
pub fn validate_boolean(value: f64, name: &str) {
    if !jv(value).is_bool() {
        throw_invalid_arg_type(name, "of type boolean", value);
    }
}

/// node `validateFunction(value, name)`.
pub fn validate_function(value: f64, name: &str) {
    if !is_closure_value(value) {
        throw_invalid_arg_type(name, "of type function", value);
    }
}

/// GC object-type tag for a heap pointer value, if it is one. Uses the
/// centralized `addr_class::try_read_gc_header` (handle-band/magnitude guard +
/// header read) rather than a hand-rolled `GcHeader` cast.
fn gc_obj_type(value: f64) -> Option<u8> {
    let v = jv(value);
    if !v.is_pointer() {
        return None;
    }
    let addr = v.as_pointer::<u8>() as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if header.obj_type <= crate::gc::GC_TYPE_MAX {
        Some(header.obj_type)
    } else {
        None
    }
}

/// node `validateObject(value, name)` with default options: rejects `null`,
/// arrays, functions, and primitives.
pub fn validate_object(value: f64, name: &str) {
    let is_plain_object = gc_obj_type(value) == Some(crate::gc::GC_TYPE_OBJECT)
        && !is_closure_value(value)
        && crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) != 1;
    if !is_plain_object {
        throw_invalid_arg_type(name, "of type object", value);
    }
}

/// True when `value` is a `Buffer`, `TypedArray`, or `DataView` — the
/// acceptance set of node's `validateBuffer` / `isArrayBufferView`.
pub fn is_buffer_like(value: f64) -> bool {
    let bits = value.to_bits() as i64;
    if crate::buffer::js_buffer_is_buffer(bits) == 1 {
        return true;
    }
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < 0x1000 {
        return false;
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        return true;
    }
    crate::buffer::is_registered_buffer(raw) && crate::buffer::is_data_view(raw)
}

/// node `validateBuffer(value, name)`.
pub fn validate_buffer(value: f64, name: &str) {
    if !is_buffer_like(value) {
        throw_invalid_arg_type(
            name,
            "an instance of Buffer, TypedArray, or DataView",
            value,
        );
    }
}

/// Validate a value that may be a string OR a `Buffer`/`TypedArray`/
/// `DataView` (the acceptance set of `hash.update`, `hmac.update`,
/// `cipher.update`, …). `expected` is the already-formatted clause node uses
/// for that specific API (the accepted view set varies slightly between
/// APIs). Throws `TypeError [ERR_INVALID_ARG_TYPE]` otherwise.
pub fn validate_string_or_buffer_view(value: f64, name: &str, expected: &str) {
    if jv(value).is_any_string() || is_buffer_like(value) {
        return;
    }
    throw_invalid_arg_type(name, expected, value);
}

/// node `validateOneOf(value, name, oneOf)` for string-valued options.
pub fn validate_one_of_str(value: f64, name: &str, one_of: &[&str]) {
    let v = jv(value);
    if v.is_any_string() {
        let content = crate::fs::validate::read_js_string_pub(value);
        if one_of.contains(&content.as_str()) {
            return;
        }
    }
    let reason = format!(
        "must be one of: {}",
        one_of
            .iter()
            .map(|s| format!("'{s}'"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let received = if v.is_any_string() {
        format!("'{}'", crate::fs::validate::read_js_string_pub(value))
    } else {
        describe_received(value)
    };
    throw_invalid_arg_value(name, &reason, &received);
}

/// node `validatePort(port, name)` — throws `RangeError
/// [ERR_SOCKET_BAD_PORT]` with node's exact sentence (note the trailing
/// period and the `determineSpecificType`-style `Received` clause).
pub fn validate_port(value: f64, name: &str) -> u16 {
    let v = jv(value);
    let ok_number = is_numeric(v) && {
        let n = number_value(value);
        n.fract() == 0.0 && (0.0..65536.0).contains(&n)
    };
    if ok_number {
        return number_value(value) as u16;
    }
    // node also accepts numeric strings ("8080").
    if v.is_any_string() {
        let content = crate::fs::validate::read_js_string_pub(value);
        if let Ok(n) = content.trim().parse::<f64>() {
            if n.fract() == 0.0 && (0.0..65536.0).contains(&n) {
                return n as u16;
            }
        }
    }
    let message = format!(
        "{} should be >= 0 and < 65536. Received {}.",
        name,
        describe_received(value)
    );
    throw_range_error_named(&message, "ERR_SOCKET_BAD_PORT")
}

// ============================================================================
// Codegen-callable `#[no_mangle]` entry points.
//
// Some stdlib entry points (notably `crypto.createHash` / `createHmac` /
// `pbkdf2*`) are dispatched by codegen, which NaN-*unboxes* the argument to a
// raw pointer *before* the runtime call. A non-string argument (e.g. a number)
// then has its bit pattern masked into a bogus pointer and dereferenced — a
// hard segfault rather than node's catchable `TypeError`. These helpers take
// the *original* NaN-boxed value (as `f64`) plus the argument name, so codegen
// can emit a `call void` validation BEFORE the unbox, throwing node's typed
// error instead of crashing. The `#[used]` anchors below keep them alive
// through the auto-optimize whole-program rebuild (the bitcode internalizer
// drops `#[no_mangle]` symbols only referenced from generated `.o`).
// ============================================================================

unsafe fn read_arg_name(name_ptr: *const u8, name_len: u32, fallback: &str) -> String {
    if name_ptr.is_null() || name_len == 0 {
        fallback.to_string()
    } else {
        let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Codegen-callable `validateString`. Throws `TypeError
/// [ERR_INVALID_ARG_TYPE]` (`The "<name>" argument must be of type string.
/// Received …`) when `value` is not a string; no-op otherwise.
///
/// # Safety
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range.
#[no_mangle]
pub unsafe extern "C" fn js_runtime_validate_string_arg(
    value: f64,
    name_ptr: *const u8,
    name_len: u32,
) {
    if jv(value).is_any_string() {
        return;
    }
    let name = read_arg_name(name_ptr, name_len, "value");
    throw_invalid_arg_type(&name, "of type string", value);
}

/// Codegen-callable validator for a `node:crypto` key-material argument
/// (`createHmac` key, etc.): accepts a string, `Buffer`, `TypedArray`,
/// `DataView`, or `ArrayBuffer`. Throws node's
/// `The "<name>" argument must be of type string or an instance of
/// ArrayBuffer, Buffer, TypedArray, DataView, KeyObject, or CryptoKey`
/// message otherwise.
///
/// # Safety
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range.
#[no_mangle]
pub unsafe extern "C" fn js_runtime_validate_crypto_key_arg(
    value: f64,
    name_ptr: *const u8,
    name_len: u32,
) {
    let v = jv(value);
    if v.is_any_string() || is_buffer_like(value) {
        return;
    }
    // A registered ArrayBuffer (without the DataView bit) is also acceptable
    // key material.
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw >= 0x1000 && crate::buffer::is_any_array_buffer(raw) {
        return;
    }
    let name = read_arg_name(name_ptr, name_len, "key");
    throw_invalid_arg_type(
        &name,
        "of type string or an instance of ArrayBuffer, Buffer, TypedArray, DataView, KeyObject, or CryptoKey",
        value,
    );
}

/// Codegen-callable `validateInteger(value, name, min, max)`. Throws
/// `TypeError [ERR_INVALID_ARG_TYPE]` on a non-number, `RangeError
/// [ERR_OUT_OF_RANGE]` on a non-integer or out-of-range value. `min`/`max`
/// are passed as `f64` (codegen has no i64 immediate convention here).
///
/// # Safety
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range.
#[no_mangle]
pub unsafe extern "C" fn js_runtime_validate_integer_arg(
    value: f64,
    name_ptr: *const u8,
    name_len: u32,
    min: f64,
    max: f64,
) {
    let name = read_arg_name(name_ptr, name_len, "value");
    validate_integer(value, &name, min, max);
}

#[used]
static KEEP_VALIDATE_STRING_ARG: unsafe extern "C" fn(f64, *const u8, u32) =
    js_runtime_validate_string_arg;
#[used]
static KEEP_VALIDATE_CRYPTO_KEY_ARG: unsafe extern "C" fn(f64, *const u8, u32) =
    js_runtime_validate_crypto_key_arg;
#[used]
static KEEP_VALIDATE_INTEGER_ARG: unsafe extern "C" fn(f64, *const u8, u32, f64, f64) =
    js_runtime_validate_integer_arg;

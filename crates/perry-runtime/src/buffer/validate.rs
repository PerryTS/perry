//! Node-compatible argument validation for the global `Buffer` factory
//! methods (#2013): `Buffer.alloc` / `allocUnsafe` / `allocUnsafeSlow`,
//! `Buffer.byteLength`, and `Buffer.concat`.
//!
//! Node throws synchronously on bad arguments to these functions with a
//! specific `.code` (`ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE`); Perry
//! previously coerced silently — `Buffer.alloc('x')` returned an empty
//! buffer, `Buffer.concat('x')` treated the string pointer as an array
//! header — so `assert.throws`-style tests saw "Missing expected exception"
//! once #1924 stopped masking the no-throw case.
//!
//! These helpers reuse the generic Node-error primitives in
//! [`crate::fs::validate`] (`describe_received`, `throw_type_error_with_code`,
//! `throw_range_error_with_code`, `validate_int32`) — the reusable validation
//! surface introduced for `fs` in #2035 and called out by the issue as the
//! shared home for this work.

use super::*;
use crate::value::JSValue;

/// Node's `buffer.constants.MAX_LENGTH` (2^53 - 1 on 64-bit platforms): the
/// upper bound `assertSize` enforces and reports in the `ERR_OUT_OF_RANGE`
/// message.
const MAX_LENGTH: f64 = 9_007_199_254_740_991.0;

/// Format a finite/non-finite number the way Node renders the `Received …`
/// clause of an `ERR_OUT_OF_RANGE` message.
fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// True if `value` is a plain `Array` (a `GC_TYPE_ARRAY` heap pointer).
/// Mirrors the array detection in `fs::validate::describe_received`.
fn is_array(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    gc_header.obj_type == crate::gc::GC_TYPE_ARRAY
}

/// `Buffer.alloc(size)` / `allocUnsafe(size)` / `allocUnsafeSlow(size)` — Node
/// `assertSize`: `size` must be a number (`ERR_INVALID_ARG_TYPE`) in the range
/// `[0, kMaxLength]`, rejecting `NaN`/`Infinity`/negatives with
/// `ERR_OUT_OF_RANGE`. Non-integers are accepted (truncated toward zero, like
/// the previous `fptosi` lowering). Returns the validated size as `i32` so the
/// codegen call site can feed it straight to the allocator; diverges via
/// `js_throw` on bad input.
#[no_mangle]
pub extern "C" fn js_buffer_validate_size(value: f64) -> i32 {
    let jv = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(jv) {
        let msg = format!(
            "The \"size\" argument must be of type number. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&msg, "ERR_INVALID_ARG_TYPE");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !(n >= 0.0 && n <= MAX_LENGTH) {
        let msg = format!(
            "The value of \"size\" is out of range. It must be >= 0 && <= 9007199254740991. Received {}",
            format_received_number(n)
        );
        crate::fs::validate::throw_range_error_with_code(&msg);
    }
    // #5067 — `size` is in `[0, kMaxLength]` per Node's `assertSize`, but the
    // backing buffer capacity is an `i32`, so anything above `i32::MAX`
    // (~2 GiB) cannot actually be allocated. Node passes `assertSize` for
    // these too and then fails the real allocation, so match its
    // `RangeError: Array buffer allocation failed` rather than truncating the
    // cast (which produced a wrong-size buffer or aborted in the allocator).
    //
    // Compare the *truncated* size: Node truncates a fractional `size` toward
    // zero before allocating, so e.g. `Buffer.alloc(2147483647.9)` is the
    // valid `i32::MAX`, not an over-range request.
    if n.trunc() > i32::MAX as f64 {
        // Plain `RangeError` (no `ERR_*` code) to match V8/Node.
        let msg = b"Array buffer allocation failed";
        let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_rangeerror_new(s);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    n as i32
}

/// `Buffer.concat(list)` — Node requires `list` to be an `Array`
/// (`ERR_INVALID_ARG_TYPE`). Returns the raw (still NaN-boxed) value bits so
/// the caller can hand them straight to `js_buffer_concat[_with_length]`,
/// which strips the tag itself; diverges via `js_throw` on a non-array.
#[no_mangle]
pub extern "C" fn js_buffer_validate_concat_list(value: f64) -> i64 {
    if !is_array(value) {
        let msg = format!(
            "The \"list\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&msg, "ERR_INVALID_ARG_TYPE");
    }
    value.to_bits() as i64
}

/// `Buffer.concat(list, totalLength)` — validate the optional `totalLength`.
/// `undefined` means "sum the element lengths" (no-op here); otherwise Node
/// requires an integer in `[0, kMaxLength]` (`validateInteger`). Reuses
/// `fs::validate::validate_int32`, whose type/integer/range message shapes
/// match Node's for the `length` argument.
pub(crate) fn validate_concat_length(total_length: f64) {
    let jv = JSValue::from_bits(total_length.to_bits());
    if jv.is_undefined() {
        return;
    }
    crate::fs::validate::validate_int32(total_length, "length", 0, MAX_LENGTH as i64);
}

/// `Buffer.byteLength(string[, encoding])` — the first argument must be a
/// string, `Buffer`, `TypedArray`, `DataView`, or `ArrayBuffer`
/// (`SharedArrayBuffer` included); anything else throws
/// `ERR_INVALID_ARG_TYPE`. No-op on a valid value.
pub(crate) fn validate_byte_length_arg(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return;
    }
    let addr = {
        let bits = value.to_bits();
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    };
    if super::js_buffer_is_buffer(value.to_bits() as i64) == 1
        || super::is_any_array_buffer(addr)
        || super::is_uint8array_buffer(addr)
        || super::is_data_view(addr)
        || crate::typedarray::lookup_typed_array_kind(addr).is_some()
    {
        return;
    }
    let msg = format!(
        "The \"string\" argument must be of type string or an instance of Buffer or ArrayBuffer. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&msg, "ERR_INVALID_ARG_TYPE");
}

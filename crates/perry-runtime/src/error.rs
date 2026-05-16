//! Error object implementation for Perry
//!
//! Provides the built-in Error class and its subclasses.

use crate::string::{js_string_from_bytes, StringHeader};

/// Object type tag for runtime type discrimination
pub const OBJECT_TYPE_REGULAR: u32 = 1;
pub const OBJECT_TYPE_ERROR: u32 = 2;

/// Error subclass discriminator (stored in `error_kind`).
/// Used by `instanceof TypeError` etc. to check kind without name string compare.
pub const ERROR_KIND_ERROR: u32 = 0;
pub const ERROR_KIND_TYPE_ERROR: u32 = 1;
pub const ERROR_KIND_RANGE_ERROR: u32 = 2;
pub const ERROR_KIND_REFERENCE_ERROR: u32 = 3;
pub const ERROR_KIND_SYNTAX_ERROR: u32 = 4;
pub const ERROR_KIND_AGGREGATE_ERROR: u32 = 5;

/// Special class IDs for `instanceof` checks (must match perry-codegen/src/expr.rs)
pub const CLASS_ID_ERROR: u32 = 0xFFFF0001;
pub const CLASS_ID_TYPE_ERROR: u32 = 0xFFFF0010;
pub const CLASS_ID_RANGE_ERROR: u32 = 0xFFFF0011;
pub const CLASS_ID_REFERENCE_ERROR: u32 = 0xFFFF0012;
pub const CLASS_ID_SYNTAX_ERROR: u32 = 0xFFFF0013;
pub const CLASS_ID_AGGREGATE_ERROR: u32 = 0xFFFF0014;

/// Error object header
#[repr(C)]
pub struct ErrorHeader {
    /// Type tag to distinguish from regular objects (must be first field!)
    pub object_type: u32,
    /// Error kind discriminator (TypeError, RangeError, etc.)
    pub error_kind: u32,
    /// Error message as a string pointer
    pub message: *mut StringHeader,
    /// Error name (e.g., "Error", "TypeError")
    pub name: *mut StringHeader,
    /// Stack trace (simplified - just a string for now)
    pub stack: *mut StringHeader,
    /// Cause (raw f64 NaN-boxed value, undefined if not set)
    pub cause: f64,
    /// Errors array for AggregateError (raw ArrayHeader pointer or null)
    pub errors: *mut crate::array::ArrayHeader,
}

unsafe fn make_stack(name: &str, message: &str) -> *mut StringHeader {
    // Build a simple "<name>: <message>\n    at <anonymous>" string.
    // Real stack traces are not implemented; the test only checks `.includes(message)`.
    let s = if message.is_empty() {
        format!("{}\n    at <anonymous>", name)
    } else {
        format!("{}: {}\n    at <anonymous>", name, message)
    };
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

unsafe fn alloc_error(
    kind: u32,
    name_bytes: &[u8],
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    let raw = crate::gc::gc_malloc(std::mem::size_of::<ErrorHeader>(), crate::gc::GC_TYPE_ERROR);
    let ptr = raw as *mut ErrorHeader;

    let error_name = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);

    let msg_str = if message.is_null() {
        ""
    } else {
        let len = (*message).byte_len as usize;
        let data = (message as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let name_str = std::str::from_utf8(name_bytes).unwrap_or("Error");
    let stack = make_stack(name_str, msg_str);

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

    (*ptr).object_type = OBJECT_TYPE_ERROR;
    (*ptr).error_kind = kind;
    (*ptr).message = if message.is_null() {
        js_string_from_bytes(b"".as_ptr(), 0)
    } else {
        message
    };
    (*ptr).name = error_name;
    (*ptr).stack = stack;
    (*ptr).cause = f64::from_bits(TAG_UNDEFINED);
    (*ptr).errors = std::ptr::null_mut();

    ptr
}

/// Create a new Error with no message
#[no_mangle]
pub extern "C" fn js_error_new() -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, b"Error", std::ptr::null_mut()) }
}

/// Create a new Error with a message
#[no_mangle]
pub extern "C" fn js_error_new_with_message(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, b"Error", message) }
}

/// Create a new Error with a message and a cause (raw f64 NaN-boxed)
#[no_mangle]
pub extern "C" fn js_error_new_with_cause(
    message: *mut StringHeader,
    cause: f64,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = alloc_error(ERROR_KIND_ERROR, b"Error", message);
        (*ptr).cause = cause;
        ptr
    }
}

/// Create a new TypeError with a message
#[no_mangle]
pub extern "C" fn js_typeerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_TYPE_ERROR, b"TypeError", message) }
}

/// Create a new RangeError with a message
#[no_mangle]
pub extern "C" fn js_rangeerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_RANGE_ERROR, b"RangeError", message) }
}

/// Create a new ReferenceError with a message
#[no_mangle]
pub extern "C" fn js_referenceerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_REFERENCE_ERROR, b"ReferenceError", message) }
}

/// Create a new SyntaxError with a message
#[no_mangle]
pub extern "C" fn js_syntaxerror_new(message: *mut StringHeader) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_SYNTAX_ERROR, b"SyntaxError", message) }
}

/// Create a new AggregateError with an errors array and a message
#[no_mangle]
pub extern "C" fn js_aggregateerror_new(
    errors: *mut crate::array::ArrayHeader,
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = alloc_error(ERROR_KIND_AGGREGATE_ERROR, b"AggregateError", message);
        (*ptr).errors = errors;
        ptr
    }
}

/// Get the message property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_message(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"".as_ptr(), 0);
        }
        (*error).message
    }
}

/// Get the name property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_name(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"Error".as_ptr(), 5);
        }
        (*error).name
    }
}

/// Get the stack property of an Error
#[no_mangle]
pub extern "C" fn js_error_get_stack(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        if error.is_null() {
            return js_string_from_bytes(b"".as_ptr(), 0);
        }
        (*error).stack
    }
}

/// Get the cause property of an Error (raw f64 NaN-boxed value)
#[no_mangle]
pub extern "C" fn js_error_get_cause(error: *mut ErrorHeader) -> f64 {
    unsafe {
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        if error.is_null() {
            return f64::from_bits(TAG_UNDEFINED);
        }
        (*error).cause
    }
}

/// Get the errors array of an AggregateError (raw ArrayHeader pointer)
#[no_mangle]
pub extern "C" fn js_error_get_errors(error: *mut ErrorHeader) -> *mut crate::array::ArrayHeader {
    unsafe {
        if error.is_null() {
            return std::ptr::null_mut();
        }
        (*error).errors
    }
}

/// Get the error kind discriminator (TypeError, RangeError, etc.)
#[no_mangle]
pub extern "C" fn js_error_get_kind(error: *mut ErrorHeader) -> u32 {
    unsafe {
        if error.is_null() {
            return ERROR_KIND_ERROR;
        }
        (*error).error_kind
    }
}

/// Issue #462: property access against `undefined` / `null` must throw
/// `TypeError` per spec. Codegen emits a tag check before the IC fast
/// path; on a TAG_UNDEFINED or TAG_NULL receiver, it calls this helper.
///
/// Issue #596: route through Perry's setjmp/longjmp exception machinery
/// so a user `try { obj.prop } catch (e) { ... }` (or the same shape
/// post-await inside an async fn body) catches the throw instead of
/// the program exiting with the diagnostic. Constructs a real
/// `TypeError` with the V8-shaped message and calls `js_throw` —
/// which longjmps to the nearest enclosing setjmp catch frame, OR
/// prints the uncaught diagnostic + exits 1 if `TRY_DEPTH == 0`
/// (preserving the prior user-visible behavior for unhandled cases).
///
/// `receiver_is_null` distinguishes "Cannot read properties of null"
/// from "Cannot read properties of undefined" (matches V8's wording).
/// `prop_name_ptr` / `prop_name_len` carry the static property name.
#[no_mangle]
pub extern "C" fn js_throw_type_error_property_access(
    receiver_is_null: u32,
    prop_name_ptr: *const u8,
    prop_name_len: usize,
) -> ! {
    let receiver = if receiver_is_null != 0 {
        "null"
    } else {
        "undefined"
    };
    let prop = if prop_name_ptr.is_null() || prop_name_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(prop_name_ptr, prop_name_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let msg = format!(
        "Cannot read properties of {} (reading '{}')",
        receiver, prop
    );
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Issue #510: calling a method on a primitive whose name doesn't
/// resolve on the auto-boxed prototype must throw `TypeError: <expr>
/// is not a function` per spec. Two call sites:
///
///   1. `crates/perry-codegen/src/lower_string_method.rs`'s
///      unknown-method catch-all — when the receiver's static type
///      narrows to `String` (any-typed local refined from a string
///      initializer) and the method name isn't in the hand-handled
///      set. Pre-fix the catch-all silently returned the receiver,
///      so `s.lengt()` evaluated to `s` and execution continued.
///
///   2. `crates/perry-runtime/src/object.rs::js_native_call_method`
///      catch-all — when the receiver is a non-string primitive
///      (number / int32 / bool / bigint) and dispatch is exhausted.
///      Pre-fix the catch-all returned `NULL_OBJECT_BYTES` and
///      execution continued.
///
/// `receiver_kind_*` carries a short label (`"string"` / `"number"` /
/// `"boolean"` / `"bigint"`) used for the diagnostic; pass null/0
/// to omit it. `prop_name_*` carries the called method name.
#[no_mangle]
pub extern "C" fn js_throw_type_error_not_a_function(
    receiver_kind_ptr: *const u8,
    receiver_kind_len: usize,
    prop_name_ptr: *const u8,
    prop_name_len: usize,
) -> ! {
    let kind = if receiver_kind_ptr.is_null() || receiver_kind_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(receiver_kind_ptr, receiver_kind_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let prop = if prop_name_ptr.is_null() || prop_name_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(prop_name_ptr, prop_name_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    // #596: route through Perry's exception machinery so the user's
    // `try { primVal.bogus() } catch (e) { ... }` catches the throw
    // rather than the program exiting. Falls back to print-and-exit
    // via `js_throw`'s `TRY_DEPTH == 0` path when there's no handler.
    let msg = if kind.is_empty() {
        format!("{} is not a function", prop)
    } else {
        format!("({}).{} is not a function", kind, prop)
    };
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Issue #615: writes to read-only / frozen / sealed / non-extensible
/// objects must throw a TypeError under strict mode (per spec). TS files
/// are strict-by-default, so Perry treats every property write as if
/// "use strict" were in effect. The runtime call sites in
/// `js_object_set_field_by_name` (and the index-set helpers) used to
/// silently `return;` when the freeze/seal/writable check tripped —
/// matching non-strict-mode JS behavior — but that left
/// `try { obj.x = 1 } catch { ... }` never firing, breaking
/// `Object.freeze` / `Object.seal` / `Object.preventExtensions` /
/// `Object.defineProperty(..., writable:false)` round-trips.
///
/// `kind`:
///   0 → "Cannot assign to read only property '<key>' of object" (writable:false / frozen overwrite)
///   1 → "Cannot add property '<key>', object is not extensible"  (sealed / preventExtensions / frozen new key)
///   2 → "Cannot delete property '<key>' of #<Object>"             (sealed / frozen delete)
#[no_mangle]
pub extern "C" fn js_throw_type_error_immutable_write(
    kind: u32,
    key_ptr: *const u8,
    key_len: usize,
) -> ! {
    let key = if key_ptr.is_null() || key_len == 0 {
        ""
    } else {
        unsafe {
            let bytes = std::slice::from_raw_parts(key_ptr, key_len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    };
    let msg = match kind {
        0 => format!(
            "Cannot assign to read only property '{}' of object '#<Object>'",
            key
        ),
        1 => format!("Cannot add property {}, object is not extensible", key),
        2 => format!("Cannot delete property '{}' of #<Object>", key),
        _ => format!("Cannot modify object: '{}'", key),
    };
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

/// Convenience wrapper for the runtime — accepts the key as a
/// Rust `&str` and re-routes through the FFI throw helper above. Not
/// `#[no_mangle]` because callers are runtime modules, not codegen.
pub(crate) fn throw_immutable_write(kind: u32, key: &str) -> ! {
    js_throw_type_error_immutable_write(kind, key.as_ptr(), key.len())
}

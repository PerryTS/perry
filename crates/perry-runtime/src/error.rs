//! Error object implementation for Perry
//!
//! Provides the built-in Error class and its subclasses.

use crate::string::{js_string_from_bytes, StringHeader};

/// Object type tag for runtime type discrimination
pub const OBJECT_TYPE_REGULAR: u32 = 1;
pub const OBJECT_TYPE_ERROR: u32 = 2;
/// #1789: a heap "class object" — the value a class EXPRESSION evaluates to
/// (a regular object stamped with the compile-time template's `class_id`,
/// carrying per-evaluation static fields as own properties). Marks the value
/// as the CLASS itself (vs an instance) so `typeof` is "function", and
/// `new`/`instanceof` read `class_id` from the object. Own-field get/set
/// treat it like OBJECT_TYPE_REGULAR (the get/set paths are gated on
/// `gc_type`/`class_id`, not on this tag).
pub const OBJECT_TYPE_CLASS: u32 = 3;

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
/// AssertionError is a plain ObjectHeader (so it can carry the extra
/// `actual` / `expected` / `operator` / `code` / `generatedMessage`
/// fields Node attaches), but it is registered via
/// `js_register_class_extends_error` at runtime init so
/// `err instanceof Error` returns true on a thrown AssertionError.
pub const CLASS_ID_ASSERTION_ERROR: u32 = 0xFFFF0020;

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
    let scope = crate::gc::RuntimeHandleScope::new();
    // #321 frontier issue #69 (sibling to #2230's `dyn_index_get` guard):
    // codegen lowers `new Error(value)` by handing the value straight to
    // `js_error_new_with_message` even when `value` is not a string pointer
    // (e.g. effect's Cause.ts ends up calling `new Error(...)` with a tag
    // value like `1` along some pretty-printer paths). `alloc_error` then
    // dereferences the pointer at `(*message).byte_len` (offset 4) — for a
    // `message` of 1 that reads address `0x5` and SIGSEGVs.
    //
    // Defensive runtime guard: if `message` does not look like a real
    // heap-allocated string pointer (NaN-boxing tag values, small ints, and
    // garbage from non-pointer dataflow all fail this gate), coerce it to
    // an empty string before any reads. The same `is_valid_obj_ptr`
    // predicate guards `dyn_index_get/set` (#2230), `js_object_keys`, the
    // by-name field setters, and several typed-feedback probes — this
    // brings the error-allocation path under the same umbrella.
    let message_ptr = if message.is_null() || !crate::object::is_valid_obj_ptr(message as *const u8)
    {
        js_string_from_bytes(b"".as_ptr(), 0)
    } else {
        message
    };
    let message_handle = scope.root_string_ptr(message_ptr);

    let error_name = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
    let error_name_handle = scope.root_string_ptr(error_name);

    let message_ptr = message_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    let msg_str = {
        let len = (*message_ptr).byte_len as usize;
        let data = (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("")
    };
    let name_str = std::str::from_utf8(name_bytes).unwrap_or("Error");
    let stack = make_stack(name_str, msg_str);
    let stack_handle = scope.root_string_ptr(stack);

    let raw = crate::arena::arena_alloc_gc(
        std::mem::size_of::<ErrorHeader>(),
        std::mem::align_of::<ErrorHeader>(),
        crate::gc::GC_TYPE_ERROR,
    );
    let ptr = raw as *mut ErrorHeader;

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

    (*ptr).object_type = OBJECT_TYPE_ERROR;
    (*ptr).error_kind = kind;
    (*ptr).message = message_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).name = error_name_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).stack = stack_handle.get_raw_const_ptr::<StringHeader>() as *mut StringHeader;
    (*ptr).cause = f64::from_bits(TAG_UNDEFINED);
    (*ptr).errors = std::ptr::null_mut();

    ptr
}

pub(crate) unsafe fn error_set_cause(error: *mut ErrorHeader, cause: f64) {
    crate::gc::runtime_store_gc_jsvalue_slot(
        error as usize,
        &(*error).cause as *const f64 as usize,
        cause.to_bits(),
    );
}

pub(crate) unsafe fn error_set_errors(
    error: *mut ErrorHeader,
    errors: *mut crate::array::ArrayHeader,
) {
    crate::gc::runtime_store_gc_heap_word_slot(
        error as usize,
        &(*error).errors as *const _ as usize,
        errors as u64,
    );
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

/// Create a new Error-like object with a custom `.name` and stack prefix.
pub(crate) fn js_error_new_with_name_message(
    name: &'static [u8],
    message: *mut StringHeader,
) -> *mut ErrorHeader {
    unsafe { alloc_error(ERROR_KIND_ERROR, name, message) }
}

/// Create a new Error with a message and a cause (raw f64 NaN-boxed)
#[no_mangle]
pub extern "C" fn js_error_new_with_cause(
    message: *mut StringHeader,
    cause: f64,
) -> *mut ErrorHeader {
    unsafe {
        let ptr = alloc_error(ERROR_KIND_ERROR, b"Error", message);
        error_set_cause(ptr, cause);
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
        error_set_errors(ptr, errors);
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

fn throw_capture_stack_trace_target_type_error() -> ! {
    let message = b"The \"targetObject\" argument must be an object";
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `Error.captureStackTrace(target[, constructorOpt])`.
///
/// Perry's stack strings are intentionally coarse today; this helper installs
/// the same non-enumerable `stack` data property shape Node exposes and
/// preserves the invalid-target TypeError contract.
#[no_mangle]
pub extern "C" fn js_error_capture_stack_trace(target: f64, _constructor_opt: f64) -> f64 {
    let target_value = crate::value::JSValue::from_bits(target.to_bits());
    if !target_value.is_pointer() {
        throw_capture_stack_trace_target_type_error();
    }

    unsafe {
        let target_ptr = target_value.as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        if target_ptr.is_null() || !crate::object::is_valid_obj_ptr(target_ptr as *const u8) {
            throw_capture_stack_trace_target_type_error();
        }

        let stack = make_stack("Error", "");
        let key = js_string_from_bytes(b"stack".as_ptr(), 5);
        let value = crate::value::js_nanbox_string(stack as i64);
        crate::object::js_object_set_field_by_name(target_ptr, key, value);
        crate::object::set_property_attrs(
            target_ptr as usize,
            "stack".to_string(),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }

    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Read a `StringHeader`'s UTF-8 bytes into an owned `String` (lossy on
/// invalid UTF-8). Empty on null.
unsafe fn read_string_header_owned(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    String::from_utf8_lossy(bytes).into_owned()
}

/// `Error.prototype.toString()` — ECMAScript §20.5.3.4. Returns `name` when
/// `message` is empty, `message` when `name` is empty, otherwise
/// `"{name}: {message}"`. Previously errors fell back to `Object.prototype`'s
/// `"[object Object]"` for both explicit `e.toString()` and string coercion
/// (`String(e)`, template literals), diverging from Node byte-for-byte (#2135).
#[no_mangle]
pub extern "C" fn js_error_to_string(error: *mut ErrorHeader) -> *mut StringHeader {
    unsafe {
        let name = read_string_header_owned(js_error_get_name(error));
        let message = read_string_header_owned(js_error_get_message(error));
        let result = if name.is_empty() {
            message
        } else if message.is_empty() {
            name
        } else {
            format!("{name}: {message}")
        };
        js_string_from_bytes(result.as_ptr(), result.len() as u32)
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

#[cfg(test)]
mod tostring_tests {
    use super::*;

    fn s(bytes: &[u8]) -> *mut StringHeader {
        js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
    }

    #[test]
    fn error_to_string_name_and_message() {
        let e = js_error_new_with_message(s(b"boom"));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "Error: boom");
    }

    #[test]
    fn error_to_string_no_message_is_just_name() {
        let e = js_error_new_with_message(s(b""));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "Error");
    }

    #[test]
    fn typed_error_to_string_uses_subclass_name() {
        let e = js_error_new_with_name_message(b"TypeError", s(b"bad"));
        let out = unsafe { read_string_header_owned(js_error_to_string(e)) };
        assert_eq!(out, "TypeError: bad");
    }
}

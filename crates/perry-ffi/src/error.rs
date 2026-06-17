//! Error-throwing surface — lets wrappers raise a JS `Error` /
//! `TypeError` / `RangeError` carrying a Node-style `.code`, plus a
//! reader that returns a `Buffer` / `TypedArray` value's raw bytes.
//!
//! # Why
//!
//! Wrappers compiled into their own staticlib (e.g. `perry-ext-http-server`)
//! cannot depend on `perry-runtime`'s Rust API and must not touch the
//! runtime's thread-local registries directly: a direct
//! `is_registered_buffer` / `register_error_code` call from the wrapper's
//! own monomorphized copy reads a *different* thread-local than the one the
//! program-side dispatch uses, so recognition silently fails. Routing
//! through these single extern symbols keeps the registry/throw logic in
//! the one runtime copy the dispatch path resolves to.

use crate::JsValue;

extern "C" {
    /// Runtime entry: build an Error subclass with a `.code`.
    /// `kind`: 0 = Error, 1 = TypeError, 2 = RangeError.
    fn js_error_value_with_code(
        msg_ptr: *const u8,
        msg_len: usize,
        code_ptr: *const u8,
        code_len: usize,
        kind: i32,
    ) -> f64;

    /// Runtime entry: build an Error subclass with a `.code`, then throw.
    /// `kind`: 0 = Error, 1 = TypeError, 2 = RangeError. Diverges.
    fn js_throw_error_with_code(
        msg_ptr: *const u8,
        msg_len: usize,
        code_ptr: *const u8,
        code_len: usize,
        kind: i32,
    ) -> !;

    /// Runtime entry: pointer to a Buffer/TypedArray value's bytes (with
    /// length via `out_len`), or null for any other value.
    fn js_value_buffer_or_typedarray_data(bits: f64, out_len: *mut u32) -> *const u8;

    /// Runtime entry: build a Node-style system Error with `.message`,
    /// `.code`, `.syscall` and `.errno`.
    fn js_node_system_error_value(
        msg_ptr: *const u8,
        msg_len: usize,
        code_ptr: *const u8,
        code_len: usize,
        syscall_ptr: *const u8,
        syscall_len: usize,
        errno: f64,
    ) -> f64;
}

/// Which JS Error subclass [`throw_with_code`] raises.
pub enum ErrorKind {
    /// A plain `Error`.
    Error,
    /// A `TypeError`.
    TypeError,
    /// A `RangeError`.
    RangeError,
}

/// Throw a JS Error subclass whose `.message` is `msg` and whose `.code`
/// is `code` (a Node `ERR_*` string). Never returns.
///
/// ```ignore
/// perry_ffi::throw_with_code(
///     "Packed settings length must be a multiple of six",
///     "ERR_HTTP2_INVALID_PACKED_SETTINGS_LENGTH",
///     perry_ffi::ErrorKind::RangeError,
/// );
/// ```
pub fn throw_with_code(msg: &str, code: &str, kind: ErrorKind) -> ! {
    let k = match kind {
        ErrorKind::Error => 0,
        ErrorKind::TypeError => 1,
        ErrorKind::RangeError => 2,
    };
    // SAFETY: both slices are valid for their lengths; the runtime copies
    // the bytes into arena-owned storage before diverging.
    unsafe { js_throw_error_with_code(msg.as_ptr(), msg.len(), code.as_ptr(), code.len(), k) }
}

/// Build a JS Error subclass whose `.message` is `msg` and whose `.code` is
/// `code` (a Node `ERR_*` string), without throwing it.
///
/// This is the non-throwing twin of [`throw_with_code`], used by APIs such as
/// `events.once()` that must return a Promise and reject it with a coded error
/// instead of throwing synchronously.
pub fn error_value_with_code(msg: &str, code: &str, kind: ErrorKind) -> JsValue {
    let k = match kind {
        ErrorKind::Error => 0,
        ErrorKind::TypeError => 1,
        ErrorKind::RangeError => 2,
    };
    // SAFETY: both slices are valid for their lengths; the runtime copies
    // the bytes into arena-owned storage before returning the error value.
    let value =
        unsafe { js_error_value_with_code(msg.as_ptr(), msg.len(), code.as_ptr(), code.len(), k) };
    JsValue::from_bits(value.to_bits())
}

/// Build a Node-style system `Error` value carrying `.message`, `.code`
/// (a Node `E*` string), `.syscall` (the failing syscall, e.g. `"connect"`
/// or `"getaddrinfo"`) and `.errno` (the libuv-negative number). This is the
/// shape Node hands to `socket`/`request` `'error'` listeners for transport
/// failures, so consumers branching on `err.code === 'ECONNREFUSED'` work.
pub fn system_error_value(msg: &str, code: &str, syscall: &str, errno: i64) -> JsValue {
    // SAFETY: all three slices are valid for their lengths; the runtime copies
    // the bytes into arena-owned storage before returning the error value.
    let value = unsafe {
        js_node_system_error_value(
            msg.as_ptr(),
            msg.len(),
            code.as_ptr(),
            code.len(),
            syscall.as_ptr(),
            syscall.len(),
            errno as f64,
        )
    };
    JsValue::from_bits(value.to_bits())
}

/// Borrow the raw bytes of a `Buffer` or `TypedArray` value. Returns
/// `None` for any value that is neither (the caller should raise a
/// `TypeError` in that case). The borrow is valid for the duration of the
/// calling FFI invocation.
pub fn value_byte_slice(value: JsValue) -> Option<&'static [u8]> {
    let mut len: u32 = 0;
    // SAFETY: `js_value_buffer_or_typedarray_data` returns either null or a
    // pointer to `len` live bytes inside the runtime arena.
    let ptr = unsafe { js_value_buffer_or_typedarray_data(f64::from_bits(value.bits()), &mut len) };
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
}

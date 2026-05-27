//! Node-compatible argument validation for the `fs` module (#2013).
//!
//! Node throws synchronously on invalid `fs` arguments with a specific
//! `.code`: a non path-like first argument yields `TypeError
//! [ERR_INVALID_ARG_TYPE]`, and the `fd`-accepting readers/writers
//! (`readFileSync`/`writeFileSync`) treat a numeric first argument as a file
//! descriptor — an invalid one fails with `Error [EBADF]`. Perry's `fs`
//! helpers previously returned a sentinel (empty string, `-1`, a zeroed
//! stats object) and never threw, so `assert.throws`-style tests saw
//! "Missing expected exception" once #1924 stopped masking the no-throw case.
//!
//! These helpers are the reusable validation surface called from the top of
//! the `fs` sync entry points. The error `.code` is recorded in the
//! per-message side table (`node_submodules`) so the `.code` getter recovers
//! it on the caught error — the same mechanism `fs` already uses for POSIX
//! errors like `ENOENT`.

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// True if `value` is a valid Node "path-like" — a string (including inline
/// SSO short strings), a `Buffer`, or a `file:` URL object. Mirrors the type
/// acceptance of Node's internal `getValidatedPath`.
pub(crate) fn is_path_like(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return true;
    }
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let protocol = crate::url::get_string_content(unsafe {
                crate::object::js_object_get_field_f64(obj, crate::url::parse::URL_PROTOCOL)
            });
            if protocol == "file:" {
                return true;
            }
        }
    }
    false
}

/// True if `value` is a JS number (a plain IEEE double *or* an INT32-tagged
/// small integer). `JSValue::is_number` deliberately excludes the INT32 tag,
/// so both must be checked.
fn is_numeric(jv: JSValue) -> bool {
    jv.is_number() || jv.is_int32()
}

fn numeric_to_i32(jv: JSValue) -> i32 {
    if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

/// Node's `Received …` clause for an `ERR_INVALID_ARG_TYPE` message.
fn describe_received(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return format!("type boolean ({})", jv.as_bool());
    }
    if is_numeric(jv) {
        let n = if jv.is_int32() {
            jv.as_int32() as f64
        } else {
            jv.as_number()
        };
        if n.fract() == 0.0 && n.is_finite() {
            return format!("type number ({})", n as i64);
        }
        return format!("type number ({})", n);
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>();
        if !ptr.is_null() && (ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
            if gc_header.obj_type == crate::gc::GC_TYPE_ARRAY {
                return "an instance of Array".to_string();
            }
        }
        return "an instance of Object".to_string();
    }
    "an unsupported value".to_string()
}

/// Throw `TypeError [ERR_INVALID_ARG_TYPE]` for a bad path argument, matching
/// Node's message shape. Diverges via `js_throw`.
pub(crate) fn throw_invalid_path_arg(arg_name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type string or an instance of Buffer or URL. Received {}",
        arg_name,
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

/// Throw `Error [EBADF]` for a numeric fd that is not an open descriptor.
fn throw_ebadf(syscall: &'static str) -> ! {
    let message = format!("EBADF: bad file descriptor, {}", syscall);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "EBADF");
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_type_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Validate the first argument of a path-only `fs` sync function (one that
/// does NOT accept a file descriptor — `accessSync`, `statSync`, `mkdirSync`,
/// `readdirSync`, `unlinkSync`, …). Throws `ERR_INVALID_ARG_TYPE` on any
/// non path-like value (including numbers). No-op when the value is valid.
pub(crate) fn validate_path(arg_name: &str, value: f64) {
    if !is_path_like(value) {
        throw_invalid_path_arg(arg_name, value);
    }
}

/// Validate the first argument of an fd-accepting reader/writer
/// (`readFileSync`, `writeFileSync`). A path-like value is accepted as-is; a
/// numeric value is treated as a file descriptor and, if it is not open,
/// throws `EBADF` (matching `fs.readFileSync(123)`); anything else throws
/// `ERR_INVALID_ARG_TYPE`. `syscall` names the operation for the EBADF error.
pub(crate) fn validate_path_or_fd(arg_name: &str, value: f64, syscall: &'static str) {
    if is_path_like(value) {
        return;
    }
    let jv = JSValue::from_bits(value.to_bits());
    if is_numeric(jv) {
        // A numeric first argument is a file descriptor. Perry's readers and
        // writers already serve a *registered* fd (`numeric_fd_value` +
        // `FD_REGISTRY`); the validation contract here is only to reject an
        // unknown/closed fd with `EBADF` (e.g. `fs.readFileSync(123)`).
        if !crate::fs::fd_is_registered(numeric_to_i32(jv)) {
            throw_ebadf(syscall);
        }
        return;
    }
    throw_invalid_path_arg(arg_name, value);
}

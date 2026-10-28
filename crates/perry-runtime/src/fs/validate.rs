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

use crate::closure::ClosureHeader;

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
            let protocol = crate::value::boxed_text::get_string_content(
                crate::object::js_object_get_field_f64(obj, crate::url::parse::URL_PROTOCOL),
            );
            return protocol == "file:" && unsafe { is_valid_file_url_path_object(obj) };
        }
    }
    false
}

fn encoded_path_separator(pathname: &str) -> bool {
    let bytes = pathname.as_bytes();
    if bytes.len() < 3 {
        return false;
    }
    for i in 0..=(bytes.len() - 3) {
        if bytes[i] != b'%' {
            continue;
        }
        let hex_hi = bytes[i + 1];
        let hex_lo = bytes[i + 2] | 0x20;
        if hex_hi == b'2' && hex_lo == b'f' {
            return true;
        }
        #[cfg(windows)]
        if hex_hi == b'5' && hex_lo == b'c' {
            return true;
        }
    }
    false
}

unsafe fn file_url_host(obj: *const crate::object::ObjectHeader) -> String {
    let hostname = crate::value::boxed_text::get_string_content(
        crate::object::js_object_get_field_f64(obj, crate::url::parse::URL_HOSTNAME),
    );
    if !hostname.is_empty() {
        return hostname;
    }
    crate::value::boxed_text::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_HOST,
    ))
}

unsafe fn file_url_pathname(obj: *const crate::object::ObjectHeader) -> String {
    crate::value::boxed_text::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PATHNAME,
    ))
}

unsafe fn is_valid_file_url_path_object(obj: *const crate::object::ObjectHeader) -> bool {
    let host = file_url_host(obj);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    !encoded_path_separator(&file_url_pathname(obj))
}

pub(crate) unsafe fn validate_file_url_path_object(obj: *const crate::object::ObjectHeader) {
    let host = file_url_host(obj);
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        throw_type_error_with_code(
            "File URL host must be \"localhost\" or empty on this platform",
            "ERR_INVALID_FILE_URL_HOST",
        );
    }
    if encoded_path_separator(&file_url_pathname(obj)) {
        throw_type_error_with_code(
            "File URL path must not include encoded / characters",
            "ERR_INVALID_FILE_URL_PATH",
        );
    }
}

fn validate_path_like_value(_arg_name: &str, value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        return true;
    }
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return true;
    }
    if !jv.is_pointer() {
        return false;
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        return false;
    }
    let protocol = crate::value::boxed_text::get_string_content(
        crate::object::js_object_get_field_f64(obj, crate::url::parse::URL_PROTOCOL),
    );
    if protocol.is_empty() {
        return false;
    }
    if protocol != "file:" {
        throw_type_error_with_code("The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
    }
    unsafe { validate_file_url_path_object(obj) };
    true
}

fn numeric_to_i32(jv: JSValue) -> i32 {
    if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

/// Throw `TypeError [ERR_INVALID_ARG_TYPE]` for a bad path argument, matching
/// Node's message shape. Diverges via `js_throw`.
pub(crate) fn throw_invalid_path_arg(arg_name: &str, value: f64) -> ! {
    throw_received_type_error(
        &format!(
            "The \"{}\" argument must be of type string or an instance of Buffer or URL. Received ",
            arg_name
        ),
        value,
    );
}

/// Throw `Error [EBADF]` for a numeric fd that is not an open descriptor.
fn throw_ebadf(syscall: &'static str) -> ! {
    crate::exception::js_throw(build_ebadf_error_value(syscall))
}

pub(crate) fn build_ebadf_error_value(syscall: &'static str) -> f64 {
    let message = format!("EBADF: bad file descriptor, {}", syscall);
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "EBADF");
    crate::node_submodules::register_error_syscall(msg, syscall);
    #[cfg(unix)]
    crate::node_submodules::register_error_errno(msg, -libc::EBADF);
    #[cfg(not(unix))]
    crate::node_submodules::register_error_errno(msg, -9);
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

/// Throw Node's null-byte path error. Node uses `ERR_INVALID_ARG_VALUE`
/// rather than an fs errno when a decoded PathLike contains `\0`.
pub(crate) fn throw_invalid_path_arg_value(arg_name: &str, received: &str) -> ! {
    let display = received.replace('\0', "\\x00");
    let message = format!(
        "The argument '{arg_name}' must be a string, Uint8Array, or URL without null bytes. Received '{display}'"
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE")
}

/// `#[used]` keepalive so the auto-optimize whole-program-LLVM rebuild does
/// not dead-strip this codegen-invoked `#[no_mangle]` entry point (see
/// project_auto_optimize_keepalive_3320). Called only from generated `.o`
/// via the stdlib/ext events validators, so without an anchor the bitcode
/// internalizer drops it and the default `perry file.ts -o out` link fails.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_VALIDATE_EVENT_LISTENER: unsafe extern "C" fn(i64, *const u8, u32) -> i64 =
    js_validate_event_listener;

/// Validate the first argument of a path-only `fs` sync function (one that
/// does NOT accept a file descriptor — `accessSync`, `statSync`, `mkdirSync`,
/// `readdirSync`, `unlinkSync`, …). Throws `ERR_INVALID_ARG_TYPE` on any
/// non path-like value (including numbers). No-op when the value is valid.
pub(crate) fn validate_path(arg_name: &str, value: f64) {
    if !validate_path_like_value(arg_name, value) {
        throw_invalid_path_arg(arg_name, value);
    }
}

/// Validate the first argument of an fd-accepting reader/writer
/// (`readFileSync`, `writeFileSync`). A path-like value is accepted as-is; a
/// numeric value is treated as a file descriptor and, if it is not open,
/// throws `EBADF` (matching `fs.readFileSync(123)`); anything else throws
/// `ERR_INVALID_ARG_TYPE`. `syscall` names the operation for the EBADF error.
pub(crate) fn validate_path_or_fd(arg_name: &str, value: f64, syscall: &'static str) {
    if validate_path_like_value(arg_name, value) {
        return;
    }
    if let Some(fd) = crate::fs::filehandle_object_fd(value) {
        if !crate::fs::fd_is_registered(fd) {
            throw_ebadf(syscall);
        }
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

/// Validate that `value` is a JS number suitable for a file descriptor —
/// finite integer in `[0, 2^31-1]`. Matches Node's `validateInt32(fd, 'fd', 0)`.
///
/// Non-numbers raise `TypeError [ERR_INVALID_ARG_TYPE]`; `NaN`, `Infinity`,
/// non-integers, and out-of-range integers raise `RangeError
/// [ERR_OUT_OF_RANGE]`. The filehandle path (`filehandle_fd(closure) as f64`)
/// always passes a real `i32`-ranged value, so the validators are no-ops there.
pub(crate) fn validate_fd(value: f64) {
    validate_int32(value, "fd", 0, i32::MAX as i64);
}

/// Issue #2013 — validate an `fd` argument AND verify it's an open
/// descriptor in Perry's `FD_REGISTRY`. Mirrors Node's "validate fd
/// type, then bounce on EBADF" pattern for the fd-only sync surface
/// (`fs.closeSync`, `fs.readSync`, `fs.readvSync`, `fs.fsyncSync`,
/// `fs.fdatasyncSync`, `fs.fchmodSync`, `fs.fchownSync`, …). Path-or-fd
/// readers/writers route through `validate_path_or_fd` instead, which
/// has its own EBADF branch keyed on the same registry probe.
pub(crate) fn validate_fd_open(value: f64, syscall: &'static str) {
    validate_fd(value);
    let jv = JSValue::from_bits(value.to_bits());
    let fd = numeric_to_i32(jv);
    if !crate::fs::fd_is_registered(fd) {
        throw_ebadf_pub(syscall);
    }
}

/// Public alias for `throw_ebadf` so the fs entry points can throw a
/// matching `EBADF` from outside this module (#2013).
pub(crate) fn throw_ebadf_pub(syscall: &'static str) -> ! {
    throw_ebadf(syscall)
}

/// Issue #3332 — callback-style fd helpers (`fs.close`, `fs.fsync`,
/// `fs.fdatasync`, `fs.fchmod`) must DELIVER the `EBADF` error to the
/// callback rather than throw it. The fd *type* validation still throws
/// synchronously (matching Node's `validateInt32` on a non-numeric fd);
/// only the "valid type but unknown descriptor" case becomes a deferred
/// callback error. Returns `Some(err_value)` when the fd is not open,
/// `None` when it is registered.
pub(crate) fn fd_open_callback_error(value: f64, syscall: &'static str) -> Option<f64> {
    validate_fd(value);
    let jv = JSValue::from_bits(value.to_bits());
    let fd = numeric_to_i32(jv);
    if crate::fs::fd_is_registered(fd) {
        None
    } else {
        Some(build_ebadf_error_value(syscall))
    }
}

/// Validate callback-style fs APIs that require a callback. Node's current fs
/// validators use the argument name `"cb"` for most legacy callback entry
/// points; `fs.opendir` is the notable `"callback"` exception, so callers pass
/// the exact name they need.
pub(crate) fn validate_required_callback(arg_name: &str, value: f64) -> *const ClosureHeader {
    let ptr = super::stream::extract_closure_ptr(value);
    if ptr.is_null() {
        throw_received_type_error(
            &format!(
                "The \"{}\" argument must be of type function. Received ",
                arg_name
            ),
            value,
        );
    }
    ptr
}

fn gc_type_for_value(value: f64) -> Option<u8> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_header.obj_type)
    } else {
        None
    }
}

fn is_options_object_like(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        return true;
    }
    crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1
        || !super::stream::extract_closure_ptr(value).is_null()
}

fn is_plain_options_object(value: f64) -> bool {
    if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1 {
        return false;
    }
    if !super::stream::extract_closure_ptr(value).is_null() {
        return false;
    }
    gc_type_for_value(value) == Some(crate::gc::GC_TYPE_OBJECT)
}

/// Validate fs options parameters that accept either an encoding string or an
/// object (`readFile`, `writeFile`, `readdir`, `readlink`, `realpath`,
/// `mkdtemp`, ...). `null` and `undefined` are accepted.
pub(crate) fn validate_string_or_object_options(arg_name: &str, value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if is_nullish(jv) || jv.is_any_string() || is_options_object_like(value) {
        return;
    }
    throw_received_type_error(
        &format!(
            "The \"{}\" argument must be one of type string or object. Received ",
            arg_name
        ),
        value,
    );
}

/// Validate fs options parameters that accept only an options object. Node
/// accepts an omitted value but rejects `null`, arrays, functions, and
/// primitives with `ERR_INVALID_ARG_TYPE`.
pub(crate) fn object_options_type_error_value(arg_name: &str, value: f64) -> Option<f64> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || is_plain_options_object(value) {
        return None;
    }
    Some(build_received_type_error(
        &format!("The \"{arg_name}\" argument must be of type object. Received "),
        value,
    ))
}

pub(crate) fn validate_object_options(arg_name: &str, value: f64) {
    if let Some(err) = object_options_type_error_value(arg_name, value) {
        crate::exception::js_throw(err);
    }
}

/// Validate the options object passed to `fs.mkdir*` (#3662). Node accepts an
/// omitted value or a numeric/string `mode` shorthand without inspecting it;
/// when an options *object* is supplied it requires `recursive` to be a boolean
/// and `mode` to be a number or string, throwing
/// `TypeError [ERR_INVALID_ARG_TYPE]` otherwise. `recursive` is checked first,
/// matching Node's `validateBoolean(recursive, 'options.recursive')` ordering.
pub(crate) fn validate_mkdir_options(options_value: f64) {
    if !is_plain_options_object(options_value) {
        return;
    }
    let obj =
        JSValue::from_bits(options_value.to_bits()).as_pointer::<crate::object::ObjectHeader>();

    let recursive_key = js_string_from_bytes(b"recursive".as_ptr(), 9);
    let recursive = crate::object::js_object_get_field_by_name_f64(obj, recursive_key);
    let recursive_jv = JSValue::from_bits(recursive.to_bits());
    if !is_nullish(recursive_jv) && !recursive_jv.is_bool() {
        throw_received_type_error(
            "The \"options.recursive\" property must be of type boolean. Received ",
            recursive,
        );
    }

    let mode_key = js_string_from_bytes(b"mode".as_ptr(), 4);
    let mode = crate::object::js_object_get_field_by_name_f64(obj, mode_key);
    let mode_jv = JSValue::from_bits(mode.to_bits());
    if !is_nullish(mode_jv) && !is_numeric(mode_jv) && !mode_jv.is_any_string() {
        throw_received_type_error(
            "The \"options.mode\" property must be of type number. Received ",
            mode,
        );
    }
}

/// Validate the `mode` bitmask used by `fs.access*` and `fs.copyFile*`.
/// Node treats `null`/`undefined` as the default, rejects non-numbers with a
/// short `ERR_INVALID_ARG_TYPE` message, and rejects values whose integer part
/// is outside the supported 0..=7 bitmask.
pub(crate) fn validate_fs_mode(value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if is_nullish(jv) {
        return;
    }
    if !is_numeric(jv) {
        throw_type_error_with_code(
            "mode must be int32 or null/undefined",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !n.is_finite() {
        throw_range_error_with_code("mode is out of range");
    }
    let mode = n as i64;
    if !(0..=7).contains(&mode) {
        throw_range_error_with_code("mode is out of range: >= 0 && <= 7");
    }
}

/// Validate that `value` is a function (closure). On failure throws
/// `TypeError [ERR_INVALID_ARG_TYPE]`. Mirrors Node's `validateFunction`
/// helper — used to catch `fs.exists(path)` / `fs.copyFile(src, dest, 0, 0)`
/// where the trailing callback is missing or the wrong type.
pub(crate) fn validate_function(arg_name: &str, value: f64) {
    if super::stream::extract_closure_ptr(value).is_null() {
        throw_received_type_error(
            &format!(
                "The \"{}\" argument must be of type function. Received ",
                arg_name
            ),
            value,
        );
    }
}

#[cfg(test)]
#[path = "received_diagnostic_tests.rs"]
mod received_diagnostic_tests;

pub(crate) use crate::validators::received::{
    bigint_decimal, build_received_type_error, build_type_error_with_code_value,
    describe_received_js, inspect_string_for_received_js, is_nullish, received_has_null_prototype,
    received_inherited_property, received_key, received_name, received_native_brand,
    received_native_fallback, received_native_prototype_shape, received_property, received_text,
    string_header_to_string, throw_received_type_error,
};
pub use crate::validators::received::{
    describe_received, format_received_number, is_numeric, js_validate_event_listener,
    throw_error_with_code, throw_range_error_named, throw_range_error_with_code,
    throw_type_error_with_code,
};

pub use crate::validators::received::read_js_string_pub;
pub(crate) use crate::validators::received::{read_js_string, validate_int32};

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
            let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
                obj,
                crate::url::parse::URL_PROTOCOL,
            ));
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
    let hostname = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_HOSTNAME,
    ));
    if !hostname.is_empty() {
        return hostname;
    }
    crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_HOST,
    ))
}

unsafe fn file_url_pathname(obj: *const crate::object::ObjectHeader) -> String {
    crate::url::get_string_content(crate::object::js_object_get_field_f64(
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
    let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
        obj,
        crate::url::parse::URL_PROTOCOL,
    ));
    if protocol.is_empty() {
        return false;
    }
    if protocol != "file:" {
        throw_type_error_with_code("The URL must be of scheme file", "ERR_INVALID_URL_SCHEME");
    }
    unsafe { validate_file_url_path_object(obj) };
    true
}

/// True if `value` is a JS number (a plain IEEE double *or* an INT32-tagged
/// small integer). `JSValue::is_number` deliberately excludes the INT32 tag,
/// so both must be checked.
pub fn is_numeric(jv: JSValue) -> bool {
    jv.is_number() || jv.is_int32()
}

fn is_nullish(jv: JSValue) -> bool {
    jv.is_undefined() || jv.is_null()
}

fn numeric_to_i32(jv: JSValue) -> i32 {
    if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

/// Node's `Received …` clause for an `ERR_INVALID_ARG_TYPE` message.
pub fn describe_received(value: f64) -> String {
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
    if jv.is_any_string() {
        return format!("type string ({})", inspect_string_for_received(value));
    }
    if jv.is_bigint() {
        return format!("type bigint ({}n)", bigint_decimal(value));
    }
    if unsafe { crate::symbol::js_is_symbol(value) != 0 } {
        let ptr = unsafe { crate::symbol::js_symbol_to_string(value) } as *const StringHeader;
        return format!("type symbol ({})", string_header_to_string(ptr));
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
    if !super::stream::extract_closure_ptr(value).is_null() {
        return "function ".to_string();
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

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

/// Read a JS string value (heap `StringHeader` or inline SSO) into a Rust
/// `String`. Used by `describe_received` to render a `Received type string
/// ('…')` clause.
fn read_js_string(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(ptr)
}

/// Render a string the way Node's `determineSpecificType` does for the
/// `Received …` clause: single-quoted (switched to double quotes when the
/// content has a single quote but no double quote), then truncated to 25
/// characters plus `...` once the quoted form exceeds 28 characters.
fn inspect_string_for_received(value: f64) -> String {
    let content = read_js_string(value);
    let quote = if content.contains('\'') && !content.contains('"') {
        '"'
    } else {
        '\''
    };
    let inspected = format!("{quote}{content}{quote}");
    if inspected.chars().count() > 28 {
        let truncated: String = inspected.chars().take(25).collect();
        format!("{truncated}...")
    } else {
        inspected
    }
}

/// Decimal rendering of a BigInt value for the `Received type bigint (…n)`
/// clause.
fn bigint_decimal(value: f64) -> String {
    let ptr = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::bigint::BigIntHeader;
    if ptr.is_null() {
        return "0".to_string();
    }
    let s = crate::bigint::js_bigint_to_string(ptr);
    if s.is_null() {
        return "0".to_string();
    }
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
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

pub(crate) fn build_type_error_with_code_value(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

pub fn throw_type_error_with_code(message: &str, code: &'static str) -> ! {
    crate::exception::js_throw(build_type_error_with_code_value(message, code))
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

/// Validate a `node:events` listener argument (#3072).
///
/// `listener_bits` is the *raw NaN-box bit pattern* of the JS value passed
/// for an EventEmitter listener (codegen routes these methods through
/// `NA_JSV`, so the callee receives the full value rather than a
/// pre-stripped pointer). Returns the closure pointer as an `i64` when the
/// value is callable; otherwise throws `TypeError [ERR_INVALID_ARG_TYPE]`
/// with Node's `The "listener" argument must be of type function. Received …`
/// message — matching `EventEmitter#on/once/addListener/prependListener/
/// prependOnceListener/removeListener/off`.
///
/// Shared by both EventEmitter implementations (`perry-stdlib::events` and
/// the out-of-tree `perry-ext-events`) so the validation, error class, code
/// and message stay byte-identical regardless of which one is linked.
///
/// # Safety
///
/// `name_ptr`/`name_len` must describe a valid UTF-8 byte range (typically a
/// `&'static str`). Callers pass `"listener"`.
#[no_mangle]
pub unsafe extern "C" fn js_validate_event_listener(
    listener_bits: i64,
    name_ptr: *const u8,
    name_len: u32,
) -> i64 {
    let value = f64::from_bits(listener_bits as u64);
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return ptr as i64;
        }
    }
    let name = if name_ptr.is_null() || name_len == 0 {
        "listener".to_string()
    } else {
        let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
        String::from_utf8_lossy(bytes).into_owned()
    };
    let message = format!(
        "The \"{name}\" argument must be of type function. Received {}",
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// `#[used]` keepalive so the auto-optimize whole-program-LLVM rebuild does
/// not dead-strip this codegen-invoked `#[no_mangle]` entry point (see
/// project_auto_optimize_keepalive_3320). Called only from generated `.o`
/// via the stdlib/ext events validators, so without an anchor the bitcode
/// internalizer drops it and the default `perry file.ts -o out` link fails.
#[used]
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
        let message = format!(
            "The \"{}\" argument must be of type function. Received {}",
            arg_name,
            describe_received(value)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
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
    let message = format!(
        "The \"{}\" argument must be one of type string or object. Received {}",
        arg_name,
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

/// Validate fs options parameters that accept only an options object. Node
/// accepts an omitted value but rejects `null`, arrays, functions, and
/// primitives with `ERR_INVALID_ARG_TYPE`.
pub(crate) fn validate_object_options(arg_name: &str, value: f64) {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || is_plain_options_object(value) {
        return;
    }
    let message = format!(
        "The \"{}\" argument must be of type object. Received {}",
        arg_name,
        describe_received(value)
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
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
        let message = format!(
            "The \"options.recursive\" property must be of type boolean. Received {}",
            describe_received(recursive)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let mode_key = js_string_from_bytes(b"mode".as_ptr(), 4);
    let mode = crate::object::js_object_get_field_by_name_f64(obj, mode_key);
    let mode_jv = JSValue::from_bits(mode.to_bits());
    if !is_nullish(mode_jv) && !is_numeric(mode_jv) && !mode_jv.is_any_string() {
        let message = format!(
            "The \"options.mode\" property must be of type number. Received {}",
            describe_received(mode)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
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

pub(crate) fn fs_mode_value(value: f64) -> i32 {
    validate_fs_mode(value);
    let jv = JSValue::from_bits(value.to_bits());
    if is_nullish(jv) {
        0
    } else if jv.is_int32() {
        jv.as_int32()
    } else {
        jv.as_number() as i32
    }
}

/// Validate that `value` is a finite integer in `[min, max]`. On type or
/// range failure throws Node's `ERR_INVALID_ARG_TYPE` / `ERR_OUT_OF_RANGE`
/// with the same `Received` clause shape Node uses.
pub(crate) fn validate_int32(value: f64, arg_name: &str, min: i64, max: i64) {
    let jv = JSValue::from_bits(value.to_bits());
    if !is_numeric(jv) {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            arg_name,
            describe_received(value)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !n.is_finite() || n.fract() != 0.0 {
        let received = if n.is_nan() {
            "NaN".to_string()
        } else if n.is_infinite() {
            if n.is_sign_negative() {
                "-Infinity".to_string()
            } else {
                "Infinity".to_string()
            }
        } else {
            format_received_number(n)
        };
        let message = format!(
            "The value of \"{}\" is out of range. It must be an integer. Received {}",
            arg_name, received
        );
        throw_range_error_with_code(&message);
    }
    let i = n as i64;
    if i < min || i > max {
        let message = format!(
            "The value of \"{}\" is out of range. It must be >= {} && <= {}. Received {}",
            arg_name,
            min,
            max,
            format_received_number(n)
        );
        throw_range_error_with_code(&message);
    }
}

/// Render an `f64` the way Node prints it in a `Received …` clause for
/// value errors (`ERR_OUT_OF_RANGE` / `ERR_INVALID_ARG_VALUE`): integers
/// without a fractional part, and `NaN`/`Infinity`/`-Infinity` spelled out
/// (Rust's `Display` would print `inf`/`NaN`). Shared by the `net`/`fs`/
/// `buffer` validators and the `process` exit/cpuUsage validators
/// (#3039-#3049).
pub fn format_received_number(n: f64) -> String {
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

/// Validate that `value` is a function (closure). On failure throws
/// `TypeError [ERR_INVALID_ARG_TYPE]`. Mirrors Node's `validateFunction`
/// helper — used to catch `fs.exists(path)` / `fs.copyFile(src, dest, 0, 0)`
/// where the trailing callback is missing or the wrong type.
pub(crate) fn validate_function(arg_name: &str, value: f64) {
    if super::stream::extract_closure_ptr(value).is_null() {
        let message = format!(
            "The \"{}\" argument must be of type function. Received {}",
            arg_name,
            describe_received(value)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
}

pub fn throw_range_error_with_code(message: &str) -> ! {
    throw_range_error_named(message, "ERR_OUT_OF_RANGE")
}

/// Throw a `RangeError` carrying an explicit Node error `code`. Most callers
/// want [`throw_range_error_with_code`] (which fixes `code` to
/// `ERR_OUT_OF_RANGE`); this variant lets the `net` port validators raise
/// `ERR_SOCKET_BAD_PORT` with the same machinery (#2013).
pub fn throw_range_error_named(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw a plain `Error` (name `"Error"`) carrying an explicit Node error
/// `code`. Used by `node:crypto` Sign/Verify finalized-state guards (#2963),
/// which raise `Error [ERR_CRYPTO_INVALID_STATE]: Not initialised` once a
/// handle has been consumed by `.sign()`/`.verify()`.
pub fn throw_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

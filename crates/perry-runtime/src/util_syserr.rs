//! `node:util` system-error helpers (#2514): `getSystemErrorName`,
//! `getSystemErrorMessage`, `getSystemErrorMap`.
//!
//! These mirror libuv's error table (which is what Node exposes), NOT libc:
//! the messages are libuv's fixed lowercase phrasings (e.g. `EEXIST` →
//! "file already exists", not libc's "File exists") and the *set* is libuv's
//! `UV_ERRNO_MAP`. Codes are the libuv-style negatives (`-2` = `ENOENT`).
//!
//! Two sub-tables keep this cross-platform:
//!   * errno-backed codes carry the host `libc::E*` value (so the negative key
//!     is correct on darwin *and* linux), with libuv's platform-independent
//!     message;
//!   * libuv-internal codes (`EAI_*`, `UNKNOWN`, `EOF`, …) have no system
//!     errno, so they use libuv's fixed negative key.
//! Both message-set and the entry list were taken verbatim from Node's
//! `util.getSystemErrorMap()`.

use crate::url::create_string_f64;
use crate::value::JSValue;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// errno-backed libuv codes: `(libc errno value, name, libuv message)`.
#[cfg(unix)]
fn errno_backed() -> Vec<(i32, &'static str, &'static str)> {
    let mut t: Vec<(i32, &'static str, &'static str)> = vec![
        (libc::EPERM, "EPERM", "operation not permitted"),
        (libc::ENOENT, "ENOENT", "no such file or directory"),
        (libc::ESRCH, "ESRCH", "no such process"),
        (libc::EINTR, "EINTR", "interrupted system call"),
        (libc::EIO, "EIO", "i/o error"),
        (libc::ENXIO, "ENXIO", "no such device or address"),
        (libc::E2BIG, "E2BIG", "argument list too long"),
        (libc::ENOEXEC, "ENOEXEC", "exec format error"),
        (libc::EBADF, "EBADF", "bad file descriptor"),
        (libc::ENOMEM, "ENOMEM", "not enough memory"),
        (libc::EACCES, "EACCES", "permission denied"),
        (
            libc::EFAULT,
            "EFAULT",
            "bad address in system call argument",
        ),
        (libc::EBUSY, "EBUSY", "resource busy or locked"),
        (libc::EEXIST, "EEXIST", "file already exists"),
        (libc::EXDEV, "EXDEV", "cross-device link not permitted"),
        (libc::ENODEV, "ENODEV", "no such device"),
        (libc::ENOTDIR, "ENOTDIR", "not a directory"),
        (libc::EISDIR, "EISDIR", "illegal operation on a directory"),
        (libc::EINVAL, "EINVAL", "invalid argument"),
        (libc::ENFILE, "ENFILE", "file table overflow"),
        (libc::EMFILE, "EMFILE", "too many open files"),
        (libc::ENOTTY, "ENOTTY", "inappropriate ioctl for device"),
        (libc::ETXTBSY, "ETXTBSY", "text file is busy"),
        (libc::EFBIG, "EFBIG", "file too large"),
        (libc::ENOSPC, "ENOSPC", "no space left on device"),
        (libc::ESPIPE, "ESPIPE", "invalid seek"),
        (libc::EROFS, "EROFS", "read-only file system"),
        (libc::EMLINK, "EMLINK", "too many links"),
        (libc::EPIPE, "EPIPE", "broken pipe"),
        (libc::ERANGE, "ERANGE", "result too large"),
        (libc::EAGAIN, "EAGAIN", "resource temporarily unavailable"),
        (libc::EALREADY, "EALREADY", "connection already in progress"),
        (libc::ENOTSOCK, "ENOTSOCK", "socket operation on non-socket"),
        (
            libc::EDESTADDRREQ,
            "EDESTADDRREQ",
            "destination address required",
        ),
        (libc::EMSGSIZE, "EMSGSIZE", "message too long"),
        (
            libc::EPROTOTYPE,
            "EPROTOTYPE",
            "protocol wrong type for socket",
        ),
        (libc::ENOPROTOOPT, "ENOPROTOOPT", "protocol not available"),
        (
            libc::EPROTONOSUPPORT,
            "EPROTONOSUPPORT",
            "protocol not supported",
        ),
        (
            libc::ESOCKTNOSUPPORT,
            "ESOCKTNOSUPPORT",
            "socket type not supported",
        ),
        (
            libc::ENOTSUP,
            "ENOTSUP",
            "operation not supported on socket",
        ),
        (
            libc::EAFNOSUPPORT,
            "EAFNOSUPPORT",
            "address family not supported",
        ),
        (libc::EADDRINUSE, "EADDRINUSE", "address already in use"),
        (
            libc::EADDRNOTAVAIL,
            "EADDRNOTAVAIL",
            "address not available",
        ),
        (libc::ENETDOWN, "ENETDOWN", "network is down"),
        (libc::ENETUNREACH, "ENETUNREACH", "network is unreachable"),
        (
            libc::ECONNABORTED,
            "ECONNABORTED",
            "software caused connection abort",
        ),
        (libc::ECONNRESET, "ECONNRESET", "connection reset by peer"),
        (libc::ENOBUFS, "ENOBUFS", "no buffer space available"),
        (libc::EISCONN, "EISCONN", "socket is already connected"),
        (libc::ENOTCONN, "ENOTCONN", "socket is not connected"),
        (
            libc::ESHUTDOWN,
            "ESHUTDOWN",
            "cannot send after transport endpoint shutdown",
        ),
        (libc::ETIMEDOUT, "ETIMEDOUT", "connection timed out"),
        (libc::ECONNREFUSED, "ECONNREFUSED", "connection refused"),
        (libc::ELOOP, "ELOOP", "too many symbolic links encountered"),
        (libc::ENAMETOOLONG, "ENAMETOOLONG", "name too long"),
        (libc::EHOSTDOWN, "EHOSTDOWN", "host is down"),
        (libc::EHOSTUNREACH, "EHOSTUNREACH", "host is unreachable"),
        (libc::ENOTEMPTY, "ENOTEMPTY", "directory not empty"),
        (libc::ENOSYS, "ENOSYS", "function not implemented"),
        (
            libc::EOVERFLOW,
            "EOVERFLOW",
            "value too large for defined data type",
        ),
        (libc::ECANCELED, "ECANCELED", "operation canceled"),
        (libc::EILSEQ, "EILSEQ", "illegal byte sequence"),
        (libc::EPROTO, "EPROTO", "protocol error"),
    ];
    // BSD/darwin-only errno.
    #[cfg(target_os = "macos")]
    {
        t.push((libc::EFTYPE, "EFTYPE", "inappropriate file type or format"));
        t.push((libc::ENODATA, "ENODATA", "no data available"));
    }
    #[cfg(target_os = "linux")]
    {
        t.push((libc::ENODATA, "ENODATA", "no data available"));
        t.push((libc::EUNATCH, "EUNATCH", "protocol driver not attached"));
        t.push((libc::EREMOTEIO, "EREMOTEIO", "remote I/O error"));
        t.push((libc::ENONET, "ENONET", "machine is not on the network"));
    }
    t
}

#[cfg(windows)]
fn errno_backed() -> Vec<(i32, &'static str, &'static str)> {
    // Keyed by the POSITIVE magnitude, like the unix table above: `lookup`
    // negates it. Codes `uv_internal` already carries are skipped so the two
    // tables cannot disagree about a key.
    UV_WINDOWS_ERRNOS
        .iter()
        .filter(|(_, name, _)| !uv_internal().iter().any(|(_, other, _)| other == name))
        .map(|(errno, name, message)| (-errno, *name, *message))
        .collect()
}

#[cfg(not(any(unix, windows)))]
fn errno_backed() -> Vec<(i32, &'static str, &'static str)> {
    Vec::new()
}

/// libuv's error numbers on Windows, with libuv's messages.
///
/// `include/uv/errno.h` defines each `UV__E*` as `-errno` only on a platform
/// that has that errno and is not `_WIN32`; on Windows every code falls back to
/// a fixed negative number, so `UV__ENOENT` is `-4058` rather than `-2`. Node
/// reports those as `err.errno` there. The messages are libuv's own
/// (`UV_ERRNO_MAP` in `uv.h`) and are the same text the unix table carries —
/// `windows_and_unix_tables_agree` pins that.
///
/// The set is the filesystem-relevant one: every code `win32_error_to_uv` can
/// produce, plus the ones `io_error_code`'s `ErrorKind` fallback can name.
///
/// Only Windows reads these tables; `cfg(test)` keeps them (and the pure
/// translation below) compiled — and asserted — on every host.
#[cfg(any(windows, test))]
const UV_WINDOWS_ERRNOS: &[(i32, &str, &str)] = &[
    (-4093, "E2BIG", "argument list too long"),
    (-4092, "EACCES", "permission denied"),
    (-4088, "EAGAIN", "resource temporarily unavailable"),
    (-4083, "EBADF", "bad file descriptor"),
    (-4082, "EBUSY", "resource busy or locked"),
    (-4081, "ECANCELED", "operation canceled"),
    (-4080, "ECHARSET", "invalid Unicode character"),
    (-4075, "EEXIST", "file already exists"),
    (-4074, "EFAULT", "bad address in system call argument"),
    (-4028, "EFTYPE", "inappropriate file type or format"),
    (-4071, "EINVAL", "invalid argument"),
    (-4070, "EIO", "i/o error"),
    (-4068, "EISDIR", "illegal operation on a directory"),
    (-4067, "ELOOP", "too many symbolic links encountered"),
    (-4066, "EMFILE", "too many open files"),
    (-4064, "ENAMETOOLONG", "name too long"),
    (-4058, "ENOENT", "no such file or directory"),
    (-4057, "ENOMEM", "not enough memory"),
    (-4055, "ENOSPC", "no space left on device"),
    (-4052, "ENOTDIR", "not a directory"),
    (-4051, "ENOTEMPTY", "directory not empty"),
    (-4049, "ENOTSUP", "operation not supported on socket"),
    (-4048, "EPERM", "operation not permitted"),
    (-4047, "EPIPE", "broken pipe"),
    (-4043, "EROFS", "read-only file system"),
    (-4039, "ETIMEDOUT", "connection timed out"),
    (-4037, "EXDEV", "cross-device link not permitted"),
    (-4095, "EOF", "end of file"),
    (-4094, "UNKNOWN", "unknown error"),
];

/// libuv's `uv_translate_sys_error` (`src/win/error.c` in libuv v1.52.1, the
/// libuv node 26.5.1 ships) restricted to the Win32 errors a filesystem call
/// returns. The socket (`WSAE*`) and network `ERROR_*` arms are left out:
/// nothing on this path produces them, and an unmapped code keeps the existing
/// `ErrorKind` fallback. The Win32 names are documentation; the numbers are
/// `windows-sys`' `Win32::Foundation` values.
#[cfg(any(windows, test))]
const WIN32_TO_UV: &[(i32, &str, &str)] = &[
    (740, "ERROR_ELEVATION_REQUIRED", "EACCES"),
    (1920, "ERROR_CANT_ACCESS_FILE", "EACCES"),
    (232, "ERROR_NO_DATA", "EAGAIN"),
    (1004, "ERROR_INVALID_FLAGS", "EBADF"),
    (6, "ERROR_INVALID_HANDLE", "EBADF"),
    (33, "ERROR_LOCK_VIOLATION", "EBUSY"),
    (231, "ERROR_PIPE_BUSY", "EBUSY"),
    (32, "ERROR_SHARING_VIOLATION", "EBUSY"),
    (995, "ERROR_OPERATION_ABORTED", "ECANCELED"),
    (1113, "ERROR_NO_UNICODE_TRANSLATION", "ECHARSET"),
    (183, "ERROR_ALREADY_EXISTS", "EEXIST"),
    (80, "ERROR_FILE_EXISTS", "EEXIST"),
    (998, "ERROR_NOACCESS", "EFAULT"),
    (122, "ERROR_INSUFFICIENT_BUFFER", "EINVAL"),
    (13, "ERROR_INVALID_DATA", "EINVAL"),
    (87, "ERROR_INVALID_PARAMETER", "EINVAL"),
    (1464, "ERROR_SYMLINK_NOT_SUPPORTED", "EINVAL"),
    (1102, "ERROR_BEGINNING_OF_MEDIA", "EIO"),
    (1111, "ERROR_BUS_RESET", "EIO"),
    (23, "ERROR_CRC", "EIO"),
    (1166, "ERROR_DEVICE_DOOR_OPEN", "EIO"),
    (1165, "ERROR_DEVICE_REQUIRES_CLEANING", "EIO"),
    (1393, "ERROR_DISK_CORRUPT", "EIO"),
    (1129, "ERROR_EOM_OVERFLOW", "EIO"),
    (1101, "ERROR_FILEMARK_DETECTED", "EIO"),
    (31, "ERROR_GEN_FAILURE", "EIO"),
    (1106, "ERROR_INVALID_BLOCK_LENGTH", "EIO"),
    (1117, "ERROR_IO_DEVICE", "EIO"),
    (1104, "ERROR_NO_DATA_DETECTED", "EIO"),
    (205, "ERROR_NO_SIGNAL_SENT", "EIO"),
    (110, "ERROR_OPEN_FAILED", "EIO"),
    (1103, "ERROR_SETMARK_DETECTED", "EIO"),
    (156, "ERROR_SIGNAL_REFUSED", "EIO"),
    (1921, "ERROR_CANT_RESOLVE_FILENAME", "ELOOP"),
    (4, "ERROR_TOO_MANY_OPEN_FILES", "EMFILE"),
    (111, "ERROR_BUFFER_OVERFLOW", "ENAMETOOLONG"),
    (206, "ERROR_FILENAME_EXCED_RANGE", "ENAMETOOLONG"),
    (161, "ERROR_BAD_PATHNAME", "ENOENT"),
    // libuv maps ERROR_DIRECTORY to ENOENT, not ENOTDIR.
    (267, "ERROR_DIRECTORY", "ENOENT"),
    (203, "ERROR_ENVVAR_NOT_FOUND", "ENOENT"),
    (2, "ERROR_FILE_NOT_FOUND", "ENOENT"),
    (123, "ERROR_INVALID_NAME", "ENOENT"),
    (15, "ERROR_INVALID_DRIVE", "ENOENT"),
    (4392, "ERROR_INVALID_REPARSE_DATA", "ENOENT"),
    (126, "ERROR_MOD_NOT_FOUND", "ENOENT"),
    (3, "ERROR_PATH_NOT_FOUND", "ENOENT"),
    (8, "ERROR_NOT_ENOUGH_MEMORY", "ENOMEM"),
    (14, "ERROR_OUTOFMEMORY", "ENOMEM"),
    (82, "ERROR_CANNOT_MAKE", "ENOSPC"),
    (112, "ERROR_DISK_FULL", "ENOSPC"),
    (277, "ERROR_EA_TABLE_FULL", "ENOSPC"),
    (1100, "ERROR_END_OF_MEDIA", "ENOSPC"),
    (39, "ERROR_HANDLE_DISK_FULL", "ENOSPC"),
    (145, "ERROR_DIR_NOT_EMPTY", "ENOTEMPTY"),
    (50, "ERROR_NOT_SUPPORTED", "ENOTSUP"),
    (109, "ERROR_BROKEN_PIPE", "EOF"),
    // libuv reports a denied Win32 access as EPERM, not EACCES.
    (5, "ERROR_ACCESS_DENIED", "EPERM"),
    (1314, "ERROR_PRIVILEGE_NOT_HELD", "EPERM"),
    (230, "ERROR_BAD_PIPE", "EPIPE"),
    (233, "ERROR_PIPE_NOT_CONNECTED", "EPIPE"),
    (19, "ERROR_WRITE_PROTECT", "EROFS"),
    (121, "ERROR_SEM_TIMEOUT", "ETIMEDOUT"),
    (17, "ERROR_NOT_SAME_DEVICE", "EXDEV"),
    (1, "ERROR_INVALID_FUNCTION", "EISDIR"),
    (208, "ERROR_META_EXPANSION_TOO_LONG", "E2BIG"),
    (193, "ERROR_BAD_EXE_FORMAT", "EFTYPE"),
];

/// The libuv error number a code has on Windows (`UV__ENOENT` → `-4058`).
#[cfg(any(windows, test))]
pub(crate) fn uv_windows_errno(code: &str) -> Option<i32> {
    UV_WINDOWS_ERRNOS
        .iter()
        .find_map(|(errno, name, _)| (*name == code).then_some(*errno))
}

/// The libuv `(errno, code)` for a Win32 error, or `None` when libuv's table
/// has no filesystem arm for it (the caller then keeps its `ErrorKind`
/// fallback). Pure, so it is unit tested on every host; only `io_error_code`
/// and `io_error_errno` call it, under `cfg(windows)`.
#[cfg(any(windows, test))]
pub(crate) fn win32_error_to_uv(win32: i32) -> Option<(i32, &'static str)> {
    let code = WIN32_TO_UV
        .iter()
        .find_map(|(value, _, code)| (*value == win32).then_some(*code))?;
    Some((uv_windows_errno(code)?, code))
}

/// libuv-internal codes with no system errno — fixed negative keys.
fn uv_internal() -> &'static [(i32, &'static str, &'static str)] {
    &[
        (-3000, "EAI_ADDRFAMILY", "address family not supported"),
        (-3001, "EAI_AGAIN", "temporary failure"),
        (-3002, "EAI_BADFLAGS", "bad ai_flags value"),
        (-3003, "EAI_CANCELED", "request canceled"),
        (-3004, "EAI_FAIL", "permanent failure"),
        (-3005, "EAI_FAMILY", "ai_family not supported"),
        (-3006, "EAI_MEMORY", "out of memory"),
        (-3007, "EAI_NODATA", "no address"),
        (-3008, "EAI_NONAME", "unknown node or service"),
        (-3009, "EAI_OVERFLOW", "argument buffer overflow"),
        (
            -3010,
            "EAI_SERVICE",
            "service not available for socket type",
        ),
        (-3011, "EAI_SOCKTYPE", "socket type not supported"),
        (-3013, "EAI_BADHINTS", "invalid value for hints"),
        (-3014, "EAI_PROTOCOL", "resolved protocol is unknown"),
        #[cfg(not(target_os = "macos"))]
        (-4028, "EFTYPE", "inappropriate file type or format"),
        #[cfg(not(target_os = "linux"))]
        (-4023, "EUNATCH", "protocol driver not attached"),
        #[cfg(not(target_os = "linux"))]
        (-4030, "EREMOTEIO", "remote I/O error"),
        #[cfg(not(target_os = "linux"))]
        (-4056, "ENONET", "machine is not on the network"),
        (-4080, "ECHARSET", "invalid Unicode character"),
        (-4094, "UNKNOWN", "unknown error"),
        (-4095, "EOF", "end of file"),
    ]
}

fn group_decimal_digits(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let first = digits.len() % 3;
    if first != 0 {
        out.push_str(&digits[..first]);
        if digits.len() > first {
            out.push('_');
        }
    }
    for (idx, chunk) in digits[first..].as_bytes().chunks(3).enumerate() {
        if idx > 0 {
            out.push('_');
        }
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
    }
    out
}

fn format_out_of_range_number(n: f64) -> String {
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
    if n.fract() == 0.0 && n.abs() <= i64::MAX as f64 {
        let sign = if n.is_sign_negative() { "-" } else { "" };
        let digits = format!("{}", n.abs() as i64);
        if n.abs() > MAX_SAFE_INTEGER {
            return format!("{sign}{}", group_decimal_digits(&digits));
        }
        return format!("{sign}{digits}");
    }
    format!("{n}")
}

fn throw_invalid_err_type(value: f64) -> ! {
    let message = format!(
        "The \"err\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_err_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"err\" is out of range. It must be a negative integer. Received {}",
        format_out_of_range_number(n)
    );
    crate::fs::validate::throw_range_error_named(&message, "ERR_OUT_OF_RANGE")
}

/// Validate a JS value as the negative safe-integer libuv code Node accepts.
pub(crate) fn validate_system_error_code(value: f64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        let n = jsval.as_int32() as f64;
        if n < 0.0 {
            return jsval.as_int32() as i64;
        }
        throw_err_out_of_range(n);
    }
    if !crate::fs::validate::is_numeric(jsval) {
        throw_invalid_err_type(value);
    }
    let n = jsval.as_number();
    if n.is_finite() && n < 0.0 && n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER {
        n as i64
    } else {
        throw_err_out_of_range(n);
    }
}

/// Find `(name, message)` for a libuv-style code, if mapped.
fn lookup(code: i64) -> Option<(&'static str, &'static str)> {
    for (k, name, msg) in uv_internal() {
        if *k as i64 == code {
            return Some((name, msg));
        }
    }
    for (errno, name, msg) in errno_backed() {
        if -(errno as i64) == code {
            return Some((name, msg));
        }
    }
    None
}

pub(crate) fn system_error_name_for_code(code: i64) -> String {
    match lookup(code) {
        Some((name, _)) => name.to_string(),
        None => format!("Unknown system error {code}"),
    }
}

/// libuv's message for a libuv-style code (`-2` → "no such file or
/// directory"), if mapped. Shared with the fs error builders.
pub(crate) fn system_error_message_for_code(code: i64) -> Option<&'static str> {
    lookup(code).map(|(_, message)| message)
}

fn system_error_name(value: f64) -> String {
    let code = validate_system_error_code(value);
    system_error_name_for_code(code)
}

fn system_error_message(value: f64) -> String {
    let code = validate_system_error_code(value);
    match lookup(code) {
        Some((_, msg)) => msg.to_string(),
        None => format!("Unknown system error {code}"),
    }
}

#[no_mangle]
pub extern "C" fn js_util_get_system_error_name(value: f64) -> f64 {
    create_string_f64(&system_error_name(value))
}

#[no_mangle]
pub extern "C" fn js_util_get_system_error_message(value: f64) -> f64 {
    create_string_f64(&system_error_message(value))
}

/// `util.getSystemErrorMap()` → `Map<number, [name, message]>` over every
/// mapped code (key = libuv negative code).
#[no_mangle]
pub extern "C" fn js_util_get_system_error_map() -> f64 {
    // Combine both sub-tables into libuv-keyed (code, name, message) entries.
    let mut entries: Vec<(i64, &str, &str)> = errno_backed()
        .into_iter()
        .map(|(errno, name, msg)| (-(errno as i64), name, msg))
        .collect();
    for (k, name, msg) in uv_internal() {
        entries.push((*k as i64, name, msg));
    }

    let map = crate::map::js_map_alloc(entries.len() as u32 + 8);
    for (code, name, msg) in entries {
        // value is `[name, message]`; js_array_push_f64 may realloc → reassign.
        let mut pair = crate::array::js_array_alloc(2);
        pair = crate::array::js_array_push_f64(pair, create_string_f64(name));
        pair = crate::array::js_array_push_f64(pair, create_string_f64(msg));
        let pair_val = f64::from_bits(JSValue::array_ptr(pair).bits());
        crate::map::js_map_set(map, code as f64, pair_val);
    }
    f64::from_bits(JSValue::pointer(map as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// libuv's Windows translation is pure, so it is checked on every host —
    /// this repo cannot run Windows.
    #[test]
    fn win32_errors_translate_to_libuv_windows_codes() {
        // (Win32 code, libuv code) pairs read off `uv_translate_sys_error`.
        assert_eq!(win32_error_to_uv(2), Some((-4058, "ENOENT"))); // ERROR_FILE_NOT_FOUND
        assert_eq!(win32_error_to_uv(3), Some((-4058, "ENOENT"))); // ERROR_PATH_NOT_FOUND
        assert_eq!(win32_error_to_uv(123), Some((-4058, "ENOENT"))); // ERROR_INVALID_NAME
                                                                     // libuv maps a denied Win32 access to EPERM and ERROR_DIRECTORY to
                                                                     // ENOENT — neither is the errno name a unix reader would guess.
        assert_eq!(win32_error_to_uv(5), Some((-4048, "EPERM"))); // ERROR_ACCESS_DENIED
        assert_eq!(win32_error_to_uv(267), Some((-4058, "ENOENT"))); // ERROR_DIRECTORY
        assert_eq!(win32_error_to_uv(183), Some((-4075, "EEXIST"))); // ERROR_ALREADY_EXISTS
        assert_eq!(win32_error_to_uv(145), Some((-4051, "ENOTEMPTY"))); // ERROR_DIR_NOT_EMPTY
        assert_eq!(win32_error_to_uv(6), Some((-4083, "EBADF"))); // ERROR_INVALID_HANDLE
        assert_eq!(win32_error_to_uv(4), Some((-4066, "EMFILE"))); // ERROR_TOO_MANY_OPEN_FILES
        assert_eq!(win32_error_to_uv(32), Some((-4082, "EBUSY"))); // ERROR_SHARING_VIOLATION
        assert_eq!(win32_error_to_uv(17), Some((-4037, "EXDEV"))); // ERROR_NOT_SAME_DEVICE
        assert_eq!(win32_error_to_uv(1), Some((-4068, "EISDIR"))); // ERROR_INVALID_FUNCTION
        assert_eq!(win32_error_to_uv(112), Some((-4055, "ENOSPC"))); // ERROR_DISK_FULL
                                                                     // A socket/network arm, or anything libuv does not map, declines so the
                                                                     // caller keeps its `ErrorKind` fallback.
        assert_eq!(win32_error_to_uv(10061), None); // WSAECONNREFUSED
        assert_eq!(win32_error_to_uv(0), None);
    }

    #[test]
    fn the_windows_tables_are_consistent() {
        for (win32, win32_name, code) in WIN32_TO_UV {
            assert!(
                uv_windows_errno(code).is_some(),
                "{win32_name} maps to {code}, which UV_WINDOWS_ERRNOS does not carry"
            );
            assert_eq!(
                WIN32_TO_UV.iter().filter(|(v, _, _)| v == win32).count(),
                1,
                "{win32_name} ({win32}) is listed twice"
            );
        }
        for (errno, name, _) in UV_WINDOWS_ERRNOS {
            assert!(*errno < 0, "{name} must be a negative libuv code");
            assert_eq!(
                UV_WINDOWS_ERRNOS
                    .iter()
                    .filter(|(_, n, _)| n == name)
                    .count(),
                1,
                "{name} is listed twice"
            );
        }
    }

    /// The Windows table and the tables serving `util.getSystemErrorMessage`
    /// must not drift: a code in both says the same thing.
    #[cfg(unix)]
    #[test]
    fn windows_and_unix_tables_agree_on_messages() {
        for (_, name, message) in UV_WINDOWS_ERRNOS {
            if let Some((_, _, unix)) = errno_backed().iter().find(|(_, n, _)| n == name) {
                assert_eq!(message, unix, "{name} message drifted");
            }
            if let Some((_, _, internal)) = uv_internal().iter().find(|(_, n, _)| n == name) {
                assert_eq!(message, internal, "{name} message drifted");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn names_and_messages_match_libuv() {
        assert_eq!(system_error_name(-(libc::ENOENT as f64)), "ENOENT");
        assert_eq!(
            system_error_message(-(libc::ENOENT as f64)),
            "no such file or directory"
        );
        // libuv phrasing, NOT libc strerror:
        assert_eq!(
            system_error_message(-(libc::EEXIST as f64)),
            "file already exists"
        );
        assert_eq!(
            system_error_message(-(libc::EISDIR as f64)),
            "illegal operation on a directory"
        );
        // libuv-internal code (no system errno):
        assert_eq!(system_error_name(-4095.0), "EOF");
        assert_eq!(system_error_message(-3008.0), "unknown node or service");
        // unmapped:
        assert_eq!(system_error_name(-999999.0), "Unknown system error -999999");
    }

    #[test]
    fn range_number_format_matches_node() {
        assert_eq!(format_out_of_range_number(f64::NAN), "NaN");
        assert_eq!(format_out_of_range_number(f64::INFINITY), "Infinity");
        assert_eq!(
            format_out_of_range_number(-9_007_199_254_740_992.0),
            "-9_007_199_254_740_992"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_libuv_aliases_match_node_map() {
        let entry_count = errno_backed().len() + uv_internal().len();
        assert_eq!(entry_count, 85);
        assert_eq!(lookup(-4028).map(|(name, _)| name), Some("EFTYPE"));
        assert_eq!(
            lookup(-(libc::EUNATCH as i64)).map(|(name, _)| name),
            Some("EUNATCH")
        );
        assert_eq!(
            lookup(-(libc::EREMOTEIO as i64)).map(|(name, _)| name),
            Some("EREMOTEIO")
        );
        assert_eq!(
            lookup(-(libc::ENONET as i64)).map(|(name, _)| name),
            Some("ENONET")
        );
        assert_eq!(lookup(-4023), None);
        assert_eq!(lookup(-4030), None);
        assert_eq!(lookup(-4056), None);
    }
}

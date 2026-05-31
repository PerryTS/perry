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

#[cfg(not(unix))]
fn errno_backed() -> Vec<(i32, &'static str, &'static str)> {
    Vec::new()
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

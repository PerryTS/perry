//! POSIX credential accessors/setters for the `process` module
//! (`getuid`/`geteuid`/`getgid`/`getegid`, `setuid`/`seteuid`/`setgid`/
//! `setegid`, `setgroups`/`initgroups`/`getgroups`). Extracted from
//! `process.rs` to keep that file under the file-size limit. On non-unix
//! targets the accessors return 0 and the setters are no-ops.

use super::format_out_of_range_number;
use crate::string::StringHeader;
use crate::value::JSValue;

/// POSIX credential accessors (#1408). On non-unix targets each returns 0.
#[no_mangle]
pub extern "C" fn js_process_getuid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getuid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_geteuid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::geteuid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_getgid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getgid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}
#[no_mangle]
pub extern "C" fn js_process_getegid() -> f64 {
    #[cfg(unix)]
    unsafe {
        libc::getegid() as f64
    }
    #[cfg(not(unix))]
    {
        0.0
    }
}

/// POSIX credential setters (#2135). Each wraps the matching `libc::set*id`
/// call; non-numeric arguments and errors are silently dropped (the call
/// returns undefined), matching the "no-op stub" shape Perry uses for the
/// other unimplemented privileged process methods. On non-unix targets the
/// setters are unconditional no-ops. The runtime ignores ID-by-username
/// forms (Node accepts `process.setuid("alice")` and resolves via NSS);
/// passing a string here is a no-op — supporting the username form needs
/// `getpwnam_r` plumbing that's out of scope for the surface-level fix.
fn unix_id_arg(value: f64) -> Option<u32> {
    let v = value;
    if v.is_finite() {
        let n = v as i64;
        if n >= 0 && n <= u32::MAX as i64 {
            return Some(n as u32);
        }
    }
    None
}

/// Node-compatible argument validation for the POSIX credential setters
/// (`setuid`/`seteuid`/`setgid`/`setegid`, #2919). Node's `validateId`:
/// the `id` argument must be a number *or* a string (username/group-name);
/// anything else throws `TypeError [ERR_INVALID_ARG_TYPE]`. A numeric `id`
/// must be a non-negative integer `<= 4294967295`; a non-integer throws
/// `RangeError [ERR_OUT_OF_RANGE]` ("must be an integer") and an out-of-range
/// value throws ("must be >= 0 && <= 4294967295"). Diverges via `js_throw` on
/// a bad value.
///
/// Returns `Some(u32)` for a valid numeric id (the syscall is then attempted),
/// `None` for a valid string id (the username form is not yet resolved — that
/// remains a no-op pending `getpwnam_r` plumbing, but it no longer *throws*).
fn validate_credential_id(value: f64) -> Option<u32> {
    use crate::fs::validate::{
        describe_received, is_numeric, throw_range_error_named, throw_type_error_with_code,
    };
    let jv = JSValue::from_bits(value.to_bits());

    // A string is a valid type (username / group-name form); accept without
    // resolving it (no-op, preserving prior behavior).
    if jv.is_any_string() {
        return None;
    }

    if !is_numeric(jv) {
        let message = format!(
            "The \"id\" argument must be one of type number or string. Received {}",
            describe_received(value)
        );
        throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let n = if jv.is_int32() {
        jv.as_int32() as f64
    } else {
        jv.as_number()
    };
    if !(n.is_finite() && n.fract() == 0.0) {
        let message = format!(
            "The value of \"id\" is out of range. It must be an integer. Received {}",
            format_out_of_range_number(n)
        );
        throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
    }
    if n < 0.0 || n > u32::MAX as f64 {
        let message = format!(
            "The value of \"id\" is out of range. It must be >= 0 && <= 4294967295. Received {}",
            format_out_of_range_number(n)
        );
        throw_range_error_named(&message, "ERR_OUT_OF_RANGE");
    }
    Some(n as u32)
}

#[no_mangle]
pub extern "C" fn js_process_setuid(uid: f64) {
    let id = validate_credential_id(uid);
    #[cfg(unix)]
    {
        if let Some(id) = id {
            unsafe {
                libc::setuid(id as libc::uid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = id;
    }
}

#[no_mangle]
pub extern "C" fn js_process_seteuid(uid: f64) {
    let id = validate_credential_id(uid);
    #[cfg(unix)]
    {
        if let Some(id) = id {
            unsafe {
                libc::seteuid(id as libc::uid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = id;
    }
}

#[no_mangle]
pub extern "C" fn js_process_setgid(gid: f64) {
    let id = validate_credential_id(gid);
    #[cfg(unix)]
    {
        if let Some(id) = id {
            unsafe {
                libc::setgid(id as libc::gid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = id;
    }
}

#[no_mangle]
pub extern "C" fn js_process_setegid(gid: f64) {
    let id = validate_credential_id(gid);
    #[cfg(unix)]
    {
        if let Some(id) = id {
            unsafe {
                libc::setegid(id as libc::gid_t);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = id;
    }
}

/// `process.setgroups(groups)` — replace the calling process's
/// supplementary GID list with the IDs from a JS number array. Each non-
/// finite / out-of-range / non-numeric entry is silently skipped. On
/// non-unix targets this is a no-op (#2135).
#[no_mangle]
pub extern "C" fn js_process_setgroups(groups: f64) {
    let arr_jsval = JSValue::from_bits(groups.to_bits());
    if !arr_jsval.is_pointer() {
        return;
    }
    let arr_ptr = arr_jsval.as_pointer::<crate::array::ArrayHeader>();
    if arr_ptr.is_null() {
        return;
    }
    let len = unsafe { crate::array::js_array_length(arr_ptr) };
    #[cfg(unix)]
    {
        let mut gids: Vec<libc::gid_t> = Vec::with_capacity(len as usize);
        for i in 0..len {
            let v = unsafe { crate::array::js_array_get_f64(arr_ptr, i) };
            if let Some(id) = unix_id_arg(v) {
                gids.push(id as libc::gid_t);
            }
        }
        if !gids.is_empty() {
            unsafe {
                libc::setgroups(gids.len() as _, gids.as_ptr());
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = len;
    }
}

/// `process.initgroups(user, extra_gid)` — initialize the supplementary
/// group access list using `getgrouplist(3)`-style semantics. The first
/// argument is a username string (or numeric UID); the second is an
/// extra group ID to include. Perry today only accepts the username-as-
/// string + numeric extra_gid form via `libc::initgroups`; numeric user
/// or non-string first argument silently no-ops. Non-unix targets no-op
/// entirely (#2135).
#[no_mangle]
pub extern "C" fn js_process_initgroups(user: f64, extra_gid: f64) {
    #[cfg(unix)]
    {
        let user_jsval = JSValue::from_bits(user.to_bits());
        if !user_jsval.is_any_string() {
            return;
        }
        let user_ptr = crate::value::js_get_string_pointer_unified(user);
        if user_ptr == 0 {
            return;
        }
        let user_str_ptr = user_ptr as *const StringHeader;
        let len = unsafe { (*user_str_ptr).byte_len } as usize;
        let data = unsafe { (user_str_ptr as *const u8).add(std::mem::size_of::<StringHeader>()) };
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let Ok(name) = std::str::from_utf8(bytes) else {
            return;
        };
        let Some(extra) = unix_id_arg(extra_gid) else {
            return;
        };
        let Ok(cname) = std::ffi::CString::new(name) else {
            return;
        };
        unsafe {
            libc::initgroups(cname.as_ptr(), extra as _);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (user, extra_gid);
    }
}

/// `process.getgroups()` — supplementary group IDs the process is a member
/// of, as a JS array of numbers. Wraps `libc::getgroups(2)`; on non-unix
/// targets returns an empty array (Node throws there, but Perry's existing
/// `getuid`/etc. family already returns `0` rather than throwing on
/// Windows, so matching that shape keeps the surface consistent). #2135.
#[no_mangle]
pub extern "C" fn js_process_getgroups() -> f64 {
    #[cfg(unix)]
    let gids: Vec<u32> = unsafe {
        let count = libc::getgroups(0, std::ptr::null_mut());
        if count <= 0 {
            Vec::new()
        } else {
            let mut buf: Vec<libc::gid_t> = vec![0; count as usize];
            let got = libc::getgroups(count, buf.as_mut_ptr());
            if got <= 0 {
                Vec::new()
            } else {
                buf.truncate(got as usize);
                buf.into_iter().collect()
            }
        }
    };
    #[cfg(not(unix))]
    let gids: Vec<u32> = Vec::new();
    let arr = crate::array::js_array_alloc(gids.len() as u32);
    for g in gids {
        crate::array::js_array_push_f64(arr, g as f64);
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

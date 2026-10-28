//! Shared Node-compatible I/O error values and libuv descriptions.
use crate::string::js_string_from_bytes;

pub(crate) fn io_error_code(err: &std::io::Error) -> &'static str {
    // #10539 review: on Windows `raw_os_error` is a Win32 code, not an errno,
    // so it goes through libuv's own translation table.
    #[cfg(windows)]
    if let Some((_, code)) = err
        .raw_os_error()
        .and_then(crate::util_syserr::win32_error_to_uv)
    {
        return code;
    }
    #[cfg(unix)]
    if let Some(raw) = err.raw_os_error() {
        match raw {
            code if code == libc::ENOENT => return "ENOENT",
            code if code == libc::EACCES => return "EACCES",
            code if code == libc::EEXIST => return "EEXIST",
            code if code == libc::ENOTDIR => return "ENOTDIR",
            code if code == libc::ENOTEMPTY => return "ENOTEMPTY",
            code if code == libc::EISDIR => return "EISDIR",
            code if code == libc::EPERM => return "EPERM",
            code if code == libc::EINVAL => return "EINVAL",
            code if code == libc::ELOOP => return "ELOOP",
            code if code == libc::EINTR => return "EINTR",
            code if code == libc::ENOSPC => return "ENOSPC",
            code if code == libc::ETIMEDOUT => return "ETIMEDOUT",
            code if code == libc::EAGAIN => return "EAGAIN",
            // Descriptor- and write-side errnos. Rust has no `ErrorKind` for
            // these, so without an arm here they fall through to the
            // `ErrorKind` match below and come back as the catch-all "EIO" —
            // `fs.write()` to a closed fd reported `EIO` where Node reports
            // `EBADF`. `io_error_errno` already returns the raw errno, so only
            // the code string was wrong.
            code if code == libc::EBADF => return "EBADF",
            code if code == libc::EPIPE => return "EPIPE",
            code if code == libc::EROFS => return "EROFS",
            code if code == libc::EFBIG => return "EFBIG",
            code if code == libc::ESPIPE => return "ESPIPE",
            code if code == libc::EBUSY => return "EBUSY",
            code if code == libc::EMFILE => return "EMFILE",
            code if code == libc::ENFILE => return "ENFILE",
            code if code == libc::EXDEV => return "EXDEV",
            _ => {}
        }
    }
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => "ENOENT",
        ErrorKind::PermissionDenied => "EACCES",
        ErrorKind::AlreadyExists => "EEXIST",
        ErrorKind::InvalidInput => "EINVAL",
        ErrorKind::InvalidData => "EINVAL",
        ErrorKind::Interrupted => "EINTR",
        ErrorKind::WriteZero => "ENOSPC",
        ErrorKind::TimedOut => "ETIMEDOUT",
        ErrorKind::WouldBlock => "EAGAIN",
        ErrorKind::UnexpectedEof => "EOF",
        _ => "EIO",
    }
}

pub(crate) fn io_error_errno(err: &std::io::Error) -> i32 {
    #[cfg(unix)]
    if let Some(raw) = err.raw_os_error() {
        return -raw;
    }
    // Windows has no errno to negate: libuv gives each code a fixed negative
    // number there (`ENOENT` is -4058, not -2), and that is what node reports.
    #[cfg(windows)]
    {
        const UV_WINDOWS_EIO: i32 = -4070;
        return crate::util_syserr::uv_windows_errno(io_error_code(err)).unwrap_or(UV_WINDOWS_EIO);
    }
    #[cfg(unix)]
    match io_error_code(err) {
        "ENOENT" => -libc::ENOENT,
        "EACCES" => -libc::EACCES,
        "EEXIST" => -libc::EEXIST,
        "ENOTDIR" => -libc::ENOTDIR,
        "ENOTEMPTY" => -libc::ENOTEMPTY,
        "EISDIR" => -libc::EISDIR,
        "EPERM" => -libc::EPERM,
        "EINVAL" => -libc::EINVAL,
        "EINTR" => -libc::EINTR,
        "ENOSPC" => -libc::ENOSPC,
        "ETIMEDOUT" => -libc::ETIMEDOUT,
        "EAGAIN" => -libc::EAGAIN,
        "EBADF" => -libc::EBADF,
        "EPIPE" => -libc::EPIPE,
        "EROFS" => -libc::EROFS,
        "EFBIG" => -libc::EFBIG,
        "ESPIPE" => -libc::ESPIPE,
        "EBUSY" => -libc::EBUSY,
        "EMFILE" => -libc::EMFILE,
        "ENFILE" => -libc::ENFILE,
        "EXDEV" => -libc::EXDEV,
        _ => -libc::EIO,
    }
    #[cfg(not(any(unix, windows)))]
    match io_error_code(err) {
        "ENOENT" => -2,
        "EACCES" => -13,
        "EEXIST" => -17,
        "ENOTDIR" => -20,
        "ENOTEMPTY" => -39,
        "EISDIR" => -21,
        "EPERM" => -1,
        "EINVAL" => -22,
        "EINTR" => -4,
        "ENOSPC" => -28,
        "ETIMEDOUT" => -110,
        "EAGAIN" => -11,
        _ => -5,
    }
}

/// Attach Node's fs diagnostic fields to `err_ptr` as **own properties of the
/// error object**.
///
/// These used to be registered in six side tables keyed by the MESSAGE
/// STRING's address (`register_error_code_pub` and friends), which produced two
/// defects:
///
/// * **Wrong error.** Any `new Error(m)` built from the same message text
///   inherited the unrelated fs error's fields — `new Error(e.message).code`
///   returned `ENOENT` where node returns `undefined`, along with `.syscall`,
///   `.errno` and `.path`. Metadata belonged to the string, not the throw.
/// * **Invisible to reflection.** In node these are ordinary own properties:
///   `Object.keys(e)` is `code,errno,path,syscall`, and `JSON.stringify(e)` and
///   `{...e}` carry them. Served from a side table behind property *getters*
///   they were absent from all of it — perry returned `{}` for both, so any
///   code that logs or serialises an fs error silently lost every field.
///
/// Keying on the error object fixes both at once, and each field then reaches
/// reflection through the same path a user assignment does.
pub(crate) unsafe fn attach_fs_error_props(
    err_ptr: *mut crate::error::ErrorHeader,
    code: &str,
    errno: i32,
    syscall: &str,
    path: Option<&str>,
    dest: Option<&str>,
) {
    use crate::node_submodules::set_error_user_prop;
    let owner = err_ptr as usize;
    let put_str = |key: &str, s: &str| {
        let boxed = js_string_from_bytes(s.as_ptr(), s.len() as u32);
        set_error_user_prop(owner, key, crate::value::js_nanbox_string(boxed as i64));
    };
    // Insertion order is observable — `Object.keys`, `for…in`, `{...err}` and
    // `JSON.stringify` all report it — so install these in the same order
    // node's `uvException` does: errno, code, syscall, path, dest.
    // `errno` is numeric in node (-2 for ENOENT), not a string.
    set_error_user_prop(owner, "errno", errno as f64);
    put_str("code", code);
    put_str("syscall", syscall);
    if let Some(p) = path {
        put_str("path", p);
    }
    if let Some(d) = dest {
        put_str("dest", d);
    }
}

/// The description Node puts in an fs error message: libuv's fixed lowercase
/// phrasing for the errno ("no such file or directory"), not Rust's `Display`,
/// which reads "No such file or directory (os error 2)" (#10451). An error
/// synthesized without an OS errno keeps its own text.
pub(crate) fn fs_error_description(err: &std::io::Error) -> String {
    if err.raw_os_error().is_some() {
        let code = io_error_errno(err) as i64;
        if let Some(message) = crate::util_syserr::system_error_message_for_code(code) {
            return message.to_string();
        }
    }
    err.to_string()
}

pub(crate) unsafe fn build_fs_error_value(
    err: &std::io::Error,
    syscall: &'static str,
    path: &str,
) -> f64 {
    let code = io_error_code(err);
    let errno = io_error_errno(err);
    let desc = fs_error_description(err);
    let msg = format!("{}: {}, {} '{}'", code, desc, syscall, path);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    attach_fs_error_props(err_ptr, code, errno, syscall, Some(path), None);
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

/// Build a Node-shaped fs error carrying both `path` and `dest`, for the
/// two-path mutators (rename/copyFile/link/symlink). Node's message reads
/// `CODE: <desc>, <syscall> '<path>' -> '<dest>'` and exposes `.path`/`.dest`.
pub(crate) unsafe fn build_fs_error_value_with_dest(
    err: &std::io::Error,
    syscall: &'static str,
    path: &str,
    dest: &str,
) -> f64 {
    let code = io_error_code(err);
    let errno = io_error_errno(err);
    let desc = fs_error_description(err);
    let msg = format!("{}: {}, {} '{}' -> '{}'", code, desc, syscall, path, dest);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    attach_fs_error_props(err_ptr, code, errno, syscall, Some(path), Some(dest));
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

pub(crate) unsafe fn build_fs_error_value_no_path(
    err: &std::io::Error,
    syscall: &'static str,
) -> f64 {
    let code = io_error_code(err);
    let errno = io_error_errno(err);
    let desc = fs_error_description(err);
    let msg = format!("{}: {}, {}", code, desc, syscall);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    attach_fs_error_props(err_ptr, code, errno, syscall, None, None);
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

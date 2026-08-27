//! fs error-value construction + callback-error probes (extracted from
//! fs/mod.rs to keep it under the 2000-line cap). `use super::*` preserves
//! parent visibility.
use super::*;

pub(crate) fn io_error_code(err: &std::io::Error) -> &'static str {
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
    #[cfg(not(unix))]
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
unsafe fn attach_fs_error_props(
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

pub(crate) unsafe fn build_fs_error_value(
    err: &std::io::Error,
    syscall: &'static str,
    path: &str,
) -> f64 {
    let code = io_error_code(err);
    let errno = io_error_errno(err);
    let msg = format!("{}: {}, {} '{}'", code, err, syscall, path);
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
    let msg = format!("{}: {}, {} '{}' -> '{}'", code, err, syscall, path, dest);
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
    let msg = format!("{}: {}, {}", code, err, syscall);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    attach_fs_error_props(err_ptr, code, errno, syscall, None, None);
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

/// Probe a path for read access and produce a NaN-boxed Error if the
/// underlying syscall would fail. Returns `None` on success.
pub(crate) unsafe fn fs_callback_read_error(path_value: f64, syscall: &'static str) -> Option<f64> {
    let path = decode_path_value(path_value)?;
    match fs::metadata(&path) {
        Ok(_) => None,
        Err(err) => Some(build_fs_error_value(&err, syscall, &path)),
    }
}

/// Probe a path for lstat-style read access (does not follow symlinks).
pub(crate) unsafe fn fs_callback_lstat_error(
    path_value: f64,
    syscall: &'static str,
) -> Option<f64> {
    let path = decode_path_value(path_value)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => None,
        Err(err) => Some(build_fs_error_value(&err, syscall, &path)),
    }
}

/// Probe the parent of a path for write access. Used by write-style ops
/// where the target file is allowed to not exist yet.
pub(crate) unsafe fn fs_callback_write_parent_error(
    path_value: f64,
    syscall: &'static str,
) -> Option<f64> {
    let path = decode_path_value(path_value)?;
    let parent = std::path::Path::new(&path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    match fs::metadata(parent) {
        Ok(meta) if meta.is_dir() => None,
        Ok(_) => {
            let err =
                std::io::Error::new(std::io::ErrorKind::NotFound, "parent is not a directory");
            Some(build_fs_error_value(&err, syscall, &path))
        }
        Err(err) => Some(build_fs_error_value(&err, syscall, &path)),
    }
}

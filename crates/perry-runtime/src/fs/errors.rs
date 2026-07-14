//! fs error-value construction + callback-error probes (extracted from
//! fs/mod.rs to keep it under the 2000-line cap). `use super::*` preserves
//! parent visibility.
#![allow(unused_imports)]
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
    // Register code/syscall/path in the per-message side tables so the
    // `.code`, `.syscall`, `.path` property getters in `field_get_set`
    // surface Node-compatible values on caught errors.
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_errno(msg_ptr, errno);
    crate::node_submodules::register_error_syscall(msg_ptr, syscall);
    crate::node_submodules::register_error_path(msg_ptr, path.to_string());
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
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_errno(msg_ptr, errno);
    crate::node_submodules::register_error_syscall(msg_ptr, syscall);
    crate::node_submodules::register_error_path(msg_ptr, path.to_string());
    crate::node_submodules::register_error_dest(msg_ptr, dest.to_string());
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
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_errno(msg_ptr, errno);
    crate::node_submodules::register_error_syscall(msg_ptr, syscall);
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

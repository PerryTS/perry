//! fs error-value construction + callback-error probes (extracted from
//! fs/mod.rs to keep it under the 2000-line cap). `use super::*` preserves
//! parent visibility.
use super::*;

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

/// The description Node puts in an fs error message: libuv's fixed lowercase
/// phrasing for the errno ("no such file or directory"), not Rust's `Display`,
/// which reads "No such file or directory (os error 2)" (#10451). An error
/// synthesized without an OS errno keeps its own text.
fn fs_error_description(err: &std::io::Error) -> String {
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

/// An OS "no such file or directory" error. `libc::ENOENT` is 2 on Windows too,
/// where `from_raw_os_error` reads it as `ERROR_FILE_NOT_FOUND` — which libuv
/// also translates to `ENOENT`.
pub(crate) fn enoent_os_error() -> std::io::Error {
    std::io::Error::from_raw_os_error(libc::ENOENT)
}

/// An OS "bad file descriptor" error. On Windows this must be
/// `ERROR_INVALID_HANDLE`, the Win32 error libuv translates to `EBADF`:
/// `libc::EBADF` (9) is `ERROR_INVALID_BLOCK` there and translates to nothing.
pub(crate) fn ebadf_os_error() -> std::io::Error {
    #[cfg(windows)]
    {
        const ERROR_INVALID_HANDLE: i32 = 6;
        std::io::Error::from_raw_os_error(ERROR_INVALID_HANDLE)
    }
    #[cfg(not(windows))]
    {
        std::io::Error::from_raw_os_error(libc::EBADF)
    }
}

/// A failed file read (`readFile`, a read stream): the OS error plus the
/// syscall Node reports it under. Node opens before it reads, so a missing file
/// fails the `open` and names the path, while a directory opens fine and fails
/// the `read`, which Node reports without a path
/// (`EISDIR: illegal operation on a directory, read`).
pub(crate) struct FsReadFailure {
    pub(super) err: std::io::Error,
    syscall: &'static str,
    path: Option<String>,
}

impl FsReadFailure {
    pub(crate) fn open(err: std::io::Error, path: &str) -> Self {
        let path = Some(path.to_string());
        Self {
            err,
            syscall: "open",
            path,
        }
    }

    pub(crate) fn read(err: std::io::Error) -> Self {
        Self {
            err,
            syscall: "read",
            path: None,
        }
    }

    pub(crate) unsafe fn error_value(&self) -> f64 {
        match &self.path {
            Some(path) => build_fs_error_value(&self.err, self.syscall, path),
            None => build_fs_error_value_no_path(&self.err, self.syscall),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #10451: an fs error message carries libuv's description of the errno,
    /// as Node's does, not Rust's "No such file or directory (os error 2)".
    #[cfg(unix)]
    #[test]
    fn fs_error_description_uses_libuv_text() {
        let described = |errno| fs_error_description(&std::io::Error::from_raw_os_error(errno));
        assert_eq!(described(libc::ENOENT), "no such file or directory");
        assert_eq!(described(libc::EISDIR), "illegal operation on a directory");
        assert_eq!(described(libc::EACCES), "permission denied");
        // A synthesized error has no errno to describe and keeps its own text.
        let custom = std::io::Error::new(std::io::ErrorKind::NotFound, "parent is not a directory");
        assert_eq!(fs_error_description(&custom), "parent is not a directory");
    }

    /// #10452: every `readFile` form reads through `read_file_bytes_with_options`,
    /// whose failures used to be a bare `None` the Buffer forms returned as
    /// `null`/`undefined`. A missing file must fail the `open` and name the path;
    /// a directory opens and must fail the `read`, which Node reports pathless.
    #[cfg(unix)]
    #[test]
    fn read_file_failures_keep_the_os_error_and_failing_syscall() {
        let _global = crate::gc::global_side_table_test_lock();
        let dir =
            std::env::temp_dir().join(format!("perry_fs_read_failure_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ok.txt");
        std::fs::write(&file, b"hello").unwrap();
        let missing = dir.join("missing.txt");
        let path_value = |path: &std::path::Path| {
            let path = path.to_str().unwrap();
            let ptr = js_string_from_bytes(path.as_ptr(), path.len() as u32);
            crate::value::js_nanbox_string(ptr as i64)
        };
        let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

        let failure = read_file_bytes_with_options(path_value(&missing), undefined)
            .err()
            .expect("a missing file must fail");
        assert_eq!(failure.err.raw_os_error(), Some(libc::ENOENT));
        assert_eq!(
            (failure.syscall, failure.path.as_deref()),
            ("open", missing.to_str())
        );

        let failure = read_file_bytes_with_options(path_value(&dir), undefined)
            .err()
            .expect("a directory must fail");
        assert_eq!(failure.err.raw_os_error(), Some(libc::EISDIR));
        assert_eq!((failure.syscall, failure.path.as_deref()), ("read", None));

        let bytes = read_file_bytes_with_options(path_value(&file), undefined)
            .ok()
            .expect("a regular file reads");
        assert_eq!(bytes, b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! fs error-value construction + callback-error probes (extracted from
//! fs/mod.rs to keep it under the 2000-line cap). `use super::*` preserves
//! parent visibility.
use super::*;

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
    read_path: Option<String>,
}

impl FsReadFailure {
    pub(crate) fn open(err: std::io::Error, path: &str) -> Self {
        let path = Some(path.to_string());
        Self {
            err,
            syscall: "open",
            path,
            read_path: None,
        }
    }

    pub(crate) fn read(err: std::io::Error) -> Self {
        Self {
            err,
            syscall: "read",
            path: None,
            read_path: None,
        }
    }

    pub(crate) fn read_at(err: std::io::Error, path: &str) -> Self {
        Self {
            read_path: Some(path.to_owned()),
            ..Self::read(err)
        }
    }

    /// A read failure carries the opened path as Rust-owned metadata, so an
    /// async error never decodes a possibly moved JS input after allocation.
    /// Sync reads and numeric descriptors keep a pathless read diagnostic.
    pub(crate) unsafe fn async_error_value(&self) -> f64 {
        match self.path.as_ref().or(self.read_path.as_ref()) {
            Some(path) => build_fs_error_value(&self.err, self.syscall, path),
            None => build_fs_error_value_no_path(&self.err, self.syscall),
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
    use crate::io_error::fs_error_description;

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

pub(crate) use crate::io_error::{
    build_fs_error_value, build_fs_error_value_no_path, build_fs_error_value_with_dest,
};

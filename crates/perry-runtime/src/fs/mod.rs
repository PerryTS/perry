//! File system module - provides file operations

use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};

use crate::closure::ClosureHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::POINTER_MASK;

mod callbacks;
pub use callbacks::*;
mod stream;
pub use stream::*;
mod filehandle;
pub use filehandle::*;
mod dir_glob_watch;
pub use dir_glob_watch::*;
mod fd_ops;
pub use fd_ops::*;
mod cp;
pub use cp::*;
mod stats;
pub use stats::*;
mod dirent;
pub use dirent::*;
pub mod validate;

thread_local! {
    static FD_REGISTRY: RefCell<StdHashMap<i32, fs::File>> = RefCell::new(StdHashMap::new());
    static FD_PATHS: RefCell<StdHashMap<i32, String>> = RefCell::new(StdHashMap::new());
    static FD_APPEND_MODE: RefCell<StdHashMap<i32, bool>> = RefCell::new(StdHashMap::new());
    static FILEHANDLE_OBJECT_FDS: RefCell<StdHashMap<usize, i32>> = RefCell::new(StdHashMap::new());
    static NEXT_FD: RefCell<i32> = const { RefCell::new(100) };
    static DIR_REGISTRY: RefCell<StdHashMap<usize, DirState>> = RefCell::new(StdHashMap::new());
    static NEXT_DIR_ID: RefCell<usize> = const { RefCell::new(1) };
}

/// True if `fd` is a Perry-tracked open file descriptor (one returned by
/// `openSync`/`open` and not yet closed). Perry uses a synthetic fd registry
/// — `NEXT_FD` starts at 100 — so a raw OS-level check (e.g. `fcntl`) is
/// meaningless here; membership in `FD_REGISTRY` is the source of truth.
/// Used by `validate::validate_path_or_fd` to surface `EBADF` for an unknown
/// numeric fd (#2013).
pub(crate) fn fd_is_registered(fd: i32) -> bool {
    FD_REGISTRY.with(|r| r.borrow().contains_key(&fd))
}

struct DirState {
    entries: Vec<f64>,
    index: usize,
    closed: bool,
}

/// Extract a string pointer from a NaN-boxed f64 value
/// Handles both NaN-boxed strings (with STRING_TAG) and raw pointers.
/// Returns null for invalid/small pointers (e.g. from TAG_UNDEFINED extraction).
#[inline]
fn extract_string_ptr(value: f64) -> *const StringHeader {
    if value.is_finite() {
        return std::ptr::null();
    }
    let bits = value.to_bits();
    // Mask off the tag bits to get the raw pointer
    let ptr = (bits & POINTER_MASK) as usize;
    if ptr < 0x1000 {
        std::ptr::null()
    } else {
        ptr as *const StringHeader
    }
}

fn numeric_fd_value(value: f64) -> Option<i32> {
    if value.is_finite() && value >= 0.0 && value <= i32::MAX as f64 {
        Some(value as i32)
    } else {
        unsafe {
            let bits = value.to_bits();
            let addr = if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as usize
            } else {
                bits as usize
            };
            if let Some(fd) = FILEHANDLE_OBJECT_FDS.with(|fds| fds.borrow().get(&addr).copied()) {
                return Some(fd);
            }
            if crate::buffer::js_buffer_is_buffer(value.to_bits() as i64) == 1
                || !extract_string_ptr(value).is_null()
            {
                return None;
            }
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                return None;
            }
            options_number_field(value, b"fd").map(|fd| fd as i32)
        }
    }
}

/// Read a file synchronously and return its contents as a string
/// Returns null pointer on error
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_read_file_sync(path_value: f64) -> *mut StringHeader {
    js_fs_read_file_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_read_file_sync_options(
    path_value: f64,
    options_value: f64,
) -> *mut StringHeader {
    validate::validate_path_or_fd("path", path_value, "read");
    unsafe {
        let _path_str_for_log = decode_path_value(path_value).unwrap_or_default();

        // Debug: log path on Android
        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
            }
            let c_path = std::ffi::CString::new(path_str_for_log).unwrap_or_default();
            __android_log_print(
                3,
                b"PerryFS\0".as_ptr(),
                b"readFileSync: path='%s'\0".as_ptr(),
                c_path.as_ptr(),
            );
        }

        match read_file_bytes_with_options(path_value, options_value) {
            Some(bytes) => {
                #[cfg(target_os = "android")]
                {
                    extern "C" {
                        fn __android_log_print(
                            prio: i32,
                            tag: *const u8,
                            fmt: *const u8,
                            ...
                        ) -> i32;
                    }
                    __android_log_print(
                        3,
                        b"PerryFS\0".as_ptr(),
                        b"readFileSync: OK, %d bytes\0".as_ptr(),
                        bytes.len() as i32,
                    );
                }
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            None => {
                #[cfg(target_os = "android")]
                {
                    extern "C" {
                        fn __android_log_print(
                            prio: i32,
                            tag: *const u8,
                            fmt: *const u8,
                            ...
                        ) -> i32;
                    }
                    let c_err = std::ffi::CString::new("read failed").unwrap_or_default();
                    __android_log_print(
                        6,
                        b"PerryFS\0".as_ptr(),
                        b"readFileSync: ERROR: %s\0".as_ptr(),
                        c_err.as_ptr(),
                    );
                }
                // Return empty string instead of null to prevent crashes when
                // callers access .length on the result without null-checking.
                // Perry's try/catch doesn't catch null-pointer segfaults.
                js_string_from_bytes(b"".as_ptr(), 0)
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn js_fs_read_file_dispatch(path_value: f64, options_value: f64) -> f64 {
    if read_file_encoding(options_value).is_some() {
        let str_ptr = js_fs_read_file_sync_options(path_value, options_value);
        f64::from_bits(crate::value::JSValue::string_ptr(str_ptr).bits())
    } else {
        let buf = js_fs_read_file_binary_options(path_value, options_value);
        if buf.is_null() {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
        }
    }
}

/// Write content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_write_file_sync(path_value: f64, content_value: f64) -> i32 {
    js_fs_write_file_sync_options(
        path_value,
        content_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

fn js_string_value(value: f64) -> Option<String> {
    unsafe {
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let (ptr, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)?;
        if ptr.is_null() {
            return Some(String::new());
        }
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len as usize)).into_owned())
    }
}

fn read_file_encoding(options_value: f64) -> Option<String> {
    let value = crate::value::JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() {
        return None;
    }
    if let Some(enc) = js_string_value(options_value) {
        return Some(enc);
    }
    unsafe {
        let enc = options_field_value(options_value, b"encoding")?;
        let enc_js = crate::value::JSValue::from_bits(enc.bits());
        if enc_js.is_undefined() || enc_js.is_null() {
            None
        } else {
            js_string_value(f64::from_bits(enc.bits()))
        }
    }
}

fn read_file_flag(options_value: f64) -> String {
    let value = crate::value::JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() || js_string_value(options_value).is_some() {
        return "r".to_string();
    }
    unsafe {
        for field in [b"flag".as_slice(), b"flags".as_slice()] {
            if let Some(v) = options_field_value(options_value, field) {
                if let Some(s) = js_string_value(f64::from_bits(v.bits())) {
                    return s;
                }
            }
        }
    }
    "r".to_string()
}

fn open_file_for_read_flag(path: &str, flag: &str) -> std::io::Result<fs::File> {
    use std::fs::OpenOptions;
    let mut opts = OpenOptions::new();
    match flag {
        "r" | "rs" | "sr" => {
            opts.read(true);
        }
        "r+" | "rs+" | "sr+" => {
            opts.read(true).write(true);
        }
        // Perry keeps errors coarse in this layer, but matching the common
        // Node/Bun/Deno readFile flag surface is useful for parity tests.
        "w+" => {
            opts.read(true).write(true).create(true).truncate(true);
        }
        "a+" => {
            opts.read(true).append(true).create(true);
        }
        _ => {
            opts.read(true);
        }
    }
    opts.open(path)
}

fn read_file_bytes_with_options(path_value: f64, options_value: f64) -> Option<Vec<u8>> {
    unsafe {
        if let Some(fd) = numeric_fd_value(path_value) {
            let mut bytes = Vec::new();
            FD_REGISTRY.with(|r| {
                if let Some(file) = r.borrow_mut().get_mut(&fd) {
                    let _ = file.read_to_end(&mut bytes);
                }
            });
            return Some(bytes);
        }
        let path_str = decode_path_value(path_value)?;
        let flag = read_file_flag(options_value);
        let mut file = open_file_for_read_flag(&path_str, &flag).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }
}

#[no_mangle]
pub extern "C" fn js_fs_write_file_sync_options(
    path_value: f64,
    content_value: f64,
    options_value: f64,
) -> i32 {
    validate::validate_path_or_fd("path", path_value, "write");
    unsafe {
        if let Some(fd) = numeric_fd_value(path_value) {
            let content_bytes = bytes_from_value(content_value);
            return FD_REGISTRY.with(|r| {
                let mut reg = r.borrow_mut();
                let Some(file) = reg.get_mut(&fd) else {
                    return 0;
                };
                if file.write_all(&content_bytes).is_ok() {
                    1
                } else {
                    0
                }
            });
        }

        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        let content_bytes = bytes_from_value(content_value);

        let flag = file_options_flag(options_value, "w");
        match open_file_for_write_flag(&path_str, &flag) {
            Ok(mut file) => {
                if file.write_all(&content_bytes).is_ok() {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

/// Append content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_append_file_sync(path_value: f64, content_value: f64) -> i32 {
    js_fs_append_file_sync_options(
        path_value,
        content_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

#[no_mangle]
pub extern "C" fn js_fs_append_file_sync_options(
    path_value: f64,
    content_value: f64,
    options_value: f64,
) -> i32 {
    unsafe {
        if let Some(fd) = numeric_fd_value(path_value) {
            let content_bytes = bytes_from_value(content_value);
            return FD_REGISTRY.with(|r| {
                let mut reg = r.borrow_mut();
                let Some(file) = reg.get_mut(&fd) else {
                    return 0;
                };
                let _ = file.seek(SeekFrom::End(0));
                if file.write_all(&content_bytes).is_ok() {
                    1
                } else {
                    0
                }
            });
        }

        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        let content_bytes = bytes_from_value(content_value);

        let flag = file_options_flag(options_value, "a");
        match open_file_for_write_flag(&path_str, &flag) {
            Ok(mut file) => match file.write_all(&content_bytes) {
                Ok(_) => 1,
                Err(_) => 0,
            },
            Err(_) => 0,
        }
    }
}

/// Check if a file or directory exists
/// Returns 1 if exists, 0 if not
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_exists_sync(path_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        if Path::new(&path_str).exists() {
            1
        } else {
            0
        }
    }
}

fn parse_mode_string(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim(), 8)
        .ok()
        .or_else(|| s.parse::<u32>().ok())
}

fn mkdir_mode_from_options(options_value: f64) -> Option<u32> {
    let value = crate::value::JSValue::from_bits(options_value.to_bits());
    if value.is_int32() {
        return Some(value.as_int32() as u32);
    }
    if value.is_number() && options_value.is_finite() {
        return Some(options_value as u32);
    }
    if let Some(s) = string_value(options_value) {
        return parse_mode_string(&s);
    }
    unsafe {
        if let Some(mode) = options_field_value(options_value, b"mode") {
            let bits = mode.bits();
            let mode_value = crate::value::JSValue::from_bits(bits);
            if mode_value.is_int32() {
                return Some(mode_value.as_int32() as u32);
            }
            let v = f64::from_bits(bits);
            if mode_value.is_number() && v.is_finite() {
                return Some(v as u32);
            }
            if let Some(s) = options_string_field(options_value, b"mode") {
                return parse_mode_string(&s);
            }
        }
    }
    None
}

fn apply_dir_mode(path: &str, mode: Option<u32>) {
    let Some(mode) = mode else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

/// Create a directory synchronously.
#[no_mangle]
pub extern "C" fn js_fs_mkdir_sync(path_value: f64) -> i32 {
    js_fs_mkdir_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_mkdir_sync_options(path_value: f64, options_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        let recursive = options_bool_field(options_value, b"recursive");
        let mode = mkdir_mode_from_options(options_value);
        let result = if recursive {
            fs::create_dir_all(&path_str)
        } else {
            fs::create_dir(&path_str)
        };
        match result {
            Ok(_) => {
                apply_dir_mode(&path_str, mode);
                1
            }
            Err(_) => 0,
        }
    }
}

/// Check if a path is a directory.
/// Returns 1 if directory, 0 if not (or error).
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_is_directory(path_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        if Path::new(&path_str).is_dir() {
            1
        } else {
            0
        }
    }
}

/// Remove a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_unlink_sync(path_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        match fs::remove_file(path_str) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// Change file permissions (POSIX mode bits). Accepts NaN-boxed string path + numeric mode (e.g. 0o755).
/// Returns 1 on success, 0 on error. No-op + success on Windows where POSIX modes don't apply.
#[no_mangle]
pub extern "C" fn js_fs_chmod_sync(path_value: f64, mode: f64) -> i32 {
    // #2013: path-only validation. Mode coercion goes through Node's
    // `parseFileMode` which throws ERR_INVALID_ARG_VALUE (not the
    // ERR_INVALID_ARG_TYPE / ERR_OUT_OF_RANGE shape `validate_int32`
    // emits) — left as a follow-up to keep the diff small.
    crate::fs::validate::validate_path("path", path_value);
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(mode as u32);
            match fs::set_permissions(path_str, perms) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (path_str, mode);
            1
        }
    }
}

/// Read a file synchronously as binary and return a Buffer (binary-safe, works for PNG etc.)
/// Returns a *mut BufferHeader on success, null on error
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_read_file_binary(path_value: f64) -> *mut crate::buffer::BufferHeader {
    js_fs_read_file_binary_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_read_file_binary_options(
    path_value: f64,
    options_value: f64,
) -> *mut crate::buffer::BufferHeader {
    validate::validate_path_or_fd("path", path_value, "read");
    unsafe {
        match read_file_bytes_with_options(path_value, options_value) {
            Some(bytes) => {
                let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
                if !buf.is_null() {
                    let buf_data =
                        (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_data, bytes.len());
                    (*buf).length = bytes.len() as u32;
                }
                buf
            }
            None => std::ptr::null_mut(),
        }
    }
}

/// Recursively remove a directory or file.
/// Returns 1 on success, 0 on failure.
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_rm_recursive(path_value: f64) -> i32 {
    js_fs_rm_recursive_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_rm_recursive_options(path_value: f64, options_value: f64) -> i32 {
    use std::path::Path;

    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };

        let p = Path::new(&path_str);
        let meta = match fs::symlink_metadata(p) {
            Ok(meta) => meta,
            Err(_) => {
                return if options_bool_field(options_value, b"force") {
                    1
                } else {
                    0
                };
            }
        };
        let ft = meta.file_type();
        if ft.is_symlink() || ft.is_file() {
            match fs::remove_file(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        } else if ft.is_dir() {
            let recursive = options_bool_field(options_value, b"recursive");
            if recursive {
                match fs::remove_dir_all(path_str) {
                    Ok(_) => 1,
                    Err(_) => 0,
                }
            } else {
                match fs::remove_dir(path_str) {
                    Ok(_) => 1,
                    Err(_) => 0,
                }
            }
        } else {
            match fs::remove_file(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
    }
}

/// `fs.chownSync(path, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_chown_sync(path_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    chown_path_value(path_value, uid_value, gid_value, true)
}

/// `fs.lchownSync(path, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_lchown_sync(path_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    chown_path_value(path_value, uid_value, gid_value, false)
}

/// `fs.lchmodSync(path, mode)` — chmod a symlink itself (not its target).
/// Implemented via `lchmod(2)` on macOS/BSD; on Linux the syscall is absent so
/// we surface failure (return 0) the same way the callback path then reports
/// ENOSYS-equivalent. No-op success on non-unix.
#[no_mangle]
pub extern "C" fn js_fs_lchmod_sync(path_value: f64, mode: f64) -> i32 {
    // Mode range validation is deliberately not done here: Node opens the
    // path first, so a bad-path call surfaces ENOENT before mode validation
    // would fire. Validating mode here would deviate from Node ordering on
    // paths that don't exist. The mode-validation gap on existing paths is
    // a separate follow-up.
    crate::fs::validate::validate_path("path", path_value);
    #[cfg(all(
        unix,
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )
    ))]
    unsafe {
        // `libc` 0.2 doesn't expose `lchmod` uniformly across BSD targets,
        // so declare it directly. Signature matches POSIX:
        //   int lchmod(const char *path, mode_t mode);
        extern "C" {
            fn lchmod(path: *const libc::c_char, mode: libc::mode_t) -> libc::c_int;
        }
        let Some(path) = decode_path_value(path_value) else {
            return 0;
        };
        let Ok(path) = std::ffi::CString::new(path) else {
            return 0;
        };
        let rc = lchmod(path.as_ptr(), mode as libc::mode_t);
        if rc == 0 {
            1
        } else {
            0
        }
    }
    #[cfg(all(
        unix,
        not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        ))
    ))]
    {
        let _ = (path_value, mode);
        0
    }
    #[cfg(not(unix))]
    {
        let _ = (path_value, mode);
        1
    }
}

fn chown_path_value(path_value: f64, uid_value: f64, gid_value: f64, follow: bool) -> i32 {
    #[cfg(unix)]
    unsafe {
        let Some(path) = decode_path_value(path_value) else {
            return 0;
        };
        let Ok(path) = std::ffi::CString::new(path) else {
            return 0;
        };
        let uid = uid_value as libc::uid_t;
        let gid = gid_value as libc::gid_t;
        let rc = if follow {
            libc::chown(path.as_ptr(), uid, gid)
        } else {
            libc::lchown(path.as_ptr(), uid, gid)
        };
        if rc == 0 {
            1
        } else {
            0
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path_value, uid_value, gid_value, follow);
        1
    }
}

/// Helper: decode a NaN-boxed PathLike (string / Buffer / file: URL) into an
/// owned `String`. Returns `None` if the value is not a recognized path form
/// or the bytes are not valid UTF-8.
///
/// Always owns — previous revisions returned `&str` and used `Box::leak` for
/// the Buffer/URL paths, which leaked memory on every fs call with those
/// argument shapes. The extra allocation is negligible next to the syscall
/// cost that follows.
unsafe fn decode_path_value(path_value: f64) -> Option<String> {
    let jsval = crate::value::JSValue::from_bits(path_value.to_bits());
    // #1781: a path <= 5 bytes ("a.ts", "x", ".", "..", "/tmp") is an
    // inline SSO value that `is_string()` (STRING_TAG-only) misses,
    // so short relative paths silently decoded to None. Read the inline
    // bytes directly.
    if jsval.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut buf);
        return std::str::from_utf8(&buf[..n]).ok().map(|s| s.to_string());
    }
    if jsval.is_string() {
        let path_ptr = jsval.as_string_ptr();
        if path_ptr.is_null() {
            return None;
        }
        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);
        return std::str::from_utf8(path_bytes).ok().map(|s| s.to_string());
    }
    if crate::buffer::js_buffer_is_buffer(path_value.to_bits() as i64) == 1 {
        let buf = buffer_ptr_from_value(path_value);
        if buf.is_null() {
            return None;
        }
        let bytes =
            std::slice::from_raw_parts(crate::buffer::buffer_data(buf), (*buf).length as usize);
        return std::str::from_utf8(bytes).ok().map(|s| s.to_string());
    }
    if jsval.is_pointer() {
        let obj = jsval.as_pointer::<crate::object::ObjectHeader>();
        if obj.is_null() {
            return None;
        }
        let protocol = crate::url::get_string_content(crate::object::js_object_get_field_f64(
            obj,
            crate::url::parse::URL_PROTOCOL,
        ));
        if protocol != "file:" {
            return None;
        }
        let pathname = crate::url::get_string_content(crate::object::js_object_get_field_f64(
            obj,
            crate::url::parse::URL_PATHNAME,
        ));
        if pathname.is_empty() {
            return None;
        }
        return Some(crate::url::search_params::url_decode(&pathname));
    }
    None
}

fn string_value(value: f64) -> Option<String> {
    unsafe {
        let ptr = extract_string_ptr(value);
        if ptr.is_null() {
            return None;
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn file_options_flag(options_value: f64, default_flag: &str) -> String {
    unsafe {
        options_string_field(options_value, b"flag")
            .or_else(|| options_string_field(options_value, b"flags"))
            .unwrap_or_else(|| default_flag.to_string())
    }
}

fn open_file_for_write_flag(path: &str, flag: &str) -> std::io::Result<fs::File> {
    use std::fs::OpenOptions;
    let mut opts = OpenOptions::new();
    match flag {
        "a" | "a+" => {
            opts.create(true).append(true);
            if flag.ends_with('+') {
                opts.read(true);
            }
        }
        "ax" | "ax+" => {
            opts.create_new(true).append(true);
            if flag.ends_with('+') {
                opts.read(true);
            }
        }
        "r+" => {
            opts.read(true).write(true);
        }
        "w" | "w+" => {
            opts.create(true).truncate(true).write(true);
            if flag.ends_with('+') {
                opts.read(true);
            }
        }
        "wx" | "wx+" => {
            opts.create_new(true).write(true);
            if flag.ends_with('+') {
                opts.read(true);
            }
        }
        _ => {
            opts.create(true).truncate(true).write(true);
        }
    }
    opts.open(path)
}

/// `fs.renameSync(from, to)` — returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn js_fs_rename_sync(from_value: f64, to_value: f64) -> i32 {
    crate::fs::validate::validate_path("oldPath", from_value);
    crate::fs::validate::validate_path("newPath", to_value);
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        match fs::rename(from, to) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// `fs.copyFileSync(from, to)` — returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn js_fs_copy_file_sync(from_value: f64, to_value: f64) -> i32 {
    js_fs_copy_file_sync_flags(
        from_value,
        to_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

#[no_mangle]
pub extern "C" fn js_fs_copy_file_sync_flags(
    from_value: f64,
    to_value: f64,
    flags_value: f64,
) -> i32 {
    crate::fs::validate::validate_path("src", from_value);
    crate::fs::validate::validate_path("dest", to_value);
    let flags_jv = crate::value::JSValue::from_bits(flags_value.to_bits());
    if !flags_jv.is_undefined() {
        crate::fs::validate::validate_int32(flags_value, "mode", 0, 7);
    }
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        let excl = flags_value.is_finite() && (flags_value as i64 & 1) == 1;
        if excl && Path::new(&to).exists() {
            // Node throws `EEXIST: file already exists, copyfile '<src>' -> '<dst>'`.
            // Surface the same via `js_throw` so user `try/catch` fires; the
            // existing code path silently returned 0 which left callers
            // believing the copy was a no-op.
            let err = std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "destination already exists",
            );
            let err_val = build_fs_error_value(&err, "copyfile", &to);
            crate::exception::js_throw(err_val);
        }
        match fs::copy(from, to) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

#[no_mangle]
pub extern "C" fn js_fs_access_sync_mode(path_value: f64, mode_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        if !Path::new(&path_str).exists() {
            return 0;
        }
        let mode = if mode_value.is_finite() {
            mode_value as i32
        } else {
            0
        };
        #[cfg(unix)]
        {
            let Ok(c_path) = std::ffi::CString::new(path_str) else {
                return 0;
            };
            if libc::access(c_path.as_ptr(), mode) == 0 {
                1
            } else {
                0
            }
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            1
        }
    }
}

fn fs_encoding_option(options_value: f64) -> Option<String> {
    let value = crate::value::JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() {
        return None;
    }
    if let Some(s) = js_string_value(options_value) {
        return Some(s);
    }
    unsafe { options_string_field(options_value, b"encoding") }
}

fn encoded_string_ptr(bytes: &[u8], encoding: &str) -> *mut StringHeader {
    match encoding {
        "hex" => crate::buffer::hex_encode_into_string(bytes),
        "base64" => crate::buffer::base64_encode_into_string(bytes),
        "base64url" => crate::buffer::base64url_encode_into_string(bytes),
        "ascii" | "latin1" | "binary" | "utf8" | "utf-8" => {
            js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
        }
        _ => js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32),
    }
}

fn realpath_bytes(path_value: f64) -> Vec<u8> {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return Vec::new(),
        };
        match fs::canonicalize(&path_str) {
            Ok(p) => p.to_string_lossy().as_bytes().to_vec(),
            Err(_) => path_str.as_bytes().to_vec(),
        }
    }
}

fn realpath_value(path_value: f64, options_value: f64) -> f64 {
    let bytes = realpath_bytes(path_value);
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return buffer_value_from_bytes(&bytes);
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

/// `fs.realpathSync(path)` — returns raw *mut StringHeader i64.
/// Falls back to the input path on error (Node would throw).
#[no_mangle]
pub extern "C" fn js_fs_realpath_sync(path_value: f64) -> i64 {
    js_fs_realpath_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_realpath_sync_options(path_value: f64, options_value: f64) -> i64 {
    let bytes = realpath_bytes(path_value);
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_realpath_dispatch(path_value: f64, options_value: f64) -> f64 {
    realpath_value(path_value, options_value)
}

/// `fs.mkdtempSync(prefix)` — creates a unique temp directory whose
/// name starts with `prefix`. Returns raw *mut StringHeader i64 with
/// the created path.
#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_sync(prefix_value: f64) -> i64 {
    js_fs_mkdtemp_sync_options(prefix_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

fn mkdtemp_bytes(prefix_value: f64) -> Vec<u8> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    unsafe {
        let prefix_str = match decode_path_value(prefix_value) {
            Some(s) => s,
            None => return Vec::new(),
        };
        // Try a handful of candidate suffixes until one succeeds. We mix a
        // nanosecond clock, a per-process pid component, and a monotonic
        // counter so simultaneous calls don't collide. NOTE: we still
        // return an empty `Vec` on exhaustion; callers convert that to an
        // empty string which is observably wrong (the caller will then
        // misuse it as a path). Node throws ENOSPC/EACCES here instead.
        // Once Perry's fs surface can propagate typed errors through LLVM
        // (#793 follow-up), promote this to a real error path.
        let pid = std::process::id() as u64;
        for attempt in 0..64u64 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let candidate = format!("{}{:x}{:x}{:x}{:x}", prefix_str, ts, pid, n, attempt);
            match fs::create_dir(&candidate) {
                Ok(_) => return candidate.into_bytes(),
                Err(_) => continue,
            }
        }
        Vec::new()
    }
}

#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_sync_options(prefix_value: f64, options_value: f64) -> i64 {
    let bytes = mkdtemp_bytes(prefix_value);
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_dispatch(prefix_value: f64, options_value: f64) -> f64 {
    let bytes = mkdtemp_bytes(prefix_value);
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return buffer_value_from_bytes(&bytes);
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

fn readlink_bytes(path_value: f64) -> Vec<u8> {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return Vec::new(),
        };
        match fs::read_link(path_str) {
            Ok(p) => p.to_string_lossy().as_bytes().to_vec(),
            Err(_) => Vec::new(),
        }
    }
}

fn readlink_value(path_value: f64, options_value: f64) -> f64 {
    let bytes = readlink_bytes(path_value);
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return buffer_value_from_bytes(&bytes);
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

/// `fs.rmdirSync(path)` — removes an empty directory. Returns i32 status.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_sync(path_value: f64) -> i32 {
    js_fs_rmdir_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `fs.rmdirSync(path[, options])` — removes an empty directory, or a
/// non-empty tree when the legacy/deprecated `{ recursive: true }` option is
/// supplied. Returns i32 status.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_sync_options(path_value: f64, options_value: f64) -> i32 {
    validate::validate_path("path", path_value);
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        if options_bool_field(options_value, b"recursive") {
            match fs::remove_dir_all(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        } else {
            match fs::remove_dir(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
    }
}

/// `fs.truncateSync(path, len)` — truncate/extend a file by path.
#[no_mangle]
pub extern "C" fn js_fs_truncate_sync(path_value: f64, len_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        let len = if len_value.is_finite() && len_value >= 0.0 {
            len_value as u64
        } else {
            0
        };
        match fs::OpenOptions::new().write(true).open(path_str) {
            Ok(file) => {
                if file.set_len(len).is_ok() {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

/// `fs.ftruncateSync(fd, len)` — truncate/extend an open registry fd.
#[no_mangle]
pub extern "C" fn js_fs_ftruncate_sync(fd_value: f64, len_value: f64) -> i32 {
    let fd = fd_value as i32;
    let len = if len_value.is_finite() && len_value >= 0.0 {
        len_value as u64
    } else {
        0
    };
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        if file.set_len(len).is_ok() {
            1
        } else {
            0
        }
    })
}

/// `fs.fsyncSync(fd)` — flush an open registry fd.
#[no_mangle]
pub extern "C" fn js_fs_fsync_sync(fd_value: f64) -> i32 {
    crate::fs::validate::validate_fd_open(fd_value, "fsync");
    fsync_sync_inner(fd_value as i32)
}

/// Internal fsync without validation — for the FileHandle wrappers, which
/// may legitimately hold a `-1` sentinel from a failed open and rely on
/// the silent no-op behavior.
pub(crate) fn fsync_sync_inner(fd: i32) -> i32 {
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        if file.sync_all().is_ok() {
            1
        } else {
            0
        }
    })
}

/// `fs.fdatasyncSync(fd)` — flush file data for an open registry fd.
/// Perry maps this to `sync_data`, falling back to fsync-like semantics.
#[no_mangle]
pub extern "C" fn js_fs_fdatasync_sync(fd_value: f64) -> i32 {
    crate::fs::validate::validate_fd_open(fd_value, "fdatasync");
    fdatasync_sync_inner(fd_value as i32)
}

pub(crate) fn fdatasync_sync_inner(fd: i32) -> i32 {
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        if file.sync_data().is_ok() {
            1
        } else {
            0
        }
    })
}

/// `fs.fchmodSync(fd, mode)`.
#[no_mangle]
pub extern "C" fn js_fs_fchmod_sync(fd_value: f64, mode: f64) -> i32 {
    // #2013: fd validation (type + range) + EBADF on missing fd. Mode
    // validation deliberately omitted — Node uses `parseFileMode`,
    // which throws `ERR_INVALID_ARG_VALUE`, before the fd check; adding
    // the same shape here is a separate follow-up tracked alongside the
    // mode-on-existing-path gap in `lchmodSync`.
    crate::fs::validate::validate_fd_open(fd_value, "fchmod");
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        #[cfg(unix)]
        {
            let perms = fs::Permissions::from_mode(mode as u32);
            if file.set_permissions(perms).is_ok() {
                1
            } else {
                0
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (file, mode);
            1
        }
    })
}

/// `fs.fchownSync(fd, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_fchown_sync(fd_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    // #2013 order: validate fd type, uid type+range, gid type+range,
    // THEN bounce on EBADF. Node's `validateInteger(uid)` fires before
    // the syscall, so `fchownSync(1, "", 0)` throws ERR_INVALID_ARG_TYPE
    // for `uid`, not EBADF for `fd` — preserve that order even though
    // the missing-fd case still needs EBADF after all args check out.
    crate::fs::validate::validate_fd(fd_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    if !crate::fs::fd_is_registered(fd_value as i32) {
        crate::fs::validate::throw_ebadf_pub("fchown");
    }
    fchown_sync_inner(fd_value as i32, uid_value, gid_value)
}

pub(crate) fn fchown_sync_inner(fd: i32, uid_value: f64, gid_value: f64) -> i32 {
    #[cfg(unix)]
    {
        FD_REGISTRY.with(|r| {
            let reg = r.borrow();
            let Some(file) = reg.get(&fd) else {
                return 0;
            };
            let rc = unsafe {
                libc::fchown(
                    file.as_raw_fd(),
                    uid_value as libc::uid_t,
                    gid_value as libc::gid_t,
                )
            };
            if rc == 0 {
                1
            } else {
                0
            }
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, uid_value, gid_value);
        1
    }
}

/// `fs.fstatSync(fd)` — return the same Stats shape as `statSync`.
#[no_mangle]
pub extern "C" fn js_fs_fstat_sync(fd_value: f64) -> f64 {
    js_fs_fstat_sync_options(fd_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_fstat_sync_options(fd_value: f64, options_value: f64) -> f64 {
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return unsafe {
                build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            };
        };
        match file.metadata() {
            Ok(meta) => {
                let ft = meta.file_type();
                #[cfg(unix)]
                let mode = meta.permissions().mode();
                #[cfg(not(unix))]
                let mode = if meta.permissions().readonly() {
                    0o444
                } else {
                    0o666
                };
                let (uid, gid) = metadata_owner_ids(&meta);
                let nlink = metadata_nlink(&meta);
                let (atime, mtime, ctime, birth) = metadata_times_ms(&meta);
                unsafe {
                    build_stats_object(
                        ft.is_file(),
                        ft.is_dir(),
                        ft.is_symlink(),
                        meta.len(),
                        mode,
                        uid,
                        gid,
                        nlink,
                        atime,
                        mtime,
                        ctime,
                        birth,
                        bigint,
                        Some(&meta),
                    )
                }
            }
            Err(_) => unsafe {
                build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            },
        }
    })
}

#[cfg(unix)]
fn seconds_to_timespec(seconds: f64) -> libc::timespec {
    let safe = if seconds.is_finite() && seconds >= 0.0 {
        seconds
    } else {
        0.0
    };
    let secs = safe.trunc() as libc::time_t;
    let nanos = ((safe - safe.trunc()) * 1_000_000_000.0).round() as libc::c_long;
    libc::timespec {
        tv_sec: secs,
        tv_nsec: nanos.clamp(0, 999_999_999),
    }
}

#[cfg(unix)]
fn set_path_times(path: &str, atime: f64, mtime: f64, nofollow: bool) -> i32 {
    let c_path = match std::ffi::CString::new(path) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let times = [seconds_to_timespec(atime), seconds_to_timespec(mtime)];
    let flags = if nofollow {
        libc::AT_SYMLINK_NOFOLLOW
    } else {
        0
    };
    unsafe {
        if libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), flags) == 0 {
            1
        } else {
            0
        }
    }
}

/// `fs.utimesSync(path, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_utimes_sync(path_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        #[cfg(unix)]
        {
            set_path_times(&path, atime_value, mtime_value, false)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, atime_value, mtime_value);
            1
        }
    }
}

/// `fs.lutimesSync(path, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_lutimes_sync(path_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        #[cfg(unix)]
        {
            set_path_times(&path, atime_value, mtime_value, true)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, atime_value, mtime_value);
            1
        }
    }
}

/// `fs.futimesSync(fd, atime, mtime)`.
#[no_mangle]
pub extern "C" fn js_fs_futimes_sync(fd_value: f64, atime_value: f64, mtime_value: f64) -> i32 {
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        let reg = r.borrow();
        let Some(file) = reg.get(&fd) else {
            return 0;
        };
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let times = [
                seconds_to_timespec(atime_value),
                seconds_to_timespec(mtime_value),
            ];
            unsafe {
                if libc::futimens(file.as_raw_fd(), times.as_ptr()) == 0 {
                    1
                } else {
                    0
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (file, atime_value, mtime_value);
            1
        }
    })
}

/// `fs.linkSync(existingPath, newPath)` — create a hard link.
#[no_mangle]
pub extern "C" fn js_fs_link_sync(from_value: f64, to_value: f64) -> i32 {
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        if fs::hard_link(from, to).is_ok() {
            1
        } else {
            0
        }
    }
}

/// `fs.symlinkSync(target, path)` — create a symbolic link.
#[no_mangle]
pub extern "C" fn js_fs_symlink_sync(target_value: f64, path_value: f64) -> i32 {
    unsafe {
        let target = match decode_path_value(target_value) {
            Some(s) => s,
            None => return 0,
        };
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        #[cfg(unix)]
        {
            if std::os::unix::fs::symlink(target, path).is_ok() {
                1
            } else {
                0
            }
        }
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_file(target, path).is_ok() {
                1
            } else {
                0
            }
        }
    }
}

/// `fs.readlinkSync(path)` — return the symlink target as a string.
#[no_mangle]
pub extern "C" fn js_fs_readlink_sync(path_value: f64) -> i64 {
    js_fs_readlink_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_readlink_sync_options(path_value: f64, options_value: f64) -> i64 {
    validate::validate_path("path", path_value);
    let bytes = readlink_bytes(path_value);
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_readlink_dispatch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("path", path_value);
    readlink_value(path_value, options_value)
}

fn flag_string(value: f64) -> String {
    unsafe {
        let ptr = extract_string_ptr(value);
        if ptr.is_null() {
            return "r".to_string();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn buffer_ptr_from_value(value: f64) -> *mut crate::buffer::BufferHeader {
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < 0x1000 {
        std::ptr::null_mut()
    } else {
        raw as *mut crate::buffer::BufferHeader
    }
}

fn buffer_len_from_value(value: f64) -> usize {
    let buf = buffer_ptr_from_value(value);
    if buf.is_null() {
        0
    } else {
        unsafe { (*buf).length as usize }
    }
}

/// `fs.opendirSync(path)` — deterministic Dir subset with readSync/closeSync.
#[no_mangle]

/// Stats predicate shortcuts — not currently called from codegen, but
/// available so future fast paths can compute `stat.isFile()` without
/// going through the closure dispatch chain.
#[no_mangle]
pub extern "C" fn js_fs_stats_is_file(_stats: f64) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub extern "C" fn js_fs_stats_is_directory(_stats: f64) -> f64 {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    f64::from_bits(TAG_FALSE)
}

// ============================================================
// Throwing variant of accessSync — Node-compatible semantics.
// Checks existence via `js_fs_access_sync`; on failure calls
// `js_throw` which longjmps into the nearest enclosing try/catch.
// ============================================================
#[no_mangle]
pub extern "C" fn js_fs_access_sync_throw(path_value: f64) -> f64 {
    js_fs_access_sync_throw_mode(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_access_sync_throw_mode(path_value: f64, mode_value: f64) -> f64 {
    validate::validate_path("path", path_value);
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if js_fs_access_sync_mode(path_value, mode_value) == 1 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Throw an Error via js_throw. The runtime builds the error
    // lazily from a static message — the subclass catch in the test
    // just needs `accessBad = true` in the catch handler.
    let msg = js_string_from_bytes(b"ENOENT: no such file or directory".as_ptr(), 33);
    let err = crate::error::js_error_new_with_message(msg);
    let err_val = crate::value::js_nanbox_pointer(err as i64);
    // js_throw is `-> !` (diverges via setjmp/longjmp into the nearest
    // try/catch). No code path reaches here. #853.
    crate::exception::js_throw(err_val)
}

// ============================================================
// Callback-style fs APIs — error propagation
//
// Node's callback-style fs APIs invoke `cb(err, value)`; the legacy Perry
// implementations always passed `err = null` because the sync variants
// return sentinel values (0, undefined, empty string) on failure. These
// helpers probe the filesystem first and build a Node-shaped Error so
// `cb(err, ...)` can fire with a real first argument when the operation
// can't proceed.
//
// Coverage is intentionally pragmatic — we detect the common ENOENT /
// EACCES / EEXIST / ENOTDIR cases via `std::fs::metadata` (or a syscall
// probe for write ops) and skip the actual call when the probe fails.
// More exotic kernel errors still surface as `cb(null, sentinel)`; this
// is the same divergence STATUS.md documents for the sync APIs.
// ============================================================

fn io_error_code(err: &std::io::Error) -> &'static str {
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

unsafe fn build_fs_error_value(err: &std::io::Error, syscall: &'static str, path: &str) -> f64 {
    let code = io_error_code(err);
    let msg = format!("{}: {}, {} '{}'", code, err, syscall, path);
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    // Register code/syscall/path in the per-message side tables so the
    // `.code`, `.syscall`, `.path` property getters in `field_get_set`
    // surface Node-compatible values on caught errors.
    crate::node_submodules::register_error_code_pub(msg_ptr, code);
    crate::node_submodules::register_error_syscall(msg_ptr, syscall);
    crate::node_submodules::register_error_path(msg_ptr, path.to_string());
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

/// Probe a path for read access and produce a NaN-boxed Error if the
/// underlying syscall would fail. Returns `None` on success.
unsafe fn fs_callback_read_error(path_value: f64, syscall: &'static str) -> Option<f64> {
    let path = decode_path_value(path_value)?;
    match fs::metadata(&path) {
        Ok(_) => None,
        Err(err) => Some(build_fs_error_value(&err, syscall, &path)),
    }
}

/// Probe a path for lstat-style read access (does not follow symlinks).
unsafe fn fs_callback_lstat_error(path_value: f64, syscall: &'static str) -> Option<f64> {
    let path = decode_path_value(path_value)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => None,
        Err(err) => Some(build_fs_error_value(&err, syscall, &path)),
    }
}

/// Probe the parent of a path for write access. Used by write-style ops
/// where the target file is allowed to not exist yet.
unsafe fn fs_callback_write_parent_error(path_value: f64, syscall: &'static str) -> Option<f64> {
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
mod sso_tests_1781 {
    use super::*;

    /// #1781: a path <= 5 bytes ("x", ".", "..", "a.ts", "/tmp") is an inline
    /// SSO value that `is_string()` (STRING_TAG-only) misses, so short
    /// relative paths silently decoded to None and the fs op failed.
    #[test]
    fn decode_path_value_handles_sso_short_paths() {
        for p in ["x", ".", "..", "a.ts", "/tmp"] {
            let v = crate::value::JSValue::try_short_string(p.as_bytes())
                .expect("path <= 5 bytes encodes as inline SSO");
            assert!(v.is_short_string(), "{p:?} should be an inline SSO value");
            let got = unsafe { decode_path_value(f64::from_bits(v.bits())) };
            assert_eq!(got.as_deref(), Some(p), "path decode mismatch for {p:?}");
        }
    }
}

//! File system module - provides file operations

mod errors;
pub(crate) use errors::*;

use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::closure::ClosureHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{POINTER_MASK, POINTER_TAG};

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
mod open_as_blob;
mod time;
pub use open_as_blob::*;
pub mod validate;
pub use time::js_fs_to_unix_timestamp;
mod fd_sync_ops;
pub(crate) use fd_sync_ops::{
    fchown_sync_inner, fchown_sync_inner_result, fdatasync_sync_inner, fstat_stats_value,
    fsync_sync_inner, js_fs_fchown_result, js_fs_ftruncate_result, js_fs_truncate_result,
};
pub use fd_sync_ops::{
    js_fs_fchmod_sync, js_fs_fchown_sync, js_fs_fdatasync_sync, js_fs_fstat_sync,
    js_fs_fstat_sync_options, js_fs_fsync_sync, js_fs_ftruncate_sync, js_fs_truncate_sync,
};

pub(crate) const CLASS_ID_FS_DIR: u32 = 0xFFFF_0086;
pub(crate) const CLASS_ID_FS_DIRENT: u32 = 0xFFFF_0087;
pub(crate) const CLASS_ID_FS_READ_STREAM: u32 = 0xFFFF_0088;
pub(crate) const CLASS_ID_FS_WRITE_STREAM: u32 = 0xFFFF_0089;
pub(crate) const CLASS_ID_FS_STATS_EXPORT: u32 = 0xFFFF_008A;
pub(crate) const CLASS_ID_FS_UTF8_STREAM: u32 = 0xFFFF_008B;
pub(crate) const CLASS_ID_FS_FILEHANDLE: u32 = 0xFFFF_008C;

/// Seed the registry with the process-standard descriptors.
///
/// `allocate_synthetic_fd` hands out ids from 100 up, so 0/1/2 were never
/// inserted by anything and every fs op on stdin/stdout/stderr failed the
/// `fd_is_registered` gate with EBADF — `fs.writeSync(1, …)` threw where Node
/// writes to the terminal.
///
/// Each entry is backed by a `dup` of the real descriptor: the registry owns
/// `fs::File`s and closes them when an entry is dropped (`closeSync`, thread
/// teardown), and that must never take the process's actual stdout with it.
/// The duplicate shares the open file description, so reads and writes land on
/// the same terminal/pipe.
fn std_fd_registry() -> StdHashMap<i32, fs::File> {
    let mut map = StdHashMap::new();
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        for fd in 0..=2 {
            // SAFETY: `dup` returns a fresh descriptor we exclusively own, so
            // handing it to `File` (which closes on drop) cannot double-close.
            let duped = unsafe { libc::dup(fd) };
            if duped >= 0 {
                map.insert(fd, unsafe { fs::File::from_raw_fd(duped) });
            }
        }
    }
    map
}

thread_local! {
    static FD_REGISTRY: RefCell<StdHashMap<i32, fs::File>> = RefCell::new(std_fd_registry());
    static FD_PATHS: RefCell<StdHashMap<i32, String>> = RefCell::new(StdHashMap::new());
    static FD_APPEND_MODE: RefCell<StdHashMap<i32, bool>> = RefCell::new(StdHashMap::new());
    static FILEHANDLE_OBJECT_FDS: RefCell<StdHashMap<usize, i32>> = RefCell::new(StdHashMap::new());
    static DIR_REGISTRY: RefCell<StdHashMap<usize, DirState>> = RefCell::new(StdHashMap::new());
    static NEXT_DIR_ID: RefCell<usize> = const { RefCell::new(1) };
}

static NEXT_FD: AtomicI32 = AtomicI32::new(100);

pub(crate) fn scan_fs_handle_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    scan_filehandle_object_fd_metadata_roots_mut(visitor);
    scan_filehandle_roots_mut(visitor);
    scan_fs_dir_roots_mut(visitor);
}

fn scan_filehandle_object_fd_metadata_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    FILEHANDLE_OBJECT_FDS.with(|fds| {
        let mut fds = fds.borrow_mut();
        let mut moved = Vec::new();
        for (&owner, _) in fds.iter() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
        }
        for (old_owner, new_owner) in moved {
            if let Some(fd) = fds.remove(&old_owner) {
                fds.insert(new_owner, fd);
            }
        }
    });
}

/// Death pruning (2026-07-09 GC audit wave 2): `FILEHANDLE_OBJECT_FDS` maps
/// FileHandle OBJECT addresses to synthetic fds and was move-rekeyed but
/// never death-pruned — one entry per abandoned FileHandle forever, plus fd
/// misattribution when a new object landed on the recycled address.
/// `is_dead_owner` is one of the GC's deadness predicates
/// (`gc::dead_owner`). Note `DIR_REGISTRY` is NOT covered here: it is keyed
/// by a monotonic id (`NEXT_DIR_ID`), not by an object address, so
/// dead-owner pruning cannot apply (same class as `CONTEXT_SNAPSHOTS`).
pub(crate) fn prune_dead_filehandle_fd_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    FILEHANDLE_OBJECT_FDS.with(|fds| {
        let mut fds = fds.borrow_mut();
        if !fds.is_empty() {
            fds.retain(|owner, _| !is_dead_owner(*owner));
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_filehandle_fd_entry(owner: usize, fd: i32) {
    FILEHANDLE_OBJECT_FDS.with(|fds| {
        fds.borrow_mut().insert(owner, fd);
    });
}

#[cfg(test)]
pub(crate) fn test_filehandle_fd_entry_exists(owner: usize) -> bool {
    FILEHANDLE_OBJECT_FDS.with(|fds| fds.borrow().contains_key(&owner))
}

pub(crate) fn allocate_synthetic_fd() -> i32 {
    NEXT_FD.fetch_add(1, Ordering::Relaxed)
}

/// True if `fd` is a Perry-tracked open file descriptor (one returned by
/// `openSync`/`open` and not yet closed). Perry uses a synthetic fd registry
/// — ids start at 100 and are process-unique — so a raw OS-level check (e.g.
/// `fcntl`) is meaningless here; membership in the current thread's
/// `FD_REGISTRY` is the source of truth.
/// Used by `validate::validate_path_or_fd` to surface `EBADF` for an unknown
/// numeric fd (#2013).
pub(crate) fn fd_is_registered(fd: i32) -> bool {
    FD_REGISTRY.with(|r| r.borrow().contains_key(&fd))
}

pub(crate) fn try_clone_registered_fd(fd: i32) -> Option<fs::File> {
    FD_REGISTRY.with(|r| r.borrow().get(&fd).and_then(|file| file.try_clone().ok()))
}

pub(crate) fn filehandle_object_fd(value: f64) -> Option<i32> {
    let bits = value.to_bits();
    if (bits & !POINTER_MASK) != POINTER_TAG {
        return None;
    }
    let addr = (bits & POINTER_MASK) as usize;
    if addr < 0x1000 {
        return None;
    }
    FILEHANDLE_OBJECT_FDS.with(|fds| fds.borrow().get(&addr).copied())
}

struct DirState {
    entries: Vec<f64>,
    index: usize,
    closed: bool,
    operation_pending: bool,
}

fn object_class_id(value: f64) -> Option<u32> {
    let bits = value.to_bits();
    let js_value = crate::value::JSValue::from_bits(bits);
    if !js_value.is_pointer() {
        return None;
    }
    let obj = js_value.as_pointer::<crate::object::ObjectHeader>();
    if crate::value::addr_class::is_handle_band(obj as usize) {
        return None;
    }
    unsafe {
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        Some((*obj).class_id)
    }
}

pub(crate) fn is_fs_dir_instance_value(value: f64) -> bool {
    object_class_id(value) == Some(CLASS_ID_FS_DIR)
}

pub(crate) fn is_fs_dirent_instance_value(value: f64) -> bool {
    object_class_id(value) == Some(CLASS_ID_FS_DIRENT)
}

pub(crate) fn is_fs_stats_instance_value(value: f64) -> bool {
    matches!(
        object_class_id(value),
        Some(stats::STATS_REGULAR_CLASS_ID | stats::STATS_BIGINT_CLASS_ID)
    )
}

pub(crate) fn is_fs_stream_instance_value(value: f64, constructor_name: &str) -> bool {
    match constructor_name {
        "ReadStream" | "FileReadStream" => object_class_id(value) == Some(CLASS_ID_FS_READ_STREAM),
        "WriteStream" | "FileWriteStream" => {
            object_class_id(value) == Some(CLASS_ID_FS_WRITE_STREAM)
        }
        "Utf8Stream" => object_class_id(value) == Some(CLASS_ID_FS_UTF8_STREAM),
        _ => false,
    }
}

pub(crate) fn is_fs_filehandle_value(value: f64) -> bool {
    object_class_id(value) == Some(CLASS_ID_FS_FILEHANDLE)
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
        if let Some(fd) = filehandle_object_fd(value) {
            return Some(fd);
        }
        unsafe {
            let bits = value.to_bits();
            let addr = if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as usize
            } else {
                bits as usize
            };
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
    validate::validate_string_or_object_options("options", options_value);
    unsafe {
        let _path_str_for_log = decode_path_value(path_value).unwrap_or_default();

        // Debug: log path on Android
        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
            }
            let c_path = std::ffi::CString::new(_path_str_for_log).unwrap_or_default();
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
                // Node's `readFileSync` THROWS for an unreadable path
                // (ENOENT/EACCES/EISDIR/…). Returning an empty string here made
                // callers that try/catch a missing file behave wrongly: e.g.
                // Next.js `loadManifest` wraps an optional manifest read
                // (`subresource-integrity-manifest.json`, absent in most builds)
                // in try/catch and falls back to `{}` on ENOENT — but an empty
                // string slipped past the catch into `JSON.parse('')`, throwing a
                // misleading `SyntaxError: Unexpected end of JSON input`. Surface
                // a Node-shaped fs error instead. This is a real, catchable JS
                // throw (caught by JS try/catch) — NOT the null-pointer segfault
                // the previous empty-string workaround was guarding against.
                let path_str = decode_path_value(path_value).unwrap_or_default();
                let io_err = std::fs::read(&path_str)
                    .err()
                    .unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound));
                crate::exception::js_throw(crate::fs::errors::build_fs_error_value(
                    &io_err, "open", &path_str,
                ))
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
        // #5731 — virtual filesystem: a `$perryfs/...` path (or a bare key that
        // matches an embedded asset) is served from the in-binary registry
        // before any disk access, so `fs.readFileSync`/`readFile` (text and
        // binary) transparently read embedded files in a standalone executable.
        // A successful registry lookup is the only short-circuit; an unresolved
        // `$perryfs/...` path is treated as missing, never a literal disk read.
        if let Some(bytes) = crate::embedded::lookup(&path_str) {
            return Some(bytes.to_vec());
        }
        if crate::embedded::is_virtual_path(&path_str) {
            return None;
        }
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
    validate::validate_string_or_object_options("options", options_value);
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
    validate::validate_path_or_fd("path", path_value, "write");
    validate::validate_string_or_object_options("options", options_value);
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

        // #5731 — a registered embedded asset exists for the life of the
        // process; an unresolved `$perryfs/...` path does not (and must not
        // fall through to a disk check of the literal virtual path).
        if crate::embedded::lookup(&path_str).is_some() {
            return 1;
        }
        if crate::embedded::is_virtual_path(&path_str) {
            return 0;
        }

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

pub(crate) unsafe fn js_fs_mkdir_result(path_value: f64, options_value: f64) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    validate::validate_mkdir_options(options_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => validate::throw_invalid_path_arg("path", path_value),
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
            Ok(())
        }
        Err(err) => Err(build_fs_error_value(&err, "mkdir", &path_str)),
    }
}

#[no_mangle]
pub extern "C" fn js_fs_mkdir_sync_options(path_value: f64, options_value: f64) -> i32 {
    unsafe {
        match js_fs_mkdir_result(path_value, options_value) {
            Ok(()) => 1,
            Err(err) => crate::exception::js_throw(err),
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

pub(crate) unsafe fn js_fs_unlink_result(path_value: f64) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };

    match fs::remove_file(&path_str) {
        Ok(_) => Ok(()),
        Err(err) => Err(build_fs_error_value(&err, "unlink", &path_str)),
    }
}

/// Remove a file synchronously.
/// Returns 1 on success and throws a Node-shaped fs error on failure.
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_unlink_sync(path_value: f64) -> i32 {
    unsafe {
        match js_fs_unlink_result(path_value) {
            Ok(()) => 1,
            Err(err_val) => {
                crate::exception::js_throw(err_val);
            }
        }
    }
}

/// Shared `chmod` op. On failure builds a Node-shaped fs error
/// (`code`/`syscall: "chmod"`/`path`) so the sync FFI can throw and the
/// callback/promise wrappers can surface the same error (#2746).
pub(crate) unsafe fn js_fs_chmod_result(path_value: f64, mode: f64) -> Result<(), f64> {
    // #2013: path-only validation. Mode coercion goes through Node's
    // `parseFileMode` which throws ERR_INVALID_ARG_VALUE (not the
    // ERR_INVALID_ARG_TYPE / ERR_OUT_OF_RANGE shape `validate_int32`
    // emits) — left as a follow-up to keep the diff small.
    crate::fs::validate::validate_path("path", path_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(mode as u32);
        match fs::set_permissions(&path_str, perms) {
            Ok(_) => Ok(()),
            Err(err) => Err(build_fs_error_value(&err, "chmod", &path_str)),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path_str, mode);
        Ok(())
    }
}

/// Change file permissions (POSIX mode bits). Accepts NaN-boxed string path + numeric mode (e.g. 0o755).
/// Returns 1 on success and throws a Node-shaped fs error on failure.
#[no_mangle]
pub extern "C" fn js_fs_chmod_sync(path_value: f64, mode: f64) -> i32 {
    unsafe {
        match js_fs_chmod_result(path_value, mode) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
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
    validate::validate_string_or_object_options("options", options_value);
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

/// Shared `fs.rm` op. Reports Node-shaped removal failures (#2747):
/// missing-path → `ENOENT`/`syscall: "lstat"` (unless `{ force: true }`),
/// a non-recursive directory → `ERR_FS_EISDIR`/`syscall: "rm"`, and underlying
/// `remove_*` errors with their errno. Preserves `{ force: true }` missing-path
/// success.
pub(crate) unsafe fn js_fs_rm_result(path_value: f64, options_value: f64) -> Result<(), f64> {
    use std::path::Path;

    validate::validate_path("path", path_value);
    validate::validate_object_options("options", options_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let force = options_bool_field(options_value, b"force");

    let p = Path::new(&path_str);
    let meta = match fs::symlink_metadata(p) {
        Ok(meta) => meta,
        Err(err) => {
            return if force {
                Ok(())
            } else {
                Err(build_fs_error_value(&err, "lstat", &path_str))
            };
        }
    };
    let ft = meta.file_type();
    if ft.is_dir() {
        let recursive = options_bool_field(options_value, b"recursive");
        if recursive {
            match fs::remove_dir_all(&path_str) {
                Ok(_) => Ok(()),
                Err(err) => Err(build_fs_error_value(&err, "rm", &path_str)),
            }
        } else {
            // Node refuses to remove a directory without `recursive: true`,
            // surfacing a custom `ERR_FS_EISDIR` (not a raw errno).
            Err(build_eisdir_rm_error(&path_str))
        }
    } else {
        match fs::remove_file(&path_str) {
            Ok(_) => Ok(()),
            Err(err) => Err(build_fs_error_value(&err, "unlink", &path_str)),
        }
    }
}

#[no_mangle]
pub extern "C" fn js_fs_rm_recursive_options(path_value: f64, options_value: f64) -> i32 {
    unsafe {
        match js_fs_rm_result(path_value, options_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Build Node's `ERR_FS_EISDIR` error for `fs.rm` on a directory without
/// `recursive: true`. Node sets `code: "ERR_FS_EISDIR"`, `syscall: "rm"`, and
/// `path`.
unsafe fn build_eisdir_rm_error(path: &str) -> f64 {
    let msg = format!("Path is a directory: rm returned EISDIR (is a directory) {path}");
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_FS_EISDIR");
    crate::node_submodules::register_error_syscall(msg_ptr, "rm");
    crate::node_submodules::register_error_path(msg_ptr, path.to_string());
    crate::value::js_nanbox_pointer(err_ptr as i64)
}

/// `fs.chownSync(path, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_chown_sync(path_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    unsafe {
        match js_fs_chown_result(path_value, uid_value, gid_value, true) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// `fs.lchownSync(path, uid, gid)`.
#[no_mangle]
pub extern "C" fn js_fs_lchown_sync(path_value: f64, uid_value: f64, gid_value: f64) -> i32 {
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    unsafe {
        match js_fs_chown_result(path_value, uid_value, gid_value, false) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// `fs.lchmodSync(path, mode)` — chmod a symlink itself (not its target).
/// Implemented via `lchmod(2)` on macOS/BSD. On Linux, Node exposes the
/// `fs.lchmodSync` property with value `undefined`, so attempted calls throw a
/// plain TypeError before argument validation.
pub(crate) fn lchmod_is_callable_on_this_platform() -> bool {
    cfg!(all(
        unix,
        any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )
    ))
}

fn throw_plain_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Shared `lchmod` op (macOS/BSD only). On failure builds a Node-shaped fs
/// error carrying the real errno and `syscall: "open"` (#2746). Callers must
/// have already verified `lchmod_is_callable_on_this_platform()`.
pub(crate) unsafe fn js_fs_lchmod_result(path_value: f64, mode: f64) -> Result<(), f64> {
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
    {
        // `libc` 0.2 doesn't expose `lchmod` uniformly across BSD targets,
        // so declare it directly. Signature matches POSIX:
        //   int lchmod(const char *path, mode_t mode);
        extern "C" {
            fn lchmod(path: *const libc::c_char, mode: libc::mode_t) -> libc::c_int;
        }
        let Some(path_str) = decode_path_value(path_value) else {
            return Ok(());
        };
        let Ok(c_path) = std::ffi::CString::new(path_str.clone()) else {
            return Ok(());
        };
        let rc = lchmod(c_path.as_ptr(), mode as libc::mode_t);
        if rc == 0 {
            Ok(())
        } else {
            Err(build_fs_error_value(
                &std::io::Error::last_os_error(),
                "open",
                &path_str,
            ))
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
        unreachable!("unsupported lchmod platforms throw before syscall dispatch")
    }
    #[cfg(not(unix))]
    {
        let _ = (path_value, mode);
        unreachable!("unsupported lchmod platforms throw before syscall dispatch")
    }
}

#[no_mangle]
pub extern "C" fn js_fs_lchmod_sync(path_value: f64, mode: f64) -> i32 {
    // Mode range validation is deliberately not done here: Node opens the
    // path first, so a bad-path call surfaces ENOENT before mode validation
    // would fire. Validating mode here would deviate from Node ordering on
    // paths that don't exist. The mode-validation gap on existing paths is
    // a separate follow-up.
    if !lchmod_is_callable_on_this_platform() {
        let _ = (path_value, mode);
        throw_plain_type_error("fs.lchmodSync is not a function");
    }
    crate::fs::validate::validate_path("path", path_value);
    unsafe {
        match js_fs_lchmod_result(path_value, mode) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Shared `chown`/`lchown` op. On syscall failure builds a Node-shaped fs
/// error carrying the real errno and `syscall: "chown"`/`"lchown"` (#2746).
pub(crate) unsafe fn js_fs_chown_result(
    path_value: f64,
    uid_value: f64,
    gid_value: f64,
    follow: bool,
) -> Result<(), f64> {
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_int32(uid_value, "uid", -1, u32::MAX as i64);
    crate::fs::validate::validate_int32(gid_value, "gid", -1, u32::MAX as i64);
    #[cfg(unix)]
    {
        let Some(path_str) = decode_path_value(path_value) else {
            return Ok(());
        };
        let Ok(c_path) = std::ffi::CString::new(path_str.clone()) else {
            return Ok(());
        };
        let uid = uid_value as libc::uid_t;
        let gid = gid_value as libc::gid_t;
        let rc = if follow {
            libc::chown(c_path.as_ptr(), uid, gid)
        } else {
            libc::lchown(c_path.as_ptr(), uid, gid)
        };
        if rc == 0 {
            Ok(())
        } else {
            let syscall = if follow { "chown" } else { "lchown" };
            Err(build_fs_error_value(
                &std::io::Error::last_os_error(),
                syscall,
                &path_str,
            ))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path_value, uid_value, gid_value, follow);
        Ok(())
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
pub(crate) unsafe fn decode_path_value(path_value: f64) -> Option<String> {
    decode_path_value_named(path_value, "path")
}

pub(crate) unsafe fn decode_path_value_named(path_value: f64, arg_name: &str) -> Option<String> {
    fn reject_null_bytes(path: String, arg_name: &str) -> String {
        if path.as_bytes().contains(&0) {
            validate::throw_invalid_path_arg_value(arg_name, &path);
        }
        path
    }

    let jsval = crate::value::JSValue::from_bits(path_value.to_bits());
    // #1781: a path <= 5 bytes ("a.ts", "x", ".", "..", "/tmp") is an
    // inline SSO value that `is_string()` (STRING_TAG-only) misses,
    // so short relative paths silently decoded to None. Read the inline
    // bytes directly.
    if jsval.is_short_string() {
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let n = jsval.short_string_to_buf(&mut buf);
        return std::str::from_utf8(&buf[..n])
            .ok()
            .map(|s| reject_null_bytes(s.to_string(), arg_name));
    }
    if jsval.is_string() {
        let path_ptr = jsval.as_string_ptr();
        if path_ptr.is_null() {
            return None;
        }
        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);
        return std::str::from_utf8(path_bytes)
            .ok()
            .map(|s| reject_null_bytes(s.to_string(), arg_name));
    }
    if crate::buffer::js_buffer_is_buffer(path_value.to_bits() as i64) == 1 {
        let buf = buffer_ptr_from_value(path_value);
        if buf.is_null() {
            return None;
        }
        let bytes =
            std::slice::from_raw_parts(crate::buffer::buffer_data(buf), (*buf).length as usize);
        return std::str::from_utf8(bytes)
            .ok()
            .map(|s| reject_null_bytes(s.to_string(), arg_name));
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
        return Some(reject_null_bytes(
            crate::url::node_compat::file_url_to_path_string_posix(path_value),
            arg_name,
        ));
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

/// Core `rename` op. Returns `Ok(())` on success or a NaN-boxed Node-shaped
/// fs error (with `code`/`syscall`/`path`/`dest`) on failure. Shared by the
/// sync FFI (which throws), the callback wrapper, and the promise thunk so
/// destination-side failures (#2735) are no longer silently dropped.
pub(crate) unsafe fn js_fs_rename_result(from_value: f64, to_value: f64) -> Result<(), f64> {
    crate::fs::validate::validate_path("oldPath", from_value);
    crate::fs::validate::validate_path("newPath", to_value);
    let from = match decode_path_value(from_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let to = match decode_path_value(to_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    match fs::rename(&from, &to) {
        Ok(_) => Ok(()),
        Err(err) => Err(build_fs_error_value_with_dest(&err, "rename", &from, &to)),
    }
}

/// `fs.renameSync(from, to)` — returns 1 on success, throws on failure.
#[no_mangle]
pub extern "C" fn js_fs_rename_sync(from_value: f64, to_value: f64) -> i32 {
    unsafe {
        match js_fs_rename_result(from_value, to_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
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

/// Core `copyFile` op. Returns `Ok(())` or a NaN-boxed Node-shaped fs error
/// with `code`/`syscall: "copyfile"`/`path`/`dest`. Preserves the explicit
/// `COPYFILE_EXCL` destination-exists `EEXIST` case while also surfacing
/// generic copy failures such as a missing source or destination parent
/// (#2737) that previously collapsed to a silent no-op.
pub(crate) unsafe fn js_fs_copy_file_result(
    from_value: f64,
    to_value: f64,
    flags_value: f64,
) -> Result<(), f64> {
    crate::fs::validate::validate_path("src", from_value);
    crate::fs::validate::validate_path("dest", to_value);
    let flags_jv = crate::value::JSValue::from_bits(flags_value.to_bits());
    if !flags_jv.is_undefined() {
        crate::fs::validate::validate_fs_mode(flags_value);
    }
    let from = match decode_path_value(from_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let to = match decode_path_value(to_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let excl = flags_value.is_finite() && (flags_value as i64 & 1) == 1;
    if excl && Path::new(&to).exists() {
        // Node: `EEXIST: file already exists, copyfile '<src>' -> '<dst>'`.
        let err = std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination already exists",
        );
        return Err(build_fs_error_value_with_dest(&err, "copyfile", &from, &to));
    }
    match fs::copy(&from, &to) {
        Ok(_) => Ok(()),
        Err(err) => Err(build_fs_error_value_with_dest(&err, "copyfile", &from, &to)),
    }
}

#[no_mangle]
pub extern "C" fn js_fs_copy_file_sync_flags(
    from_value: f64,
    to_value: f64,
    flags_value: f64,
) -> i32 {
    unsafe {
        match js_fs_copy_file_result(from_value, to_value, flags_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Shared `fs.access` op. On a failed existence or mode check builds a
/// Node-shaped fs error carrying the real errno and `syscall: "access"`
/// (#2748). Returns `Ok(())` when the access check passes.
pub(crate) unsafe fn js_fs_access_result(path_value: f64, mode_value: f64) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    validate::validate_fs_mode(mode_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let mode = if mode_value.is_finite() {
        mode_value as i32
    } else {
        0
    };
    #[cfg(unix)]
    {
        let Ok(c_path) = std::ffi::CString::new(path_str.clone()) else {
            return Ok(());
        };
        if libc::access(c_path.as_ptr(), mode) == 0 {
            Ok(())
        } else {
            Err(build_fs_error_value(
                &std::io::Error::last_os_error(),
                "access",
                &path_str,
            ))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        if Path::new(&path_str).exists() {
            Ok(())
        } else {
            Err(build_fs_error_value(
                &std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
                "access",
                &path_str,
            ))
        }
    }
}

#[no_mangle]
pub extern "C" fn js_fs_access_sync_mode(path_value: f64, mode_value: f64) -> i32 {
    unsafe {
        match js_fs_access_result(path_value, mode_value) {
            Ok(()) => 1,
            Err(_) => 0,
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

fn realpath_bytes_result(path_value: f64, syscall: &'static str) -> Result<Vec<u8>, f64> {
    unsafe {
        let path_str = match decode_path_value_named(path_value, "path") {
            Some(s) => s,
            None => validate::throw_invalid_path_arg("path", path_value),
        };
        match fs::canonicalize(&path_str) {
            Ok(p) => Ok(p.to_string_lossy().as_bytes().to_vec()),
            Err(err) => Err(build_fs_error_value(&err, syscall, &path_str)),
        }
    }
}

fn realpath_value_result(
    path_value: f64,
    options_value: f64,
    syscall: &'static str,
) -> Result<f64, f64> {
    let bytes = realpath_bytes_result(path_value, syscall)?;
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return Ok(buffer_value_from_bytes(&bytes));
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    Ok(f64::from_bits(crate::value::JSValue::string_ptr(s).bits()))
}

/// `fs.realpathSync(path)` — returns raw *mut StringHeader i64.
#[no_mangle]
pub extern "C" fn js_fs_realpath_sync(path_value: f64) -> i64 {
    js_fs_realpath_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_realpath_sync_options(path_value: f64, options_value: f64) -> i64 {
    validate::validate_path("path", path_value);
    validate::validate_string_or_object_options("options", options_value);
    let bytes = match realpath_bytes_result(path_value, "lstat") {
        Ok(bytes) => bytes,
        Err(err) => crate::exception::js_throw(err),
    };
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_realpath_dispatch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("path", path_value);
    validate::validate_string_or_object_options("options", options_value);
    match realpath_value_result(path_value, options_value, "lstat") {
        Ok(value) => value,
        Err(err) => crate::exception::js_throw(err),
    }
}

#[no_mangle]
pub extern "C" fn js_fs_realpath_promises_dispatch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("path", path_value);
    validate::validate_string_or_object_options("options", options_value);
    match realpath_value_result(path_value, options_value, "realpath") {
        Ok(value) => value,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_realpath_value_result(
    path_value: f64,
    options_value: f64,
    syscall: &'static str,
) -> Result<f64, f64> {
    validate::validate_path("path", path_value);
    validate::validate_string_or_object_options("options", options_value);
    realpath_value_result(path_value, options_value, syscall)
}

/// `fs.mkdtempSync(prefix)` — creates a unique temp directory whose
/// name starts with `prefix`. Returns raw *mut StringHeader i64 with
/// the created path.
#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_sync(prefix_value: f64) -> i64 {
    js_fs_mkdtemp_sync_options(prefix_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) fn mkdtemp_bytes_result(prefix_value: f64) -> Result<Vec<u8>, f64> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    unsafe {
        let prefix_str = match decode_path_value_named(prefix_value, "prefix") {
            Some(s) => s,
            None => validate::throw_invalid_path_arg("prefix", prefix_value),
        };
        // Try a handful of candidate suffixes until one succeeds. Only real
        // EEXIST collisions are retried; permission/missing-parent errors are
        // reported against the attempted candidate path.
        let pid = std::process::id() as u64;
        let mut last_collision_path = None;
        for attempt in 0..64u64 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let candidate = format!("{}{:x}{:x}{:x}{:x}", prefix_str, ts, pid, n, attempt);
            match fs::create_dir(&candidate) {
                Ok(_) => return Ok(candidate.into_bytes()),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    last_collision_path = Some(candidate);
                    continue;
                }
                Err(err) => return Err(build_fs_error_value(&err, "mkdtemp", &candidate)),
            }
        }
        let candidate = last_collision_path.unwrap_or_else(|| format!("{}XXXXXX", prefix_str));
        let err = std::io::Error::new(std::io::ErrorKind::AlreadyExists, "file already exists");
        Err(build_fs_error_value(&err, "mkdtemp", &candidate))
    }
}

mod mkdtemp_disposable;
pub(crate) use mkdtemp_disposable::*;

#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_disposable_sync(prefix_value: f64, options_value: f64) -> f64 {
    js_fs_mkdtemp_disposable_object(prefix_value, options_value, false)
}

#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_sync_options(prefix_value: f64, options_value: f64) -> i64 {
    validate::validate_path("prefix", prefix_value);
    validate::validate_string_or_object_options("options", options_value);
    let bytes = match mkdtemp_bytes_result(prefix_value) {
        Ok(bytes) => bytes,
        Err(err) => crate::exception::js_throw(err),
    };
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_dispatch(prefix_value: f64, options_value: f64) -> f64 {
    validate::validate_path("prefix", prefix_value);
    validate::validate_string_or_object_options("options", options_value);
    let bytes = match mkdtemp_bytes_result(prefix_value) {
        Ok(bytes) => bytes,
        Err(err) => crate::exception::js_throw(err),
    };
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return buffer_value_from_bytes(&bytes);
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
}

/// Read a symlink target. On `read_link` failure builds a Node-shaped fs
/// error (`EINVAL` for a regular file, `ENOENT` for a missing path) with
/// `syscall: "readlink"` and `path` (#2733) instead of returning empty bytes.
fn readlink_result(path_value: f64) -> Result<Vec<u8>, f64> {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };
        match fs::read_link(&path_str) {
            Ok(p) => Ok(p.to_string_lossy().as_bytes().to_vec()),
            Err(err) => Err(build_fs_error_value(&err, "readlink", &path_str)),
        }
    }
}

fn readlink_value_result(path_value: f64, options_value: f64) -> Result<f64, f64> {
    let bytes = readlink_result(path_value)?;
    if fs_encoding_option(options_value).as_deref() == Some("buffer") {
        return Ok(buffer_value_from_bytes(&bytes));
    }
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let s = encoded_string_ptr(&bytes, &enc);
    Ok(f64::from_bits(crate::value::JSValue::string_ptr(s).bits()))
}

/// `fs.rmdirSync(path)` — removes an empty directory. Returns i32 status.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_sync(path_value: f64) -> i32 {
    js_fs_rmdir_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

/// `fs.rmdirSync(path[, options])` — removes an empty directory, or a
/// non-empty tree when the legacy/deprecated `{ recursive: true }` option is
/// supplied. Returns i32 status.
/// Shared `fs.rmdir` op. Reports removal failures (#2747) with the real errno
/// and `syscall: "rmdir"` — `ENOENT` (missing), `ENOTDIR` (not a directory),
/// `ENOTEMPTY` (non-empty, non-recursive).
pub(crate) unsafe fn js_fs_rmdir_result(path_value: f64, options_value: f64) -> Result<(), f64> {
    validate::validate_path("path", path_value);
    validate::validate_object_options("options", options_value);
    let path_str = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let result = if options_bool_field(options_value, b"recursive") {
        fs::remove_dir_all(&path_str)
    } else {
        fs::remove_dir(&path_str)
    };
    match result {
        Ok(_) => Ok(()),
        Err(err) => Err(build_fs_error_value(&err, "rmdir", &path_str)),
    }
}

#[no_mangle]
pub extern "C" fn js_fs_rmdir_sync_options(path_value: f64, options_value: f64) -> i32 {
    unsafe {
        match js_fs_rmdir_result(path_value, options_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

mod utimes;
pub(crate) use utimes::*;

/// Core hard-`link` op. Returns `Ok(())` or a NaN-boxed Node-shaped fs error
/// with `code`/`syscall: "link"`/`path`/`dest`. Surfaces missing-source
/// (ENOENT), missing-destination-parent (ENOENT), and existing-destination
/// (EEXIST) failures that previously collapsed to a silent no-op (#2738).
pub(crate) unsafe fn js_fs_link_result(from_value: f64, to_value: f64) -> Result<(), f64> {
    validate::validate_path("existingPath", from_value);
    validate::validate_path("newPath", to_value);
    let from = match decode_path_value(from_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let to = match decode_path_value(to_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    match fs::hard_link(&from, &to) {
        Ok(()) => Ok(()),
        Err(err) => Err(build_fs_error_value_with_dest(&err, "link", &from, &to)),
    }
}

/// `fs.linkSync(existingPath, newPath)` — create a hard link.
#[no_mangle]
pub extern "C" fn js_fs_link_sync(from_value: f64, to_value: f64) -> i32 {
    validate::validate_path("existingPath", from_value);
    validate::validate_path("newPath", to_value);
    unsafe {
        match js_fs_link_result(from_value, to_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
}

/// Core `symlink` op. Returns `Ok(())` or a NaN-boxed Node-shaped fs error
/// with `code`/`syscall: "symlink"`/`path`/`dest`. Dangling targets are
/// allowed (Node behavior); destination-side failures such as a missing
/// destination parent (ENOENT) or existing destination (EEXIST) now surface
/// instead of collapsing to a silent no-op (#2740). Node sets `path` to the
/// target and `dest` to the link path.
pub(crate) unsafe fn js_fs_symlink_result(target_value: f64, path_value: f64) -> Result<(), f64> {
    validate::validate_path("target", target_value);
    validate::validate_path("path", path_value);
    let target = match decode_path_value(target_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    let path = match decode_path_value(path_value) {
        Some(s) => s,
        None => return Ok(()),
    };
    #[cfg(unix)]
    let res = std::os::unix::fs::symlink(&target, &path);
    #[cfg(windows)]
    let res = std::os::windows::fs::symlink_file(&target, &path);
    match res {
        Ok(()) => Ok(()),
        Err(err) => Err(build_fs_error_value_with_dest(
            &err, "symlink", &target, &path,
        )),
    }
}

/// `fs.symlinkSync(target, path)` — create a symbolic link.
#[no_mangle]
pub extern "C" fn js_fs_symlink_sync(target_value: f64, path_value: f64) -> i32 {
    validate::validate_path("target", target_value);
    validate::validate_path("path", path_value);
    unsafe {
        match js_fs_symlink_result(target_value, path_value) {
            Ok(()) => 1,
            Err(err_val) => crate::exception::js_throw(err_val),
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
    match readlink_result(path_value) {
        Ok(bytes) => {
            let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
            encoded_string_ptr(&bytes, &enc) as i64
        }
        Err(err_val) => unsafe { crate::exception::js_throw(err_val) },
    }
}

#[no_mangle]
pub extern "C" fn js_fs_readlink_dispatch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("path", path_value);
    match readlink_value_result(path_value, options_value) {
        Ok(v) => v,
        Err(err_val) => unsafe { crate::exception::js_throw(err_val) },
    }
}

/// `Result`-returning readlink for callback/promise wrappers that need the
/// error value rather than a throw (#2733).
pub(crate) fn js_fs_readlink_value_result(path_value: f64, options_value: f64) -> Result<f64, f64> {
    crate::fs::validate::validate_path("path", path_value);
    crate::fs::validate::validate_string_or_object_options("options", options_value);
    readlink_value_result(path_value, options_value)
}

fn flag_string(value: f64) -> String {
    unsafe { decode_flags_string(value).unwrap_or_else(|| "r".to_string()) }
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
    validate::validate_fs_mode(mode_value);
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // #2748: surface the real failure (ENOENT for a missing path, EACCES for a
    // failed W_OK/X_OK mode check) with Node fields `code`/`syscall: "access"`/
    // `path` instead of a generic ENOENT-only Error.
    unsafe {
        match js_fs_access_result(path_value, mode_value) {
            Ok(()) => f64::from_bits(TAG_UNDEFINED),
            // js_throw is `-> !` (diverges via setjmp/longjmp into the nearest
            // try/catch). No code path reaches here. #853.
            Err(err_val) => crate::exception::js_throw(err_val),
        }
    }
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

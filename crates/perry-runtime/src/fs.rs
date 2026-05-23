//! File system module - provides file operations

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::POINTER_MASK;

thread_local! {
    static FD_REGISTRY: RefCell<StdHashMap<i32, fs::File>> = RefCell::new(StdHashMap::new());
    static FD_PATHS: RefCell<StdHashMap<i32, String>> = RefCell::new(StdHashMap::new());
    static FD_APPEND_MODE: RefCell<StdHashMap<i32, bool>> = RefCell::new(StdHashMap::new());
    static FILEHANDLE_OBJECT_FDS: RefCell<StdHashMap<usize, i32>> = RefCell::new(StdHashMap::new());
    static NEXT_FD: RefCell<i32> = const { RefCell::new(100) };
    static DIR_REGISTRY: RefCell<StdHashMap<usize, DirState>> = RefCell::new(StdHashMap::new());
    static NEXT_DIR_ID: RefCell<usize> = const { RefCell::new(1) };
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
    unsafe {
        let path_str_for_log = decode_path_value(path_value).unwrap_or_default();

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

// ---------- Dirent object ----------
//
// Issue #631: `fs.readdirSync(path, { withFileTypes: true })` returns
// `Dirent[]` instead of `string[]`. Each Dirent has a `name` field plus
// `isFile()` / `isDirectory()` / `isSymbolicLink()` predicate methods —
// same shape as Stats but populated per-entry from the OS directory
// iterator's file type. Predicate closures capture the pre-computed
// boolean so calling them is a single-slot read.

unsafe fn build_dirent_object(
    name: &str,
    parent_path: &str,
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
) -> f64 {
    use crate::string::js_string_from_bytes;
    use crate::value::js_nanbox_string;

    // Field slots: name, parentPath, path, isFile, isDirectory, isSymbolicLink.
    let obj = crate::object::js_object_alloc(0, 6);

    let set = |field: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };

    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    set("name", js_nanbox_string(name_ptr as i64));

    // `parentPath` is the new (Node 20+) name; `path` is the deprecated
    // alias still used by older code. Set both for compatibility.
    let pp_ptr = js_string_from_bytes(parent_path.as_ptr(), parent_path.len() as u32);
    let pp_nan = js_nanbox_string(pp_ptr as i64);
    set("parentPath", pp_nan);
    set("path", pp_nan);

    set("isFile", make_stats_predicate(is_file));
    set("isDirectory", make_stats_predicate(is_dir));
    set("isSymbolicLink", make_stats_predicate(is_symlink));

    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (obj as u64 & 0x0000_FFFF_FFFF_FFFF))
}

/// Decode a NaN-boxed object's `withFileTypes` field as a boolean.
/// Returns false when the options arg is undefined / not an object /
/// the field is absent or falsy.
unsafe fn options_with_file_types(options_value: f64) -> bool {
    let bits = options_value.to_bits();
    let value = crate::value::JSValue::from_bits(bits);
    let raw_ptr = if value.is_pointer() {
        value.as_pointer::<crate::object::ObjectHeader>() as usize
    } else if bits >> 48 == 0x0000 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        return false;
    };
    if raw_ptr < 0x1000 {
        return false;
    }
    let obj_ptr = raw_ptr as *const crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return false;
    }
    let key = crate::string::js_string_from_bytes(b"withFileTypes".as_ptr(), 13);
    let val = crate::object::js_object_get_field_by_name(obj_ptr, key);
    crate::value::js_is_truthy(f64::from_bits(val.bits())) != 0
}

unsafe fn options_bool_field(options_value: f64, field: &[u8]) -> bool {
    let Some(val) = options_field_value(options_value, field) else {
        return false;
    };
    crate::value::js_is_truthy(f64::from_bits(val.bits())) != 0
}

unsafe fn options_number_field(options_value: f64, field: &[u8]) -> Option<f64> {
    let val = options_field_value(options_value, field)?;
    let js = crate::value::JSValue::from_bits(val.bits());
    if js.is_int32() {
        return Some(js.as_int32() as f64);
    }
    let n = f64::from_bits(val.bits());
    if js.is_number() && n.is_finite() {
        Some(n)
    } else {
        None
    }
}

unsafe fn options_has_field(options_value: f64, field: &[u8]) -> bool {
    options_field_value(options_value, field).is_some()
}

unsafe fn options_field_value(options_value: f64, field: &[u8]) -> Option<crate::value::JSValue> {
    let bits = options_value.to_bits();
    let value = crate::value::JSValue::from_bits(bits);
    let raw_ptr = if value.is_pointer() {
        value.as_pointer::<crate::object::ObjectHeader>() as usize
    } else if bits >> 48 == 0x0000 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        return None;
    };
    if raw_ptr < 0x1000 {
        return None;
    }
    let obj_ptr = raw_ptr as *const crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return None;
    }
    let keys = (*obj_ptr).keys_array;
    if !keys.is_null() {
        let key_count = crate::array::js_array_length(keys) as usize;
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        for i in 0..key_count {
            let key_val = crate::array::js_array_get_f64(keys, i as u32);
            if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key_val, &mut scratch) {
                if !ptr.is_null() && std::slice::from_raw_parts(ptr, len as usize) == field {
                    return Some(crate::object::js_object_get_field(
                        obj_ptr as *mut _,
                        i as u32,
                    ));
                }
            }
        }
    }
    let key = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let val = crate::object::js_object_get_field_by_name(obj_ptr, key);
    if val.bits() == crate::value::TAG_UNDEFINED {
        None
    } else {
        Some(val)
    }
}

unsafe fn options_string_field(options_value: f64, field: &[u8]) -> Option<String> {
    let val = options_field_value(options_value, field)?;
    let val_bits = val.bits();
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some((ptr, len)) =
        crate::string::str_bytes_from_jsvalue(f64::from_bits(val_bits), &mut scratch)
    {
        if ptr.is_null() {
            return Some(String::new());
        }
        return Some(
            String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len as usize)).into_owned(),
        );
    }
    let ptr = if (val_bits >> 48) == 0 && val_bits > 4096 {
        (val_bits & POINTER_MASK) as *const StringHeader
    } else {
        extract_string_ptr(f64::from_bits(val_bits))
    };
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
}

fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if !buf.is_null() {
        unsafe {
            let data = crate::buffer::buffer_data_mut(buf);
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
            (*buf).length = bytes.len() as u32;
        }
    }
    f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
}

fn bytes_to_readdir_value(bytes: &[u8], as_buffer: bool) -> f64 {
    if as_buffer {
        buffer_value_from_bytes(bytes)
    } else {
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        crate::value::js_nanbox_string(str_ptr as i64)
    }
}

fn readdir_encoding_buffer(options_value: f64) -> bool {
    let value = crate::value::JSValue::from_bits(options_value.to_bits());
    if value.is_undefined() || value.is_null() {
        return false;
    }
    if let Some(s) = js_string_value(options_value) {
        return s == "buffer";
    }
    unsafe {
        options_field_value(options_value, b"encoding")
            .and_then(|v| js_string_value(f64::from_bits(v.bits())))
            .is_some_and(|s| s == "buffer")
    }
}

fn collect_readdir_recursive_strings(root: &Path, current: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    // Capture (path, is_dir) from the DirEntry itself — calling `Path::is_dir`
    // later would issue a second stat syscall per entry.
    let mut items: Vec<(std::path::PathBuf, bool)> = entries
        .flatten()
        .filter_map(|e| {
            let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
            Some((e.path(), is_dir))
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, _) in &items {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
    for (path, is_dir) in items {
        if is_dir {
            collect_readdir_recursive_strings(root, &path, out);
        }
    }
}

fn collect_readdir_recursive_dirents(
    current: &Path,
    out: &mut Vec<(String, String, bool, bool, bool)>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let mut dirs = Vec::new();
    for path in &paths {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let ft = meta.file_type();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let parent = path
            .parent()
            .unwrap_or(current)
            .to_string_lossy()
            .into_owned();
        out.push((
            name.to_string(),
            parent,
            ft.is_file(),
            ft.is_dir(),
            ft.is_symlink(),
        ));
        if ft.is_dir() {
            dirs.push(path.clone());
        }
    }
    for path in dirs {
        collect_readdir_recursive_dirents(&path, out);
    }
}

/// Read directory entries synchronously. By default returns an array of
/// string filenames. With `{ withFileTypes: true }` as the second arg,
/// returns an array of Dirent objects (each with `name`, `parentPath`
/// and `isFile()` / `isDirectory()` / `isSymbolicLink()` methods),
/// matching Node's `fs.readdirSync(path, options)` shape (issue #631).
/// Returns an empty array on error.
#[no_mangle]
pub extern "C" fn js_fs_readdir_sync(path_value: f64, options_value: f64) -> f64 {
    use crate::array::{js_array_alloc, js_array_push_f64};

    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                let arr = js_array_alloc(0);
                return f64::from_bits(i64::cast_unsigned(arr as i64));
            }
        };

        let with_file_types = options_with_file_types(options_value);
        let recursive = options_bool_field(options_value, b"recursive");
        let encoding_buffer = readdir_encoding_buffer(options_value);

        match fs::read_dir(&path_str) {
            Ok(entries) => {
                if recursive && !with_file_types {
                    let mut names = Vec::new();
                    collect_readdir_recursive_strings(
                        Path::new(&path_str),
                        Path::new(&path_str),
                        &mut names,
                    );
                    let mut arr = js_array_alloc(names.len() as u32);
                    for name in &names {
                        let bytes = name.as_bytes();
                        arr =
                            js_array_push_f64(arr, bytes_to_readdir_value(bytes, encoding_buffer));
                    }
                    return f64::from_bits(i64::cast_unsigned(arr as i64));
                }
                if with_file_types {
                    if recursive {
                        let mut items = Vec::new();
                        collect_readdir_recursive_dirents(Path::new(&path_str), &mut items);
                        let mut arr = js_array_alloc(items.len() as u32);
                        for (name, parent, is_file, is_dir, is_symlink) in &items {
                            let dirent =
                                build_dirent_object(name, parent, *is_file, *is_dir, *is_symlink);
                            arr = js_array_push_f64(arr, dirent);
                        }
                        return f64::from_bits(i64::cast_unsigned(arr as i64));
                    }
                    // Dirent path: collect (name, file_type) pairs first
                    // so we can sort by name without losing the type info.
                    let mut items: Vec<(String, std::fs::FileType)> = Vec::new();
                    for e in entries.flatten() {
                        if let Some(name) = e.file_name().to_str() {
                            if let Ok(ft) = e.file_type() {
                                items.push((name.to_string(), ft));
                            }
                        }
                    }
                    items.sort_by(|a, b| a.0.cmp(&b.0));

                    let mut arr = js_array_alloc(items.len() as u32);
                    for (name, ft) in &items {
                        let dirent = build_dirent_object(
                            name,
                            &path_str,
                            ft.is_file(),
                            ft.is_dir(),
                            ft.is_symlink(),
                        );
                        arr = js_array_push_f64(arr, dirent);
                    }
                    f64::from_bits(i64::cast_unsigned(arr as i64))
                } else {
                    let mut names: Vec<String> = Vec::new();
                    for e in entries.flatten() {
                        if let Some(name) = e.file_name().to_str() {
                            names.push(name.to_string());
                        }
                    }
                    names.sort();

                    let mut arr = js_array_alloc(names.len() as u32);
                    for name in &names {
                        let bytes = name.as_bytes();
                        arr =
                            js_array_push_f64(arr, bytes_to_readdir_value(bytes, encoding_buffer));
                    }
                    f64::from_bits(i64::cast_unsigned(arr as i64))
                }
            }
            Err(_) => {
                let arr = js_array_alloc(0);
                f64::from_bits(i64::cast_unsigned(arr as i64))
            }
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
    chown_path_value(path_value, uid_value, gid_value, false)
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

// ---------- Stats object ----------
//
// `fs.statSync(path)` returns a Node-style Stats object supporting
// `isFile()`, `isDirectory()`, `isSymbolicLink()` methods and a numeric
// `size` property. We implement it as a plain ObjectHeader populated
// with three closure fields (one per predicate) and a size field. The
// closures capture a pre-computed boolean result so calling them just
// returns the stored value via `js_closure_get_capture_f64`.

extern "C" fn stats_closure_return_captured(closure: *const crate::closure::ClosureHeader) -> f64 {
    // Slot 0 holds the pre-computed NaN-boxed boolean.
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

unsafe fn make_stats_predicate(value: bool) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let tag = if value { TAG_TRUE } else { TAG_FALSE };
    let closure = crate::closure::js_closure_alloc(stats_closure_return_captured as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(tag));
    // NaN-box the closure pointer with POINTER_TAG so the dynamic
    // dispatch path in `js_native_call_method` can unwrap it.
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (closure as u64 & 0x0000_FFFF_FFFF_FFFF))
}

fn bigint_u64_value(value: u64) -> f64 {
    let ptr = crate::bigint::js_bigint_from_u64(value);
    crate::value::js_nanbox_bigint(ptr as i64)
}

fn bigint_i64_value(value: i64) -> f64 {
    let ptr = crate::bigint::js_bigint_from_i64(value);
    crate::value::js_nanbox_bigint(ptr as i64)
}

// Pre-packed Stats key lists. Null-separated bytes are the format
// `js_object_alloc_class_with_keys` expects; the shape cache builds the
// JS keys array once and reuses it across every `statSync` invocation.
//
// Class IDs are reserved for Perry's runtime-internal Stats shapes:
//   - 0xFE5C: regular Stats (numeric fields)
//   - 0xFE5D: bigint Stats (adds *Ns fields)
//
// Field order MUST match the order writes are emitted below.
const STATS_KEYS_REGULAR: &[u8] = b"isFile\0isDirectory\0isSymbolicLink\0size\0atimeMs\0mtimeMs\0ctimeMs\0birthtimeMs\0mode\0uid\0gid\0nlink\0dev\0rdev\0blksize\0ino\0blocks\0";
const STATS_REGULAR_COUNT: u32 = 17;
const STATS_REGULAR_CLASS_ID: u32 = 0xFFFF_0070;

const STATS_KEYS_BIGINT: &[u8] = b"isFile\0isDirectory\0isSymbolicLink\0size\0atimeMs\0mtimeMs\0ctimeMs\0birthtimeMs\0atimeNs\0mtimeNs\0ctimeNs\0birthtimeNs\0mode\0uid\0gid\0nlink\0dev\0rdev\0blksize\0ino\0blocks\0";
const STATS_BIGINT_COUNT: u32 = 21;
const STATS_BIGINT_CLASS_ID: u32 = 0xFFFF_0071;

unsafe fn build_stats_object(
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    mode: u32,
    uid: f64,
    gid: f64,
    nlink: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
    bigint: bool,
    meta_extra: Option<&fs::Metadata>,
) -> f64 {
    let (dev, rdev, blksize, ino, blocks) = metadata_node_extra_fields(meta_extra);
    // Real nanosecond timestamps when we have a Metadata in hand; otherwise
    // fall back to the millisecond × 1e6 approximation below.
    let times_ns = meta_extra.map(metadata_times_ns);

    let (obj, count) = if bigint {
        let o = crate::object::js_object_alloc_class_with_keys(
            STATS_BIGINT_CLASS_ID,
            0,
            STATS_BIGINT_COUNT,
            STATS_KEYS_BIGINT.as_ptr(),
            (STATS_KEYS_BIGINT.len() - 1) as u32,
        );
        (o, STATS_BIGINT_COUNT)
    } else {
        let o = crate::object::js_object_alloc_class_with_keys(
            STATS_REGULAR_CLASS_ID,
            0,
            STATS_REGULAR_COUNT,
            STATS_KEYS_REGULAR.as_ptr(),
            (STATS_KEYS_REGULAR.len() - 1) as u32,
        );
        (o, STATS_REGULAR_COUNT)
    };
    let _ = count;
    let set = |idx: u32, v: f64| {
        crate::object::js_object_set_field_f64(obj, idx, v);
    };
    set(0, make_stats_predicate(is_file));
    set(1, make_stats_predicate(is_dir));
    set(2, make_stats_predicate(is_symlink));
    if bigint {
        let (a_ns, m_ns, c_ns, b_ns) = times_ns.unwrap_or((
            (atime_ms as i64).saturating_mul(1_000_000),
            (mtime_ms as i64).saturating_mul(1_000_000),
            (ctime_ms as i64).saturating_mul(1_000_000),
            (birthtime_ms as i64).saturating_mul(1_000_000),
        ));
        set(3, bigint_u64_value(size));
        set(4, bigint_i64_value(atime_ms as i64));
        set(5, bigint_i64_value(mtime_ms as i64));
        set(6, bigint_i64_value(ctime_ms as i64));
        set(7, bigint_i64_value(birthtime_ms as i64));
        set(8, bigint_i64_value(a_ns));
        set(9, bigint_i64_value(m_ns));
        set(10, bigint_i64_value(c_ns));
        set(11, bigint_i64_value(b_ns));
        set(12, bigint_u64_value(mode as u64));
        set(13, bigint_i64_value(uid as i64));
        set(14, bigint_i64_value(gid as i64));
        set(15, bigint_i64_value(nlink as i64));
        set(16, bigint_u64_value(dev));
        set(17, bigint_u64_value(rdev));
        set(18, bigint_u64_value(blksize));
        set(19, bigint_u64_value(ino));
        set(20, bigint_u64_value(blocks));
    } else {
        set(3, size as f64);
        set(4, atime_ms);
        set(5, mtime_ms);
        set(6, ctime_ms);
        set(7, birthtime_ms);
        set(8, mode as f64);
        set(9, uid);
        set(10, gid);
        set(11, nlink);
        set(12, dev as f64);
        set(13, rdev as f64);
        set(14, blksize as f64);
        set(15, ino as f64);
        set(16, blocks as f64);
    }
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (obj as u64 & 0x0000_FFFF_FFFF_FFFF))
}

fn system_time_ms(time: std::io::Result<std::time::SystemTime>) -> f64 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn metadata_times_ms(meta: &fs::Metadata) -> (f64, f64, f64, f64) {
    let atime = system_time_ms(meta.accessed());
    let mtime = system_time_ms(meta.modified());
    let birth = system_time_ms(meta.created());
    let ctime = unix_ctime_ms(meta).unwrap_or(mtime);
    (atime, mtime, ctime, birth)
}

#[cfg(unix)]
fn unix_ctime_ms(meta: &fs::Metadata) -> Option<f64> {
    // `MetadataExt::ctime` is seconds since epoch; combine with the
    // nanosecond fraction so we don't drop sub-second precision in the
    // ms conversion. Matches Node's stat.ctimeMs on POSIX.
    let secs = meta.ctime();
    let nsecs = meta.ctime_nsec().max(0) as f64;
    Some(secs as f64 * 1000.0 + nsecs / 1_000_000.0)
}

#[cfg(not(unix))]
fn unix_ctime_ms(_meta: &fs::Metadata) -> Option<f64> {
    None
}

/// Nanosecond timestamps for `bigint: true` Stats. On Unix we read the
/// real `*time_nsec` fields directly; elsewhere we fall back to the
/// millisecond × 1_000_000 approximation.
#[cfg(unix)]
fn metadata_times_ns(meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    let to_ns = |secs: i64, nsecs: i64| -> i64 {
        secs.saturating_mul(1_000_000_000)
            .saturating_add(nsecs.max(0))
    };
    let a = to_ns(meta.atime(), meta.atime_nsec());
    let m = to_ns(meta.mtime(), meta.mtime_nsec());
    let c = to_ns(meta.ctime(), meta.ctime_nsec());
    // birthtime is not always available via MetadataExt across Unixen;
    // when unset fall back to a derived value from `created()`.
    let birth = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(m);
    (a, m, c, birth)
}

#[cfg(not(unix))]
fn metadata_times_ns(_meta: &fs::Metadata) -> (i64, i64, i64, i64) {
    // Sentinel; the bigint stats path multiplies ms × 1e6 when this
    // fallback is hit so we keep behavior backward-compatible on Windows.
    (0, 0, 0, 0)
}

fn metadata_owner_ids(meta: &fs::Metadata) -> (f64, f64) {
    #[cfg(unix)]
    {
        (meta.uid() as f64, meta.gid() as f64)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (-1.0, -1.0)
    }
}

fn metadata_nlink(meta: &fs::Metadata) -> f64 {
    #[cfg(unix)]
    {
        meta.nlink() as f64
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        1.0
    }
}

fn metadata_node_extra_fields(meta: Option<&fs::Metadata>) -> (u64, u64, u64, u64, u64) {
    #[cfg(unix)]
    {
        if let Some(meta) = meta {
            return (
                meta.dev(),
                meta.rdev(),
                meta.blksize(),
                meta.ino(),
                meta.blocks(),
            );
        }
    }
    let _ = meta;
    (0, 0, 0, 0, 0)
}

/// `fs.statSync(path)` — returns a Stats-like object with `isFile()`,
/// `isDirectory()`, `isSymbolicLink()` predicate methods and a `size`
/// numeric field. On error, returns an object where all predicates are
/// false and size is 0 (Node throws on ENOENT, but Perry's LLVM backend
/// doesn't have a catch-unwind path for runtime panics — graceful
/// degradation is safer here).
#[no_mangle]
pub extern "C" fn js_fs_stat_sync(path_value: f64) -> f64 {
    js_fs_stat_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_stat_sync_options(path_value: f64, options_value: f64) -> f64 {
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                return build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            }
        };
        match fs::metadata(path_str) {
            Ok(meta) => {
                let is_file = meta.is_file();
                let is_dir = meta.is_dir();
                let is_symlink = meta.file_type().is_symlink();
                let size = meta.len();
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
                build_stats_object(
                    is_file,
                    is_dir,
                    is_symlink,
                    size,
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
            Err(_) => build_stats_object(
                false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
            ),
        }
    }
}

/// `fs.lstatSync(path)` — same Stats shape as `statSync`, but uses
/// symlink metadata so `isSymbolicLink()` works for links.
#[no_mangle]
pub extern "C" fn js_fs_lstat_sync(path_value: f64) -> f64 {
    js_fs_lstat_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_lstat_sync_options(path_value: f64, options_value: f64) -> f64 {
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => {
                return build_stats_object(
                    false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
                )
            }
        };
        match fs::symlink_metadata(path_str) {
            Ok(meta) => {
                let ft = meta.file_type();
                let size = meta.len();
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
                build_stats_object(
                    ft.is_file(),
                    ft.is_dir(),
                    ft.is_symlink(),
                    size,
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
            Err(_) => build_stats_object(
                false, false, false, 0, 0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, bigint, None,
            ),
        }
    }
}

/// `fs.renameSync(from, to)` — returns 1 on success, 0 on failure.
#[no_mangle]
pub extern "C" fn js_fs_rename_sync(from_value: f64, to_value: f64) -> i32 {
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

#[derive(Clone, Copy)]
struct FsCopyOptions {
    force: bool,
    error_on_exist: bool,
    preserve_timestamps: bool,
    dereference: bool,
    verbatim_symlinks: bool,
    recursive: bool,
    filter: f64,
}

unsafe fn fs_copy_options_from_value(options_value: f64) -> FsCopyOptions {
    let force = if options_has_field(options_value, b"force") {
        options_bool_field(options_value, b"force")
    } else {
        true
    };
    FsCopyOptions {
        force,
        error_on_exist: options_bool_field(options_value, b"errorOnExist"),
        preserve_timestamps: options_bool_field(options_value, b"preserveTimestamps"),
        dereference: options_bool_field(options_value, b"dereference"),
        verbatim_symlinks: options_bool_field(options_value, b"verbatimSymlinks"),
        recursive: options_bool_field(options_value, b"recursive"),
        filter: options_field_value(options_value, b"filter")
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED)),
    }
}

fn copy_filter_allows(src: &Path, dst: &Path, opts: FsCopyOptions) -> bool {
    let filter = extract_closure_ptr(opts.filter);
    if filter.is_null() {
        return true;
    }
    let src_string = src.to_string_lossy();
    let dst_string = dst.to_string_lossy();
    let src_value = unsafe {
        let s = js_string_from_bytes(src_string.as_bytes().as_ptr(), src_string.len() as u32);
        crate::value::js_nanbox_string(s as i64)
    };
    let dst_value = unsafe {
        let s = js_string_from_bytes(dst_string.as_bytes().as_ptr(), dst_string.len() as u32);
        crate::value::js_nanbox_string(s as i64)
    };
    let result = crate::closure::js_closure_call2(filter, src_value, dst_value);
    crate::value::js_is_truthy(result) != 0
}

fn copy_preserve_timestamps(src: &Path, dst: &Path, follow: bool) {
    let meta = if follow {
        fs::metadata(src)
    } else {
        fs::symlink_metadata(src)
    };
    let Ok(meta) = meta else {
        return;
    };
    let (atime, mtime, _, _) = metadata_times_ms(&meta);
    let dst_string = dst.to_string_lossy();
    let _ = set_path_times(&dst_string, atime / 1000.0, mtime / 1000.0, !follow);
}

fn lexical_normalize_path(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn copy_file_with_options(src: &Path, dst: &Path, opts: FsCopyOptions) -> std::io::Result<()> {
    if !copy_filter_allows(src, dst, opts) {
        return Ok(());
    }
    if dst.exists() {
        if !opts.force {
            if opts.error_on_exist {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "destination exists",
                ));
            }
            return Ok(());
        }
    } else if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(src, dst)?;
    if opts.preserve_timestamps {
        copy_preserve_timestamps(src, dst, opts.dereference);
    }
    Ok(())
}

fn copy_symlink_with_options(src: &Path, dst: &Path, opts: FsCopyOptions) -> std::io::Result<()> {
    if !copy_filter_allows(src, dst, opts) {
        return Ok(());
    }
    if opts.dereference {
        let target_meta = fs::metadata(src)?;
        if target_meta.is_dir() {
            copy_dir_recursive(src, dst, opts)
        } else {
            copy_file_with_options(src, dst, opts)
        }
    } else {
        if dst.exists() {
            if !opts.force {
                if opts.error_on_exist {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "destination exists",
                    ));
                }
                return Ok(());
            }
            let _ = fs::remove_file(dst);
        } else if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut target = fs::read_link(src)?;
        if !opts.verbatim_symlinks && target.is_relative() {
            if let Some(parent) = src.parent() {
                target = lexical_normalize_path(parent.join(target));
            }
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, dst)?;
        if opts.preserve_timestamps {
            copy_preserve_timestamps(src, dst, false);
        }
        Ok(())
    }
}

fn copy_dir_recursive(from: &Path, to: &Path, opts: FsCopyOptions) -> std::io::Result<()> {
    copy_dir_recursive_depth(from, to, opts, 0)
}

// Guard against symlink cycles under `dereference: true`. Node's cp gives up
// with ELOOP via the OS; we bound depth defensively so a malicious tree can't
// stack-overflow Perry's process.
const COPY_DIR_MAX_DEPTH: u32 = 256;

fn copy_dir_recursive_depth(
    from: &Path,
    to: &Path,
    opts: FsCopyOptions,
    depth: u32,
) -> std::io::Result<()> {
    if depth >= COPY_DIR_MAX_DEPTH {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "cpSync: directory nesting exceeds limit (possible symlink cycle)",
        ));
    }
    if !copy_filter_allows(from, to, opts) {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_recursive_depth(&src, &dst, opts, depth + 1)?;
        } else if file_type.is_file() {
            copy_file_with_options(&src, &dst, opts)?;
        } else if file_type.is_symlink() {
            copy_symlink_with_options(&src, &dst, opts)?;
        }
    }
    if opts.preserve_timestamps {
        copy_preserve_timestamps(from, to, opts.dereference);
    }
    Ok(())
}

/// `fs.cpSync(from, to, { recursive: true })` — deterministic subset:
/// copies files, regular directory trees, and the most common
/// force/errorOnExist/preserveTimestamps/dereference options.
#[no_mangle]
pub extern "C" fn js_fs_cp_sync(from_value: f64, to_value: f64) -> i32 {
    js_fs_cp_sync_options(
        from_value,
        to_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

#[no_mangle]
pub extern "C" fn js_fs_cp_sync_options(from_value: f64, to_value: f64, options_value: f64) -> i32 {
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        let src = Path::new(&from);
        let dst = Path::new(&to);
        let opts = fs_copy_options_from_value(options_value);
        // Node throws ERR_FS_CP_EINVAL if `src == dest`. We don't propagate
        // typed errors yet, so return 0 (failure) to keep `cpSync` from
        // silently no-op'ing into itself.
        if let (Ok(canon_src), Ok(canon_dst)) = (fs::canonicalize(src), fs::canonicalize(dst)) {
            if canon_src == canon_dst {
                return 0;
            }
        }
        let meta = if opts.dereference {
            fs::metadata(src)
        } else {
            fs::symlink_metadata(src)
        };
        // Node requires `{ recursive: true }` to copy directories; otherwise
        // it throws ERR_FS_EISDIR. Surface the same gate via `js_throw` so
        // `try/catch` around `cpSync` actually fires.
        if matches!(meta, Ok(ref m) if m.is_dir()) && !opts.recursive {
            let bytes = b"ERR_FS_EISDIR: cpSync: src is a directory (use { recursive: true })";
            let msg = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(msg);
            let err_val = crate::value::js_nanbox_pointer(err as i64);
            crate::exception::js_throw(err_val);
        }
        let result = match meta {
            Ok(meta) if meta.is_dir() => copy_dir_recursive(src, dst, opts),
            Ok(meta) if meta.file_type().is_symlink() => copy_symlink_with_options(src, dst, opts),
            Ok(_) => copy_file_with_options(src, dst, opts),
            Err(err) => Err(err),
        };
        if result.is_ok() {
            1
        } else {
            0
        }
    }
}

/// `fs.accessSync(path)` — returns 1 if accessible, 0 otherwise.
/// Unlike Node's `accessSync` which throws on failure, this returns a
/// status code; the LLVM codegen wraps the result so `try/catch` works.
#[no_mangle]
pub extern "C" fn js_fs_access_sync(path_value: f64) -> i32 {
    js_fs_access_sync_mode(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
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
    let fd = fd_value as i32;
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
    let fd = fd_value as i32;
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
    let fd = fd_value as i32;
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
    let bytes = readlink_bytes(path_value);
    let enc = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    encoded_string_ptr(&bytes, &enc) as i64
}

#[no_mangle]
pub extern "C" fn js_fs_readlink_dispatch(path_value: f64, options_value: f64) -> f64 {
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

fn array_ptr_from_value(value: f64) -> *const crate::array::ArrayHeader {
    let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
    if raw < 0x1000 {
        std::ptr::null()
    } else {
        raw as *const crate::array::ArrayHeader
    }
}

/// `fs.openSync(path, flags)` — small fd registry for deterministic tests.
#[no_mangle]
pub extern "C" fn js_fs_open_sync(path_value: f64, flags_value: f64) -> f64 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return -1.0,
        };
        let mut opts = fs::OpenOptions::new();
        let append_mode;
        if flags_value.is_finite() {
            let flags = flags_value as i32;
            append_mode = flags & 0x8 != 0;
            let access = flags & 0x3;
            match access {
                1 => {
                    opts.write(true);
                }
                2 => {
                    opts.read(true).write(true);
                }
                _ => {
                    opts.read(true);
                }
            }
            if flags & 0x200 != 0 && flags & 0x800 != 0 {
                opts.create_new(true);
            } else if flags & 0x200 != 0 {
                opts.create(true);
            }
            if flags & 0x400 != 0 {
                opts.truncate(true);
            }
            if append_mode {
                opts.append(true).write(true);
            }
        } else {
            let flags = flag_string(flags_value);
            append_mode = matches!(flags.as_str(), "a" | "a+" | "ax" | "ax+");
            match flags.as_str() {
                "r" | "rs" => {
                    opts.read(true);
                }
                "r+" | "rs+" => {
                    opts.read(true).write(true);
                }
                "w" => {
                    opts.write(true).create(true).truncate(true);
                }
                "w+" => {
                    opts.read(true).write(true).create(true).truncate(true);
                }
                "a" => {
                    opts.write(true).create(true).append(true);
                }
                "a+" => {
                    opts.read(true).write(true).create(true).append(true);
                }
                "wx" => {
                    opts.write(true).create_new(true);
                }
                "wx+" => {
                    opts.read(true).write(true).create_new(true);
                }
                "ax" => {
                    opts.write(true).create_new(true).append(true);
                }
                "ax+" => {
                    opts.read(true).write(true).create_new(true).append(true);
                }
                _ => {
                    opts.read(true);
                }
            }
        }
        match opts.open(&path_str) {
            Ok(file) => {
                let fd = NEXT_FD.with(|n| {
                    let mut n = n.borrow_mut();
                    let fd = *n;
                    *n += 1;
                    fd
                });
                FD_REGISTRY.with(|r| {
                    r.borrow_mut().insert(fd, file);
                });
                FD_PATHS.with(|r| {
                    r.borrow_mut().insert(fd, path_str.to_string());
                });
                FD_APPEND_MODE.with(|r| {
                    r.borrow_mut().insert(fd, append_mode);
                });
                fd as f64
            }
            Err(_) => -1.0,
        }
    }
}

/// `fs.closeSync(fd)` — close a registry fd.
#[no_mangle]
pub extern "C" fn js_fs_close_sync(fd_value: f64) -> i32 {
    let fd = fd_value as i32;
    FD_REGISTRY.with(|r| {
        if r.borrow_mut().remove(&fd).is_some() {
            FD_PATHS.with(|paths| {
                paths.borrow_mut().remove(&fd);
            });
            FD_APPEND_MODE.with(|flags| {
                flags.borrow_mut().remove(&fd);
            });
            1
        } else {
            0
        }
    })
}

/// `fs.readSync(fd, buffer, offset, length, position)` — Buffer subset.
#[no_mangle]
pub extern "C" fn js_fs_read_sync(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> f64 {
    let fd = fd_value as i32;
    let offset = offset_value.max(0.0) as usize;
    let length = length_value.max(0.0) as usize;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buf = buffer_ptr_from_value(buffer_value);
    if buf.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        unsafe {
            let cap = (*buf).length as usize;
            if offset >= cap {
                if let Some(pos) = restore_pos {
                    let _ = file.seek(SeekFrom::Start(pos));
                }
                return 0.0;
            }
            let n = length.min(cap - offset);
            let data = crate::buffer::buffer_data_mut(buf).add(offset);
            let result = match file.read(std::slice::from_raw_parts_mut(data, n)) {
                Ok(read) => read as f64,
                Err(_) => 0.0,
            };
            if let Some(pos) = restore_pos {
                let _ = file.seek(SeekFrom::Start(pos));
            }
            result
        }
    })
}

#[no_mangle]
pub extern "C" fn js_fs_read_sync_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        let offset = options_number_field(options_value, b"offset").unwrap_or(0.0);
        let length = options_number_field(options_value, b"length")
            .unwrap_or_else(|| buffer_len_from_value(buffer_value) as f64 - offset.max(0.0));
        let position = options_field_value(options_value, b"position")
            .map(|v| f64::from_bits(v.bits()))
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_NULL));
        js_fs_read_sync(fd_value, buffer_value, offset, length, position)
    }
}

/// `fs.writeSync(fd, string)` — string subset.
#[no_mangle]
pub extern "C" fn js_fs_write_sync(fd_value: f64, data_value: f64) -> f64 {
    js_fs_write_string_sync_options(
        fd_value,
        data_value,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    )
}

/// `fs.writeSync(fd, string[, position[, encoding]])`.
#[no_mangle]
pub extern "C" fn js_fs_write_string_sync_options(
    fd_value: f64,
    data_value: f64,
    position_value: f64,
) -> f64 {
    let fd = fd_value as i32;
    let bytes = bytes_from_value(data_value);
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let result = match file.write(&bytes) {
            Ok(n) => n as f64,
            Err(_) => 0.0,
        };
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        result
    })
}

/// `fs.writeSync(fd, buffer, offset, length, position)` — Buffer subset.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_sync(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
) -> f64 {
    let fd = fd_value as i32;
    let offset = offset_value.max(0.0) as usize;
    let length = length_value.max(0.0) as usize;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buf = buffer_ptr_from_value(buffer_value);
    if buf.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        unsafe {
            let cap = (*buf).length as usize;
            if offset >= cap {
                if let Some(pos) = restore_pos {
                    let _ = file.seek(SeekFrom::Start(pos));
                }
                return 0.0;
            }
            let n = length.min(cap - offset);
            let data = crate::buffer::buffer_data(buf).add(offset);
            let result = match file.write(std::slice::from_raw_parts(data, n)) {
                Ok(written) => written as f64,
                Err(_) => 0.0,
            };
            if let Some(pos) = restore_pos {
                let _ = file.seek(SeekFrom::Start(pos));
            }
            result
        }
    })
}

#[no_mangle]
pub extern "C" fn js_fs_write_sync_options_dispatch(
    fd_value: f64,
    data_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        if options_field_value(options_value, b"offset").is_some()
            || options_field_value(options_value, b"length").is_some()
            || options_field_value(options_value, b"position").is_some()
        {
            let offset = options_number_field(options_value, b"offset").unwrap_or(0.0);
            let length = options_number_field(options_value, b"length")
                .unwrap_or_else(|| buffer_len_from_value(data_value) as f64 - offset.max(0.0));
            let position = options_field_value(options_value, b"position")
                .map(|v| f64::from_bits(v.bits()))
                .unwrap_or_else(|| f64::from_bits(crate::value::TAG_NULL));
            js_fs_write_buffer_sync(fd_value, data_value, offset, length, position)
        } else {
            js_fs_write_string_sync_options(fd_value, data_value, options_value)
        }
    }
}

/// `fs.readvSync(fd, buffers[, position])` — deterministic Buffer[] subset.
#[no_mangle]
pub extern "C" fn js_fs_readv_sync(fd_value: f64, buffers_value: f64, position_value: f64) -> f64 {
    let fd = fd_value as i32;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buffers = array_ptr_from_value(buffers_value);
    if buffers.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let mut total = 0usize;
        unsafe {
            let len = crate::array::js_array_length(buffers);
            for i in 0..len {
                let value = crate::array::js_array_get_f64(buffers, i);
                let buf = buffer_ptr_from_value(value);
                if buf.is_null() {
                    continue;
                }
                let cap = (*buf).length as usize;
                if cap == 0 {
                    continue;
                }
                let data = crate::buffer::buffer_data_mut(buf);
                // Node's readv fills each iovec completely (short read only
                // at EOF). Use `read` in a loop so we don't return partially
                // filled buffers when the kernel splits the read.
                let mut filled = 0usize;
                let mut eof = false;
                while filled < cap {
                    let slice = std::slice::from_raw_parts_mut(data.add(filled), cap - filled);
                    match file.read(slice) {
                        Ok(0) => {
                            eof = true;
                            break;
                        }
                        Ok(n) => filled += n,
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            eof = true;
                            break;
                        }
                    }
                }
                total += filled;
                if eof {
                    break;
                }
            }
        }
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        total as f64
    })
}

/// `fs.writevSync(fd, buffers[, position])` — deterministic Buffer[] subset.
#[no_mangle]
pub extern "C" fn js_fs_writev_sync(fd_value: f64, buffers_value: f64, position_value: f64) -> f64 {
    let fd = fd_value as i32;
    let position = if position_value.is_finite() && position_value >= 0.0 {
        Some(position_value as u64)
    } else {
        None
    };
    let buffers = array_ptr_from_value(buffers_value);
    if buffers.is_null() {
        return 0.0;
    }
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return 0.0;
        };
        let restore_pos = position.and_then(|_| file.stream_position().ok());
        if let Some(pos) = position {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        let mut total = 0usize;
        unsafe {
            let len = crate::array::js_array_length(buffers);
            for i in 0..len {
                let value = crate::array::js_array_get_f64(buffers, i);
                let buf = buffer_ptr_from_value(value);
                if buf.is_null() {
                    continue;
                }
                let cap = (*buf).length as usize;
                if cap == 0 {
                    continue;
                }
                let data = crate::buffer::buffer_data(buf);
                // Node guarantees each iovec is fully written before the
                // next; use `write_all` semantics to match.
                let slice = std::slice::from_raw_parts(data, cap);
                if file.write_all(slice).is_err() {
                    break;
                }
                total += cap;
            }
        }
        if let Some(pos) = restore_pos {
            let _ = file.seek(SeekFrom::Start(pos));
        }
        total as f64
    })
}

unsafe fn build_statfs_object(
    bsize: f64,
    blocks: f64,
    bfree: f64,
    bavail: f64,
    bigint: bool,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 4);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    if bigint {
        set("bsize", bigint_u64_value(bsize as u64));
        set("blocks", bigint_u64_value(blocks as u64));
        set("bfree", bigint_u64_value(bfree as u64));
        set("bavail", bigint_u64_value(bavail as u64));
    } else {
        set("bsize", bsize);
        set("blocks", blocks);
        set("bfree", bfree);
        set("bavail", bavail);
    }
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

/// `fs.statfsSync(path)` — stable StatFs subset used by Node/Bun tests.
#[no_mangle]
pub extern "C" fn js_fs_statfs_sync(path_value: f64) -> f64 {
    js_fs_statfs_sync_options(path_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_statfs_sync_options(path_value: f64, options_value: f64) -> f64 {
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return build_statfs_object(0.0, 0.0, 0.0, 0.0, bigint),
        };
        #[cfg(unix)]
        {
            let c_path = match std::ffi::CString::new(path) {
                Ok(s) => s,
                Err(_) => return build_statfs_object(0.0, 0.0, 0.0, 0.0, bigint),
            };
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                return build_statfs_object(
                    stat.f_bsize as f64,
                    stat.f_blocks as f64,
                    stat.f_bfree as f64,
                    stat.f_bavail as f64,
                    bigint,
                );
            }
        }
        #[cfg(not(unix))]
        {
            let _ = path;
        }
        build_statfs_object(0.0, 0.0, 0.0, 0.0, bigint)
    }
}

fn alloc_dir_state(entries: Vec<f64>) -> usize {
    let id = NEXT_DIR_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    DIR_REGISTRY.with(|r| {
        r.borrow_mut().insert(
            id,
            DirState {
                entries,
                index: 0,
                closed: false,
            },
        );
    });
    id
}

fn dir_id_of(closure: *const ClosureHeader) -> usize {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as usize
}

fn dir_read_next(id: usize) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    DIR_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return f64::from_bits(TAG_NULL);
        };
        if state.closed || state.index >= state.entries.len() {
            return f64::from_bits(TAG_NULL);
        }
        let value = state.entries[state.index];
        state.index += 1;
        value
    })
}

fn make_dir_method(id: usize, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, id as i64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

extern "C" fn dir_read_sync_impl(closure: *const ClosureHeader) -> f64 {
    dir_read_next(dir_id_of(closure))
}

extern "C" fn dir_close_sync_impl(closure: *const ClosureHeader) -> f64 {
    let id = dir_id_of(closure);
    DIR_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.closed = true;
        }
    });
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn dir_read_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let entry = dir_read_next(dir_id_of(closure));
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entry);
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    promise_value_fs(entry)
}

extern "C" fn dir_close_impl(closure: *const ClosureHeader, callback: f64) -> f64 {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let _ = dir_close_sync_impl(closure);
    let cb = extract_closure_ptr(callback);
    if !cb.is_null() {
        crate::closure::js_closure_call1(cb, f64::from_bits(TAG_NULL));
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    promise_undefined_fs()
}

unsafe fn build_dir_object(id: usize, path: &str) -> f64 {
    crate::closure::js_register_closure_arity(dir_read_impl as *const u8, 1);
    crate::closure::js_register_closure_arity(dir_close_impl as *const u8, 1);
    let obj = crate::object::js_object_alloc(0, 6);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    let path_ptr = js_string_from_bytes(path.as_ptr(), path.len() as u32);
    set("path", crate::value::js_nanbox_string(path_ptr as i64));
    set(
        "readSync",
        make_dir_method(id, dir_read_sync_impl as *const u8),
    );
    set(
        "closeSync",
        make_dir_method(id, dir_close_sync_impl as *const u8),
    );
    set("read", make_dir_method(id, dir_read_impl as *const u8));
    set("close", make_dir_method(id, dir_close_impl as *const u8));
    set(
        "Symbol.asyncIterator",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

/// `fs.opendirSync(path)` — deterministic Dir subset with readSync/closeSync.
#[no_mangle]
pub extern "C" fn js_fs_opendir_sync(path_value: f64) -> f64 {
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(s) => s,
            None => return build_dir_object(alloc_dir_state(Vec::new()), ""),
        };
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(&path) {
            let mut items: Vec<(String, std::fs::FileType)> = Vec::new();
            for entry in read_dir.flatten() {
                if let (Some(name), Ok(ft)) = (entry.file_name().to_str(), entry.file_type()) {
                    items.push((name.to_string(), ft));
                }
            }
            items.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, ft) in items {
                entries.push(build_dirent_object(
                    &name,
                    &path,
                    ft.is_file(),
                    ft.is_dir(),
                    ft.is_symlink(),
                ));
            }
        }
        build_dir_object(alloc_dir_state(entries), &path)
    }
}

fn glob_regex_from_pattern(pattern: &str) -> Option<regex::Regex> {
    let normalized = pattern.replace('\\', "/");
    let mut out = String::from("^");
    let mut chars = normalized.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' => {
                if chars.peek() == Some(&'*') {
                    let _ = chars.next();
                    if chars.peek() == Some(&'/') {
                        let _ = chars.next();
                        out.push_str("(?:.*/)?");
                    } else {
                        out.push_str(".*");
                    }
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            '/' => out.push('/'),
            other => out.push(other),
        }
    }
    out.push('$');
    regex::Regex::new(&out).ok()
}

fn glob_search_root(pattern: &str) -> String {
    let normalized = pattern.replace('\\', "/");
    let first_meta = normalized
        .find(|c| matches!(c, '*' | '?' | '[' | '{'))
        .unwrap_or(normalized.len());
    let prefix = &normalized[..first_meta];
    match prefix.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => prefix[..idx].to_string(),
        None => ".".to_string(),
    }
}

fn walk_paths_for_glob(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        out.push(path.to_string_lossy().replace('\\', "/"));
        if path.is_dir() {
            walk_paths_for_glob(&path, out);
        }
    }
}

/// `fs.globSync(pattern)` — deterministic subset covering `*`, `?`, and `**`.
#[no_mangle]
pub extern "C" fn js_fs_glob_sync(pattern_value: f64) -> f64 {
    js_fs_glob_sync_options(pattern_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

fn glob_cwd_from_options(options_value: f64) -> Option<String> {
    unsafe {
        let cwd = options_field_value(options_value, b"cwd")?;
        decode_path_value(f64::from_bits(cwd.bits())).map(|s| s.to_string())
    }
}

#[no_mangle]
pub extern "C" fn js_fs_glob_sync_options(pattern_value: f64, options_value: f64) -> f64 {
    use crate::array::{js_array_alloc, js_array_push_f64};
    use crate::value::js_nanbox_string;

    unsafe {
        let pattern = match decode_path_value(pattern_value) {
            Some(s) => s,
            None => {
                let arr = js_array_alloc(0);
                return f64::from_bits(i64::cast_unsigned(arr as i64));
            }
        };
        let cwd = glob_cwd_from_options(options_value);
        let pattern_for_match = if let Some(cwd) = &cwd {
            if Path::new(&pattern).is_absolute() {
                pattern.to_string()
            } else {
                format!("{}/{}", cwd.trim_end_matches('/'), pattern)
            }
        } else {
            pattern.to_string()
        }
        .replace('\\', "/");
        let Some(re) = glob_regex_from_pattern(&pattern_for_match) else {
            let arr = js_array_alloc(0);
            return f64::from_bits(i64::cast_unsigned(arr as i64));
        };
        let root = glob_search_root(&pattern_for_match);
        let mut candidates = Vec::new();
        walk_paths_for_glob(Path::new(&root), &mut candidates);
        let mut matches: Vec<String> = candidates.into_iter().filter(|p| re.is_match(p)).collect();
        matches.sort();

        let mut arr = js_array_alloc(matches.len() as u32);
        for path in &matches {
            let output = if let Some(cwd) = &cwd {
                let prefix = format!("{}/", cwd.trim_end_matches('/')).replace('\\', "/");
                path.strip_prefix(&prefix).unwrap_or(path).to_string()
            } else {
                path.clone()
            };
            let bytes = output.as_bytes();
            let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            arr = js_array_push_f64(arr, js_nanbox_string(s as i64));
        }
        f64::from_bits(i64::cast_unsigned(arr as i64))
    }
}

extern "C" fn fs_watcher_noop_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn make_zero_capture_method(func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 0);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

unsafe fn build_fs_watcher_object(include_close: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, if include_close { 8 } else { 7 });
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    let method = make_zero_capture_method(fs_watcher_noop_impl as *const u8);
    if include_close {
        set("close", method);
    }
    set("ref", method);
    set("unref", method);
    set("on", method);
    set("once", method);
    set("addListener", method);
    set("removeListener", method);
    set("off", method);
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

/// `fs.watch(path[, options][, listener])` — lightweight watcher object
/// shape. Event delivery is intentionally not implemented yet.
#[no_mangle]
pub extern "C" fn js_fs_watch(path_value: f64, _arg1: f64, _arg2: f64) -> f64 {
    let _ = path_value;
    unsafe { build_fs_watcher_object(true) }
}

/// `fs.watchFile(path[, options], listener)` — lightweight StatWatcher shape.
#[no_mangle]
pub extern "C" fn js_fs_watch_file(path_value: f64, _arg1: f64, _arg2: f64) -> f64 {
    let _ = path_value;
    unsafe { build_fs_watcher_object(false) }
}

/// `fs.unwatchFile(path[, listener])`.
#[no_mangle]
pub extern "C" fn js_fs_unwatch_file(path_value: f64, _listener: f64) -> f64 {
    let _ = path_value;
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn promise_value_fs(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

fn promise_undefined_fs() -> f64 {
    promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED))
}

unsafe fn build_file_io_result(count_name: &str, count: f64, value_name: &str, value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set(count_name, count);
    set(value_name, value);
    f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
}

fn make_filehandle_method(fd: i32, func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, fd as i64);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

fn filehandle_fd(closure: *const ClosureHeader) -> i32 {
    crate::closure::js_closure_get_capture_ptr(closure, 0) as i32
}

extern "C" fn filehandle_close_impl(closure: *const ClosureHeader) -> f64 {
    let _ = js_fs_close_sync(filehandle_fd(closure) as f64);
    promise_undefined_fs()
}

extern "C" fn filehandle_sync_impl(closure: *const ClosureHeader) -> f64 {
    let _ = js_fs_fsync_sync(filehandle_fd(closure) as f64);
    promise_undefined_fs()
}

extern "C" fn filehandle_datasync_impl(closure: *const ClosureHeader) -> f64 {
    let _ = js_fs_fdatasync_sync(filehandle_fd(closure) as f64);
    promise_undefined_fs()
}

extern "C" fn filehandle_stat_impl(closure: *const ClosureHeader, options: f64) -> f64 {
    promise_value_fs(js_fs_fstat_sync_options(
        filehandle_fd(closure) as f64,
        options,
    ))
}

extern "C" fn filehandle_truncate_impl(closure: *const ClosureHeader, len: f64) -> f64 {
    let _ = js_fs_ftruncate_sync(filehandle_fd(closure) as f64, len);
    promise_undefined_fs()
}

extern "C" fn filehandle_utimes_impl(closure: *const ClosureHeader, atime: f64, mtime: f64) -> f64 {
    let _ = js_fs_futimes_sync(filehandle_fd(closure) as f64, atime, mtime);
    promise_undefined_fs()
}

extern "C" fn filehandle_chmod_impl(closure: *const ClosureHeader, mode: f64) -> f64 {
    let _ = js_fs_fchmod_sync(filehandle_fd(closure) as f64, mode);
    promise_undefined_fs()
}

extern "C" fn filehandle_chown_impl(closure: *const ClosureHeader, uid: f64, gid: f64) -> f64 {
    let _ = js_fs_fchown_sync(filehandle_fd(closure) as f64, uid, gid);
    promise_undefined_fs()
}

extern "C" fn filehandle_read_file_impl(closure: *const ClosureHeader, encoding: f64) -> f64 {
    let fd = filehandle_fd(closure);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(file) = reg.get_mut(&fd) else {
            return promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED));
        };
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        if read_file_encoding(encoding).is_none() {
            let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
            if !buf.is_null() {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        crate::buffer::buffer_data_mut(buf),
                        bytes.len(),
                    );
                    (*buf).length = bytes.len() as u32;
                }
            }
            promise_value_fs(f64::from_bits(
                crate::value::JSValue::pointer(buf as *const u8).bits(),
            ))
        } else {
            let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            promise_value_fs(f64::from_bits(crate::value::JSValue::string_ptr(s).bits()))
        }
    })
}

extern "C" fn filehandle_write_file_impl(closure: *const ClosureHeader, data: f64) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes = bytes_from_value(data);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(file) = reg.get_mut(&fd) {
            let append =
                FD_APPEND_MODE.with(|flags| flags.borrow().get(&fd).copied().unwrap_or(false));
            if append {
                let _ = file.seek(SeekFrom::End(0));
            }
            // Note: Node does NOT rewind/truncate on FileHandle#writeFile —
            // empirically the file pointer advances naturally so successive
            // writeFile calls concatenate (see parity test
            // `fs-promises/basic/write-append-flush-options`). When the
            // caller wants replace-semantics they should reopen the handle.
            let _ = file.write_all(&bytes);
        }
    });
    promise_undefined_fs()
}

extern "C" fn filehandle_append_file_impl(closure: *const ClosureHeader, data: f64) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes = bytes_from_value(data);
    FD_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(file) = reg.get_mut(&fd) {
            let _ = file.seek(SeekFrom::End(0));
            let _ = file.write_all(&bytes);
        }
    });
    promise_undefined_fs()
}

extern "C" fn filehandle_read_impl(
    closure: *const ClosureHeader,
    buffer: f64,
    offset: f64,
    length: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let (actual_buffer, actual_offset, actual_length, actual_position) =
        if crate::buffer::js_buffer_is_buffer(buffer.to_bits() as i64) == 1 {
            let buffer_len = buffer_len_from_value(buffer) as f64;
            let actual_offset = if offset.is_finite() { offset } else { 0.0 };
            let actual_length = if length.is_finite() {
                length
            } else {
                (buffer_len - actual_offset).max(0.0)
            };
            (buffer, actual_offset, actual_length, position)
        } else {
            unsafe {
                let actual_buffer = options_field_value(buffer, b"buffer")
                    .map(|v| f64::from_bits(v.bits()))
                    .unwrap_or_else(|| {
                        let buf = crate::buffer::js_buffer_alloc(16 * 1024, 0);
                        f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
                    });
                let buffer_len = buffer_len_from_value(actual_buffer) as f64;
                let actual_offset = options_number_field(buffer, b"offset").unwrap_or(0.0);
                let actual_length = options_number_field(buffer, b"length")
                    .unwrap_or_else(|| (buffer_len - actual_offset).max(0.0));
                let actual_position = options_number_field(buffer, b"position")
                    .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
                (actual_buffer, actual_offset, actual_length, actual_position)
            }
        };
    let bytes_read = js_fs_read_sync(
        fd as f64,
        actual_buffer,
        actual_offset,
        actual_length,
        actual_position,
    );
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesRead",
            bytes_read,
            "buffer",
            actual_buffer,
        ))
    }
}

extern "C" fn filehandle_write_impl(
    closure: *const ClosureHeader,
    data: f64,
    offset: f64,
    length: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_written = if crate::buffer::js_buffer_is_buffer(data.to_bits() as i64) == 1 {
        let buffer_len = buffer_len_from_value(data) as f64;
        let actual_offset = if offset.is_finite() { offset } else { 0.0 };
        let actual_length = if length.is_finite() {
            length
        } else {
            (buffer_len - actual_offset).max(0.0)
        };
        js_fs_write_buffer_sync(fd as f64, data, actual_offset, actual_length, position)
    } else {
        js_fs_write_string_sync_options(fd as f64, data, offset)
    };
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesWritten",
            bytes_written,
            "buffer",
            data,
        ))
    }
}

extern "C" fn filehandle_readv_impl(
    closure: *const ClosureHeader,
    buffers: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_read = js_fs_readv_sync(fd as f64, buffers, position);
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesRead",
            bytes_read,
            "buffers",
            buffers,
        ))
    }
}

extern "C" fn filehandle_writev_impl(
    closure: *const ClosureHeader,
    buffers: f64,
    position: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    let bytes_written = js_fs_writev_sync(fd as f64, buffers, position);
    unsafe {
        promise_value_fs(build_file_io_result(
            "bytesWritten",
            bytes_written,
            "buffers",
            buffers,
        ))
    }
}

fn path_for_fd(fd: i32) -> Option<String> {
    FD_PATHS.with(|paths| paths.borrow().get(&fd).cloned())
}

extern "C" fn filehandle_create_read_stream_impl(
    closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    if let Some(path) = path_for_fd(fd) {
        let s = js_string_from_bytes(path.as_ptr(), path.len() as u32);
        js_fs_create_read_stream(crate::value::js_nanbox_string(s as i64), options)
    } else {
        let s = js_string_from_bytes(b"".as_ptr(), 0);
        js_fs_create_read_stream(crate::value::js_nanbox_string(s as i64), options)
    }
}

extern "C" fn filehandle_create_write_stream_impl(
    closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let fd = filehandle_fd(closure);
    if let Some(path) = path_for_fd(fd) {
        let s = js_string_from_bytes(path.as_ptr(), path.len() as u32);
        js_fs_create_write_stream(crate::value::js_nanbox_string(s as i64), options)
    } else {
        let s = js_string_from_bytes(b"".as_ptr(), 0);
        js_fs_create_write_stream(crate::value::js_nanbox_string(s as i64), options)
    }
}

/// Build a minimal `fs.promises.FileHandle` object for deterministic parity.
#[no_mangle]
pub extern "C" fn js_fs_filehandle_open(path_value: f64, flags_value: f64) -> f64 {
    let fd = js_fs_open_sync(path_value, flags_value) as i32;
    unsafe {
        crate::closure::js_register_closure_arity(filehandle_stat_impl as *const u8, 1);
        crate::closure::js_register_closure_arity(filehandle_read_impl as *const u8, 5);
        crate::closure::js_register_closure_arity(filehandle_write_impl as *const u8, 5);
        let obj = crate::object::js_object_alloc(0, 18);
        let set = |name: &str, v: f64| {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(obj, key, v);
        };
        set("fd", fd as f64);
        set(
            "close",
            make_filehandle_method(fd, filehandle_close_impl as *const u8),
        );
        set(
            "sync",
            make_filehandle_method(fd, filehandle_sync_impl as *const u8),
        );
        set(
            "datasync",
            make_filehandle_method(fd, filehandle_datasync_impl as *const u8),
        );
        set(
            "stat",
            make_filehandle_method(fd, filehandle_stat_impl as *const u8),
        );
        set(
            "truncate",
            make_filehandle_method(fd, filehandle_truncate_impl as *const u8),
        );
        set(
            "utimes",
            make_filehandle_method(fd, filehandle_utimes_impl as *const u8),
        );
        set(
            "chmod",
            make_filehandle_method(fd, filehandle_chmod_impl as *const u8),
        );
        set(
            "chown",
            make_filehandle_method(fd, filehandle_chown_impl as *const u8),
        );
        set(
            "readFile",
            make_filehandle_method(fd, filehandle_read_file_impl as *const u8),
        );
        set(
            "writeFile",
            make_filehandle_method(fd, filehandle_write_file_impl as *const u8),
        );
        set(
            "appendFile",
            make_filehandle_method(fd, filehandle_append_file_impl as *const u8),
        );
        set(
            "read",
            make_filehandle_method(fd, filehandle_read_impl as *const u8),
        );
        set(
            "write",
            make_filehandle_method(fd, filehandle_write_impl as *const u8),
        );
        set(
            "readv",
            make_filehandle_method(fd, filehandle_readv_impl as *const u8),
        );
        set(
            "writev",
            make_filehandle_method(fd, filehandle_writev_impl as *const u8),
        );
        set(
            "createReadStream",
            make_filehandle_method(fd, filehandle_create_read_stream_impl as *const u8),
        );
        set(
            "createWriteStream",
            make_filehandle_method(fd, filehandle_create_write_stream_impl as *const u8),
        );
        FILEHANDLE_OBJECT_FDS.with(|fds| {
            fds.borrow_mut().insert(obj as usize, fd);
        });
        f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits())
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
// createWriteStream / createReadStream — real implementation.
//
// Returns an ObjectHeader whose fields are NaN-boxed closure
// pointers keyed by method name (`write`, `end`, `on`, `once`,
// `close`). The closures capture a stream id in slot 0, which
// indexes into STREAM_REGISTRY for the in-memory buffer/state.
//
// The generic `js_native_call_method` dispatcher scans object
// keys and dispatches the matching closure via `js_native_call_value`,
// so `ws.write(x)` / `ws.on('finish', cb)` flow through unchanged.
//
// Stream semantics match Node's common `end(); on('finish', cb)`
// pattern: write() buffers, end() flushes to disk and marks the
// state finished, and on('finish', cb) fires cb inline if the
// stream is already finished (or stashes it otherwise).
// ============================================================
use std::cell::RefCell;
use std::collections::HashMap as StdHashMap;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_alloc_with_shape, js_object_set_field, ObjectHeader};
use crate::value::JSValue;

const TAG_UNDEFINED_STREAM: u64 = 0x7FFC_0000_0000_0001;
const STREAM_SHAPE_ID: u32 = 0x7FFF_FE40;

/// State for a single file stream (read OR write).
#[derive(Default)]
struct StreamState {
    /// Filesystem path the stream is bound to.
    path: String,
    /// In-memory buffer: for write streams this accumulates chunks
    /// until `end()` flushes them; for read streams it holds the
    /// pre-read file contents.
    buffer: Vec<u8>,
    /// True once `end()` has been called (write streams) or the
    /// initial read has happened (read streams).
    finished: bool,
    /// If an IO error occurred, this holds the error message.
    error_msg: Option<String>,
    /// If `on('finish', cb)` was registered BEFORE `end()` was
    /// called, the callback is stashed here and fired from end().
    pending_finish: Option<f64>,
    /// Write stream open flag (`w`, `a`, `wx`, ...). Read streams leave this empty.
    write_flag: String,
    /// Whether read streams should emit strings instead of Buffers.
    read_as_string: bool,
}

thread_local! {
    static STREAM_REGISTRY: RefCell<StdHashMap<usize, StreamState>> = RefCell::new(StdHashMap::new());
    static FS_STREAM_NEXT_ID: RefCell<usize> = const { RefCell::new(1) };
}

/// Allocate a new stream id and store the initial state.
fn alloc_stream(state: StreamState) -> usize {
    let id = FS_STREAM_NEXT_ID.with(|c| {
        let mut c = c.borrow_mut();
        let id = *c;
        *c += 1;
        id
    });
    STREAM_REGISTRY.with(|r| {
        r.borrow_mut().insert(id, state);
    });
    id
}

/// Extract a UTF-8 path from a NaN-boxed string value. Returns
/// empty string if the value isn't a string.
fn path_from_value(v: f64) -> String {
    unsafe { decode_path_value(v).unwrap_or_default() }
}

/// Extract raw UTF-8 bytes from a NaN-boxed string value.
fn bytes_from_value(v: f64) -> Vec<u8> {
    unsafe {
        if crate::buffer::js_buffer_is_buffer(v.to_bits() as i64) == 1 {
            let buf = buffer_ptr_from_value(v);
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = crate::buffer::buffer_data(buf);
                return std::slice::from_raw_parts(data, len).to_vec();
            }
        }
        let bits = v.to_bits();
        let addr = if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        };
        if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
            let elem_size = crate::typedarray::elem_size_for_kind(kind);
            let ta = addr as *const crate::typedarray::TypedArrayHeader;
            if !ta.is_null() {
                let len = (*ta).length as usize * elem_size;
                let data = (ta as *const u8)
                    .add(std::mem::size_of::<crate::typedarray::TypedArrayHeader>());
                return std::slice::from_raw_parts(data, len).to_vec();
            }
        }
        let ptr = extract_string_ptr(v);
        if ptr.is_null() {
            return Vec::new();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

/// Allocate a fresh ClosureHeader whose func_ptr is `func` and
/// whose slot 0 holds the given stream id.
fn make_stream_closure(func: extern "C" fn(), stream_id: usize) -> *mut ClosureHeader {
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, stream_id as i64);
    closure
}

/// Build the stream object: an ObjectHeader keyed by method names
/// whose values are NaN-boxed closure pointers. The caller provides
/// the per-method extern helper functions; each closure captures the
/// stream id in slot 0.
#[allow(clippy::type_complexity)]
fn build_stream_object(
    stream_id: usize,
    method_funcs: &[(&str, extern "C" fn())],
) -> *mut ObjectHeader {
    // Build a packed-keys byte sequence: "write\0end\0on\0once\0close\0"
    let mut packed: Vec<u8> = Vec::new();
    for (name, _) in method_funcs {
        packed.extend_from_slice(name.as_bytes());
        packed.push(0);
    }
    // Use a unique shape id per method-set so the SHAPE_CACHE doesn't
    // collide with other allocations. Since read/write use different
    // method sets, we use +0 for write, +1 for read (set by caller).
    let field_count = method_funcs.len() as u32;
    // NOTE: shape id uniqueness is on the caller side — pass the right
    // constant. We use STREAM_SHAPE_ID as base below.
    let obj = js_object_alloc_with_shape(
        STREAM_SHAPE_ID + method_funcs.len() as u32,
        field_count,
        packed.as_ptr(),
        packed.len() as u32,
    );
    for (i, (_name, func)) in method_funcs.iter().enumerate() {
        let closure = make_stream_closure(*func, stream_id);
        // Store as a NaN-boxed pointer (POINTER_TAG) so the dispatcher's
        // `field_val.is_pointer()` check succeeds.
        let val = JSValue::pointer(closure as *const u8);
        js_object_set_field(obj, i as u32, val);
    }
    obj
}

// ------------------------------------------------------------
// Write stream helpers.
// Each helper is an `extern "C" fn(*const ClosureHeader, ...)`
// matching the closure-call ABI. Slot 0 of the closure holds the
// stream id.
// ------------------------------------------------------------

/// Extract the stream id from the closure's capture slot 0.
#[inline]
fn stream_id_of(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

/// `ws.write(chunk)` — append chunk bytes to the in-memory buffer.
extern "C" fn write_stream_write_impl(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let id = stream_id_of(closure);
    let chunk_bytes = bytes_from_value(chunk);
    STREAM_REGISTRY.with(|r| {
        if let Some(state) = r.borrow_mut().get_mut(&id) {
            state.buffer.extend_from_slice(&chunk_bytes);
        }
    });
    // Node returns `true` if the buffer is below the highWaterMark.
    // For our sync impl, always return true.
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    f64::from_bits(TAG_TRUE)
}

/// `ws.end()` — flush the buffer to disk, mark finished, and fire
/// any pending finish listener.
extern "C" fn write_stream_end0_impl(closure: *const ClosureHeader) -> f64 {
    write_stream_end_internal(closure, None)
}

/// `ws.end(finalChunk)` — write finalChunk, then flush.
extern "C" fn write_stream_end1_impl(closure: *const ClosureHeader, chunk: f64) -> f64 {
    write_stream_end_internal(closure, Some(chunk))
}

fn write_stream_end_internal(closure: *const ClosureHeader, final_chunk: Option<f64>) -> f64 {
    use crate::closure::js_closure_call0;
    let id = stream_id_of(closure);

    // Append optional final chunk.
    if let Some(chunk) = final_chunk {
        let bytes = bytes_from_value(chunk);
        STREAM_REGISTRY.with(|r| {
            if let Some(state) = r.borrow_mut().get_mut(&id) {
                state.buffer.extend_from_slice(&bytes);
            }
        });
    }

    // Flush to disk. Take the buffer out so we don't hold the
    // registry borrow across `fs::write`.
    let (path, buffer, flag) = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(state) = reg.get_mut(&id) {
            let p = state.path.clone();
            let b = std::mem::take(&mut state.buffer);
            let f = if state.write_flag.is_empty() {
                "w".to_string()
            } else {
                state.write_flag.clone()
            };
            (p, b, f)
        } else {
            (String::new(), Vec::new(), "w".to_string())
        }
    });

    let write_result = if path.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no path",
        ))
    } else {
        open_file_for_write_flag(&path, &flag).and_then(|mut file| file.write_all(&buffer))
    };

    // Mark finished / record error.
    let pending_finish = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let state = reg.get_mut(&id);
        if let Some(state) = state {
            state.finished = true;
            if let Err(e) = &write_result {
                state.error_msg = Some(format!("{}", e));
            }
            state.pending_finish.take()
        } else {
            None
        }
    });

    // Fire any pending finish listener.
    if let Some(cb) = pending_finish {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }

    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `ws.on(event, cb)` — register a listener. For 'finish' this
/// fires synchronously if the stream is already finished; for
/// 'error' it checks for a recorded error. Unknown events noop.
extern "C" fn write_stream_on_impl(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    use crate::closure::{js_closure_call0, js_closure_call1};
    let id = stream_id_of(closure);
    let event_bytes = bytes_from_value(event);

    // Snapshot state under the borrow, then act without holding it.
    let (is_finished, err_msg, cb_is_finish) = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        let Some(state) = reg.get_mut(&id) else {
            return (false, None, false);
        };
        match event_bytes.as_slice() {
            b"finish" | b"close" => {
                if state.finished && state.error_msg.is_none() {
                    (true, None, true)
                } else if !state.finished {
                    // Stash for later — will fire from end().
                    state.pending_finish = Some(cb);
                    (false, None, false)
                } else {
                    (false, None, false)
                }
            }
            b"error" => {
                if let Some(msg) = &state.error_msg {
                    (false, Some(msg.clone()), false)
                } else {
                    (false, None, false)
                }
            }
            _ => (false, None, false),
        }
    });

    if cb_is_finish && is_finished {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            js_closure_call0(cb_ptr);
        }
    }

    if let Some(msg) = err_msg {
        let cb_ptr = extract_closure_ptr(cb);
        if !cb_ptr.is_null() {
            let msg_bytes = msg.as_bytes();
            let err_str = js_string_from_bytes(msg_bytes.as_ptr(), msg_bytes.len() as u32);
            let err_obj = crate::error::js_error_new_with_message(err_str);
            let err_val = crate::value::js_nanbox_pointer(err_obj as i64);
            js_closure_call1(cb_ptr, err_val);
        }
    }

    // `.on()` in Node returns the stream itself for chaining, but
    // we don't track the receiver inside the closure — return
    // undefined, which matches most practical uses since the test
    // pattern `stream.on('...', cb)` discards the return.
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `ws.close()` — noop; the stream is flushed on end().
extern "C" fn write_stream_close_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

// ------------------------------------------------------------
// Read stream helpers.
// ------------------------------------------------------------

/// `rs.on(event, cb)` — for 'data' fires cb(contents) once,
/// for 'end' fires cb() once (after all data), for 'error'
/// noops unless the file was unreadable.
extern "C" fn read_stream_on_impl(closure: *const ClosureHeader, event: f64, cb: f64) -> f64 {
    use crate::closure::{js_closure_call0, js_closure_call1};
    let id = stream_id_of(closure);
    let event_bytes = bytes_from_value(event);

    // Pull needed data out of the registry without holding the borrow
    // across the callback invocation.
    let (buffer_copy, err_msg, read_as_string) = STREAM_REGISTRY.with(|r| {
        let reg = r.borrow();
        match reg.get(&id) {
            Some(s) => (s.buffer.clone(), s.error_msg.clone(), s.read_as_string),
            None => (Vec::new(), None, false),
        }
    });

    match event_bytes.as_slice() {
        b"data" => {
            if err_msg.is_some() {
                return f64::from_bits(TAG_UNDEFINED_STREAM);
            }
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                let chunk_val = if read_as_string {
                    let chunk =
                        js_string_from_bytes(buffer_copy.as_ptr(), buffer_copy.len() as u32);
                    f64::from_bits(crate::value::js_nanbox_string(chunk as i64).to_bits())
                } else {
                    buffer_value_from_bytes(&buffer_copy)
                };
                js_closure_call1(cb_ptr, chunk_val);
            }
        }
        b"end" | b"close" => {
            if err_msg.is_some() {
                return f64::from_bits(TAG_UNDEFINED_STREAM);
            }
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                js_closure_call0(cb_ptr);
            }
        }
        b"error" => {
            if let Some(msg) = err_msg {
                let cb_ptr = extract_closure_ptr(cb);
                if !cb_ptr.is_null() {
                    let msg_bytes = msg.as_bytes();
                    let err_str = js_string_from_bytes(msg_bytes.as_ptr(), msg_bytes.len() as u32);
                    let err_obj = crate::error::js_error_new_with_message(err_str);
                    let err_val = crate::value::js_nanbox_pointer(err_obj as i64);
                    js_closure_call1(cb_ptr, err_val);
                }
            }
        }
        _ => {}
    }

    f64::from_bits(TAG_UNDEFINED_STREAM)
}

/// `rs.pipe(dest)` — not implemented beyond the noop signature.
extern "C" fn read_stream_pipe_impl(_closure: *const ClosureHeader, dest: f64) -> f64 {
    dest
}

/// `rs.close()` — noop.
extern "C" fn read_stream_close_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED_STREAM)
}

// ------------------------------------------------------------
// Closure pointer extraction helper.
// ------------------------------------------------------------

/// Extract a raw ClosureHeader pointer from a NaN-boxed f64.
fn extract_closure_ptr(v: f64) -> *const ClosureHeader {
    let bits = v.to_bits();
    let top16 = bits >> 48;
    let raw = if (0x7FF8..=0x7FFF).contains(&top16) {
        // Tagged NaN-box — mask off the tag.
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return std::ptr::null();
    };
    if raw < 0x1000 || !crate::closure::is_closure_ptr(raw) {
        std::ptr::null()
    } else {
        raw as *const ClosureHeader
    }
}

// ------------------------------------------------------------
// Entry points: js_fs_create_write_stream / js_fs_create_read_stream
// ------------------------------------------------------------

/// Create a write stream bound to `path_value`. Returns a NaN-boxed
/// ObjectHeader pointer whose fields dispatch to the write-stream
/// helpers.
#[no_mangle]
pub extern "C" fn js_fs_create_write_stream(path_value: f64, options_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let write_flag = file_options_flag(options_value, "w");
    let state = StreamState {
        path,
        write_flag,
        ..StreamState::default()
    };
    let id = alloc_stream(state);
    // Method table. Order is locked in — it determines the shape keys.
    // Using a unique method count (6) that differs from the read
    // stream's (5) so the shape cache doesn't alias.
    let method_funcs: [(&str, extern "C" fn()); 6] = [
        ("write", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_write_impl,
            )
        }),
        ("end", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                write_stream_end1_impl,
            )
        }),
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(write_stream_on_impl)
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                write_stream_close_impl,
            )
        }),
    ];
    let obj = build_stream_object(id, &method_funcs);
    // NaN-box as POINTER_TAG so the dispatcher's `is_pointer()` check
    // routes through the object-field scan in js_native_call_method.
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// Create a read stream: pre-read the file contents into the
/// registry buffer, then return an ObjectHeader whose `.on` fires
/// the data/end callbacks synchronously on first call.
#[no_mangle]
pub extern "C" fn js_fs_create_read_stream(path_value: f64, options_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let mut state = StreamState {
        path: path.clone(),
        read_as_string: fs_encoding_option(options_value).is_some_and(|enc| enc != "buffer"),
        ..StreamState::default()
    };
    // Eagerly read the file so the data callback can fire synchronously.
    match std::fs::read(&path) {
        Ok(contents) => {
            let start = unsafe { options_number_field(options_value, b"start") }
                .map(|n| n.max(0.0) as usize)
                .unwrap_or(0);
            let end_inclusive = unsafe { options_number_field(options_value, b"end") }
                .map(|n| n.max(0.0) as usize)
                .unwrap_or_else(|| contents.len().saturating_sub(1));
            state.buffer = if start >= contents.len() {
                Vec::new()
            } else {
                let end_exclusive = end_inclusive.saturating_add(1).min(contents.len());
                contents[start..end_exclusive].to_vec()
            };
            state.finished = true;
        }
        Err(e) => {
            state.error_msg = Some(format!("{}", e));
        }
    }
    let id = alloc_stream(state);
    // Method set of length 5 to avoid shape-cache collision with write
    // streams (which have length 6).
    let method_funcs: [(&str, extern "C" fn()); 5] = [
        ("on", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("once", unsafe {
            std::mem::transmute::<
                extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
                extern "C" fn(),
            >(read_stream_on_impl)
        }),
        ("pipe", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(
                read_stream_pipe_impl,
            )
        }),
        ("close", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
        ("destroy", unsafe {
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                read_stream_close_impl,
            )
        }),
    ];
    let obj = build_stream_object(id, &method_funcs);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
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

/// `fs.readFile(path, encoding?, callback)` — sync read + immediate
/// callback invocation. Stub that just reads the file synchronously
/// and invokes the callback with `(null, contents)`.
#[no_mangle]
pub extern "C" fn js_fs_read_file_callback(path_value: f64, encoding: f64, callback: f64) -> f64 {
    use crate::closure::js_closure_call2;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

    let cb_ptr = last_callback(&[encoding, callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "open") {
            if !cb_ptr.is_null() {
                js_closure_call2(cb_ptr, err_val, f64::from_bits(TAG_UNDEFINED));
            }
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let encoding_is_callback = !extract_closure_ptr(encoding).is_null();
    let want_buffer = encoding_is_callback || read_file_encoding(encoding).is_none();
    let data_val = if want_buffer {
        let buf = js_fs_read_file_binary_options(path_value, encoding);
        if buf.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits())
        }
    } else {
        let str_ptr = js_fs_read_file_sync_options(path_value, encoding);
        if str_ptr.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(crate::value::js_nanbox_string(str_ptr as i64).to_bits())
        }
    };

    if !cb_ptr.is_null() {
        js_closure_call2(cb_ptr, f64::from_bits(TAG_NULL), data_val);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn last_callback(args: &[f64]) -> *const ClosureHeader {
    for value in args.iter().rev() {
        let ptr = extract_closure_ptr(*value);
        if !ptr.is_null() {
            return ptr;
        }
    }
    std::ptr::null()
}

fn call_cb0(callback: *const ClosureHeader) {
    if !callback.is_null() {
        crate::closure::js_closure_call1(callback, f64::from_bits(0x7FFC_0000_0000_0002));
    }
}

/// Invoke a 2-arg callback with (err, undefined). Used by read-style ops
/// when the pre-flight probe detected an io::Error.
unsafe fn call_cb_err2(callback: *const ClosureHeader, err_val: f64) {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if !callback.is_null() {
        crate::closure::js_closure_call2(callback, err_val, f64::from_bits(TAG_UNDEFINED));
    }
}

/// Invoke a 1-arg callback with (err). Used by void ops (mkdir/unlink/rm/…)
/// when the pre-flight probe detected an io::Error.
unsafe fn call_cb_err1(callback: *const ClosureHeader, err_val: f64) {
    if !callback.is_null() {
        crate::closure::js_closure_call1(callback, err_val);
    }
}

/// `fs.writeFile(path, data, callback)` — sync write + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_write_file_callback(
    path_value: f64,
    content_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_write_parent_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_write_file_sync_options(path_value, content_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.appendFile(path, data, callback)` — sync append + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_append_file_callback(
    path_value: f64,
    content_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_write_parent_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_append_file_sync_options(path_value, content_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.mkdir(path[, options], callback)` — sync mkdir + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_mkdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let _ = js_fs_mkdir_sync_options(path_value, options);
    call_cb0(last_callback(&[arg1, arg2]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.unlink(path, callback)` — sync unlink + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_unlink_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "unlink") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_unlink_sync(path_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rm(path[, options], callback)` — recursive sync removal + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_rm_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let _ = js_fs_rm_recursive_options(path_value, options);
    call_cb0(last_callback(&[arg1, arg2]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.access(path[, mode], callback)` — sync access + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_access_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let mode = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "access") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_access_sync_mode(path_value, mode);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.exists(path, callback)` — deprecated Node callback shape:
/// invokes the callback with a single boolean, not `(err, value)`.
#[no_mangle]
pub extern "C" fn js_fs_exists_callback(path_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let exists = js_fs_exists_sync(path_value) == 1;
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        let arg = if exists { TAG_TRUE } else { TAG_FALSE };
        crate::closure::js_closure_call1(cb, f64::from_bits(arg));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readdir(path[, options], callback)` — sync readdir + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_readdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "scandir") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let entries = js_fs_readdir_sync(path_value, arg1);
    let entries =
        f64::from_bits(crate::value::JSValue::pointer(entries.to_bits() as *const u8).bits());
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entries);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.stat(path[, options], callback)` — sync stat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_stat_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "stat") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_stat_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lstat(path[, options], callback)` — sync lstat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_lstat_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "lstat") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_lstat_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.statfs(path[, options], callback)` — sync statfs + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_statfs_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "statfs") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let stats = js_fs_statfs_sync_options(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.opendir(path[, options], callback)` — sync open + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_opendir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "opendir") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let dir = js_fs_opendir_sync(path_value);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), dir);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.glob(pattern[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_glob_callback(pattern_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let raw = js_fs_glob_sync_options(pattern_value, options);
    let entries = f64::from_bits(crate::value::JSValue::pointer(raw.to_bits() as *const u8).bits());
    let cb = last_callback(&[arg1, arg2]);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), entries);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fstat(fd, callback)` — sync fstat + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fstat_callback(fd_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let stats = js_fs_fstat_sync_options(fd_value, options);
    let cb = last_callback(&[arg1, arg2]);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), stats);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.chmod(path, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_chmod_callback(path_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "chmod") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_chmod_sync(path_value, mode_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.chown(path, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_chown_callback(
    path_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "chown") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_chown_sync(path_value, uid_value, gid_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lchown(path, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_lchown_callback(
    path_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "lchown") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_lchown_sync(path_value, uid_value, gid_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.truncate(path, len, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_truncate_callback(path_value: f64, len_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let len = if extract_closure_ptr(len_value).is_null() {
        len_value
    } else {
        0.0
    };
    let cb = last_callback(&[len_value, callback]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "open") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_truncate_sync(path_value, len);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.link(existingPath, newPath, callback)` — sync hard link + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_link_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "link") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_link_sync(from_value, to_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.symlink(target, path[, type], callback)` — sync symlink + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_symlink_callback(
    from_value: f64,
    to_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_symlink_sync(from_value, to_value);
    call_cb0(last_callback(&[arg2, arg3]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readlink(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_readlink_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(path_value, "readlink") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let value = js_fs_readlink_dispatch(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.realpath(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_realpath_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "realpath") {
            call_cb_err2(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let value = js_fs_realpath_dispatch(path_value, options);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.mkdtemp(prefix[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_callback(prefix_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let value = js_fs_mkdtemp_dispatch(prefix_value, options);
    let cb = last_callback(&[arg1, arg2]);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.open(path, flags, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_open_callback(path_value: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let cb = last_callback(&[arg1, arg2, arg3]);
    let flags = if !extract_closure_ptr(arg1).is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        arg1
    };
    // Probe only the clear read-only cases (`"r"`, `"r+"`, or undefined ⇒
    // Node defaults to `"r"`). Anything else — `"w"`, `"a"`, numeric flag
    // bitsets like `O_CREAT|O_WRONLY` — may create the file, so we defer
    // to the underlying open instead of pre-rejecting on a missing path.
    let read_only = unsafe { open_flag_is_read_only(flags) };
    if read_only {
        unsafe {
            if let Some(err_val) = fs_callback_read_error(path_value, "open") {
                call_cb_err2(cb, err_val);
                return f64::from_bits(TAG_UNDEFINED);
            }
        }
    }
    let fd = js_fs_open_sync(path_value, flags);
    if !cb.is_null() {
        crate::closure::js_closure_call2(cb, f64::from_bits(TAG_NULL), fd);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Probe used by `fs/promises.open` to decide whether to resolve with a
/// FileHandle or reject. Returns `Some(err_val)` when the underlying open
/// would fail with ENOENT/EACCES/EEXIST/etc., else `None`.
///
/// Exposed at `pub(crate)` so `node_submodules::thunk_fs_promises_open` can
/// turn the io::Error into a rejected Promise instead of resolving with a
/// FileHandle whose `fd === -1`.
pub(crate) unsafe fn fs_promises_open_probe_error(
    path_value: f64,
    flags_value: f64,
) -> Option<f64> {
    // Only probe for read-only flags; anything that may create the file —
    // including numeric `O_CREAT|…` bitsets — is left to the underlying
    // open so it can succeed when the file doesn't exist yet.
    if open_flag_is_read_only(flags_value) {
        fs_callback_read_error(path_value, "open")
    } else {
        None
    }
}

/// Returns true when an `open` flags value is unambiguously read-only.
/// Treats `undefined` (Node's default) as read-only, the string flags
/// `"r"` and `"r+"` as read-only, and everything else — including any
/// numeric/integer flag — as potentially creating, so the caller skips
/// the missing-path probe and defers to the syscall.
unsafe fn open_flag_is_read_only(flags_value: f64) -> bool {
    let jsval = crate::value::JSValue::from_bits(flags_value.to_bits());
    if jsval.is_undefined() {
        return true;
    }
    match decode_flags_string(flags_value).as_deref() {
        Some("r") | Some("r+") => true,
        _ => false,
    }
}

unsafe fn decode_flags_string(value: f64) -> Option<String> {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_string() {
        return None;
    }
    let ptr = jsval.as_string_ptr();
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(|s| s.to_string())
}

/// `fs.close(fd, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_close_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_close_sync(fd_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.cp(src, dest, options, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_cp_callback(from_value: f64, to_value: f64, arg2: f64, arg3: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "cp") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_cp_sync_options(from_value, to_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rmdir(path[, options], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_callback(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let options = if extract_closure_ptr(arg1).is_null() {
        arg1
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg1, arg2]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(path_value, "rmdir") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_rmdir_sync_options(path_value, options);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.ftruncate(fd, len, callback)` — sync ftruncate + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_ftruncate_callback(fd_value: f64, len_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_ftruncate_sync(fd_value, len_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fsync(fd, callback)` — sync fsync + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_fsync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fsync_sync(fd_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fdatasync(fd, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fdatasync_callback(fd_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fdatasync_sync(fd_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fchmod(fd, mode, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fchmod_callback(fd_value: f64, mode_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fchmod_sync(fd_value, mode_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.fchown(fd, uid, gid, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_fchown_callback(
    fd_value: f64,
    uid_value: f64,
    gid_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_fchown_sync(fd_value, uid_value, gid_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.utimes(path, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_utimes_callback(
    path_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_utimes_sync(path_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.lutimes(path, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_lutimes_callback(
    path_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_lutimes_sync(path_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.futimes(fd, atime, mtime, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_futimes_callback(
    fd_value: f64,
    atime_value: f64,
    mtime_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let _ = js_fs_futimes_sync(fd_value, atime_value, mtime_value);
    call_cb0(last_callback(&[callback]));
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.read(fd, buffer, offset, length, position, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_read_callback(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bytes = js_fs_read_sync(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    );
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.read(fd, buffer, options, callback)` object-options form.
#[no_mangle]
pub extern "C" fn js_fs_read_callback_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = js_fs_read_sync(fd_value, buffer_value, offset, length, position);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, string, callback)` / deterministic string subset.
#[no_mangle]
pub extern "C" fn js_fs_write_callback(fd_value: f64, data_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bytes = js_fs_write_sync(fd_value, data_value);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, data_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, buffer, options, callback)` object-options form.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_callback_options(
    fd_value: f64,
    buffer_value: f64,
    options_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let buffer_len = buffer_len_from_value(buffer_value) as f64;
    let offset = unsafe { options_number_field(options_value, b"offset") }.unwrap_or(0.0);
    let length = unsafe { options_number_field(options_value, b"length") }
        .unwrap_or_else(|| (buffer_len - offset).max(0.0));
    let position = unsafe { options_number_field(options_value, b"position") }
        .unwrap_or(f64::from_bits(crate::value::TAG_NULL));
    let bytes = js_fs_write_buffer_sync(fd_value, buffer_value, offset, length, position);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.write(fd, buffer, offset, length, position, callback)`.
#[no_mangle]
pub extern "C" fn js_fs_write_buffer_callback(
    fd_value: f64,
    buffer_value: f64,
    offset_value: f64,
    length_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bytes = js_fs_write_buffer_sync(
        fd_value,
        buffer_value,
        offset_value,
        length_value,
        position_value,
    );
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffer_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.readv(fd, buffers[, position], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_readv_callback(
    fd_value: f64,
    buffers_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bytes = js_fs_readv_sync(fd_value, buffers_value, position_value);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffers_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.writev(fd, buffers[, position], callback)`.
#[no_mangle]
pub extern "C" fn js_fs_writev_callback(
    fd_value: f64,
    buffers_value: f64,
    position_value: f64,
    callback: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bytes = js_fs_writev_sync(fd_value, buffers_value, position_value);
    let cb = last_callback(&[callback]);
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, f64::from_bits(TAG_NULL), bytes, buffers_value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.rename(oldPath, newPath, callback)` — sync rename + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_rename_callback(from_value: f64, to_value: f64, callback: f64) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let cb = last_callback(&[callback]);
    unsafe {
        if let Some(err_val) = fs_callback_lstat_error(from_value, "rename") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_rename_sync(from_value, to_value);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

/// `fs.copyFile(src, dest, callback)` — sync copy + immediate callback.
#[no_mangle]
pub extern "C" fn js_fs_copy_file_callback(
    from_value: f64,
    to_value: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let flags = if extract_closure_ptr(arg2).is_null() {
        arg2
    } else {
        f64::from_bits(TAG_UNDEFINED)
    };
    let cb = last_callback(&[arg2, arg3]);
    unsafe {
        if let Some(err_val) = fs_callback_read_error(from_value, "copyfile") {
            call_cb_err1(cb, err_val);
            return f64::from_bits(TAG_UNDEFINED);
        }
    }
    let _ = js_fs_copy_file_sync_flags(from_value, to_value, flags);
    call_cb0(cb);
    f64::from_bits(TAG_UNDEFINED)
}

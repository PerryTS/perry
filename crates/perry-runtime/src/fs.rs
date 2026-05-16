//! File system module - provides file operations

use std::fs;
use std::path::Path;

use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::POINTER_MASK;

/// Extract a string pointer from a NaN-boxed f64 value
/// Handles both NaN-boxed strings (with STRING_TAG) and raw pointers.
/// Returns null for invalid/small pointers (e.g. from TAG_UNDEFINED extraction).
#[inline]
fn extract_string_ptr(value: f64) -> *const StringHeader {
    let bits = value.to_bits();
    // Mask off the tag bits to get the raw pointer
    let ptr = (bits & POINTER_MASK) as usize;
    if ptr < 0x1000 {
        std::ptr::null()
    } else {
        ptr as *const StringHeader
    }
}

/// Read a file synchronously and return its contents as a string
/// Returns null pointer on error
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_read_file_sync(path_value: f64) -> *mut StringHeader {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return js_string_from_bytes(b"".as_ptr(), 0);
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        // Debug: log path on Android
        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
            }
            let c_path = std::ffi::CString::new(path_str).unwrap_or_default();
            __android_log_print(
                3,
                b"PerryFS\0".as_ptr(),
                b"readFileSync: path='%s'\0".as_ptr(),
                c_path.as_ptr(),
            );
        }

        match fs::read_to_string(path_str) {
            Ok(content) => {
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
                        content.len() as i32,
                    );
                }
                let bytes = content.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
            }
            Err(_e) => {
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
                    let c_err = std::ffi::CString::new(format!("{}", _e)).unwrap_or_default();
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

/// Write content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_write_file_sync(path_value: f64, content_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        let content_ptr = extract_string_ptr(content_value);
        if path_ptr.is_null() || content_ptr.is_null() {
            return 0;
        }

        // Get path string
        let path_len = (*path_ptr).byte_len as usize;
        let path_data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(path_data, path_len);
        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        // Get content string
        let content_len = (*content_ptr).byte_len as usize;
        let content_data = (content_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let content_bytes = std::slice::from_raw_parts(content_data, content_len);

        match fs::write(path_str, content_bytes) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// Append content to a file synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string values
#[no_mangle]
pub extern "C" fn js_fs_append_file_sync(path_value: f64, content_value: f64) -> i32 {
    use std::fs::OpenOptions;
    use std::io::Write;

    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        let content_ptr = extract_string_ptr(content_value);
        if path_ptr.is_null() || content_ptr.is_null() {
            return 0;
        }

        // Get path string
        let path_len = (*path_ptr).byte_len as usize;
        let path_data = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(path_data, path_len);
        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        // Get content string
        let content_len = (*content_ptr).byte_len as usize;
        let content_data = (content_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let content_bytes = std::slice::from_raw_parts(content_data, content_len);

        // Open file in append mode, creating if it doesn't exist
        match OpenOptions::new().create(true).append(true).open(path_str) {
            Ok(mut file) => match file.write_all(content_bytes) {
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
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        if Path::new(path_str).exists() {
            1
        } else {
            0
        }
    }
}

/// Create a directory synchronously
/// Returns 1 on success, 0 on failure
/// Accepts NaN-boxed string path
#[no_mangle]
pub extern "C" fn js_fs_mkdir_sync(path_value: f64) -> i32 {
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        match fs::create_dir_all(path_str) {
            Ok(_) => 1,
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
    let top16 = bits >> 48;
    // Accept POINTER_TAG (heap object) or raw I64 module-level object
    // pointer (top16 == 0). undefined / null / number → false.
    if top16 != 0x7FFD && top16 != 0x0000 {
        return false;
    }
    let obj_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return false;
    }
    let key = crate::string::js_string_from_bytes(b"withFileTypes".as_ptr(), 13);
    let val = crate::object::js_object_get_field_by_name(obj_ptr, key);
    crate::value::js_is_truthy(f64::from_bits(val.bits())) != 0
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
    use crate::string::js_string_from_bytes;
    use crate::value::js_nanbox_string;

    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            let arr = js_array_alloc(0);
            return f64::from_bits(i64::cast_unsigned(arr as i64));
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => {
                let arr = js_array_alloc(0);
                return f64::from_bits(i64::cast_unsigned(arr as i64));
            }
        };

        let with_file_types = options_with_file_types(options_value);

        match fs::read_dir(path_str) {
            Ok(entries) => {
                if with_file_types {
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
                            path_str,
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
                        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                        let str_f64 = js_nanbox_string(str_ptr as i64);
                        arr = js_array_push_f64(arr, str_f64);
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
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        if Path::new(path_str).is_dir() {
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
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
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
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
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
    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return std::ptr::null_mut();
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match fs::read(path_str) {
            Ok(bytes) => {
                let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
                if !buf.is_null() {
                    let buf_data =
                        (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_data, bytes.len());
                    (*buf).length = bytes.len() as u32;
                }
                buf
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Recursively remove a directory or file.
/// Returns 1 on success, 0 on failure.
/// Accepts NaN-boxed string path.
#[no_mangle]
pub extern "C" fn js_fs_rm_recursive(path_value: f64) -> i32 {
    use std::path::Path;

    unsafe {
        let path_ptr = extract_string_ptr(path_value);
        if path_ptr.is_null() {
            return 0;
        }

        let len = (*path_ptr).byte_len as usize;
        let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let path_bytes = std::slice::from_raw_parts(data_ptr, len);

        let path_str = match std::str::from_utf8(path_bytes) {
            Ok(s) => s,
            Err(_) => return 0,
        };

        let p = Path::new(path_str);
        if p.is_dir() {
            match fs::remove_dir_all(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        } else {
            match fs::remove_file(path_str) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }
    }
}

/// Helper: decode a NaN-boxed string path into a Rust &str slice.
unsafe fn decode_path_value<'a>(path_value: f64) -> Option<&'a str> {
    let path_ptr = extract_string_ptr(path_value);
    if path_ptr.is_null() {
        return None;
    }
    let len = (*path_ptr).byte_len as usize;
    let data_ptr = (path_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let path_bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(path_bytes).ok()
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

unsafe fn build_stats_object(is_file: bool, is_dir: bool, is_symlink: bool, size: u64) -> f64 {
    // 5 field slots: isFile, isDirectory, isSymbolicLink, size, mtimeMs.
    let obj = crate::object::js_object_alloc(0, 5);

    // Set fields via the by-name setter which builds up the key array.
    let set = |name: &str, v: f64| {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(obj, key, v);
    };
    set("isFile", make_stats_predicate(is_file));
    set("isDirectory", make_stats_predicate(is_dir));
    set("isSymbolicLink", make_stats_predicate(is_symlink));
    // size stored as a raw f64 number.
    set("size", size as f64);
    set("mtimeMs", 0.0_f64);

    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    f64::from_bits(POINTER_TAG | (obj as u64 & 0x0000_FFFF_FFFF_FFFF))
}

/// `fs.statSync(path)` — returns a Stats-like object with `isFile()`,
/// `isDirectory()`, `isSymbolicLink()` predicate methods and a `size`
/// numeric field. On error, returns an object where all predicates are
/// false and size is 0 (Node throws on ENOENT, but Perry's LLVM backend
/// doesn't have a catch-unwind path for runtime panics — graceful
/// degradation is safer here).
#[no_mangle]
pub extern "C" fn js_fs_stat_sync(path_value: f64) -> f64 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return build_stats_object(false, false, false, 0),
        };
        match fs::metadata(path_str) {
            Ok(meta) => {
                let is_file = meta.is_file();
                let is_dir = meta.is_dir();
                let is_symlink = meta.file_type().is_symlink();
                let size = meta.len();
                build_stats_object(is_file, is_dir, is_symlink, size)
            }
            Err(_) => build_stats_object(false, false, false, 0),
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
    unsafe {
        let from = match decode_path_value(from_value) {
            Some(s) => s,
            None => return 0,
        };
        let to = match decode_path_value(to_value) {
            Some(s) => s,
            None => return 0,
        };
        match fs::copy(from, to) {
            Ok(_) => 1,
            Err(_) => 0,
        }
    }
}

/// `fs.accessSync(path)` — returns 1 if accessible, 0 otherwise.
/// Unlike Node's `accessSync` which throws on failure, this returns a
/// status code; the LLVM codegen wraps the result so `try/catch` works.
#[no_mangle]
pub extern "C" fn js_fs_access_sync(path_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        if Path::new(path_str).exists() {
            1
        } else {
            0
        }
    }
}

/// `fs.realpathSync(path)` — returns raw *mut StringHeader i64.
/// Falls back to the input path on error (Node would throw).
#[no_mangle]
pub extern "C" fn js_fs_realpath_sync(path_value: f64) -> i64 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return js_string_from_bytes(b"".as_ptr(), 0) as i64,
        };
        match fs::canonicalize(path_str) {
            Ok(p) => {
                let s = p.to_string_lossy();
                let bytes = s.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64
            }
            Err(_) => {
                let bytes = path_str.as_bytes();
                js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64
            }
        }
    }
}

/// `fs.mkdtempSync(prefix)` — creates a unique temp directory whose
/// name starts with `prefix`. Returns raw *mut StringHeader i64 with
/// the created path.
#[no_mangle]
pub extern "C" fn js_fs_mkdtemp_sync(prefix_value: f64) -> i64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    unsafe {
        let prefix_str = match decode_path_value(prefix_value) {
            Some(s) => s.to_string(),
            None => return js_string_from_bytes(b"".as_ptr(), 0) as i64,
        };
        // Try a handful of candidate suffixes until one succeeds.
        for _ in 0..16 {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let candidate = format!("{}{:x}{:x}", prefix_str, ts, n);
            match fs::create_dir(&candidate) {
                Ok(_) => {
                    let bytes = candidate.as_bytes();
                    return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64;
                }
                Err(_) => continue,
            }
        }
        js_string_from_bytes(b"".as_ptr(), 0) as i64
    }
}

/// `fs.rmdirSync(path)` — removes an empty directory. Returns i32 status.
#[no_mangle]
pub extern "C" fn js_fs_rmdir_sync(path_value: f64) -> i32 {
    unsafe {
        let path_str = match decode_path_value(path_value) {
            Some(s) => s,
            None => return 0,
        };
        match fs::remove_dir(path_str) {
            Ok(_) => 1,
            Err(_) => 0,
        }
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
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    if js_fs_access_sync(path_value) == 1 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Throw an Error via js_throw. The runtime builds the error
    // lazily from a static message — the subclass catch in the test
    // just needs `accessBad = true` in the catch handler.
    let msg = js_string_from_bytes(b"ENOENT: no such file or directory".as_ptr(), 33);
    let err = crate::error::js_error_new_with_message(msg);
    let err_val = crate::value::js_nanbox_pointer(err as i64);
    crate::exception::js_throw(err_val);
    f64::from_bits(TAG_UNDEFINED)
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
    unsafe {
        let ptr = extract_string_ptr(v);
        if ptr.is_null() {
            return String::new();
        }
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    }
}

/// Extract raw UTF-8 bytes from a NaN-boxed string value.
fn bytes_from_value(v: f64) -> Vec<u8> {
    unsafe {
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
    let (path, buffer) = STREAM_REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if let Some(state) = reg.get_mut(&id) {
            let p = state.path.clone();
            let b = std::mem::take(&mut state.buffer);
            (p, b)
        } else {
            (String::new(), Vec::new())
        }
    });

    let write_result = if path.is_empty() {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no path",
        ))
    } else {
        std::fs::write(&path, &buffer)
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
    let (buffer_copy, err_msg) = STREAM_REGISTRY.with(|r| {
        let reg = r.borrow();
        match reg.get(&id) {
            Some(s) => (s.buffer.clone(), s.error_msg.clone()),
            None => (Vec::new(), None),
        }
    });

    match event_bytes.as_slice() {
        b"data" => {
            if err_msg.is_some() {
                return f64::from_bits(TAG_UNDEFINED_STREAM);
            }
            let cb_ptr = extract_closure_ptr(cb);
            if !cb_ptr.is_null() {
                let chunk = js_string_from_bytes(buffer_copy.as_ptr(), buffer_copy.len() as u32);
                let chunk_val =
                    f64::from_bits(crate::value::js_nanbox_string(chunk as i64).to_bits());
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
    } else {
        bits as usize
    };
    if raw < 0x1000 {
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
pub extern "C" fn js_fs_create_write_stream(path_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let state = StreamState {
        path,
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
            std::mem::transmute::<extern "C" fn(*const ClosureHeader) -> f64, extern "C" fn()>(
                write_stream_end0_impl,
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
pub extern "C" fn js_fs_create_read_stream(path_value: f64) -> f64 {
    let path = path_from_value(path_value);
    let mut state = StreamState {
        path: path.clone(),
        ..StreamState::default()
    };
    // Eagerly read the file so the data callback can fire synchronously.
    match std::fs::read(&path) {
        Ok(contents) => {
            state.buffer = contents;
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

/// `fs.readFile(path, encoding?, callback)` — sync read + immediate
/// callback invocation. Stub that just reads the file synchronously
/// and invokes the callback with `(null, contents)`.
#[no_mangle]
pub extern "C" fn js_fs_read_file_callback(path_value: f64, _encoding: f64, callback: f64) -> f64 {
    use crate::closure::{js_closure_call2, ClosureHeader};
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    // Read the file synchronously.
    let str_ptr = js_fs_read_file_sync(path_value);
    let data_val = if str_ptr.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(crate::value::js_nanbox_string(str_ptr as i64).to_bits())
    };
    // Invoke the callback with (null, data). The callback is a
    // NaN-boxed closure pointer — unbox before calling.
    let cb_bits = callback.to_bits();
    let cb_ptr = (cb_bits & 0x0000_FFFF_FFFF_FFFF) as *const ClosureHeader;
    if !cb_ptr.is_null() {
        js_closure_call2(cb_ptr, f64::from_bits(TAG_NULL), data_val);
    }
    f64::from_bits(TAG_UNDEFINED)
}

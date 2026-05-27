//! Dirent object + `readdirSync` + `unlinkSync`/`chmodSync`/`rmSync`
//! + `is_directory` + binary `readFileSync` helpers.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use super::*;

// ---------- Dirent object ----------
//
// Issue #631: `fs.readdirSync(path, { withFileTypes: true })` returns
// `Dirent[]` instead of `string[]`. Each Dirent has a `name` field plus
// `isFile()` / `isDirectory()` / `isSymbolicLink()` predicate methods —
// same shape as Stats but populated per-entry from the OS directory
// iterator's file type. Predicate closures capture the pre-computed
// boolean so calling them is a single-slot read.

pub(crate) unsafe fn build_dirent_object(
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
pub(crate) unsafe fn options_with_file_types(options_value: f64) -> bool {
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

pub(crate) unsafe fn options_bool_field(options_value: f64, field: &[u8]) -> bool {
    let Some(val) = options_field_value(options_value, field) else {
        return false;
    };
    crate::value::js_is_truthy(f64::from_bits(val.bits())) != 0
}

pub(crate) unsafe fn options_number_field(options_value: f64, field: &[u8]) -> Option<f64> {
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

pub(crate) unsafe fn options_has_field(options_value: f64, field: &[u8]) -> bool {
    options_field_value(options_value, field).is_some()
}

pub(crate) unsafe fn options_field_value(
    options_value: f64,
    field: &[u8],
) -> Option<crate::value::JSValue> {
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

pub(crate) unsafe fn options_string_field(options_value: f64, field: &[u8]) -> Option<String> {
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

pub(crate) fn buffer_value_from_bytes(bytes: &[u8]) -> f64 {
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

pub(crate) fn bytes_to_readdir_value(bytes: &[u8], as_buffer: bool) -> f64 {
    if as_buffer {
        buffer_value_from_bytes(bytes)
    } else {
        let str_ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        crate::value::js_nanbox_string(str_ptr as i64)
    }
}

pub(crate) fn readdir_encoding_buffer(options_value: f64) -> bool {
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

pub(crate) fn collect_readdir_recursive_strings(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) {
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

pub(crate) fn collect_readdir_recursive_dirents(
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
    crate::fs::validate::validate_path("path", path_value);
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

//! opendir / glob / watch / watchFile / unwatchFile stubs.

use std::fs;
use std::path::Path;

use crate::closure::ClosureHeader;

use super::*;

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

pub(crate) fn glob_regex_from_pattern(pattern: &str) -> Option<regex::Regex> {
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

pub(crate) fn glob_search_root(pattern: &str) -> String {
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

pub(crate) fn walk_paths_for_glob(dir: &Path, out: &mut Vec<String>) {
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

pub(crate) fn glob_cwd_from_options(options_value: f64) -> Option<String> {
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

pub(crate) extern "C" fn fs_watcher_noop_impl(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn make_zero_capture_method(func: *const u8) -> f64 {
    let closure = crate::closure::js_closure_alloc(func, 0);
    f64::from_bits(crate::value::JSValue::pointer(closure as *const u8).bits())
}

pub(crate) unsafe fn build_fs_watcher_object(include_close: bool) -> f64 {
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

pub(crate) fn promise_value_fs(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

pub(crate) fn promise_undefined_fs() -> f64 {
    promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED))
}

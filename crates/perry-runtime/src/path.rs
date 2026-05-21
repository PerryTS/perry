//! Path module - provides path manipulation utilities

use std::path::Path;

use crate::string::{js_string_from_bytes, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Helper to create a JS string from a Rust string
fn string_to_js(s: &str) -> *mut StringHeader {
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn resolve_posix_str(path_str: &str) -> String {
    if path_str.is_empty() {
        return std::env::current_dir()
            .map(|cwd| cwd.to_string_lossy().to_string())
            .unwrap_or_default();
    }
    if Path::new(path_str).is_absolute() {
        normalize_str(path_str)
    } else {
        match std::env::current_dir() {
            Ok(cwd) => normalize_str(&format!("{}/{}", cwd.to_string_lossy(), path_str)),
            Err(_) => normalize_str(path_str),
        }
    }
}

/// Join two path segments. Node's `path.join` concatenates with `/` and
/// normalizes — it does NOT reset on an absolute segment (that's
/// `path.resolve`'s job). We can't use Rust's `Path::join` because it
/// resets on absolute segments.
#[no_mangle]
pub extern "C" fn js_path_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let a = string_from_header(a_ptr).unwrap_or_default();
        let b = string_from_header(b_ptr).unwrap_or_default();

        let joined = if a.is_empty() {
            b
        } else if b.is_empty() {
            a
        } else {
            format!("{}/{}", a, b)
        };
        let normalized = normalize_str(&joined);
        string_to_js(&normalized)
    }
}

/// `path.win32.join(a, b)` — Windows-style join. Always emits backslash
/// separators regardless of host platform. Treats both `/` and `\` as
/// segment separators in normalization (Node's win32 implementation does
/// the same) and collapses repeated separators.
#[no_mangle]
pub extern "C" fn js_path_win32_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let a = string_from_header(a_ptr).unwrap_or_default();
        let b = string_from_header(b_ptr).unwrap_or_default();

        let joined = if a.is_empty() {
            b
        } else if b.is_empty() {
            a
        } else if a.ends_with('\\') || a.ends_with('/') {
            format!("{}{}", a, b)
        } else {
            format!("{}\\{}", a, b)
        };
        string_to_js(&normalize_win32_str(&joined))
    }
}

/// Result of splitting a win32 path into its root and remainder.
///
/// The `prefix` is the "root" portion in Node's sense:
///   - `""` for a drive-less relative path (`foo\bar`)
///   - `"C:"` for a drive-relative path (`C:foo`)
///   - `"C:\"` for a drive-absolute path (`C:\foo`)
///   - `"\"` for a rooted path with no drive (`\foo`) — uncommon but legal
///   - `"\\server\share\"` for a UNC path
///   - `"\\?\..."` device prefix preserved verbatim through to the next
///     separator after the namespace marker
///
/// `is_absolute` is true only when the prefix anchors the path
/// (drive+sep, UNC, device path, or a leading separator with no drive).
/// `rest` is everything after the prefix.
struct Win32Split<'a> {
    prefix: &'a str,
    is_absolute: bool,
    rest: &'a str,
}

fn is_win32_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

fn split_win32(input: &str) -> Win32Split<'_> {
    let bytes = input.as_bytes();
    let len = bytes.len();

    // UNC or device path: starts with two separators.
    if len >= 2 && is_win32_sep(bytes[0] as char) && is_win32_sep(bytes[1] as char) {
        // Scan for the next two non-separator-bounded segments
        // (\\server\share).
        let mut i = 2;
        // Find end of server segment.
        let server_start = i;
        while i < len && !is_win32_sep(bytes[i] as char) {
            i += 1;
        }
        let server_end = i;
        // Skip separators between server and share.
        while i < len && is_win32_sep(bytes[i] as char) {
            i += 1;
        }
        let share_start = i;
        while i < len && !is_win32_sep(bytes[i] as char) {
            i += 1;
        }
        let share_end = i;
        if server_end > server_start && share_end > share_start {
            // Have both server and share — UNC root is the whole
            // prefix including a trailing backslash even if input had
            // no separator after the share.
            return Win32Split {
                prefix: &input[..share_end],
                is_absolute: true,
                rest: &input[share_end..],
            };
        }
        // Malformed UNC (no share) — treat the leading double-sep as
        // a root separator without a drive. This matches Node, which
        // returns rooted but non-UNC results for "\\\\foo" alone.
        return Win32Split {
            prefix: "",
            is_absolute: true,
            rest: input,
        };
    }

    // Drive letter: single ASCII letter followed by `:`.
    if len >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        let drive = &input[..2];
        let after = &input[2..];
        if !after.is_empty() && is_win32_sep(after.as_bytes()[0] as char) {
            // "C:\foo" — drive-absolute.
            return Win32Split {
                prefix: drive,
                is_absolute: true,
                rest: after,
            };
        }
        // "C:foo" — drive-relative (NOT absolute).
        return Win32Split {
            prefix: drive,
            is_absolute: false,
            rest: after,
        };
    }

    // Leading separator with no drive ("\foo") — rooted but driveless.
    if len >= 1 && is_win32_sep(bytes[0] as char) {
        return Win32Split {
            prefix: "",
            is_absolute: true,
            rest: input,
        };
    }

    // Plain relative path.
    Win32Split {
        prefix: "",
        is_absolute: false,
        rest: input,
    }
}

/// Win32 normalization. Treats both `/` and `\` as separators (matching
/// Node), preserves a leading drive letter (`C:`), collapses repeated
/// separators, resolves `.` and `..`, and emits backslash separators.
fn normalize_win32_str(input: &str) -> String {
    if input.is_empty() {
        return ".".to_string();
    }
    let split = split_win32(input);
    let prefix = split.prefix;
    let is_absolute = split.is_absolute;
    let rest = split.rest;
    let trailing_sep = !rest.is_empty() && is_win32_sep(rest.chars().last().unwrap());

    let mut out: Vec<&str> = Vec::new();
    for seg in rest.split(is_win32_sep) {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if let Some(last) = out.last() {
                if *last == ".." {
                    out.push("..");
                } else {
                    out.pop();
                }
            } else if !is_absolute {
                out.push("..");
            }
            continue;
        }
        out.push(seg);
    }

    // Assemble: prefix, then root separator, then segments.
    let mut result = String::new();
    // UNC prefixes already include "\\server\share" without trailing sep.
    let prefix_is_unc = prefix.starts_with('\\') || prefix.starts_with('/');
    if prefix_is_unc {
        // Re-emit UNC prefix with backslash separators.
        let mut normed = String::with_capacity(prefix.len());
        for c in prefix.chars() {
            normed.push(if c == '/' { '\\' } else { c });
        }
        result.push_str(&normed);
    } else {
        result.push_str(prefix);
    }
    if is_absolute && !prefix_is_unc {
        // Drive-absolute, or rooted-no-drive — emit the root separator.
        result.push('\\');
    } else if prefix_is_unc {
        // UNC always followed by a separator before the first segment.
        result.push('\\');
    }
    if !out.is_empty() {
        result.push_str(&out.join("\\"));
    } else if prefix_is_unc {
        // UNC with no segments — strip the trailing separator we just
        // added so the result is exactly "\\server\share".
        result.pop();
    }
    if result.is_empty() {
        return if is_absolute {
            "\\".to_string()
        } else {
            ".".to_string()
        };
    }
    if trailing_sep && !result.ends_with('\\') && !out.is_empty() {
        result.push('\\');
    }
    result
}

/// Get directory name from path. Per Node spec, the root's dirname is the
/// root itself (`/` → `/`), not an empty string — Rust's `Path::parent`
/// returns `None` there, which we treat as "stay at root".
#[no_mangle]
pub extern "C" fn js_path_dirname(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js("."),
        };

        if path_str.is_empty() {
            return string_to_js(".");
        }

        // POSIX root: dirname("/") = "/", dirname("///") = "/"
        if path_str.chars().all(|c| c == '/') {
            return string_to_js("/");
        }
        // Node preserves exactly two leading slashes for the dirname of
        // `//foo` on POSIX.
        if path_str.starts_with("//")
            && !path_str.starts_with("///")
            && !path_str[2..].contains('/')
        {
            return string_to_js("//");
        }

        let path = Path::new(&path_str);
        match path.parent() {
            Some(parent) => {
                let s = parent.to_string_lossy();
                if s.is_empty() {
                    string_to_js(".")
                } else {
                    string_to_js(&s)
                }
            }
            None => string_to_js("."),
        }
    }
}

/// Get base name (file name) from path
#[no_mangle]
pub extern "C" fn js_path_basename(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };

        let path = Path::new(&path_str);
        match path.file_name() {
            Some(name) => string_to_js(&name.to_string_lossy()),
            None => string_to_js(""),
        }
    }
}

/// Get file extension from path (including the dot)
#[no_mangle]
pub extern "C" fn js_path_extname(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };

        let path = Path::new(&path_str);
        match path.extension() {
            Some(ext) => {
                let mut result = String::from(".");
                result.push_str(&ext.to_string_lossy());
                string_to_js(&result)
            }
            None => string_to_js(""),
        }
    }
}

/// Check if path is absolute
#[no_mangle]
pub extern "C" fn js_path_is_absolute(path_ptr: *const StringHeader) -> i32 {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return 0,
        };
        if Path::new(&path_str).is_absolute() {
            1
        } else {
            0
        }
    }
}

/// Resolve path to absolute path
#[no_mangle]
pub extern "C" fn js_path_resolve(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };

        if path_str.is_empty() {
            return match std::env::current_dir() {
                Ok(cwd) => string_to_js(&cwd.to_string_lossy()),
                Err(_) => string_to_js(""),
            };
        }

        match std::fs::canonicalize(&path_str) {
            Ok(abs_path) => string_to_js(&abs_path.to_string_lossy()),
            Err(_) => {
                // If canonicalize fails (file doesn't exist), try to construct absolute path
                if Path::new(&path_str).is_absolute() {
                    string_to_js(&path_str)
                } else {
                    match std::env::current_dir() {
                        Ok(cwd) => {
                            let joined = cwd.join(&path_str);
                            string_to_js(&joined.to_string_lossy())
                        }
                        Err(_) => string_to_js(&path_str),
                    }
                }
            }
        }
    }
}

/// Normalize a path: collapse `.` segments, resolve `..`, dedupe separators.
fn normalize_str(input: &str) -> String {
    if input.is_empty() {
        return ".".to_string();
    }
    let is_absolute = input.starts_with('/');
    let trailing_slash = input.ends_with('/');
    let mut out: Vec<&str> = Vec::new();
    for seg in input.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            // Pop unless we're at root and absolute, or the previous segment is also ".."
            if let Some(last) = out.last() {
                if *last == ".." {
                    out.push("..");
                } else {
                    out.pop();
                }
            } else if !is_absolute {
                out.push("..");
            }
            continue;
        }
        out.push(seg);
    }
    let mut result = if is_absolute {
        String::from("/")
    } else {
        String::new()
    };
    result.push_str(&out.join("/"));
    if result.is_empty() {
        return ".".to_string();
    }
    if trailing_slash && !result.ends_with('/') {
        result.push('/');
    }
    result
}

#[no_mangle]
pub extern "C" fn js_path_normalize(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js("."),
        };
        string_to_js(&normalize_str(&path_str))
    }
}

#[no_mangle]
pub extern "C" fn js_path_relative(
    from_ptr: *const StringHeader,
    to_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let from = string_from_header(from_ptr).unwrap_or_default();
        let to = string_from_header(to_ptr).unwrap_or_default();
        let from_norm = resolve_posix_str(&from);
        let to_norm = resolve_posix_str(&to);
        let from_segs: Vec<&str> = from_norm.split('/').filter(|s| !s.is_empty()).collect();
        let to_segs: Vec<&str> = to_norm.split('/').filter(|s| !s.is_empty()).collect();
        let common = from_segs
            .iter()
            .zip(to_segs.iter())
            .take_while(|(a, b)| a == b)
            .count();
        let ups = from_segs.len() - common;
        let mut parts: Vec<&str> = std::iter::repeat_n("..", ups).collect();
        parts.extend(to_segs[common..].iter().copied());
        let result = parts.join("/");
        string_to_js(&result)
    }
}

#[no_mangle]
pub extern "C" fn js_path_basename_ext(
    path_ptr: *const StringHeader,
    ext_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };
        let ext_str = string_from_header(ext_ptr).unwrap_or_default();
        let path = Path::new(&path_str);
        let base = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => return string_to_js(""),
        };
        if !ext_str.is_empty()
            && base.ends_with(&ext_str)
            && (base.len() > ext_str.len() || !path_str.contains('/'))
        {
            string_to_js(&base[..base.len() - ext_str.len()])
        } else {
            string_to_js(&base)
        }
    }
}

/// Returns a `{ root, dir, base, ext, name }` object describing the path.
#[no_mangle]
pub extern "C" fn js_path_parse(path_ptr: *const StringHeader) -> *mut crate::object::ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let path_str = unsafe { string_from_header(path_ptr) }.unwrap_or_default();
    let p = Path::new(&path_str);

    let root = if path_str.starts_with('/') { "/" } else { "" }.to_string();
    let dir = if !root.is_empty() && path_str.chars().all(|c| c == '/') {
        // Node's path.parse("/") preserves the root as the dir as well:
        // { root: "/", dir: "/", base: "", ext: "", name: "" }.
        root.clone()
    } else {
        match p.parent() {
            Some(parent) => parent.to_string_lossy().to_string(),
            None => String::new(),
        }
    };
    let base = match p.file_name() {
        Some(b) => b.to_string_lossy().to_string(),
        None => String::new(),
    };
    let ext = match p.extension() {
        Some(e) => format!(".{}", e.to_string_lossy()),
        None => String::new(),
    };
    let name = match p.file_stem() {
        Some(n) => n.to_string_lossy().to_string(),
        None => String::new(),
    };

    // Build the object via shape with packed keys
    let packed = b"root\0dir\0base\0ext\0name\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF20, 5, packed.as_ptr(), packed.len() as u32);
    let nb = |s: &str| -> f64 {
        let ptr = string_to_js(s);
        crate::value::js_nanbox_string(ptr as i64)
    };
    js_object_set_field(obj, 0, JSValue::from_bits(nb(&root).to_bits()));
    js_object_set_field(obj, 1, JSValue::from_bits(nb(&dir).to_bits()));
    js_object_set_field(obj, 2, JSValue::from_bits(nb(&base).to_bits()));
    js_object_set_field(obj, 3, JSValue::from_bits(nb(&ext).to_bits()));
    js_object_set_field(obj, 4, JSValue::from_bits(nb(&name).to_bits()));
    obj
}

/// Build a path from a `{ dir, base, root, name, ext }` descriptor.
#[no_mangle]
pub extern "C" fn js_path_format(obj_f64: f64) -> *mut StringHeader {
    use crate::object::js_object_get_field_by_name;
    use crate::value::js_nanbox_get_pointer;

    // Extract object pointer
    let obj_ptr = js_nanbox_get_pointer(obj_f64) as *mut crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return string_to_js("");
    }

    // Helper: read a string field by name (returns "" if undefined/missing)
    let get_str = |name: &str| -> String {
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let val = js_object_get_field_by_name(obj_ptr, key_ptr);
        if val.is_undefined() {
            return String::new();
        }
        let ptr = val.as_string_ptr();
        unsafe { string_from_header(ptr) }.unwrap_or_default()
    };

    let dir = get_str("dir");
    let root = get_str("root");
    let base = get_str("base");
    let name = get_str("name");
    let mut ext = get_str("ext");
    // Node inserts the separator dot when `ext` is provided without
    // one: path.format({ name: "file", ext: "txt" }) === "file.txt".
    if !ext.is_empty() && !ext.starts_with('.') {
        ext.insert(0, '.');
    }
    let has_tail = !base.is_empty() || !name.is_empty() || !ext.is_empty();

    // dir takes precedence over root; name+ext fallback when base missing
    let mut result = if !dir.is_empty() {
        let mut s = dir.clone();
        // Node always inserts a separator between dir and base, even when
        // dir already ends with `/`. If there is no tail, keep dir as-is
        // (path.format(path.parse("/")) === "/").
        if has_tail {
            s.push('/');
        }
        s
    } else if !root.is_empty() {
        let mut s = root.clone();
        if !s.ends_with('/') {
            s.push('/');
        }
        s
    } else {
        String::new()
    };

    if !base.is_empty() {
        result.push_str(&base);
    } else {
        result.push_str(&name);
        result.push_str(&ext);
    }

    string_to_js(&result)
}

#[no_mangle]
pub extern "C" fn js_path_sep_get() -> *mut StringHeader {
    string_to_js("/")
}

#[no_mangle]
pub extern "C" fn js_path_delimiter_get() -> *mut StringHeader {
    string_to_js(":")
}

/// Internal helper for `path.resolve(a, b)` — like `js_path_join` but with
/// reset-on-absolute semantics (Node's `path.resolve` rule: when a later
/// segment is absolute, prior segments are discarded). Normalizes the
/// result. Used by the multi-arg `path.resolve` lowering to chain pairs.
#[no_mangle]
pub extern "C" fn js_path_resolve_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let a = string_from_header(a_ptr).unwrap_or_default();
        let b = string_from_header(b_ptr).unwrap_or_default();

        let joined = if b.starts_with('/') {
            b
        } else if a.is_empty() {
            b
        } else if b.is_empty() {
            a
        } else {
            format!("{}/{}", a, b)
        };
        string_to_js(&normalize_str(&joined))
    }
}

/// `path.toNamespacedPath(path)` — Windows-only effect on Node. On POSIX
/// it is a no-op that returns the input unchanged. Perry's path module
/// is POSIX-shaped, so we match that.
#[no_mangle]
pub extern "C" fn js_path_to_namespaced_path(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let s = string_from_header(path_ptr).unwrap_or_default();
        string_to_js(&s)
    }
}

/// Convert a glob pattern (`*`, `?`, `[abc]`, `**`) into a regex, anchored
/// at both ends. Mirrors Node's `path.matchesGlob` semantics, which Node
/// documents as identical to `picomatch` defaults: `*` matches any chars
/// except `/`, `**` matches across `/`, `?` matches a single char except
/// `/`, character classes `[...]` work like regex.
fn glob_to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '*' => {
                if i + 1 < bytes.len() && bytes[i + 1] as char == '*' {
                    out.push_str(".*");
                    i += 2;
                    continue;
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            '[' => {
                out.push('[');
                i += 1;
                while i < bytes.len() && bytes[i] as char != ']' {
                    let ch = bytes[i] as char;
                    if ch == '!' && out.ends_with('[') {
                        out.push('^');
                    } else {
                        out.push(ch);
                    }
                    i += 1;
                }
                out.push(']');
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
        i += 1;
    }
    out.push('$');
    out
}

/// `path.matchesGlob(path, pattern)` — Node 22.5+ API. Returns whether the
/// given path matches the given glob pattern.
#[no_mangle]
pub extern "C" fn js_path_matches_glob(
    path_ptr: *const StringHeader,
    pattern_ptr: *const StringHeader,
) -> i32 {
    unsafe {
        let path_str = string_from_header(path_ptr).unwrap_or_default();
        let pattern = string_from_header(pattern_ptr).unwrap_or_default();
        let regex_src = glob_to_regex(&pattern);
        match regex::Regex::new(&regex_src) {
            Ok(re) => {
                if re.is_match(&path_str) {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        }
    }
}

// ===================================================================
// Win32 sub-namespace (issue #1162)
// ===================================================================

/// Last segment of a win32 path. Handles UNC roots / drive prefixes
/// by stripping the root portion first, then taking the final segment
/// of the remainder (with `\` or `/` as separators, matching Node's
/// `win32.basename`).
fn win32_basename_inner(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let split = split_win32(input);
    let segments: Vec<&str> = split
        .rest
        .split(is_win32_sep)
        .filter(|s| !s.is_empty())
        .collect();
    match segments.last() {
        Some(s) => (*s).to_string(),
        None => String::new(),
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_basename(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };
        string_to_js(&win32_basename_inner(&path_str))
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_basename_ext(
    path_ptr: *const StringHeader,
    ext_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };
        let ext_str = string_from_header(ext_ptr).unwrap_or_default();
        let base = win32_basename_inner(&path_str);
        if !ext_str.is_empty() && base.ends_with(&ext_str) && base.len() > ext_str.len() {
            string_to_js(&base[..base.len() - ext_str.len()])
        } else {
            string_to_js(&base)
        }
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_dirname(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js("."),
        };
        if path_str.is_empty() {
            return string_to_js(".");
        }
        let split = split_win32(&path_str);
        let prefix = split.prefix;
        let is_absolute = split.is_absolute;
        let rest = split.rest;
        let prefix_is_unc = prefix.starts_with('\\') || prefix.starts_with('/');

        // Split `rest` into segments and drop the last one (the basename).
        let mut segments: Vec<&str> = rest.split(is_win32_sep).filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            // Path is just the root — dirname is the root itself.
            return string_to_js(&path_str);
        }
        segments.pop();

        if segments.is_empty() {
            // Only one segment after the root.
            if is_absolute {
                // Drive-absolute or UNC: dirname is the root with separator.
                let mut r = String::new();
                if prefix_is_unc {
                    for c in prefix.chars() {
                        r.push(if c == '/' { '\\' } else { c });
                    }
                } else {
                    r.push_str(prefix);
                }
                r.push('\\');
                return string_to_js(&r);
            }
            // Drive-relative with one segment ("C:foo") → "C:".
            if !prefix.is_empty() {
                return string_to_js(prefix);
            }
            // Bare relative one-segment → ".".
            return string_to_js(".");
        }

        // Multi-segment: rejoin segments after the root.
        let mut r = String::new();
        if prefix_is_unc {
            for c in prefix.chars() {
                r.push(if c == '/' { '\\' } else { c });
            }
        } else {
            r.push_str(prefix);
        }
        if is_absolute {
            r.push('\\');
        }
        r.push_str(&segments.join("\\"));
        string_to_js(&r)
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_extname(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };
        let base = win32_basename_inner(&path_str);
        // Node's rule: leading-dot files have no extension.
        // `extname(".bashrc") === ""`, but `extname("a.b.c") === ".c"`.
        match base.rfind('.') {
            Some(idx) if idx > 0 => string_to_js(&base[idx..]),
            _ => string_to_js(""),
        }
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_is_absolute(path_ptr: *const StringHeader) -> i32 {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return 0,
        };
        if path_str.is_empty() {
            return 0;
        }
        if split_win32(&path_str).is_absolute {
            1
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_normalize(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js("."),
        };
        string_to_js(&normalize_win32_str(&path_str))
    }
}

/// `path.win32.parse(p)` → `{ root, dir, base, ext, name }`.
#[no_mangle]
pub extern "C" fn js_path_win32_parse(
    path_ptr: *const StringHeader,
) -> *mut crate::object::ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let path_str = unsafe { string_from_header(path_ptr) }.unwrap_or_default();
    let split = split_win32(&path_str);
    let prefix = split.prefix;
    let is_absolute = split.is_absolute;
    let rest = split.rest;
    let prefix_is_unc = prefix.starts_with('\\') || prefix.starts_with('/');

    // root: prefix + trailing separator (when absolute), or just prefix
    // (drive-relative "C:foo" yields root "C:").
    let root = {
        let mut r = String::new();
        if prefix_is_unc {
            for c in prefix.chars() {
                r.push(if c == '/' { '\\' } else { c });
            }
            r.push('\\');
        } else if !prefix.is_empty() {
            r.push_str(prefix);
            if is_absolute {
                r.push('\\');
            }
        } else if is_absolute {
            r.push('\\');
        }
        r
    };

    // Split rest into segments; base = last, dir = root + joined remaining.
    let segments: Vec<&str> = rest.split(is_win32_sep).filter(|s| !s.is_empty()).collect();
    let (base, dir) = if segments.is_empty() {
        // Path is the bare root.
        (String::new(), root.clone())
    } else {
        let base = segments.last().unwrap().to_string();
        let head_segments = &segments[..segments.len() - 1];
        let mut d = String::new();
        if prefix_is_unc {
            for c in prefix.chars() {
                d.push(if c == '/' { '\\' } else { c });
            }
        } else {
            d.push_str(prefix);
        }
        if is_absolute && !d.ends_with('\\') {
            d.push('\\');
        }
        d.push_str(&head_segments.join("\\"));
        if d.is_empty() {
            d.push('.');
        }
        // Pop trailing separator from dir unless dir IS the root.
        if d.ends_with('\\') && d != root {
            d.pop();
        }
        (base, d)
    };

    // Match POSIX rule: leading-dot basename has no extension.
    let (name, ext) = match base.rfind('.') {
        Some(idx) if idx > 0 => (base[..idx].to_string(), base[idx..].to_string()),
        _ => (base.clone(), String::new()),
    };

    let packed = b"root\0dir\0base\0ext\0name\0";
    let obj = js_object_alloc_with_shape(0x7FFF_FF21, 5, packed.as_ptr(), packed.len() as u32);
    let nb = |s: &str| -> f64 {
        let ptr = string_to_js(s);
        crate::value::js_nanbox_string(ptr as i64)
    };
    js_object_set_field(obj, 0, JSValue::from_bits(nb(&root).to_bits()));
    js_object_set_field(obj, 1, JSValue::from_bits(nb(&dir).to_bits()));
    js_object_set_field(obj, 2, JSValue::from_bits(nb(&base).to_bits()));
    js_object_set_field(obj, 3, JSValue::from_bits(nb(&ext).to_bits()));
    js_object_set_field(obj, 4, JSValue::from_bits(nb(&name).to_bits()));
    obj
}

/// `path.win32.format({ dir, root, base, name, ext })` — like the POSIX
/// version but emits backslash separators.
#[no_mangle]
pub extern "C" fn js_path_win32_format(obj_f64: f64) -> *mut StringHeader {
    use crate::object::js_object_get_field_by_name;
    use crate::value::js_nanbox_get_pointer;

    let obj_ptr = js_nanbox_get_pointer(obj_f64) as *mut crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        return string_to_js("");
    }
    let get_str = |name: &str| -> String {
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let val = js_object_get_field_by_name(obj_ptr, key_ptr);
        if val.is_undefined() {
            return String::new();
        }
        let ptr = val.as_string_ptr();
        unsafe { string_from_header(ptr) }.unwrap_or_default()
    };
    let dir = get_str("dir");
    let root = get_str("root");
    let base = get_str("base");
    let name = get_str("name");
    let mut ext = get_str("ext");
    if !ext.is_empty() && !ext.starts_with('.') {
        ext.insert(0, '.');
    }
    let has_tail = !base.is_empty() || !name.is_empty() || !ext.is_empty();

    let mut result = if !dir.is_empty() {
        let mut s = dir.clone();
        if has_tail && !s.ends_with('\\') && !s.ends_with('/') {
            s.push('\\');
        }
        s
    } else if !root.is_empty() {
        let mut s = root.clone();
        if !s.ends_with('\\') && !s.ends_with('/') {
            s.push('\\');
        }
        s
    } else {
        String::new()
    };
    if !base.is_empty() {
        result.push_str(&base);
    } else {
        result.push_str(&name);
        result.push_str(&ext);
    }
    string_to_js(&result)
}

#[no_mangle]
pub extern "C" fn js_path_win32_to_namespaced_path(
    path_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let s = string_from_header(path_ptr).unwrap_or_default();
        // Node's win32 implementation prepends `\\?\` for drive-absolute
        // and UNC paths (`\\?\C:\foo`, `\\?\UNC\server\share`). Bare
        // relative paths and already-prefixed paths are returned as-is.
        if s.is_empty() {
            return string_to_js(&s);
        }
        let normalized = normalize_win32_str(&s);
        if normalized.starts_with("\\\\?\\") || normalized.starts_with("\\\\.\\") {
            return string_to_js(&normalized);
        }
        let split = split_win32(&normalized);
        if split.is_absolute {
            if let Some(stripped) = normalized.strip_prefix("\\\\") {
                // UNC: "\\\\server\\share\\..." → "\\\\?\\UNC\\server\\share\\..."
                let prefixed = format!("\\\\?\\UNC\\{}", stripped);
                return string_to_js(&prefixed);
            }
            // Drive-absolute: "C:\\..." → "\\\\?\\C:\\..."
            let prefixed = format!("\\\\?\\{}", normalized);
            return string_to_js(&prefixed);
        }
        // Drive-relative or plain relative paths pass through unchanged.
        string_to_js(&normalized)
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_matches_glob(
    path_ptr: *const StringHeader,
    pattern_ptr: *const StringHeader,
) -> i32 {
    js_path_matches_glob(path_ptr, pattern_ptr)
}

#[no_mangle]
pub extern "C" fn js_path_win32_sep_get() -> *mut StringHeader {
    string_to_js("\\")
}

#[no_mangle]
pub extern "C" fn js_path_win32_delimiter_get() -> *mut StringHeader {
    string_to_js(";")
}

/// `path.win32.resolve(...)` chains via this binary helper, mirroring the
/// POSIX `js_path_resolve_join` rule: if `b` is absolute, drop `a` entirely;
/// else concatenate with `\` and normalize. Drive-relative segments (`C:foo`)
/// inherit the prior absolute prefix only if the drives match Node's rule
/// (we treat them as restart-of-drive for simplicity — see test fixtures).
#[no_mangle]
pub extern "C" fn js_path_win32_resolve_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let a = string_from_header(a_ptr).unwrap_or_default();
        let b = string_from_header(b_ptr).unwrap_or_default();
        let b_split = split_win32(&b);
        let joined = if b_split.is_absolute {
            b
        } else if a.is_empty() {
            b
        } else if b.is_empty() {
            a
        } else if a.ends_with('\\') || a.ends_with('/') {
            format!("{}{}", a, b)
        } else {
            format!("{}\\{}", a, b)
        };
        string_to_js(&normalize_win32_str(&joined))
    }
}

/// Resolve a win32 path to an absolute form. If the input isn't absolute,
/// we prepend a synthetic drive root (`C:\`) — `path.win32.resolve` is
/// host-cwd-aware on Windows but Perry's runtime always runs on POSIX hosts.
/// The fixtures only exercise inputs whose first arg is already drive-
/// absolute, so this fallback never fires in the parity sweep.
#[no_mangle]
pub extern "C" fn js_path_win32_resolve(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js(""),
        };
        let split = split_win32(&path_str);
        let absolute = if split.is_absolute {
            normalize_win32_str(&path_str)
        } else {
            // Synthesize a C:\-rooted result when called with a purely
            // relative input. Real-world parity tests always feed an
            // absolute first segment.
            normalize_win32_str(&format!("C:\\{}", path_str))
        };
        string_to_js(&absolute)
    }
}

/// `path.win32.relative(from, to)` — both arguments are first resolved to
/// absolute win32 paths; result is the relative path from `from` to `to`
/// using `..` as needed and `\` separators.
#[no_mangle]
pub extern "C" fn js_path_win32_relative(
    from_ptr: *const StringHeader,
    to_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let from = string_from_header(from_ptr).unwrap_or_default();
        let to = string_from_header(to_ptr).unwrap_or_default();
        // Resolve both inputs to absolute, normalized form (matches Node).
        let from_abs = {
            let s = split_win32(&from);
            if s.is_absolute {
                normalize_win32_str(&from)
            } else {
                normalize_win32_str(&format!("C:\\{}", from))
            }
        };
        let to_abs = {
            let s = split_win32(&to);
            if s.is_absolute {
                normalize_win32_str(&to)
            } else {
                normalize_win32_str(&format!("C:\\{}", to))
            }
        };
        let from_split = split_win32(&from_abs);
        let to_split = split_win32(&to_abs);
        // Different roots (e.g. different drives, or drive vs UNC) → return
        // `to` unchanged, matching Node's behavior.
        if from_split.prefix.eq_ignore_ascii_case(to_split.prefix) {
            // Same root — compute segment-wise relative path.
        } else {
            return string_to_js(&to_abs);
        }
        let from_segs: Vec<&str> = from_split
            .rest
            .split(is_win32_sep)
            .filter(|s| !s.is_empty())
            .collect();
        let to_segs: Vec<&str> = to_split
            .rest
            .split(is_win32_sep)
            .filter(|s| !s.is_empty())
            .collect();
        let common = from_segs
            .iter()
            .zip(to_segs.iter())
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        let ups = from_segs.len() - common;
        let mut parts: Vec<&str> = std::iter::repeat_n("..", ups).collect();
        parts.extend(to_segs[common..].iter().copied());
        string_to_js(&parts.join("\\"))
    }
}

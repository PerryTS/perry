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
        let from_norm = normalize_str(&from);
        let to_norm = normalize_str(&to);
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
        if !ext_str.is_empty() && base.ends_with(&ext_str) && base.len() > ext_str.len() {
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
    let dir = match p.parent() {
        Some(parent) => parent.to_string_lossy().to_string(),
        None => String::new(),
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

    // dir takes precedence over root; name+ext fallback when base missing
    let mut result = if !dir.is_empty() {
        let mut s = dir.clone();
        if !s.ends_with('/') {
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
        let name = get_str("name");
        let ext = get_str("ext");
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

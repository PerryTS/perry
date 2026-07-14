//! Path module - provides path manipulation utilities

use std::path::Path;

use crate::string::{js_string_from_bytes, js_string_materialize_to_heap, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if !is_string_header_ptr(ptr) {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn is_string_header_ptr(ptr: *const StringHeader) -> bool {
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    if ptr == crate::string::js_get_empty_string() {
        return true;
    }
    let gc_header =
        (ptr as *const u8).wrapping_sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    // A string outside the managed arenas (generation `Unknown`) is still a
    // real string when the GC malloc registry tracks its header — e.g. a
    // string grown through the append realloc path (`gc_malloc_realloc` →
    // `gc_malloc(_, GC_TYPE_STRING)`) or a Symbol description. Rejecting those
    // made every `path.*` builtin throw `The "path" argument must be of type
    // string.` on a perfectly valid argument. The registry lookup is
    // dereference-free, so forged pointers are still rejected before the
    // header reads below (mirrors the empty-string singleton special case
    // above and `current_heap_header_for_user_ptr`'s Unknown→malloc rule).
    if matches!(
        crate::arena::classify_heap_generation(ptr as usize),
        crate::arena::HeapGeneration::Unknown
    ) && !crate::gc::gc_malloc_header_is_tracked(gc_header)
    {
        return false;
    }
    unsafe {
        if (*gc_header).obj_type != crate::gc::GC_TYPE_STRING {
            return false;
        }
        let byte_len = (*ptr).byte_len;
        let capacity = (*ptr).capacity;
        byte_len <= capacity && capacity < 1_073_741_824
    }
}

/// Helper to create a JS string from a Rust string
fn string_to_js(s: &str) -> *mut StringHeader {
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn split_extension(base: &str) -> (String, String) {
    if base.is_empty() || base == "." || base == ".." {
        return (String::new(), base.to_string());
    }
    match base.rfind('.') {
        Some(0) | None => (String::new(), base.to_string()),
        Some(dot) => (base[dot..].to_string(), base[..dot].to_string()),
    }
}

fn parse_posix_components(path_str: &str) -> (String, String, String, String, String) {
    if path_str.is_empty() {
        return (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        );
    }

    let root = if path_str.starts_with('/') { "/" } else { "" }.to_string();
    let root_len = root.len();
    let bytes = path_str.as_bytes();
    let mut end = bytes.len();
    while end > root_len && bytes[end - 1] == b'/' {
        end -= 1;
    }
    if end == root_len && root_len > 0 {
        return (
            root.clone(),
            root,
            String::new(),
            String::new(),
            String::new(),
        );
    }

    let trimmed = &path_str[..end];
    let sep = trimmed.rfind('/');
    let base_start = sep.map_or(0, |idx| idx + 1);
    let base = trimmed[base_start..].to_string();
    let dir = match sep {
        Some(0) => "/".to_string(),
        Some(idx) => path_str[..idx].to_string(),
        None => String::new(),
    };
    let (ext, name) = split_extension(&base);
    (root, dir, base, ext, name)
}

pub(crate) fn throw_invalid_path_arg_type() -> ! {
    let msg = b"The \"path\" argument must be of type string.";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

fn string_from_header_or_throw(ptr: *const StringHeader) -> String {
    unsafe { string_from_header(ptr) }.unwrap_or_else(|| throw_invalid_path_arg_type())
}

fn optional_suffix_from_header_or_throw(ptr: *const StringHeader) -> String {
    let undefined_handle = (crate::value::TAG_UNDEFINED & crate::value::POINTER_MASK) as usize;
    if ptr as usize == undefined_handle {
        String::new()
    } else {
        string_from_header_or_throw(ptr)
    }
}

pub(crate) fn resolve_posix_str(path_str: &str) -> String {
    let mut resolved = if path_str.is_empty() {
        std::env::current_dir()
            .map(|cwd| cwd.to_string_lossy().to_string())
            .unwrap_or_default()
    } else if Path::new(path_str).is_absolute() {
        normalize_str(path_str)
    } else {
        match std::env::current_dir() {
            Ok(cwd) => normalize_str(&format!("{}/{}", cwd.to_string_lossy(), path_str)),
            Err(_) => normalize_str(path_str),
        }
    };
    while resolved.len() > 1 && resolved.ends_with('/') {
        resolved.pop();
    }
    resolved
}

/// Join two path segments. Node's `path.join` concatenates with `/` and
/// normalizes — it does NOT reset on an absolute segment (that's
/// `path.resolve`'s job). We can't use Rust's `Path::join` because it
/// resets on absolute segments.
pub(crate) fn js_path_join_unchecked(
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

#[no_mangle]
pub extern "C" fn js_path_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    let _ = string_from_header_or_throw(a_ptr);
    let _ = string_from_header_or_throw(b_ptr);
    js_path_join_unchecked(a_ptr, b_ptr)
}

/// `path.win32.join(a, b)` — Windows-style join. Always emits backslash
/// separators regardless of host platform. Treats both `/` and `\` as
/// segment separators in normalization (Node's win32 implementation does
/// the same) and collapses repeated separators.
pub(crate) fn js_path_win32_join_unchecked(
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

#[no_mangle]
pub extern "C" fn js_path_win32_join(
    a_ptr: *const StringHeader,
    b_ptr: *const StringHeader,
) -> *mut StringHeader {
    let _ = string_from_header_or_throw(a_ptr);
    let _ = string_from_header_or_throw(b_ptr);
    js_path_win32_join_unchecked(a_ptr, b_ptr)
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

fn is_win32_drive_prefix(prefix: &str) -> bool {
    let bytes = prefix.as_bytes();
    bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

/// Win32 normalization. Treats both `/` and `\` as separators (matching
/// Node), preserves a leading drive letter (`C:`), collapses repeated
/// separators, resolves `.` and `..`, and emits backslash separators.
fn normalize_win32_str(input: &str) -> String {
    if input.is_empty() {
        return ".".to_string();
    }
    // Node fast-path: a single char is either the root separator or itself.
    if input.len() == 1 {
        let c = input.as_bytes()[0] as char;
        return if is_win32_sep(c) {
            "\\".to_string()
        } else {
            input.to_string()
        };
    }

    let split = split_win32(input);
    let is_absolute = split.is_absolute;
    let rest = split.rest;
    // A non-empty prefix is Node's "device" — a drive (`C:`) or a
    // UNC/long-path root (`\\server\share`, `\\?\C:`).
    let has_device = !split.prefix.is_empty();
    let prefix_is_unc = split.prefix.starts_with('\\') || split.prefix.starts_with('/');
    let device: String = if prefix_is_unc {
        // Re-emit the UNC/device root with backslash separators.
        split
            .prefix
            .chars()
            .map(|c| if c == '/' { '\\' } else { c })
            .collect()
    } else {
        split.prefix.to_string()
    };

    // Collapse `.`/`..`/duplicate-separator segments in the remainder.
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
    let mut tail = out.join("\\");

    // #1728: an empty tail on a non-absolute ref becomes `.` — so `C:` → `C:.`
    // (the drive's cwd, not its root) and `.\` → `.`, never an empty string.
    if tail.is_empty() && !is_absolute {
        tail = ".".to_string();
    }
    // #1728: preserve a trailing separator the input carried, matching Node
    // (`.\` → `.\`, `C:\foo\` → `C:\foo\`). Only meaningful with a non-empty tail.
    let input_trailing_sep = input.chars().next_back().is_some_and(is_win32_sep);
    if !tail.is_empty() && input_trailing_sep {
        tail.push('\\');
    }

    // Assemble device + root separator + tail (mirrors Node's win32 normalize).
    if !has_device {
        if is_absolute {
            return if tail.is_empty() {
                "\\".to_string()
            } else {
                format!("\\{}", tail)
            };
        }
        return tail;
    }
    if is_absolute {
        // Drive-absolute (`C:\...`) or UNC/device root (`\\server\share`,
        // `\\?\C:`): a bare root still keeps its trailing separator (#1728).
        return if tail.is_empty() {
            format!("{}\\", device)
        } else {
            format!("{}\\{}", device, tail)
        };
    }
    // Drive-relative (`C:foo`, `C:.`) — no separator between device and tail.
    format!("{}{}", device, tail)
}

fn posix_cwd_as_win32_path() -> String {
    std::env::current_dir()
        .map(|cwd| cwd.to_string_lossy().replace('/', "\\"))
        .unwrap_or_else(|_| "\\".to_string())
}

fn join_win32_paths(base: &str, tail: &str) -> String {
    if tail.is_empty() {
        base.to_string()
    } else if base.ends_with('\\') || base.ends_with('/') {
        format!("{}{}", base, tail)
    } else {
        format!("{}\\{}", base, tail)
    }
}

fn win32_resolve_inner(path_str: &str) -> String {
    let split = split_win32(path_str);
    if split.is_absolute {
        return normalize_win32_str(path_str);
    }

    let cwd = posix_cwd_as_win32_path();
    let path = if split.prefix.is_empty() {
        join_win32_paths(&cwd, path_str)
    } else {
        let drive_cwd = format!("{}{}", split.prefix, cwd);
        join_win32_paths(&drive_cwd, split.rest)
    };
    normalize_win32_str(&path)
}

pub(crate) fn resolve_win32_str(path_str: &str) -> String {
    win32_resolve_inner(path_str)
}

/// Get directory name from path — Node's `path.posix.dirname`, which is purely
/// lexical. The previous implementation delegated to Rust's `Path::parent()`,
/// whose OS path semantics normalize components away and disagree with Node
/// (`dirname("a/./b")` → `"a"` instead of `"a/."`, `dirname("a//b")` → `"a"`
/// instead of `"a/"`, `dirname("/.")` → `"."` instead of `"/"`). The root
/// (`"/"` → `"/"`, `"///"` → `"/"`) and the two-leading-slash case
/// (`"//foo"` → `"//"`) both fall out of the algorithm.
#[no_mangle]
pub extern "C" fn js_path_dirname(path_ptr: *const StringHeader) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);
    string_to_js(&posix_dirname_inner(&path_str))
}

/// Get base name (file name) from path
#[no_mangle]
pub extern "C" fn js_path_basename(path_ptr: *const StringHeader) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);
    string_to_js(posix_basename_inner(&path_str))
}

/// Get file extension from path (including the dot)
#[no_mangle]
pub extern "C" fn js_path_extname(path_ptr: *const StringHeader) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);

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

/// Check if path is absolute
#[no_mangle]
pub extern "C" fn js_path_is_absolute(path_ptr: *const StringHeader) -> i32 {
    let path_str = string_from_header_or_throw(path_ptr);
    if Path::new(&path_str).is_absolute() {
        1
    } else {
        0
    }
}

/// Resolve path to absolute path
#[no_mangle]
pub extern "C" fn js_path_resolve(path_ptr: *const StringHeader) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);
    string_to_js(&resolve_posix_str(&path_str))
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
        // Node keeps the trailing separator even when everything normalized away:
        // `normalize("./") === "./"` (only a bare `normalize("") === "."`).
        return if trailing_slash {
            "./".to_string()
        } else {
            ".".to_string()
        };
    }
    if trailing_slash && !result.ends_with('/') {
        result.push('/');
    }
    result
}

/// Node's `path.posix.basename` is a purely LEXICAL string operation: the last
/// non-empty `/`-delimited segment, with `.` and `..` treated as ordinary segment
/// names (`basename(".") === "."`, `basename("a/..") === ".."`).
///
/// Rust's `Path::file_name()` applies OS path semantics instead and cannot be used
/// here: it returns `None` for `.`, `..`, and any path whose last component is
/// `..` (so those yielded `""`), and it silently DROPS a `.` component —
/// `Path::new("a/.").file_name()` is `Some("a")`, which returned the PARENT.
/// Mirrors [`win32_basename_inner`], which was already lexical (and correct).
fn posix_basename_inner(input: &str) -> &str {
    input.split('/').rfind(|s| !s.is_empty()).unwrap_or("")
}

/// Node's `path.posix.basename(path, ext)` — ported from `lib/path.js`, QUIRKS
/// INCLUDED. The ext branch is deliberately not "lexical basename minus the
/// suffix": Node scans backwards matching `ext` byte-by-byte and, when the match
/// is abandoned or the component start is reached, falls back to indices that can
/// span trailing separators. The observable consequence is
/// `basename("/x/", ".x") === "x/"` — trailing slash and all. A simplified
/// "strip the suffix" version disagrees with Node here.
fn posix_basename_ext_inner(path: &str, ext: &str) -> String {
    let bytes = path.as_bytes();
    let ext_b = ext.as_bytes();
    if ext_b.is_empty() || ext_b.len() > bytes.len() {
        return posix_basename_inner(path).to_string();
    }
    if ext == path {
        return String::new();
    }

    let mut start: usize = 0;
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut ext_idx: isize = ext_b.len() as isize - 1;
    let mut first_non_slash_end: isize = -1;

    for i in (0..bytes.len()).rev() {
        let code = bytes[i];
        if code == b'/' {
            // Only a separator that is NOT part of a trailing run ends the scan.
            if !matched_slash {
                start = i + 1;
                break;
            }
        } else {
            if first_non_slash_end == -1 {
                matched_slash = false;
                first_non_slash_end = (i + 1) as isize;
            }
            if ext_idx >= 0 {
                if code == ext_b[ext_idx as usize] {
                    ext_idx -= 1;
                    if ext_idx == -1 {
                        end = i as isize;
                    }
                } else {
                    // Suffix didn't match — the result is the whole component.
                    ext_idx = -1;
                    end = first_non_slash_end;
                }
            }
        }
    }

    if start as isize == end {
        end = first_non_slash_end;
    } else if end == -1 {
        end = bytes.len() as isize;
    }
    let end = end.max(0) as usize;
    // Byte indices land on char boundaries for well-formed input, but never risk a
    // panic on an odd UTF-8 split: fall back to the plain lexical basename.
    if start > end || !path.is_char_boundary(start) || !path.is_char_boundary(end) {
        return posix_basename_inner(path).to_string();
    }
    path[start..end].to_string()
}

/// Node's `path.posix.dirname`, ported from `lib/path.js`. Also purely lexical —
/// `dirname("a/./b") === "a/."` and `dirname("a//b") === "a/"`, neither of which
/// Rust's `Path::parent()` produces (it normalizes the `.` and the empty segment
/// away, giving `"a"` for both).
fn posix_dirname_inner(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let bytes = path.as_bytes();
    let has_root = bytes[0] == b'/';
    let mut end: Option<usize> = None;
    let mut matched_slash = true;
    // Scan back to index 1 (never 0 — a leading `/` is the root, not a separator).
    for i in (1..bytes.len()).rev() {
        if bytes[i] == b'/' {
            if !matched_slash {
                end = Some(i);
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    match end {
        None => {
            if has_root {
                "/".to_string()
            } else {
                ".".to_string()
            }
        }
        // Node preserves exactly two leading slashes: `dirname("//foo") === "//"`.
        Some(1) if has_root => "//".to_string(),
        // `end` is the byte index of an ASCII `/`, so this is a char boundary.
        Some(end) => path[..end].to_string(),
    }
}

#[no_mangle]
pub extern "C" fn js_path_normalize(path_ptr: *const StringHeader) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);
    string_to_js(&normalize_str(&path_str))
}

/// Validate that a `path.relative` operand is a string, materializing it to a
/// heap `StringHeader`. Throws `TypeError [ERR_INVALID_ARG_TYPE]` naming the
/// offending argument (`from` / `to`) for non-string values, matching Node.
fn require_relative_arg(value: f64, arg_name: &str) -> *const StringHeader {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_any_string() {
        let ptr = js_string_materialize_to_heap(value);
        if !ptr.is_null() {
            return ptr;
        }
    }
    let message = format!(
        "The \"{}\" argument must be of type string. Received {}",
        arg_name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

/// Validating entry point for the compiled `path.relative(from, to)` fast path.
/// Both operands arrive NaN-boxed so their type can be checked before the
/// underlying string-only helper is invoked (#2995).
#[no_mangle]
pub extern "C" fn js_path_relative_checked(from_f64: f64, to_f64: f64) -> *mut StringHeader {
    let from = require_relative_arg(from_f64, "from");
    let to = require_relative_arg(to_f64, "to");
    js_path_relative(from, to)
}

/// `path.win32.relative(from, to)` validating entry point — see
/// [`js_path_relative_checked`].
#[no_mangle]
pub extern "C" fn js_path_win32_relative_checked(from_f64: f64, to_f64: f64) -> *mut StringHeader {
    let from = require_relative_arg(from_f64, "from");
    let to = require_relative_arg(to_f64, "to");
    js_path_win32_relative(from, to)
}

/// Keepalive anchors: these are emitted only from generated code, so the
/// whole-program auto-optimize bitcode pass would otherwise dead-strip them.
#[used]
static KEEP_PATH_RELATIVE_CHECKED: extern "C" fn(f64, f64) -> *mut StringHeader =
    js_path_relative_checked;
#[used]
static KEEP_PATH_WIN32_RELATIVE_CHECKED: extern "C" fn(f64, f64) -> *mut StringHeader =
    js_path_win32_relative_checked;

#[no_mangle]
pub extern "C" fn js_path_relative(
    from_ptr: *const StringHeader,
    to_ptr: *const StringHeader,
) -> *mut StringHeader {
    let from = string_from_header_or_throw(from_ptr);
    let to = string_from_header_or_throw(to_ptr);
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

#[no_mangle]
pub extern "C" fn js_path_basename_ext(
    path_ptr: *const StringHeader,
    ext_ptr: *const StringHeader,
) -> *mut StringHeader {
    let path_str = string_from_header_or_throw(path_ptr);
    let ext_str = optional_suffix_from_header_or_throw(ext_ptr);
    if !ext_str.is_empty() && ext_str == path_str {
        return string_to_js("");
    }
    string_to_js(&posix_basename_ext_inner(&path_str, &ext_str))
}

/// Returns a `{ root, dir, base, ext, name }` object describing the path.
#[no_mangle]
pub extern "C" fn js_path_parse(path_ptr: *const StringHeader) -> *mut crate::object::ObjectHeader {
    use crate::object::{js_object_alloc_with_shape, js_object_set_field};
    use crate::value::JSValue;

    let path_str = unsafe { string_from_header(path_ptr) }.unwrap_or_default();
    let (root, dir, base, ext, name) = parse_posix_components(&path_str);

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

/// Throw `TypeError [ERR_INVALID_ARG_TYPE]` for a `path.format` descriptor that
/// is not an object (Node validates the top-level `pathObject` argument).
fn throw_invalid_path_object(value: f64) -> ! {
    let message = format!(
        "The \"pathObject\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

/// A single descriptor field read from a `path.format` argument: its
/// Node-`ToString`-coerced text and whether the raw value is truthy.
/// Node's `_format` uses `pathObject.field || ''`, so falsy fields (including
/// `0`, `false`, `null`, `""`) contribute nothing.
struct FormatField {
    coerced: String,
    truthy: bool,
}

/// Read a descriptor field by name and coerce it the way Node does (template
/// literal / `||` semantics): truthy values are stringified via the standard
/// `ToString`, falsy values become empty.
fn read_format_field(obj_ptr: *mut crate::object::ObjectHeader, name: &str) -> FormatField {
    use crate::object::js_object_get_field_by_name;
    let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let val = js_object_get_field_by_name(obj_ptr, key_ptr);
    let raw = f64::from_bits(val.bits());
    if crate::value::js_is_truthy(raw) == 0 {
        return FormatField {
            coerced: String::new(),
            truthy: false,
        };
    }
    let s_ptr = crate::value::js_jsvalue_to_string(raw);
    let coerced = unsafe { string_from_header(s_ptr) }.unwrap_or_default();
    FormatField {
        coerced,
        truthy: true,
    }
}

/// Shared `path.format` implementation for both posix (`sep = '/'`) and win32
/// (`sep = '\\'`). Mirrors Node's `_format`:
/// `dir = pathObject.dir || pathObject.root; base = pathObject.base ||
/// `${name||''}${formatExt(ext)}`; if (!dir) return base; return dir ===
/// pathObject.root ? dir+base : dir+sep+base;`
fn format_descriptor(obj_f64: f64, sep: char) -> String {
    use crate::value::js_nanbox_get_pointer;

    let jv = crate::value::JSValue::from_bits(obj_f64.to_bits());
    // Node: typeof pathObject !== 'object' || pathObject === null throws.
    // Objects and arrays are POINTER_TAG values; strings/numbers/bool/null/
    // undefined are not.
    if !jv.is_pointer() {
        throw_invalid_path_object(obj_f64);
    }
    let obj_ptr = js_nanbox_get_pointer(obj_f64) as *mut crate::object::ObjectHeader;
    if obj_ptr.is_null() {
        throw_invalid_path_object(obj_f64);
    }

    let dir_f = read_format_field(obj_ptr, "dir");
    let root_f = read_format_field(obj_ptr, "root");
    let base_f = read_format_field(obj_ptr, "base");
    let name_f = read_format_field(obj_ptr, "name");
    let ext_f = read_format_field(obj_ptr, "ext");

    // formatExt: ensure a leading dot when ext is truthy.
    let ext = if ext_f.truthy && !ext_f.coerced.starts_with('.') {
        format!(".{}", ext_f.coerced)
    } else {
        ext_f.coerced.clone()
    };

    // base = pathObject.base || `${name||''}${formatExt(ext)}`
    let base = if base_f.truthy {
        base_f.coerced.clone()
    } else {
        format!("{}{}", name_f.coerced, ext)
    };

    // dir = pathObject.dir || pathObject.root
    let (dir, dir_from_root) = if dir_f.truthy {
        (dir_f.coerced.clone(), false)
    } else {
        (root_f.coerced.clone(), true)
    };

    if dir.is_empty() {
        return base;
    }

    // dir === pathObject.root ? dir+base : dir+sep+base. dir equals root when
    // it fell through to root, or when the (string) dir and root values match.
    let no_sep = dir_from_root || (dir_f.truthy && root_f.truthy && dir == root_f.coerced);
    if no_sep {
        format!("{dir}{base}")
    } else {
        format!("{dir}{sep}{base}")
    }
}

/// Build a path from a `{ dir, base, root, name, ext }` descriptor.
#[no_mangle]
pub extern "C" fn js_path_format(obj_f64: f64) -> *mut StringHeader {
    string_to_js(&format_descriptor(obj_f64, '/'))
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
    let a = string_from_header_or_throw(a_ptr);
    let b = string_from_header_or_throw(b_ptr);

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

fn string_value_to_namespaced_path(value: f64, win32: bool) -> f64 {
    let path_ptr = js_string_materialize_to_heap(value);
    if path_ptr.is_null() {
        return value;
    }

    let Some(path) = (unsafe { string_from_header(path_ptr as *const StringHeader) }) else {
        return value;
    };
    let result = if win32 {
        win32_to_namespaced_path(&path)
    } else {
        path
    };
    f64::from_bits(crate::value::JSValue::string_ptr(string_to_js(&result)).bits())
}

#[no_mangle]
pub extern "C" fn js_path_to_namespaced_path_value(value: f64) -> f64 {
    string_value_to_namespaced_path(value, false)
}

#[cfg(feature = "regex-engine")]
fn brace_alternation(pattern: &str, open: usize) -> Option<(usize, Vec<&str>)> {
    let bytes = pattern.as_bytes();
    let mut depth = 0usize;
    let mut arm_start = open + 1;
    let mut arms = Vec::new();
    let mut saw_comma = false;
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] as char {
            '{' => depth += 1,
            '}' if depth == 0 => {
                if !saw_comma {
                    return None;
                }
                arms.push(&pattern[arm_start..i]);
                return Some((i, arms));
            }
            '}' => depth -= 1,
            ',' if depth == 0 => {
                saw_comma = true;
                arms.push(&pattern[arm_start..i]);
                arm_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(feature = "regex-engine")]
fn extglob_alternation(pattern: &str, open: usize) -> Option<(usize, char, Vec<&str>)> {
    let bytes = pattern.as_bytes();
    if open + 1 >= bytes.len() || bytes[open + 1] != b'(' {
        return None;
    }
    let op = bytes[open] as char;
    if !matches!(op, '@' | '+' | '?' | '*') {
        return None;
    }

    let mut depth = 0usize;
    let mut arm_start = open + 2;
    let mut arms = Vec::new();
    let mut i = open + 2;
    while i < bytes.len() {
        match bytes[i] as char {
            '[' => {
                i += 1;
                while i < bytes.len() && bytes[i] as char != ']' {
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' if depth == 0 => {
                arms.push(&pattern[arm_start..i]);
                return Some((i, op, arms));
            }
            ')' => depth -= 1,
            '|' if depth == 0 => {
                arms.push(&pattern[arm_start..i]);
                arm_start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(feature = "regex-engine")]
fn push_regex_literal(c: char, out: &mut String) {
    match c {
        '.' | '+' | '(' | ')' | '|' | '^' | '$' | '}' | '\\' => {
            out.push('\\');
            out.push(c);
        }
        _ => out.push(c),
    }
}

#[cfg(feature = "regex-engine")]
fn push_glob_regex(pattern: &str, out: &mut String) {
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if matches!(c, '@' | '+' | '?' | '*') && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            if let Some((close, op, arms)) = extglob_alternation(pattern, i) {
                out.push_str("(?:");
                for (idx, arm) in arms.iter().enumerate() {
                    if idx > 0 {
                        out.push('|');
                    }
                    push_glob_regex(arm, out);
                }
                out.push(')');
                match op {
                    '+' => out.push('+'),
                    '?' => out.push('?'),
                    '*' => out.push('*'),
                    _ => {}
                }
                i = close + 1;
                continue;
            }
        }

        match c {
            '*' => {
                if i + 1 < bytes.len() && bytes[i + 1] as char == '*' {
                    let after = i + 2;
                    let segment_start = i == 0 || bytes[i - 1] == b'/';
                    let segment_end = after == bytes.len() || bytes[after] == b'/';
                    if segment_start && segment_end {
                        if after < bytes.len() && bytes[after] == b'/' {
                            out.push_str("(?:[^/]+/)*");
                            i = after + 1;
                        } else {
                            out.push_str(".*");
                            i = after;
                        }
                    } else {
                        out.push_str("[^/]*");
                        i += 2;
                    }
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
            '{' => {
                if let Some((close, arms)) = brace_alternation(pattern, i) {
                    out.push_str("(?:");
                    for (idx, arm) in arms.iter().enumerate() {
                        if idx > 0 {
                            out.push('|');
                        }
                        push_glob_regex(arm, out);
                    }
                    out.push(')');
                    i = close;
                } else {
                    out.push('\\');
                    out.push(c);
                }
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '}' | '\\' => push_regex_literal(c, out),
            _ => push_regex_literal(c, out),
        }
        i += 1;
    }
}

/// Convert a glob pattern (`*`, `?`, `[abc]`, `{a,b}`, `**`, positive
/// extglobs) into a regex anchored at both ends. Node's path matcher uses
/// minimatch with `windowsPathsNoEscape`, so backslashes in the pattern are
/// path separators, not escapes. `**` is a globstar only as a whole path
/// segment; embedded `**` has ordinary `*` segment-wildcard behavior.
#[cfg(feature = "regex-engine")]
fn glob_to_regex(pattern: &str) -> String {
    let mut out = String::from("^");
    let normalized = pattern.replace('\\', "/");
    push_glob_regex(&normalized, &mut out);
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
    #[cfg(feature = "regex-engine")]
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
    // Glob matching is built on the regex engine; with it gated off, report
    // "no match" (a program that calls `path.matchesGlob` forces the engine on).
    #[cfg(not(feature = "regex-engine"))]
    {
        let _ = (path_ptr, pattern_ptr);
        0
    }
}

// ===================================================================
// Win32 sub-namespace (issue #1162)
// ===================================================================

/// Last segment of a win32 path, matching Node's `win32.basename`: skip a
/// leading drive letter, then take the final non-empty separator-delimited
/// segment (`\` or `/`). UNC server/share segments are ordinary segments, so
/// `\\server\share\` yields `share`.
fn win32_basename_inner(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    // Node skips a leading drive letter (`C:`) so its colon/separator isn't
    // mistaken for a path separator, then takes the last non-empty segment.
    let bytes = input.as_bytes();
    let scan = if bytes.len() >= 2 && (bytes[0] as char).is_ascii_alphabetic() && bytes[1] == b':' {
        &input[2..]
    } else {
        input
    };
    // #1728: UNC server/share segments count as ordinary segments here, so
    // `\\server\share\` → `share` rather than the old root-stripped empty.
    scan.split(is_win32_sep)
        .rfind(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
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
    let path_str = string_from_header_or_throw(path_ptr);
    let ext_str = optional_suffix_from_header_or_throw(ext_ptr);
    if !ext_str.is_empty() && ext_str == path_str {
        return string_to_js("");
    }
    let base = win32_basename_inner(&path_str);
    if !ext_str.is_empty() && base.ends_with(&ext_str) && base.len() > ext_str.len() {
        string_to_js(&base[..base.len() - ext_str.len()])
    } else {
        string_to_js(&base)
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_dirname(path_ptr: *const StringHeader) -> *mut StringHeader {
    unsafe {
        let path_str = match string_from_header(path_ptr) {
            Some(s) => s,
            None => return string_to_js("."),
        };
        string_to_js(&win32_dirname_inner(&path_str))
    }
}

fn win32_dirname_inner(path_str: &str) -> String {
    let bytes = path_str.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return ".".to_string();
    }
    if len == 1 {
        return if is_win32_sep(bytes[0] as char) {
            path_str.to_string()
        } else {
            ".".to_string()
        };
    }

    let mut root_end: Option<usize> = None;
    let mut offset = 0usize;

    if is_win32_sep(bytes[0] as char) {
        root_end = Some(1);
        offset = 1;
        if is_win32_sep(bytes[1] as char) {
            let mut j = 2usize;
            let mut last = j;
            while j < len && !is_win32_sep(bytes[j] as char) {
                j += 1;
            }
            if j < len && j != last {
                last = j;
                while j < len && is_win32_sep(bytes[j] as char) {
                    j += 1;
                }
                if j < len && j != last {
                    last = j;
                    while j < len && !is_win32_sep(bytes[j] as char) {
                        j += 1;
                    }
                    if j == len {
                        return path_str.to_string();
                    }
                    if j != last {
                        root_end = Some(j + 1);
                        offset = j + 1;
                    }
                }
            }
        }
    } else if len >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        let end = if len > 2 && is_win32_sep(bytes[2] as char) {
            3
        } else {
            2
        };
        root_end = Some(end);
        offset = end;
    }

    let mut end = None;
    let mut matched_slash = true;
    for i in (offset..len).rev() {
        if is_win32_sep(bytes[i] as char) {
            if !matched_slash {
                end = Some(i);
                break;
            }
        } else {
            matched_slash = false;
        }
    }

    match end.or(root_end) {
        Some(end) => path_str[..end].to_string(),
        None => ".".to_string(),
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
        let (ext, _) = split_extension(&base);
        string_to_js(&ext)
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
        // Pop trailing separator from dir unless dir IS the root.
        if d.ends_with('\\') && d != root {
            d.pop();
        }
        (base, d)
    };

    let (ext, name) = split_extension(&base);

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
    string_to_js(&format_descriptor(obj_f64, '\\'))
}

fn current_dir_as_win32() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|cwd| normalize_win32_str(&cwd.to_string_lossy()))
}

fn resolve_win32_for_namespace(path_str: &str) -> String {
    let normalized = normalize_win32_str(path_str);
    let split = split_win32(&normalized);
    if split.is_absolute {
        return normalized;
    }

    if !split.prefix.is_empty() {
        let cwd = current_dir_as_win32().unwrap_or_else(|| "\\".to_string());
        let cwd_tail = cwd.trim_start_matches('\\');
        if split.rest.is_empty() || split.rest == "." {
            return normalize_win32_str(&format!("{}\\{}", split.prefix, cwd_tail));
        }
        return normalize_win32_str(&format!("{}\\{}\\{}", split.prefix, cwd_tail, split.rest));
    }

    let cwd = current_dir_as_win32().unwrap_or_default();
    if cwd.is_empty() {
        normalized
    } else if cwd.ends_with('\\') {
        normalize_win32_str(&format!("{}{}", cwd, normalized))
    } else {
        normalize_win32_str(&format!("{}\\{}", cwd, normalized))
    }
}

fn win32_to_namespaced_path(path_str: &str) -> String {
    if path_str.is_empty() {
        return String::new();
    }
    let normalized = normalize_win32_str(path_str);
    if normalized.starts_with("\\\\?\\") || normalized.starts_with("\\\\.\\") {
        return normalized;
    }

    let resolved = resolve_win32_for_namespace(path_str);
    if resolved.len() <= 2 {
        return path_str.to_string();
    }
    if let Some(stripped) = resolved.strip_prefix("\\\\") {
        let third = stripped.as_bytes().first().copied();
        if third != Some(b'?') && third != Some(b'.') {
            return format!("\\\\?\\UNC\\{}", stripped);
        }
    }

    let split = split_win32(&resolved);
    if split.is_absolute
        && split.prefix.len() == 2
        && split.prefix.as_bytes()[1] == b':'
        && split.prefix.as_bytes()[0].is_ascii_alphabetic()
    {
        return format!("\\\\?\\{}", resolved);
    }

    resolved
}

#[no_mangle]
pub extern "C" fn js_path_win32_to_namespaced_path(
    path_ptr: *const StringHeader,
) -> *mut StringHeader {
    unsafe {
        let s = string_from_header(path_ptr).unwrap_or_default();
        string_to_js(&win32_to_namespaced_path(&s))
    }
}

#[no_mangle]
pub extern "C" fn js_path_win32_to_namespaced_path_value(value: f64) -> f64 {
    string_value_to_namespaced_path(value, true)
}

#[no_mangle]
pub extern "C" fn js_path_win32_matches_glob(
    path_ptr: *const StringHeader,
    pattern_ptr: *const StringHeader,
) -> i32 {
    #[cfg(feature = "regex-engine")]
    unsafe {
        let path_str = string_from_header(path_ptr)
            .unwrap_or_default()
            .replace('\\', "/");
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
    #[cfg(not(feature = "regex-engine"))]
    {
        let _ = (path_ptr, pattern_ptr);
        0
    }
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
    let a = string_from_header_or_throw(a_ptr);
    let b = string_from_header_or_throw(b_ptr);
    let b_split = split_win32(&b);
    let joined = if b_split.is_absolute {
        b.clone()
    } else if b.is_empty() {
        a.clone()
    } else if is_win32_drive_prefix(b_split.prefix) {
        let a_split = split_win32(&a);
        let same_drive = a_split.prefix.eq_ignore_ascii_case(b_split.prefix);
        if a.is_empty() {
            b.clone()
        } else if a_split.is_absolute && (a_split.prefix.is_empty() || same_drive) {
            let base = if a_split.prefix.is_empty() {
                format!("{}{}", b_split.prefix, a_split.rest)
            } else {
                a.clone()
            };
            if b_split.rest.is_empty() || b_split.rest == "." {
                base
            } else {
                join_win32_paths(&base, b_split.rest)
            }
        } else if !a_split.is_absolute && (a_split.prefix.is_empty() || same_drive) {
            let base = if a_split.prefix.is_empty() {
                format!("{}{}", b_split.prefix, a)
            } else {
                a.clone()
            };
            if b_split.rest.is_empty() || b_split.rest == "." {
                base
            } else {
                join_win32_paths(&base, b_split.rest)
            }
        } else {
            b.clone()
        }
    } else if a.is_empty() {
        b.clone()
    } else if a.ends_with('\\') || a.ends_with('/') {
        format!("{}{}", a, b)
    } else {
        format!("{}\\{}", a, b)
    };
    string_to_js(&normalize_win32_str(&joined))
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
        string_to_js(&win32_resolve_inner(&path_str))
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
    let from = string_from_header_or_throw(from_ptr);
    let to = string_from_header_or_throw(to_ptr);
    let from_abs = win32_resolve_inner(&from);
    let to_abs = win32_resolve_inner(&to);
    let from_split = split_win32(&from_abs);
    let to_split = split_win32(&to_abs);
    // Different roots (e.g. different drives, or drive vs UNC) → return
    // `to` unchanged, matching Node's behavior.
    if !from_split.prefix.eq_ignore_ascii_case(to_split.prefix) {
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

#[cfg(test)]
mod posix_parse_tests {
    use super::parse_posix_components;

    fn parse(path: &str) -> (String, String, String, String, String) {
        parse_posix_components(path)
    }

    #[test]
    fn final_dot_segments_are_literal_base_names() {
        assert_eq!(
            parse("/tmp/."),
            (
                "/".to_string(),
                "/tmp".to_string(),
                ".".to_string(),
                String::new(),
                ".".to_string()
            )
        );
        assert_eq!(
            parse("/tmp/.."),
            (
                "/".to_string(),
                "/tmp".to_string(),
                "..".to_string(),
                String::new(),
                "..".to_string()
            )
        );
    }

    #[test]
    fn trailing_separators_are_ignored_without_normalizing() {
        assert_eq!(
            parse("/foo//bar//"),
            (
                "/".to_string(),
                "/foo/".to_string(),
                "bar".to_string(),
                String::new(),
                "bar".to_string()
            )
        );
        assert_eq!(
            parse("foo//"),
            (
                String::new(),
                String::new(),
                "foo".to_string(),
                String::new(),
                "foo".to_string()
            )
        );
    }

    #[test]
    fn dotfile_extension_rules_match_node() {
        assert_eq!(
            parse("/.bashrc"),
            (
                "/".to_string(),
                "/".to_string(),
                ".bashrc".to_string(),
                String::new(),
                ".bashrc".to_string()
            )
        );
        assert_eq!(
            parse(".profile.js"),
            (
                String::new(),
                String::new(),
                ".profile.js".to_string(),
                ".js".to_string(),
                ".profile".to_string()
            )
        );
    }
}

#[cfg(all(test, feature = "regex-engine"))]
mod glob_tests {
    use super::glob_to_regex;

    #[test]
    fn brace_alternation_expands_to_group() {
        assert_eq!(glob_to_regex("*.{md,txt}"), "^[^/]*\\.(?:md|txt)$");
        assert_eq!(
            glob_to_regex("src/{app,test}.ts"),
            "^src/(?:app|test)\\.ts$"
        );
    }

    #[test]
    fn braces_without_alternation_stay_literal() {
        assert_eq!(glob_to_regex("file.{md}"), "^file\\.\\{md\\}$");
    }

    #[test]
    fn extglob_positive_groups_expand() {
        assert_eq!(glob_to_regex("*.@(js|ts)"), "^[^/]*\\.(?:js|ts)$");
        assert_eq!(glob_to_regex("*.+(js|ts)"), "^[^/]*\\.(?:js|ts)+$");
        assert_eq!(glob_to_regex("*.?(js|ts)"), "^[^/]*\\.(?:js|ts)?$");
    }

    #[test]
    fn globstar_is_segment_aware() {
        assert_eq!(glob_to_regex("a/**/c"), "^a/(?:[^/]+/)*c$");
        assert_eq!(glob_to_regex("a/**"), "^a/.*$");
        assert_eq!(glob_to_regex("a**b"), "^a[^/]*b$");
    }

    #[test]
    fn pattern_backslashes_are_separators() {
        assert_eq!(glob_to_regex("foo\\*"), "^foo/[^/]*$");
    }
}

#[cfg(test)]
mod win32_normalize_tests {
    use super::{
        current_dir_as_win32, join_win32_paths, normalize_win32_str, posix_cwd_as_win32_path,
        win32_basename_inner, win32_dirname_inner, win32_resolve_inner, win32_to_namespaced_path,
    };

    #[test]
    fn drive_relative_bare_appends_dot() {
        // #1728: a bare drive ref is the drive's *current dir*, not the root.
        assert_eq!(normalize_win32_str("C:"), "C:.");
        assert_eq!(normalize_win32_str("c:"), "c:.");
    }

    #[test]
    fn trailing_separator_preserved() {
        // #1728: a trailing separator the input carried is kept.
        assert_eq!(normalize_win32_str(".\\"), ".\\");
        assert_eq!(normalize_win32_str("C:\\foo\\"), "C:\\foo\\");
        assert_eq!(normalize_win32_str("\\\\?\\C:\\"), "\\\\?\\C:\\");
    }

    #[test]
    fn unc_root_keeps_trailing_separator() {
        // #1728: a bare UNC/device root normalizes with a trailing separator.
        assert_eq!(
            normalize_win32_str("\\\\server\\share"),
            "\\\\server\\share\\"
        );
        assert_eq!(
            normalize_win32_str("\\\\server\\share\\"),
            "\\\\server\\share\\"
        );
        // Content after the root is unaffected (no spurious trailing sep).
        assert_eq!(
            normalize_win32_str("\\\\server\\share\\foo\\..\\bar"),
            "\\\\server\\share\\bar"
        );
        assert_eq!(
            normalize_win32_str("//server/share/a/b"),
            "\\\\server\\share\\a\\b"
        );
    }

    #[test]
    fn basename_handles_unc_root_and_drive() {
        // #1728: win32.basename of a UNC root is the share segment.
        assert_eq!(win32_basename_inner("\\\\server\\share\\"), "share");
        assert_eq!(win32_basename_inner("\\\\server\\share\\file"), "file");
        assert_eq!(win32_basename_inner("C:\\foo\\bar\\baz.txt"), "baz.txt");
        assert_eq!(win32_basename_inner("C:foo"), "foo");
    }

    #[test]
    fn dirname_preserves_input_separator_style() {
        assert_eq!(win32_dirname_inner("/foo/bar"), "/foo");
        assert_eq!(win32_dirname_inner("/foo/bar/"), "/foo");
        assert_eq!(win32_dirname_inner("foo/bar/baz"), "foo/bar");
        assert_eq!(win32_dirname_inner("C:/foo/bar"), "C:/foo");
        assert_eq!(win32_dirname_inner("//server/share"), "//server/share");
        assert_eq!(win32_dirname_inner("//server/share/a"), "//server/share/");
    }

    #[test]
    fn drive_relative_with_segments_unchanged() {
        // The `.` is only appended when there are no segments.
        assert_eq!(normalize_win32_str("C:foo"), "C:foo");
        assert_eq!(normalize_win32_str("C:.."), "C:..");
        assert_eq!(normalize_win32_str("C:foo\\bar"), "C:foo\\bar");
    }

    #[test]
    fn drive_absolute_and_others_unaffected() {
        // Regression guard for the cases that already matched Node.
        assert_eq!(normalize_win32_str("C:\\"), "C:\\");
        assert_eq!(normalize_win32_str("C:\\foo"), "C:\\foo");
        assert_eq!(normalize_win32_str("a//b//../b"), "a\\b");
        assert_eq!(normalize_win32_str("/foo/../../../bar"), "\\bar");
        assert_eq!(normalize_win32_str(""), ".");
    }

    #[test]
    fn resolve_drive_relative_uses_posix_cwd_as_drive_cwd() {
        let cwd = posix_cwd_as_win32_path();
        let drive_cwd = format!("C:{}", cwd);
        assert_eq!(
            win32_resolve_inner("C:foo"),
            normalize_win32_str(&join_win32_paths(&drive_cwd, "foo"))
        );
        assert_ne!(win32_resolve_inner("C:foo"), "C:\\C:foo");
        assert_eq!(
            win32_resolve_inner("foo"),
            normalize_win32_str(&join_win32_paths(&cwd, "foo"))
        );
    }

    #[test]
    fn to_namespaced_path_resolves_but_only_namespaces_drive_and_unc() {
        let cwd = current_dir_as_win32().unwrap();
        let expected_relative = normalize_win32_str(&format!("{}\\foo", cwd));
        assert_eq!(win32_to_namespaced_path("foo"), expected_relative);
        assert_eq!(win32_to_namespaced_path("/tmp/x"), "\\tmp\\x");
        assert_eq!(win32_to_namespaced_path("C:\\foo"), "\\\\?\\C:\\foo");
        assert_eq!(
            win32_to_namespaced_path("\\\\server\\share\\file"),
            "\\\\?\\UNC\\server\\share\\file"
        );
        assert_eq!(
            win32_to_namespaced_path("\\\\?\\C:\\already"),
            "\\\\?\\C:\\already"
        );
    }
}

#[cfg(test)]
mod malloc_backed_string_arg_tests {
    use super::*;

    /// Build a REAL string whose backing allocation is malloc-tracked (not
    /// arena) — the shape produced by the string-append realloc path
    /// (`gc_malloc_realloc` → `gc_malloc(_, GC_TYPE_STRING)`) and by Symbol
    /// descriptions. Its user address classifies as `HeapGeneration::Unknown`.
    fn malloc_backed_string(bytes: &[u8]) -> *mut StringHeader {
        let payload = std::mem::size_of::<StringHeader>() + bytes.len();
        let user = crate::gc::gc_malloc(payload, crate::gc::GC_TYPE_STRING) as *mut StringHeader;
        assert!(!user.is_null());
        unsafe {
            crate::string::init_string_header(
                user,
                bytes.len() as u32,
                bytes.len() as u32,
                bytes.len() as u32,
                0,
                0,
            );
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                (user as *mut u8).add(std::mem::size_of::<StringHeader>()),
                bytes.len(),
            );
        }
        user
    }

    /// A malloc-backed (generation-Unknown) string is a valid `path` argument.
    /// `is_string_header_ptr` used to reject every generation-Unknown pointer,
    /// so `path.dirname()` (and every other `path.*` builtin) threw
    /// `TypeError: The "path" argument must be of type string.` whenever a
    /// program passed a string that had grown through the append realloc path.
    #[test]
    fn path_builtins_accept_malloc_backed_strings() {
        let path = malloc_backed_string(b"/a/b/c.txt");
        assert!(is_string_header_ptr(path));

        let dir = js_path_dirname(path);
        let dir_str = unsafe { string_from_header(dir) }.expect("dirname returns a string");
        assert_eq!(dir_str, "/a/b");

        let base = js_path_basename(path);
        let base_str = unsafe { string_from_header(base) }.expect("basename returns a string");
        assert_eq!(base_str, "c.txt");
    }

    /// Forged pointers (not in the malloc registry, not in an arena) must
    /// still be rejected without dereferencing the candidate header.
    #[test]
    fn forged_unknown_pointer_is_still_rejected() {
        let bogus = 0x0000_7777_0000_1000usize as *const StringHeader;
        assert!(!is_string_header_ptr(bogus));
    }
}

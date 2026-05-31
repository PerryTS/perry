//! opendir / glob / watch / watchFile / unwatchFile.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Once;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_arity, ClosureHeader,
};

use super::*;

pub extern "C" fn js_fs_opendir_sync(path_value: f64) -> f64 {
    match js_fs_opendir_value(path_value) {
        Ok(dir) => dir,
        Err(err) => crate::exception::js_throw(err),
    }
}

pub(crate) fn js_fs_opendir_value(path_value: f64) -> Result<f64, f64> {
    js_fs_opendir_value_inner(path_value, false)
}

pub(crate) fn js_fs_opendir_value_with_path(path_value: f64) -> Result<f64, f64> {
    js_fs_opendir_value_inner(path_value, true)
}

fn js_fs_opendir_value_inner(path_value: f64, include_path: bool) -> Result<f64, f64> {
    validate::validate_path("path", path_value);
    unsafe {
        let path = match decode_path_value(path_value) {
            Some(path) => path,
            None => validate::throw_invalid_path_arg("path", path_value),
        };
        let read_dir = match fs::read_dir(&path) {
            Ok(read_dir) => read_dir,
            Err(err) => {
                return Err(if include_path {
                    build_fs_error_value(&err, "opendir", &path)
                } else {
                    build_fs_error_value_no_path(&err, "opendir")
                });
            }
        };
        let mut entries = Vec::new();
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
                DirentKind::from_file_type(&ft),
            ));
        }
        Ok(build_dir_object(alloc_dir_state(entries), &path))
    }
}

#[derive(Clone)]
pub(crate) struct FsGlobMatch {
    output: String,
    actual_path: String,
    dirent_name: String,
    dirent_parent: String,
    kind: DirentKind,
}

struct FsGlobRun {
    matches: Vec<FsGlobMatch>,
    with_file_types: bool,
}

struct FsGlobOptions {
    cwd_actual: String,
    cwd_display: String,
    with_file_types: bool,
    follow_symlinks: bool,
    exclude_patterns: Vec<fancy_regex::Regex>,
    exclude_fn: Option<*const ClosureHeader>,
}

struct GlobCandidate {
    actual_path: String,
    kind: DirentKind,
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn pathbuf_to_slashes(path: PathBuf) -> String {
    normalize_slashes(&path.to_string_lossy())
}

fn current_dir_slashes() -> String {
    std::env::current_dir()
        .map(pathbuf_to_slashes)
        .unwrap_or_else(|_| ".".to_string())
}

fn trim_trailing_slashes(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path
    } else {
        trimmed
    }
}

fn join_slash(base: &str, child: &str) -> String {
    if child.is_empty() || child == "." {
        return normalize_slashes(base);
    }
    if Path::new(child).is_absolute() {
        return normalize_slashes(child);
    }
    let base = trim_trailing_slashes(base);
    if base.is_empty() || base == "." {
        normalize_slashes(child)
    } else if base == "/" {
        format!("/{}", child.trim_start_matches('/'))
    } else {
        format!("{}/{}", base, child.trim_start_matches('/'))
    }
}

fn absolutize_slash(path: &str) -> String {
    let normalized = normalize_slashes(path);
    if Path::new(&normalized).is_absolute() {
        normalized
    } else {
        join_slash(&current_dir_slashes(), &normalized)
    }
}

fn relative_to_base(path: &str, base: &str) -> String {
    let path = normalize_slashes(path);
    let base = normalize_slashes(base);
    let base_trim = trim_trailing_slashes(&base);
    if path == base_trim {
        return ".".to_string();
    }
    let prefix = if base_trim == "/" {
        "/".to_string()
    } else {
        format!("{base_trim}/")
    };
    path.strip_prefix(&prefix).unwrap_or(&path).to_string()
}

fn parent_display_for_relative(cwd_display: &str, rel_parent: &str) -> String {
    if rel_parent == "." || rel_parent.is_empty() {
        if cwd_display.is_empty() {
            ".".to_string()
        } else {
            cwd_display.to_string()
        }
    } else if cwd_display == "." || cwd_display.is_empty() {
        rel_parent.to_string()
    } else {
        join_slash(cwd_display, rel_parent)
    }
}

fn decode_string_value(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    Some(
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len as usize) })
            .into_owned(),
    )
}

fn decode_string_or_file_url(value: f64) -> Option<String> {
    if let Some(s) = decode_string_value(value) {
        return Some(s);
    }
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
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
    Some(crate::url::search_params::url_decode(&pathname))
}

fn array_ptr_from_value(value: f64) -> Option<*const crate::array::ArrayHeader> {
    if crate::array::js_array_is_array(value).to_bits() != crate::value::TAG_TRUE {
        return None;
    }
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let ptr = jsval.as_pointer::<crate::array::ArrayHeader>();
    if ptr.is_null() {
        None
    } else {
        Some(ptr)
    }
}

fn throw_glob_pattern_string(arg_name: &str, value: f64) -> ! {
    let message = format!(
        "The \"{arg_name}\" argument must be of type string. Received {}",
        validate::describe_received(value)
    );
    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_glob_patterns_array(value: f64) -> ! {
    let message = format!(
        "The \"patterns\" argument must be an instance of Array. Received {}",
        validate::describe_received(value)
    );
    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn glob_patterns_from_value(pattern_value: f64) -> Vec<String> {
    if let Some(pattern) = decode_string_value(pattern_value) {
        return vec![normalize_slashes(&pattern)];
    }
    if let Some(arr) = array_ptr_from_value(pattern_value) {
        let len = crate::array::js_array_length(arr) as usize;
        let mut patterns = Vec::with_capacity(len);
        for i in 0..len {
            let value = crate::array::js_array_get_f64(arr, i as u32);
            let Some(pattern) = decode_string_value(value) else {
                throw_glob_pattern_string(&format!("patterns[{i}]"), value);
            };
            patterns.push(normalize_slashes(&pattern));
        }
        return patterns;
    }
    let js = crate::value::JSValue::from_bits(pattern_value.to_bits());
    if js.is_null() || js.is_pointer() {
        throw_glob_patterns_array(pattern_value);
    }
    throw_glob_pattern_string("patterns", pattern_value);
}

fn compile_exclude_patterns(exclude_value: f64, cwd_actual: &str) -> Vec<fancy_regex::Regex> {
    let Some(arr) = array_ptr_from_value(exclude_value) else {
        let message = format!(
            "The \"options.exclude\" property must be of type function or string[]. Received {}",
            validate::describe_received(exclude_value)
        );
        validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let len = crate::array::js_array_length(arr) as usize;
    let mut patterns = Vec::with_capacity(len);
    for i in 0..len {
        let value = crate::array::js_array_get_f64(arr, i as u32);
        let Some(pattern) = decode_string_value(value) else {
            let message = format!(
                "The \"options.exclude[{i}]\" property must be of type string. Received {}",
                validate::describe_received(value)
            );
            validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        let normalized = normalize_slashes(&pattern);
        let absolute = if Path::new(&normalized).is_absolute() {
            normalized
        } else {
            join_slash(cwd_actual, &normalized)
        };
        if let Some(re) = glob_regex_from_pattern(&absolute) {
            patterns.push(re);
        }
    }
    patterns
}

fn glob_options_from_value(options_value: f64) -> FsGlobOptions {
    validate::validate_object_options("options", options_value);
    let mut cwd_actual = current_dir_slashes();
    let mut cwd_display = ".".to_string();
    unsafe {
        if let Some(cwd) = options_field_value(options_value, b"cwd") {
            let cwd_value = f64::from_bits(cwd.bits());
            if !is_nullish(cwd_value) {
                let Some(cwd_raw) = decode_string_or_file_url(cwd_value) else {
                    let message = format!(
                        "The \"paths[0]\" argument must be of type string. Received {}",
                        validate::describe_received(cwd_value)
                    );
                    validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
                };
                let cwd_norm = normalize_slashes(&cwd_raw);
                cwd_actual = absolutize_slash(&cwd_norm);
                cwd_display = cwd_norm;
            }
        }
    }
    let with_file_types = unsafe { options_bool_field(options_value, b"withFileTypes") };
    let follow_symlinks = unsafe { options_bool_field(options_value, b"followSymlinks") };
    let mut exclude_patterns = Vec::new();
    let mut exclude_fn = None;
    unsafe {
        if let Some(exclude) = options_field_value(options_value, b"exclude") {
            let exclude_value = f64::from_bits(exclude.bits());
            if !is_nullish(exclude_value) {
                let callable = extract_closure_ptr(exclude_value);
                if callable.is_null() {
                    exclude_patterns = compile_exclude_patterns(exclude_value, &cwd_actual);
                } else {
                    exclude_fn = Some(callable);
                }
            }
        }
    }
    FsGlobOptions {
        cwd_actual,
        cwd_display,
        with_file_types,
        follow_symlinks,
        exclude_patterns,
        exclude_fn,
    }
}

fn regex_escape_char(out: &mut String, ch: char) {
    if matches!(
        ch,
        '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\'
    ) {
        out.push('\\');
    }
    out.push(ch);
}

fn split_top_level(input: &str, separator: char) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '[' => {
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            '{' => brace_depth += 1,
            '}' if brace_depth > 0 => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' if paren_depth > 0 => paren_depth -= 1,
            ch if ch == separator && brace_depth == 0 && paren_depth == 0 => {
                parts.push(chars[start..i].iter().collect());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(chars[start..].iter().collect());
    parts
}

fn take_balanced(chars: &[char], pos: &mut usize, open: char, close: char) -> Option<String> {
    let mut depth = 1i32;
    let start = *pos;
    let mut i = *pos;
    while i < chars.len() {
        match chars[i] {
            '[' => {
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            ch if ch == open => depth += 1,
            ch if ch == close => {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[start..i].iter().collect();
                    *pos = i + 1;
                    return Some(inner);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn parse_char_class(chars: &[char], pos: &mut usize) -> String {
    let start = pos.saturating_sub(1);
    let mut class = String::from("[");
    if *pos < chars.len() && matches!(chars[*pos], '!' | '^') {
        class.push('^');
        *pos += 1;
    }
    if *pos < chars.len() && chars[*pos] == ']' {
        class.push(']');
        *pos += 1;
    }
    while *pos < chars.len() {
        let ch = chars[*pos];
        *pos += 1;
        if ch == ']' {
            class.push(']');
            return class;
        }
        if ch == '\\' {
            class.push('\\');
            class.push('\\');
        } else {
            class.push(ch);
        }
    }
    let literal: String = chars[start..*pos].iter().collect();
    regex::escape(&literal)
}

fn glob_fragment_to_regex(pattern: &str) -> Option<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0usize;
    parse_glob_chars(&chars, &mut pos)
}

fn parse_glob_chars(chars: &[char], pos: &mut usize) -> Option<String> {
    let mut out = String::new();
    while *pos < chars.len() {
        let ch = chars[*pos];
        if matches!(ch, '@' | '+' | '*' | '?' | '!') && chars.get(*pos + 1) == Some(&'(') {
            *pos += 2;
            let inner = take_balanced(chars, pos, '(', ')')?;
            let alternatives: Vec<String> = split_top_level(&inner, '|')
                .into_iter()
                .map(|part| glob_fragment_to_regex(&part))
                .collect::<Option<Vec<_>>>()?;
            let joined = alternatives.join("|");
            match ch {
                '@' => out.push_str(&format!("(?:{joined})")),
                '?' => out.push_str(&format!("(?:{joined})?")),
                '+' => out.push_str(&format!("(?:{joined})+")),
                '*' => out.push_str(&format!("(?:{joined})*")),
                '!' => out.push_str(&format!("(?!(?:{joined})(?:/|$))[^/]*")),
                _ => {}
            }
            continue;
        }
        *pos += 1;
        match ch {
            '*' => {
                if chars.get(*pos) == Some(&'*') {
                    *pos += 1;
                    if chars.get(*pos) == Some(&'/') {
                        *pos += 1;
                        out.push_str("(?:.*/)?");
                    } else {
                        out.push_str(".*");
                    }
                } else {
                    out.push_str("[^/]*");
                }
            }
            '?' => out.push_str("[^/]"),
            '{' => {
                let inner = take_balanced(chars, pos, '{', '}')?;
                let alternatives: Vec<String> = split_top_level(&inner, ',')
                    .into_iter()
                    .map(|part| glob_fragment_to_regex(&part))
                    .collect::<Option<Vec<_>>>()?;
                out.push_str(&format!("(?:{})", alternatives.join("|")));
            }
            '[' => out.push_str(&parse_char_class(chars, pos)),
            '/' => out.push('/'),
            other => regex_escape_char(&mut out, other),
        }
    }
    Some(out)
}

pub(crate) fn glob_regex_from_pattern(pattern: &str) -> Option<fancy_regex::Regex> {
    let normalized = normalize_slashes(pattern);
    let body = glob_fragment_to_regex(&normalized)?;
    fancy_regex::Regex::new(&format!("^{body}$")).ok()
}

fn first_glob_meta(pattern: &str) -> usize {
    let chars: Vec<(usize, char)> = pattern.char_indices().collect();
    for (idx, (byte_idx, ch)) in chars.iter().enumerate() {
        if matches!(ch, '*' | '?' | '[' | '{') {
            return *byte_idx;
        }
        if matches!(ch, '@' | '+' | '!') && chars.get(idx + 1).map(|(_, next)| *next) == Some('(') {
            return *byte_idx;
        }
    }
    pattern.len()
}

pub(crate) fn glob_search_root(pattern: &str) -> String {
    let normalized = normalize_slashes(pattern);
    let first_meta = first_glob_meta(&normalized);
    let prefix = &normalized[..first_meta];
    match prefix.rfind('/') {
        Some(0) => "/".to_string(),
        Some(idx) => prefix[..idx].to_string(),
        None => ".".to_string(),
    }
}

fn walk_paths_for_glob(dir: &Path, follow_symlinks: bool, out: &mut Vec<GlobCandidate>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by(|a, b| a.path().cmp(&b.path()));
    for entry in entries {
        let path = entry.path();
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let kind = DirentKind::from_file_type(&ft);
        out.push(GlobCandidate {
            actual_path: path.to_string_lossy().replace('\\', "/"),
            kind,
        });
        if ft.is_dir() || (follow_symlinks && path.is_dir()) {
            walk_paths_for_glob(&path, follow_symlinks, out);
        }
    }
}

fn glob_match_from_candidate(
    candidate: &GlobCandidate,
    pattern_is_absolute: bool,
    options: &FsGlobOptions,
) -> Option<FsGlobMatch> {
    let actual_path = normalize_slashes(&candidate.actual_path);
    let rel_output = relative_to_base(&actual_path, &options.cwd_actual);
    let output = if pattern_is_absolute {
        actual_path.clone()
    } else {
        rel_output.clone()
    };
    let name = Path::new(&actual_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return None;
    }
    let dirent_parent = if pattern_is_absolute {
        Path::new(&actual_path)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| ".".to_string())
    } else {
        let rel_parent = Path::new(&rel_output)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| ".".to_string());
        parent_display_for_relative(&options.cwd_display, &rel_parent)
    };
    Some(FsGlobMatch {
        output,
        actual_path,
        dirent_name: name,
        dirent_parent,
        kind: candidate.kind,
    })
}

fn excluded_by_patterns(path: &str, options: &FsGlobOptions) -> bool {
    options
        .exclude_patterns
        .iter()
        .any(|re| re.is_match(path).unwrap_or(false))
}

fn excluded_by_function(entry: &FsGlobMatch, options: &FsGlobOptions) -> bool {
    let Some(callback) = options.exclude_fn else {
        return false;
    };
    let arg = if options.with_file_types {
        unsafe { build_dirent_object(&entry.dirent_name, &entry.dirent_parent, entry.kind) }
    } else {
        string_value(entry.output.as_bytes())
    };
    crate::value::js_is_truthy(crate::closure::js_closure_call1(callback, arg)) != 0
}

fn glob_entry_value(entry: &FsGlobMatch, with_file_types: bool) -> f64 {
    if with_file_types {
        unsafe { build_dirent_object(&entry.dirent_name, &entry.dirent_parent, entry.kind) }
    } else {
        string_value(entry.output.as_bytes())
    }
}

fn run_fs_glob(pattern_value: f64, options_value: f64) -> FsGlobRun {
    let patterns = glob_patterns_from_value(pattern_value);
    let options = glob_options_from_value(options_value);
    let mut matches: BTreeMap<String, FsGlobMatch> = BTreeMap::new();
    for pattern in patterns {
        let pattern_is_absolute = Path::new(&pattern).is_absolute();
        let pattern_for_match = if pattern_is_absolute {
            normalize_slashes(&pattern)
        } else {
            normalize_slashes(&pattern)
        };
        let Some(re) = glob_regex_from_pattern(&pattern_for_match) else {
            continue;
        };
        let root = glob_search_root(&pattern_for_match);
        let root_actual = if pattern_is_absolute {
            root
        } else {
            join_slash(&options.cwd_actual, &root)
        };
        let mut candidates = Vec::new();
        walk_paths_for_glob(
            Path::new(&root_actual),
            options.follow_symlinks,
            &mut candidates,
        );
        for candidate in &candidates {
            let target = if pattern_is_absolute {
                candidate.actual_path.clone()
            } else {
                relative_to_base(&candidate.actual_path, &options.cwd_actual)
            };
            if !re.is_match(&target).unwrap_or(false) {
                continue;
            }
            let Some(entry) = glob_match_from_candidate(candidate, pattern_is_absolute, &options)
            else {
                continue;
            };
            if excluded_by_patterns(&entry.actual_path, &options)
                || excluded_by_function(&entry, &options)
            {
                continue;
            }
            matches.entry(entry.output.clone()).or_insert(entry);
        }
    }
    FsGlobRun {
        matches: matches.into_values().collect(),
        with_file_types: options.with_file_types,
    }
}

/// `fs.globSync(pattern)` — deterministic Node-compatible glob subset.
#[no_mangle]
pub extern "C" fn js_fs_glob_sync(pattern_value: f64) -> f64 {
    js_fs_glob_sync_options(pattern_value, f64::from_bits(crate::value::TAG_UNDEFINED))
}

#[no_mangle]
pub extern "C" fn js_fs_glob_sync_options(pattern_value: f64, options_value: f64) -> f64 {
    use crate::array::{js_array_alloc, js_array_push_f64};

    let run = run_fs_glob(pattern_value, options_value);
    let mut arr = js_array_alloc(run.matches.len() as u32);
    for entry in &run.matches {
        arr = js_array_push_f64(arr, glob_entry_value(entry, run.with_file_types));
    }
    f64::from_bits(i64::cast_unsigned(arr as i64))
}

const FS_WATCH_POLL_INTERVAL_MS: f64 = 25.0;
const WATCH_FILE_DEFAULT_INTERVAL_MS: f64 = 5007.0;

#[derive(Clone, Copy)]
struct WatchListener {
    callback: f64,
    once: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct WatchEntry {
    is_file: bool,
    is_dir: bool,
    is_symlink: bool,
    len: u64,
    mode: u32,
    modified_ns: i128,
    created_ns: i128,
}

type WatchSnapshot = BTreeMap<String, WatchEntry>;

#[derive(Clone)]
struct WatchEvent {
    event_type: &'static str,
    filename: String,
}

struct FsWatchState {
    path: String,
    recursive: bool,
    encoding: String,
    object_value: f64,
    timer_id: i64,
    snapshot: WatchSnapshot,
    listeners: HashMap<String, Vec<WatchListener>>,
    signal: f64,
    abort_listener: f64,
}

#[derive(Clone, PartialEq)]
struct StatSnapshot {
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
}

struct WatchFileState {
    path: String,
    object_value: f64,
    timer_id: i64,
    bigint: bool,
    previous: Option<StatSnapshot>,
    listeners: HashMap<String, Vec<WatchListener>>,
}

struct PromiseWatchState {
    path: String,
    recursive: bool,
    encoding: String,
    object_value: f64,
    timer_id: i64,
    snapshot: WatchSnapshot,
    queue: VecDeque<WatchEvent>,
    pending: VecDeque<*mut crate::promise::Promise>,
    signal: f64,
    abort_listener: f64,
    closed: bool,
    abort_reason: Option<f64>,
}

struct GlobIteratorState {
    entries: Vec<FsGlobMatch>,
    index: usize,
    with_file_types: bool,
    closed: bool,
}

thread_local! {
    static NEXT_WATCH_ID: RefCell<usize> = const { RefCell::new(1) };
    static NEXT_GLOB_ITERATOR_ID: RefCell<usize> = const { RefCell::new(1) };
    static FS_WATCHERS: RefCell<HashMap<usize, FsWatchState>> = RefCell::new(HashMap::new());
    static WATCH_FILE_STATES: RefCell<HashMap<usize, WatchFileState>> = RefCell::new(HashMap::new());
    static WATCH_FILE_PATHS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    static PROMISE_WATCHERS: RefCell<HashMap<usize, PromiseWatchState>> = RefCell::new(HashMap::new());
    static GLOB_ITERATORS: RefCell<HashMap<usize, GlobIteratorState>> = RefCell::new(HashMap::new());
}

fn next_watch_id() -> usize {
    NEXT_WATCH_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1);
        id
    })
}

fn next_glob_iterator_id() -> usize {
    NEXT_GLOB_ITERATOR_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1);
        id
    })
}

fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(value).bits())
}

fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(ptr).bits())
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

fn is_nullish(value: f64) -> bool {
    let js = crate::value::JSValue::from_bits(value.to_bits());
    js.is_undefined() || js.is_null()
}

fn is_callable(value: f64) -> bool {
    !extract_closure_ptr(value).is_null()
}

fn read_string_value(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) {
        if ptr.is_null() {
            return Some(String::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    None
}

fn event_name(value: f64) -> String {
    read_string_value(value).unwrap_or_default()
}

fn validate_listener(value: f64) {
    unsafe {
        let _ = validate::js_validate_event_listener(
            value.to_bits() as i64,
            b"listener".as_ptr(),
            b"listener".len() as u32,
        );
    }
}

fn optional_listener(value: f64) -> Option<f64> {
    if is_nullish(value) {
        None
    } else {
        validate_listener(value);
        Some(value)
    }
}

fn option_bool_default_local(options_value: f64, field: &[u8], default_value: bool) -> bool {
    unsafe {
        match options_field_value(options_value, field) {
            Some(value) => crate::value::js_is_truthy(f64::from_bits(value.bits())) != 0,
            None => default_value,
        }
    }
}

fn option_interval_ms(options_value: f64) -> f64 {
    unsafe {
        options_number_field(options_value, b"interval")
            .filter(|n| n.is_finite() && *n > 0.0)
            .unwrap_or(WATCH_FILE_DEFAULT_INTERVAL_MS)
    }
}

fn signal_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"options.signal\" property must be an instance of AbortSignal. Received {}",
        validate::describe_received(value)
    );
    validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn option_signal_value(options_value: f64) -> Result<Option<f64>, f64> {
    let options_js = crate::value::JSValue::from_bits(options_value.to_bits());
    if options_js.is_undefined() || options_js.is_null() || options_js.is_any_string() {
        return Ok(None);
    }
    unsafe {
        let Some(signal_value) = options_field_value(options_value, b"signal") else {
            return Ok(None);
        };
        let signal = f64::from_bits(signal_value.bits());
        if is_nullish(signal) {
            return Ok(None);
        }
        if crate::url::abort::abort_signal_ptr_from_value(signal).is_some() {
            Ok(Some(signal))
        } else {
            Err(signal_type_error(signal))
        }
    }
}

fn signal_is_aborted(signal: f64) -> bool {
    crate::url::abort::abort_signal_ptr_from_value(signal)
        .is_some_and(|ptr| crate::url::js_abort_signal_is_aborted(ptr) != 0)
}

fn signal_abort_reason(signal: f64) -> f64 {
    let Some(ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return crate::url::js_abort_error_value();
    };
    let reason = crate::object::js_object_get_field_f64(ptr, 1);
    if crate::value::JSValue::from_bits(reason.to_bits()).is_undefined() {
        crate::url::js_abort_error_value()
    } else {
        reason
    }
}

fn add_abort_listener(
    signal: f64,
    id: usize,
    func: extern "C" fn(*const ClosureHeader) -> f64,
) -> f64 {
    let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return undefined_value();
    };
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, id as f64);
    let listener = boxed_ptr(closure as *const u8);
    crate::url::js_abort_signal_add_listener(signal_ptr, string_value(b"abort"), listener);
    listener
}

fn remove_abort_listener(signal: f64, listener: f64) {
    if is_nullish(signal) || is_nullish(listener) {
        return;
    }
    if let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) {
        crate::url::js_abort_signal_remove_listener(signal_ptr, string_value(b"abort"), listener);
    }
}

fn metadata_time_ns(time: std::io::Result<std::time::SystemTime>) -> i128 {
    time.ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0)
}

fn watch_entry_from_metadata(meta: &fs::Metadata) -> WatchEntry {
    let ft = meta.file_type();
    #[cfg(unix)]
    let mode = meta.permissions().mode();
    #[cfg(not(unix))]
    let mode = if meta.permissions().readonly() {
        0o444
    } else {
        0o666
    };
    WatchEntry {
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        len: meta.len(),
        mode,
        modified_ns: metadata_time_ns(meta.modified()),
        created_ns: metadata_time_ns(meta.created()),
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn walk_watch_dir(root: &Path, dir: &Path, recursive: bool, out: &mut WatchSnapshot) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        let rel = relative_path(root, &path);
        out.insert(rel, watch_entry_from_metadata(&meta));
        if recursive && meta.is_dir() {
            walk_watch_dir(root, &path, true, out);
        }
    }
}

fn snapshot_watch_target(path: &str, recursive: bool) -> std::io::Result<WatchSnapshot> {
    let root = Path::new(path);
    let meta = fs::symlink_metadata(root)?;
    let mut snapshot = WatchSnapshot::new();
    if meta.is_dir() {
        walk_watch_dir(root, root, recursive, &mut snapshot);
    } else {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string());
        snapshot.insert(name, watch_entry_from_metadata(&meta));
    }
    Ok(snapshot)
}

fn diff_watch_snapshots(previous: &WatchSnapshot, current: &WatchSnapshot) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    let mut keys = BTreeMap::<String, ()>::new();
    for key in previous.keys() {
        keys.insert(key.clone(), ());
    }
    for key in current.keys() {
        keys.insert(key.clone(), ());
    }
    for key in keys.keys() {
        match (previous.get(key), current.get(key)) {
            (None, Some(_)) | (Some(_), None) => events.push(WatchEvent {
                event_type: "rename",
                filename: key.clone(),
            }),
            (Some(a), Some(b)) if a != b => events.push(WatchEvent {
                event_type: "change",
                filename: key.clone(),
            }),
            _ => {}
        }
    }
    events
}

fn stat_snapshot(path: &str) -> Option<StatSnapshot> {
    let meta = fs::metadata(path).ok()?;
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
    let (atime_ms, mtime_ms, ctime_ms, birthtime_ms) = metadata_times_ms(&meta);
    Some(StatSnapshot {
        is_file: ft.is_file(),
        is_dir: ft.is_dir(),
        is_symlink: ft.is_symlink(),
        size: meta.len(),
        mode,
        uid,
        gid,
        nlink,
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    })
}

fn zero_stat_snapshot() -> StatSnapshot {
    StatSnapshot {
        is_file: false,
        is_dir: false,
        is_symlink: false,
        size: 0,
        mode: 0,
        uid: -1.0,
        gid: -1.0,
        nlink: 0.0,
        atime_ms: 0.0,
        mtime_ms: 0.0,
        ctime_ms: 0.0,
        birthtime_ms: 0.0,
    }
}

fn build_stat_value(snapshot: &StatSnapshot, bigint: bool) -> f64 {
    unsafe {
        build_stats_object(
            snapshot.is_file,
            snapshot.is_dir,
            snapshot.is_symlink,
            snapshot.size,
            snapshot.mode,
            snapshot.uid,
            snapshot.gid,
            snapshot.nlink,
            snapshot.atime_ms,
            snapshot.mtime_ms,
            snapshot.ctime_ms,
            snapshot.birthtime_ms,
            bigint,
            None,
        )
    }
}

fn add_listener(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: String,
    callback: f64,
    once: bool,
) {
    listeners
        .entry(event)
        .or_default()
        .push(WatchListener { callback, once });
}

fn take_event_listeners(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: &str,
) -> Vec<WatchListener> {
    let snapshot = listeners.get(event).cloned().unwrap_or_default();
    if snapshot.iter().any(|listener| listener.once) {
        if let Some(list) = listeners.get_mut(event) {
            list.retain(|listener| !listener.once);
        }
    }
    snapshot
}

fn remove_listener(
    listeners: &mut HashMap<String, Vec<WatchListener>>,
    event: &str,
    callback: f64,
) {
    if let Some(list) = listeners.get_mut(event) {
        let bits = callback.to_bits();
        list.retain(|listener| listener.callback.to_bits() != bits);
    }
}

fn has_change_listeners(listeners: &HashMap<String, Vec<WatchListener>>) -> bool {
    listeners
        .get("change")
        .is_some_and(|listeners| !listeners.is_empty())
}

fn with_watcher_uncaught_trap<F: FnOnce()>(f: F) {
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    if jumped == 0 {
        f();
    } else {
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::os::emit_process_uncaught_exception(exc);
    }
    crate::exception::js_try_end();
}

fn filename_arg_value(filename: &str, encoding: &str) -> f64 {
    let bytes = filename.as_bytes();
    if encoding == "buffer" {
        let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
        if !buf.is_null() && !bytes.is_empty() {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    crate::buffer::buffer_data_mut(buf),
                    bytes.len(),
                );
            }
        }
        boxed_ptr(buf as *const u8)
    } else {
        let ptr = encoded_string_ptr(bytes, encoding);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
}

fn emit_listener0(object_value: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object_handle = scope.root_nanbox_f64(object_value);
    let callback_handle = scope.root_nanbox_f64(callback);
    let cb = extract_closure_ptr(callback_handle.get_nanbox_f64());
    if cb.is_null() {
        return;
    }
    let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
    with_watcher_uncaught_trap(|| {
        crate::closure::js_closure_call0(cb);
    });
    crate::object::js_implicit_this_set(prev_this);
}

fn emit_fs_watch_event(
    object_value: f64,
    callbacks: Vec<WatchListener>,
    event: &WatchEvent,
    encoding: &str,
) {
    if callbacks.is_empty() {
        return;
    }
    let raw_callbacks: Vec<f64> = callbacks.iter().map(|listener| listener.callback).collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handles = scope.root_nanbox_f64_slice(&raw_callbacks);
    let object_handle = scope.root_nanbox_f64(object_value);
    let event_type = string_value(event.event_type.as_bytes());
    let event_type_handle = scope.root_nanbox_f64(event_type);
    let filename = filename_arg_value(&event.filename, encoding);
    let args = [event_type_handle.get_nanbox_f64(), filename];
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let refreshed_callbacks =
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&callback_handles);
    let refreshed_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    for callback in refreshed_callbacks {
        let cb = extract_closure_ptr(callback);
        if cb.is_null() {
            continue;
        }
        let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
        with_watcher_uncaught_trap(|| {
            crate::closure::js_closure_call2(cb, refreshed_args[0], refreshed_args[1]);
        });
        crate::object::js_implicit_this_set(prev_this);
    }
}

fn emit_watch_file_change(
    object_value: f64,
    callbacks: Vec<WatchListener>,
    curr: &StatSnapshot,
    prev: &StatSnapshot,
    bigint: bool,
) {
    if callbacks.is_empty() {
        return;
    }
    let raw_callbacks: Vec<f64> = callbacks.iter().map(|listener| listener.callback).collect();
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback_handles = scope.root_nanbox_f64_slice(&raw_callbacks);
    let object_handle = scope.root_nanbox_f64(object_value);
    let curr_value = build_stat_value(curr, bigint);
    let curr_handle = scope.root_nanbox_f64(curr_value);
    let prev_value = build_stat_value(prev, bigint);
    let args = [curr_handle.get_nanbox_f64(), prev_value];
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let refreshed_callbacks =
        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&callback_handles);
    let refreshed_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    for callback in refreshed_callbacks {
        let cb = extract_closure_ptr(callback);
        if cb.is_null() {
            continue;
        }
        let prev_this = crate::object::js_implicit_this_set(object_handle.get_nanbox_f64());
        with_watcher_uncaught_trap(|| {
            crate::closure::js_closure_call2(cb, refreshed_args[0], refreshed_args[1]);
        });
        crate::object::js_implicit_this_set(prev_this);
    }
}

fn close_fs_watcher(id: usize) {
    let removed = FS_WATCHERS.with(|watchers| watchers.borrow_mut().remove(&id));
    let Some(mut state) = removed else {
        return;
    };
    crate::timer::clearInterval(state.timer_id);
    remove_abort_listener(state.signal, state.abort_listener);
    let close_listeners = take_event_listeners(&mut state.listeners, "close");
    for listener in close_listeners {
        emit_listener0(state.object_value, listener.callback);
    }
}

fn close_watch_file_state(id: usize) {
    let removed = WATCH_FILE_STATES.with(|states| states.borrow_mut().remove(&id));
    if let Some(state) = removed {
        crate::timer::clearInterval(state.timer_id);
        WATCH_FILE_PATHS.with(|paths| {
            paths.borrow_mut().remove(&state.path);
        });
    }
}

fn close_promise_watcher_return(id: usize) -> Vec<*mut crate::promise::Promise> {
    let removed = PROMISE_WATCHERS.with(|watchers| watchers.borrow_mut().remove(&id));
    let Some(state) = removed else {
        return Vec::new();
    };
    crate::timer::clearInterval(state.timer_id);
    remove_abort_listener(state.signal, state.abort_listener);
    state.pending.into_iter().collect()
}

fn abort_promise_watcher(id: usize, reason: f64) -> Vec<*mut crate::promise::Promise> {
    PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        crate::timer::clearInterval(state.timer_id);
        remove_abort_listener(state.signal, state.abort_listener);
        state.timer_id = 0;
        state.signal = undefined_value();
        state.abort_listener = undefined_value();
        state.object_value = undefined_value();
        state.closed = true;
        state.abort_reason = Some(reason);
        state.queue.clear();
        state.pending.drain(..).collect()
    })
}

fn iterator_result(value: f64, done: bool) -> f64 {
    let value_key = js_string_from_bytes(b"value".as_ptr(), b"value".len() as u32);
    let done_key = js_string_from_bytes(b"done".as_ptr(), b"done".len() as u32);
    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field_by_name(obj, value_key, value);
    crate::object::js_object_set_field_by_name(obj, done_key, bool_value(done));
    boxed_ptr(obj as *const u8)
}

fn set_named_field(obj: *mut crate::object::ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

fn watch_event_object(event: &WatchEvent, encoding: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event_type = string_value(event.event_type.as_bytes());
    let event_type_handle = scope.root_nanbox_f64(event_type);
    let filename = filename_arg_value(&event.filename, encoding);
    let filename_handle = scope.root_nanbox_f64(filename);
    let event_type_key = js_string_from_bytes(b"eventType".as_ptr(), b"eventType".len() as u32);
    let filename_key = js_string_from_bytes(b"filename".as_ptr(), b"filename".len() as u32);
    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field_by_name(
        obj,
        event_type_key,
        event_type_handle.get_nanbox_f64(),
    );
    crate::object::js_object_set_field_by_name(obj, filename_key, filename_handle.get_nanbox_f64());
    boxed_ptr(obj as *const u8)
}

fn promise_value_from_ptr(promise: *mut crate::promise::Promise) -> f64 {
    boxed_ptr(promise as *const u8)
}

fn resolved_iterator_promise(value: f64, done: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let result = iterator_result(value_handle.get_nanbox_f64(), done);
    let result_handle = scope.root_nanbox_f64(result);
    promise_value_from_ptr(crate::promise::js_promise_resolved(
        result_handle.get_nanbox_f64(),
    ))
}

fn rejected_promise_value(reason: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason_handle = scope.root_nanbox_f64(reason);
    promise_value_from_ptr(crate::promise::js_promise_rejected(
        reason_handle.get_nanbox_f64(),
    ))
}

fn resolve_promise_with_event(
    promise: *mut crate::promise::Promise,
    event: WatchEvent,
    encoding: String,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let event_value = watch_event_object(&event, &encoding);
    let event_handle = scope.root_nanbox_f64(event_value);
    let result = iterator_result(event_handle.get_nanbox_f64(), false);
    let result_handle = scope.root_nanbox_f64(result);
    crate::promise::js_promise_resolve(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        result_handle.get_nanbox_f64(),
    );
}

fn resolve_promise_done(promise: *mut crate::promise::Promise) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let result = iterator_result(undefined_value(), true);
    let result_handle = scope.root_nanbox_f64(result);
    crate::promise::js_promise_resolve(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        result_handle.get_nanbox_f64(),
    );
}

fn reject_promise(promise: *mut crate::promise::Promise, reason: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let reason_handle = scope.root_nanbox_f64(reason);
    crate::promise::js_promise_reject(
        promise_handle.get_raw_mut_ptr::<crate::promise::Promise>(),
        reason_handle.get_nanbox_f64(),
    );
}

extern "C" fn fs_watcher_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let deliveries = FS_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        let current = snapshot_watch_target(&state.path, state.recursive).unwrap_or_default();
        let events = diff_watch_snapshots(&state.snapshot, &current);
        state.snapshot = current;
        events
            .into_iter()
            .map(|event| {
                let callbacks = take_event_listeners(&mut state.listeners, "change");
                (state.object_value, callbacks, event, state.encoding.clone())
            })
            .collect()
    });
    for (object_value, callbacks, event, encoding) in deliveries {
        emit_fs_watch_event(object_value, callbacks, &event, &encoding);
    }
    undefined_value()
}

extern "C" fn promise_watcher_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let actions = PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return Vec::new();
        };
        if state.closed {
            return Vec::new();
        }
        let current = snapshot_watch_target(&state.path, state.recursive).unwrap_or_default();
        let events = diff_watch_snapshots(&state.snapshot, &current);
        state.snapshot = current;
        let mut actions = Vec::new();
        for event in events {
            if let Some(promise) = state.pending.pop_front() {
                actions.push((promise, event, state.encoding.clone()));
            } else {
                state.queue.push_back(event);
            }
        }
        actions
    });
    for (promise, event, encoding) in actions {
        resolve_promise_with_event(promise, event, encoding);
    }
    undefined_value()
}

extern "C" fn watch_file_poll_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let delivery = WATCH_FILE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            return None;
        };
        let current = stat_snapshot(&state.path);
        if current == state.previous {
            return None;
        }
        let prev = state.previous.clone().unwrap_or_else(zero_stat_snapshot);
        let curr = current.clone().unwrap_or_else(zero_stat_snapshot);
        state.previous = current;
        let callbacks = take_event_listeners(&mut state.listeners, "change");
        Some((state.object_value, callbacks, curr, prev, state.bigint))
    });
    if let Some((object_value, callbacks, curr, prev, bigint)) = delivery {
        emit_watch_file_change(object_value, callbacks, &curr, &prev, bigint);
    }
    undefined_value()
}

extern "C" fn fs_watcher_abort_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    close_fs_watcher(id);
    undefined_value()
}

extern "C" fn promise_watcher_abort_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let signal = PROMISE_WATCHERS.with(|watchers| {
        watchers
            .borrow()
            .get(&id)
            .map(|state| state.signal)
            .unwrap_or_else(undefined_value)
    });
    let reason = signal_abort_reason(signal);
    let pending = abort_promise_watcher(id, reason);
    for promise in pending {
        reject_promise(promise, reason);
    }
    undefined_value()
}

extern "C" fn fs_watcher_close_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    close_fs_watcher(id);
    self_value
}

extern "C" fn fs_watcher_ref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow().get(&id) {
            crate::timer::js_timer_ref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn fs_watcher_unref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow().get(&id) {
            crate::timer::js_timer_unref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn fs_watcher_on_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, false);
        }
    });
    self_value
}

extern "C" fn fs_watcher_once_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, true);
        }
    });
    self_value
}

extern "C" fn fs_watcher_off_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    FS_WATCHERS.with(|watchers| {
        if let Some(state) = watchers.borrow_mut().get_mut(&id) {
            remove_listener(&mut state.listeners, &event, listener);
        }
    });
    self_value
}

extern "C" fn stat_watcher_ref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow().get(&id) {
            crate::timer::js_timer_ref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn stat_watcher_unref_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow().get(&id) {
            crate::timer::js_timer_unref(state.timer_id);
        }
    });
    self_value
}

extern "C" fn stat_watcher_on_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, false);
        }
    });
    self_value
}

extern "C" fn stat_watcher_once_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            add_listener(&mut state.listeners, event, listener, true);
        }
    });
    self_value
}

extern "C" fn stat_watcher_off_impl(
    closure: *const ClosureHeader,
    event_value: f64,
    listener: f64,
) -> f64 {
    validate_listener(listener);
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let self_value = js_closure_get_capture_f64(closure, 1);
    let event = event_name(event_value);
    WATCH_FILE_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().get_mut(&id) {
            remove_listener(&mut state.listeners, &event, listener);
        }
    });
    self_value
}

enum PromiseNextAction {
    Done,
    Reject(f64),
    Event(WatchEvent, String),
    Pending,
}

enum GlobNextAction {
    Done,
    Entry(FsGlobMatch, bool),
}

extern "C" fn glob_iterator_next_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let action = GLOB_ITERATORS.with(|iterators| {
        let mut iterators = iterators.borrow_mut();
        let Some(state) = iterators.get_mut(&id) else {
            return GlobNextAction::Done;
        };
        if state.closed || state.index >= state.entries.len() {
            state.closed = true;
            return GlobNextAction::Done;
        }
        let entry = state.entries[state.index].clone();
        state.index += 1;
        GlobNextAction::Entry(entry, state.with_file_types)
    });
    match action {
        GlobNextAction::Done => resolved_iterator_promise(undefined_value(), true),
        GlobNextAction::Entry(entry, with_file_types) => {
            resolved_iterator_promise(glob_entry_value(&entry, with_file_types), false)
        }
    }
}

extern "C" fn glob_iterator_return_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    GLOB_ITERATORS.with(|iterators| {
        iterators.borrow_mut().remove(&id);
    });
    resolved_iterator_promise(undefined_value(), true)
}

extern "C" fn glob_iterator_self_impl(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 1)
}

extern "C" fn promise_watcher_next_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let action = PROMISE_WATCHERS.with(|watchers| {
        let mut watchers = watchers.borrow_mut();
        let Some(state) = watchers.get_mut(&id) else {
            return PromiseNextAction::Done;
        };
        if let Some(reason) = state.abort_reason {
            return PromiseNextAction::Reject(reason);
        }
        if state.closed {
            return PromiseNextAction::Done;
        }
        if let Some(event) = state.queue.pop_front() {
            return PromiseNextAction::Event(event, state.encoding.clone());
        }
        PromiseNextAction::Pending
    });
    match action {
        PromiseNextAction::Done => resolved_iterator_promise(undefined_value(), true),
        PromiseNextAction::Reject(reason) => rejected_promise_value(reason),
        PromiseNextAction::Event(event, encoding) => {
            let value = watch_event_object(&event, &encoding);
            resolved_iterator_promise(value, false)
        }
        PromiseNextAction::Pending => {
            let promise = crate::promise::js_promise_new();
            PROMISE_WATCHERS.with(|watchers| {
                if let Some(state) = watchers.borrow_mut().get_mut(&id) {
                    state.pending.push_back(promise);
                }
            });
            promise_value_from_ptr(promise)
        }
    }
}

extern "C" fn promise_watcher_return_impl(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as usize;
    let pending = close_promise_watcher_return(id);
    for promise in pending {
        resolve_promise_done(promise);
    }
    resolved_iterator_promise(undefined_value(), true)
}

extern "C" fn promise_watcher_self_impl(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 1)
}

fn ensure_watch_method_arities() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        js_register_closure_arity(fs_watcher_poll_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_poll_impl as *const u8, 0);
        js_register_closure_arity(watch_file_poll_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_abort_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_abort_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_close_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_ref_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_unref_impl as *const u8, 0);
        js_register_closure_arity(fs_watcher_on_impl as *const u8, 2);
        js_register_closure_arity(fs_watcher_once_impl as *const u8, 2);
        js_register_closure_arity(fs_watcher_off_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_ref_impl as *const u8, 0);
        js_register_closure_arity(stat_watcher_unref_impl as *const u8, 0);
        js_register_closure_arity(stat_watcher_on_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_once_impl as *const u8, 2);
        js_register_closure_arity(stat_watcher_off_impl as *const u8, 2);
        js_register_closure_arity(promise_watcher_next_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_return_impl as *const u8, 0);
        js_register_closure_arity(promise_watcher_self_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_next_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_return_impl as *const u8, 0);
        js_register_closure_arity(glob_iterator_self_impl as *const u8, 0);
    });
}

fn method_value(func: *const u8, id: usize, self_value: f64) -> f64 {
    let closure = js_closure_alloc(func, 2);
    js_closure_set_capture_f64(closure, 0, id as f64);
    js_closure_set_capture_f64(closure, 1, self_value);
    boxed_ptr(closure as *const u8)
}

fn poll_closure_value(func: *const u8, id: usize) -> *mut ClosureHeader {
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_f64(closure, 0, id as f64);
    closure
}

fn build_fs_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 8);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"close",
        method_value(fs_watcher_close_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"ref",
        method_value(fs_watcher_ref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"unref",
        method_value(fs_watcher_unref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"on",
        method_value(fs_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"once",
        method_value(fs_watcher_once_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"addListener",
        method_value(fs_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"removeListener",
        method_value(fs_watcher_off_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"off",
        method_value(fs_watcher_off_impl as *const u8, id, self_value),
    );
    self_value
}

fn build_stat_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 7);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"ref",
        method_value(stat_watcher_ref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"unref",
        method_value(stat_watcher_unref_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"on",
        method_value(stat_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"once",
        method_value(stat_watcher_once_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"addListener",
        method_value(stat_watcher_on_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"removeListener",
        method_value(stat_watcher_off_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"off",
        method_value(stat_watcher_off_impl as *const u8, id, self_value),
    );
    self_value
}

fn build_promise_watcher_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 2);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"next",
        method_value(promise_watcher_next_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"return",
        method_value(promise_watcher_return_impl as *const u8, id, self_value),
    );
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let symbol_value = boxed_ptr(async_iterator as *const u8);
        let method = method_value(promise_watcher_self_impl as *const u8, id, self_value);
        unsafe {
            crate::symbol::js_object_set_symbol_property(self_value, symbol_value, method);
        }
    }
    self_value
}

fn build_glob_iterator_object(id: usize) -> f64 {
    ensure_watch_method_arities();
    let obj = crate::object::js_object_alloc(0, 3);
    let self_value = boxed_ptr(obj as *const u8);
    set_named_field(
        obj,
        b"next",
        method_value(glob_iterator_next_impl as *const u8, id, self_value),
    );
    set_named_field(
        obj,
        b"return",
        method_value(glob_iterator_return_impl as *const u8, id, self_value),
    );
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let symbol_value = boxed_ptr(async_iterator as *const u8);
        let method = method_value(glob_iterator_self_impl as *const u8, id, self_value);
        unsafe {
            crate::symbol::js_object_set_symbol_property(self_value, symbol_value, method);
        }
    }
    self_value
}

pub(crate) fn js_fs_promises_glob_iterator(pattern_value: f64, options_value: f64) -> f64 {
    let run = run_fs_glob(pattern_value, options_value);
    let id = next_glob_iterator_id();
    GLOB_ITERATORS.with(|iterators| {
        iterators.borrow_mut().insert(
            id,
            GlobIteratorState {
                entries: run.matches,
                index: 0,
                with_file_types: run.with_file_types,
                closed: false,
            },
        );
    });
    build_glob_iterator_object(id)
}

fn normalized_watch_args(arg1: f64, arg2: f64) -> (f64, Option<f64>) {
    if is_callable(arg1) {
        (undefined_value(), Some(arg1))
    } else {
        let listener = optional_listener(arg2);
        (arg1, listener)
    }
}

/// `fs.watch(path[, options][, listener])` — polling-backed watcher.
#[no_mangle]
pub extern "C" fn js_fs_watch(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let (options_value, listener) = normalized_watch_args(arg1, arg2);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let encoding = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let recursive = option_bool_default_local(options_value, b"recursive", false);
    let signal = match option_signal_value(options_value) {
        Ok(signal) => signal,
        Err(err) => crate::exception::js_throw(err),
    };
    let snapshot = match snapshot_watch_target(&path, recursive) {
        Ok(snapshot) => snapshot,
        Err(err) => unsafe {
            crate::exception::js_throw(build_fs_error_value(&err, "watch", &path));
        },
    };
    let id = next_watch_id();
    let object_value = build_fs_watcher_object(id);
    let timer_callback = poll_closure_value(fs_watcher_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, FS_WATCH_POLL_INTERVAL_MS);
    if !persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    let abort_listener = signal
        .map(|signal| add_abort_listener(signal, id, fs_watcher_abort_impl))
        .unwrap_or_else(undefined_value);
    let signal_value = signal.unwrap_or_else(undefined_value);
    let mut listeners = HashMap::new();
    if let Some(listener) = listener {
        add_listener(&mut listeners, "change".to_string(), listener, false);
    }
    FS_WATCHERS.with(|watchers| {
        watchers.borrow_mut().insert(
            id,
            FsWatchState {
                path,
                recursive,
                encoding,
                object_value,
                timer_id,
                snapshot,
                listeners,
                signal: signal_value,
                abort_listener,
            },
        );
    });
    if signal.map(signal_is_aborted).unwrap_or(false) {
        close_fs_watcher(id);
    }
    object_value
}

/// `fs.watchFile(path[, options], listener)` — stat-polling watcher.
#[no_mangle]
pub extern "C" fn js_fs_watch_file(path_value: f64, arg1: f64, arg2: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let (options_value, listener) = if is_callable(arg1) {
        (undefined_value(), arg1)
    } else {
        validate_listener(arg2);
        (arg1, arg2)
    };
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    if let Some(existing_id) = WATCH_FILE_PATHS.with(|paths| paths.borrow().get(&path).copied()) {
        WATCH_FILE_STATES.with(|states| {
            if let Some(state) = states.borrow_mut().get_mut(&existing_id) {
                add_listener(&mut state.listeners, "change".to_string(), listener, false);
            }
        });
        return WATCH_FILE_STATES.with(|states| {
            states
                .borrow()
                .get(&existing_id)
                .map(|state| state.object_value)
                .unwrap_or_else(undefined_value)
        });
    }
    let id = next_watch_id();
    let object_value = build_stat_watcher_object(id);
    let interval = option_interval_ms(options_value);
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let bigint = unsafe { options_bool_field(options_value, b"bigint") };
    let timer_callback = poll_closure_value(watch_file_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, interval);
    if !persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    let mut listeners = HashMap::new();
    add_listener(&mut listeners, "change".to_string(), listener, false);
    WATCH_FILE_STATES.with(|states| {
        states.borrow_mut().insert(
            id,
            WatchFileState {
                path: path.clone(),
                object_value,
                timer_id,
                bigint,
                previous: stat_snapshot(&path),
                listeners,
            },
        );
    });
    WATCH_FILE_PATHS.with(|paths| {
        paths.borrow_mut().insert(path, id);
    });
    object_value
}

/// `fs.unwatchFile(path[, listener])`.
#[no_mangle]
pub extern "C" fn js_fs_unwatch_file(path_value: f64, listener: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let Some(id) = WATCH_FILE_PATHS.with(|paths| paths.borrow().get(&path).copied()) else {
        return undefined_value();
    };
    if is_nullish(listener) {
        close_watch_file_state(id);
        return undefined_value();
    }
    validate_listener(listener);
    let should_close = WATCH_FILE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            return false;
        };
        remove_listener(&mut state.listeners, "change", listener);
        !has_change_listeners(&state.listeners)
    });
    if should_close {
        close_watch_file_state(id);
    }
    undefined_value()
}

pub extern "C" fn js_fs_promises_watch(path_value: f64, options_value: f64) -> f64 {
    validate::validate_path("filename", path_value);
    let path = unsafe {
        decode_path_value(path_value)
            .unwrap_or_else(|| validate::throw_invalid_path_arg("filename", path_value))
    };
    let encoding = fs_encoding_option(options_value).unwrap_or_else(|| "utf8".to_string());
    let persistent = option_bool_default_local(options_value, b"persistent", true);
    let recursive = option_bool_default_local(options_value, b"recursive", false);
    let signal = match option_signal_value(options_value) {
        Ok(signal) => signal,
        Err(err) => crate::exception::js_throw(err),
    };
    let snapshot = match snapshot_watch_target(&path, recursive) {
        Ok(snapshot) => snapshot,
        Err(err) => unsafe {
            crate::exception::js_throw(build_fs_error_value(&err, "watch", &path));
        },
    };
    let id = next_watch_id();
    let object_value = build_promise_watcher_object(id);
    let timer_callback = poll_closure_value(promise_watcher_poll_impl as *const u8, id);
    let timer_id = crate::timer::setInterval(timer_callback as i64, FS_WATCH_POLL_INTERVAL_MS);
    if !persistent {
        crate::timer::js_timer_unref(timer_id);
    }
    let abort_listener = signal
        .filter(|signal| !signal_is_aborted(*signal))
        .map(|signal| add_abort_listener(signal, id, promise_watcher_abort_impl))
        .unwrap_or_else(undefined_value);
    let signal_value = signal.unwrap_or_else(undefined_value);
    let abort_reason = if signal.map(signal_is_aborted).unwrap_or(false) {
        Some(signal_abort_reason(signal_value))
    } else {
        None
    };
    PROMISE_WATCHERS.with(|watchers| {
        watchers.borrow_mut().insert(
            id,
            PromiseWatchState {
                path,
                recursive,
                encoding,
                object_value,
                timer_id,
                snapshot,
                queue: VecDeque::new(),
                pending: VecDeque::new(),
                signal: signal_value,
                abort_listener,
                closed: abort_reason.is_some(),
                abort_reason,
            },
        );
    });
    if abort_reason.is_some() {
        crate::timer::clearInterval(timer_id);
    }
    object_value
}

pub(crate) fn scan_fs_watcher_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    FS_WATCHERS.with(|watchers| {
        for state in watchers.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            visitor.visit_nanbox_f64_slot(&mut state.signal);
            visitor.visit_nanbox_f64_slot(&mut state.abort_listener);
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
        }
    });
    WATCH_FILE_STATES.with(|states| {
        for state in states.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            for listeners in state.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_f64_slot(&mut listener.callback);
                }
            }
        }
    });
    PROMISE_WATCHERS.with(|watchers| {
        for state in watchers.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.object_value);
            visitor.visit_nanbox_f64_slot(&mut state.signal);
            visitor.visit_nanbox_f64_slot(&mut state.abort_listener);
            if let Some(reason) = &mut state.abort_reason {
                visitor.visit_nanbox_f64_slot(reason);
            }
            for promise in state.pending.iter_mut() {
                visitor.visit_raw_mut_ptr_slot(promise);
            }
        }
    });
}

pub(crate) fn promise_value_fs(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

pub(crate) fn promise_undefined_fs() -> f64 {
    promise_value_fs(f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) fn promise_rejected_fs(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_reject(promise, reason);
    f64::from_bits(crate::value::JSValue::pointer(promise as *const u8).bits())
}

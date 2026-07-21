use super::*;
// Disambiguate from the private `crate::fs::string_value` pulled in by the
// `use crate::fs::*` glob below — this module wants the trunk's
// `(&[u8]) -> f64` helper.
use super::string_value;
// See the note in `opendir.rs`: the parent `fs` module's helpers are globbed in
// directly here (we are a grandchild of `fs`); the two private-to-`fs/mod.rs`
// helpers are named explicitly so a glob that skips privates can't drop them.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(feature = "regex-engine")]
use std::path::PathBuf;

use crate::closure::ClosureHeader;

/// Compiled exclude-pattern type for `fs.glob`. Backed by `fancy_regex::Regex`.
/// Only referenced by the regex-engine-gated glob machinery (`FsGlobOptions`),
/// so it's defined only when that engine is linked.
#[cfg(feature = "regex-engine")]
type GlobExcludeRegex = fancy_regex::Regex;

#[derive(Clone)]
pub(crate) struct FsGlobMatch {
    output: String,
    // Only consulted by the regex-engine-gated exclude-pattern filter; with the
    // engine off no `FsGlobMatch` is ever built, so the field is absent.
    #[cfg(feature = "regex-engine")]
    actual_path: String,
    dirent_name: String,
    dirent_parent: String,
    kind: DirentKind,
}

pub(crate) struct FsGlobRun {
    pub(crate) matches: Vec<FsGlobMatch>,
    pub(crate) with_file_types: bool,
}

#[cfg(feature = "regex-engine")]
struct FsGlobOptions {
    cwd_actual: String,
    cwd_display: String,
    with_file_types: bool,
    follow_symlinks: bool,
    exclude_patterns: Vec<GlobExcludeRegex>,
    exclude_fn: Option<*const ClosureHeader>,
}

#[cfg(feature = "regex-engine")]
struct GlobCandidate {
    actual_path: String,
    kind: DirentKind,
}

#[cfg(feature = "regex-engine")]
fn pathbuf_to_slashes(path: PathBuf) -> String {
    normalize_slashes(&path.to_string_lossy())
}

#[cfg(feature = "regex-engine")]
fn current_dir_slashes() -> String {
    std::env::current_dir()
        .map(pathbuf_to_slashes)
        .unwrap_or_else(|_| ".".to_string())
}

#[cfg(feature = "regex-engine")]
fn trim_trailing_slashes(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        path
    } else {
        trimmed
    }
}

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
fn absolutize_slash(path: &str) -> String {
    let normalized = normalize_slashes(path);
    if Path::new(&normalized).is_absolute() {
        normalized
    } else {
        join_slash(&current_dir_slashes(), &normalized)
    }
}

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
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
    unsafe {
        crate::fs::validate::validate_file_url_path_object(obj);
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

fn glob_pattern_string_error(arg_name: &str, value: f64) -> f64 {
    let message = format!(
        "The \"{arg_name}\" argument must be of type string. Received {}",
        validate::describe_received(value)
    );
    validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn glob_patterns_array_error(value: f64) -> f64 {
    let message = format!(
        "The \"patterns\" argument must be an instance of Array. Received {}",
        validate::describe_received(value)
    );
    validate::build_type_error_with_code_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn glob_patterns_from_value_result(pattern_value: f64) -> Result<Vec<String>, f64> {
    if let Some(pattern) = decode_string_value(pattern_value) {
        return Ok(vec![normalize_slashes(&pattern)]);
    }
    if let Some(arr) = array_ptr_from_value(pattern_value) {
        let len = crate::array::js_array_length(arr) as usize;
        let mut patterns = Vec::with_capacity(len);
        for i in 0..len {
            let value = crate::array::js_array_get_f64(arr, i as u32);
            let Some(pattern) = decode_string_value(value) else {
                return Err(glob_pattern_string_error(&format!("patterns[{i}]"), value));
            };
            patterns.push(normalize_slashes(&pattern));
        }
        return Ok(patterns);
    }
    let js = crate::value::JSValue::from_bits(pattern_value.to_bits());
    if js.is_null() || js.is_pointer() {
        return Err(glob_patterns_array_error(pattern_value));
    }
    Err(glob_pattern_string_error("patterns", pattern_value))
}

#[cfg(feature = "regex-engine")]
fn compile_exclude_patterns_result(
    exclude_value: f64,
    cwd_actual: &str,
) -> Result<Vec<GlobExcludeRegex>, f64> {
    let Some(arr) = array_ptr_from_value(exclude_value) else {
        let message = format!(
            "The \"options.exclude\" property must be of type function or string[]. Received {}",
            validate::describe_received(exclude_value)
        );
        return Err(validate::build_type_error_with_code_value(
            &message,
            "ERR_INVALID_ARG_TYPE",
        ));
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
            return Err(validate::build_type_error_with_code_value(
                &message,
                "ERR_INVALID_ARG_TYPE",
            ));
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
    Ok(patterns)
}

#[cfg(feature = "regex-engine")]
fn glob_options_from_value_result(options_value: f64) -> Result<FsGlobOptions, f64> {
    if let Some(err) = validate::object_options_type_error_value("options", options_value) {
        return Err(err);
    }
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
                    return Err(validate::build_type_error_with_code_value(
                        &message,
                        "ERR_INVALID_ARG_TYPE",
                    ));
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
                    exclude_patterns = compile_exclude_patterns_result(exclude_value, &cwd_actual)?;
                } else {
                    exclude_fn = Some(callable);
                }
            }
        }
    }
    Ok(FsGlobOptions {
        cwd_actual,
        cwd_display,
        with_file_types,
        follow_symlinks,
        exclude_patterns,
        exclude_fn,
    })
}

#[cfg(feature = "regex-engine")]
fn regex_escape_char(out: &mut String, ch: char) {
    if matches!(
        ch,
        '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\'
    ) {
        out.push('\\');
    }
    out.push(ch);
}

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
fn glob_fragment_to_regex(pattern: &str) -> Option<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut pos = 0usize;
    parse_glob_chars(&chars, &mut pos)
}

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
pub(crate) fn glob_regex_from_pattern(pattern: &str) -> Option<fancy_regex::Regex> {
    let normalized = normalize_slashes(pattern);
    let body = glob_fragment_to_regex(&normalized)?;
    fancy_regex::Regex::new(&format!("^{body}$")).ok()
}

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
fn walk_paths_for_glob(dir: &Path, follow_symlinks: bool, out: &mut Vec<GlobCandidate>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|a| a.path());
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

#[cfg(feature = "regex-engine")]
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

#[cfg(feature = "regex-engine")]
fn excluded_by_patterns(path: &str, options: &FsGlobOptions) -> bool {
    options
        .exclude_patterns
        .iter()
        .any(|re| re.is_match(path).unwrap_or(false))
}

#[cfg(feature = "regex-engine")]
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

pub(crate) fn glob_entry_value(entry: &FsGlobMatch, with_file_types: bool) -> f64 {
    if with_file_types {
        unsafe { build_dirent_object(&entry.dirent_name, &entry.dirent_parent, entry.kind) }
    } else {
        string_value(entry.output.as_bytes())
    }
}

#[cfg(feature = "regex-engine")]
pub(crate) fn run_fs_glob_result(pattern_value: f64, options_value: f64) -> Result<FsGlobRun, f64> {
    let patterns = glob_patterns_from_value_result(pattern_value)?;
    let options = glob_options_from_value_result(options_value)?;
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
    Ok(FsGlobRun {
        matches: matches.into_values().collect(),
        with_file_types: options.with_file_types,
    })
}

/// Regex engine gated off: `fs.glob*` matching is built on the regex engine, so
/// with it absent return no matches. The pattern argument is still validated so
/// bad-input `TypeError`s are preserved; the empty-result path is dead in
/// practice (a program calling `fs.globSync` forces the engine on).
#[cfg(not(feature = "regex-engine"))]
pub(crate) fn run_fs_glob_result(
    pattern_value: f64,
    _options_value: f64,
) -> Result<FsGlobRun, f64> {
    glob_patterns_from_value_result(pattern_value)?;
    Ok(FsGlobRun {
        matches: Vec::new(),
        with_file_types: false,
    })
}

fn run_fs_glob(pattern_value: f64, options_value: f64) -> FsGlobRun {
    match run_fs_glob_result(pattern_value, options_value) {
        Ok(run) => run,
        Err(err) => crate::exception::js_throw(err),
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

//! Bun's small CLI/data utility surface (#9600).
//!
//! Object-valued exports (`YAML`, `TOML`, `semver`, and `JSONL`) are ordinary
//! Perry objects containing native closures. This deliberately keeps their
//! nested method calls on the normal dynamic-property path, while the direct
//! exports use the native module call table.

use super::{
    bool_value, boxed_str, is_string_value, is_undefined_or_null, key_ptr, object_field,
    payload_bytes, promise_rejected, promise_value, value_to_string,
};
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::gc::{RootedValues, RuntimeHandle, RuntimeHandleScope};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;
use std::cmp::Ordering;

fn null_value() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn error_value(message: &str) -> f64 {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_error_new_with_message(message);
    f64::from_bits(JSValue::pointer(error as *const u8).bits())
}

fn syntax_error_value(message: &str) -> f64 {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_syntaxerror_new(message);
    f64::from_bits(JSValue::pointer(error as *const u8).bits())
}

fn throw_type_error(message: &str) -> ! {
    let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_typeerror_new(message);
    crate::exception::js_throw(f64::from_bits(JSValue::pointer(error as *const u8).bits()))
}

fn number_arg(value: f64) -> Option<f64> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_int32() {
        Some(value.as_int32() as f64)
    } else if value.is_number() {
        Some(value.as_number())
    } else {
        None
    }
}

fn closure1(name: &str, func: extern "C" fn(*const ClosureHeader, f64) -> f64) -> f64 {
    js_register_closure_arity(func as *const u8, 1);
    let closure = js_closure_alloc(func as *const u8, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, 1);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn closure2(
    name: &str,
    func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
    length: u32,
) -> f64 {
    js_register_closure_arity(func as *const u8, 2);
    let closure = js_closure_alloc(func as *const u8, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn closure3(
    name: &str,
    func: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64,
    length: u32,
) -> f64 {
    js_register_closure_arity(func as *const u8, 3);
    let closure = js_closure_alloc(func as *const u8, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn rooted_object_ptr(handle: &RuntimeHandle<'_>) -> *mut crate::object::ObjectHeader {
    JSValue::from_bits(handle.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>() as *mut _
}

/// Store a property while keeping both the target and value current if key
/// allocation triggers a moving collection.
fn set_rooted_field(
    scope: &RuntimeHandleScope,
    object: &RuntimeHandle<'_>,
    key: &[u8],
    value: f64,
) {
    let value = scope.root_nanbox_f64(value);
    let key = key_ptr(key);
    js_object_set_field_by_name(rooted_object_ptr(object), key, value.get_nanbox_f64());
}

fn namespace_object(fields: &[(&[u8], f64)]) -> f64 {
    let scope = RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(f64::from_bits(
        JSValue::pointer(js_object_alloc(0, fields.len() as u32) as *const u8).bits(),
    ));
    for (key, value) in fields {
        set_rooted_field(&scope, &object, key, *value);
    }
    object.get_nanbox_f64()
}

// ---------------------------------------------------------------------------
// Object-valued exports
// ---------------------------------------------------------------------------

extern "C" fn yaml_parse_closure(_closure: *const ClosureHeader, input: f64) -> f64 {
    yaml_parse(input)
}

extern "C" fn yaml_stringify_closure(
    _closure: *const ClosureHeader,
    input: f64,
    replacer: f64,
    space: f64,
) -> f64 {
    yaml_stringify(input, replacer, space)
}

pub fn js_bun_yaml() -> f64 {
    namespace_object(&[
        (b"parse", closure1("parse", yaml_parse_closure)),
        (
            b"stringify",
            closure3("stringify", yaml_stringify_closure, 1),
        ),
    ])
}

extern "C" fn toml_parse_closure(_closure: *const ClosureHeader, input: f64) -> f64 {
    let source = value_to_string(input);
    // `Value::from_str` in toml 1.x parses a single TOML value expression;
    // Bun.TOML.parse consumes a complete document, whose root is a table.
    let parsed = match toml::from_str::<toml::Table>(&source) {
        Ok(parsed) => parsed,
        Err(error) => crate::exception::js_throw(syntax_error_value(&format!(
            "Failed to parse TOML: {error}"
        ))),
    };
    let json = match serde_json::to_string(&parsed) {
        Ok(json) => json,
        Err(error) => crate::exception::js_throw(syntax_error_value(&format!(
            "Failed to convert TOML value: {error}"
        ))),
    };
    let source = js_string_from_bytes(json.as_ptr(), json.len() as u32);
    match unsafe { crate::json::js_json_parse_result(source) } {
        Ok(value) => f64::from_bits(value.bits()),
        Err(error) => crate::exception::js_throw(error),
    }
}

pub fn js_bun_toml() -> f64 {
    namespace_object(&[(b"parse", closure1("parse", toml_parse_closure))])
}

fn normalize_semver_version(input: &str) -> &str {
    input
        .trim()
        .strip_prefix('v')
        .or_else(|| input.trim().strip_prefix('='))
        .unwrap_or_else(|| input.trim())
}

extern "C" fn semver_order_closure(_closure: *const ClosureHeader, left: f64, right: f64) -> f64 {
    let left_source = value_to_string(left);
    let right_source = value_to_string(right);
    let left = match node_semver::Version::parse(normalize_semver_version(&left_source)) {
        Ok(version) => version,
        Err(_) => crate::exception::js_throw(error_value(&format!(
            "Invalid SemVer: {}",
            left_source.trim()
        ))),
    };
    let right = match node_semver::Version::parse(normalize_semver_version(&right_source)) {
        Ok(version) => version,
        Err(_) => crate::exception::js_throw(error_value(&format!(
            "Invalid SemVer: {}",
            right_source.trim()
        ))),
    };
    match left.cmp(&right) {
        Ordering::Less => -1.0,
        Ordering::Equal => 0.0,
        Ordering::Greater => 1.0,
    }
}

extern "C" fn semver_satisfies_closure(
    _closure: *const ClosureHeader,
    version: f64,
    range: f64,
) -> f64 {
    let version = value_to_string(version);
    let range = value_to_string(range);
    let Ok(version) = node_semver::Version::parse(normalize_semver_version(&version)) else {
        return bool_value(false);
    };
    let range = range.trim();
    if range.is_empty() || range == "*" || range.eq_ignore_ascii_case("latest") {
        return bool_value(!version.is_prerelease());
    }
    let satisfied = node_semver::Range::parse(range)
        .map(|range| range.satisfies(&version))
        .unwrap_or(false);
    bool_value(satisfied)
}

pub fn js_bun_semver() -> f64 {
    namespace_object(&[
        (b"order", closure2("order", semver_order_closure, 2)),
        (
            b"satisfies",
            closure2("satisfies", semver_satisfies_closure, 2),
        ),
    ])
}

extern "C" fn jsonl_parse_chunk_closure(
    _closure: *const ClosureHeader,
    input: f64,
    start: f64,
    end: f64,
) -> f64 {
    jsonl_parse_chunk(input, start, end)
}

pub fn js_bun_jsonl() -> f64 {
    namespace_object(&[(
        b"parseChunk",
        closure3("parseChunk", jsonl_parse_chunk_closure, 1),
    )])
}

// ---------------------------------------------------------------------------
// Equality, terminal strings, executable lookup
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_bun_deep_equals(left: f64, right: f64, strict: f64) -> f64 {
    if !is_undefined_or_null(strict) && crate::value::js_is_truthy(strict) != 0 {
        crate::builtins::js_util_is_deep_strict_equal(left, right)
    } else {
        crate::builtins::js_util_is_deep_strict_equal_skip_prototype(left, right)
    }
}

fn strip_ansi_text(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] != '\u{1b}' {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        if index + 1 >= chars.len() {
            break;
        }
        match chars[index + 1] {
            '[' => {
                index += 2;
                while index < chars.len() {
                    let final_byte = chars[index] as u32;
                    index += 1;
                    if (0x40..=0x7e).contains(&final_byte) {
                        break;
                    }
                }
            }
            ']' => {
                index += 2;
                while index < chars.len() {
                    if matches!(chars[index], '\u{7}' | '\u{9c}') {
                        index += 1;
                        break;
                    }
                    if chars[index] == '\u{1b}'
                        && index + 1 < chars.len()
                        && chars[index + 1] == '\\'
                    {
                        index += 2;
                        break;
                    }
                    index += 1;
                }
            }
            _ => index += 1,
        }
    }
    out
}

#[no_mangle]
pub extern "C" fn js_bun_strip_ansi(input: f64) -> f64 {
    boxed_str(strip_ansi_text(&value_to_string(input)).as_bytes())
}

fn visible_width(input: &str, ambiguous_is_narrow: bool) -> usize {
    let cps: Vec<u32> = input.chars().map(u32::from).collect();
    super::bun_string_width(&cps, false, ambiguous_is_narrow)
}

/// Split at display columns without cutting a CSI/OSC escape sequence.
fn hard_wrap_word(input: &str, columns: usize, ambiguous_is_narrow: bool) -> Vec<String> {
    if columns == 0 || visible_width(input, ambiguous_is_narrow) <= columns {
        return vec![input.to_string()];
    }
    let chars: Vec<char> = input.chars().collect();
    let mut chunks = vec![String::new()];
    let mut width = 0usize;
    let mut index = 0usize;
    while index < chars.len() {
        if chars[index] == '\u{1b}' {
            let start = index;
            index += 1;
            if index < chars.len() && chars[index] == '[' {
                index += 1;
                while index < chars.len() {
                    let c = chars[index] as u32;
                    index += 1;
                    if (0x40..=0x7e).contains(&c) {
                        break;
                    }
                }
            } else if index < chars.len() && chars[index] == ']' {
                index += 1;
                while index < chars.len() {
                    let c = chars[index];
                    index += 1;
                    if matches!(c, '\u{7}' | '\u{9c}') {
                        break;
                    }
                    if c == '\u{1b}' && index < chars.len() && chars[index] == '\\' {
                        index += 1;
                        break;
                    }
                }
            }
            chunks.last_mut().unwrap().extend(&chars[start..index]);
            continue;
        }
        let char_width = visible_width(&chars[index].to_string(), ambiguous_is_narrow);
        if width > 0 && width + char_width > columns {
            chunks.push(String::new());
            width = 0;
        }
        chunks.last_mut().unwrap().push(chars[index]);
        width += char_width;
        index += 1;
    }
    chunks
}

fn wrap_word_paragraph(
    input: &str,
    columns: usize,
    hard: bool,
    trim: bool,
    ambiguous_is_narrow: bool,
) -> String {
    if input.is_empty() {
        return String::new();
    }
    if !trim {
        let mut out = String::new();
        let mut width = 0usize;
        for chunk in hard_wrap_word(input, columns, ambiguous_is_narrow) {
            let chunk_width = visible_width(&chunk, ambiguous_is_narrow);
            if width > 0 && width + chunk_width > columns {
                out.push('\n');
                width = 0;
            }
            out.push_str(&chunk);
            width += chunk_width;
        }
        return out;
    }

    let words: Vec<&str> = input.split_whitespace().collect();
    let mut out = String::new();
    let mut line_width = 0usize;
    for word in words {
        let pieces = if hard {
            hard_wrap_word(word, columns, ambiguous_is_narrow)
        } else {
            vec![word.to_string()]
        };
        for (piece_index, piece) in pieces.into_iter().enumerate() {
            let piece_width = visible_width(&piece, ambiguous_is_narrow);
            let separator = usize::from(line_width > 0 && piece_index == 0);
            if line_width > 0 && line_width + separator + piece_width > columns {
                out.push('\n');
                line_width = 0;
            } else if separator != 0 {
                out.push(' ');
                line_width += 1;
            }
            out.push_str(&piece);
            line_width += piece_width;
        }
    }
    out
}

fn wrap_columns_paragraph(
    input: &str,
    columns: usize,
    trim: bool,
    ambiguous_is_narrow: bool,
) -> String {
    let source = if trim { input.trim() } else { input };
    hard_wrap_word(source, columns, ambiguous_is_narrow).join("\n")
}

#[no_mangle]
pub extern "C" fn js_bun_wrap_ansi(input: f64, columns: f64, options: f64) -> f64 {
    let input = value_to_string(input);
    let columns = number_arg(columns).unwrap_or(0.0).max(0.0) as usize;
    if columns == 0 {
        return boxed_str(input.as_bytes());
    }
    let hard =
        object_field(options, b"hard").is_some_and(|value| crate::value::js_is_truthy(value) != 0);
    let trim = object_field(options, b"trim")
        .map(|value| crate::value::js_is_truthy(value) != 0)
        .unwrap_or(true);
    let word_wrap = object_field(options, b"wordWrap")
        .map(|value| crate::value::js_is_truthy(value) != 0)
        .unwrap_or(true);
    let ambiguous_is_narrow = object_field(options, b"ambiguousIsNarrow")
        .map(|value| crate::value::js_is_truthy(value) != 0)
        .unwrap_or(true);
    let output = input
        .split('\n')
        .map(|paragraph| {
            if word_wrap {
                wrap_word_paragraph(paragraph, columns, hard, trim, ambiguous_is_narrow)
            } else {
                wrap_columns_paragraph(paragraph, columns, trim, ambiguous_is_narrow)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    boxed_str(output.as_bytes())
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

#[no_mangle]
pub extern "C" fn js_bun_which(command: f64, options: f64) -> f64 {
    let command = value_to_string(command);
    if command.is_empty() {
        return null_value();
    }
    let command_path = std::path::Path::new(&command);
    if command_path.components().count() > 1 {
        return if is_executable(command_path) {
            boxed_str(command.as_bytes())
        } else {
            null_value()
        };
    }

    let path = object_field(options, b"PATH")
        .or_else(|| object_field(options, b"path"))
        .map(value_to_string)
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    #[cfg(windows)]
    let extensions: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(str::to_string)
        .collect();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(&command);
        if is_executable(&candidate) {
            return boxed_str(candidate.to_string_lossy().as_bytes());
        }
        #[cfg(windows)]
        for extension in &extensions {
            let candidate = directory.join(format!("{command}{extension}"));
            if is_executable(&candidate) {
                return boxed_str(candidate.to_string_lossy().as_bytes());
            }
        }
    }
    null_value()
}

// ---------------------------------------------------------------------------
// zstd, xxHash64, GC and heap snapshots
// ---------------------------------------------------------------------------

fn zstd_decode(input: f64) -> Result<f64, f64> {
    let bytes = payload_bytes(input)?;
    match zstd::stream::decode_all(bytes.as_slice()) {
        Ok(decoded) => Ok(crate::node_submodules::consumers::bytes_to_buffer_value(
            &decoded,
        )),
        Err(_) => Err(error_value("Decompression failed: InvalidZstdData")),
    }
}

#[no_mangle]
pub extern "C" fn js_bun_zstd_decompress_sync(input: f64) -> f64 {
    match zstd_decode(input) {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

#[no_mangle]
pub extern "C" fn js_bun_zstd_decompress(input: f64) -> f64 {
    match zstd_decode(input) {
        Ok(value) => promise_value(value),
        Err(error) => promise_rejected(error),
    }
}

extern "C" fn xxhash64_closure(_closure: *const ClosureHeader, input: f64, seed: f64) -> f64 {
    let bytes = payload_bytes(input).unwrap_or_default();
    let hash = xxhash_rust::xxh64::xxh64(&bytes, super::hash_seed(seed));
    let bigint = crate::bigint::js_bigint_from_u64(hash);
    crate::value::js_nanbox_bigint(bigint as i64)
}

pub fn decorate_bun_hash(value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let hash = scope.root_nanbox_f64(value);
    let xxhash = scope.root_nanbox_f64(closure2("xxHash64", xxhash64_closure, 1));
    let raw = JSValue::from_bits(hash.get_nanbox_f64().to_bits()).as_pointer::<u8>() as usize;
    crate::closure::closure_set_dynamic_prop(raw, "xxHash64", xxhash.get_nanbox_f64());
    hash.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_bun_gc(_full: f64) -> f64 {
    crate::gc::js_gc_collect();
    undefined()
}

#[no_mangle]
pub extern "C" fn js_bun_generate_heap_snapshot(format: f64, encoding: f64) -> f64 {
    if is_undefined_or_null(format) {
        throw_type_error(
            "Bun.generateHeapSnapshot() uses JavaScriptCore's snapshot object; Perry supports the explicit 'v8' format",
        );
    }
    if value_to_string(format) != "v8" {
        throw_type_error("Bun.generateHeapSnapshot format must be 'v8'");
    }
    #[cfg(feature = "diagnostics")]
    {
        let json = crate::gc::gc_build_v8_heap_snapshot_json();
        if !is_undefined_or_null(encoding) && value_to_string(encoding) == "arraybuffer" {
            return crate::node_submodules::consumers::bytes_to_array_buffer_value(json.as_bytes());
        }
        if !is_undefined_or_null(encoding) && value_to_string(encoding) != "string" {
            throw_type_error("Bun.generateHeapSnapshot encoding must be 'string' or 'arraybuffer'");
        }
        boxed_str(json.as_bytes())
    }
    #[cfg(not(feature = "diagnostics"))]
    {
        let _ = encoding;
        throw_type_error("Heap snapshot diagnostics are not enabled in this Perry runtime")
    }
}

// ---------------------------------------------------------------------------
// Bun.JSONL.parseChunk
// ---------------------------------------------------------------------------

fn jsonl_parse_chunk(input: f64, start: f64, end: f64) -> f64 {
    let (bytes, string_input) = if is_string_value(input) {
        (value_to_string(input).into_bytes(), true)
    } else {
        match payload_bytes(input) {
            Ok(bytes) => (bytes, false),
            Err(error) => crate::exception::js_throw(error),
        }
    };
    let mut cursor = number_arg(start)
        .unwrap_or(0.0)
        .max(0.0)
        .min(bytes.len() as f64) as usize;
    let limit = number_arg(end)
        .unwrap_or(bytes.len() as f64)
        .max(cursor as f64)
        .min(bytes.len() as f64) as usize;
    let initial = cursor;
    let mut spans = Vec::<(usize, usize)>::new();
    let mut last_read = initial;
    let mut done = true;
    let mut syntax_error = false;

    while cursor < limit {
        let line_end = bytes[cursor..limit]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| cursor + offset)
            .unwrap_or(limit);
        let mut content_start = cursor;
        let mut content_end = line_end;
        while content_start < content_end && bytes[content_start].is_ascii_whitespace() {
            content_start += 1;
        }
        while content_end > content_start && bytes[content_end - 1].is_ascii_whitespace() {
            content_end -= 1;
        }
        if content_start == content_end {
            cursor = if line_end < limit {
                line_end + 1
            } else {
                limit
            };
            continue;
        }

        match serde_json::from_slice::<serde_json::Value>(&bytes[content_start..content_end]) {
            Ok(_) => {
                spans.push((content_start, content_end));
                // Bun reports an absolute character/byte offset immediately
                // after the final parsed JSON token, excluding the delimiter.
                last_read = if string_input {
                    String::from_utf8_lossy(&bytes[..content_end])
                        .encode_utf16()
                        .count()
                } else {
                    content_end
                };
                cursor = if line_end < limit {
                    line_end + 1
                } else {
                    limit
                };
            }
            Err(error) if error.is_eof() => {
                done = false;
                break;
            }
            Err(_) => {
                done = false;
                syntax_error = true;
                break;
            }
        }
    }

    let scope = RuntimeHandleScope::new();
    let values = crate::array::js_array_alloc_with_length(spans.len() as u32);
    let values =
        scope.root_nanbox_f64(f64::from_bits(JSValue::pointer(values as *const u8).bits()));
    for (index, (start, end)) in spans.into_iter().enumerate() {
        let source = js_string_from_bytes(bytes[start..end].as_ptr(), (end - start) as u32);
        let value = match unsafe { crate::json::js_json_parse_result(source) } {
            Ok(value) => f64::from_bits(value.bits()),
            Err(error) => crate::exception::js_throw(error),
        };
        let value = scope.root_nanbox_f64(value);
        let array = JSValue::from_bits(values.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        crate::array::js_array_set_f64(array as *mut _, index as u32, value.get_nanbox_f64());
    }

    let error = if syntax_error {
        syntax_error_value("Failed to parse JSONL")
    } else {
        null_value()
    };
    let error = scope.root_nanbox_f64(error);
    let result = scope.root_nanbox_f64(f64::from_bits(
        JSValue::pointer(js_object_alloc(0, 4) as *const u8).bits(),
    ));
    set_rooted_field(&scope, &result, b"values", values.get_nanbox_f64());
    set_rooted_field(&scope, &result, b"read", last_read as f64);
    set_rooted_field(&scope, &result, b"done", bool_value(done));
    set_rooted_field(&scope, &result, b"error", error.get_nanbox_f64());
    result.get_nanbox_f64()
}

// ---------------------------------------------------------------------------
// Bun.YAML.parse — libyaml node graph -> rooted Perry graph
// ---------------------------------------------------------------------------

unsafe fn yaml_scalar_bytes<'a>(node: *const unsafe_libyaml::yaml_node_t) -> &'a [u8] {
    let scalar = (*node).data.scalar;
    std::slice::from_raw_parts(scalar.value, scalar.length as usize)
}

unsafe fn yaml_scalar_text(node: *const unsafe_libyaml::yaml_node_t) -> String {
    String::from_utf8_lossy(yaml_scalar_bytes(node)).into_owned()
}

fn yaml_plain_number(text: &str) -> Option<f64> {
    let compact = text.replace('_', "");
    let (negative, unsigned) = compact
        .strip_prefix('-')
        .map(|rest| (true, rest))
        .or_else(|| compact.strip_prefix('+').map(|rest| (false, rest)))
        .unwrap_or((false, compact.as_str()));
    let integer = if let Some(value) = unsigned.strip_prefix("0x") {
        i128::from_str_radix(value, 16).ok()
    } else if let Some(value) = unsigned.strip_prefix("0o") {
        i128::from_str_radix(value, 8).ok()
    } else if let Some(value) = unsigned.strip_prefix("0b") {
        i128::from_str_radix(value, 2).ok()
    } else if !unsigned.contains(['.', 'e', 'E']) {
        unsigned.parse::<i128>().ok()
    } else {
        None
    };
    if let Some(integer) = integer {
        let value = integer as f64;
        return Some(if negative { -value } else { value });
    }
    match compact.to_ascii_lowercase().as_str() {
        ".inf" | "+.inf" => Some(f64::INFINITY),
        "-.inf" => Some(f64::NEG_INFINITY),
        ".nan" | "+.nan" | "-.nan" => Some(f64::NAN),
        _ => compact.parse::<f64>().ok(),
    }
}

unsafe fn yaml_scalar_to_js(node: *const unsafe_libyaml::yaml_node_t, source: &[u8]) -> f64 {
    let scalar = (*node).data.scalar;
    let text = yaml_scalar_text(node);
    let start = ((*node).start_mark.index as usize).min(source.len());
    let end = ((*node).end_mark.index as usize).min(source.len());
    let spelling = String::from_utf8_lossy(&source[start..end]);
    let explicitly_string = spelling.trim_start().starts_with("!!str")
        || spelling
            .trim_start()
            .starts_with("!<tag:yaml.org,2002:str>");
    let plain = scalar.style == unsafe_libyaml::YAML_PLAIN_SCALAR_STYLE;
    if plain && !explicitly_string {
        if text.is_empty() || text == "~" || text.eq_ignore_ascii_case("null") {
            return null_value();
        }
        if text.eq_ignore_ascii_case("true") {
            return bool_value(true);
        }
        if text.eq_ignore_ascii_case("false") {
            return bool_value(false);
        }
        if let Some(number) = yaml_plain_number(&text) {
            return number;
        }
    }
    boxed_str(text.as_bytes())
}

unsafe fn yaml_node_count(document: *const unsafe_libyaml::yaml_document_t) -> usize {
    (*document).nodes.top.offset_from((*document).nodes.start) as usize
}

unsafe fn yaml_node(
    document: *const unsafe_libyaml::yaml_document_t,
    id: i32,
) -> *const unsafe_libyaml::yaml_node_t {
    (*document).nodes.start.add(id.saturating_sub(1) as usize)
}

unsafe fn yaml_mapping_key(
    document: *const unsafe_libyaml::yaml_document_t,
    roots: &RootedValues<'_>,
    id: i32,
) -> String {
    let node = yaml_node(document, id);
    if (*node).type_ == unsafe_libyaml::YAML_SCALAR_NODE {
        yaml_scalar_text(node)
    } else {
        value_to_string(roots.get(id.saturating_sub(1) as usize))
    }
}

unsafe fn yaml_set_mapping_pair(
    scope: &RuntimeHandleScope,
    document: *const unsafe_libyaml::yaml_document_t,
    roots: &RootedValues<'_>,
    target_index: usize,
    key_id: i32,
    value_id: i32,
) {
    if key_id <= 0 || value_id <= 0 {
        return;
    }
    let key = yaml_mapping_key(document, roots, key_id);
    let key_ptr = key_ptr(key.as_bytes());
    let target = JSValue::from_bits(roots.get(target_index).to_bits())
        .as_pointer::<crate::object::ObjectHeader>() as *mut _;
    let value = scope.root_nanbox_f64(roots.get(value_id as usize - 1));
    js_object_set_field_by_name(target, key_ptr, value.get_nanbox_f64());
}

unsafe fn yaml_apply_merge(
    scope: &RuntimeHandleScope,
    document: *const unsafe_libyaml::yaml_document_t,
    roots: &RootedValues<'_>,
    target_index: usize,
    source_id: i32,
    depth: usize,
) {
    if source_id <= 0 || depth > 64 {
        return;
    }
    let source = yaml_node(document, source_id);
    if (*source).type_ == unsafe_libyaml::YAML_MAPPING_NODE {
        let pairs = (*source).data.mapping.pairs;
        let count = pairs.top.offset_from(pairs.start) as usize;
        for index in 0..count {
            let pair = *pairs.start.add(index);
            if yaml_mapping_key(document, roots, pair.key) != "<<" {
                yaml_set_mapping_pair(scope, document, roots, target_index, pair.key, pair.value);
            }
        }
    } else if (*source).type_ == unsafe_libyaml::YAML_SEQUENCE_NODE {
        let items = (*source).data.sequence.items;
        let count = items.top.offset_from(items.start) as usize;
        // Earlier merge sources have precedence in YAML. Apply in reverse so
        // their values are the final ones before explicit keys overwrite them.
        for index in (0..count).rev() {
            yaml_apply_merge(
                scope,
                document,
                roots,
                target_index,
                *items.start.add(index),
                depth + 1,
            );
        }
    }
}

unsafe fn yaml_document_to_js(
    document: *mut unsafe_libyaml::yaml_document_t,
    source: &[u8],
    scope: &RuntimeHandleScope,
) -> f64 {
    let count = yaml_node_count(document);
    let mut roots = RootedValues::with_capacity(scope, count);
    for index in 0..count {
        let node = (*document).nodes.start.add(index);
        let value = if (*node).type_ == unsafe_libyaml::YAML_SCALAR_NODE {
            yaml_scalar_to_js(node, source)
        } else if (*node).type_ == unsafe_libyaml::YAML_SEQUENCE_NODE {
            let items = (*node).data.sequence.items;
            let length = items.top.offset_from(items.start).max(0) as u32;
            f64::from_bits(
                JSValue::pointer(crate::array::js_array_alloc_with_length(length) as *const u8)
                    .bits(),
            )
        } else if (*node).type_ == unsafe_libyaml::YAML_MAPPING_NODE {
            let pairs = (*node).data.mapping.pairs;
            let length = pairs.top.offset_from(pairs.start).max(0) as u32;
            f64::from_bits(JSValue::pointer(js_object_alloc(0, length) as *const u8).bits())
        } else {
            null_value()
        };
        roots.push(value);
    }

    for index in 0..count {
        let node = (*document).nodes.start.add(index);
        if (*node).type_ == unsafe_libyaml::YAML_SEQUENCE_NODE {
            let items = (*node).data.sequence.items;
            let length = items.top.offset_from(items.start).max(0) as usize;
            for item_index in 0..length {
                let child_id = *items.start.add(item_index);
                if child_id <= 0 {
                    continue;
                }
                let child = scope.root_nanbox_f64(roots.get(child_id as usize - 1));
                let target = JSValue::from_bits(roots.get(index).to_bits())
                    .as_pointer::<crate::array::ArrayHeader>()
                    as *mut _;
                crate::array::js_array_set_f64(target, item_index as u32, child.get_nanbox_f64());
            }
        } else if (*node).type_ == unsafe_libyaml::YAML_MAPPING_NODE {
            let pairs = (*node).data.mapping.pairs;
            let length = pairs.top.offset_from(pairs.start).max(0) as usize;
            for pair_index in 0..length {
                let pair = *pairs.start.add(pair_index);
                if yaml_mapping_key(document, &roots, pair.key) == "<<" {
                    yaml_apply_merge(scope, document, &roots, index, pair.value, 0);
                }
            }
            for pair_index in 0..length {
                let pair = *pairs.start.add(pair_index);
                if yaml_mapping_key(document, &roots, pair.key) != "<<" {
                    yaml_set_mapping_pair(scope, document, &roots, index, pair.key, pair.value);
                }
            }
        }
    }
    roots.get(0)
}

fn yaml_parse(input: f64) -> f64 {
    let source = value_to_string(input).into_bytes();
    unsafe {
        let mut parser = std::mem::MaybeUninit::<unsafe_libyaml::yaml_parser_t>::uninit();
        if unsafe_libyaml::yaml_parser_initialize(parser.as_mut_ptr()).fail {
            crate::exception::js_throw(syntax_error_value("Failed to initialize YAML parser"));
        }
        let mut parser = parser.assume_init();
        unsafe_libyaml::yaml_parser_set_input_string(
            &mut parser,
            source.as_ptr(),
            source.len() as u64,
        );
        let scope = RuntimeHandleScope::new();
        let mut documents = RootedValues::new(&scope);
        loop {
            let mut document = std::mem::MaybeUninit::<unsafe_libyaml::yaml_document_t>::uninit();
            if unsafe_libyaml::yaml_parser_load(&mut parser, document.as_mut_ptr()).fail {
                let problem = if parser.problem.is_null() {
                    "invalid YAML".to_string()
                } else {
                    std::ffi::CStr::from_ptr(parser.problem.cast::<std::ffi::c_char>())
                        .to_string_lossy()
                        .into_owned()
                };
                let line = parser.problem_mark.line + 1;
                let column = parser.problem_mark.column + 1;
                unsafe_libyaml::yaml_parser_delete(&mut parser);
                crate::exception::js_throw(syntax_error_value(&format!(
                    "Failed to parse YAML at {line}:{column}: {problem}"
                )));
            }
            let mut document = document.assume_init();
            let root = unsafe_libyaml::yaml_document_get_root_node(&mut document);
            if root.is_null() {
                unsafe_libyaml::yaml_document_delete(&mut document);
                break;
            }
            let value = yaml_document_to_js(&mut document, &source, &scope);
            documents.push(value);
            unsafe_libyaml::yaml_document_delete(&mut document);
        }
        unsafe_libyaml::yaml_parser_delete(&mut parser);

        if documents.is_empty() {
            return null_value();
        }
        if documents.len() == 1 {
            return documents.get(0);
        }
        let output = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(
                crate::array::js_array_alloc_with_length(documents.len() as u32) as *const u8,
            )
            .bits(),
        ));
        for index in 0..documents.len() {
            let value = scope.root_nanbox_f64(documents.get(index));
            let array = JSValue::from_bits(output.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>() as *mut _;
            crate::array::js_array_set_f64(array, index as u32, value.get_nanbox_f64());
        }
        output.get_nanbox_f64()
    }
}

// ---------------------------------------------------------------------------
// Bun.YAML.stringify — rooted Perry graph -> libyaml document graph
// ---------------------------------------------------------------------------

const YAML_STR_TAG: &[u8] = b"tag:yaml.org,2002:str\0";
const YAML_INT_TAG: &[u8] = b"tag:yaml.org,2002:int\0";
const YAML_FLOAT_TAG: &[u8] = b"tag:yaml.org,2002:float\0";
const YAML_BOOL_TAG: &[u8] = b"tag:yaml.org,2002:bool\0";
const YAML_NULL_TAG: &[u8] = b"tag:yaml.org,2002:null\0";

unsafe fn yaml_add_scalar(
    document: *mut unsafe_libyaml::yaml_document_t,
    tag: &[u8],
    text: &str,
) -> i32 {
    unsafe_libyaml::yaml_document_add_scalar(
        document,
        tag.as_ptr(),
        text.as_ptr(),
        text.len() as i32,
        unsafe_libyaml::YAML_ANY_SCALAR_STYLE,
    )
}

fn yaml_number_text(value: f64) -> String {
    if value.is_nan() {
        return ".nan".to_string();
    }
    if value == f64::INFINITY {
        return ".inf".to_string();
    }
    if value == f64::NEG_INFINITY {
        return "-.inf".to_string();
    }
    if value == 0.0 && value.is_sign_negative() {
        return "-0".to_string();
    }
    let mut buffer = ryu::Buffer::new();
    buffer.format(value).to_string()
}

unsafe fn yaml_add_js_value(
    document: *mut unsafe_libyaml::yaml_document_t,
    scope: &RuntimeHandleScope,
    roots: &mut RootedValues<'_>,
    value_index: usize,
    memo: &mut Vec<(usize, i32)>,
    flow: bool,
    depth: usize,
) -> i32 {
    if depth > 1_000 {
        throw_type_error("YAML.stringify: value nested deeper than 1000 levels");
    }
    let raw = roots.get(value_index);
    let value = JSValue::from_bits(raw.to_bits());
    if value.is_undefined() {
        return 0;
    }
    if value.is_null() {
        return yaml_add_scalar(document, YAML_NULL_TAG, "null");
    }
    if value.is_bool() {
        return yaml_add_scalar(
            document,
            YAML_BOOL_TAG,
            if value.as_bool() { "true" } else { "false" },
        );
    }
    if value.is_int32() {
        return yaml_add_scalar(document, YAML_INT_TAG, &value.as_int32().to_string());
    }
    if value.is_number() {
        let number = value.as_number();
        let tag = if number.is_finite() && number.fract() == 0.0 {
            YAML_INT_TAG
        } else {
            YAML_FLOAT_TAG
        };
        return yaml_add_scalar(document, tag, &yaml_number_text(number));
    }
    if value.is_string() || value.is_short_string() {
        return yaml_add_scalar(document, YAML_STR_TAG, &value_to_string(raw));
    }
    if crate::value::js_nanbox_is_bigint(raw) != 0 {
        return yaml_add_scalar(document, YAML_INT_TAG, &value_to_string(raw));
    }
    if !value.is_pointer() {
        return 0;
    }
    let address = value.as_pointer::<u8>() as usize;
    if !crate::json::ptr_is_tracked_heap_object(address as *const u8) {
        return 0;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(address) else {
        return 0;
    };
    if !matches!(
        header.obj_type,
        crate::gc::GC_TYPE_ARRAY | crate::gc::GC_TYPE_OBJECT
    ) {
        return 0;
    }
    for (memo_index, node_id) in memo.iter().copied() {
        if roots.get(memo_index).to_bits() == roots.get(value_index).to_bits() {
            return node_id;
        }
    }

    if header.obj_type == crate::gc::GC_TYPE_ARRAY {
        let style = if flow {
            unsafe_libyaml::YAML_FLOW_SEQUENCE_STYLE
        } else {
            unsafe_libyaml::YAML_BLOCK_SEQUENCE_STYLE
        };
        let node_id = unsafe_libyaml::yaml_document_add_sequence(document, std::ptr::null(), style);
        if node_id == 0 {
            return 0;
        }
        memo.push((value_index, node_id));
        let array = roots.get(value_index);
        let array = JSValue::from_bits(array.to_bits()).as_pointer::<crate::array::ArrayHeader>();
        let length = crate::array::js_array_length(array);
        for index in 0..length {
            let array = roots.get(value_index);
            let array =
                JSValue::from_bits(array.to_bits()).as_pointer::<crate::array::ArrayHeader>();
            let child = crate::array::js_array_get_f64(array, index);
            roots.push(child);
            let child_index = roots.len() - 1;
            let child_id =
                yaml_add_js_value(document, scope, roots, child_index, memo, flow, depth + 1);
            if child_id != 0 {
                let _ =
                    unsafe_libyaml::yaml_document_append_sequence_item(document, node_id, child_id);
            }
        }
        return node_id;
    }

    let style = if flow {
        unsafe_libyaml::YAML_FLOW_MAPPING_STYLE
    } else {
        unsafe_libyaml::YAML_BLOCK_MAPPING_STYLE
    };
    let node_id = unsafe_libyaml::yaml_document_add_mapping(document, std::ptr::null(), style);
    if node_id == 0 {
        return 0;
    }
    memo.push((value_index, node_id));
    let object = roots.get(value_index);
    let object = JSValue::from_bits(object.to_bits()).as_pointer::<crate::object::ObjectHeader>();
    let keys = crate::object::js_object_keys(object);
    roots.push(f64::from_bits(JSValue::pointer(keys as *const u8).bits()));
    let keys_index = roots.len() - 1;
    let keys = JSValue::from_bits(roots.get(keys_index).to_bits())
        .as_pointer::<crate::array::ArrayHeader>();
    let length = crate::array::js_array_length(keys);
    for index in 0..length {
        let keys = JSValue::from_bits(roots.get(keys_index).to_bits())
            .as_pointer::<crate::array::ArrayHeader>();
        let key_value = crate::array::js_array_get_f64(keys, index);
        let key = value_to_string(key_value);
        let lookup_key = key_ptr(key.as_bytes());
        let object = JSValue::from_bits(roots.get(value_index).to_bits())
            .as_pointer::<crate::object::ObjectHeader>();
        let child = crate::object::js_object_get_field_by_name_f64(object, lookup_key);
        roots.push(child);
        let child_index = roots.len() - 1;
        let child_id =
            yaml_add_js_value(document, scope, roots, child_index, memo, flow, depth + 1);
        if child_id == 0 {
            continue;
        }
        let key_id = yaml_add_scalar(document, YAML_STR_TAG, &key);
        if key_id != 0 {
            let _ = unsafe_libyaml::yaml_document_append_mapping_pair(
                document, node_id, key_id, child_id,
            );
        }
    }
    let _ = scope;
    node_id
}

unsafe fn yaml_vec_write(
    data: *mut libc::c_void,
    buffer: *mut libc::c_uchar,
    size: u64,
) -> libc::c_int {
    let output = &mut *(data as *mut Vec<u8>);
    output.extend_from_slice(std::slice::from_raw_parts(buffer, size as usize));
    1
}

fn compact_flow_yaml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    while let Some(character) = chars.next() {
        if let Some(active) = quote {
            out.push(character);
            if active == '"' && character == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if character == active && !escaped {
                quote = None;
            }
            escaped = false;
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            out.push(character);
            continue;
        }
        out.push(character);
        if matches!(character, ',' | ':') {
            while chars.peek() == Some(&' ') {
                chars.next();
            }
        }
    }
    out
}

fn yaml_stringify(input: f64, replacer: f64, space: f64) -> f64 {
    let _ = replacer;
    if JSValue::from_bits(input.to_bits()).is_undefined() {
        return undefined();
    }
    let indent = if is_undefined_or_null(space) {
        0
    } else if is_string_value(space) {
        value_to_string(space).chars().count().clamp(1, 9) as i32
    } else {
        number_arg(space).unwrap_or(0.0).clamp(0.0, 9.0) as i32
    };
    let flow = indent == 0;

    unsafe {
        let mut document = std::mem::MaybeUninit::<unsafe_libyaml::yaml_document_t>::uninit();
        if unsafe_libyaml::yaml_document_initialize(
            document.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            true,
            true,
        )
        .fail
        {
            throw_type_error("Failed to initialize YAML document");
        }
        let mut document = document.assume_init();
        let scope = RuntimeHandleScope::new();
        let mut roots = RootedValues::new(&scope);
        roots.push(input);
        let root_id = yaml_add_js_value(
            &mut document,
            &scope,
            &mut roots,
            0,
            &mut Vec::new(),
            flow,
            0,
        );
        if root_id == 0 {
            unsafe_libyaml::yaml_document_delete(&mut document);
            return undefined();
        }

        let mut emitter = std::mem::MaybeUninit::<unsafe_libyaml::yaml_emitter_t>::uninit();
        if unsafe_libyaml::yaml_emitter_initialize(emitter.as_mut_ptr()).fail {
            unsafe_libyaml::yaml_document_delete(&mut document);
            throw_type_error("Failed to initialize YAML emitter");
        }
        let mut emitter = emitter.assume_init();
        let mut output = Vec::<u8>::new();
        unsafe_libyaml::yaml_emitter_set_output(
            &mut emitter,
            yaml_vec_write,
            &mut output as *mut Vec<u8> as *mut libc::c_void,
        );
        unsafe_libyaml::yaml_emitter_set_unicode(&mut emitter, true);
        unsafe_libyaml::yaml_emitter_set_width(&mut emitter, -1);
        if indent > 0 {
            unsafe_libyaml::yaml_emitter_set_indent(&mut emitter, indent.max(2));
        }
        if unsafe_libyaml::yaml_emitter_dump(&mut emitter, &mut document).fail {
            unsafe_libyaml::yaml_emitter_delete(&mut emitter);
            throw_type_error("Failed to emit YAML");
        }
        let _ = unsafe_libyaml::yaml_emitter_close(&mut emitter);
        unsafe_libyaml::yaml_emitter_delete(&mut emitter);

        let mut output = String::from_utf8_lossy(&output).into_owned();
        if let Some(without_end) = output.strip_suffix("...\n") {
            output = without_end.to_string();
        }
        while output.ends_with('\n') {
            output.pop();
        }
        if flow {
            output = compact_flow_yaml(&output);
        }
        boxed_str(output.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_handles_csi_and_osc() {
        assert_eq!(strip_ansi_text("\x1b[31ma\x1b[0mb"), "ab");
        assert_eq!(
            strip_ansi_text("\x1b]8;;https://x\x1b\\link\x1b]8;;\x07"),
            "link"
        );
    }

    #[test]
    fn flow_compaction_does_not_touch_quoted_spaces() {
        assert_eq!(compact_flow_yaml("{a: 1, b: 'x, y'}"), "{a:1,b:'x, y'}");
    }
}

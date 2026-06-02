//! Pragmatic `URLPattern` runtime surface.
//!
//! This models the Node-visible constructor/export and the common component
//! matching path Perry's parity suite exercises. It is not a full WHATWG
//! URLPattern engine.

use super::*;

use super::parse::{is_valid_absolute_url, parse_url, resolve_url};
use crate::object::js_object_set_field_by_name;

#[derive(Clone)]
struct UrlPatternParts {
    protocol: String,
    username: String,
    password: String,
    hostname: String,
    port: String,
    pathname: String,
    search: String,
    hash: String,
}

struct MatchResult {
    inputs: Vec<String>,
    protocol: String,
    username: String,
    password: String,
    hostname: String,
    port: String,
    pathname: String,
    search: String,
    hash: String,
    pathname_groups: Vec<(String, String)>,
}

struct MatchInput {
    parts: UrlPatternParts,
    inputs: Vec<String>,
}

impl UrlPatternParts {
    fn wildcard() -> Self {
        Self {
            protocol: "*".to_string(),
            username: "*".to_string(),
            password: "*".to_string(),
            hostname: "*".to_string(),
            port: "*".to_string(),
            pathname: "*".to_string(),
            search: "*".to_string(),
            hash: "*".to_string(),
        }
    }
}

fn null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn normalize_protocol(value: &str) -> String {
    value
        .strip_suffix(':')
        .unwrap_or(value)
        .to_ascii_lowercase()
}

fn component_value_from_object(obj: *mut ObjectHeader, key: &str) -> Option<String> {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        None
    } else {
        Some(string_from_header(js_url_coerce_string(value)))
    }
}

fn parts_from_object(obj: *mut ObjectHeader) -> UrlPatternParts {
    let mut parts = UrlPatternParts::wildcard();
    if let Some(value) = component_value_from_object(obj, "protocol") {
        parts.protocol = normalize_protocol(&value);
    }
    if let Some(value) = component_value_from_object(obj, "username") {
        parts.username = value;
    }
    if let Some(value) = component_value_from_object(obj, "password") {
        parts.password = value;
    }
    if let Some(value) = component_value_from_object(obj, "hostname") {
        parts.hostname = value.to_ascii_lowercase();
    }
    if let Some(value) = component_value_from_object(obj, "port") {
        parts.port = value;
    }
    if let Some(value) = component_value_from_object(obj, "pathname") {
        parts.pathname = value;
    }
    if let Some(value) = component_value_from_object(obj, "search") {
        parts.search = value.strip_prefix('?').unwrap_or(&value).to_string();
    }
    if let Some(value) = component_value_from_object(obj, "hash") {
        parts.hash = value.strip_prefix('#').unwrap_or(&value).to_string();
    }
    parts
}

fn parts_from_absolute_url(url: &str) -> UrlPatternParts {
    let (protocol, _host, hostname, port, pathname, search, hash) = parse_url(url);
    let mut parts = UrlPatternParts::wildcard();
    parts.protocol = normalize_protocol(&protocol);
    parts.hostname = hostname.to_ascii_lowercase();
    parts.port = if port.is_empty() {
        "*".to_string()
    } else {
        port
    };
    parts.pathname = pathname;
    parts.search = if search.is_empty() {
        "*".to_string()
    } else {
        search.strip_prefix('?').unwrap_or(&search).to_string()
    };
    parts.hash = if hash.is_empty() {
        "*".to_string()
    } else {
        hash.strip_prefix('#').unwrap_or(&hash).to_string()
    };
    parts
}

fn parts_from_string(input: &str, base: Option<&str>) -> Option<UrlPatternParts> {
    let input = input.trim();
    if input.is_empty() {
        return Some(UrlPatternParts::wildcard());
    }
    if input == "*" {
        return None;
    }
    if is_valid_absolute_url(input) {
        return Some(parts_from_absolute_url(input));
    }
    if let Some(base) = base {
        if is_valid_absolute_url(base) {
            if input.starts_with('/') {
                let mut parts = parts_from_absolute_url(base);
                parts.pathname = input.to_string();
                parts.search = "*".to_string();
                parts.hash = "*".to_string();
                return Some(parts);
            }
            let resolved = resolve_url(input, base);
            if is_valid_absolute_url(&resolved) {
                return Some(parts_from_absolute_url(&resolved));
            }
        }
    }
    if input.starts_with('/') {
        let mut parts = UrlPatternParts::wildcard();
        parts.pathname = input.to_string();
        return Some(parts);
    }
    None
}

fn throw_invalid_url_pattern() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "Failed to construct URLPattern",
        "ERR_INVALID_URL_PATTERN",
    )
}

fn set_named(obj: *mut ObjectHeader, key: &str, value: f64) {
    let key_ptr = js_string_from_bytes(key.as_ptr(), key.len() as u32);
    js_object_set_field_by_name(obj, key_ptr, value);
}

fn set_named_str(obj: *mut ObjectHeader, key: &str, value: &str) {
    set_named(obj, key, create_string_f64(value));
}

fn set_url_pattern_prototype(obj: *mut ObjectHeader) {
    let proto = crate::object::builtin_prototype_value("URLPattern");
    if proto.to_bits() >> 48 == 0x7FFD {
        crate::object::prototype_chain::object_set_static_prototype(obj as usize, proto.to_bits());
    }
}

fn create_url_pattern_object(parts: UrlPatternParts) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, 0);
    if obj.is_null() {
        return obj;
    }
    set_named_str(obj, "protocol", &parts.protocol);
    set_named_str(obj, "username", &parts.username);
    set_named_str(obj, "password", &parts.password);
    set_named_str(obj, "hostname", &parts.hostname);
    set_named_str(obj, "port", &parts.port);
    set_named_str(obj, "pathname", &parts.pathname);
    set_named_str(obj, "search", &parts.search);
    set_named_str(obj, "hash", &parts.hash);
    set_named(obj, "hasRegExpGroups", bool_value(false));
    set_url_pattern_prototype(obj);
    obj
}

fn value_to_parts(input: f64, base: f64) -> Option<UrlPatternParts> {
    let input_js = crate::value::JSValue::from_bits(input.to_bits());
    if input_js.is_undefined() || input_js.is_null() {
        return Some(UrlPatternParts::wildcard());
    }
    if let Some(obj) = object_from_f64(input) {
        return Some(parts_from_object(obj));
    }
    let input_string = string_from_header(js_url_coerce_string(input));
    let base_string = {
        let base_js = crate::value::JSValue::from_bits(base.to_bits());
        if base_js.is_undefined() || base_js.is_null() {
            None
        } else {
            Some(string_from_header(js_url_coerce_string(base)))
        }
    };
    parts_from_string(&input_string, base_string.as_deref())
}

#[no_mangle]
pub extern "C" fn js_url_pattern_new(input: f64, base: f64) -> *mut ObjectHeader {
    let parts = value_to_parts(input, base).unwrap_or_else(|| throw_invalid_url_pattern());
    create_url_pattern_object(parts)
}

#[no_mangle]
pub extern "C" fn js_url_pattern_constructor_call(_input: f64, _base: f64) -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Cannot call constructor without `new`",
        "ERR_CONSTRUCT_CALL_REQUIRED",
    )
}

fn get_part(pattern: *mut ObjectHeader, key: &str) -> String {
    object_prop_string(pattern, key)
}

fn pattern_parts(pattern: *mut ObjectHeader) -> UrlPatternParts {
    UrlPatternParts {
        protocol: get_part(pattern, "protocol"),
        username: get_part(pattern, "username"),
        password: get_part(pattern, "password"),
        hostname: get_part(pattern, "hostname"),
        port: get_part(pattern, "port"),
        pathname: get_part(pattern, "pathname"),
        search: get_part(pattern, "search"),
        hash: get_part(pattern, "hash"),
    }
}

fn component_matches(pattern: &str, value: &str) -> bool {
    pattern == "*" || pattern == value
}

fn match_path_pattern(pattern: &str, value: &str) -> Option<Vec<(String, String)>> {
    if pattern == "*" {
        return Some(Vec::new());
    }
    let pattern_parts: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let value_parts: Vec<&str> = value.split('/').filter(|s| !s.is_empty()).collect();
    if pattern_parts.len() != value_parts.len() {
        return None;
    }
    let mut groups = Vec::new();
    for (pattern_part, value_part) in pattern_parts.iter().zip(value_parts.iter()) {
        if let Some(name) = pattern_part.strip_prefix(':') {
            if name.is_empty() {
                return None;
            }
            groups.push((name.to_string(), (*value_part).to_string()));
        } else if *pattern_part != *value_part {
            return None;
        }
    }
    Some(groups)
}

fn input_parts(input: f64, base: f64) -> Option<MatchInput> {
    let input_string = string_from_header(js_url_coerce_string(input));
    let base_js = crate::value::JSValue::from_bits(base.to_bits());
    let base_string = if !base_js.is_undefined() && !base_js.is_null() {
        Some(string_from_header(js_url_coerce_string(base)))
    } else {
        None
    };
    let inputs = if let Some(base) = &base_string {
        vec![input_string.clone(), base.clone()]
    } else {
        vec![input_string.clone()]
    };
    if is_valid_absolute_url(&input_string) {
        return Some(MatchInput {
            parts: parts_from_absolute_url(&input_string),
            inputs,
        });
    }
    if let Some(base_string) = base_string {
        if is_valid_absolute_url(&base_string) {
            let resolved = resolve_url(&input_string, &base_string);
            if is_valid_absolute_url(&resolved) {
                return Some(MatchInput {
                    parts: parts_from_absolute_url(&resolved),
                    inputs,
                });
            }
        }
    }
    None
}

fn exec_match(pattern: *mut ObjectHeader, input: f64, base: f64) -> Option<MatchResult> {
    if pattern.is_null() {
        return None;
    }
    let pattern = pattern_parts(pattern);
    let input = input_parts(input, base)?;
    let inputs = input.inputs;
    let input = input.parts;
    if !component_matches(&pattern.protocol, &input.protocol)
        || !component_matches(&pattern.username, &input.username)
        || !component_matches(&pattern.password, &input.password)
        || !component_matches(&pattern.hostname, &input.hostname)
        || !component_matches(&pattern.port, &input.port)
        || !component_matches(&pattern.search, &input.search)
        || !component_matches(&pattern.hash, &input.hash)
    {
        return None;
    }
    let pathname_groups = match_path_pattern(&pattern.pathname, &input.pathname)?;
    Some(MatchResult {
        inputs,
        protocol: input.protocol,
        username: input.username,
        password: input.password,
        hostname: input.hostname,
        port: input.port,
        pathname: input.pathname,
        search: input.search,
        hash: input.hash,
        pathname_groups,
    })
}

fn component_result(input: &str, groups: &[(String, String)]) -> f64 {
    let obj = js_object_alloc(0, 0);
    set_named_str(obj, "input", input);
    let groups_obj = js_object_alloc(0, 0);
    for (name, value) in groups {
        set_named_str(groups_obj, name, value);
    }
    set_named(
        obj,
        "groups",
        crate::value::js_nanbox_pointer(groups_obj as i64),
    );
    crate::value::js_nanbox_pointer(obj as i64)
}

fn build_exec_result(result: MatchResult) -> f64 {
    let obj = js_object_alloc(0, 0);
    let mut inputs = crate::array::js_array_alloc(0);
    for input in &result.inputs {
        inputs = crate::array::js_array_push_f64(inputs, create_string_f64(input));
    }
    set_named(
        obj,
        "inputs",
        crate::value::js_nanbox_pointer(inputs as i64),
    );
    set_named(obj, "protocol", component_result(&result.protocol, &[]));
    set_named(obj, "username", component_result(&result.username, &[]));
    set_named(obj, "password", component_result(&result.password, &[]));
    set_named(obj, "hostname", component_result(&result.hostname, &[]));
    set_named(obj, "port", component_result(&result.port, &[]));
    set_named(
        obj,
        "pathname",
        component_result(&result.pathname, &result.pathname_groups),
    );
    set_named(obj, "search", component_result(&result.search, &[]));
    set_named(obj, "hash", component_result(&result.hash, &[]));
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_url_pattern_test(pattern: *mut ObjectHeader, input: f64, base: f64) -> f64 {
    bool_value(exec_match(pattern, input, base).is_some())
}

#[no_mangle]
pub extern "C" fn js_url_pattern_exec(pattern: *mut ObjectHeader, input: f64, base: f64) -> f64 {
    match exec_match(pattern, input, base) {
        Some(result) => build_exec_result(result),
        None => null(),
    }
}

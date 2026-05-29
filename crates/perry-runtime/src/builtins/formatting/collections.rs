//! `util.inspect` / `console.log` formatting for the collection types that
//! the generic object/array formatter can't render from a keys array: `Map`,
//! `Set`, and `RegExp`. Node prints these as `Map(2) { 'a' => 1 }`,
//! `Set(3) { 1, 2, 3 }`, and `/ab+c/gi` respectively — see #800.
//!
//! Kept in a sibling submodule so the (already ~2000-line) parent
//! `formatting.rs` stays under the file-size gate. Reaches back into the
//! parent for `format_jsvalue` and the circular-reference bookkeeping via
//! `super::`.

use super::{format_jsvalue, inspect_compact_enabled, inspect_depth_limit};
use crate::string::StringHeader;
use crate::value::JSValue;

/// Format a single Map key/value or Set element the way Node's `util.inspect`
/// does *inside* a container: strings are quoted (`'hello'`), everything else
/// uses the normal `format_jsvalue` rendering.
fn format_member(value: f64, depth: usize) -> String {
    let jsval = JSValue::from_bits(value.to_bits());
    let rendered = format_jsvalue(value, depth + 1);
    if jsval.is_any_string() {
        format!("'{}'", rendered)
    } else {
        rendered
    }
}

/// Wrap `parts` as `TypeName(count) { ... }`, breaking onto multiple lines
/// the same way the array/object formatters do (Node uses one line until the
/// content is long or the entry count is high).
fn wrap(label: &str, count: usize, parts: &[String]) -> String {
    if count == 0 {
        return format!("{}(0) {{}}", label);
    }
    let inner = parts.join(", ");
    let use_multiline =
        !inspect_compact_enabled() || count > 6 || inner.len() + label.len() + 8 > 76;
    if !use_multiline {
        format!("{}({}) {{ {} }}", label, count, inner)
    } else {
        let body = parts
            .iter()
            .map(|p| format!("  {}", p))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("{}({}) {{\n{}\n}}", label, count, body)
    }
}

/// Render a `Map` as `Map(size) { key => value, ... }`.
///
/// # Safety
/// `map` must be a live `MapHeader` (callers gate on `GC_TYPE_MAP`).
pub(super) unsafe fn format_map(map: *const crate::map::MapHeader, depth: usize) -> String {
    if map.is_null() {
        return "Map(0) {}".to_string();
    }
    if depth > inspect_depth_limit() {
        return "[Map]".to_string();
    }
    let size = (*map).size as usize;
    let entries = (*map).entries;
    if size == 0 || entries.is_null() {
        return "Map(0) {}".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(size);
    for i in 0..size {
        let key = *entries.add(i * 2);
        let val = *entries.add(i * 2 + 1);
        parts.push(format!(
            "{} => {}",
            format_member(key, depth),
            format_member(val, depth)
        ));
    }
    wrap("Map", size, &parts)
}

/// Render a `Set` as `Set(size) { value, ... }`.
///
/// # Safety
/// `set` must be a live `SetHeader` (callers gate on `GC_TYPE_SET`).
pub(super) unsafe fn format_set(set: *const crate::set::SetHeader, depth: usize) -> String {
    if set.is_null() {
        return "Set(0) {}".to_string();
    }
    if depth > inspect_depth_limit() {
        return "[Set]".to_string();
    }
    let size = (*set).size as usize;
    let elements = (*set).elements;
    if size == 0 || elements.is_null() {
        return "Set(0) {}".to_string();
    }
    let mut parts: Vec<String> = Vec::with_capacity(size);
    for i in 0..size {
        parts.push(format_member(*elements.add(i), depth));
    }
    wrap("Set", size, &parts)
}

/// `format_map` wrapped in the parent's circular-reference bookkeeping so a
/// self-referential Map prints `[Circular *N]` instead of recursing forever.
///
/// # Safety
/// `ptr` must point at a live `MapHeader` (callers gate on `GC_TYPE_MAP`).
pub(super) unsafe fn format_map_with_cycle(
    ptr: *const crate::array::ArrayHeader,
    depth: usize,
) -> String {
    match super::inspect_enter_circular(ptr as usize) {
        Err(id) => format!("[Circular *{}]", id),
        Ok(()) => {
            let body = format_map(ptr as *const crate::map::MapHeader, depth);
            super::inspect_finish_circular(ptr as usize, body)
        }
    }
}

/// `format_set` wrapped in the parent's circular-reference bookkeeping.
///
/// # Safety
/// `ptr` must point at a live `SetHeader` (callers gate on `GC_TYPE_SET`).
pub(super) unsafe fn format_set_with_cycle(
    ptr: *const crate::array::ArrayHeader,
    depth: usize,
) -> String {
    match super::inspect_enter_circular(ptr as usize) {
        Err(id) => format!("[Circular *{}]", id),
        Ok(()) => {
            let body = format_set(ptr as *const crate::set::SetHeader, depth);
            super::inspect_finish_circular(ptr as usize, body)
        }
    }
}

/// If `value` is a RAW (non-NaN-boxed) heap pointer to a TypedArray or
/// Buffer — the bit pattern Perry uses for those object fields — render it
/// via the main `format_jsvalue` and return `Some`. Returns `None` for an
/// ordinary number. Mirrors `format_jsvalue`'s own raw-pointer probe so the
/// JSON-ish field formatter doesn't print the pointer bits as a float.
pub(super) fn raw_heap_pointer_display(value: f64, depth: usize) -> Option<String> {
    let raw_bits = value.to_bits();
    if raw_bits > 0x1000
        && (raw_bits >> 48) == 0
        && (crate::typedarray::lookup_typed_array_kind(raw_bits as usize).is_some()
            || crate::buffer::is_registered_buffer(raw_bits as usize))
    {
        Some(super::format_jsvalue(value, depth))
    } else {
        None
    }
}

/// Read a `StringHeader` into an owned `String` (empty on null).
unsafe fn read_string_header(s: *const StringHeader) -> String {
    if s.is_null() {
        return String::new();
    }
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

/// Render a `RegExp` literal as `/source/flags` (Node prints an empty source
/// as `/(?:)/` so the result always re-parses as a regex).
///
/// # Safety
/// `re` must be a registered `RegExpHeader` (callers gate on
/// `crate::regex::is_registered_regex`).
pub(super) unsafe fn format_regexp(re: *const crate::regex::RegExpHeader) -> String {
    let source = read_string_header(crate::regex::js_regexp_get_source(re));
    let flags = read_string_header(crate::regex::js_regexp_get_flags(re));
    let source = if source.is_empty() {
        "(?:)".to_string()
    } else {
        source
    };
    format!("/{}/{}", source, flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_formats_entries_with_size_prefix() {
        unsafe {
            let mut m = crate::map::js_map_alloc(4);
            m = crate::map::js_map_set(m, 1.0, 2.0);
            m = crate::map::js_map_set(m, 3.0, 4.0);
            let out = format_map(m as *const crate::map::MapHeader, 1);
            assert_eq!(out, "Map(2) { 1 => 2, 3 => 4 }");
        }
    }

    #[test]
    fn empty_map_and_set_use_zero_prefix() {
        unsafe {
            let m = crate::map::js_map_alloc(0);
            assert_eq!(
                format_map(m as *const crate::map::MapHeader, 1),
                "Map(0) {}"
            );
            let s = crate::set::js_set_alloc(0);
            assert_eq!(
                format_set(s as *const crate::set::SetHeader, 1),
                "Set(0) {}"
            );
        }
    }

    #[test]
    fn set_formats_elements_with_size_prefix() {
        unsafe {
            let mut s = crate::set::js_set_alloc(4);
            s = crate::set::js_set_add(s, 1.0);
            s = crate::set::js_set_add(s, 2.0);
            s = crate::set::js_set_add(s, 3.0);
            let out = format_set(s as *const crate::set::SetHeader, 1);
            assert_eq!(out, "Set(3) { 1, 2, 3 }");
        }
    }

    #[test]
    fn deep_nesting_collapses_to_type_label() {
        unsafe {
            let m = crate::map::js_map_alloc(0);
            // Past the inspect depth limit, Node prints `[Map]` / `[Set]`.
            assert_eq!(format_map(m as *const crate::map::MapHeader, 99), "[Map]");
            let s = crate::set::js_set_alloc(0);
            assert_eq!(format_set(s as *const crate::set::SetHeader, 99), "[Set]");
        }
    }
}

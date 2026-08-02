//! `node:test/reporters` — the deterministic spec / tap / dot / junit / lcov
//! formatters and the transform-stream shims that feed them.
//!
//! Split verbatim out of the parent [`super`] module to keep `test.rs` under
//! the 2000-line gate. Pure code move — no behavior change.

use super::*;

fn reporter_with_kind(kind: i32, source: f64) -> f64 {
    if JSValue::from_bits(source.to_bits()).is_undefined() {
        return reporter_transform(kind);
    }
    let events = collect_event_values(source);
    let output = format_reporter_events(kind, &events);
    readable_from_text(output)
}

pub(crate) extern "C" fn thunk_reporter_spec(_closure: *const ClosureHeader, source: f64) -> f64 {
    reporter_with_kind(REPORTER_SPEC, source)
}

pub(crate) extern "C" fn thunk_reporter_tap(_closure: *const ClosureHeader, source: f64) -> f64 {
    reporter_with_kind(REPORTER_TAP, source)
}

pub(crate) extern "C" fn thunk_reporter_dot(_closure: *const ClosureHeader, source: f64) -> f64 {
    reporter_with_kind(REPORTER_DOT, source)
}

pub(crate) extern "C" fn thunk_reporter_junit(_closure: *const ClosureHeader, source: f64) -> f64 {
    reporter_with_kind(REPORTER_JUNIT, source)
}

pub(crate) extern "C" fn thunk_reporter_lcov(_closure: *const ClosureHeader, source: f64) -> f64 {
    reporter_with_kind(REPORTER_LCOV, source)
}

fn reporter_transform(kind: i32) -> f64 {
    let transform = make_closure(reporter_transform_chunk as *const u8, 3, 1);
    js_closure_set_capture_f64(transform, 0, kind as f64);
    let opts = js_object_alloc(0, 1);
    set_field(opts, "transform", boxed_ptr(transform));
    crate::node_stream::js_node_stream_transform_new(boxed_ptr(opts))
}

extern "C" fn reporter_transform_chunk(
    closure: *const ClosureHeader,
    chunk: f64,
    _encoding: f64,
    callback: f64,
) -> f64 {
    let kind = js_closure_get_capture_f64(closure, 0) as i32;
    let output = format_reporter_event(kind, chunk);
    if !output.is_empty() {
        let this = crate::object::js_implicit_this_get();
        let handle = (this.to_bits() & POINTER_MASK) as i64;
        crate::node_stream::js_node_stream_method_push(handle, string_value(&output));
    }
    if is_callable_value(callback) {
        js_closure_call0(raw_ptr_from_value(callback) as *const ClosureHeader);
    }
    undefined_value()
}

fn collect_event_values(source: f64) -> Vec<f64> {
    if let Some(values) = array_values(source) {
        return values;
    }
    if let Some(Ok(chunks)) = crate::node_stream::js_node_stream_collect_chunks_result(source) {
        return array_values(chunks).unwrap_or_else(|| vec![chunks]);
    }
    vec![source]
}

fn readable_from_text(text: String) -> f64 {
    let mut arr = crate::array::js_array_alloc(if text.is_empty() { 0 } else { 1 });
    if !text.is_empty() {
        arr = crate::array::js_array_push_f64(arr, string_value(&text));
    }
    crate::node_stream::js_node_stream_readable_from(boxed_ptr(arr))
}

fn event_type(event: f64) -> Option<String> {
    object_string(event, b"type")
}

fn event_data(event: f64) -> f64 {
    object_property(event, b"data").unwrap_or(undefined_value())
}

fn format_reporter_events(kind: i32, events: &[f64]) -> String {
    if kind == REPORTER_LCOV {
        return String::new();
    }
    let mut out = String::new();
    if kind == REPORTER_TAP {
        out.push_str("TAP version 13\n");
    } else if kind == REPORTER_JUNIT {
        out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<testsuites>\n");
    }
    for &event in events {
        out.push_str(&format_reporter_event(kind, event));
    }
    if kind == REPORTER_DOT && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if kind == REPORTER_JUNIT {
        out.push_str("</testsuites>\n");
    }
    out
}

fn format_reporter_event(kind: i32, event: f64) -> String {
    let Some(typ) = event_type(event) else {
        return String::new();
    };
    let data = event_data(event);
    match kind {
        REPORTER_SPEC => match typ.as_str() {
            "test:pass" => object_string(data, b"name")
                .map(|name| format!("✔ {name}\n"))
                .unwrap_or_default(),
            "test:diagnostic" => object_string(data, b"message")
                .map(|message| format!("ℹ {message}\n"))
                .unwrap_or_default(),
            _ => String::new(),
        },
        REPORTER_TAP => match typ.as_str() {
            "test:start" => object_string(data, b"name")
                .map(|name| format!("# Subtest: {name}\n"))
                .unwrap_or_default(),
            "test:pass" => {
                let name = object_string(data, b"name").unwrap_or_default();
                let detail_type = object_property(data, b"details")
                    .and_then(|details| object_string(details, b"type"))
                    .unwrap_or_else(|| "test".to_string());
                format!("ok undefined - {name}\n  ---\n  type: '{detail_type}'\n  ...\n")
            }
            "test:diagnostic" => object_string(data, b"message")
                .map(|message| format!("# {message}\n"))
                .unwrap_or_default(),
            _ => String::new(),
        },
        REPORTER_DOT => {
            if typ == "test:pass" {
                ".".to_string()
            } else {
                String::new()
            }
        }
        REPORTER_JUNIT => match typ.as_str() {
            "test:pass" => {
                let name = xml_escape(&object_string(data, b"name").unwrap_or_default());
                let class = object_property(data, b"details")
                    .and_then(|details| object_string(details, b"type"))
                    .unwrap_or_else(|| "test".to_string());
                let class = xml_escape(&class);
                format!("\t<testcase name=\"{name}\" time=\"NaN\" classname=\"{class}\"/>\n")
            }
            "test:diagnostic" => object_string(data, b"message")
                .map(|message| format!("\t<!-- {} -->\n", xml_escape_comment(&message)))
                .unwrap_or_default(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn xml_escape_comment(input: &str) -> String {
    input.replace("--", "- -")
}

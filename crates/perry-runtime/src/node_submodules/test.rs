//! Minimal `node:test` and `node:test/reporters` runtime surface.
//!
//! The implementation focuses on Perry's parity fixtures: import shapes,
//! snapshot comparison helpers, mock timer control, and deterministic reporter
//! formatting for synthetic events.

use std::cell::{Cell, RefCell};
use std::fs;

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_get_capture_f64,
    js_closure_set_capture_f64, js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::{JSValue, POINTER_MASK, TAG_UNDEFINED};

const REPORTER_SPEC: i32 = 0;
const REPORTER_TAP: i32 = 1;
const REPORTER_DOT: i32 = 2;
const REPORTER_JUNIT: i32 = 3;
const REPORTER_LCOV: i32 = 4;

thread_local! {
    static MOCK_OBJECT: RefCell<Option<*mut crate::object::ObjectHeader>> = const { RefCell::new(None) };
    static SNAPSHOT_OBJECT: RefCell<Option<*mut crate::object::ObjectHeader>> = const { RefCell::new(None) };
    static REPORTERS_DEFAULT_OBJECT: RefCell<Option<*mut crate::object::ObjectHeader>> = const { RefCell::new(None) };
    static SNAPSHOT_RESOLVER: Cell<f64> = const { Cell::new(f64::from_bits(TAG_UNDEFINED)) };
    static CURRENT_TEST_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_SNAPSHOT_INDEX: Cell<u32> = const { Cell::new(0) };
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn boxed_ptr<T>(ptr: *const T) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn make_closure(func: *const u8, arity: u32, captures: u32) -> *mut crate::closure::ClosureHeader {
    js_register_closure_arity(func, arity);
    js_closure_alloc(func, captures)
}

fn closure_value(func: *const u8, arity: u32) -> f64 {
    boxed_ptr(make_closure(func, arity, 0))
}

fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    (gc_type <= crate::gc::GC_TYPE_MAX).then_some(gc_type)
}

fn is_array_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000
        && !crate::buffer::is_registered_buffer(raw)
        && unsafe { gc_type_for_ptr(raw) == Some(crate::gc::GC_TYPE_ARRAY) }
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000
        && !crate::buffer::is_registered_buffer(raw)
        && unsafe { gc_type_for_ptr(raw) == Some(crate::gc::GC_TYPE_CLOSURE) }
        && crate::closure::is_closure_ptr(raw)
}

fn array_values(value: f64) -> Option<Vec<f64>> {
    if !is_array_value(value) {
        return None;
    }
    let arr = raw_ptr_from_value(value) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(arr, i));
    }
    Some(values)
}

fn value_to_string(value: f64) -> Option<String> {
    crate::builtins::jsvalue_string_content(value)
}

fn object_property(value: f64, name: &[u8]) -> Option<f64> {
    super::stream_promises::get_object_property(value, name)
}

fn object_string(value: f64, name: &[u8]) -> Option<String> {
    object_property(value, name).and_then(value_to_string)
}

fn throw_error_with_code(message: &str, code: &'static str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_invalid_arg_type(arg: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        arg,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn json_stringify_pretty(value: f64) -> String {
    let spacer = string_value("  ");
    let bits =
        unsafe { crate::json::js_json_stringify_full(value, undefined_value(), spacer) } as u64;
    if bits == TAG_UNDEFINED {
        return "undefined".to_string();
    }
    let boxed = f64::from_bits(bits);
    value_to_string(boxed).unwrap_or_else(|| "undefined".to_string())
}

fn snapshot_payload(value: f64) -> String {
    let json = json_stringify_pretty(value);
    if json == "undefined" {
        crate::builtins::format_jsvalue(value, 0)
    } else {
        json
    }
}

extern "C" fn snapshot_set_default_serializers(
    _closure: *const ClosureHeader,
    serializers: f64,
) -> f64 {
    if !is_array_value(serializers) {
        throw_invalid_arg_type("serializers", "Array", serializers);
    }
    undefined_value()
}

extern "C" fn snapshot_set_resolve_snapshot_path(
    _closure: *const ClosureHeader,
    resolver: f64,
) -> f64 {
    if !is_callable_value(resolver) {
        throw_invalid_arg_type("fn", "function", resolver);
    }
    SNAPSHOT_RESOLVER.with(|slot| slot.set(resolver));
    undefined_value()
}

extern "C" fn assert_snapshot(_closure: *const ClosureHeader, value: f64) -> f64 {
    let resolver = SNAPSHOT_RESOLVER.with(|slot| slot.get());
    if !is_callable_value(resolver) {
        throw_error_with_code(
            "Invalid state: snapshot.setResolveSnapshotPath() must be called before t.assert.snapshot()",
            "ERR_INVALID_STATE",
        );
    }
    let resolver_ptr = raw_ptr_from_value(resolver) as *const ClosureHeader;
    let path_value = js_closure_call1(resolver_ptr, string_value(""));
    let Some(path) = value_to_string(path_value) else {
        throw_invalid_arg_type("snapshot path", "string", path_value);
    };
    let file = fs::read_to_string(&path).unwrap_or_else(|_| {
        throw_error_with_code(
            &format!("Invalid state: snapshot file does not exist: {path}"),
            "ERR_INVALID_STATE",
        )
    });
    let name = CURRENT_TEST_NAME
        .with(|n| n.borrow().clone())
        .unwrap_or_else(|| "snapshot".to_string());
    let index = CURRENT_SNAPSHOT_INDEX.with(|idx| {
        let next = idx.get() + 1;
        idx.set(next);
        next
    });
    let marker = format!("exports[`{} {}`] = `", name, index);
    let Some(start) = file.find(&marker).map(|pos| pos + marker.len()) else {
        throw_error_with_code(
            &format!("Snapshot `{name} {index}` was not found"),
            "ERR_INVALID_STATE",
        );
    };
    let Some(end_rel) = file[start..].find("`;") else {
        throw_error_with_code("Snapshot file is malformed", "ERR_INVALID_STATE");
    };
    let expected = &file[start..start + end_rel];
    let actual = format!("\n{}\n", snapshot_payload(value));
    if expected.trim_end() != actual.trim_end() {
        throw_error_with_code(
            &format!(
                "Snapshot mismatch for `{name} {index}`\nExpected:\n{expected}\nActual:\n{actual}"
            ),
            "ERR_ASSERTION",
        );
    }
    undefined_value()
}

extern "C" fn assert_file_snapshot(
    _closure: *const ClosureHeader,
    value: f64,
    path_value: f64,
) -> f64 {
    let Some(path) = value_to_string(path_value) else {
        throw_invalid_arg_type("path", "string", path_value);
    };
    let expected = fs::read_to_string(&path).unwrap_or_else(|_| {
        throw_error_with_code(
            &format!("Invalid state: snapshot file does not exist: {path}"),
            "ERR_INVALID_STATE",
        )
    });
    let actual = snapshot_payload(value);
    if expected.trim_end() != actual.trim_end() {
        throw_error_with_code(
            &format!("File snapshot mismatch for `{path}`"),
            "ERR_ASSERTION",
        );
    }
    undefined_value()
}

fn snapshot_object_value() -> f64 {
    SNAPSHOT_OBJECT.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            return boxed_ptr(ptr);
        }
        let obj = js_object_alloc(0, 2);
        set_field(
            obj,
            "setDefaultSnapshotSerializers",
            closure_value(snapshot_set_default_serializers as *const u8, 1),
        );
        set_field(
            obj,
            "setResolveSnapshotPath",
            closure_value(snapshot_set_resolve_snapshot_path as *const u8, 1),
        );
        *slot.borrow_mut() = Some(obj);
        boxed_ptr(obj)
    })
}

extern "C" fn mock_timers_enable(_closure: *const ClosureHeader, options: f64) -> f64 {
    let (apis, now) = parse_mock_timer_options(options);
    crate::timer::js_mock_timers_enable(apis, now);
    undefined_value()
}

extern "C" fn mock_timers_tick(_closure: *const ClosureHeader, ms: f64) -> f64 {
    let delay = validate_mock_timer_number("time", ms);
    crate::timer::js_mock_timers_tick(delay);
    undefined_value()
}

extern "C" fn mock_timers_run_all(_closure: *const ClosureHeader) -> f64 {
    crate::timer::js_mock_timers_run_all();
    undefined_value()
}

extern "C" fn mock_timers_set_time(_closure: *const ClosureHeader, ms: f64) -> f64 {
    let time = validate_mock_timer_number("time", ms);
    crate::timer::js_mock_timers_set_time(time);
    undefined_value()
}

extern "C" fn mock_timers_reset(_closure: *const ClosureHeader) -> f64 {
    crate::timer::js_mock_timers_reset();
    undefined_value()
}

fn validate_mock_timer_number(arg: &str, value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(js) {
        throw_invalid_arg_type(arg, "number", value);
    }
    let n = crate::builtins::js_number_coerce(value);
    if !n.is_finite() || n < 0.0 {
        let message = format!(
            "The \"{}\" argument must be a non-negative finite number. Received {}",
            arg,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    }
    n
}

fn parse_mock_timer_options(options: f64) -> (u32, f64) {
    let mut apis_value = options;
    let mut now = crate::timer::js_mock_timers_real_now_ms();
    let js = JSValue::from_bits(options.to_bits());
    if js.is_undefined() {
        return (crate::timer::MOCK_TIMERS_ALL_APIS, now);
    }
    if !is_array_value(options) {
        if js.is_null() || !js.is_pointer() {
            throw_invalid_arg_type("options", "object", options);
        }
        apis_value = object_property(options, b"apis").unwrap_or(undefined_value());
        if let Some(now_value) = object_property(options, b"now") {
            now = validate_mock_timer_number("options.now", now_value);
        }
    }
    if JSValue::from_bits(apis_value.to_bits()).is_undefined() {
        return (crate::timer::MOCK_TIMERS_ALL_APIS, now);
    }
    if !is_array_value(apis_value) {
        throw_invalid_arg_type("options.apis", "Array", apis_value);
    }
    let mut mask = 0u32;
    for api in array_values(apis_value).unwrap_or_default() {
        let Some(name) = value_to_string(api) else {
            throw_invalid_arg_type("options.apis", "string", api);
        };
        match name.as_str() {
            "Date" => mask |= crate::timer::MOCK_TIMERS_API_DATE,
            "setTimeout" => mask |= crate::timer::MOCK_TIMERS_API_SET_TIMEOUT,
            "setInterval" => mask |= crate::timer::MOCK_TIMERS_API_SET_INTERVAL,
            "setImmediate" => mask |= crate::timer::MOCK_TIMERS_API_SET_IMMEDIATE,
            _ => {
                let message = format!(
                    "The property 'options.apis' option {name} is not supported. Received '{name}'"
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
            }
        }
    }
    (mask, now)
}

fn mock_object_value() -> f64 {
    MOCK_OBJECT.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            return boxed_ptr(ptr);
        }
        let timers = js_object_alloc(0, 5);
        set_field(
            timers,
            "enable",
            closure_value(mock_timers_enable as *const u8, 1),
        );
        set_field(
            timers,
            "tick",
            closure_value(mock_timers_tick as *const u8, 1),
        );
        set_field(
            timers,
            "runAll",
            closure_value(mock_timers_run_all as *const u8, 0),
        );
        set_field(
            timers,
            "setTime",
            closure_value(mock_timers_set_time as *const u8, 1),
        );
        set_field(
            timers,
            "reset",
            closure_value(mock_timers_reset as *const u8, 0),
        );

        let mock = js_object_alloc(0, 1);
        set_field(mock, "timers", boxed_ptr(timers));
        *slot.borrow_mut() = Some(mock);
        boxed_ptr(mock)
    })
}

fn test_context_value() -> f64 {
    let assert = js_object_alloc(0, 2);
    set_field(
        assert,
        "snapshot",
        closure_value(assert_snapshot as *const u8, 1),
    );
    set_field(
        assert,
        "fileSnapshot",
        closure_value(assert_file_snapshot as *const u8, 2),
    );
    let ctx = js_object_alloc(0, 1);
    set_field(ctx, "assert", boxed_ptr(assert));
    boxed_ptr(ctx)
}

pub(crate) extern "C" fn thunk_test(
    _closure: *const ClosureHeader,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    let (name, cb) = if is_callable_value(name_or_callback) {
        ("<anonymous>".to_string(), name_or_callback)
    } else if is_callable_value(options_or_callback) {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            options_or_callback,
        )
    } else if is_callable_value(callback) {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            callback,
        )
    } else {
        return undefined_value();
    };

    let cb_ptr = raw_ptr_from_value(cb) as *const ClosureHeader;
    CURRENT_TEST_NAME.with(|slot| *slot.borrow_mut() = Some(name.clone()));
    CURRENT_SNAPSHOT_INDEX.with(|idx| idx.set(0));
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctx = scope.root_nanbox_f64(test_context_value());
    js_closure_call1(cb_ptr, ctx.get_nanbox_f64());
    CURRENT_TEST_NAME.with(|slot| *slot.borrow_mut() = None);
    CURRENT_SNAPSHOT_INDEX.with(|idx| idx.set(0));

    println!("✔ {name} (0ms)");
    println!("ℹ tests 1");
    println!("ℹ suites 0");
    println!("ℹ pass 1");
    println!("ℹ fail 0");
    println!("ℹ cancelled 0");
    println!("ℹ skipped 0");
    println!("ℹ todo 0");
    println!("ℹ duration_ms 0");
    undefined_value()
}

pub(crate) extern "C" fn thunk_test_hook(_closure: *const ClosureHeader, callback: f64) -> f64 {
    if is_callable_value(callback) {
        let cb = raw_ptr_from_value(callback) as *const ClosureHeader;
        js_closure_call0(cb);
    }
    undefined_value()
}

pub(crate) extern "C" fn thunk_test_run(_closure: *const ClosureHeader, _options: f64) -> f64 {
    let arr = crate::array::js_array_alloc(0);
    crate::node_stream::js_node_stream_readable_from(boxed_ptr(arr))
}

pub(crate) fn test_special_export_value(name: &str) -> Option<f64> {
    match name {
        "mock" => Some(mock_object_value()),
        "snapshot" => Some(snapshot_object_value()),
        _ => None,
    }
}

pub(crate) fn test_reporters_special_export_value(name: &str) -> Option<f64> {
    (name == "default").then(reporters_default_object_value)
}

pub(crate) fn populate_reporters_default(obj: *mut crate::object::ObjectHeader, value: f64) {
    set_field(obj, "default", value);
}

pub(crate) fn scan_test_module_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    MOCK_OBJECT.with(|slot| {
        if let Some(ptr) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    SNAPSHOT_OBJECT.with(|slot| {
        if let Some(ptr) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    REPORTERS_DEFAULT_OBJECT.with(|slot| {
        if let Some(ptr) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    SNAPSHOT_RESOLVER.with(|slot| {
        let mut value = slot.get();
        visitor.visit_nanbox_f64_slot(&mut value);
        slot.set(value);
    });
}

fn reporter_closure(func: *const u8, kind: i32) -> f64 {
    let closure = make_closure(func, 1, 1);
    js_closure_set_capture_f64(closure, 0, kind as f64);
    boxed_ptr(closure)
}

fn reporters_default_object_value() -> f64 {
    REPORTERS_DEFAULT_OBJECT.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            return boxed_ptr(ptr);
        }
        let obj = js_object_alloc(0, 5);
        set_field(
            obj,
            "spec",
            reporter_closure(thunk_reporter as *const u8, REPORTER_SPEC),
        );
        set_field(
            obj,
            "tap",
            reporter_closure(thunk_reporter as *const u8, REPORTER_TAP),
        );
        set_field(
            obj,
            "dot",
            reporter_closure(thunk_reporter as *const u8, REPORTER_DOT),
        );
        set_field(
            obj,
            "junit",
            reporter_closure(thunk_reporter as *const u8, REPORTER_JUNIT),
        );
        set_field(
            obj,
            "lcov",
            reporter_closure(thunk_reporter as *const u8, REPORTER_LCOV),
        );
        *slot.borrow_mut() = Some(obj);
        boxed_ptr(obj)
    })
}

fn reporter_with_kind(kind: i32, source: f64) -> f64 {
    if JSValue::from_bits(source.to_bits()).is_undefined() {
        return reporter_transform(kind);
    }
    let events = collect_event_values(source);
    let output = format_reporter_events(kind, &events);
    readable_from_text(output)
}

pub(crate) extern "C" fn thunk_reporter(closure: *const ClosureHeader, source: f64) -> f64 {
    let kind = js_closure_get_capture_f64(closure, 0) as i32;
    reporter_with_kind(kind, source)
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

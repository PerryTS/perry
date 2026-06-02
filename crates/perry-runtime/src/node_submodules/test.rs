//! Minimal `node:test` and `node:test/reporters` runtime surface.
//!
//! The implementation focuses on Perry's parity fixtures: import shapes,
//! snapshot comparison helpers, mock timer control, and deterministic reporter
//! formatting for synthetic events.

use std::cell::{Cell, RefCell};
use std::fs;
use std::os::raw::c_int;

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_get_capture_f64,
    js_closure_get_capture_ptr, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    js_register_closure_arity, js_register_closure_rest, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::js_string_from_bytes;
use crate::value::{JSValue, POINTER_MASK, TAG_UNDEFINED};

const REPORTER_SPEC: i32 = 0;
const REPORTER_TAP: i32 = 1;
const REPORTER_DOT: i32 = 2;
const REPORTER_JUNIT: i32 = 3;
const REPORTER_LCOV: i32 = 4;
const TEST_OVERRIDE_NONE: i8 = 0;
const TEST_OVERRIDE_SKIP: i8 = 1;
const TEST_OVERRIDE_TODO: i8 = 2;

thread_local! {
    static MOCK_OBJECT: RefCell<Option<*mut crate::object::ObjectHeader>> = const { RefCell::new(None) };
    static SNAPSHOT_OBJECT: RefCell<Option<*mut crate::object::ObjectHeader>> = const { RefCell::new(None) };
    static SNAPSHOT_RESOLVER: Cell<f64> = const { Cell::new(f64::from_bits(TAG_UNDEFINED)) };
    static CURRENT_TEST_NAME: RefCell<Option<String>> = const { RefCell::new(None) };
    static CURRENT_DIAGNOSTICS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CURRENT_SNAPSHOT_INDEX: Cell<u32> = const { Cell::new(0) };
    static CURRENT_ASSERT_COUNT: Cell<u32> = const { Cell::new(0) };
    static CURRENT_PLAN: Cell<Option<u32>> = const { Cell::new(None) };
    static CURRENT_TEST_OVERRIDE: Cell<i8> = const { Cell::new(TEST_OVERRIDE_NONE) };
    static NEXT_MOCK_ID: Cell<i64> = const { Cell::new(1) };
    static MOCK_STATES: RefCell<Vec<MockState>> = const { RefCell::new(Vec::new()) };
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn is_undefined_value(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
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

fn closure_value_with_id(func: *const u8, arity: u32, id: i64) -> f64 {
    let closure = make_closure(func, arity, 1);
    js_closure_set_capture_ptr(closure, 0, id);
    boxed_ptr(closure)
}

fn rest_closure_value_with_id(func: *const u8, fixed_arity: u32, id: i64) -> f64 {
    js_register_closure_rest(func, fixed_arity);
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(closure, 0, id);
    boxed_ptr(closure)
}

fn closure_id(closure: *const ClosureHeader) -> i64 {
    js_closure_get_capture_ptr(closure, 0)
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

fn catch_js<F: FnOnce() -> f64>(f: F) -> Result<f64, f64> {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
    if jumped == 0 {
        let result = f();
        crate::exception::js_try_end();
        Ok(result)
    } else {
        crate::exception::js_try_end();
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(err)
    }
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

fn assert_callable_arg(arg: &str, value: f64) {
    if !is_callable_value(value) {
        throw_invalid_arg_type(arg, "function", value);
    }
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
    CURRENT_ASSERT_COUNT.with(|count| count.set(count.get() + 1));
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
    CURRENT_ASSERT_COUNT.with(|count| count.set(count.get() + 1));
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

#[derive(Clone)]
enum MockRestoreTarget {
    None,
    ObjectProperty {
        target: f64,
        property: String,
        original: f64,
    },
}

struct MockState {
    id: i64,
    original: f64,
    implementation: f64,
    once: Vec<f64>,
    calls: f64,
    context: f64,
    function: f64,
    restore: MockRestoreTarget,
}

fn next_mock_id() -> i64 {
    NEXT_MOCK_ID.with(|slot| {
        let id = slot.get();
        slot.set(id + 1);
        id
    })
}

fn update_mock_context_calls(context: f64, calls: f64) {
    let ptr = raw_ptr_from_value(context);
    if ptr >= 0x10000 {
        set_field(ptr as *mut crate::object::ObjectHeader, "calls", calls);
    }
}

fn set_property_value(target: f64, property: &str, value: f64) {
    let raw = raw_ptr_from_value(target);
    if raw < 0x10000 {
        throw_invalid_arg_type("object", "object", target);
    }
    if crate::closure::is_closure_ptr(raw) {
        crate::closure::closure_set_dynamic_prop(raw, property, value);
    } else {
        set_field(raw as *mut crate::object::ObjectHeader, property, value);
    }
}

fn get_property_value(target: f64, property: &str) -> f64 {
    let raw = raw_ptr_from_value(target);
    if raw >= 0x10000 && crate::closure::is_closure_ptr(raw) {
        return crate::closure::closure_get_dynamic_prop(raw, property);
    }
    object_property(target, property.as_bytes()).unwrap_or(undefined_value())
}

fn property_name(value: f64) -> String {
    value_to_string(value).unwrap_or_else(|| {
        throw_invalid_arg_type("propertyName", "string", value);
    })
}

fn mock_context_object(id: i64, calls: f64, include_call_tracking: bool) -> f64 {
    let obj = js_object_alloc(0, 6);
    if include_call_tracking {
        set_field(obj, "calls", calls);
        set_field(
            obj,
            "callCount",
            closure_value_with_id(mock_context_call_count as *const u8, 0, id),
        );
        set_field(
            obj,
            "resetCalls",
            closure_value_with_id(mock_context_reset_calls as *const u8, 0, id),
        );
        set_field(
            obj,
            "mockImplementation",
            closure_value_with_id(mock_context_mock_implementation as *const u8, 1, id),
        );
        set_field(
            obj,
            "mockImplementationOnce",
            closure_value_with_id(mock_context_mock_implementation_once as *const u8, 1, id),
        );
    }
    set_field(
        obj,
        "restore",
        closure_value_with_id(mock_context_restore as *const u8, 0, id),
    );
    boxed_ptr(obj)
}

fn create_mock_function(original: f64, implementation: f64, restore: MockRestoreTarget) -> f64 {
    if !JSValue::from_bits(original.to_bits()).is_undefined() {
        assert_callable_arg("original", original);
    }
    if !JSValue::from_bits(implementation.to_bits()).is_undefined() {
        assert_callable_arg("implementation", implementation);
    }

    let id = next_mock_id();
    let calls = boxed_ptr(crate::array::js_array_alloc(0));
    let context = mock_context_object(id, calls, true);
    let function = rest_closure_value_with_id(mock_function_invoke as *const u8, 0, id);
    let closure_ptr = raw_ptr_from_value(function);
    if closure_ptr != 0 {
        crate::object::set_bound_native_closure_name(closure_ptr as *mut ClosureHeader, "mockFn");
        crate::closure::closure_set_dynamic_prop(closure_ptr, "mock", context);
    }

    MOCK_STATES.with(|states| {
        states.borrow_mut().push(MockState {
            id,
            original,
            implementation,
            once: Vec::new(),
            calls,
            context,
            function,
            restore,
        });
    });
    function
}

fn create_restore_context(restore: MockRestoreTarget) -> f64 {
    let id = next_mock_id();
    let calls = boxed_ptr(crate::array::js_array_alloc(0));
    let context = mock_context_object(id, calls, false);
    MOCK_STATES.with(|states| {
        states.borrow_mut().push(MockState {
            id,
            original: undefined_value(),
            implementation: undefined_value(),
            once: Vec::new(),
            calls,
            context,
            function: undefined_value(),
            restore,
        });
    });
    context
}

fn reset_mock_state_calls(state: &mut MockState) {
    state.calls = boxed_ptr(crate::array::js_array_alloc(0));
    update_mock_context_calls(state.context, state.calls);
}

fn restore_mock_state(id: i64) {
    let restore = MOCK_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.iter_mut().find(|state| state.id == id) else {
            return None;
        };
        state.implementation = state.original;
        state.once.clear();
        reset_mock_state_calls(state);
        Some(state.restore.clone())
    });
    if let Some(MockRestoreTarget::ObjectProperty {
        target,
        property,
        original,
    }) = restore
    {
        set_property_value(target, &property, original);
    }
}

fn record_mock_call(id: i64, args_value: f64, this_value: f64, result: f64, error: f64) {
    let calls_value = MOCK_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .map(|state| state.calls)
            .unwrap_or_else(undefined_value)
    });
    if !is_array_value(calls_value) {
        return;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let args_handle = scope.root_nanbox_f64(args_value);
    let this_handle = scope.root_nanbox_f64(this_value);
    let result_handle = scope.root_nanbox_f64(result);
    let error_handle = scope.root_nanbox_f64(error);
    let calls_handle = scope.root_nanbox_f64(calls_value);

    let call = js_object_alloc(0, 5);
    set_field(call, "arguments", args_handle.get_nanbox_f64());
    set_field(call, "this", this_handle.get_nanbox_f64());
    set_field(call, "target", undefined_value());
    set_field(call, "result", result_handle.get_nanbox_f64());
    set_field(call, "error", error_handle.get_nanbox_f64());
    let call_handle = scope.root_nanbox_f64(boxed_ptr(call));

    let calls_ptr =
        raw_ptr_from_value(calls_handle.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
    let new_calls = crate::array::js_array_push_f64(calls_ptr, call_handle.get_nanbox_f64());
    let new_calls_value = boxed_ptr(new_calls);
    MOCK_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.calls = new_calls_value;
            update_mock_context_calls(state.context, state.calls);
        }
    });
}

extern "C" fn mock_function_invoke(closure: *const ClosureHeader, rest: f64) -> f64 {
    let id = closure_id(closure);
    let args = array_values(rest).unwrap_or_default();
    let implementation = MOCK_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.iter_mut().find(|state| state.id == id) else {
            return undefined_value();
        };
        if !state.once.is_empty() {
            state.once.remove(0)
        } else {
            state.implementation
        }
    });

    let this_value = crate::object::js_implicit_this_get();
    if JSValue::from_bits(implementation.to_bits()).is_undefined() {
        record_mock_call(id, rest, this_value, undefined_value(), undefined_value());
        return undefined_value();
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let implementation_handle = scope.root_nanbox_f64(implementation);
    let rest_handle = scope.root_nanbox_f64(rest);
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let call_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    match catch_js(|| unsafe {
        crate::closure::js_native_call_value(
            implementation_handle.get_nanbox_f64(),
            call_args.as_ptr(),
            call_args.len(),
        )
    }) {
        Ok(result) => {
            let result_handle = scope.root_nanbox_f64(result);
            record_mock_call(
                id,
                rest_handle.get_nanbox_f64(),
                this_value,
                result_handle.get_nanbox_f64(),
                undefined_value(),
            );
            result_handle.get_nanbox_f64()
        }
        Err(err) => {
            let err_handle = scope.root_nanbox_f64(err);
            record_mock_call(
                id,
                rest_handle.get_nanbox_f64(),
                this_value,
                undefined_value(),
                err_handle.get_nanbox_f64(),
            );
            crate::exception::js_throw(err_handle.get_nanbox_f64())
        }
    }
}

extern "C" fn mock_context_call_count(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    MOCK_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .find(|state| state.id == id)
            .and_then(|state| {
                is_array_value(state.calls).then(|| {
                    crate::array::js_array_length(
                        raw_ptr_from_value(state.calls) as *const crate::array::ArrayHeader
                    ) as f64
                })
            })
            .unwrap_or(0.0)
    })
}

extern "C" fn mock_context_reset_calls(closure: *const ClosureHeader) -> f64 {
    let id = closure_id(closure);
    MOCK_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            reset_mock_state_calls(state);
        }
    });
    undefined_value()
}

extern "C" fn mock_context_mock_implementation(
    closure: *const ClosureHeader,
    implementation: f64,
) -> f64 {
    assert_callable_arg("implementation", implementation);
    let id = closure_id(closure);
    MOCK_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.implementation = implementation;
        }
    });
    undefined_value()
}

extern "C" fn mock_context_mock_implementation_once(
    closure: *const ClosureHeader,
    implementation: f64,
) -> f64 {
    assert_callable_arg("implementation", implementation);
    let id = closure_id(closure);
    MOCK_STATES.with(|states| {
        if let Some(state) = states.borrow_mut().iter_mut().find(|state| state.id == id) {
            state.once.push(implementation);
        }
    });
    undefined_value()
}

extern "C" fn mock_context_restore(closure: *const ClosureHeader) -> f64 {
    restore_mock_state(closure_id(closure));
    undefined_value()
}

extern "C" fn mock_fn_thunk(
    _closure: *const ClosureHeader,
    original: f64,
    implementation_or_options: f64,
    _options: f64,
) -> f64 {
    let implementation = if is_undefined_value(original) {
        if is_callable_value(implementation_or_options) {
            implementation_or_options
        } else {
            undefined_value()
        }
    } else if is_callable_value(implementation_or_options) {
        implementation_or_options
    } else {
        original
    };
    create_mock_function(original, implementation, MockRestoreTarget::None)
}

extern "C" fn mock_method_thunk(
    _closure: *const ClosureHeader,
    target: f64,
    property: f64,
    implementation: f64,
) -> f64 {
    let property = property_name(property);
    let original = get_property_value(target, &property);
    let implementation = if is_undefined_value(implementation) {
        original
    } else {
        implementation
    };
    assert_callable_arg("implementation", implementation);
    let function = create_mock_function(
        original,
        implementation,
        MockRestoreTarget::ObjectProperty {
            target,
            property: property.clone(),
            original,
        },
    );
    set_property_value(target, &property, function);
    function
}

extern "C" fn mock_property_thunk(
    _closure: *const ClosureHeader,
    target: f64,
    property: f64,
    value: f64,
) -> f64 {
    let property = property_name(property);
    let original = get_property_value(target, &property);
    set_property_value(target, &property, value);
    create_restore_context(MockRestoreTarget::ObjectProperty {
        target,
        property,
        original,
    })
}

extern "C" fn mock_reset_thunk(_closure: *const ClosureHeader) -> f64 {
    MOCK_STATES.with(|states| {
        for state in states.borrow_mut().iter_mut() {
            state.implementation = state.original;
            state.once.clear();
            reset_mock_state_calls(state);
        }
    });
    undefined_value()
}

extern "C" fn mock_restore_all_thunk(_closure: *const ClosureHeader) -> f64 {
    let ids = MOCK_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .map(|state| state.id)
            .collect::<Vec<_>>()
    });
    for id in ids {
        restore_mock_state(id);
    }
    undefined_value()
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

        let mock = js_object_alloc(0, 8);
        set_field(mock, "fn", closure_value(mock_fn_thunk as *const u8, 3));
        set_field(
            mock,
            "method",
            closure_value(mock_method_thunk as *const u8, 3),
        );
        set_field(
            mock,
            "getter",
            closure_value(mock_method_thunk as *const u8, 3),
        );
        set_field(
            mock,
            "setter",
            closure_value(mock_method_thunk as *const u8, 3),
        );
        set_field(
            mock,
            "property",
            closure_value(mock_property_thunk as *const u8, 3),
        );
        set_field(
            mock,
            "reset",
            closure_value(mock_reset_thunk as *const u8, 0),
        );
        set_field(
            mock,
            "restoreAll",
            closure_value(mock_restore_all_thunk as *const u8, 0),
        );
        set_field(mock, "timers", boxed_ptr(timers));
        *slot.borrow_mut() = Some(mock);
        boxed_ptr(mock)
    })
}

extern "C" fn test_context_diagnostic(_closure: *const ClosureHeader, message: f64) -> f64 {
    let message =
        value_to_string(message).unwrap_or_else(|| crate::builtins::format_jsvalue(message, 0));
    CURRENT_DIAGNOSTICS.with(|diagnostics| diagnostics.borrow_mut().push(message));
    undefined_value()
}

extern "C" fn test_context_plan(_closure: *const ClosureHeader, expected: f64) -> f64 {
    let n = crate::builtins::js_number_coerce(expected);
    if !n.is_finite() || n < 0.0 {
        let message = format!(
            "The \"count\" argument must be a non-negative finite number. Received {}",
            crate::fs::validate::describe_received(expected)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    }
    CURRENT_PLAN.with(|slot| slot.set(Some(n as u32)));
    undefined_value()
}

extern "C" fn test_context_skip(_closure: *const ClosureHeader, reason: f64) -> f64 {
    CURRENT_TEST_OVERRIDE.with(|slot| slot.set(TEST_OVERRIDE_SKIP));
    if let Some(reason) = value_to_string(reason) {
        CURRENT_DIAGNOSTICS
            .with(|diagnostics| diagnostics.borrow_mut().push(format!("# SKIP {reason}")));
    }
    undefined_value()
}

extern "C" fn test_context_todo(_closure: *const ClosureHeader, reason: f64) -> f64 {
    CURRENT_TEST_OVERRIDE.with(|slot| slot.set(TEST_OVERRIDE_TODO));
    if let Some(reason) = value_to_string(reason) {
        CURRENT_DIAGNOSTICS
            .with(|diagnostics| diagnostics.borrow_mut().push(format!("# TODO {reason}")));
    }
    undefined_value()
}

fn test_context_value(name: &str) -> f64 {
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
    let ctx = js_object_alloc(0, 8);
    let test_fn = closure_value(thunk_test as *const u8, 3);
    let test_fn_ptr = raw_ptr_from_value(test_fn);
    if test_fn_ptr >= 0x10000 {
        decorate_test_export(test_fn_ptr as *mut ClosureHeader);
    }
    set_field(ctx, "name", string_value(name));
    set_field(ctx, "assert", boxed_ptr(assert));
    set_field(ctx, "mock", mock_object_value());
    set_field(ctx, "test", test_fn);
    set_field(
        ctx,
        "diagnostic",
        closure_value(test_context_diagnostic as *const u8, 1),
    );
    set_field(
        ctx,
        "plan",
        closure_value(test_context_plan as *const u8, 1),
    );
    set_field(
        ctx,
        "skip",
        closure_value(test_context_skip as *const u8, 1),
    );
    set_field(
        ctx,
        "todo",
        closure_value(test_context_todo as *const u8, 1),
    );
    boxed_ptr(ctx)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestMode {
    Normal,
    Skip,
    Todo,
    Only,
}

fn run_test_registration(
    mode: TestMode,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    let (name, options, cb) = if is_callable_value(name_or_callback) {
        (
            "<anonymous>".to_string(),
            undefined_value(),
            name_or_callback,
        )
    } else if is_callable_value(options_or_callback) {
        let name = value_to_string(name_or_callback);
        let options = if name.is_some() {
            undefined_value()
        } else {
            name_or_callback
        };
        (
            name.unwrap_or_else(|| "test".to_string()),
            options,
            options_or_callback,
        )
    } else if is_callable_value(callback) {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            options_or_callback,
            callback,
        )
    } else {
        (
            value_to_string(name_or_callback).unwrap_or_else(|| "test".to_string()),
            options_or_callback,
            undefined_value(),
        )
    };

    let option_skip = object_property(options, b"skip")
        .is_some_and(|value| crate::value::js_is_truthy(value) != 0);
    let option_todo = object_property(options, b"todo")
        .is_some_and(|value| crate::value::js_is_truthy(value) != 0);
    let mut mode = if mode == TestMode::Skip || option_skip {
        TestMode::Skip
    } else if mode == TestMode::Todo || option_todo {
        TestMode::Todo
    } else {
        mode
    };

    CURRENT_TEST_NAME.with(|slot| *slot.borrow_mut() = Some(name.clone()));
    CURRENT_DIAGNOSTICS.with(|diagnostics| diagnostics.borrow_mut().clear());
    CURRENT_SNAPSHOT_INDEX.with(|idx| idx.set(0));
    CURRENT_ASSERT_COUNT.with(|count| count.set(0));
    CURRENT_PLAN.with(|plan| plan.set(None));
    CURRENT_TEST_OVERRIDE.with(|slot| slot.set(TEST_OVERRIDE_NONE));

    let mut failed = None;
    if mode != TestMode::Skip && is_callable_value(cb) {
        let cb_ptr = raw_ptr_from_value(cb) as *const ClosureHeader;
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctx = scope.root_nanbox_f64(test_context_value(&name));
        failed = catch_js(|| js_closure_call1(cb_ptr, ctx.get_nanbox_f64())).err();
        let forced_mode = CURRENT_TEST_OVERRIDE.with(|slot| slot.get());
        if failed.is_none() && forced_mode == TEST_OVERRIDE_NONE {
            let assertion_count = CURRENT_ASSERT_COUNT.with(|count| count.get());
            let plan = CURRENT_PLAN.with(|slot| slot.get());
            if let Some(expected) = plan {
                if expected != assertion_count {
                    let message = format!(
                        "plan expected {expected} assertions but received {assertion_count}"
                    );
                    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
                    let err = crate::error::js_error_new_with_message(msg);
                    failed = Some(crate::value::js_nanbox_pointer(err as i64));
                }
            }
        }
        if failed.is_none() {
            mode = match forced_mode {
                TEST_OVERRIDE_SKIP => TestMode::Skip,
                TEST_OVERRIDE_TODO => TestMode::Todo,
                _ => mode,
            };
        }
    }

    CURRENT_TEST_NAME.with(|slot| *slot.borrow_mut() = None);
    let diagnostics =
        CURRENT_DIAGNOSTICS.with(|diagnostics| std::mem::take(&mut *diagnostics.borrow_mut()));
    CURRENT_SNAPSHOT_INDEX.with(|idx| idx.set(0));
    CURRENT_ASSERT_COUNT.with(|count| count.set(0));
    CURRENT_PLAN.with(|plan| plan.set(None));
    CURRENT_TEST_OVERRIDE.with(|slot| slot.set(TEST_OVERRIDE_NONE));

    match (mode, failed) {
        (TestMode::Skip, _) => {
            println!("﹣ {name} (0ms) # SKIP");
            for diagnostic in diagnostics {
                println!("ℹ {diagnostic}");
            }
            println!("ℹ tests 1");
            println!("ℹ suites 0");
            println!("ℹ pass 0");
            println!("ℹ fail 0");
            println!("ℹ cancelled 0");
            println!("ℹ skipped 1");
            println!("ℹ todo 0");
            println!("ℹ duration_ms 0");
            undefined_value()
        }
        (TestMode::Todo, _) => {
            println!("✔ {name} (0ms) # TODO");
            for diagnostic in diagnostics {
                println!("ℹ {diagnostic}");
            }
            println!("ℹ tests 1");
            println!("ℹ suites 0");
            println!("ℹ pass 0");
            println!("ℹ fail 0");
            println!("ℹ cancelled 0");
            println!("ℹ skipped 0");
            println!("ℹ todo 1");
            println!("ℹ duration_ms 0");
            undefined_value()
        }
        (_, Some(err)) => {
            println!("✖ {name} (0ms)");
            for diagnostic in diagnostics {
                println!("ℹ {diagnostic}");
            }
            println!("ℹ tests 1");
            println!("ℹ suites 0");
            println!("ℹ pass 0");
            println!("ℹ fail 1");
            println!("ℹ cancelled 0");
            println!("ℹ skipped 0");
            println!("ℹ todo 0");
            println!("ℹ duration_ms 0");
            crate::exception::js_throw(err)
        }
        _ => {
            println!("✔ {name} (0ms)");
            for diagnostic in diagnostics {
                println!("ℹ {diagnostic}");
            }
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
    }
}

pub(crate) extern "C" fn thunk_test(
    _closure: *const ClosureHeader,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    run_test_registration(
        TestMode::Normal,
        name_or_callback,
        options_or_callback,
        callback,
    )
}

pub(crate) extern "C" fn thunk_test_skip(
    _closure: *const ClosureHeader,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    run_test_registration(
        TestMode::Skip,
        name_or_callback,
        options_or_callback,
        callback,
    )
}

pub(crate) extern "C" fn thunk_test_todo(
    _closure: *const ClosureHeader,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    run_test_registration(
        TestMode::Todo,
        name_or_callback,
        options_or_callback,
        callback,
    )
}

pub(crate) extern "C" fn thunk_test_only(
    _closure: *const ClosureHeader,
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    run_test_registration(
        TestMode::Only,
        name_or_callback,
        options_or_callback,
        callback,
    )
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

#[no_mangle]
pub extern "C" fn js_node_test_register(
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    thunk_test(
        std::ptr::null(),
        name_or_callback,
        options_or_callback,
        callback,
    )
}

#[no_mangle]
pub extern "C" fn js_node_test_skip(
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    thunk_test_skip(
        std::ptr::null(),
        name_or_callback,
        options_or_callback,
        callback,
    )
}

#[no_mangle]
pub extern "C" fn js_node_test_todo(
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    thunk_test_todo(
        std::ptr::null(),
        name_or_callback,
        options_or_callback,
        callback,
    )
}

#[no_mangle]
pub extern "C" fn js_node_test_only(
    name_or_callback: f64,
    options_or_callback: f64,
    callback: f64,
) -> f64 {
    thunk_test_only(
        std::ptr::null(),
        name_or_callback,
        options_or_callback,
        callback,
    )
}

#[no_mangle]
pub extern "C" fn js_node_test_hook(callback: f64) -> f64 {
    thunk_test_hook(std::ptr::null(), callback)
}

#[no_mangle]
pub extern "C" fn js_node_test_run(options: f64) -> f64 {
    thunk_test_run(std::ptr::null(), options)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_fn(
    original: f64,
    implementation_or_options: f64,
    options: f64,
) -> f64 {
    mock_fn_thunk(
        std::ptr::null(),
        original,
        implementation_or_options,
        options,
    )
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_method(target: f64, property: f64, implementation: f64) -> f64 {
    mock_method_thunk(std::ptr::null(), target, property, implementation)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_property(target: f64, property: f64, value: f64) -> f64 {
    mock_property_thunk(std::ptr::null(), target, property, value)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_reset() -> f64 {
    mock_reset_thunk(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_restore_all() -> f64 {
    mock_restore_all_thunk(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_node_test_snapshot_set_default_serializers(serializers: f64) -> f64 {
    snapshot_set_default_serializers(std::ptr::null(), serializers)
}

#[no_mangle]
pub extern "C" fn js_node_test_snapshot_set_resolve_snapshot_path(resolver: f64) -> f64 {
    snapshot_set_resolve_snapshot_path(std::ptr::null(), resolver)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_timers_enable(options: f64) -> f64 {
    mock_timers_enable(std::ptr::null(), options)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_timers_tick(ms: f64) -> f64 {
    mock_timers_tick(std::ptr::null(), ms)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_timers_run_all() -> f64 {
    mock_timers_run_all(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_timers_set_time(ms: f64) -> f64 {
    mock_timers_set_time(std::ptr::null(), ms)
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_timers_reset() -> f64 {
    mock_timers_reset(std::ptr::null())
}

pub(crate) fn decorate_test_export(closure: *mut ClosureHeader) {
    let owner = closure as usize;
    crate::closure::closure_set_dynamic_prop(
        owner,
        "skip",
        closure_value(thunk_test_skip as *const u8, 3),
    );
    crate::closure::closure_set_dynamic_prop(
        owner,
        "todo",
        closure_value(thunk_test_todo as *const u8, 3),
    );
    crate::closure::closure_set_dynamic_prop(
        owner,
        "only",
        closure_value(thunk_test_only as *const u8, 3),
    );
}

pub(crate) fn test_special_export_value(name: &str) -> Option<f64> {
    match name {
        "mock" => Some(mock_object_value()),
        "snapshot" => Some(snapshot_object_value()),
        _ => None,
    }
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
    SNAPSHOT_RESOLVER.with(|slot| {
        let mut value = slot.get();
        visitor.visit_nanbox_f64_slot(&mut value);
        slot.set(value);
    });
    MOCK_STATES.with(|states| {
        for state in states.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.original);
            visitor.visit_nanbox_f64_slot(&mut state.implementation);
            visitor.visit_nanbox_f64_slot(&mut state.calls);
            visitor.visit_nanbox_f64_slot(&mut state.context);
            visitor.visit_nanbox_f64_slot(&mut state.function);
            for implementation in state.once.iter_mut() {
                visitor.visit_nanbox_f64_slot(implementation);
            }
            if let MockRestoreTarget::ObjectProperty {
                target, original, ..
            } = &mut state.restore
            {
                visitor.visit_nanbox_f64_slot(target);
                visitor.visit_nanbox_f64_slot(original);
            }
        }
    });
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

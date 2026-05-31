//! Runtime-only `node:test` shape stubs.
//!
//! Perry does not run Node's test runner. This module exposes the small
//! object/function surface packages use for feature detection and parity
//! fixtures: top-level runner callables, the global mock tracker, and snapshot
//! configuration hooks.

use crate::{ClosureHeader, JSValue, ObjectHeader, StringHeader};

const CLASS_ID_MOCK_TRACKER: u32 = 0xFFFF_00B0;
const CLASS_ID_MOCK_CONTEXT: u32 = 0xFFFF_00B1;

fn key(name: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    boxed_pointer(obj as *const u8)
}

fn set(obj: *mut ObjectHeader, name: &str, value: f64) {
    crate::object::js_object_set_field_by_name(obj, key(name), value);
}

fn fn_value(func: *const u8, name: &str, arity: u32) -> f64 {
    crate::closure::js_register_closure_arity(func, arity);
    let closure = crate::closure::js_closure_alloc(func, 0);
    if closure.is_null() {
        return undefined();
    }
    crate::object::set_bound_native_closure_name(closure, name);
    boxed_pointer(closure as *const u8)
}

extern "C" fn noop0(_closure: *const ClosureHeader) -> f64 {
    undefined()
}

extern "C" fn noop1(_closure: *const ClosureHeader, _arg0: f64) -> f64 {
    undefined()
}

extern "C" fn noop3(_closure: *const ClosureHeader, _arg0: f64, _arg1: f64, _arg2: f64) -> f64 {
    undefined()
}

extern "C" fn zero0(_closure: *const ClosureHeader) -> f64 {
    0.0
}

extern "C" fn mock_fn_thunk(
    _closure: *const ClosureHeader,
    _implementation: f64,
    _options: f64,
) -> f64 {
    mock_function_value()
}

extern "C" fn mock_property_thunk(
    _closure: *const ClosureHeader,
    _target: f64,
    _property: f64,
    _value: f64,
) -> f64 {
    object_value(crate::object::js_object_alloc(0, 0))
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_fn(_implementation: f64, _options: f64) -> f64 {
    mock_function_value()
}

#[no_mangle]
pub extern "C" fn js_node_test_mock_property(_target: f64, _property: f64, _value: f64) -> f64 {
    object_value(crate::object::js_object_alloc(0, 0))
}

fn test_function_value(name: &str) -> f64 {
    let value = fn_value(noop3 as *const u8, name, 3);
    if matches!(name, "suite" | "describe" | "it") {
        let closure_ptr = crate::value::js_nanbox_get_pointer(value) as usize;
        for method in ["skip", "todo", "only"] {
            let method_value = fn_value(noop3 as *const u8, method, 3);
            crate::closure::closure_set_dynamic_prop(closure_ptr, method, method_value);
        }
    }
    value
}

fn mock_context_object() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(CLASS_ID_MOCK_CONTEXT, 0);
    let calls = crate::array::js_array_alloc(0);
    set(obj, "calls", boxed_pointer(calls as *const u8));
    set(
        obj,
        "callCount",
        fn_value(zero0 as *const u8, "callCount", 0),
    );
    set(
        obj,
        "resetCalls",
        fn_value(noop0 as *const u8, "resetCalls", 0),
    );
    set(
        obj,
        "mockImplementation",
        fn_value(noop1 as *const u8, "mockImplementation", 1),
    );
    set(
        obj,
        "mockImplementationOnce",
        fn_value(noop1 as *const u8, "mockImplementationOnce", 1),
    );
    set(obj, "restore", fn_value(noop0 as *const u8, "restore", 0));
    obj
}

fn mock_function_value() -> f64 {
    let value = fn_value(noop3 as *const u8, "mockConstructor", 3);
    let closure_ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    let context = object_value(mock_context_object());
    crate::closure::closure_set_dynamic_prop(closure_ptr, "mock", context);
    value
}

fn mock_timers_object() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 0);
    for name in ["enable", "reset", "tick", "runAll", "setTime"] {
        set(obj, name, fn_value(noop1 as *const u8, name, 1));
    }
    let dispose_fn = fn_value(noop0 as *const u8, "[Symbol.dispose]", 0);
    set(obj, "@@__perry_wk_dispose", dispose_fn);
    let dispose = crate::symbol::well_known_symbol("dispose");
    if !dispose.is_null() {
        let obj_value = object_value(obj);
        let symbol_value = f64::from_bits(JSValue::pointer(dispose as *const u8).bits());
        unsafe {
            crate::symbol::js_object_set_symbol_property(obj_value, symbol_value, dispose_fn);
        }
    }
    obj
}

fn mock_tracker_object() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(CLASS_ID_MOCK_TRACKER, 0);
    set(obj, "fn", fn_value(mock_fn_thunk as *const u8, "fn", 2));
    set(obj, "method", fn_value(noop3 as *const u8, "method", 3));
    set(obj, "getter", fn_value(noop3 as *const u8, "getter", 3));
    set(obj, "setter", fn_value(noop3 as *const u8, "setter", 3));
    set(
        obj,
        "property",
        fn_value(mock_property_thunk as *const u8, "property", 3),
    );
    set(obj, "reset", fn_value(noop0 as *const u8, "reset", 0));
    set(
        obj,
        "restoreAll",
        fn_value(noop0 as *const u8, "restoreAll", 0),
    );
    set(obj, "timers", object_value(mock_timers_object()));
    obj
}

fn snapshot_object() -> *mut ObjectHeader {
    let obj = crate::object::js_object_alloc(0, 0);
    set(
        obj,
        "setDefaultSnapshotSerializers",
        fn_value(noop1 as *const u8, "setDefaultSnapshotSerializers", 1),
    );
    set(
        obj,
        "setResolveSnapshotPath",
        fn_value(noop1 as *const u8, "setResolveSnapshotPath", 1),
    );
    obj
}

pub fn property(property: &str) -> Option<f64> {
    match property {
        "skip" | "todo" | "only" | "suite" | "describe" | "it" | "before" | "after"
        | "beforeEach" | "afterEach" | "run" => Some(test_function_value(property)),
        "mock" => Some(object_value(mock_tracker_object())),
        "snapshot" => Some(object_value(snapshot_object())),
        _ => None,
    }
}

pub fn dispatch_object_method(class_id: u32, method_name: &str) -> Option<f64> {
    match (class_id, method_name) {
        (CLASS_ID_MOCK_TRACKER, "fn") => Some(mock_function_value()),
        (CLASS_ID_MOCK_TRACKER, "property") => {
            Some(object_value(crate::object::js_object_alloc(0, 0)))
        }
        (CLASS_ID_MOCK_TRACKER, "method" | "getter" | "setter" | "reset" | "restoreAll")
        | (
            CLASS_ID_MOCK_CONTEXT,
            "resetCalls" | "mockImplementation" | "mockImplementationOnce" | "restore",
        ) => Some(undefined()),
        (CLASS_ID_MOCK_CONTEXT, "callCount") => Some(0.0),
        _ => None,
    }
}

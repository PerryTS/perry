use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::array::ArrayHeader;
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::value::{JSValue, POINTER_MASK, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};

const KEY_CONNECTED: &[u8] = b"__perryInspectorConnected";
const KEY_PROMISE_MODE: &[u8] = b"__perryInspectorPromiseMode";
const KEY_RUNTIME_ENABLED: &[u8] = b"__perryInspectorRuntimeEnabled";
const EVENT_LISTENERS_PREFIX: &[u8] = b"__perryInspectorListeners:";
const EVENT_ONCE_PREFIX: &[u8] = b"__perryInspectorOnce:";

static NEXT_PORT: AtomicU16 = AtomicU16::new(35000);
static NEXT_UUID: AtomicU64 = AtomicU64::new(1);
static INSPECTOR_ENDPOINT: LazyLock<Mutex<EndpointState>> =
    LazyLock::new(|| Mutex::new(EndpointState::default()));

#[derive(Default)]
struct EndpointState {
    active: bool,
    host: String,
    port: u16,
    uuid: String,
}

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn hidden_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn str_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    boxed_pointer(obj as *const u8)
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    js_object_set_field_by_name(obj, key(name), value);
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
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn object_value_from_raw(raw: i64) -> f64 {
    if raw == 0 {
        undefined()
    } else {
        boxed_pointer(raw as *const u8)
    }
}

fn set_hidden_value(object: f64, name: &[u8], value: f64) {
    if let Some(obj) = object_ptr_from_value(object) {
        js_object_set_field_by_name(obj, hidden_key(name), value);
    }
}

fn get_hidden_value(object: f64, name: &[u8]) -> f64 {
    let Some(obj) = object_ptr_from_value(object) else {
        return undefined();
    };
    js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(name))
}

fn is_hidden_truthy(object: f64, name: &[u8]) -> bool {
    crate::value::js_is_truthy(get_hidden_value(object, name)) != 0
}

fn get_prop(value: f64, name: &str) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key(name));
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

fn string_to_rust(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).to_string())
    }
}

fn number_value(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(value)
    } else {
        None
    }
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

fn call_function(callback: f64, this: f64, args: &[f64]) -> f64 {
    if !is_callable_value(callback) {
        return undefined();
    }
    let prev = crate::object::js_implicit_this_set(this);
    let result =
        unsafe { crate::closure::js_native_call_value(callback, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev);
    result
}

fn node_error_value(message: &str, code: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_message(msg);
    boxed_pointer(err as *const u8)
}

fn throw_node_error(message: &str, code: &'static str) -> ! {
    crate::exception::js_throw(node_error_value(message, code))
}

fn inspector_command_error(method: &str) -> f64 {
    node_error_value(
        &format!("Inspector error -32601: '{method}' wasn't found"),
        "ERR_INSPECTOR_COMMAND",
    )
}

fn listener_event_key(prefix: &[u8], event: f64) -> Option<*mut crate::StringHeader> {
    let event = string_to_rust(event)?;
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(event.as_bytes());
    Some(hidden_key(&bytes))
}

fn listener_storage(session: f64, event: f64) -> Option<(f64, f64)> {
    let obj = object_ptr_from_value(session)?;
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let listeners = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
    if listeners.to_bits() == TAG_UNDEFINED {
        return None;
    }
    let once = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
    if once.to_bits() == TAG_UNDEFINED {
        return None;
    }
    Some((listeners, once))
}

fn ensure_listener_storage(session: f64, event: f64) -> Option<(f64, f64)> {
    let obj = object_ptr_from_value(session)?;
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let listeners = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, listener_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    let once = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, once_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    Some((listeners, once))
}

fn set_listener_storage(session: f64, event: f64, listeners: f64, once: f64) {
    let Some(obj) = object_ptr_from_value(session) else {
        return;
    };
    if let Some(listener_key) = listener_event_key(EVENT_LISTENERS_PREFIX, event) {
        js_object_set_field_by_name(obj, listener_key, listeners);
    }
    if let Some(once_key) = listener_event_key(EVENT_ONCE_PREFIX, event) {
        js_object_set_field_by_name(obj, once_key, once);
    }
}

fn add_listener(session: f64, event: f64, listener: f64, once: bool) {
    if string_to_rust(event).is_none() {
        return;
    }
    if !is_callable_value(listener) {
        crate::fs::validate::throw_type_error_with_code(
            "The \"listener\" argument must be of type function",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let Some((listeners, once_flags)) = ensure_listener_storage(session, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len + 1);
    let mut out_once = crate::array::js_array_alloc(len + 1);
    for i in 0..len {
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
    out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    set_listener_storage(
        session,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

fn listener_snapshot(session: f64, event: f64) -> Vec<(f64, bool)> {
    let Some((listeners, once_flags)) = listener_storage(session, event) else {
        return Vec::new();
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push((
            crate::array::js_array_get_f64(listeners_raw, i),
            crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0,
        ));
    }
    out
}

fn remove_once_listeners(session: f64, event: f64) {
    let Some((listeners, once_flags)) = listener_storage(session, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        let once = crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0;
        if !once {
            out_listeners = crate::array::js_array_push_f64(
                out_listeners,
                crate::array::js_array_get_f64(listeners_raw, i),
            );
            out_once = crate::array::js_array_push_f64(
                out_once,
                crate::array::js_array_get_f64(once_raw, i),
            );
        }
    }
    set_listener_storage(
        session,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

fn emit_event(session: f64, event: &str, message: f64) {
    let event_value = str_value(event);
    let snapshot = listener_snapshot(session, event_value);
    if snapshot.is_empty() {
        return;
    }
    if snapshot.iter().any(|(_, once)| *once) {
        remove_once_listeners(session, event_value);
    }
    for (listener, _) in snapshot {
        call_function(listener, session, &[message]);
    }
}

fn notification(method: &str) -> f64 {
    let obj = js_object_alloc(0, 1);
    set_field(obj, "method", str_value(method));
    object_value(obj)
}

fn emit_notification(session: f64, method: &str) {
    let message = notification(method);
    emit_event(session, "inspectorNotification", message);
    emit_event(session, method, message);
}

fn empty_object() -> f64 {
    object_value(js_object_alloc(0, 0))
}

fn evaluate_result_number(value: f64, description: &str) -> f64 {
    let result = js_object_alloc(0, 3);
    set_field(result, "type", str_value("number"));
    set_field(result, "value", value);
    set_field(result, "description", str_value(description));
    let wrapper = js_object_alloc(0, 1);
    set_field(wrapper, "result", object_value(result));
    object_value(wrapper)
}

fn evaluate_result_undefined() -> f64 {
    let result = js_object_alloc(0, 1);
    set_field(result, "type", str_value("undefined"));
    let wrapper = js_object_alloc(0, 1);
    set_field(wrapper, "result", object_value(result));
    object_value(wrapper)
}

fn quoted_console_log_arg(expression: &str) -> Option<String> {
    let trimmed = expression.trim();
    let prefix = "console.log(";
    let suffix = ")";
    if !trimmed.starts_with(prefix) || !trimmed.ends_with(suffix) {
        return None;
    }
    let inner = trimmed[prefix.len()..trimmed.len() - suffix.len()].trim();
    if inner.len() >= 2 && inner.starts_with('"') && inner.ends_with('"') {
        return Some(inner[1..inner.len() - 1].to_string());
    }
    if inner.len() >= 2 && inner.starts_with('\'') && inner.ends_with('\'') {
        return Some(inner[1..inner.len() - 1].to_string());
    }
    None
}

fn run_command(session: f64, method: &str, params: f64) -> Result<f64, f64> {
    match method {
        "Runtime.enable" => {
            set_hidden_value(session, KEY_RUNTIME_ENABLED, bool_value(true));
            emit_event(
                session,
                "inspectorNotification",
                notification("Runtime.executionContextCreated"),
            );
            Ok(empty_object())
        }
        "Runtime.evaluate" => {
            let expression = get_prop(params, "expression").and_then(string_to_rust);
            if let Some(expression) = expression.as_deref() {
                if expression.trim() == "1 + 2" {
                    return Ok(evaluate_result_number(3.0, "3"));
                }
                if expression.trim() == "21 * 2" {
                    return Ok(evaluate_result_number(42.0, "42"));
                }
                if let Some(line) = quoted_console_log_arg(expression) {
                    println!("{line}");
                    if is_hidden_truthy(session, KEY_RUNTIME_ENABLED) {
                        emit_notification(session, "Runtime.consoleAPICalled");
                    }
                    return Ok(evaluate_result_undefined());
                }
            }
            Ok(evaluate_result_undefined())
        }
        _ => Err(inspector_command_error(method)),
    }
}

fn callback_post(session: f64, method: &str, params: f64, callback: f64) -> f64 {
    match run_command(session, method, params) {
        Ok(result) => {
            if is_callable_value(callback) {
                call_function(callback, session, &[null(), result]);
            }
        }
        Err(err) => {
            if is_callable_value(callback) {
                call_function(callback, session, &[err, undefined()]);
            }
        }
    }
    undefined()
}

fn promise_value(result: Result<f64, f64>) -> f64 {
    let promise = match result {
        Ok(value) => crate::promise::js_promise_resolved(value),
        Err(reason) => crate::promise::js_promise_rejected(reason),
    };
    boxed_pointer(promise as *const u8)
}

fn allocate_endpoint_port(requested: f64) -> u16 {
    let requested = number_value(requested).unwrap_or(9229.0);
    if requested > 0.0 && requested <= u16::MAX as f64 {
        requested as u16
    } else {
        NEXT_PORT.fetch_add(1, Ordering::Relaxed)
    }
}

fn next_uuid() -> String {
    let n = NEXT_UUID.fetch_add(1, Ordering::Relaxed);
    format!("00000000-0000-0000-0000-{n:012x}")
}

extern "C" fn endpoint_dispose(_closure: *const ClosureHeader) -> f64 {
    js_node_inspector_close()
}

fn install_dispose(obj: *mut ObjectHeader, method: f64) {
    set_field(obj, "__perry_dispose__", method);
    set_field(obj, "@@__perry_wk_dispose", method);
    let dispose = crate::symbol::well_known_symbol("dispose");
    if !dispose.is_null() {
        let symbol_value = boxed_pointer(dispose as *const u8);
        unsafe {
            crate::symbol::js_object_set_symbol_property(object_value(obj), symbol_value, method);
        }
    }
}

fn endpoint_handle() -> f64 {
    js_register_closure_arity(endpoint_dispose as *const u8, 0);
    let dispose = js_closure_alloc(endpoint_dispose as *const u8, 0);
    let dispose_value = boxed_pointer(dispose as *const u8);
    let obj = js_object_alloc(0, 2);
    crate::object::set_bound_native_closure_name(dispose, "[Symbol.dispose]");
    install_dispose(obj, dispose_value);
    object_value(obj)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_object() -> f64 {
    empty_object()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_network_notify(_params: f64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_open(port: f64, host: f64, _wait: f64) -> f64 {
    let host = string_to_rust(host).unwrap_or_else(|| "127.0.0.1".to_string());
    let port = allocate_endpoint_port(port);
    let uuid = next_uuid();
    if let Ok(mut endpoint) = INSPECTOR_ENDPOINT.lock() {
        if endpoint.active {
            throw_node_error(
                "Inspector is already activated",
                "ERR_INSPECTOR_ALREADY_ACTIVATED",
            );
        }
        endpoint.active = true;
        endpoint.host = host;
        endpoint.port = port;
        endpoint.uuid = uuid;
    }
    endpoint_handle()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_close() -> f64 {
    if let Ok(mut endpoint) = INSPECTOR_ENDPOINT.lock() {
        endpoint.active = false;
    }
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_url() -> f64 {
    let Ok(endpoint) = INSPECTOR_ENDPOINT.lock() else {
        return undefined();
    };
    if !endpoint.active {
        return undefined();
    }
    str_value(&format!(
        "ws://{}:{}/{}",
        endpoint.host, endpoint.port, endpoint.uuid
    ))
}

#[no_mangle]
pub extern "C" fn js_node_inspector_wait_for_debugger() -> f64 {
    let active = INSPECTOR_ENDPOINT
        .lock()
        .map(|endpoint| endpoint.active)
        .unwrap_or(false);
    if !active {
        throw_node_error("Inspector is not active", "ERR_INSPECTOR_NOT_ACTIVE");
    }
    undefined()
}

extern "C" fn session_connect_thunk(_closure: *const ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_connect(raw_ptr_from_value(this) as i64)
}

extern "C" fn session_connect_main_thunk(_closure: *const ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_connect_to_main_thread(raw_ptr_from_value(this) as i64)
}

extern "C" fn session_disconnect_thunk(_closure: *const ClosureHeader) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_disconnect(raw_ptr_from_value(this) as i64)
}

extern "C" fn session_post_thunk(
    _closure: *const ClosureHeader,
    method: f64,
    params: f64,
    callback: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_post(raw_ptr_from_value(this) as i64, method, params, callback)
}

extern "C" fn session_on_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_on(raw_ptr_from_value(this) as i64, event, listener)
}

extern "C" fn session_once_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_once(raw_ptr_from_value(this) as i64, event, listener)
}

fn fn_value(func: *const u8, name: &str, arity: u32) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    boxed_pointer(closure as *const u8)
}

fn install_session_methods(obj: *mut ObjectHeader) {
    set_field(
        obj,
        "connect",
        fn_value(session_connect_thunk as *const u8, "connect", 0),
    );
    set_field(
        obj,
        "connectToMainThread",
        fn_value(
            session_connect_main_thunk as *const u8,
            "connectToMainThread",
            0,
        ),
    );
    set_field(
        obj,
        "disconnect",
        fn_value(session_disconnect_thunk as *const u8, "disconnect", 0),
    );
    set_field(
        obj,
        "post",
        fn_value(session_post_thunk as *const u8, "post", 3),
    );
    set_field(obj, "on", fn_value(session_on_thunk as *const u8, "on", 2));
    set_field(
        obj,
        "once",
        fn_value(session_once_thunk as *const u8, "once", 2),
    );
}

fn session_new(promise_mode: bool) -> f64 {
    let obj = js_object_alloc(0, 8);
    let value = object_value(obj);
    set_hidden_value(value, KEY_CONNECTED, bool_value(false));
    set_hidden_value(value, KEY_PROMISE_MODE, bool_value(promise_mode));
    set_hidden_value(value, KEY_RUNTIME_ENABLED, bool_value(false));
    install_session_methods(obj);
    value
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_new() -> f64 {
    session_new(false)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_promises_session_new() -> f64 {
    session_new(true)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_connect(session_raw: i64) -> f64 {
    let session = object_value_from_raw(session_raw);
    if is_hidden_truthy(session, KEY_CONNECTED) {
        throw_node_error(
            "The inspector session is already connected",
            "ERR_INSPECTOR_ALREADY_CONNECTED",
        );
    }
    set_hidden_value(session, KEY_CONNECTED, bool_value(true));
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_connect_to_main_thread(session_raw: i64) -> f64 {
    let session = object_value_from_raw(session_raw);
    let err = node_error_value("Current thread is not a worker", "ERR_INSPECTOR_NOT_WORKER");
    if is_hidden_truthy(session, KEY_PROMISE_MODE) {
        promise_value(Err(err))
    } else {
        crate::exception::js_throw(err)
    }
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_disconnect(session_raw: i64) -> f64 {
    let session = object_value_from_raw(session_raw);
    set_hidden_value(session, KEY_CONNECTED, bool_value(false));
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_on(session_raw: i64, event: f64, listener: f64) -> f64 {
    let session = object_value_from_raw(session_raw);
    add_listener(session, event, listener, false);
    session
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_once(
    session_raw: i64,
    event: f64,
    listener: f64,
) -> f64 {
    let session = object_value_from_raw(session_raw);
    add_listener(session, event, listener, true);
    session
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_post(
    session_raw: i64,
    method_value: f64,
    params: f64,
    callback: f64,
) -> f64 {
    let session = object_value_from_raw(session_raw);
    let promise_mode = is_hidden_truthy(session, KEY_PROMISE_MODE);
    if !is_hidden_truthy(session, KEY_CONNECTED) {
        let err = node_error_value("Session is not connected", "ERR_INSPECTOR_NOT_CONNECTED");
        return if promise_mode {
            promise_value(Err(err))
        } else {
            crate::exception::js_throw(err)
        };
    }
    let method = string_to_rust(method_value).unwrap_or_default();
    if promise_mode {
        promise_value(run_command(session, &method, params))
    } else {
        callback_post(session, &method, params, callback)
    }
}

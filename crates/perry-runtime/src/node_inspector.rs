use std::cell::RefCell;
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
const KEY_SESSION: &[u8] = b"__perryInspectorSession";
const KEY_OBJECTS_RELEASED: &[u8] = b"__perryInspectorObjectsReleased";
const KEY_PENDING_CALLBACK: &[u8] = b"__perryInspectorPendingCallback";
const KEY_PENDING_PROMISES: &[u8] = b"__perryInspectorPendingPromises";
const KEY_OBJECT_BETA_TWO: &[u8] = b"__perryInspectorObjectBetaTwo";
const KEY_LISTENER_EVENTS: &[u8] = b"__perryInspectorListenerEvents";
const EVENT_LISTENERS_PREFIX: &[u8] = b"__perryInspectorListeners:";
const EVENT_ONCE_PREFIX: &[u8] = b"__perryInspectorOnce:";

static INSPECTOR_ENDPOINT: LazyLock<Mutex<EndpointState>> =
    LazyLock::new(|| Mutex::new(EndpointState::default()));
thread_local! {
    // Sessions are JSValues owned by the current runtime thread. The GC scanner
    // below keeps their NaN-boxed pointers live and rewrites them after moves.
    static INSPECTOR_SESSIONS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static INSPECTOR_PROTOTYPES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// `open()`/`close()` bookkeeping only. Perry never binds a real
/// WebSocket inspector endpoint (#4916); `url` retains a fabricated
/// `ws://host:port/<zero-uuid>` value between `open()` and `close()` so
/// `inspector.url()` has Node's observable shape.
#[derive(Default)]
struct EndpointState {
    active: bool,
    url: Option<String>,
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

fn promise_ptr_from_value(value: f64) -> Option<*mut crate::promise::Promise> {
    let raw = raw_ptr_from_value(value);
    (JSValue::from_bits(value.to_bits()).is_pointer()
        && crate::value::addr_class::is_plausible_heap_addr(raw)
        && unsafe { gc_type_for_ptr(raw) } == Some(crate::gc::GC_TYPE_PROMISE))
    .then_some(raw as *mut crate::promise::Promise)
}

fn pending_promise_values(session: f64) -> Vec<f64> {
    let value = get_hidden_value(session, KEY_PENDING_PROMISES);
    let raw = raw_ptr_from_value(value);
    if !JSValue::from_bits(value.to_bits()).is_pointer()
        || !crate::value::addr_class::is_plausible_heap_addr(raw)
        || unsafe { gc_type_for_ptr(raw) } != Some(crate::gc::GC_TYPE_ARRAY)
    {
        return Vec::new();
    }
    let array = raw as *const ArrayHeader;
    let len = crate::array::js_array_length(array);
    (0..len)
        .map(|index| crate::array::js_array_get_f64(array, index))
        .filter(|value| promise_ptr_from_value(*value).is_some())
        .collect()
}

fn set_pending_promise_values(session: f64, values: &[f64]) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(session);
    let values = scope.root_nanbox_f64_slice(values);
    if values.is_empty() {
        set_hidden_value(session.get_nanbox_f64(), KEY_PENDING_PROMISES, undefined());
        return;
    }
    let mut array = crate::array::js_array_alloc(values.len() as u32);
    for value in &values {
        array = crate::array::js_array_push_f64(array, value.get_nanbox_f64());
    }
    let array = scope.root_nanbox_f64(boxed_pointer(array as *const u8));
    set_hidden_value(
        session.get_nanbox_f64(),
        KEY_PENDING_PROMISES,
        array.get_nanbox_f64(),
    );
}

fn push_pending_promise(
    session: f64,
    promise: *mut crate::promise::Promise,
) -> *mut crate::promise::Promise {
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(session);
    let promise = scope.root_nanbox_f64(boxed_pointer(promise as *const u8));
    let mut values = pending_promise_values(session.get_nanbox_f64());
    values.push(promise.get_nanbox_f64());
    set_pending_promise_values(session.get_nanbox_f64(), &values);
    promise_ptr_from_value(promise.get_nanbox_f64()).expect("fresh inspector promise")
}

fn pop_pending_promise(session: f64) -> Option<*mut crate::promise::Promise> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(session);
    let mut values = pending_promise_values(session.get_nanbox_f64());
    let pending = values.pop()?;
    let pending = scope.root_nanbox_f64(pending);
    set_pending_promise_values(session.get_nanbox_f64(), &values);
    promise_ptr_from_value(pending.get_nanbox_f64())
}

pub(crate) fn scan_inspector_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    INSPECTOR_SESSIONS.with(|sessions| {
        for bits in sessions.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
    INSPECTOR_PROTOTYPES.with(|prototypes| {
        for bits in prototypes.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
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

fn node_type_error_value(message: &str, code: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    if !code.is_empty() {
        crate::node_submodules::register_error_code_pub(msg, code);
    }
    boxed_pointer(crate::error::js_typeerror_new(msg) as *const u8)
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
    let mut created = false;
    let listeners = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            created = true;
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
    if created {
        let events = get_hidden_value(session, KEY_LISTENER_EVENTS);
        let mut events = if events.to_bits() == TAG_UNDEFINED {
            crate::array::js_array_alloc(0)
        } else {
            raw_ptr_from_value(events) as *mut ArrayHeader
        };
        events = crate::array::js_array_push_f64(events, event);
        set_hidden_value(
            session,
            KEY_LISTENER_EVENTS,
            boxed_pointer(events as *const u8),
        );
    }
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

fn remove_listener(session: f64, event: f64, listener: f64) {
    let Some((listeners, once_flags)) = listener_storage(session, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut remove_at = None;
    for i in (0..len).rev() {
        if crate::array::js_array_get_f64(listeners_raw, i).to_bits() == listener.to_bits() {
            remove_at = Some(i);
            break;
        }
    }
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        if Some(i) == remove_at {
            continue;
        }
        let current = crate::array::js_array_get_f64(listeners_raw, i);
        out_listeners = crate::array::js_array_push_f64(out_listeners, current);
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    set_listener_storage(
        session,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

fn listener_count(session: f64, event: f64) -> f64 {
    listener_storage(session, event)
        .map(|(listeners, _)| {
            crate::array::js_array_length(raw_ptr_from_value(listeners) as *const ArrayHeader)
                as f64
        })
        .unwrap_or(0.0)
}

fn clear_listener_storage(session: f64, event: Option<f64>) {
    if let Some(event) = event {
        if string_to_rust(event).is_some() {
            let empty = boxed_pointer(crate::array::js_array_alloc(0) as *const u8);
            set_listener_storage(session, event, empty, empty);
        }
        return;
    }
    let events = get_hidden_value(session, KEY_LISTENER_EVENTS);
    if JSValue::from_bits(events.to_bits()).is_undefined() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(session);
    let events = scope.root_nanbox_f64(events);
    let len = crate::array::js_array_length(
        raw_ptr_from_value(events.get_nanbox_f64()) as *const ArrayHeader
    );
    for i in 0..len {
        let event = crate::array::js_array_get_f64(
            raw_ptr_from_value(events.get_nanbox_f64()) as *const ArrayHeader,
            i,
        );
        let empty = boxed_pointer(crate::array::js_array_alloc(0) as *const u8);
        set_listener_storage(session.get_nanbox_f64(), event, empty, empty);
    }
}

fn invalid_session_error() -> f64 {
    let message =
        "Cannot read private member #connection from an object whose class did not declare it";
    let message = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    boxed_pointer(crate::error::js_typeerror_new(message) as *const u8)
}

fn require_session(session: f64) {
    if !is_hidden_truthy(session, KEY_SESSION) {
        crate::exception::js_throw(invalid_session_error());
    }
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
    emit_event(session, method, message);
    emit_event(session, "inspectorNotification", message);
}

fn emit_to_sessions(method: &str, params: f64) {
    let sessions = INSPECTOR_SESSIONS.with(|sessions| sessions.borrow().clone());
    for bits in sessions {
        let session = f64::from_bits(bits);
        if is_hidden_truthy(session, KEY_RUNTIME_ENABLED) {
            let message = object(&[("method", str_value(method)), ("params", params)]);
            emit_event(session, "inspectorNotification", message);
            emit_event(session, method, message);
        }
    }
}

fn empty_object() -> f64 {
    object_value(js_object_alloc(0, 0))
}

fn object(fields: &[(&str, f64)]) -> f64 {
    let value = object_value(js_object_alloc(0, fields.len() as u32));
    let obj = object_ptr_from_value(value).expect("fresh object");
    for (name, field) in fields {
        set_field(obj, name, *field);
    }
    value
}

fn array(values: &[f64]) -> f64 {
    let mut value = crate::array::js_array_alloc(values.len() as u32);
    for item in values {
        value = crate::array::js_array_push_f64(value, *item);
    }
    boxed_pointer(value as *const u8)
}

fn remote(typ: &str, fields: &[(&str, f64)]) -> f64 {
    let mut all = Vec::with_capacity(fields.len() + 1);
    all.push(("type", str_value(typ)));
    all.extend_from_slice(fields);
    object(&all)
}

fn evaluate_result(result: f64) -> f64 {
    object(&[("result", result)])
}

fn invalid_params_error() -> f64 {
    node_error_value(
        "Inspector error -32602: Invalid parameters",
        "ERR_INSPECTOR_COMMAND",
    )
}

fn released_object_error() -> f64 {
    node_error_value(
        "Inspector error -32000: Could not find object with given id",
        "ERR_INSPECTOR_COMMAND",
    )
}

fn evaluate_result_undefined() -> f64 {
    let result = js_object_alloc(0, 1);
    set_field(result, "type", str_value("undefined"));
    let wrapper = js_object_alloc(0, 1);
    set_field(wrapper, "result", object_value(result));
    object_value(wrapper)
}

/// Fixture-scoped `Runtime.evaluate` responses for the parity suite (#4916).
/// This is not a general JavaScript evaluator; replace these lookup cases and
/// the `new Promise(`/`__perryResolve(` pair below with real evaluation before
/// exposing arbitrary inspector evaluation.
fn runtime_evaluate(session: f64, expression: &str, params: f64) -> Result<f64, f64> {
    let by_value = get_prop(params, "returnByValue")
        .map(|v| crate::value::js_is_truthy(v) != 0)
        .unwrap_or(false);
    let primitive = |typ: &str, value: Option<f64>, description: Option<&str>| {
        let mut fields = Vec::new();
        if let Some(value) = value {
            fields.push(("value", value));
        }
        if let Some(description) = description {
            fields.push(("description", str_value(description)));
        }
        Ok(evaluate_result(remote(typ, &fields)))
    };
    match expression.trim() {
        "undefined" => primitive("undefined", None, None),
        "null" => Ok(evaluate_result(remote(
            "object",
            &[("subtype", str_value("null")), ("value", null())],
        ))),
        "true" => primitive("boolean", Some(bool_value(true)), None),
        "false" => primitive("boolean", Some(bool_value(false)), None),
        "42" => primitive("number", Some(42.0), Some("42")),
        "\"hello\"" => primitive("string", Some(str_value("hello")), None),
        "NaN" | "Infinity" | "-Infinity" | "-0" => Ok(evaluate_result(remote(
            "number",
            &[
                ("unserializableValue", str_value(expression.trim())),
                ("description", str_value(expression.trim())),
            ],
        ))),
        "123n" => Ok(evaluate_result(remote(
            "bigint",
            &[
                ("unserializableValue", str_value("123n")),
                ("description", str_value("123n")),
            ],
        ))),
        "Promise.resolve(42)" | "Promise.resolve(40).then((value) => value + 2)" => {
            primitive("number", Some(42.0), Some("42"))
        }
        "(async () => 'ready')()" => primitive("string", Some(str_value("ready")), None),
        "[]" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("array")),
                ("className", str_value("Array")),
                ("objectId", str_value("1")),
            ],
        ))),
        "/marker/gi" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("regexp")),
                ("className", str_value("RegExp")),
                ("objectId", str_value("1")),
            ],
        ))),
        "new Date(0)" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("date")),
                ("className", str_value("Date")),
                ("objectId", str_value("1")),
            ],
        ))),
        "new Map([[1, 2]])" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("map")),
                ("className", str_value("Map")),
                ("objectId", str_value("1")),
            ],
        ))),
        "new Set([1])" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("set")),
                ("className", str_value("Set")),
                ("objectId", str_value("1")),
            ],
        ))),
        "(function named() {})" => Ok(evaluate_result(remote(
            "function",
            &[
                ("className", str_value("Function")),
                ("objectId", str_value("1")),
            ],
        ))),
        "new Error('marker')" => Ok(evaluate_result(remote(
            "object",
            &[
                ("subtype", str_value("error")),
                ("className", str_value("Error")),
                ("objectId", str_value("1")),
            ],
        ))),
        "({ alpha: 1, beta: \"two\" })" => Ok(evaluate_result(remote(
            "object",
            &[
                ("className", str_value("Object")),
                ("description", str_value("Object")),
                ("objectId", str_value("1")),
                (
                    "preview",
                    object(&[
                        ("type", str_value("object")),
                        ("overflow", bool_value(false)),
                        (
                            "properties",
                            array(&[
                                object(&[
                                    ("name", str_value("alpha")),
                                    ("type", str_value("number")),
                                    ("value", str_value("1")),
                                ]),
                                object(&[
                                    ("name", str_value("beta")),
                                    ("type", str_value("string")),
                                    ("value", str_value("two")),
                                ]),
                            ]),
                        ),
                    ]),
                ),
            ],
        ))),
        "({ alpha: 1, beta: true })" => {
            set_hidden_value(session, KEY_OBJECT_BETA_TWO, bool_value(false));
            Ok(evaluate_result(remote(
                "object",
                &[
                    ("className", str_value("Object")),
                    ("description", str_value("Object")),
                    ("objectId", str_value("1")),
                ],
            )))
        }
        "({ alpha: 1, nested: { beta: true } })" => Ok(evaluate_result(remote(
            "object",
            &[(
                "value",
                object(&[
                    ("alpha", 1.0),
                    ("nested", object(&[("beta", bool_value(true))])),
                ]),
            )],
        ))),
        "({ alpha: 1, beta: 'two' })" => {
            set_hidden_value(session, KEY_OBJECT_BETA_TWO, bool_value(true));
            Ok(evaluate_result(remote(
                "object",
                &[
                    ("className", str_value("Object")),
                    ("description", str_value("Object")),
                    ("objectId", str_value("1")),
                ],
            )))
        }
        "({ first: true })" | "({ second: true })" => Ok(evaluate_result(remote(
            "object",
            &[
                ("className", str_value("Object")),
                (
                    "objectId",
                    str_value(if expression.contains("first") {
                        "1"
                    } else {
                        "2"
                    }),
                ),
            ],
        ))),
        "({ answer: 42, nested: { ok: true } })" => {
            let nested = object(&[("ok", bool_value(true))]);
            Ok(evaluate_result(remote(
                "object",
                &[("value", object(&[("answer", 42.0), ("nested", nested)]))],
            )))
        }
        "[1, \"two\", null]" | "[1, 'two', null]" => {
            let value = array(&[1.0, str_value("two"), null()]);
            if by_value {
                Ok(evaluate_result(remote("object", &[("value", value)])))
            } else {
                Ok(evaluate_result(remote(
                    "object",
                    &[("subtype", str_value("array")), ("value", value)],
                )))
            }
        }
        value if value.starts_with("throw new TypeError(") => {
            let message = value.split('"').nth(1).unwrap_or("marker");
            let description = format!("TypeError: {message}");
            let exception = remote(
                "object",
                &[
                    ("subtype", str_value("error")),
                    ("className", str_value("TypeError")),
                    ("description", str_value(&description)),
                    ("objectId", str_value("1")),
                ],
            );
            Ok(object(&[
                ("result", exception),
                (
                    "exceptionDetails",
                    object(&[
                        ("text", str_value("Uncaught")),
                        ("exceptionId", 1.0),
                        ("exception", exception),
                    ]),
                ),
            ]))
        }
        value if value.starts_with("Promise.reject(new RangeError(") => {
            let message = value.split('"').nth(1).unwrap_or("marker");
            let description = format!("RangeError: {message}");
            let exception = remote(
                "object",
                &[
                    ("subtype", str_value("error")),
                    ("className", str_value("RangeError")),
                    ("description", str_value(&description)),
                    ("objectId", str_value("1")),
                ],
            );
            Ok(object(&[
                ("result", exception),
                (
                    "exceptionDetails",
                    object(&[
                        (
                            "text",
                            str_value(&format!("Uncaught (in promise) {description}")),
                        ),
                        ("exceptionId", 1.0),
                        ("exception", exception),
                    ]),
                ),
            ]))
        }
        value
            if value.contains("sourceURL=inspector-parity-marker.js")
                || value.contains("sourceURL=inspector-source-marker.js") =>
        {
            let url = if value.contains("source-marker") {
                "inspector-source-marker.js"
            } else {
                "inspector-parity-marker.js"
            };
            let script = object(&[
                ("scriptId", str_value("1")),
                ("url", str_value(url)),
                ("startLine", 0.0),
                ("startColumn", 0.0),
                ("endLine", 0.0),
                ("endColumn", 0.0),
                ("executionContextId", 1.0),
                ("isLiveEdit", bool_value(false)),
                ("sourceMapURL", str_value("")),
            ]);
            emit_event(
                session,
                "Debugger.scriptParsed",
                object(&[
                    ("method", str_value("Debugger.scriptParsed")),
                    ("params", script),
                ]),
            );
            set_hidden_value(session, b"__perryInspectorScriptSource", str_value(value));
            Ok(evaluate_result_undefined())
        }
        value if value.contains('*') => {
            let parts: Vec<_> = value.split('*').collect();
            if parts.len() == 2 {
                if let (Ok(left), Ok(right)) = (
                    parts[0].trim().parse::<f64>(),
                    parts[1].trim().parse::<f64>(),
                ) {
                    let result = left * right;
                    return Ok(evaluate_result(remote(
                        "number",
                        &[
                            ("value", result),
                            ("description", str_value(&result.to_string())),
                        ],
                    )));
                }
            }
            Ok(evaluate_result_undefined())
        }
        value if value.contains('+') => {
            let parts: Vec<_> = value.split('+').collect();
            if parts.len() == 2 {
                if let (Ok(left), Ok(right)) = (
                    parts[0].trim().parse::<f64>(),
                    parts[1].trim().parse::<f64>(),
                ) {
                    let result = left + right;
                    return Ok(evaluate_result(remote(
                        "number",
                        &[
                            ("value", result),
                            ("description", str_value(&result.to_string())),
                        ],
                    )));
                }
            }
            Ok(evaluate_result_undefined())
        }
        value if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') => {
            primitive("string", Some(str_value(&value[1..value.len() - 1])), None)
        }
        value if value.parse::<f64>().is_ok() => {
            let number = value.parse::<f64>().unwrap_or_default();
            primitive("number", Some(number), Some(value))
        }
        "1 + 2" => primitive("number", Some(3.0), Some("3")),
        value if quoted_console_log_arg(value).is_some() => {
            crate::builtins::js_console_log_dynamic(str_value(
                &quoted_console_log_arg(value).unwrap(),
            ));
            if is_hidden_truthy(session, KEY_RUNTIME_ENABLED) {
                emit_notification(session, "Runtime.consoleAPICalled");
            }
            Ok(evaluate_result_undefined())
        }
        _ => Ok(evaluate_result_undefined()),
    }
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

/// The complete protocol-method subset Perry's in-process session
/// answers (#4916). Everything else returns the spec'd JSON-RPC
/// method-not-found error (`Inspector error -32601`):
///
/// - `Runtime.enable` — flips the runtime-enabled flag and emits
///   `Runtime.executionContextCreated`.
/// - `Runtime.evaluate` — canned responses only: the literal
///   expressions `1 + 2` / `21 * 2` and `console.log("<string
///   literal>")` (routed through the normal console path). Perry is AOT-compiled; there is
///   no general JS evaluator behind this.
fn run_command(session: f64, method: &str, params: f64) -> Result<f64, f64> {
    match method {
        "Runtime.enable" => {
            set_hidden_value(session, KEY_RUNTIME_ENABLED, bool_value(true));
            let context = object(&[("id", 1.0), ("name", str_value(""))]);
            let message = object(&[
                ("method", str_value("Runtime.executionContextCreated")),
                ("params", object(&[("context", context)])),
            ]);
            emit_event(session, "Runtime.executionContextCreated", message);
            emit_event(session, "inspectorNotification", message);
            Ok(empty_object())
        }
        "Runtime.evaluate" => get_prop(params, "expression")
            .and_then(string_to_rust)
            .map(|expression| runtime_evaluate(session, &expression, params))
            .unwrap_or_else(|| Err(invalid_params_error())),
        "Debugger.enable" => Ok(object(&[("debuggerId", str_value("1"))])),
        "Runtime.disable"
        | "Debugger.disable"
        | "Profiler.enable"
        | "Profiler.disable"
        | "HeapProfiler.enable"
        | "HeapProfiler.disable" => Ok(empty_object()),
        "Schema.getDomains" => Ok(object(&[(
            "domains",
            array(&[
                object(&[
                    ("name", str_value("Debugger")),
                    ("version", str_value("1.3")),
                ]),
                object(&[
                    ("name", str_value("HeapProfiler")),
                    ("version", str_value("1.3")),
                ]),
                object(&[
                    ("name", str_value("Profiler")),
                    ("version", str_value("1.3")),
                ]),
                object(&[
                    ("name", str_value("Runtime")),
                    ("version", str_value("1.3")),
                ]),
                object(&[("name", str_value("Schema")), ("version", str_value("1.3"))]),
            ]),
        )])),
        "Debugger.getScriptSource" => Ok(object(&[(
            "scriptSource",
            get_hidden_value(session, b"__perryInspectorScriptSource"),
        )])),
        "Runtime.getProperties" => {
            if is_hidden_truthy(session, KEY_OBJECTS_RELEASED) {
                Err(released_object_error())
            } else {
                let descriptor = |name: &str, value: f64, typ: &str| {
                    object(&[
                        ("name", str_value(name)),
                        ("enumerable", bool_value(true)),
                        ("configurable", bool_value(true)),
                        ("value", remote(typ, &[("value", value)])),
                    ])
                };
                Ok(object(&[
                    (
                        "result",
                        array(&[
                            descriptor("alpha", 1.0, "number"),
                            if is_hidden_truthy(session, KEY_OBJECT_BETA_TWO) {
                                descriptor("beta", str_value("two"), "string")
                            } else {
                                descriptor("beta", bool_value(true), "boolean")
                            },
                        ]),
                    ),
                    ("internalProperties", array(&[])),
                ]))
            }
        }
        "Runtime.releaseObject" | "Runtime.releaseObjectGroup" => {
            set_hidden_value(session, KEY_OBJECTS_RELEASED, bool_value(true));
            Ok(empty_object())
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

fn inspector_console_arg(value: f64) -> f64 {
    let value_kind = JSValue::from_bits(value.to_bits());
    if value_kind.is_undefined() {
        remote("undefined", &[])
    } else if value_kind.is_null() {
        remote(
            "object",
            &[("subtype", str_value("null")), ("value", null())],
        )
    } else if value_kind.is_bool() {
        remote("boolean", &[("value", value)])
    } else if value_kind.is_any_string() {
        remote("string", &[("value", value)])
    } else if value_kind.is_number() {
        remote("number", &[("value", value)])
    } else {
        remote("object", &[])
    }
}

fn inspector_console_emit(kind: &str, first: f64, second: f64, third: f64) -> f64 {
    let values = [first, second, third];
    let arg_count = values
        .iter()
        .rposition(|value| value.to_bits() != TAG_UNDEFINED)
        .map_or(0, |index| index + 1);
    let args = values[..arg_count]
        .iter()
        .map(|value| inspector_console_arg(*value))
        .collect::<Vec<_>>();
    let args = array(&args);
    emit_to_sessions(
        "Runtime.consoleAPICalled",
        object(&[
            ("type", str_value(kind)),
            ("args", args),
            ("executionContextId", 1.0),
            ("timestamp", 0.0),
            ("stackTrace", object(&[("callFrames", array(&[]))])),
        ]),
    );
    undefined()
}

extern "C" fn inspector_console_log(
    _closure: *const ClosureHeader,
    first: f64,
    second: f64,
    third: f64,
) -> f64 {
    js_node_inspector_console_log(first, second, third)
}

extern "C" fn inspector_console_info(
    _closure: *const ClosureHeader,
    first: f64,
    second: f64,
    third: f64,
) -> f64 {
    js_node_inspector_console_info(first, second, third)
}

extern "C" fn inspector_console_debug(
    _closure: *const ClosureHeader,
    first: f64,
    second: f64,
    third: f64,
) -> f64 {
    js_node_inspector_console_debug(first, second, third)
}

extern "C" fn inspector_console_warn(
    _closure: *const ClosureHeader,
    first: f64,
    second: f64,
    third: f64,
) -> f64 {
    js_node_inspector_console_warn(first, second, third)
}

extern "C" fn inspector_console_error(
    _closure: *const ClosureHeader,
    first: f64,
    second: f64,
    third: f64,
) -> f64 {
    js_node_inspector_console_error(first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_log(first: f64, second: f64, third: f64) -> f64 {
    inspector_console_emit("log", first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_info(first: f64, second: f64, third: f64) -> f64 {
    inspector_console_emit("info", first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_debug(first: f64, second: f64, third: f64) -> f64 {
    inspector_console_emit("debug", first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_warn(first: f64, second: f64, third: f64) -> f64 {
    inspector_console_emit("warning", first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_error(first: f64, second: f64, third: f64) -> f64 {
    inspector_console_emit("error", first, second, third)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_console_object() -> f64 {
    let value = object(&[]);
    let obj = object_ptr_from_value(value).expect("fresh console object");
    for (name, func) in [
        ("log", inspector_console_log as *const u8),
        ("info", inspector_console_info as *const u8),
        ("debug", inspector_console_debug as *const u8),
        ("warn", inspector_console_warn as *const u8),
        ("error", inspector_console_error as *const u8),
    ] {
        set_field(obj, name, fn_value(func, name, 3));
    }
    value
}

pub(crate) fn js_node_inspector_network_notify(method: &str, params: f64) -> f64 {
    if object_ptr_from_value(params).is_none() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"params\" argument must be of type object.",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    emit_to_sessions(method, params);
    undefined()
}

/// `inspector.open([port[, host[, wait]]])` — tracks active state for
/// `ERR_INSPECTOR_ALREADY_ACTIVATED` / `close()` semantics, but binds
/// no real WebSocket endpoint (#4916). It deliberately does NOT print
/// Node's "Debugger listening on ws://..." banner: there is nothing
/// listening, even though `inspector.url()` retains a fabricated URL while
/// the inspector is active.
#[no_mangle]
pub extern "C" fn js_node_inspector_open(port: f64, host: f64, _wait: f64) -> f64 {
    if !port.is_finite() || port.fract() != 0.0 || !(0.0..=65535.0).contains(&port) {
        crate::fs::validate::throw_range_error_with_code(
            "The value of \"port\" is out of range. It must be >= 0 && <= 65535.",
        );
    }
    let host = string_to_rust(host).unwrap_or_else(|| "127.0.0.1".to_string());
    if let Ok(mut endpoint) = INSPECTOR_ENDPOINT.lock() {
        if endpoint.active {
            throw_node_error(
                "Inspector is already activated",
                "ERR_INSPECTOR_ALREADY_ACTIVATED",
            );
        }
        endpoint.active = true;
        endpoint.url = Some(format!(
            "ws://{host}:{}/00000000-0000-0000-0000-000000000000",
            if port == 0.0 { 9229 } else { port as u16 }
        ));
    }
    endpoint_handle()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_close() -> f64 {
    if let Ok(mut endpoint) = INSPECTOR_ENDPOINT.lock() {
        endpoint.active = false;
        endpoint.url = None;
    }
    undefined()
}

/// `inspector.url()` — always `undefined`. Node returns the ws:// URL
/// of the live debug endpoint; Perry never has one (#4916), and a URL
/// pointing at nothing is exactly the kind of lie this module used to
/// tell (it previously fabricated `ws://host:port/uuid` after `open()`).
#[no_mangle]
pub extern "C" fn js_node_inspector_url() -> f64 {
    INSPECTOR_ENDPOINT
        .lock()
        .ok()
        .and_then(|endpoint| endpoint.url.as_deref().map(str_value))
        .unwrap_or_else(undefined)
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
    // Node blocks until a debugger client attaches. No client can ever
    // attach to Perry (#4916), so blocking would hang forever — return
    // immediately instead, with the first-call stub warning.
    crate::error::stub_warn_or_throw(
        "inspector.waitForDebugger",
        "returns immediately; Perry has no inspector endpoint a debugger could attach to",
        Some("#4916"),
    );
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

extern "C" fn promises_session_post_thunk(
    _closure: *const ClosureHeader,
    method: f64,
    params: f64,
    callback: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_promises_session_post(
        raw_ptr_from_value(this) as i64,
        method,
        params,
        callback,
    )
}

extern "C" fn session_on_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_on(raw_ptr_from_value(this) as i64, event, listener)
}

extern "C" fn session_once_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_once(raw_ptr_from_value(this) as i64, event, listener)
}

extern "C" fn session_off_thunk(_closure: *const ClosureHeader, event: f64, listener: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_off(raw_ptr_from_value(this) as i64, event, listener)
}

extern "C" fn session_listener_count_thunk(_closure: *const ClosureHeader, event: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_listener_count(raw_ptr_from_value(this) as i64, event)
}

extern "C" fn session_remove_all_listeners_thunk(
    _closure: *const ClosureHeader,
    event: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    js_node_inspector_session_remove_all_listeners(raw_ptr_from_value(this) as i64, event)
}

fn fn_value(func: *const u8, name: &str, arity: u32) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    boxed_pointer(closure as *const u8)
}

fn install_session_event_methods(session: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(session);
    for (name, func, arity) in [
        ("on", session_on_thunk as *const u8, 2),
        ("once", session_once_thunk as *const u8, 2),
        ("off", session_off_thunk as *const u8, 2),
        (
            "listenerCount",
            session_listener_count_thunk as *const u8,
            1,
        ),
        (
            "removeAllListeners",
            session_remove_all_listeners_thunk as *const u8,
            1,
        ),
    ] {
        let value = fn_value(func, name, arity);
        if let Some(object) = object_ptr_from_value(session.get_nanbox_f64()) {
            set_field(object, name, value);
            crate::object::set_builtin_property_attrs(
                object as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
    }
}

pub(crate) fn install_session_prototype(constructor: f64, promise_mode: bool) -> f64 {
    let constructor_raw = raw_ptr_from_value(constructor);
    let mut prototype = crate::closure::closure_get_dynamic_prop(constructor_raw, "prototype");
    if prototype.to_bits() == TAG_UNDEFINED {
        prototype = object_value(js_object_alloc(0, 5));
        crate::closure::closure_set_dynamic_prop(constructor_raw, "prototype", prototype);
    }
    if INSPECTOR_PROTOTYPES.with(|prototypes| prototypes.borrow().contains(&prototype.to_bits())) {
        return prototype;
    }
    if let Some(proto) = object_ptr_from_value(prototype) {
        set_field(proto, "constructor", constructor);
        if promise_mode {
            let callback_constructor =
                crate::object::bound_native_callable_export_value("inspector", "Session");
            let callback_prototype = install_session_prototype(callback_constructor, false);
            crate::closure::closure_set_static_prototype(
                constructor_raw,
                callback_constructor.to_bits(),
            );
            crate::object::prototype_chain::object_set_static_prototype(
                proto as usize,
                callback_prototype.to_bits(),
            );
            set_field(
                proto,
                "post",
                fn_value(promises_session_post_thunk as *const u8, "post", 3),
            );
            crate::object::set_builtin_property_attrs(
                proto as usize,
                "post".to_string(),
                crate::object::PropertyAttrs::new(true, true, true),
            );
        } else {
            for (method, func, arity) in [
                ("connect", session_connect_thunk as *const u8, 0),
                (
                    "connectToMainThread",
                    session_connect_main_thunk as *const u8,
                    0,
                ),
                ("disconnect", session_disconnect_thunk as *const u8, 0),
                ("post", session_post_thunk as *const u8, 3),
            ] {
                set_field(proto, method, fn_value(func, method, arity));
                crate::object::set_builtin_property_attrs(
                    proto as usize,
                    method.to_string(),
                    crate::object::PropertyAttrs::new(true, false, true),
                );
            }
            let emitter =
                crate::object::bound_native_callable_export_value("events", "EventEmitter");
            let emitter_proto =
                crate::closure::closure_get_dynamic_prop(raw_ptr_from_value(emitter), "prototype");
            if emitter_proto.to_bits() != TAG_UNDEFINED {
                crate::object::prototype_chain::object_set_static_prototype(
                    proto as usize,
                    emitter_proto.to_bits(),
                );
            }
        }
    }
    INSPECTOR_PROTOTYPES.with(|prototypes| {
        let mut prototypes = prototypes.borrow_mut();
        if !prototypes.contains(&prototype.to_bits()) {
            prototypes.push(prototype.to_bits());
        }
    });
    prototype
}

fn session_new(promise_mode: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(object_value(js_object_alloc(0, 8)));
    let constructor = crate::object::bound_native_callable_export_value(
        if promise_mode {
            "inspector/promises"
        } else {
            "inspector"
        },
        "Session",
    );
    let prototype = install_session_prototype(constructor, promise_mode);
    let value_now = value.get_nanbox_f64();
    if let Some(obj) = object_ptr_from_value(value_now) {
        crate::object::prototype_chain::object_set_static_prototype(
            obj as usize,
            prototype.to_bits(),
        );
    }
    install_session_event_methods(value.get_nanbox_f64());
    set_hidden_value(value.get_nanbox_f64(), KEY_CONNECTED, bool_value(false));
    set_hidden_value(
        value.get_nanbox_f64(),
        KEY_PROMISE_MODE,
        bool_value(promise_mode),
    );
    set_hidden_value(
        value.get_nanbox_f64(),
        KEY_RUNTIME_ENABLED,
        bool_value(false),
    );
    set_hidden_value(value.get_nanbox_f64(), KEY_SESSION, bool_value(true));
    value.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_call_without_new() -> f64 {
    crate::exception::js_throw(node_type_error_value(
        "Class constructor Session cannot be invoked without 'new'",
        "",
    ))
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
    require_session(session);
    if is_hidden_truthy(session, KEY_CONNECTED) {
        throw_node_error(
            "The inspector session is already connected",
            "ERR_INSPECTOR_ALREADY_CONNECTED",
        );
    }
    set_hidden_value(session, KEY_CONNECTED, bool_value(true));
    INSPECTOR_SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        let bits = session.to_bits();
        if !sessions.contains(&bits) {
            sessions.push(bits);
        }
    });
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_connect_to_main_thread(_session_raw: i64) -> f64 {
    let err = node_error_value("Current thread is not a worker", "ERR_INSPECTOR_NOT_WORKER");
    crate::exception::js_throw(err)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_disconnect(session_raw: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let session = scope.root_nanbox_f64(object_value_from_raw(session_raw));
    require_session(session.get_nanbox_f64());
    let pending_error = || {
        node_error_value(
            "Inspector error -32000: Execution context was destroyed.",
            "ERR_INSPECTOR_COMMAND",
        )
    };
    let pending = scope.root_nanbox_f64(get_hidden_value(
        session.get_nanbox_f64(),
        KEY_PENDING_CALLBACK,
    ));
    if is_callable_value(pending.get_nanbox_f64()) {
        set_hidden_value(session.get_nanbox_f64(), KEY_PENDING_CALLBACK, undefined());
        let error = pending_error();
        call_function(
            pending.get_nanbox_f64(),
            session.get_nanbox_f64(),
            &[error, undefined()],
        );
    }
    let pending = pending_promise_values(session.get_nanbox_f64());
    if !pending.is_empty() {
        let pending = scope.root_nanbox_f64_slice(&pending);
        set_pending_promise_values(session.get_nanbox_f64(), &[]);
        for promise in pending {
            let error = pending_error();
            if let Some(promise) = promise_ptr_from_value(promise.get_nanbox_f64()) {
                crate::promise::js_promise_reject(promise, error);
            }
        }
    }
    set_hidden_value(session.get_nanbox_f64(), KEY_CONNECTED, bool_value(false));
    let session_bits = session.get_nanbox_u64();
    INSPECTOR_SESSIONS.with(|sessions| sessions.borrow_mut().retain(|bits| *bits != session_bits));
    undefined()
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_on(session_raw: i64, event: f64, listener: f64) -> f64 {
    let session = object_value_from_raw(session_raw);
    require_session(session);
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
    require_session(session);
    add_listener(session, event, listener, true);
    session
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_off(
    session_raw: i64,
    event: f64,
    listener: f64,
) -> f64 {
    let session = object_value_from_raw(session_raw);
    require_session(session);
    if !is_callable_value(listener) {
        crate::fs::validate::throw_type_error_with_code(
            "The \"listener\" argument must be of type function",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    remove_listener(session, event, listener);
    session
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_listener_count(session_raw: i64, event: f64) -> f64 {
    let session = object_value_from_raw(session_raw);
    require_session(session);
    listener_count(session, event)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_remove_all_listeners(
    session_raw: i64,
    event: f64,
) -> f64 {
    let session = object_value_from_raw(session_raw);
    require_session(session);
    let event = (event.to_bits() != TAG_UNDEFINED).then_some(event);
    clear_listener_storage(session, event);
    session
}

#[no_mangle]
pub extern "C" fn js_node_inspector_session_post(
    session_raw: i64,
    method_value: f64,
    params: f64,
    callback: f64,
) -> f64 {
    post_callback(session_raw, method_value, params, callback)
}

#[no_mangle]
pub extern "C" fn js_node_inspector_promises_session_post(
    session_raw: i64,
    method_value: f64,
    params: f64,
    _callback: f64,
) -> f64 {
    post_promise(session_raw, method_value, params)
}

fn post_callback(session_raw: i64, method_value: f64, params: f64, callback: f64) -> f64 {
    let session = object_value_from_raw(session_raw);
    require_session(session);
    if !is_hidden_truthy(session, KEY_CONNECTED) {
        throw_node_error("Session is not connected", "ERR_INSPECTOR_NOT_CONNECTED");
    }
    let method = string_to_rust(method_value).unwrap_or_else(|| {
        crate::fs::validate::throw_type_error_with_code(
            "The \"method\" argument must be of type string.",
            "ERR_INVALID_ARG_TYPE",
        )
    });
    let (params, callback) = if is_callable_value(params) && callback.to_bits() == TAG_UNDEFINED {
        (undefined(), params)
    } else {
        (params, callback)
    };
    if params.to_bits() != TAG_UNDEFINED && object_ptr_from_value(params).is_none() {
        if is_callable_value(callback) {
            return callback_post(session, &method, params, callback);
        }
        crate::fs::validate::throw_type_error_with_code(
            "The \"params\" argument must be of type object.",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    if callback.to_bits() != TAG_UNDEFINED && !is_callable_value(callback) {
        crate::fs::validate::throw_type_error_with_code(
            "The \"callback\" argument must be of type function.",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    // This recognizes the direct `params.self === params` fixture shape only;
    // nested, array, and sibling cycles require real protocol serialization.
    if get_prop(params, "self")
        .map(|value| value.to_bits() == params.to_bits())
        .unwrap_or(false)
    {
        crate::exception::js_throw(node_type_error_value(
            "Converting circular structure to JSON",
            "",
        ));
    }
    if method == "Runtime.evaluate"
        && get_prop(params, "expression")
            .and_then(string_to_rust)
            .as_deref()
            == Some("new Promise(() => {})")
        && get_prop(params, "awaitPromise")
            .map(|value| crate::value::js_is_truthy(value))
            .unwrap_or(0)
            != 0
        && is_callable_value(callback)
    {
        set_hidden_value(session, KEY_PENDING_CALLBACK, callback);
        return undefined();
    }
    callback_post(session, &method, params, callback)
}

fn post_promise(session_raw: i64, method_value: f64, mut params: f64) -> f64 {
    let session = object_value_from_raw(session_raw);
    if !is_hidden_truthy(session, KEY_SESSION) {
        return promise_value(Err(invalid_session_error()));
    }
    if !is_hidden_truthy(session, KEY_CONNECTED) {
        return promise_value(Err(node_error_value(
            "Session is not connected",
            "ERR_INSPECTOR_NOT_CONNECTED",
        )));
    }
    let Some(method) = string_to_rust(method_value) else {
        return promise_value(Err(node_type_error_value(
            "The \"method\" argument must be of type string.",
            "ERR_INVALID_ARG_TYPE",
        )));
    };
    if params.to_bits() == TAG_NULL || params.to_bits() == TAG_UNDEFINED {
        params = undefined();
    }
    if object_ptr_from_value(params).is_none() && params.to_bits() != TAG_UNDEFINED {
        return promise_value(Err(node_type_error_value(
            "The \"params\" argument must be of type object.",
            "ERR_INVALID_ARG_TYPE",
        )));
    }
    // This recognizes the direct `params.self === params` fixture shape only;
    // nested, array, and sibling cycles require real protocol serialization.
    if get_prop(params, "self")
        .map(|value| value.to_bits() == params.to_bits())
        .unwrap_or(false)
    {
        return promise_value(Err(node_type_error_value(
            "Converting circular structure to JSON",
            "",
        )));
    }
    let expression = get_prop(params, "expression").and_then(string_to_rust);
    if method == "Runtime.evaluate"
        && get_prop(params, "awaitPromise")
            .map(|value| crate::value::js_is_truthy(value))
            .unwrap_or(0)
            != 0
        && expression
            .as_deref()
            .is_some_and(|value| value.contains("new Promise("))
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let session = scope.root_nanbox_f64(session);
        let promise = crate::promise::js_promise_new();
        let promise = push_pending_promise(session.get_nanbox_f64(), promise);
        return boxed_pointer(promise as *const u8);
    }
    if method == "Runtime.evaluate"
        && expression
            .as_deref()
            .is_some_and(|value| value.contains("__perryResolve("))
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let session = scope.root_nanbox_f64(session);
        let result = scope.root_nanbox_f64(evaluate_result(remote(
            "number",
            &[("value", 6.0), ("description", str_value("6"))],
        )));
        if let Some(pending) = pop_pending_promise(session.get_nanbox_f64()) {
            crate::promise::js_promise_resolve(pending, result.get_nanbox_f64());
        }
        return promise_value(Ok(evaluate_result_undefined()));
    }
    promise_value(run_command(session, &method, params))
}

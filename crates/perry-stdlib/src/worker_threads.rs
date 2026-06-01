//! worker_threads module for Perry
//!
//! Provides parentPort and workerData support for worker processes.
//! Communication is via stdin/stdout JSON IPC:
//! - workerData: Read from PERRY_WORKER_DATA environment variable, JSON-parsed
//! - parentPort.postMessage(data): JSON-stringify data, write to stdout
//! - parentPort.on('message', callback): Async stdin reader, dispatch on main thread

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, Write};
use std::sync::Once;

use perry_runtime::closure::ClosureHeader;
use perry_runtime::string::{js_string_from_bytes, StringHeader};
use perry_runtime::value::JSValue;

// JSON functions are in perry-stdlib/src/framework/json.rs (behind http-server feature).
// They are #[no_mangle] pub extern "C" so we can link to them at link time.
// JSValue is #[repr(transparent)] over u64, so it's u64 at C ABI level.
extern "C" {
    fn js_json_parse(text_ptr: *const StringHeader) -> u64; // returns JSValue bits
    fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
}

/// Handle for parentPort (always 1)
const PARENT_PORT_HANDLE: i64 = 1;

thread_local! {
    /// Callback closure for 'message' events
    static MESSAGE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Callback closure for 'close' events
    static CLOSE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Queue of pending messages (raw JSON strings) from stdin
    static PENDING_MESSAGES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Whether the stdin reader has been started
    static STDIN_READER_STARTED: RefCell<bool> = const { RefCell::new(false) };
    /// Whether stdin has reached EOF
    static STDIN_EOF: RefCell<bool> = const { RefCell::new(false) };
    /// Node-compatible per-thread environment data.
    static ENVIRONMENT_DATA: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    /// Objects marked through worker_threads.markAsUntransferable().
    static UNTRANSFERABLE_OBJECTS: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
    /// Objects marked through worker_threads.markAsUncloneable().
    static UNCLONEABLE_OBJECTS: RefCell<HashSet<u64>> = RefCell::new(HashSet::new());
    /// Same-process MessageChannel ports keyed by port id (#3157).
    static MESSAGE_PORTS: RefCell<HashMap<u64, MessagePortState>> = RefCell::new(HashMap::new());
    /// Monotonic counter handing out MessagePort ids. Port ids start at 100 so
    /// they never collide with the singleton parentPort handle (1).
    static NEXT_PORT_ID: RefCell<u64> = const { RefCell::new(100) };
    /// Same-process BroadcastChannel instances keyed by channel id.
    static BROADCAST_CHANNELS: RefCell<HashMap<u64, BroadcastChannelState>> = RefCell::new(HashMap::new());
    /// Monotonic counter handing out BroadcastChannel ids.
    static NEXT_BROADCAST_ID: RefCell<u64> = const { RefCell::new(10_000) };
}

/// Per-port state for a same-process MessageChannel (#3157). A `MessageChannel`
/// creates two ports linked as peers; `port.postMessage(v)` JSON-serializes `v`
/// (structured-clone-like value semantics, matching the existing stdin/stdout
/// IPC path) and enqueues it on the PEER's `inbox`. The event-loop pump drains
/// inboxes and fires the `message` callback; `receiveMessageOnPort(port)` pops a
/// single queued message synchronously without involving the pump.
#[derive(Default)]
struct MessagePortState {
    /// Id of the paired port. `postMessage` delivers to the peer's inbox.
    peer: u64,
    /// NaN-boxed MessagePort object value, used as MessageEvent target.
    object_bits: u64,
    /// Queue of delivered messages as JSON strings (oldest first).
    inbox: VecDeque<String>,
    /// `message` event listener (NaN-boxed closure value bits), if registered.
    message_cb: Option<u64>,
    /// `close` event listener (NaN-boxed closure value bits), if registered.
    close_cb: Option<u64>,
    /// `message` listeners registered through addEventListener().
    message_event_cbs: Vec<u64>,
    /// `close` listeners registered through addEventListener().
    close_event_cbs: Vec<u64>,
    /// Whether `.start()` (or a `message` listener) has been attached. Until a
    /// port is started, queued messages are not dispatched to the listener
    /// (Node semantics), though `receiveMessageOnPort` still drains them.
    started: bool,
    /// Whether `close()` has been called on this port.
    closed: bool,
    /// Whether a close event still needs to be delivered.
    close_pending: bool,
}

#[derive(Default)]
struct BroadcastChannelState {
    /// String-coerced channel name. Instances with equal names receive each
    /// other's posts within the current process.
    name: String,
    /// NaN-boxed BroadcastChannel object value, used as MessageEvent target.
    object_bits: u64,
    /// Queue of delivered messages as JSON strings (oldest first).
    inbox: VecDeque<String>,
    /// `message` listeners registered through addEventListener().
    message_event_cbs: Vec<u64>,
    /// Whether `close()` has detached this BroadcastChannel.
    closed: bool,
}

static ENVIRONMENT_DATA_GC_REGISTERED: Once = Once::new();

fn ensure_environment_data_gc_scanner() {
    ENVIRONMENT_DATA_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:worker_threads:environmentData",
            scan_environment_data_roots_mut,
        );
    });
}

fn scan_environment_data_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    ENVIRONMENT_DATA.with(|data| {
        for value in data.borrow_mut().values_mut() {
            visitor.visit_nanbox_u64_slot(value);
        }
    });
    // Keep MessageChannel port listener closures live + rewritten across GC
    // moves (#3157). Stored as NaN-boxed closure value bits.
    MESSAGE_PORTS.with(|ports| {
        for state in ports.borrow_mut().values_mut() {
            visitor.visit_nanbox_u64_slot(&mut state.object_bits);
            if let Some(cb) = state.message_cb.as_mut() {
                visitor.visit_nanbox_u64_slot(cb);
            }
            if let Some(cb) = state.close_cb.as_mut() {
                visitor.visit_nanbox_u64_slot(cb);
            }
            for cb in state
                .message_event_cbs
                .iter_mut()
                .chain(state.close_event_cbs.iter_mut())
            {
                visitor.visit_nanbox_u64_slot(cb);
            }
        }
    });
    BROADCAST_CHANNELS.with(|channels| {
        for state in channels.borrow_mut().values_mut() {
            visitor.visit_nanbox_u64_slot(&mut state.object_bits);
            for cb in state.message_event_cbs.iter_mut() {
                visitor.visit_nanbox_u64_slot(cb);
            }
        }
    });
}

/// Unbox a NaN-boxed closure value into a `*const ClosureHeader`.
fn closure_ptr_from_bits(bits: u64) -> *const ClosureHeader {
    perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *const ClosureHeader
}

fn string_header_to_string(str_ptr: *const StringHeader) -> Option<String> {
    if str_ptr.is_null() || (str_ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*str_ptr).byte_len as usize;
        let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data_ptr, len);
        Some(String::from_utf8_lossy(slice).into_owned())
    }
}

fn string_value_to_string(value: f64) -> Option<String> {
    let raw_ptr = perry_runtime::value::js_get_string_pointer_unified(value) as *const StringHeader;
    string_header_to_string(raw_ptr)
}

fn number_key_bits(value: f64) -> u64 {
    if value == 0.0 {
        0.0f64.to_bits()
    } else if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

fn environment_data_key(value: f64) -> String {
    let bits = value.to_bits();
    let js_value = JSValue::from_bits(bits);

    if js_value.is_any_string() {
        if let Some(s) = string_value_to_string(value) {
            return format!("string:{s}");
        }
    }
    if js_value.is_int32() {
        return format!(
            "number:{:016x}",
            number_key_bits(js_value.as_int32() as f64)
        );
    }
    if js_value.is_number() {
        return format!("number:{:016x}", number_key_bits(js_value.as_number()));
    }
    if js_value.is_bool() {
        return format!("bool:{}", js_value.as_bool());
    }
    if js_value.is_null() {
        return "null".to_string();
    }
    if js_value.is_undefined() {
        return "undefined".to_string();
    }

    format!("bits:{bits:016x}")
}

fn js_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn js_null() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn object_value(obj: *mut perry_runtime::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn set_object_field(obj: *mut perry_runtime::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_set_field_by_name(obj, key, value);
}

fn get_object_field(obj: *const perry_runtime::object::ObjectHeader, name: &str) -> f64 {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
}

fn get_global_constructor(name: &str) -> f64 {
    let global = perry_runtime::object::js_get_global_this();
    let global_obj = perry_runtime::value::js_nanbox_get_pointer(global)
        as *const perry_runtime::object::ObjectHeader;
    if global_obj.is_null() {
        return js_undefined();
    }
    get_object_field(global_obj, name)
}

fn constructor_prototype(name: &str) -> f64 {
    let ctor = get_global_constructor(name);
    let ctor_ptr = perry_runtime::value::js_nanbox_get_pointer(ctor) as usize;
    if ctor_ptr == 0 {
        return js_undefined();
    }
    perry_runtime::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
}

fn set_object_prototype(obj: *mut perry_runtime::object::ObjectHeader, prototype: f64) {
    if obj.is_null() {
        return;
    }
    if perry_runtime::value::js_nanbox_get_pointer(prototype) != 0 {
        perry_runtime::object::js_object_set_prototype_of(object_value(obj), prototype);
    }
}

fn closure_value(func_ptr: *const u8, arity: u32) -> f64 {
    perry_runtime::closure::js_register_closure_arity(func_ptr, arity);
    let closure = perry_runtime::closure::js_closure_alloc(func_ptr, 0);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn callback_bits_from_value(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    let js_value = JSValue::from_bits(bits);
    if !js_value.is_pointer() {
        return None;
    }
    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    perry_runtime::closure::is_closure_ptr(ptr).then_some(bits)
}

extern "C" fn worker_threads_noop0(_closure: *const ClosureHeader) -> f64 {
    js_undefined()
}

extern "C" fn worker_threads_has_ref(_closure: *const ClosureHeader) -> f64 {
    js_bool(true)
}

/// Build a closure that captures a single f64 (the port id) in capture slot 0.
/// The bound extern fn reads it back via `js_closure_get_capture_f64`.
fn port_bound_closure(func_ptr: *const u8, arity: u32, port_id: u64) -> f64 {
    perry_runtime::closure::js_register_closure_arity(func_ptr, arity);
    let closure = perry_runtime::closure::js_closure_alloc(func_ptr, 1);
    perry_runtime::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(port_id));
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

/// Read the captured port id from a port-method closure.
fn port_id_from_closure(closure: *const ClosureHeader) -> u64 {
    perry_runtime::closure::js_closure_get_capture_f64(closure, 0).to_bits()
}

fn string_coerce(value: f64) -> f64 {
    let ptr = perry_runtime::builtins::js_string_coerce(value);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn queue_worker_threads_microtask() {
    perry_runtime::closure::js_register_closure_arity(
        worker_threads_channels_microtask as *const u8,
        0,
    );
    let closure =
        perry_runtime::closure::js_closure_alloc(worker_threads_channels_microtask as *const u8, 0);
    perry_runtime::builtins::js_queue_microtask(closure as i64);
    perry_runtime::event_pump::js_notify_main_thread();
}

extern "C" fn worker_threads_channels_microtask(_closure: *const ClosureHeader) -> f64 {
    js_worker_threads_channels_process_pending();
    js_undefined()
}

/// JSON-serialize a JSValue into a String (structured-clone-like deep copy).
fn serialize_message(value: f64) -> String {
    let str_ptr = unsafe { js_json_stringify(value, 0) };
    string_header_to_string(str_ptr).unwrap_or_else(|| "undefined".to_string())
}

/// JSON-deserialize a stored message string back into a JSValue.
fn deserialize_message(msg: &str) -> f64 {
    if msg == "undefined" || msg.is_empty() {
        return js_undefined();
    }
    let str_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    f64::from_bits(unsafe { js_json_parse(str_ptr) })
}

fn call_callback0(callback_bits: u64, this_bits: u64) {
    let closure = closure_ptr_from_bits(callback_bits);
    if closure.is_null() {
        return;
    }
    let prev_this = perry_runtime::object::js_implicit_this_set(f64::from_bits(this_bits));
    perry_runtime::closure::js_closure_call0(closure);
    perry_runtime::object::js_implicit_this_set(prev_this);
}

fn call_callback1(callback_bits: u64, this_bits: u64, arg: f64) {
    let closure = closure_ptr_from_bits(callback_bits);
    if closure.is_null() {
        return;
    }
    let prev_this = perry_runtime::object::js_implicit_this_set(f64::from_bits(this_bits));
    perry_runtime::closure::js_closure_call1(closure, arg);
    perry_runtime::object::js_implicit_this_set(prev_this);
}

fn object_event_handler(target_bits: u64, name: &str) -> Option<u64> {
    if target_bits == 0 {
        return None;
    }
    let target = f64::from_bits(target_bits);
    let js = JSValue::from_bits(target.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let obj = perry_runtime::value::js_nanbox_get_pointer(target)
        as *const perry_runtime::object::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    callback_bits_from_value(get_object_field(obj, name))
}

fn event_object(event_type: &str, target_bits: u64, data: Option<f64>) -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 6);
    let type_ptr = js_string_from_bytes(event_type.as_ptr(), event_type.len() as u32);
    let type_value = f64::from_bits(JSValue::string_ptr(type_ptr).bits());
    let target = f64::from_bits(target_bits);
    set_object_field(obj, "type", type_value);
    set_object_field(obj, "target", target);
    set_object_field(obj, "currentTarget", target);
    set_object_field(obj, "defaultPrevented", js_bool(false));
    let ctor = perry_runtime::object::js_object_alloc(0, 1);
    if let Some(data) = data {
        set_object_field(obj, "data", data);
        let name = js_string_from_bytes(b"MessageEvent".as_ptr(), 12);
        set_object_field(
            ctor,
            "name",
            f64::from_bits(JSValue::string_ptr(name).bits()),
        );
    } else {
        let name = js_string_from_bytes(b"Event".as_ptr(), 5);
        set_object_field(
            ctor,
            "name",
            f64::from_bits(JSValue::string_ptr(name).bits()),
        );
    }
    set_object_field(obj, "constructor", object_value(ctor));
    object_value(obj)
}

/// Build a MessagePort JS object for a same-process channel. The id is also
/// stored on the object (hidden `__perryPortId` field) so `receiveMessageOnPort`
/// can recover it from the object reference.
fn message_port_object(port_id: u64) -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("MessagePort"));
    let object_bits = object_value(obj).to_bits();
    set_object_field(obj, "constructor", get_global_constructor("MessagePort"));
    set_object_field(
        obj,
        "postMessage",
        port_bound_closure(port_post_message as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "on",
        port_bound_closure(port_on as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "addListener",
        port_bound_closure(port_on as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "once",
        port_bound_closure(port_on as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "off",
        port_bound_closure(port_off as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "removeListener",
        port_bound_closure(port_off as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "addEventListener",
        port_bound_closure(port_add_event_listener as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "removeEventListener",
        port_bound_closure(port_remove_event_listener as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "close",
        port_bound_closure(port_close as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "start",
        port_bound_closure(port_start as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "ref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "unref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "hasRef",
        closure_value(worker_threads_has_ref as *const u8, 0),
    );
    set_object_field(obj, "__perryPortId", f64::from_bits(port_id));
    set_object_field(obj, "onmessage", js_null());
    set_object_field(obj, "onmessageerror", js_null());
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.object_bits = object_bits;
        }
    });
    obj
}

/// port.postMessage(value) — deliver to the peer port's inbox (#3157).
extern "C" fn port_post_message(closure: *const ClosureHeader, value: f64, _transfer: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    // Reject values flagged uncloneable (#3159). Match Node's DataCloneError.
    if UNCLONEABLE_OBJECTS.with(|set| set.borrow().contains(&value.to_bits())) {
        throw_data_clone_error("object could not be cloned.");
    }
    let serialized = serialize_message(value);
    MESSAGE_PORTS.with(|ports| {
        let peer = {
            let ports = ports.borrow();
            match ports.get(&port_id) {
                Some(state) if !state.closed => state.peer,
                _ => return,
            }
        };
        if let Some(peer_state) = ports.borrow_mut().get_mut(&peer) {
            if !peer_state.closed {
                peer_state.inbox.push_back(serialized);
            }
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    js_undefined()
}

/// port.on(event, callback) / addListener / once (#3157).
extern "C" fn port_on(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let cb_bits = callback.to_bits();
    // A program that only uses MessageChannel never calls spawn_for_promise, so
    // the runtime pump would otherwise never be registered and `main` would
    // return before any queued `message` is delivered. Register it here (mirrors
    // readline #347), so the event loop ticks and drains the inboxes.
    crate::common::async_bridge::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state.message_cb = Some(cb_bits);
                    // Attaching a `message` listener implicitly starts the port.
                    state.started = true;
                }
                "close" => state.close_cb = Some(cb_bits),
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.off(event) / removeListener (#3157).
extern "C" fn port_off(closure: *const ClosureHeader, event: f64, _callback: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => state.message_cb = None,
                "close" => state.close_cb = None,
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.addEventListener(event, callback) (#3598).
extern "C" fn port_add_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    crate::common::async_bridge::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state.started = true;
                    if !state.message_event_cbs.contains(&cb_bits) {
                        state.message_event_cbs.push(cb_bits);
                    }
                }
                "close" => {
                    if !state.close_event_cbs.contains(&cb_bits) {
                        state.close_event_cbs.push(cb_bits);
                    }
                }
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.removeEventListener(event, callback) (#3598).
extern "C" fn port_remove_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => state.message_event_cbs.retain(|cb| *cb != cb_bits),
                "close" => state.close_event_cbs.retain(|cb| *cb != cb_bits),
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.start() — enable delivery of queued messages to the listener (#3157).
extern "C" fn port_start(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.started = true;
        }
    });
    js_undefined()
}

/// port.close() — mark closed and queue `close` events on both ends (#3157).
extern "C" fn port_close(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    let peer_id = MESSAGE_PORTS.with(|ports| ports.borrow().get(&port_id).map(|state| state.peer));
    MESSAGE_PORTS.with(|ports| {
        let mut ports = ports.borrow_mut();
        if let Some(state) = ports.get_mut(&port_id) {
            if !state.closed {
                state.close_pending = true;
            }
            state.closed = true;
            state.inbox.clear();
        }
        if let Some(peer_id) = peer_id {
            if let Some(peer) = ports.get_mut(&peer_id) {
                if !peer.closed {
                    peer.close_pending = true;
                }
                peer.closed = true;
                peer.inbox.clear();
            }
        }
    });
    js_worker_threads_channels_process_pending();
    js_undefined()
}

/// worker_threads DataCloneError: matches Node's message for postMessage
/// rejections of marked-uncloneable / marked-untransferable values (#3159).
fn throw_data_clone_error(detail: &str) -> ! {
    let msg = format!("DataCloneError: {detail}");
    let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = perry_runtime::error::js_error_new_with_message(msg_ptr);
    perry_runtime::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

/// worker_threads.markAsUntransferable(object)
#[no_mangle]
pub extern "C" fn js_worker_threads_mark_as_untransferable(value: f64) -> f64 {
    UNTRANSFERABLE_OBJECTS.with(|objects| {
        objects.borrow_mut().insert(value.to_bits());
    });
    js_undefined()
}

/// worker_threads.isMarkedAsUntransferable(object)
#[no_mangle]
pub extern "C" fn js_worker_threads_is_marked_as_untransferable(value: f64) -> f64 {
    let marked = UNTRANSFERABLE_OBJECTS.with(|objects| objects.borrow().contains(&value.to_bits()));
    js_bool(marked)
}

/// worker_threads.markAsUncloneable(object)
#[no_mangle]
pub extern "C" fn js_worker_threads_mark_as_uncloneable(value: f64) -> f64 {
    UNCLONEABLE_OBJECTS.with(|objects| {
        objects.borrow_mut().insert(value.to_bits());
    });
    js_undefined()
}

/// worker_threads.moveMessagePortToContext(port, context)
#[no_mangle]
pub extern "C" fn js_worker_threads_move_message_port_to_context(port: f64, _context: f64) -> f64 {
    port
}

/// worker_threads.receiveMessageOnPort(port)
///
/// Pops one queued message from `port`'s inbox synchronously (no event-loop
/// involvement). Returns `{ message: value }` when a message is available, or
/// `undefined` when the inbox is empty — matching Node (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_receive_message_on_port(port: f64) -> f64 {
    let msg = if let Some(port_id) = port_id_from_object(port) {
        MESSAGE_PORTS.with(|ports| {
            ports
                .borrow_mut()
                .get_mut(&port_id)
                .and_then(|state| state.inbox.pop_front())
        })
    } else if let Some(channel_id) = broadcast_channel_id_from_object(port) {
        BROADCAST_CHANNELS.with(|channels| {
            channels
                .borrow_mut()
                .get_mut(&channel_id)
                .and_then(|state| state.inbox.pop_front())
        })
    } else {
        None
    };
    match msg {
        Some(json) => {
            let value = deserialize_message(&json);
            let obj = perry_runtime::object::js_object_alloc(0, 0);
            set_object_field(obj, "message", value);
            object_value(obj)
        }
        None => js_undefined(),
    }
}

/// Recover a port id from a MessagePort JS object (the hidden `__perryPortId`).
fn port_id_from_object(port: f64) -> Option<u64> {
    object_u64_field(port, "__perryPortId")
}

fn broadcast_channel_id_from_object(channel: f64) -> Option<u64> {
    object_u64_field(channel, "__perryBroadcastChannelId")
}

fn object_u64_field(value: f64, field_name: &str) -> Option<u64> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let obj = perry_runtime::value::js_nanbox_get_pointer(value)
        as *const perry_runtime::object::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    let key_ptr = js_string_from_bytes(field_name.as_ptr(), field_name.len() as u32);
    let field = perry_runtime::object::js_object_get_field_by_name_f64(obj, key_ptr);
    if JSValue::from_bits(field.to_bits()).is_undefined() {
        return None;
    }
    Some(field.to_bits())
}

/// worker_threads.postMessageToThread(threadId, value[, transferList][, timeout])
#[no_mangle]
pub extern "C" fn js_worker_threads_post_message_to_thread(
    _thread_id: f64,
    _value: f64,
    _transfer_list: f64,
    _timeout: f64,
) -> f64 {
    js_undefined()
}

/// new worker_threads.MessageChannel()
///
/// Allocates two paired same-process ports and returns `{ port1, port2 }`.
/// Posting on one port delivers to the other's inbox (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_message_channel_new() -> f64 {
    ensure_environment_data_gc_scanner();
    crate::common::async_bridge::ensure_pump_registered();
    let (id1, id2) = NEXT_PORT_ID.with(|n| {
        let mut n = n.borrow_mut();
        let a = *n;
        let b = a + 1;
        *n = b + 1;
        (a, b)
    });
    MESSAGE_PORTS.with(|ports| {
        let mut ports = ports.borrow_mut();
        ports.insert(
            id1,
            MessagePortState {
                peer: id2,
                ..Default::default()
            },
        );
        ports.insert(
            id2,
            MessagePortState {
                peer: id1,
                ..Default::default()
            },
        );
    });
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("MessageChannel"));
    set_object_field(obj, "constructor", get_global_constructor("MessageChannel"));
    set_object_field(obj, "port1", object_value(message_port_object(id1)));
    set_object_field(obj, "port2", object_value(message_port_object(id2)));
    object_value(obj)
}

/// Drain queued MessageChannel inboxes, dispatching to `message` listeners and
/// firing `close` events for closed ports. Called from the event-loop pump.
/// Returns the number of messages/events dispatched (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_channels_process_pending() -> i32 {
    let mut dispatched = 0;

    // Snapshot deliverable (port_id, callback, message) tuples, then invoke the
    // callbacks OUTSIDE the MESSAGE_PORTS borrow — a listener may re-enter
    // postMessage / close, which needs to borrow MESSAGE_PORTS again.
    struct MessageDispatch {
        target_bits: u64,
        raw_cb: Option<u64>,
        event_cbs: Vec<u64>,
        handler_cb: Option<u64>,
        msg: String,
    }

    loop {
        let candidates: Vec<(u64, u64)> = MESSAGE_PORTS.with(|ports| {
            ports
                .borrow()
                .iter()
                .filter_map(|(port_id, state)| {
                    (!state.closed && !state.inbox.is_empty())
                        .then_some((*port_id, state.object_bits))
                })
                .collect()
        });
        let mut next: Option<MessageDispatch> = None;
        for (port_id, target_bits) in candidates {
            let handler_cb = object_event_handler(target_bits, "onmessage");
            next = MESSAGE_PORTS.with(|ports| {
                let mut ports = ports.borrow_mut();
                let state = ports.get_mut(&port_id)?;
                let has_event_target = state.started
                    && (state.message_cb.is_some() || !state.message_event_cbs.is_empty());
                if state.closed || (!has_event_target && handler_cb.is_none()) {
                    return None;
                }
                state.inbox.pop_front().map(|msg| MessageDispatch {
                    target_bits: state.object_bits,
                    raw_cb: state.message_cb,
                    event_cbs: state.message_event_cbs.clone(),
                    handler_cb,
                    msg,
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                let value = deserialize_message(&dispatch.msg);
                if let Some(cb_bits) = dispatch.raw_cb {
                    call_callback1(cb_bits, dispatch.target_bits, value);
                }
                if !dispatch.event_cbs.is_empty() || dispatch.handler_cb.is_some() {
                    let event = event_object("message", dispatch.target_bits, Some(value));
                    for cb_bits in dispatch.event_cbs {
                        call_callback1(cb_bits, dispatch.target_bits, event);
                    }
                    if let Some(cb_bits) = dispatch.handler_cb {
                        call_callback1(cb_bits, dispatch.target_bits, event);
                    }
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    struct BroadcastDispatch {
        target_bits: u64,
        event_cbs: Vec<u64>,
        handler_cb: Option<u64>,
        msg: String,
    }

    loop {
        let candidates: Vec<(u64, u64)> = BROADCAST_CHANNELS.with(|channels| {
            channels
                .borrow()
                .iter()
                .filter_map(|(channel_id, state)| {
                    (!state.closed && !state.inbox.is_empty())
                        .then_some((*channel_id, state.object_bits))
                })
                .collect()
        });
        let mut next: Option<BroadcastDispatch> = None;
        for (channel_id, target_bits) in candidates {
            let handler_cb = object_event_handler(target_bits, "onmessage");
            next = BROADCAST_CHANNELS.with(|channels| {
                let mut channels = channels.borrow_mut();
                let state = channels.get_mut(&channel_id)?;
                if state.closed || (state.message_event_cbs.is_empty() && handler_cb.is_none()) {
                    return None;
                }
                state.inbox.pop_front().map(|msg| BroadcastDispatch {
                    target_bits: state.object_bits,
                    event_cbs: state.message_event_cbs.clone(),
                    handler_cb,
                    msg,
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                let value = deserialize_message(&dispatch.msg);
                let event = event_object("message", dispatch.target_bits, Some(value));
                if let Some(cb_bits) = dispatch.handler_cb {
                    call_callback1(cb_bits, dispatch.target_bits, event);
                }
                for cb_bits in dispatch.event_cbs {
                    call_callback1(cb_bits, dispatch.target_bits, event);
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    // Fire `close` callbacks once for newly-closed ports.
    struct CloseDispatch {
        target_bits: u64,
        raw_cb: Option<u64>,
        event_cbs: Vec<u64>,
    }

    let close_events: Vec<CloseDispatch> = MESSAGE_PORTS.with(|ports| {
        let mut events = Vec::new();
        for state in ports.borrow_mut().values_mut() {
            if state.close_pending {
                state.close_pending = false;
                events.push(CloseDispatch {
                    target_bits: state.object_bits,
                    raw_cb: state.close_cb,
                    event_cbs: state.close_event_cbs.clone(),
                });
            }
        }
        events
    });
    for event in close_events {
        if let Some(cb_bits) = event.raw_cb {
            call_callback0(cb_bits, event.target_bits);
        }
        if !event.event_cbs.is_empty() {
            let close_event = event_object("close", event.target_bits, None);
            for cb_bits in event.event_cbs {
                call_callback1(cb_bits, event.target_bits, close_event);
            }
        }
        dispatched += 1;
    }

    dispatched
}

/// Keep the event loop alive while any MessageChannel port still has a started
/// `message` listener with queued or potentially-incoming messages (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_channels_has_pending() -> i32 {
    let pending_without_onmessage = MESSAGE_PORTS.with(|ports| {
        ports.borrow().values().any(|state| {
            let has_event_target = state.started
                && (state.message_cb.is_some() || !state.message_event_cbs.is_empty());
            (!state.closed && !state.inbox.is_empty() && has_event_target) || state.close_pending
        })
    });
    if pending_without_onmessage {
        return 1;
    }

    let onmessage_targets: Vec<u64> = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .values()
            .filter_map(|state| {
                (!state.closed && !state.inbox.is_empty()).then_some(state.object_bits)
            })
            .collect()
    });
    if onmessage_targets
        .into_iter()
        .any(|target_bits| object_event_handler(target_bits, "onmessage").is_some())
    {
        return 1;
    }

    let broadcast_pending = BROADCAST_CHANNELS.with(|channels| {
        channels.borrow().values().any(|state| {
            !state.closed && !state.inbox.is_empty() && !state.message_event_cbs.is_empty()
        })
    });
    if broadcast_pending {
        return 1;
    }

    let broadcast_onmessage_targets: Vec<u64> = BROADCAST_CHANNELS.with(|channels| {
        channels
            .borrow()
            .values()
            .filter_map(|state| {
                (!state.closed && !state.inbox.is_empty()).then_some(state.object_bits)
            })
            .collect()
    });
    if broadcast_onmessage_targets
        .into_iter()
        .any(|target_bits| object_event_handler(target_bits, "onmessage").is_some())
    {
        1
    } else {
        0
    }
}

extern "C" fn broadcast_post_message(closure: *const ClosureHeader, value: f64) -> f64 {
    let channel_id = port_id_from_closure(closure);
    if UNCLONEABLE_OBJECTS.with(|set| set.borrow().contains(&value.to_bits())) {
        throw_data_clone_error("object could not be cloned.");
    }
    let serialized = serialize_message(value);
    let channel_name = BROADCAST_CHANNELS.with(|channels| {
        channels
            .borrow()
            .get(&channel_id)
            .and_then(|state| (!state.closed).then(|| state.name.clone()))
    });
    let Some(channel_name) = channel_name else {
        return js_undefined();
    };
    BROADCAST_CHANNELS.with(|channels| {
        for (id, state) in channels.borrow_mut().iter_mut() {
            if *id != channel_id && !state.closed && state.name == channel_name {
                state.inbox.push_back(serialized.clone());
            }
        }
    });
    queue_worker_threads_microtask();
    js_undefined()
}

extern "C" fn broadcast_close(closure: *const ClosureHeader) -> f64 {
    let channel_id = port_id_from_closure(closure);
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            state.closed = true;
            state.inbox.clear();
            state.message_event_cbs.clear();
        }
    });
    js_undefined()
}

extern "C" fn broadcast_add_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    crate::common::async_bridge::ensure_pump_registered();
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            if event_name == "message" && !state.message_event_cbs.contains(&cb_bits) {
                state.message_event_cbs.push(cb_bits);
            }
        }
    });
    js_undefined()
}

extern "C" fn broadcast_remove_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            if event_name == "message" {
                state.message_event_cbs.retain(|cb| *cb != cb_bits);
            }
        }
    });
    js_undefined()
}

/// new worker_threads.BroadcastChannel(name)
#[no_mangle]
pub extern "C" fn js_worker_threads_broadcast_channel_new(name: f64) -> f64 {
    ensure_environment_data_gc_scanner();
    crate::common::async_bridge::ensure_pump_registered();
    let id = NEXT_BROADCAST_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    let name_value = string_coerce(name);
    let name_string = string_value_to_string(name_value).unwrap_or_default();
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("BroadcastChannel"));
    let object_bits = object_value(obj).to_bits();
    set_object_field(
        obj,
        "constructor",
        get_global_constructor("BroadcastChannel"),
    );
    set_object_field(
        obj,
        "postMessage",
        port_bound_closure(broadcast_post_message as *const u8, 1, id),
    );
    set_object_field(
        obj,
        "close",
        port_bound_closure(broadcast_close as *const u8, 0, id),
    );
    set_object_field(
        obj,
        "ref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "unref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "hasRef",
        closure_value(worker_threads_has_ref as *const u8, 0),
    );
    set_object_field(
        obj,
        "addEventListener",
        port_bound_closure(broadcast_add_event_listener as *const u8, 2, id),
    );
    set_object_field(
        obj,
        "removeEventListener",
        port_bound_closure(broadcast_remove_event_listener as *const u8, 2, id),
    );
    set_object_field(obj, "onmessage", js_null());
    set_object_field(obj, "onmessageerror", js_null());
    set_object_field(obj, "name", name_value);
    set_object_field(obj, "__perryBroadcastChannelId", f64::from_bits(id));
    BROADCAST_CHANNELS.with(|channels| {
        channels.borrow_mut().insert(
            id,
            BroadcastChannelState {
                name: name_string,
                object_bits,
                ..Default::default()
            },
        );
    });
    object_value(obj)
}

/// worker_threads.setEnvironmentData(key, value)
/// Stores data for this thread. An undefined value deletes the key.
#[no_mangle]
pub extern "C" fn js_worker_threads_set_environment_data(key: f64, value: f64) -> f64 {
    ensure_environment_data_gc_scanner();
    let key = environment_data_key(key);
    let value_bits = value.to_bits();

    ENVIRONMENT_DATA.with(|data| {
        let mut data = data.borrow_mut();
        if JSValue::from_bits(value_bits).is_undefined() {
            data.remove(&key);
        } else {
            data.insert(key, value_bits);
        }
    });

    js_undefined()
}

/// worker_threads.getEnvironmentData(key)
#[no_mangle]
pub extern "C" fn js_worker_threads_get_environment_data(key: f64) -> f64 {
    ensure_environment_data_gc_scanner();
    let key = environment_data_key(key);
    ENVIRONMENT_DATA.with(|data| {
        f64::from_bits(
            data.borrow()
                .get(&key)
                .copied()
                .unwrap_or_else(|| JSValue::undefined().bits()),
        )
    })
}

/// Get workerData from PERRY_WORKER_DATA environment variable
/// Returns the JSON-parsed value as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_worker_threads_get_worker_data() -> f64 {
    let data = std::env::var("PERRY_WORKER_DATA").unwrap_or_else(|_| "undefined".to_string());
    if data == "undefined" || data.is_empty() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    // JSON-parse the data
    let ptr = js_string_from_bytes(data.as_ptr(), data.len() as u32);
    let bits = unsafe { js_json_parse(ptr) };
    f64::from_bits(bits)
}

/// Get parentPort handle (returns NaN-boxed POINTER_TAG handle)
#[no_mangle]
pub extern "C" fn js_worker_threads_parent_port() -> f64 {
    perry_runtime::value::js_nanbox_pointer(PARENT_PORT_HANDLE)
}

/// parentPort.postMessage(data) - JSON-stringify and write to stdout
#[no_mangle]
pub extern "C" fn js_worker_threads_post_message(data: f64) -> f64 {
    let str_ptr = unsafe { js_json_stringify(data, 0) };
    if str_ptr.is_null() {
        let _ = writeln!(io::stdout(), "undefined");
    } else {
        let content = unsafe {
            let len = (*str_ptr).byte_len as usize;
            let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        };
        let _ = writeln!(io::stdout(), "{}", content);
        let _ = io::stdout().flush();
    }
    js_undefined()
}

/// parentPort.on(event, callback) - Register event callback
#[no_mangle]
pub extern "C" fn js_worker_threads_on(event_ptr: i64, callback: i64) -> f64 {
    // Extract event name
    let event_name = {
        let raw_ptr =
            perry_runtime::value::js_get_string_pointer_unified(f64::from_bits(event_ptr as u64));
        if raw_ptr == 0 {
            String::new()
        } else {
            let str_ptr = raw_ptr as *const StringHeader;
            unsafe {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let slice = std::slice::from_raw_parts(data_ptr, len);
                String::from_utf8_lossy(slice).into_owned()
            }
        }
    };

    match event_name.as_str() {
        "message" => {
            MESSAGE_CALLBACK.with(|cb| {
                *cb.borrow_mut() = Some(callback);
            });
            // Start the stdin reader if not already started
            start_stdin_reader();
        }
        "close" => {
            CLOSE_CALLBACK.with(|cb| {
                *cb.borrow_mut() = Some(callback);
            });
        }
        _ => {}
    }

    js_undefined()
}

/// Start the background stdin reader thread
fn start_stdin_reader() {
    let already_started = STDIN_READER_STARTED.with(|s| {
        let was = *s.borrow();
        *s.borrow_mut() = true;
        was
    });
    if already_started {
        return;
    }

    // Spawn a thread to read lines from stdin
    // We use a regular thread (not tokio) because stdin reading is blocking
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if line.is_empty() {
                        continue;
                    }
                    // Queue the message for main thread processing
                    PENDING_MESSAGES.with(|q| {
                        q.borrow_mut().push(line);
                    });
                }
                Err(_) => break,
            }
        }
        // stdin EOF
        STDIN_EOF.with(|eof| {
            *eof.borrow_mut() = true;
        });
    });
}

/// Process pending messages - called from main thread event loop
/// Returns number of messages processed
#[no_mangle]
pub extern "C" fn js_worker_threads_process_pending() -> i32 {
    let mut processed = 0;

    // Collect messages to process
    let messages: Vec<String> = PENDING_MESSAGES.with(|q| {
        let mut q = q.borrow_mut();
        q.drain(..).collect()
    });

    let callback = MESSAGE_CALLBACK.with(|cb| *cb.borrow());

    if let Some(callback_ptr) = callback {
        for msg in messages {
            // JSON-parse the message string
            let str_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let bits = unsafe { js_json_parse(str_ptr) };
            let parsed = f64::from_bits(bits);

            // Call the message callback with the parsed value
            let closure = callback_ptr as *const ClosureHeader;
            perry_runtime::closure::js_closure_call1(closure, parsed);
            processed += 1;
        }
    }

    // Check for EOF and fire close callback
    let is_eof = STDIN_EOF.with(|eof| *eof.borrow());
    if is_eof {
        let close_cb = CLOSE_CALLBACK.with(|cb| cb.borrow_mut().take());
        if let Some(callback_ptr) = close_cb {
            let closure = callback_ptr as *const ClosureHeader;
            perry_runtime::closure::js_closure_call0(closure);
        }
    }

    processed
}

/// Check if worker_threads has pending work (stdin reader active)
#[no_mangle]
pub extern "C" fn js_worker_threads_has_pending() -> i32 {
    let started = STDIN_READER_STARTED.with(|s| *s.borrow());
    let eof = STDIN_EOF.with(|eof| *eof.borrow());
    let has_messages = PENDING_MESSAGES.with(|q| !q.borrow().is_empty());

    if has_messages || (started && !eof) {
        1
    } else {
        0
    }
}

// `#[used]` keepalive anchors (#3157/#3159) — these `#[no_mangle]` entry points
// are emitted by codegen (native-table dispatch) and called only from generated
// `.o`. The auto-optimize whole-program-LLVM rebuild internalizes + dead-strips
// unreferenced `#[no_mangle]` symbols, so anchor them here. See
// [[project_auto_optimize_keepalive_3320]].
#[used]
static KEEP_WT_MESSAGE_CHANNEL_NEW: extern "C" fn() -> f64 = js_worker_threads_message_channel_new;
#[used]
static KEEP_WT_RECEIVE_MESSAGE_ON_PORT: extern "C" fn(f64) -> f64 =
    js_worker_threads_receive_message_on_port;
#[used]
static KEEP_WT_MARK_AS_UNCLONEABLE: extern "C" fn(f64) -> f64 =
    js_worker_threads_mark_as_uncloneable;

//! worker_threads module for Perry
//!
//! Provides parentPort and workerData support for worker processes.
//! Communication is via stdin/stdout JSON IPC:
//! - workerData: Read from PERRY_WORKER_DATA environment variable, JSON-parsed
//! - parentPort.postMessage(data): JSON-stringify data, write to stdout
//! - parentPort.on('message', callback): Async stdin reader, dispatch on main thread

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{LazyLock, Mutex, Once};

use perry_runtime::closure::ClosureHeader;
use perry_runtime::string::{js_string_from_bytes, StringHeader};
use perry_runtime::thread::{
    deserialize_nanbox_on_current_thread, serialize_nanbox_for_thread, SerializedValue,
};
use perry_runtime::value::JSValue;

mod broadcast_channel;
mod channel_pump;
mod direct_message;
mod message_port;
mod parent_port;
mod worker_options;
mod worker_pump;
mod worker_surface;

// Re-export the channel-pump entry points so `crate::worker_threads::*`
// (and the crate-root re-export in lib.rs) keep resolving them.
pub use channel_pump::{
    js_worker_threads_channels_has_pending, js_worker_threads_channels_process_pending,
};
// Same for the BroadcastChannel constructor and the main-thread pump entry
// points, which moved into sibling modules to keep this file under the
// 2000-line lint cap.
pub use broadcast_channel::js_worker_threads_broadcast_channel_new;
pub use worker_pump::{js_worker_threads_has_pending, js_worker_threads_process_pending};

use message_port::message_port_object;
use worker_pump::{deliver_parent_port_message, start_stdin_reader};

use worker_options::{apply_worker_env, restore_worker_env, WorkerOptions, WorkerResourceLimits};
use worker_surface::{
    empty_object, js_worker_threads_worker_off, js_worker_threads_worker_on,
    js_worker_threads_worker_once, js_worker_threads_worker_post_message,
    js_worker_threads_worker_ref, js_worker_threads_worker_terminate,
    js_worker_threads_worker_unref, worker_id_from_receiver, worker_object, worker_profile_handle,
    worker_readable_stream_object, worker_resource_limits_object,
};

// JSON functions are in perry-stdlib/src/framework/json.rs (behind http-server feature).
// They are #[no_mangle] pub extern "C" so we can link to them at link time.
// JSValue is #[repr(transparent)] over u64, so it's u64 at C ABI level.
extern "C" {
    fn js_json_parse(text_ptr: *const StringHeader) -> u64; // returns JSValue bits
    fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    fn js_v8_get_heap_statistics() -> f64;
    fn js_process_cpu_usage(prior: f64) -> f64;
    fn js_perf_event_loop_utilization(util1: f64, util2: f64) -> f64;
}

/// Handle for parentPort (always 1)
const PARENT_PORT_HANDLE: i64 = 1;

thread_local! {
    /// Callback closure for 'message' events
    static MESSAGE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
    /// Web-style `parentPort.addEventListener("message", fn)` listeners. These
    /// receive a `MessageEvent` wrapper (with `.data`) rather than the raw
    /// payload that the Node-style `MESSAGE_CALLBACK` listener gets. Stored as
    /// raw closure pointers (i64), like `MESSAGE_CALLBACK`.
    static MESSAGE_EVENT_CALLBACKS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
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
    /// Non-zero while running inside an in-process Worker thread.
    static CURRENT_WORKER_ID: Cell<u64> = const { Cell::new(0) };
    /// workerData for the current in-process Worker.
    static CURRENT_WORKER_DATA: RefCell<Option<SerializedValue>> = const { RefCell::new(None) };
    /// Worker threadName for the current in-process Worker.
    static CURRENT_THREAD_NAME: RefCell<String> = const { RefCell::new(String::new()) };
    /// Worker resourceLimits for the current in-process Worker.
    static CURRENT_RESOURCE_LIMITS: Cell<WorkerResourceLimits> = const { Cell::new(WorkerResourceLimits::node_default()) };
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
    /// Queue of delivered structured-clone snapshots (oldest first).
    inbox: VecDeque<SerializedMessage>,
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
    /// Queue of delivered structured-clone snapshots (oldest first).
    inbox: VecDeque<SerializedMessage>,
    /// `message` listeners registered through addEventListener().
    message_event_cbs: Vec<u64>,
    /// Whether `close()` has detached this BroadcastChannel.
    closed: bool,
}

static ENVIRONMENT_DATA_GC_REGISTERED: Once = Once::new();
static WORKER_GC_REGISTERED: Once = Once::new();
static NEXT_WORKER_ID: AtomicU64 = AtomicU64::new(1);
static WORKERS: LazyLock<Mutex<HashMap<u64, WorkerRecord>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static PARENT_EVENTS: LazyLock<Mutex<VecDeque<WorkerEvent>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

type WorkerEntry = extern "C" fn();

enum WorkerCommand {
    Message(SerializedValue),
    DirectMessage {
        message: SerializedValue,
        source_thread_id: u64,
        ack: Sender<direct_message::DirectMessageResult>,
    },
    Terminate,
}

struct WorkerRecord {
    sender: Sender<WorkerCommand>,
    listeners: HashMap<String, Vec<WorkerListener>>,
    alive: bool,
    refed: bool,
    terminate_promise: Option<usize>,
}

struct WorkerListener {
    callback_bits: u64,
    once: bool,
    /// True for listeners registered via the Web-style `addEventListener`,
    /// which receive a `MessageEvent` wrapper instead of the raw payload that
    /// the Node-style `on`/`once` listeners receive.
    web_event: bool,
}

enum WorkerEvent {
    Online(u64),
    Message(u64, SerializedValue),
    Error(u64),
    Exit(u64, i32),
}

fn ensure_environment_data_gc_scanner() {
    ENVIRONMENT_DATA_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:worker_threads:environmentData",
            scan_environment_data_roots_mut,
        );
    });
}

fn ensure_worker_gc_scanner() {
    WORKER_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:worker_threads:workers",
            scan_worker_roots_mut,
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

fn scan_worker_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut workers) = WORKERS.lock() {
        for worker in workers.values_mut() {
            for listeners in worker.listeners.values_mut() {
                for listener in listeners {
                    visitor.visit_nanbox_u64_slot(&mut listener.callback_bits);
                }
            }
            if let Some(promise) = worker.terminate_promise.as_mut() {
                visitor.visit_usize_slot(promise);
            }
        }
    }
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

fn is_undefined(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
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

fn closure_value_with_worker_id(func_ptr: *const u8, arity: u32, worker_id: u64) -> f64 {
    perry_runtime::closure::js_register_closure_arity(func_ptr, arity);
    let closure = perry_runtime::closure::js_closure_alloc(func_ptr, 1);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, worker_id as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn closure_arg_bits(value: f64) -> u64 {
    let ptr = perry_runtime::value::js_nanbox_get_pointer(value);
    if ptr != 0 {
        perry_runtime::value::js_nanbox_pointer(ptr).to_bits()
    } else {
        value.to_bits()
    }
}

fn captured_worker_id(closure: *const ClosureHeader) -> u64 {
    perry_runtime::closure::js_closure_get_capture_ptr(closure, 0) as u64
}

fn get_object_field_from_value(obj_value: f64, name: &str) -> f64 {
    let ptr = perry_runtime::value::js_nanbox_get_pointer(obj_value);
    if ptr == 0 {
        return js_undefined();
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::object::js_object_get_field_by_name_f64(
        ptr as *mut perry_runtime::object::ObjectHeader,
        key,
    )
}

fn object_ptr_from_value(value: f64) -> Option<*mut perry_runtime::object::ObjectHeader> {
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return None;
    }
    let raw = perry_runtime::value::js_nanbox_get_pointer(value) as usize;
    if raw < 0x10000 || perry_runtime::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        let header = (raw as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
            as *const perry_runtime::gc::GcHeader;
        if (*header).obj_type != perry_runtime::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(raw as *mut perry_runtime::object::ObjectHeader)
}

fn array_ptr_from_value(value: f64) -> Option<*mut perry_runtime::array::ArrayHeader> {
    if !JSValue::from_bits(value.to_bits()).is_pointer() {
        return None;
    }
    let raw = perry_runtime::value::js_nanbox_get_pointer(value) as usize;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header = (raw as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
            as *const perry_runtime::gc::GcHeader;
        let obj_type = (*header).obj_type;
        if obj_type == perry_runtime::gc::GC_TYPE_ARRAY
            || obj_type == perry_runtime::gc::GC_TYPE_LAZY_ARRAY
        {
            return Some(raw as *mut perry_runtime::array::ArrayHeader);
        }
    }
    None
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

/// Same-agent message snapshot. JSON remains the fallback for ordinary
/// values; typed arrays retain their element kind and are reconstructed as a
/// fresh typed array rather than degrading to a plain JSON object (#6763).
#[derive(Clone)]
enum SerializedMessage {
    Json(String),
    ArrayBuffer(Vec<u8>),
    BigIntTypedArray { kind: u8, lanes: Vec<u64> },
    TypedArray { kind: u8, elements: Vec<f64> },
}

fn serialize_message(value: f64) -> SerializedMessage {
    let raw = perry_runtime::value::js_nanbox_get_pointer(value) as usize;
    // Perry's Uint8Array constructor is BufferHeader-backed rather than a
    // TypedArrayHeader, so preserve that branded representation too.
    if perry_runtime::buffer::is_uint8array_buffer(raw) {
        let buffer = raw as *const perry_runtime::buffer::BufferHeader;
        let len = perry_runtime::buffer::js_buffer_length(buffer).max(0);
        let elements = (0..len)
            .map(|index| perry_runtime::buffer::js_buffer_get(buffer, index) as f64)
            .collect();
        return SerializedMessage::TypedArray {
            kind: perry_runtime::typedarray::KIND_UINT8,
            elements,
        };
    }
    if perry_runtime::buffer::is_array_buffer(raw) {
        let buffer = raw as *const perry_runtime::buffer::BufferHeader;
        let len = perry_runtime::buffer::js_buffer_length(buffer).max(0);
        let bytes = (0..len)
            .map(|index| perry_runtime::buffer::js_buffer_get(buffer, index) as u8)
            .collect();
        return SerializedMessage::ArrayBuffer(bytes);
    }
    if let Some(kind) = perry_runtime::typedarray::lookup_typed_array_kind(raw) {
        let typed = raw as *const perry_runtime::typedarray::TypedArrayHeader;
        let len = perry_runtime::typedarray::js_typed_array_length(typed).max(0);
        if matches!(
            kind,
            perry_runtime::typedarray::KIND_BIGINT64 | perry_runtime::typedarray::KIND_BIGUINT64
        ) {
            let lanes = (0..len)
                .map(|index| {
                    perry_runtime::typedarray::bigint_lane_bits(typed, index).unwrap_or_default()
                })
                .collect();
            return SerializedMessage::BigIntTypedArray { kind, lanes };
        }
        let elements = (0..len)
            .map(|index| perry_runtime::typedarray::js_typed_array_get(typed, index))
            .collect();
        return SerializedMessage::TypedArray { kind, elements };
    }

    let str_ptr = unsafe { js_json_stringify(value, 0) };
    SerializedMessage::Json(
        string_header_to_string(str_ptr).unwrap_or_else(|| "undefined".to_string()),
    )
}

fn deserialize_message(msg: &SerializedMessage) -> f64 {
    match msg {
        SerializedMessage::ArrayBuffer(bytes) => {
            let buffer = perry_runtime::buffer::js_array_buffer_new(bytes.len() as i32);
            for (index, value) in bytes.iter().enumerate() {
                perry_runtime::buffer::js_buffer_set(buffer, index as i32, *value as i32);
            }
            f64::from_bits(JSValue::pointer(buffer as *const u8).bits())
        }
        SerializedMessage::BigIntTypedArray { kind, lanes } => {
            let typed = perry_runtime::typedarray::js_typed_array_new_empty(
                *kind as i32,
                lanes.len() as i32,
            );
            for (index, bits) in lanes.iter().enumerate() {
                perry_runtime::typedarray::set_bigint_lane_bits(typed, index as i32, *bits);
            }
            f64::from_bits(JSValue::pointer(typed as *const u8).bits())
        }
        SerializedMessage::TypedArray { kind, elements } => {
            if *kind == perry_runtime::typedarray::KIND_UINT8 {
                let buffer = perry_runtime::buffer::js_uint8array_alloc(elements.len() as i32);
                for (index, value) in elements.iter().enumerate() {
                    perry_runtime::buffer::js_buffer_set(buffer, index as i32, *value as i32);
                }
                return f64::from_bits(JSValue::pointer(buffer as *const u8).bits());
            }
            let typed = perry_runtime::typedarray::js_typed_array_new_empty(
                *kind as i32,
                elements.len() as i32,
            );
            for (index, value) in elements.iter().enumerate() {
                perry_runtime::typedarray::js_typed_array_set(typed, index as i32, *value);
            }
            f64::from_bits(JSValue::pointer(typed as *const u8).bits())
        }
        SerializedMessage::Json(json) => {
            if json == "undefined" || json.is_empty() {
                return js_undefined();
            }
            let str_ptr = js_string_from_bytes(json.as_ptr(), json.len() as u32);
            f64::from_bits(unsafe { js_json_parse(str_ptr) })
        }
    }
}

/// Reject functions, symbols, marked values, and MessagePorts anywhere in a
/// submitted message graph. The visited set makes cyclic graphs terminate;
/// JSON remains the ordinary-object snapshot fallback.
fn message_value_is_uncloneable(value: f64, visited: &mut HashSet<usize>) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return false;
    }
    let raw = perry_runtime::value::js_nanbox_get_pointer(value) as usize;
    // Node deliberately ignores markAsUncloneable(ArrayBuffer): backing
    // stores remain cloneable (and transferable) even when marked.
    if UNCLONEABLE_OBJECTS.with(|set| set.borrow().contains(&value.to_bits()))
        && !perry_runtime::buffer::is_array_buffer(raw)
    {
        return true;
    }
    if raw < 0x10000
        || perry_runtime::buffer::is_registered_buffer(raw)
        || perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some()
        || perry_runtime::set::is_registered_set(raw)
        || perry_runtime::shared_sab::is_shared_sab(raw)
    {
        return false;
    }
    if perry_runtime::closure::is_closure_ptr(raw)
        || perry_runtime::symbol::is_registered_symbol(raw)
        || port_id_from_object(value).is_some()
    {
        return true;
    }
    if !visited.insert(raw) {
        return false;
    }

    if let Some(array) = array_ptr_from_value(value) {
        let len = perry_runtime::array::js_array_length(array);
        return (0..len).any(|index| {
            message_value_is_uncloneable(
                perry_runtime::array::js_array_get_f64(array, index),
                visited,
            )
        });
    }
    let Some(object) = object_ptr_from_value(value) else {
        return false;
    };
    let field_count = unsafe {
        if (*object).keys_array.is_null() {
            (*object).field_count
        } else {
            perry_runtime::array::js_array_length((*object).keys_array)
        }
    };
    (0..field_count).any(|index| {
        let field = perry_runtime::object::js_object_get_field(object, index);
        message_value_is_uncloneable(f64::from_bits(field.bits()), visited)
    })
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

/// worker_threads DataCloneError: matches Node's message for postMessage
/// rejections of marked-uncloneable / marked-untransferable values (#3159).
fn throw_data_clone_error(detail: &str) -> ! {
    throw_dom_exception("DataCloneError", detail)
}

fn throw_invalid_state_error(detail: &str) -> ! {
    throw_dom_exception("InvalidStateError", detail)
}

fn throw_dom_exception(name: &str, detail: &str) -> ! {
    let message = string_value(detail);
    let name = string_value(name);
    let err = perry_runtime::event_target::js_dom_exception_new(message, name);
    perry_runtime::exception::js_throw(f64::from_bits(JSValue::pointer(err as *const u8).bits()))
}

fn string_value(value: &str) -> f64 {
    f64::from_bits(
        JSValue::string_ptr(js_string_from_bytes(value.as_ptr(), value.len() as u32)).bits(),
    )
}

fn resolved_promise_value(value: f64) -> f64 {
    let promise = perry_runtime::js_promise_resolved(value);
    perry_runtime::value::js_nanbox_pointer(promise as i64)
}

fn event_name(value: f64) -> Option<String> {
    string_value_to_string(value)
}

fn push_parent_event(event: WorkerEvent) {
    PARENT_EVENTS.lock().unwrap().push_back(event);
    perry_runtime::event_pump::js_notify_main_thread();
}

extern "C" fn worker_on(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    worker_add_listener(captured_worker_id(closure), event, callback, false, false)
}

extern "C" fn worker_once(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    worker_add_listener(captured_worker_id(closure), event, callback, true, false)
}

/// `worker.addEventListener(type, listener)` — Web-style listener registration
/// on the main-thread Worker handle. Unlike `on`, the listener receives a
/// `MessageEvent` (with `.data`) for "message" events.
extern "C" fn worker_add_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    worker_add_listener(captured_worker_id(closure), event, callback, false, true)
}

/// `worker.removeEventListener(type, listener)`.
extern "C" fn worker_remove_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    worker_off(closure, event, callback)
}

fn worker_add_listener(
    worker_id: u64,
    event: f64,
    callback: f64,
    once: bool,
    web_event: bool,
) -> f64 {
    ensure_worker_gc_scanner();
    let Some(event) = event_name(event) else {
        return js_undefined();
    };
    let mut workers = WORKERS.lock().unwrap();
    if let Some(worker) = workers.get_mut(&worker_id) {
        worker
            .listeners
            .entry(event)
            .or_default()
            .push(WorkerListener {
                callback_bits: closure_arg_bits(callback),
                once,
                web_event,
            });
    }
    js_undefined()
}

extern "C" fn worker_off(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    let worker_id = captured_worker_id(closure);
    let Some(event) = event_name(event) else {
        return js_undefined();
    };
    let callback_bits = closure_arg_bits(callback);
    let mut workers = WORKERS.lock().unwrap();
    if let Some(worker) = workers.get_mut(&worker_id) {
        if let Some(listeners) = worker.listeners.get_mut(&event) {
            listeners.retain(|listener| listener.callback_bits != callback_bits);
        }
    }
    js_undefined()
}

extern "C" fn worker_post_message(closure: *const ClosureHeader, value: f64) -> f64 {
    let worker_id = captured_worker_id(closure);
    let message = unsafe { serialize_nanbox_for_thread(value.to_bits()) };
    let sender = WORKERS
        .lock()
        .unwrap()
        .get(&worker_id)
        .map(|worker| worker.sender.clone());
    if let Some(sender) = sender {
        let _ = sender.send(WorkerCommand::Message(message));
    }
    js_undefined()
}

extern "C" fn worker_terminate(closure: *const ClosureHeader) -> f64 {
    worker_terminate_by_id(captured_worker_id(closure))
}

extern "C" fn worker_ref(closure: *const ClosureHeader) -> f64 {
    worker_ref_by_id(captured_worker_id(closure))
}

fn worker_ref_by_id(worker_id: u64) -> f64 {
    if let Some(worker) = WORKERS.lock().unwrap().get_mut(&worker_id) {
        worker.refed = true;
    }
    js_undefined()
}

extern "C" fn worker_unref(closure: *const ClosureHeader) -> f64 {
    worker_unref_by_id(captured_worker_id(closure))
}

fn worker_unref_by_id(worker_id: u64) -> f64 {
    if let Some(worker) = WORKERS.lock().unwrap().get_mut(&worker_id) {
        worker.refed = false;
    }
    js_undefined()
}

extern "C" fn worker_get_heap_statistics(closure: *const ClosureHeader) -> f64 {
    worker_get_heap_statistics_by_id(captured_worker_id(closure))
}

fn worker_get_heap_statistics_by_id(_worker_id: u64) -> f64 {
    resolved_promise_value(unsafe { js_v8_get_heap_statistics() })
}

extern "C" fn worker_cpu_usage(closure: *const ClosureHeader, prior: f64) -> f64 {
    worker_cpu_usage_by_id(captured_worker_id(closure), prior)
}

fn worker_cpu_usage_by_id(_worker_id: u64, prior: f64) -> f64 {
    resolved_promise_value(unsafe { js_process_cpu_usage(prior) })
}

extern "C" fn worker_get_heap_snapshot(closure: *const ClosureHeader, options: f64) -> f64 {
    worker_get_heap_snapshot_by_id(captured_worker_id(closure), options)
}

fn worker_get_heap_snapshot_by_id(_worker_id: u64, _options: f64) -> f64 {
    resolved_promise_value(worker_readable_stream_object())
}

extern "C" fn worker_start_cpu_profile(closure: *const ClosureHeader) -> f64 {
    worker_start_cpu_profile_by_id(captured_worker_id(closure))
}

fn worker_start_cpu_profile_by_id(_worker_id: u64) -> f64 {
    resolved_promise_value(worker_profile_handle(0))
}

extern "C" fn worker_start_heap_profile(closure: *const ClosureHeader) -> f64 {
    worker_start_heap_profile_by_id(captured_worker_id(closure))
}

fn worker_start_heap_profile_by_id(_worker_id: u64) -> f64 {
    resolved_promise_value(worker_profile_handle(1))
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_get_heap_statistics(receiver: i64) -> f64 {
    let worker_id = worker_id_from_receiver(receiver).unwrap_or(0);
    worker_get_heap_statistics_by_id(worker_id)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_cpu_usage(receiver: i64, prior: f64) -> f64 {
    let worker_id = worker_id_from_receiver(receiver).unwrap_or(0);
    worker_cpu_usage_by_id(worker_id, prior)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_get_heap_snapshot(receiver: i64, options: f64) -> f64 {
    let worker_id = worker_id_from_receiver(receiver).unwrap_or(0);
    worker_get_heap_snapshot_by_id(worker_id, options)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_start_cpu_profile(receiver: i64) -> f64 {
    let worker_id = worker_id_from_receiver(receiver).unwrap_or(0);
    worker_start_cpu_profile_by_id(worker_id)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_start_heap_profile(receiver: i64) -> f64 {
    let worker_id = worker_id_from_receiver(receiver).unwrap_or(0);
    worker_start_heap_profile_by_id(worker_id)
}

fn worker_terminate_by_id(worker_id: u64) -> f64 {
    let promise = unsafe { crate::common::async_bridge::js_promise_new_for_native_resolution() };
    let promise_ptr = promise as usize;
    let resolved_now = {
        let mut workers = WORKERS.lock().unwrap();
        match workers.get_mut(&worker_id) {
            // The worker already exited (its entry returned and it pushed an
            // Exit event) — there is no live thread to receive `Terminate`, so
            // no further Exit event will arrive to settle the promise. Resolve
            // it immediately. `terminate()` on an already-finished worker is a
            // no-op in Node and resolves at once. Without this, a worker pool
            // whose workers run-and-exit (no `parentPort` message loop) would
            // hang forever on `Promise.all(workers.map(w => w.terminate()))`.
            Some(worker) if !worker.alive => true,
            Some(worker) => {
                worker.terminate_promise = Some(promise_ptr);
                let _ = worker.sender.send(WorkerCommand::Terminate);
                false
            }
            None => true,
        }
    };
    if resolved_now {
        perry_runtime::js_promise_resolve(promise, 1.0);
    }
    perry_runtime::value::js_nanbox_pointer(promise as i64)
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

#[no_mangle]
pub extern "C" fn js_worker_threads_move_message_port_to_context(port: f64, _context: f64) -> f64 {
    let Some(port_id) = port_id_from_object(port) else {
        return js_undefined();
    };
    if !MESSAGE_PORTS.with(|ports| ports.borrow().contains_key(&port_id)) {
        return js_undefined();
    }
    object_value(message_port_object(port_id))
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

#[no_mangle]
pub extern "C" fn js_worker_threads_worker_new(entry_ptr: i64, options: f64) -> f64 {
    ensure_worker_gc_scanner();
    crate::common::async_bridge::ensure_pump_registered();

    let worker_id = NEXT_WORKER_ID.fetch_add(1, Ordering::Relaxed);
    let options_state = WorkerOptions::from_value(options);
    let worker_data = if is_undefined(options) {
        None
    } else {
        let data = get_object_field_from_value(options, "workerData");
        if is_undefined(data) {
            None
        } else {
            Some(unsafe { serialize_nanbox_for_thread(data.to_bits()) })
        }
    };
    let (tx, rx) = mpsc::channel::<WorkerCommand>();
    WORKERS.lock().unwrap().insert(
        worker_id,
        WorkerRecord {
            sender: tx,
            listeners: HashMap::new(),
            alive: true,
            refed: true,
            terminate_promise: None,
        },
    );

    let thread_options = options_state.clone();
    std::thread::spawn(move || {
        let previous_env = apply_worker_env(&thread_options.env);
        CURRENT_WORKER_ID.with(|id| id.set(worker_id));
        CURRENT_WORKER_DATA.with(|slot| *slot.borrow_mut() = worker_data);
        CURRENT_THREAD_NAME.with(|slot| *slot.borrow_mut() = thread_options.thread_name.clone());
        CURRENT_RESOURCE_LIMITS.with(|slot| slot.set(thread_options.resource_limits));
        push_parent_event(WorkerEvent::Online(worker_id));

        let entry: WorkerEntry = unsafe { std::mem::transmute(entry_ptr as usize) };
        let mut exit_code = 0;
        let result = catch_unwind(AssertUnwindSafe(|| {
            entry();
            // Keep the worker thread alive to service main→worker messages only
            // if it registered a `message` consumer (Node-style `on` OR
            // Web-style `addEventListener`). Otherwise the worker is done once
            // its entry returns.
            let has_message_consumer = MESSAGE_CALLBACK.with(|cb| cb.borrow().is_some())
                || MESSAGE_EVENT_CALLBACKS.with(|cbs| !cbs.borrow().is_empty());
            if !has_message_consumer {
                return;
            }
            loop {
                match rx.recv() {
                    Ok(WorkerCommand::Message(message)) => {
                        deliver_parent_port_message(&message);
                    }
                    Ok(WorkerCommand::DirectMessage {
                        message,
                        source_thread_id,
                        ack,
                    }) => {
                        let result =
                            direct_message::deliver_worker_message(&message, source_thread_id);
                        let _ = ack.send(result);
                    }
                    Ok(WorkerCommand::Terminate) => {
                        exit_code = 1;
                        break;
                    }
                    Err(_) => break,
                }
            }
        }));
        restore_worker_env(previous_env);

        let exit_code = match result {
            Ok(()) => exit_code,
            Err(_) => {
                push_parent_event(WorkerEvent::Error(worker_id));
                1
            }
        };
        push_parent_event(WorkerEvent::Exit(worker_id, exit_code));
    });

    object_value(worker_object(worker_id, &options_state))
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
    if let Some(bits) = CURRENT_WORKER_DATA.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|data| unsafe { deserialize_nanbox_on_current_thread(data) })
    }) {
        return f64::from_bits(bits);
    }
    // Node defaults `workerData` to `null` (typeof === "object") on the main
    // thread and in workers spawned without a workerData option — not
    // `undefined`. CURRENT_WORKER_DATA above carries the in-worker payload;
    // this fallback must stay `null` to match the value-only main-thread
    // surface (#3899) that the namespace getter now routes through here.
    let data = std::env::var("PERRY_WORKER_DATA").unwrap_or_else(|_| "undefined".to_string());
    if data == "undefined" || data.is_empty() {
        return f64::from_bits(JSValue::null().bits());
    }
    // JSON-parse the data
    let ptr = js_string_from_bytes(data.as_ptr(), data.len() as u32);
    let bits = unsafe { js_json_parse(ptr) };
    f64::from_bits(bits)
}

#[no_mangle]
pub extern "C" fn js_worker_threads_is_main_thread() -> f64 {
    js_bool(CURRENT_WORKER_ID.with(|id| id.get()) == 0)
}

/// Get parentPort handle (returns NaN-boxed POINTER_TAG handle)
#[no_mangle]
pub extern "C" fn js_worker_threads_parent_port() -> f64 {
    if CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        return object_value(parent_port::worker_parent_port_object());
    }
    // On the main thread there is no parent port: Node exposes `parentPort`
    // as `null` (only a spawned Worker has a MessagePort back to its parent).
    // Returning a `{}` object here made `if (parentPort)` truthy on the main
    // thread, diverging from Node.
    js_null()
}

#[no_mangle]
pub extern "C" fn js_worker_threads_thread_name() -> f64 {
    CURRENT_THREAD_NAME.with(|slot| string_value(&slot.borrow()))
}

#[no_mangle]
pub extern "C" fn js_worker_threads_resource_limits() -> f64 {
    if CURRENT_WORKER_ID.with(|id| id.get()) == 0 {
        return object_value(empty_object());
    }
    CURRENT_RESOURCE_LIMITS
        .with(|limits| object_value(worker_resource_limits_object(&limits.get())))
}

/// parentPort.postMessage(data) - JSON-stringify and write to stdout
#[no_mangle]
pub extern "C" fn js_worker_threads_post_message(data: f64) -> f64 {
    let worker_id = CURRENT_WORKER_ID.with(|id| id.get());
    if worker_id != 0 {
        let message = unsafe { serialize_nanbox_for_thread(data.to_bits()) };
        push_parent_event(WorkerEvent::Message(worker_id, message));
        return js_undefined();
    }
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
            if CURRENT_WORKER_ID.with(|id| id.get()) == 0 {
                // Start the stdin reader if not already started.
                start_stdin_reader();
            }
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

/// `parentPort.addEventListener("message", fn)` / `removeEventListener`.
/// Web-style registration on the in-worker parent port. `add` adds the
/// listener; otherwise removes it.
fn parent_port_event_listener(event_ptr: i64, callback: i64, add: bool) -> f64 {
    let event_name = string_value_to_string(f64::from_bits(event_ptr as u64)).unwrap_or_default();
    if event_name != "message" || callback == 0 {
        return js_undefined();
    }
    MESSAGE_EVENT_CALLBACKS.with(|cbs| {
        let mut cbs = cbs.borrow_mut();
        if add {
            if !cbs.contains(&callback) {
                cbs.push(callback);
            }
        } else {
            cbs.retain(|c| *c != callback);
        }
    });
    js_undefined()
}

/// `parentPort.addEventListener("message", fn)` — called from parent_port.rs.
/// Registers the worker-side GC scanner for the listener closures.
pub(super) fn js_worker_threads_parent_port_event_add(event_ptr: i64, callback: i64) -> f64 {
    ensure_parent_port_event_gc_scanner();
    parent_port_event_listener(event_ptr, callback, true)
}

/// `parentPort.removeEventListener("message", fn)` — called from parent_port.rs.
pub(super) fn js_worker_threads_parent_port_event_remove(event_ptr: i64, callback: i64) -> f64 {
    parent_port_event_listener(event_ptr, callback, false)
}

static PARENT_PORT_EVENT_GC_REGISTERED: Once = Once::new();

fn ensure_parent_port_event_gc_scanner() {
    PARENT_PORT_EVENT_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:worker_threads:parentPortEventListeners",
            scan_parent_port_event_roots_mut,
        );
    });
}

/// Visit one raw closure pointer stored as `i64`.
///
/// Factored out because this scanner covers three sibling slots and three
/// copies of the box/visit/unbox dance is how the fourth one gets forgotten —
/// which is precisely what happened to `MESSAGE_CALLBACK` and `CLOSE_CALLBACK`
/// (#7231).
fn visit_raw_closure_i64(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>, cb: &mut i64) {
    // Box it into a NaN-boxed pointer slot so the GC can visit + relocate it,
    // then unbox.
    let mut boxed = perry_runtime::value::js_nanbox_pointer(*cb).to_bits();
    visitor.visit_nanbox_u64_slot(&mut boxed);
    *cb = perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(boxed));
}

/// Root + rewrite every `parentPort` listener closure.
///
/// **#7231: this scanner used to walk one of three sibling slots.**
/// `MESSAGE_EVENT_CALLBACKS`, `MESSAGE_CALLBACK` and `CLOSE_CALLBACK` are
/// declared in the same `thread_local!` block and all hold the same thing — a
/// raw `ClosureHeader*` as `i64`, the only reference to a closure the user
/// passed to `parentPort.on(...)` / `.addEventListener(...)`. Only the first
/// was visited, so the Node-style `on('message')` and `on('close')` handlers
/// were reclaimed or relocated by the next collection inside a worker.
///
/// A partially-correct scanner is worse than an absent one: it reads as
/// covered.
fn scan_parent_port_event_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    MESSAGE_EVENT_CALLBACKS.with(|cbs| {
        for cb in cbs.borrow_mut().iter_mut() {
            visit_raw_closure_i64(visitor, cb);
        }
    });
    MESSAGE_CALLBACK.with(|cb| {
        if let Some(ptr) = cb.borrow_mut().as_mut() {
            visit_raw_closure_i64(visitor, ptr);
        }
    });
    CLOSE_CALLBACK.with(|cb| {
        if let Some(ptr) = cb.borrow_mut().as_mut() {
            visit_raw_closure_i64(visitor, ptr);
        }
    });
}

// `#[used]` keepalive anchors (#3157/#3159) — these `#[no_mangle]` entry points
// are emitted by codegen (native-table dispatch) and called only from generated
// `.o`. The auto-optimize whole-program-LLVM rebuild internalizes + dead-strips
// unreferenced `#[no_mangle]` symbols, so anchor them here. See
// [[project_auto_optimize_keepalive_3320]].
#[used]
static KEEP_WT_MESSAGE_CHANNEL_NEW: extern "C" fn() -> f64 = js_worker_threads_message_channel_new;
#[used]
static KEEP_WT_BROADCAST_NEW: extern "C" fn(f64) -> f64 = js_worker_threads_broadcast_channel_new;
#[used]
static KEEP_WT_RECEIVE_MESSAGE_ON_PORT: extern "C" fn(f64) -> f64 =
    js_worker_threads_receive_message_on_port;
#[used]
static KEEP_WT_MARK_AS_UNCLONEABLE: extern "C" fn(f64) -> f64 =
    js_worker_threads_mark_as_uncloneable;
#[used]
static KEEP_WT_WORKER_NEW: extern "C" fn(i64, f64) -> f64 = js_worker_threads_worker_new;
#[used]
static KEEP_WT_WORKER_POST_MESSAGE: extern "C" fn(i64, f64) -> f64 =
    js_worker_threads_worker_post_message;
#[used]
static KEEP_WT_WORKER_ON: extern "C" fn(i64, f64, i64) -> f64 = js_worker_threads_worker_on;
#[used]
static KEEP_WT_WORKER_ONCE: extern "C" fn(i64, f64, i64) -> f64 = js_worker_threads_worker_once;
#[used]
static KEEP_WT_WORKER_OFF: extern "C" fn(i64, f64, i64) -> f64 = js_worker_threads_worker_off;
#[used]
static KEEP_WT_WORKER_ADD_EVENT_LISTENER: extern "C" fn(i64, f64, i64) -> f64 =
    worker_surface::js_worker_threads_worker_add_event_listener;
#[used]
static KEEP_WT_WORKER_REMOVE_EVENT_LISTENER: extern "C" fn(i64, f64, i64) -> f64 =
    worker_surface::js_worker_threads_worker_remove_event_listener;
#[used]
static KEEP_WT_WORKER_TERMINATE: extern "C" fn(i64) -> f64 = js_worker_threads_worker_terminate;
#[used]
static KEEP_WT_WORKER_REF: extern "C" fn(i64) -> f64 = js_worker_threads_worker_ref;
#[used]
static KEEP_WT_WORKER_UNREF: extern "C" fn(i64) -> f64 = js_worker_threads_worker_unref;

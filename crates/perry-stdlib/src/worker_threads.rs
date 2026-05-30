//! worker_threads module for Perry
//!
//! Provides parentPort and workerData support for worker processes.
//! Communication is via stdin/stdout JSON IPC:
//! - workerData: Read from PERRY_WORKER_DATA environment variable, JSON-parsed
//! - parentPort.postMessage(data): JSON-stringify data, write to stdout
//! - parentPort.on('message', callback): Async stdin reader, dispatch on main thread

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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

fn closure_value(func_ptr: *const u8, arity: u32) -> f64 {
    perry_runtime::closure::js_register_closure_arity(func_ptr, arity);
    let closure = perry_runtime::closure::js_closure_alloc(func_ptr, 0);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

extern "C" fn worker_threads_noop0(_closure: *const ClosureHeader) -> f64 {
    js_undefined()
}

extern "C" fn worker_threads_noop1(_closure: *const ClosureHeader, _arg0: f64) -> f64 {
    js_undefined()
}

extern "C" fn worker_threads_noop2(_closure: *const ClosureHeader, _arg0: f64, _arg1: f64) -> f64 {
    js_undefined()
}

extern "C" fn worker_threads_has_ref(_closure: *const ClosureHeader) -> f64 {
    js_bool(true)
}

fn message_port_object() -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "postMessage",
        closure_value(worker_threads_noop2 as *const u8, 2),
    );
    set_object_field(
        obj,
        "close",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "start",
        closure_value(worker_threads_noop0 as *const u8, 0),
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
    obj
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
pub extern "C" fn js_worker_threads_mark_as_uncloneable(_value: f64) -> f64 {
    js_undefined()
}

/// worker_threads.moveMessagePortToContext(port, context)
#[no_mangle]
pub extern "C" fn js_worker_threads_move_message_port_to_context(port: f64, _context: f64) -> f64 {
    port
}

/// worker_threads.receiveMessageOnPort(port)
#[no_mangle]
pub extern "C" fn js_worker_threads_receive_message_on_port(_port: f64) -> f64 {
    js_undefined()
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
#[no_mangle]
pub extern "C" fn js_worker_threads_message_channel_new() -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(obj, "port1", object_value(message_port_object()));
    set_object_field(obj, "port2", object_value(message_port_object()));
    object_value(obj)
}

/// new worker_threads.BroadcastChannel(name)
#[no_mangle]
pub extern "C" fn js_worker_threads_broadcast_channel_new(name: f64) -> f64 {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_field(
        obj,
        "postMessage",
        closure_value(worker_threads_noop1 as *const u8, 1),
    );
    set_object_field(
        obj,
        "close",
        closure_value(worker_threads_noop0 as *const u8, 0),
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
    set_object_field(obj, "onmessage", js_null());
    set_object_field(obj, "onmessageerror", js_null());
    set_object_field(obj, "name", name);
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

//! Child-side `process` IPC surface.
//!
//! Node initializes this from `NODE_CHANNEL_FD` during bootstrap. Perry adopts
//! the same inherited Unix fd convention used by its `child_process.fork()`
//! parent side and speaks newline-delimited JSON frames for this cut.

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    js_register_closure_arity, js_register_closure_length, ClosureHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(unix)]
use std::io::{BufRead, Write};
#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(unix)]
use std::os::unix::net::UnixStream;

enum IpcEvent {
    Message(String),
    Closed,
}

struct ChildIpcState {
    initialized: bool,
    available: bool,
    connected: bool,
    refed: bool,
    disconnect_emitted: bool,
    #[cfg(unix)]
    send: Option<UnixStream>,
    queue: VecDeque<IpcEvent>,
}

impl ChildIpcState {
    fn new() -> Self {
        Self {
            initialized: false,
            available: false,
            connected: false,
            refed: false,
            disconnect_emitted: false,
            #[cfg(unix)]
            send: None,
            queue: VecDeque::new(),
        }
    }
}

static CHILD_IPC: OnceLock<Mutex<ChildIpcState>> = OnceLock::new();

fn ipc_state() -> &'static Mutex<ChildIpcState> {
    CHILD_IPC.get_or_init(|| Mutex::new(ChildIpcState::new()))
}

fn ipc_lock() -> MutexGuard<'static, ChildIpcState> {
    match ipc_state().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits())
}

fn set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

type IpcFunction0 = extern "C" fn(*const ClosureHeader) -> f64;
type IpcFunction4 = extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64;

fn ipc_function0(name: &str, thunk: IpcFunction0, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    js_register_closure_arity(func_ptr, 0);
    js_register_closure_length(func_ptr, length);
    let closure = js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn ipc_function4(name: &str, thunk: IpcFunction4, length: u32) -> f64 {
    let func_ptr = thunk as *const u8;
    js_register_closure_arity(func_ptr, 4);
    js_register_closure_length(func_ptr, length);
    let closure = js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, length);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Initialize the inherited child IPC channel once. This also removes Node's
/// bootstrap-only env vars so `process.env.NODE_CHANNEL_FD` follows Node.
pub(crate) fn process_ipc_ensure_initialized() {
    {
        let mut state = ipc_lock();
        if state.initialized {
            return;
        }
        state.initialized = true;
    }

    let fd_var = std::env::var("NODE_CHANNEL_FD").ok();
    let serialization_mode =
        std::env::var("NODE_CHANNEL_SERIALIZATION_MODE").unwrap_or_else(|_| "json".to_string());
    std::env::remove_var("NODE_CHANNEL_FD");
    std::env::remove_var("NODE_CHANNEL_SERIALIZATION_MODE");

    #[cfg(unix)]
    initialize_unix_ipc(fd_var, &serialization_mode);

    #[cfg(not(unix))]
    {
        let _ = (fd_var, serialization_mode);
    }
}

#[cfg(unix)]
fn initialize_unix_ipc(fd_var: Option<String>, serialization_mode: &str) {
    if serialization_mode == "advanced" {
        return;
    }
    let Some(fd) = fd_var
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|fd| *fd >= 0)
    else {
        return;
    };

    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    let send = match stream.try_clone() {
        Ok(send) => send,
        Err(_) => return,
    };
    spawn_ipc_reader(stream);

    let mut state = ipc_lock();
    state.available = true;
    state.connected = true;
    state.refed = false;
    state.disconnect_emitted = false;
    state.send = Some(send);
}

#[cfg(unix)]
fn spawn_ipc_reader(sock: UnixStream) {
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(sock);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => push_ipc_event(IpcEvent::Message(line)),
                Ok(_) => {}
                Err(_) => break,
            }
        }
        push_ipc_event(IpcEvent::Closed);
    });
}

fn push_ipc_event(event: IpcEvent) {
    ipc_lock().queue.push_back(event);
}

fn ipc_is_available() -> bool {
    ipc_lock().available
}

fn ipc_is_connected() -> bool {
    let state = ipc_lock();
    state.available && state.connected
}

pub(crate) fn process_ipc_property(name: &str) -> Option<f64> {
    match name {
        "send" | "disconnect" | "connected" | "channel" => {}
        _ => return None,
    }

    process_ipc_ensure_initialized();
    let available = ipc_is_available();
    Some(match name {
        "send" => {
            if available {
                ipc_function4("send", process_ipc_send_fn, 4)
            } else {
                undefined_value()
            }
        }
        "disconnect" => {
            if available {
                ipc_function0("disconnect", process_ipc_disconnect_fn, 0)
            } else {
                undefined_value()
            }
        }
        "connected" => {
            if available {
                bool_value(ipc_is_connected())
            } else {
                undefined_value()
            }
        }
        "channel" => {
            if !available {
                undefined_value()
            } else if ipc_is_connected() {
                process_channel_value()
            } else {
                null_value()
            }
        }
        _ => undefined_value(),
    })
}

fn process_channel_value() -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    set_field(obj, "ref", ipc_function0("ref", process_channel_ref_fn, 0));
    set_field(
        obj,
        "unref",
        ipc_function0("unref", process_channel_unref_fn, 0),
    );
    object_value(obj)
}

extern "C" fn process_channel_ref_fn(_closure: *const ClosureHeader) -> f64 {
    process_ipc_ensure_initialized();
    let mut state = ipc_lock();
    if state.available && state.connected {
        state.refed = true;
    }
    undefined_value()
}

extern "C" fn process_channel_unref_fn(_closure: *const ClosureHeader) -> f64 {
    process_ipc_ensure_initialized();
    let mut state = ipc_lock();
    if state.available && state.connected {
        state.refed = false;
    }
    undefined_value()
}

extern "C" fn process_ipc_disconnect_fn(_closure: *const ClosureHeader) -> f64 {
    process_ipc_disconnect_call()
}

extern "C" fn process_ipc_send_fn(
    _closure: *const ClosureHeader,
    message: f64,
    a2: f64,
    a3: f64,
    a4: f64,
) -> f64 {
    process_ipc_send_call(message, a2, a3, a4)
}

pub(crate) fn process_ipc_send_call(message: f64, a2: f64, a3: f64, a4: f64) -> f64 {
    let callback = [a4, a3, a2]
        .into_iter()
        .find(|v| !crate::fs::extract_closure_ptr(*v).is_null());
    let ok = process_ipc_send_message(message);
    if let Some(cb) = callback {
        defer_send_callback(cb, ok);
    }
    bool_value(ok)
}

pub(crate) fn process_ipc_disconnect_call() -> f64 {
    process_ipc_disconnect_local();
    undefined_value()
}

fn process_ipc_send_message(message: f64) -> bool {
    process_ipc_ensure_initialized();
    if !ipc_is_connected() {
        return false;
    }

    #[cfg(not(unix))]
    {
        let _ = message;
        false
    }

    #[cfg(unix)]
    {
        let Some(frame) = json_frame(message) else {
            return false;
        };
        let mut emit_disconnect = false;
        let ok = {
            let mut state = ipc_lock();
            if !state.available || !state.connected {
                false
            } else {
                let write_ok = state
                    .send
                    .as_mut()
                    .is_some_and(|sock| sock.write_all(&frame).is_ok());
                if write_ok {
                    true
                } else {
                    state.connected = false;
                    state.refed = false;
                    state.send = None;
                    if !state.disconnect_emitted {
                        state.disconnect_emitted = true;
                        emit_disconnect = true;
                    }
                    false
                }
            }
        };
        if emit_disconnect {
            crate::os::emit_process_event("disconnect", &[]);
        }
        ok
    }
}

/// Write an already-serialized JSON object as one newline-delimited frame on
/// the child IPC channel. Used by `node:cluster` internal messages
/// (`{cmd:"NODE_CLUSTER", ...}`, #4914) where the payload is built in Rust and
/// never exists as a JS value.
pub(crate) fn process_ipc_send_raw_json(json: &str) -> bool {
    process_ipc_ensure_initialized();
    #[cfg(not(unix))]
    {
        let _ = json;
        false
    }
    #[cfg(unix)]
    {
        let mut frame = Vec::with_capacity(json.len() + 1);
        frame.extend_from_slice(json.as_bytes());
        frame.push(b'\n');
        let mut state = ipc_lock();
        if !state.available || !state.connected {
            return false;
        }
        state
            .send
            .as_mut()
            .is_some_and(|sock| sock.write_all(&frame).is_ok())
    }
}

#[cfg(unix)]
fn json_frame(message: f64) -> Option<Vec<u8>> {
    let sh = unsafe { crate::json::js_json_stringify(message, 0) };
    if sh.is_null() {
        return None;
    }
    let len = unsafe { (*sh).byte_len as usize };
    let data = unsafe { (sh as *const u8).add(std::mem::size_of::<StringHeader>()) };
    let mut line = Vec::with_capacity(len + 1);
    line.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
    line.push(b'\n');
    Some(line)
}

fn defer_send_callback(cb: f64, ok: bool) {
    let func_ptr = process_ipc_send_callback_thunk as *const u8;
    js_register_closure_arity(func_ptr, 0);
    js_register_closure_length(func_ptr, 0);
    let deferred = js_closure_alloc(func_ptr, 2);
    js_closure_set_capture_ptr(deferred, 0, cb.to_bits() as i64);
    let flag = if ok { TAG_TRUE } else { TAG_FALSE };
    js_closure_set_capture_ptr(deferred, 1, flag as i64);
    crate::timer::js_set_immediate_callback(deferred as i64);
}

extern "C" fn process_ipc_send_callback_thunk(closure: *const ClosureHeader) -> f64 {
    let cb = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    if crate::fs::extract_closure_ptr(cb).is_null() {
        return undefined_value();
    }
    let flag = js_closure_get_capture_ptr(closure, 1) as u64;
    let arg = if flag == TAG_TRUE {
        null_value()
    } else {
        channel_closed_error()
    };
    let args = [arg];
    unsafe { js_native_call_value(cb, args.as_ptr(), args.len()) };
    undefined_value()
}

fn channel_closed_error() -> f64 {
    let msg = js_string_from_bytes(b"Channel closed".as_ptr(), 14);
    crate::node_submodules::register_error_code_pub(msg, "ERR_IPC_CHANNEL_CLOSED");
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

fn process_ipc_disconnect_local() {
    process_ipc_ensure_initialized();
    #[cfg(unix)]
    let send = {
        let mut state = ipc_lock();
        if !state.available {
            return;
        }
        state.connected = false;
        state.refed = false;
        state.send.take()
    };
    #[cfg(unix)]
    if let Some(sock) = send {
        let _ = sock.shutdown(std::net::Shutdown::Both);
    }

    if mark_disconnect_emitted() {
        crate::os::emit_process_event("disconnect", &[]);
    }
}

fn mark_closed_from_event() -> bool {
    let mut state = ipc_lock();
    if !state.available {
        return false;
    }
    state.connected = false;
    state.refed = false;
    #[cfg(unix)]
    {
        state.send = None;
    }
    if state.disconnect_emitted {
        false
    } else {
        state.disconnect_emitted = true;
        true
    }
}

fn mark_disconnect_emitted() -> bool {
    let mut state = ipc_lock();
    if !state.available || state.disconnect_emitted {
        false
    } else {
        state.disconnect_emitted = true;
        true
    }
}

pub(crate) fn process_ipc_note_listener(event: &str) {
    if !matches!(event, "message" | "disconnect") {
        return;
    }
    process_ipc_ensure_initialized();
    let mut state = ipc_lock();
    if state.available && state.connected {
        state.refed = true;
    }
}

#[no_mangle]
pub extern "C" fn js_process_ipc_drain() -> i32 {
    process_ipc_ensure_initialized();
    let events = {
        let mut state = ipc_lock();
        state.queue.drain(..).collect::<Vec<_>>()
    };
    let mut count = 0i32;
    for event in events {
        match event {
            IpcEvent::Message(json) => {
                let sh = js_string_from_bytes(json.as_ptr(), json.len() as u32);
                let msg = f64::from_bits(unsafe { crate::json::js_json_parse(sh) }.bits());
                crate::os::emit_process_event("message", &[msg]);
                count = count.saturating_add(1);
            }
            IpcEvent::Closed => {
                if mark_closed_from_event() {
                    crate::os::emit_process_event("disconnect", &[]);
                }
                count = count.saturating_add(1);
            }
        }
    }
    count
}

#[no_mangle]
pub extern "C" fn js_process_ipc_has_active() -> i32 {
    process_ipc_ensure_initialized();
    let state = ipc_lock();
    if state.available && state.connected && state.refed {
        1
    } else {
        0
    }
}

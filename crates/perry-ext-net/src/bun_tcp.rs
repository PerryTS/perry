//! Bun's low-level TCP facade over the existing `node:net` transport.
//!
//! The transport, accept loop, and event pump remain shared with `net.Socket`.
//! This module only owns Bun's handler-table calling convention, per-socket
//! `.data`, Promise settlement for `connect`, and bounded write admission.

use bytes::Bytes;
use perry_ffi::{
    alloc_buffer, alloc_string, JsClosure, JsPromise, JsValue, Promise, RawClosureHeader,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::{
    build_error_object, get_object_number_field, get_object_string_field, get_object_value_field,
    statics, SocketCommand,
};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const WRITE_HIGH_WATER_MARK: usize = 64 * 1024;

#[derive(Clone, Default)]
struct Handlers {
    open: i64,
    data: i64,
    drain: i64,
    close: i64,
    error: i64,
    connect_error: i64,
    end: i64,
}

struct BunSocket {
    handlers: Handlers,
    data_bits: u64,
    connect_promise: usize,
    listener: Option<i64>,
    opened: bool,
    paused: bool,
    paused_data: VecDeque<Bytes>,
    paused_end: bool,
    paused_close: bool,
    shutting_down: bool,
    needs_drain: bool,
    last_error: Option<String>,
}

struct BunServer {
    handlers: Handlers,
    data_bits: u64,
    refed: bool,
    ready: bool,
}

#[derive(Clone)]
enum Endpoint {
    Tcp { host: String, port: f64 },
    Unix(String),
}

struct ParsedOptions {
    endpoint: Endpoint,
    handlers: Handlers,
    data_bits: u64,
}

fn sockets() -> &'static Mutex<HashMap<i64, BunSocket>> {
    static SOCKETS: OnceLock<Mutex<HashMap<i64, BunSocket>>> = OnceLock::new();
    SOCKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn servers() -> &'static Mutex<HashMap<i64, BunServer>> {
    static SERVERS: OnceLock<Mutex<HashMap<i64, BunServer>>> = OnceLock::new();
    SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn write_tokens() -> &'static Mutex<HashMap<u64, i64>> {
    static TOKENS: OnceLock<Mutex<HashMap<u64, i64>>> = OnceLock::new();
    TOKENS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn handle_value(handle: i64) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & POINTER_MASK))
}

fn callback_pointer(value: f64) -> i64 {
    extern "C" {
        fn js_net_callback_ptr(value: f64) -> i64;
    }
    unsafe { js_net_callback_ptr(value) }
}

/// Parse every GC-managed option while transient roots keep the option object,
/// handler object, callbacks, and initial data current across property-key
/// allocations. No Perry allocation occurs between this returning and the
/// caller publishing the resulting pointers in the scanned side tables.
unsafe fn parse_options(options: f64, listen: bool) -> Option<ParsedOptions> {
    let roots = perry_ffi::TransientRootScope::enter();
    let options = roots.root_nanbox(options);
    let handler_value = get_object_value_field(options.get(), "socket")?;
    let handler = roots.root_nanbox(handler_value);
    let names = [
        "open",
        "data",
        "drain",
        "close",
        "error",
        "connectError",
        "end",
    ];
    let callbacks = names
        .iter()
        .map(|name| {
            roots.root_nanbox(get_object_value_field(handler.get(), name).unwrap_or_else(undefined))
        })
        .collect::<Vec<_>>();
    let data =
        roots.root_nanbox(get_object_value_field(options.get(), "data").unwrap_or_else(undefined));

    let unix = get_object_string_field(options.get(), "unix")
        .or_else(|| get_object_string_field(options.get(), "path"));
    let endpoint = if let Some(path) = unix {
        Endpoint::Unix(path)
    } else {
        let default_host = if listen { "0.0.0.0" } else { "127.0.0.1" };
        let host = get_object_string_field(options.get(), "hostname")
            .or_else(|| get_object_string_field(options.get(), "host"))
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| default_host.to_string());
        let port = get_object_number_field(options.get(), "port")?;
        Endpoint::Tcp { host, port }
    };

    let ptr = |index: usize| callback_pointer(callbacks[index].get());
    Some(ParsedOptions {
        endpoint,
        handlers: Handlers {
            open: ptr(0),
            data: ptr(1),
            drain: ptr(2),
            close: ptr(3),
            error: ptr(4),
            connect_error: ptr(5),
            end: ptr(6),
        },
        data_bits: data.get().to_bits(),
    })
}

unsafe fn parse_reload_handlers(options: f64) -> Option<Handlers> {
    let roots = perry_ffi::TransientRootScope::enter();
    let options = roots.root_nanbox(options);
    let nested = get_object_value_field(options.get(), "socket").unwrap_or(options.get());
    let handler = roots.root_nanbox(nested);
    let names = [
        "open",
        "data",
        "drain",
        "close",
        "error",
        "connectError",
        "end",
    ];
    let callbacks = names
        .iter()
        .map(|name| {
            roots.root_nanbox(get_object_value_field(handler.get(), name).unwrap_or_else(undefined))
        })
        .collect::<Vec<_>>();
    let ptr = |index: usize| callback_pointer(callbacks[index].get());
    Some(Handlers {
        open: ptr(0),
        data: ptr(1),
        drain: ptr(2),
        close: ptr(3),
        error: ptr(4),
        connect_error: ptr(5),
        end: ptr(6),
    })
}

fn nanbox_string(value: &str) -> f64 {
    let string = alloc_string(value).as_raw();
    f64::from_bits(0x7FFF_0000_0000_0000 | (string as u64 & POINTER_MASK))
}

fn dispatch_one(callback: i64, socket: i64) {
    if callback == 0 {
        return;
    }
    let frame = crate::dispatch_custody::DispatchFrame::park(vec![callback]);
    unsafe {
        let _ =
            JsClosure::from_raw(frame.cb(0) as *const RawClosureHeader).call1(handle_value(socket));
    }
}

fn dispatch_two(callback: i64, socket: i64, payload: f64) {
    if callback == 0 {
        return;
    }
    let mut frame = crate::dispatch_custody::DispatchFrame::park(vec![callback]);
    frame.set_payload(payload.to_bits());
    unsafe {
        let _ = JsClosure::from_raw(frame.cb(0) as *const RawClosureHeader)
            .call2(handle_value(socket), f64::from_bits(frame.payload_bits()));
    }
}

fn dispatch_error(callback: i64, socket: i64, message: &str) {
    if callback == 0 {
        return;
    }
    let mut frame = crate::dispatch_custody::DispatchFrame::park(vec![callback]);
    frame.set_payload(unsafe { build_error_object(message) }.to_bits());
    unsafe {
        let _ = JsClosure::from_raw(frame.cb(0) as *const RawClosureHeader)
            .call2(handle_value(socket), f64::from_bits(frame.payload_bits()));
    }
}

fn settle_connect(handle: i64, error: Option<&str>) {
    let promise = sockets()
        .lock()
        .unwrap()
        .get_mut(&handle)
        .map(|socket| std::mem::take(&mut socket.connect_promise))
        .unwrap_or(0);
    if promise == 0 {
        return;
    }
    let roots = perry_ffi::TransientRootScope::enter();
    let promise = roots.root_addr(promise as i64);
    unsafe {
        if let Some(message) = error {
            let reason = roots.root_nanbox(build_error_object(message));
            JsPromise::from_raw(promise.get() as *mut Promise)
                .reject(JsValue::from_bits(reason.get().to_bits()));
        } else {
            JsPromise::from_raw(promise.get() as *mut Promise)
                .resolve(JsValue::from_bits(handle_value(handle).to_bits()));
        }
    }
}

/// Install Bun's runtime dispatch bucket together with the ext-net callback
/// used by extracted `listen`/`connect` exports. The generated import path
/// calls this wrapper before it can mint a callable export, so registration
/// does not depend on the event loop having initialized perry-stdlib yet.
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_nm_install() {
    extern "C" {
        fn js_nm_install_bun();
    }

    crate::dispatch::ensure_runtime_dispatch_registered();
    js_nm_install_bun();
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_listen(options: f64) -> i64 {
    crate::ensure_gc_scanner_registered();
    crate::dispatch::ensure_runtime_dispatch_registered();
    let roots = perry_ffi::TransientRootScope::enter();
    let options = roots.root_nanbox(options);
    let handle = crate::js_ext_net_create_server(0, 0);
    let parsed = match parse_options(options.get(), true) {
        Some(parsed) => parsed,
        None => {
            crate::server_state::remove_server(handle);
            statics::servers().lock().unwrap().remove(&handle);
            statics::listeners().lock().unwrap().remove(&handle);
            return 0;
        }
    };
    servers().lock().unwrap().insert(
        handle,
        BunServer {
            handlers: parsed.handlers,
            data_bits: parsed.data_bits,
            refed: true,
            ready: false,
        },
    );

    match parsed.endpoint {
        Endpoint::Tcp { host, port } => {
            crate::js_net_server_listen(handle, port, nanbox_string(&host), undefined());
        }
        Endpoint::Unix(path) => {
            crate::js_net_server_listen(handle, nanbox_string(&path), undefined(), undefined());
        }
    }

    // Bun.listen binds before returning, which makes `listen({ port: 0 }).port`
    // immediately usable by a client. Perry's node:net bind is asynchronous;
    // drive that shared task/pump just until its ServerListening event arrives.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if servers()
            .lock()
            .unwrap()
            .get(&handle)
            .map(|server| server.ready)
            .unwrap_or(true)
        {
            break;
        }
        perry_ffi::run_pending(2);
        crate::js_ext_net_drain_pending();
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_connect(options: f64) -> *mut Promise {
    crate::ensure_gc_scanner_registered();
    crate::dispatch::ensure_runtime_dispatch_registered();
    let roots = perry_ffi::TransientRootScope::enter();
    let options = roots.root_nanbox(options);
    let handle = crate::js_net_socket_alloc();
    let promise = JsPromise::new();
    let promise = roots.root_addr(promise.as_raw() as i64);
    let parsed = match parse_options(options.get(), false) {
        Some(parsed) => parsed,
        None => {
            statics::sockets().lock().unwrap().remove(&handle);
            statics::listeners().lock().unwrap().remove(&handle);
            let rooted = JsPromise::from_raw(promise.get() as *mut Promise);
            rooted.reject_string("Bun.connect requires socket handlers and a TCP or Unix endpoint");
            return promise.get() as *mut Promise;
        }
    };
    sockets().lock().unwrap().insert(
        handle,
        BunSocket {
            handlers: parsed.handlers,
            data_bits: parsed.data_bits,
            connect_promise: promise.get() as usize,
            listener: None,
            opened: false,
            paused: false,
            paused_data: VecDeque::new(),
            paused_end: false,
            paused_close: false,
            shutting_down: false,
            needs_drain: false,
            last_error: None,
        },
    );
    match parsed.endpoint {
        Endpoint::Tcp { host, port } => {
            crate::js_net_socket_method_connect(handle, port, nanbox_string(&host), undefined());
        }
        Endpoint::Unix(path) => crate::ipc::connect_existing(handle, path),
    }
    promise.get() as *mut Promise
}

/// Indirect runtime bridge for captured `Bun.listen` / `Bun.connect` values.
#[no_mangle]
pub unsafe extern "C" fn js_bun_tcp_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method = if method_ptr.is_null() {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("")
    };
    let options = if args_ptr.is_null() || args_len == 0 {
        undefined()
    } else {
        *args_ptr
    };
    match method {
        "listen" => handle_value(js_bun_tcp_listen(options)),
        "connect" => handle_value(js_bun_tcp_connect(options) as i64),
        _ => undefined(),
    }
}

pub(crate) fn is_socket(handle: i64) -> bool {
    sockets().lock().unwrap().contains_key(&handle)
}

pub(crate) fn is_server(handle: i64) -> bool {
    servers().lock().unwrap().contains_key(&handle)
}

pub(crate) fn server_keeps_alive(handle: i64) -> bool {
    servers()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|server| server.refed)
        .unwrap_or(true)
}

pub(crate) fn on_server_listening(handle: i64) {
    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
        server.ready = true;
    }
}

pub(crate) fn on_server_close(handle: i64) {
    servers().lock().unwrap().remove(&handle);
}

pub(crate) fn on_accept(server_id: i64, socket_id: i64) -> bool {
    let (handlers, data_bits) = match servers().lock().unwrap().get(&server_id) {
        Some(server) => (server.handlers.clone(), server.data_bits),
        None => return false,
    };
    let open = handlers.open;
    sockets().lock().unwrap().insert(
        socket_id,
        BunSocket {
            handlers,
            data_bits,
            connect_promise: 0,
            listener: Some(server_id),
            opened: true,
            paused: false,
            paused_data: VecDeque::new(),
            paused_end: false,
            paused_close: false,
            shutting_down: false,
            needs_drain: false,
            last_error: None,
        },
    );
    dispatch_one(open, socket_id);
    true
}

pub(crate) fn on_connect(handle: i64) -> bool {
    let callback = {
        let mut sockets = sockets().lock().unwrap();
        let Some(socket) = sockets.get_mut(&handle) else {
            return false;
        };
        socket.opened = true;
        socket.handlers.open
    };
    dispatch_one(callback, handle);
    settle_connect(handle, None);
    true
}

fn dispatch_data(handle: i64, bytes: &Bytes) {
    let callback = sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| socket.handlers.data)
        .unwrap_or(0);
    if callback == 0 {
        return;
    }
    let frame = crate::dispatch_custody::DispatchFrame::park(vec![callback]);
    let buffer = alloc_buffer(bytes);
    if buffer.is_null() {
        return;
    }
    let mut frame = frame;
    frame.set_payload(POINTER_TAG | (buffer as u64 & POINTER_MASK));
    unsafe {
        let _ = JsClosure::from_raw(frame.cb(0) as *const RawClosureHeader)
            .call2(handle_value(handle), f64::from_bits(frame.payload_bits()));
    }
}

pub(crate) fn on_data(handle: i64, bytes: &Bytes) -> bool {
    {
        let mut sockets = sockets().lock().unwrap();
        let Some(socket) = sockets.get_mut(&handle) else {
            return false;
        };
        if socket.paused {
            socket.paused_data.push_back(bytes.clone());
            return true;
        }
    }
    dispatch_data(handle, bytes);
    true
}

pub(crate) fn on_end(handle: i64) -> bool {
    let callback = match sockets().lock().unwrap().get_mut(&handle) {
        Some(socket) if socket.paused => {
            socket.paused_end = true;
            return true;
        }
        Some(socket) => socket.handlers.end,
        None => return false,
    };
    dispatch_one(callback, handle);
    true
}

pub(crate) fn on_error(handle: i64, message: &str) -> bool {
    let (callback, opened) = {
        let mut sockets = sockets().lock().unwrap();
        let Some(socket) = sockets.get_mut(&handle) else {
            return false;
        };
        socket.last_error = Some(message.to_string());
        let callback = if !socket.opened && socket.handlers.connect_error != 0 {
            socket.handlers.connect_error
        } else {
            socket.handlers.error
        };
        (callback, socket.opened)
    };
    dispatch_error(callback, handle, message);
    if !opened {
        settle_connect(handle, Some(message));
    }
    true
}

pub(crate) fn on_close(handle: i64) -> bool {
    let (callback, error, opened) = match sockets().lock().unwrap().get_mut(&handle) {
        Some(socket) if socket.paused => {
            socket.paused_close = true;
            return true;
        }
        Some(socket) => (
            socket.handlers.close,
            socket.last_error.clone(),
            socket.opened,
        ),
        None => return false,
    };
    if let Some(message) = error.as_deref() {
        dispatch_error(callback, handle, message);
    } else {
        dispatch_two(callback, handle, undefined());
    }
    if !opened {
        settle_connect(
            handle,
            Some(
                error
                    .as_deref()
                    .unwrap_or("Socket closed before connecting"),
            ),
        );
    }
    write_tokens()
        .lock()
        .unwrap()
        .retain(|_, socket| *socket != handle);
    // Net handle ids remain reserved for the lifetime of their JS facade.
    // Keep only the closed socket's user data so late reads still work and
    // write/end can report -1, while releasing callback and buffer roots.
    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
        socket.handlers = Handlers::default();
        socket.connect_promise = 0;
        socket.paused = false;
        socket.paused_data.clear();
        socket.paused_end = false;
        socket.paused_close = false;
        socket.shutting_down = true;
        socket.needs_drain = false;
        socket.last_error = None;
    }
    true
}

fn number_arg(value: f64) -> Option<usize> {
    let value = JsValue::from_bits(value.to_bits());
    if value.is_number() {
        let number = value.to_number();
        if number.is_finite() && number >= 0.0 {
            return Some(number as usize);
        }
    }
    None
}

unsafe fn socket_write(handle: i64, value: f64, offset: f64, length: f64) -> f64 {
    let js_value = JsValue::from_bits(value.to_bits());
    let is_string = js_value.is_any_string();
    let Some(mut bytes) = crate::jsvalue_to_socket_bytes(value) else {
        return -1.0;
    };
    if !is_string {
        let start = number_arg(offset).unwrap_or(0).min(bytes.len());
        let available = bytes.len().saturating_sub(start);
        let count = number_arg(length).unwrap_or(available).min(available);
        bytes = bytes[start..start + count].to_vec();
    }

    let (accepted, token, sender) = {
        let mut net_sockets = statics::sockets().lock().unwrap();
        let Some(socket) = net_sockets.get_mut(&handle) else {
            return -1.0;
        };
        if !socket.is_open || socket.destroyed {
            return -1.0;
        }
        if sockets()
            .lock()
            .unwrap()
            .get(&handle)
            .map(|socket| socket.shutting_down)
            .unwrap_or(true)
        {
            return -1.0;
        }
        let capacity = WRITE_HIGH_WATER_MARK.saturating_sub(socket.bytes_queued as usize);
        let accepted = capacity.min(bytes.len());
        if accepted == 0 {
            if let Some(bun) = sockets().lock().unwrap().get_mut(&handle) {
                bun.needs_drain = true;
            }
            return 0.0;
        }
        static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1 << 63);
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        let sender = socket.cmd_tx.clone();
        socket.bytes_queued = socket.bytes_queued.saturating_add(accepted as u64);
        (accepted, token, sender)
    };
    if accepted < bytes.len() {
        if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
            socket.needs_drain = true;
        }
    }
    write_tokens().lock().unwrap().insert(token, handle);
    if sender
        .send(SocketCommand::Write(bytes[..accepted].to_vec(), token))
        .is_err()
    {
        write_tokens().lock().unwrap().remove(&token);
        if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&handle) {
            socket.bytes_queued = socket.bytes_queued.saturating_sub(accepted as u64);
        }
        return -1.0;
    }
    accepted as f64
}

unsafe fn socket_end(handle: i64, value: f64, offset: f64, length: f64) -> f64 {
    if sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| socket.shutting_down)
        .unwrap_or(true)
    {
        return -1.0;
    }
    let has_data = !JsValue::from_bits(value.to_bits()).is_undefined()
        && !JsValue::from_bits(value.to_bits()).is_null();
    let result = if has_data {
        socket_write(handle, value, offset, length)
    } else {
        if !is_socket(handle) {
            return -1.0;
        }
        0.0
    };
    if result < 0.0 {
        return result;
    }
    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
        socket.shutting_down = true;
    }
    crate::js_ext_net_socket_end(handle, TAG_UNDEFINED as i64);
    result
}

pub(crate) fn on_write_complete(handle: i64, token: u64, succeeded: bool) -> bool {
    if write_tokens().lock().unwrap().remove(&token).is_none() {
        return false;
    }
    if !succeeded {
        return true;
    }
    let queued = statics::sockets()
        .lock()
        .unwrap()
        .get(&handle)
        .map(|socket| socket.bytes_queued as usize)
        .unwrap_or(0);
    let callback = {
        let mut sockets = sockets().lock().unwrap();
        let Some(socket) = sockets.get_mut(&handle) else {
            return true;
        };
        if socket.needs_drain && queued < WRITE_HIGH_WATER_MARK {
            socket.needs_drain = false;
            socket.handlers.drain
        } else {
            0
        }
    };
    dispatch_one(callback, handle);
    true
}

pub(crate) fn method_name(handle: i64, property: &str) -> Option<&'static [u8]> {
    let socket = is_socket(handle);
    let server = !socket && is_server(handle);
    match (socket, server, property) {
        (true, _, "write") => Some(b"write"),
        (true, _, "end") => Some(b"end"),
        (true, _, "close") => Some(b"close"),
        (true, _, "terminate") => Some(b"terminate"),
        (true, _, "ref") => Some(b"ref"),
        (true, _, "unref") => Some(b"unref"),
        (true, _, "pause") => Some(b"pause"),
        (true, _, "resume") => Some(b"resume"),
        (true, _, "flush") => Some(b"flush"),
        (true, _, "reload") => Some(b"reload"),
        (true, _, "shutdown") => Some(b"shutdown"),
        (_, true, "stop") => Some(b"stop"),
        (_, true, "ref") => Some(b"ref"),
        (_, true, "unref") => Some(b"unref"),
        (_, true, "reload") => Some(b"reload"),
        _ => None,
    }
}

pub(crate) unsafe fn dispatch_method(handle: i64, method: &str, args: &[f64]) -> Option<f64> {
    if method_name(handle, method).is_none() {
        return None;
    }
    let arg = |index: usize| args.get(index).copied().unwrap_or_else(undefined);
    let result = if is_socket(handle) {
        match method {
            "write" => socket_write(handle, arg(0), arg(1), arg(2)),
            "end" => socket_end(handle, arg(0), arg(1), arg(2)),
            "close" => {
                let _ = socket_end(handle, undefined(), undefined(), undefined());
                undefined()
            }
            "shutdown" => {
                if JsValue::from_bits(arg(0).to_bits()).to_bool() {
                    let _ = socket_end(handle, undefined(), undefined(), undefined());
                } else {
                    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
                        socket.shutting_down = true;
                    }
                    crate::js_ext_net_destroy_socket(handle);
                }
                undefined()
            }
            "terminate" => {
                if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
                    socket.shutting_down = true;
                }
                crate::js_ext_net_destroy_socket(handle);
                undefined()
            }
            "ref" => {
                crate::js_net_socket_ref(handle);
                undefined()
            }
            "unref" => {
                crate::js_net_socket_unref(handle);
                undefined()
            }
            "pause" => {
                if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
                    socket.paused = true;
                }
                undefined()
            }
            "resume" => {
                let (pending, ended, closed) = sockets()
                    .lock()
                    .unwrap()
                    .get_mut(&handle)
                    .map(|socket| {
                        socket.paused = false;
                        let pending = socket.paused_data.drain(..).collect::<Vec<_>>();
                        let ended = std::mem::take(&mut socket.paused_end);
                        let closed = std::mem::take(&mut socket.paused_close);
                        (pending, ended, closed)
                    })
                    .unwrap_or_default();
                for bytes in pending {
                    crate::push_event(crate::PendingNetEvent::Data(handle, bytes));
                }
                if ended {
                    crate::push_event(crate::PendingNetEvent::End(handle));
                }
                if closed {
                    crate::push_event(crate::PendingNetEvent::Close(handle));
                }
                undefined()
            }
            "reload" => {
                if let Some(handlers) = parse_reload_handlers(arg(0)) {
                    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
                        socket.handlers = handlers;
                    }
                }
                undefined()
            }
            "flush" => undefined(),
            _ => undefined(),
        }
    } else {
        match method {
            "stop" => {
                let close_active = JsValue::from_bits(arg(0).to_bits()).to_bool();
                if close_active {
                    let active = statics::sockets()
                        .lock()
                        .unwrap()
                        .iter()
                        .filter_map(|(id, socket)| {
                            (socket.server_id == Some(handle)).then_some(*id)
                        })
                        .collect::<Vec<_>>();
                    for socket in active {
                        crate::js_ext_net_destroy_socket(socket);
                    }
                }
                crate::js_net_server_close(handle, 0);
                undefined()
            }
            "ref" | "unref" => {
                if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
                    server.refed = method == "ref";
                }
                perry_ffi::notify_main_thread();
                undefined()
            }
            "reload" => {
                if let Some(handlers) = parse_reload_handlers(arg(0)) {
                    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
                        server.handlers = handlers.clone();
                    }
                    for socket in sockets().lock().unwrap().values_mut() {
                        if socket.listener == Some(handle) {
                            socket.handlers = handlers.clone();
                        }
                    }
                }
                undefined()
            }
            _ => undefined(),
        }
    };
    Some(result)
}

pub(crate) fn property(handle: i64, property: &str) -> Option<f64> {
    if is_socket(handle) {
        let (data_bits, listener, shutting_down) = {
            let sockets = sockets().lock().unwrap();
            let socket = sockets.get(&handle)?;
            (socket.data_bits, socket.listener, socket.shutting_down)
        };
        return Some(match property {
            "data" => f64::from_bits(data_bits),
            "listener" => listener
                .filter(|server| is_server(*server))
                .map(handle_value)
                .unwrap_or_else(undefined),
            "remoteAddress" => unsafe { crate::js_net_socket_get_remote_address(handle) },
            "remotePort" => unsafe { crate::js_net_socket_get_remote_port(handle) },
            "remoteFamily" => unsafe { crate::js_net_socket_get_remote_family(handle) },
            "localAddress" => unsafe { crate::js_net_socket_get_local_address(handle) },
            "localPort" => unsafe { crate::js_net_socket_get_local_port(handle) },
            "localFamily" => unsafe { crate::js_net_socket_get_local_family(handle) },
            "bytesWritten" => unsafe { crate::js_net_socket_get_bytes_written(handle) },
            "readyState" => {
                let state = statics::sockets()
                    .lock()
                    .unwrap()
                    .get(&handle)
                    .map(|socket| {
                        if socket.destroyed || !socket.is_open {
                            0.0
                        } else if shutting_down {
                            -2.0
                        } else {
                            1.0
                        }
                    })
                    .unwrap_or(0.0);
                state
            }
            _ => return None,
        });
    }
    if !is_server(handle) {
        return None;
    }
    let data_bits = servers().lock().unwrap().get(&handle)?.data_bits;
    Some(match property {
        "data" => f64::from_bits(data_bits),
        "port" => statics::servers()
            .lock()
            .unwrap()
            .get(&handle)
            .map(|server| server.bound_port as f64)
            .unwrap_or(0.0),
        "hostname" => {
            let host = statics::servers()
                .lock()
                .unwrap()
                .get(&handle)
                .map(|server| server.bound_host.clone())
                .unwrap_or_default();
            nanbox_string(&host)
        }
        "unix" => {
            let path = statics::servers()
                .lock()
                .unwrap()
                .get(&handle)
                .and_then(|server| server.bound_path.clone());
            path.as_deref().map(nanbox_string).unwrap_or_else(undefined)
        }
        _ => return None,
    })
}

pub(crate) fn set_property(handle: i64, property: &str, value: f64) -> bool {
    if property != "data" {
        return false;
    }
    if let Some(socket) = sockets().lock().unwrap().get_mut(&handle) {
        socket.data_bits = value.to_bits();
        return true;
    }
    if let Some(server) = servers().lock().unwrap().get_mut(&handle) {
        server.data_bits = value.to_bits();
        return true;
    }
    false
}

fn scan_handlers(handlers: &mut Handlers, visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    visitor.visit_i64_slot(&mut handlers.open);
    visitor.visit_i64_slot(&mut handlers.data);
    visitor.visit_i64_slot(&mut handlers.drain);
    visitor.visit_i64_slot(&mut handlers.close);
    visitor.visit_i64_slot(&mut handlers.error);
    visitor.visit_i64_slot(&mut handlers.connect_error);
    visitor.visit_i64_slot(&mut handlers.end);
}

pub(crate) fn scan_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    if let Ok(mut sockets) = sockets().lock() {
        for socket in sockets.values_mut() {
            scan_handlers(&mut socket.handlers, visitor);
            visitor.visit_nanbox_u64_slot(&mut socket.data_bits);
            if socket.connect_promise != 0 {
                let mut promise = socket.connect_promise as *mut Promise;
                visitor.visit_raw_mut_ptr_slot(&mut promise);
                socket.connect_promise = promise as usize;
            }
        }
    }
    if let Ok(mut servers) = servers().lock() {
        for server in servers.values_mut() {
            scan_handlers(&mut server.handlers, visitor);
            visitor.visit_nanbox_u64_slot(&mut server.data_bits);
        }
    }
}

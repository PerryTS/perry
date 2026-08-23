//! `http.Agent` / `https.Agent` (#2129 / #2154).
//!
//! The first PR (#2157) shipped the Agent surface as a metadata-only
//! shim: a constructible class with a real `getName(options)` and
//! chainable no-op methods. This module extends that surface with the
//! pieces #2154 calls out:
//!
//! - **Argument validation** — `new Agent({ maxSockets: -1 })` and
//!   friends now throw `RangeError [ERR_OUT_OF_RANGE]` with Node's
//!   exact message shape. Closes `test-http-agent-maxtotalsockets.js`.
//! - **`sockets` / `freeSockets` / `requests` accessors** — expose the
//!   per-origin active, idle, and queued counts used by maxSockets admission.
//! - **Per-agent reqwest client** — `options.agent = new Agent({...})`
//!   now actually routes requests through a per-agent `reqwest::Client`
//!   whose connection pool honors the agent's `keepAlive` /
//!   `maxFreeSockets` / `keepAliveMsecs` configuration, instead of
//!   ignoring the agent and reusing the global `HTTP_CLIENT` every time.
//! - **Tunable property setters** — `agent.maxSockets = 4` writes the
//!   new value (with the same validation as the constructor) instead
//!   of being silently dropped.
//! - **`createConnection` / `createSocket` setter storage** — the
//!   user-supplied closure is stored and GC-rooted on the agent so later
//!   code reading `agent.createConnection` gets back the same function.
//! - **`createConnection` / `createSocket` request-path invocation** — when
//!   `http.request` services a request whose agent defines a
//!   `createConnection` or `createSocket` override, the override is invoked
//!   (on the main thread) to produce a `net.Socket`, and the HTTP/1.1 exchange
//!   is driven over that socket via the raw-net bridge (`perry_ffi::raw_net`,
//!   published by perry-ext-net) instead of reqwest. `createConnection`
//!   returns the socket synchronously (see `try_create_connection_socket`
//!   here); `createSocket(req, options, cb)` follows Node's
//!   `Agent.prototype.addRequest` contract and delivers the socket via its
//!   `cb(err, socket)` callback (see `invoke_create_socket` +
//!   `http_create_socket_cb` in `lib.rs`). `createSocket` takes precedence
//!   when both are set, mirroring Node. This is Node's full socket-injection
//!   behavior (#2154).
//!
//! The implementation is mirrored in `crates/perry-stdlib/src/http.rs`
//! so the default `full` build (perry-stdlib's `http-client` feature)
//! and the well-known-flip build (this crate) expose the same surface.

use crate::ensure_gc_scanner_registered;
use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handles_of_mut, register_handle, ErrorKind,
    GcRootVisitor, Handle, JsClosure, JsString, JsValue, ObjectHeader, RawClosureHeader,
    StringHeader,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, Once};

mod tls_compat;
pub(crate) use tls_compat::{
    client_for_agent_tls, emit_client_keylog, invalidate_tls_sessions_for_server_port,
    merge_tls_defaults, parsed_pfx_identity, request_key_from_options, resolve_https_agent_handle,
    tls_session_for_request,
};
use tls_compat::{emit_default_https_agent, sync_default_https_agent};

const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

extern "C" {
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
}

fn bool_f64(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

fn bind_agent_method(handle: Handle, name: &'static [u8]) -> i64 {
    (bind_agent_method_value(handle, name).to_bits() & PTR_MASK) as i64
}

fn handle_value(handle: Handle) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

fn bind_agent_method_value(handle: Handle, name: &'static [u8]) -> f64 {
    let instance = handle_value(handle);
    unsafe { js_class_method_bind(instance, name.as_ptr(), name.len()) }
}

// ------------------------------------------------------------------
// AgentHandle
// ------------------------------------------------------------------

/// `http.Agent` / `https.Agent` instance state.
///
/// Tracker: #2129 (initial constructor + getName); #2154 (validation +
/// per-agent reqwest client + socket-counter accessors + setters).
pub struct AgentHandle {
    pub protocol: Option<String>,
    pub keep_alive: bool,
    pub keep_alive_msecs: f64,
    /// Milliseconds subtracted from a server's advertised keep-alive timeout
    /// before an idle socket is reused. Node retains finite, non-negative
    /// constructor values and falls back to 1_000 for every other value.
    pub agent_keep_alive_timeout_buffer: f64,
    pub max_sockets: f64,
    pub max_total_sockets: f64,
    pub max_free_sockets: f64,
    pub scheduling: String,
    pub timeout_ms: Option<f64>,
    /// Set by `agent.destroy()` — recorded so the accessors that mirror
    /// Node's `destroyed` getter can return true. The actual pool teardown
    /// is implicit (the reqwest::Client gets dropped when the handle is
    /// dropped).
    pub destroyed: bool,
    /// User-supplied `createConnection` override closure pointer. Stored and
    /// GC-rooted; the request path invokes it before falling back to reqwest.
    pub create_connection: i64,
    /// User-supplied `createSocket` override closure pointer (same notes
    /// as `create_connection`).
    pub create_socket: i64,
    /// Active sockets per host:port key (= `getName(options)`).
    /// Incremented at dispatch, decremented when the response or error
    /// pump fires on the main thread.
    pub sockets: HashMap<String, u32>,
    /// Idle keep-alive connection count per host. reqwest owns the actual
    /// transport sockets; this mirrors their Agent-visible lifecycle.
    pub free_sockets: HashMap<String, u32>,
    /// Queued request count per host, mirrored by `queued_requests` below.
    pub requests: HashMap<String, u32>,
    /// Request handles waiting for a per-origin `maxSockets` slot. Kept
    /// separately from the public counts so the next request can be resumed
    /// when the active response reaches a terminal edge.
    pub queued_requests: HashMap<String, Vec<Handle>>,
    /// Stable public socket handles backing the count mirrors above. The HTTP
    /// transport remains owned by reqwest; these alloc-only net.Socket handles
    /// provide Node's observable Agent/ClientRequest socket identity and
    /// EventEmitter lifecycle.
    pub active_socket_handles: HashMap<String, Vec<Handle>>,
    pub free_socket_handles: HashMap<String, Vec<Handle>>,
    /// Generation of each current free-pool admission. Idle invalidation is
    /// asynchronous, so a generation prevents an old guard from destroying a
    /// socket that has since been reused and returned to the pool again.
    pub free_socket_generations: HashMap<Handle, u64>,
    pub next_free_socket_generation: u64,
    /// Synthetic public session identity layered over rustls' real cached
    /// sessions. Node exposes opaque session bytes through `TLSSocket`, while
    /// reqwest intentionally hides them; this mirror preserves the cache and
    /// eviction semantics without exposing backend internals.
    pub max_cached_sessions: usize,
    pub tls_sessions: HashMap<String, u64>,
    pub tls_session_order: VecDeque<String>,
    pub next_tls_session: u64,
    /// HTTPS options supplied to the Agent constructor. Request-local
    /// options override these before verifier/session selection.
    pub(crate) tls_defaults: crate::tls_client::TlsOptions,
}

impl Default for AgentHandle {
    fn default() -> Self {
        AgentHandle {
            protocol: Some("http:".to_string()),
            keep_alive: false,
            keep_alive_msecs: 1000.0,
            agent_keep_alive_timeout_buffer: 1000.0,
            max_sockets: f64::INFINITY,
            max_total_sockets: f64::INFINITY,
            max_free_sockets: 256.0,
            scheduling: "lifo".to_string(),
            timeout_ms: None,
            destroyed: false,
            create_connection: 0,
            create_socket: 0,
            sockets: HashMap::new(),
            free_sockets: HashMap::new(),
            requests: HashMap::new(),
            queued_requests: HashMap::new(),
            active_socket_handles: HashMap::new(),
            free_socket_handles: HashMap::new(),
            free_socket_generations: HashMap::new(),
            next_free_socket_generation: 0,
            max_cached_sessions: 100,
            tls_sessions: HashMap::new(),
            tls_session_order: VecDeque::new(),
            next_tls_session: 0,
            tls_defaults: crate::tls_client::TlsOptions::default(),
        }
    }
}

// SAFETY: closure pointers point into program-global code; the GC
// scanner pins them.
unsafe impl Send for AgentHandle {}
unsafe impl Sync for AgentHandle {}

// ------------------------------------------------------------------
// Per-agent reqwest client cache
// ------------------------------------------------------------------
//
// Keyed by agent handle id (the i64 perry_ffi::register_handle returns).
// Building a fresh `reqwest::Client` per request would defeat the
// purpose of an Agent — the whole point is connection pooling — so we
// memoize one client per agent and feed its `keepAlive` /
// `maxFreeSockets` / `keepAliveMsecs` settings into reqwest's pool
// config. When the Agent handle is dropped the cache entry leaks
// (clients self-trim via `pool_idle_timeout`); we don't unregister
// because tracking Agent destruction would mean adding a finalizer
// hook to the handle registry, which today's perry-ffi handle API
// doesn't expose.

lazy_static! {
    static ref AGENT_CLIENTS: Mutex<HashMap<Handle, reqwest::Client>> = Mutex::new(HashMap::new());
}

/// Build (or fetch the cached) `reqwest::Client` for `handle`. Falls back
/// to the global client if the handle is missing or the per-agent client
/// fails to build. Inspects this agent's `keepAlive` / `maxFreeSockets` /
/// `keepAliveMsecs` to derive `pool_max_idle_per_host` +
/// `pool_idle_timeout`.
pub(crate) fn client_for_agent(handle: Handle) -> reqwest::Client {
    {
        let cache = AGENT_CLIENTS.lock().unwrap();
        if let Some(c) = cache.get(&handle) {
            return c.clone();
        }
    }
    let (keep_alive, max_free_sockets, keep_alive_msecs) = get_handle_mut::<AgentHandle>(handle)
        .map(|a| (a.keep_alive, a.max_free_sockets, a.keep_alive_msecs))
        .unwrap_or((false, 256.0, 1000.0));

    let pool_max_idle = if keep_alive {
        // f64 → usize: clamp Infinity, NaN, negatives to a sane upper.
        if !max_free_sockets.is_finite() || max_free_sockets > usize::MAX as f64 {
            256
        } else {
            max_free_sockets.max(1.0) as usize
        }
    } else {
        0
    };

    let idle_timeout = if keep_alive {
        let ms = if keep_alive_msecs.is_finite() && keep_alive_msecs > 0.0 {
            keep_alive_msecs
        } else {
            1000.0
        };
        std::time::Duration::from_millis(ms as u64)
    } else {
        // `Duration::ZERO` would still let reqwest stash one connection
        // before noticing it's expired; explicit short window prevents
        // any keep-alive when the agent has `keepAlive: false`.
        std::time::Duration::from_millis(0)
    };

    let built = crate::apply_node_proxy_policy(
        reqwest::Client::builder()
            .pool_max_idle_per_host(pool_max_idle)
            .pool_idle_timeout(idle_timeout)
            .tcp_keepalive(std::time::Duration::from_secs(60)),
    )
    .build()
    .unwrap_or_else(|_| crate::default_client());

    let mut cache = AGENT_CLIENTS.lock().unwrap();
    cache.entry(handle).or_insert(built).clone()
}

/// The `(keep_alive, max_free_sockets, keep_alive_msecs)` pool config for
/// `handle`, or `None` when the handle isn't a live AgentHandle. Used by
/// the #4906 TLS-customized client path, which builds its own
/// `reqwest::Client` (bypassing the per-agent cache) but still folds in
/// the Agent's pool settings.
pub(crate) fn agent_pool_config(handle: Handle) -> Option<(bool, f64, f64)> {
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| (a.keep_alive, a.max_free_sockets, a.keep_alive_msecs))
}

/// Drop the cached client for `handle` so the next dispatch rebuilds it
/// with the new pool config. Used by the `__set_keepAlive` /
/// `__set_maxFreeSockets` / `__set_keepAliveMsecs` setters.
fn invalidate_agent_client(handle: Handle) {
    let _ = AGENT_CLIENTS.lock().map(|mut c| c.remove(&handle));
    tls_compat::invalidate_tls_client_cache(handle);
}

// ------------------------------------------------------------------
// Validation
// ------------------------------------------------------------------

/// Throw `RangeError [ERR_OUT_OF_RANGE]` with Node's exact message
/// shape. Reaches into perry-runtime directly — ext-http already depends
/// on it for the GC scanner registration, so no new dep.
fn throw_out_of_range(name: &str, bound: &str, received: f64) -> ! {
    let received_str = format_received_number(received);
    let message = format!(
        "The value of \"{}\" is out of range. It must be {}. Received {}",
        name, bound, received_str
    );
    perry_ffi::throw_with_code(&message, "ERR_OUT_OF_RANGE", ErrorKind::RangeError)
}

fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// Validate that a `> 0` numeric option (e.g. `maxSockets`) is sane.
/// Mirrors Node's `if (value <= 0) throw ERR_OUT_OF_RANGE(...)` for the
/// Agent option group.
fn validate_positive(name: &str, value: f64) {
    if value.is_nan() || value <= 0.0 {
        throw_out_of_range(name, "> 0", value);
    }
}

fn normalize_agent_keep_alive_timeout_buffer(value: Option<f64>) -> f64 {
    match value {
        Some(value) if value.is_finite() && value >= 0.0 => value,
        _ => 1000.0,
    }
}

// ------------------------------------------------------------------
// Object-field helpers (NaN-boxed reads from the options object)
// ------------------------------------------------------------------
//
// `parse_options_object` in lib.rs goes through `json_stringify`, which
// throws away pointer-tagged fields (the Agent handle stored as
// `options.agent` doesn't survive a JSON round-trip). The helpers below
// read the raw NaN-boxed object fields so we can extract the agent
// handle id and the callback closure pointers without losing tags.

/// Read a field's raw bits from a NaN-boxed options object via
/// perry-runtime's name-keyed getter, returning `None` when the value
/// is missing / undefined / null. Uses perry-runtime's `JSValue` /
/// `ObjectHeader` types (not perry-ffi's) — they're a separate type
/// universe so we can't mix them on the `js_object_get_field_by_name`
/// boundary.
unsafe fn read_field_bits(obj_f64: f64, field: &str) -> Option<u64> {
    let value = perry_ffi::object_field_by_name(JsValue::from_bits(obj_f64.to_bits()), field);
    if value.is_undefined() || value.is_null() {
        None
    } else {
        Some(value.bits())
    }
}

unsafe fn raw_object_ptr_is_null(value: f64) -> bool {
    !JsValue::from_bits(value.to_bits()).is_pointer_or_raw()
}

unsafe fn read_number_field(obj_f64: f64, field: &str) -> Option<f64> {
    let bits = read_field_bits(obj_f64, field)?;
    let val = JsValue::from_bits(bits);
    if val.is_number() {
        Some(val.to_number())
    } else if val.is_int32() {
        Some(val.to_int32() as f64)
    } else {
        None
    }
}

unsafe fn read_bool_field(obj_f64: f64, field: &str) -> Option<bool> {
    let bits = read_field_bits(obj_f64, field)?;
    let val = JsValue::from_bits(bits);
    if val.is_bool() {
        Some(val.to_bool())
    } else {
        None
    }
}

unsafe fn read_string_field(obj_f64: f64, field: &str) -> Option<String> {
    let bits = read_field_bits(obj_f64, field)?;
    let val = JsValue::from_bits(bits);
    if !val.is_string() {
        return None;
    }
    let ptr = val.as_string_ptr();
    if ptr.is_null() {
        return None;
    }
    let js = JsString::from_raw(ptr);
    perry_ffi::read_string(js).map(String::from)
}

extern "C" fn agent_connection_abort_listener(closure: *const RawClosureHeader) -> f64 {
    let socket = unsafe { perry_ffi::closure_capture_f64(closure, 0) } as i64;
    let signal_value = unsafe { perry_ffi::closure_capture_f64(closure, 1) };
    unsafe {
        extern "C" {
            fn js_abort_signal_resolve_ptr(value: f64) -> *mut ObjectHeader;
            fn js_abort_signal_remove_listener(
                signal: *mut ObjectHeader,
                event_type: f64,
                listener: f64,
            );
        }
        let scope = perry_ffi::TransientRootScope::enter();
        let signal_value = scope.root_nanbox(signal_value);
        let listener_value =
            scope.root_nanbox(f64::from_bits(POINTER_TAG | (closure as u64 & PTR_MASK)));
        let event = scope.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(alloc_string("abort").as_raw()).bits(),
        ));
        let signal = js_abort_signal_resolve_ptr(signal_value.get());
        if !signal.is_null() {
            js_abort_signal_remove_listener(signal, event.get(), listener_value.get());
        }
    }
    perry_ext_net::js_ext_net_socket_emit_abort_error(socket);
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn install_connection_abort_signal(options: f64, socket: Handle) {
    let Some(signal_bits) = read_field_bits(options, "signal") else {
        return;
    };
    let signal_value = f64::from_bits(signal_bits);
    extern "C" {
        fn js_abort_signal_resolve_ptr(value: f64) -> *mut ObjectHeader;
        fn js_abort_signal_is_aborted(signal: *mut ObjectHeader) -> i32;
        fn js_abort_signal_add_listener(signal: *mut ObjectHeader, event_type: f64, listener: f64);
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let signal_value = scope.root_nanbox(signal_value);
    let signal = js_abort_signal_resolve_ptr(signal_value.get());
    if signal.is_null() {
        return;
    }
    if js_abort_signal_is_aborted(signal) != 0 {
        perry_ext_net::js_ext_net_socket_emit_abort_error(socket);
        return;
    }
    let listener = perry_ffi::alloc_closure(agent_connection_abort_listener as *const u8, 2);
    if listener.is_null() {
        return;
    }
    let listener_value =
        scope.root_nanbox(f64::from_bits(POINTER_TAG | (listener as u64 & PTR_MASK)));
    perry_ffi::set_closure_capture_f64(listener, 0, socket as f64);
    perry_ffi::set_closure_capture_f64(listener, 1, signal_value.get());
    let event = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string("abort").as_raw()).bits(),
    ));
    let signal = js_abort_signal_resolve_ptr(signal_value.get());
    js_abort_signal_add_listener(signal, event.get(), listener_value.get());
}

/// Extract a closure pointer field (e.g. `options.agent.createConnection`)
/// as a raw `i64`. Returns 0 when the field is absent or not a closure.
unsafe fn read_closure_field(obj_f64: f64, field: &str) -> i64 {
    let bits = match read_field_bits(obj_f64, field) {
        Some(b) => b,
        None => return 0,
    };
    let upper = bits >> 48;
    if upper == 0x7FFD {
        (bits & PTR_MASK) as i64
    } else if upper == 0 && bits >= 0x10000 {
        bits as i64
    } else {
        0
    }
}

/// Extract an `options.agent` handle from `options_f64`. Returns `None`
/// when the field is missing, not a pointer, or doesn't resolve to an
/// AgentHandle.
pub(crate) unsafe fn agent_handle_from_options(options_f64: f64) -> Option<Handle> {
    let bits = read_field_bits(options_f64, "agent")?;
    let upper = bits >> 48;
    let candidate = if upper == 0x7FFD {
        (bits & PTR_MASK) as Handle
    } else if upper == 0 && bits >= 0x10000 {
        bits as Handle
    } else {
        return None;
    };
    if get_handle_mut::<AgentHandle>(candidate).is_some() {
        Some(candidate)
    } else {
        None
    }
}

// ------------------------------------------------------------------
// Empty-object accessor helper (for sockets / freeSockets / requests)
// ------------------------------------------------------------------

fn empty_object_f64() -> f64 {
    let (packed, shape_id) = perry_ffi::build_object_shape(&[]);
    let obj = unsafe {
        perry_ffi::js_object_alloc_with_shape(shape_id, 0, packed.as_ptr(), packed.len() as u32)
    };
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let v = JsValue::from_object_ptr(obj as *mut u8);
    f64::from_bits(v.bits())
}

fn handle_map_object_f64(values: &HashMap<String, Vec<Handle>>) -> f64 {
    let mut entries: Vec<(&str, &Vec<Handle>)> = values
        .iter()
        .filter(|(_, handles)| !handles.is_empty())
        .map(|(key, handles)| (key.as_str(), handles))
        .collect();
    entries.sort_by_key(|(key, _)| *key);
    if entries.is_empty() {
        return empty_object_f64();
    }

    let keys: Vec<&str> = entries.iter().map(|(key, _)| *key).collect();
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let object = unsafe {
        perry_ffi::js_object_alloc_with_shape(
            shape_id,
            keys.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    if object.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let object = scope.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(object as *mut u8).bits(),
    ));
    for (index, (_, handles)) in entries.iter().enumerate() {
        let mut array = scope.root_nanbox(f64::from_bits(
            JsValue::from_object_ptr(
                unsafe { perry_ffi::js_array_alloc(handles.len() as u32) } as *mut u8
            )
            .bits(),
        ));
        for handle in handles.iter().copied() {
            let current =
                JsValue::from_bits(array.get().to_bits()).as_pointer::<perry_ffi::ArrayHeader>();
            let pushed = unsafe {
                perry_ffi::js_array_push(
                    current,
                    JsValue::from_bits(handle_value(handle).to_bits()),
                )
            };
            array = scope.root_nanbox(f64::from_bits(
                JsValue::from_object_ptr(pushed as *mut u8).bits(),
            ));
        }
        let object_ptr = JsValue::from_bits(object.get().to_bits()).as_pointer::<ObjectHeader>();
        unsafe {
            perry_ffi::js_object_set_field(
                object_ptr,
                index as u32,
                JsValue::from_bits(array.get().to_bits()),
            );
        }
    }
    object.get()
}

pub(crate) fn allocate_agent_socket() -> Handle {
    ensure_agent_socket_hook_registered();
    let socket = unsafe { perry_ext_net::js_net_socket_alloc() };
    perry_ext_net::js_ext_net_set_http_agent_phase(socket, 1);
    socket
}

extern "C" fn agent_socket_event(handle: Handle, event_ptr: *const u8, event_len: usize) {
    if event_ptr.is_null() || event_len == 0 {
        return;
    }
    let event = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(event_ptr, event_len)).unwrap_or("")
    };
    if !matches!(event, "error" | "close") {
        return;
    }
    iter_handles_of_mut::<AgentHandle, _>(|agent| {
        for (key, handles) in &mut agent.active_socket_handles {
            let before = handles.len();
            handles.retain(|socket| *socket != handle);
            if handles.len() != before {
                agent.sockets.insert(key.clone(), handles.len() as u32);
            }
        }
        for (key, handles) in &mut agent.free_socket_handles {
            let before = handles.len();
            handles.retain(|socket| *socket != handle);
            if handles.len() != before {
                agent.free_sockets.insert(key.clone(), handles.len() as u32);
            }
        }
        agent.sockets.retain(|_, count| *count > 0);
        agent.free_sockets.retain(|_, count| *count > 0);
        agent
            .active_socket_handles
            .retain(|_, sockets| !sockets.is_empty());
        agent
            .free_socket_handles
            .retain(|_, sockets| !sockets.is_empty());
        agent.free_socket_generations.remove(&handle);
    });
    tls_compat::sync_default_https_agent_if_initialized();
}

fn ensure_agent_socket_hook_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        perry_ext_net::js_ext_net_register_http_agent_socket_event_hook(agent_socket_event);
    });
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PoolAdmission {
    Active { reused: bool, socket: Handle },
    Queued,
}

/// Convert a request URL into Node's per-origin Agent key.
pub(crate) fn request_key(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return "localhost::".to_string();
    };
    let host = parsed.host_str().unwrap_or("localhost");
    let port = parsed
        .port_or_known_default()
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!("{host}:{port}:")
}

/// Acquire an Agent slot or append the request to the per-origin queue.
pub(crate) fn admit_request(handle: Handle, key: &str, request_handle: Handle) -> PoolAdmission {
    let result = admit_request_inner(handle, key, request_handle);
    sync_default_https_agent(handle);
    result
}

fn admit_request_inner(handle: Handle, key: &str, request_handle: Handle) -> PoolAdmission {
    let Some(agent) = get_handle_mut::<AgentHandle>(handle) else {
        return PoolAdmission::Active {
            reused: false,
            socket: 0,
        };
    };

    if let Some(free) = agent.free_sockets.get_mut(key) {
        *free = free.saturating_sub(1);
        if *free == 0 {
            agent.free_sockets.remove(key);
        }
        let socket = agent
            .free_socket_handles
            .get_mut(key)
            .and_then(Vec::pop)
            .unwrap_or_else(allocate_agent_socket);
        agent.free_socket_generations.remove(&socket);
        if agent
            .free_socket_handles
            .get(key)
            .is_some_and(Vec::is_empty)
        {
            agent.free_socket_handles.remove(key);
        }
        perry_ext_net::js_ext_net_set_http_agent_phase(socket, 1);
        agent
            .active_socket_handles
            .entry(key.to_string())
            .or_default()
            .push(socket);
        *agent.sockets.entry(key.to_string()).or_default() += 1;
        return PoolAdmission::Active {
            reused: true,
            socket,
        };
    }

    let active_for_key = agent.sockets.get(key).copied().unwrap_or(0);
    let active_total: u32 = agent.sockets.values().copied().sum();
    if (active_for_key as f64) < agent.max_sockets
        && (active_total as f64) < agent.max_total_sockets
    {
        let socket = allocate_agent_socket();
        agent
            .active_socket_handles
            .entry(key.to_string())
            .or_default()
            .push(socket);
        *agent.sockets.entry(key.to_string()).or_default() += 1;
        return PoolAdmission::Active {
            reused: false,
            socket,
        };
    }

    let queue = agent.queued_requests.entry(key.to_string()).or_default();
    queue.push(request_handle);
    agent.requests.insert(key.to_string(), queue.len() as u32);
    PoolAdmission::Queued
}

/// Release one active slot. A queued request, when present, inherits the slot
/// and is returned for dispatch; otherwise successful keep-alive exchanges
/// become one observable idle pooled socket.
pub(crate) fn release_request(
    handle: Handle,
    key: &str,
    socket: Handle,
    keep_alive: bool,
) -> Option<(Handle, bool, Handle)> {
    let result = release_request_inner(handle, key, socket, keep_alive);
    let became_free = get_handle_mut::<AgentHandle>(handle)
        .and_then(|agent| agent.free_socket_handles.get(key))
        .is_some_and(|sockets| sockets.contains(&socket));
    sync_default_https_agent(handle);
    if became_free {
        emit_default_https_agent(
            handle,
            "free",
            handle_value(socket),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    result
}

fn release_request_inner(
    handle: Handle,
    key: &str,
    socket: Handle,
    keep_alive: bool,
) -> Option<(Handle, bool, Handle)> {
    let agent = get_handle_mut::<AgentHandle>(handle)?;
    if let Some(active) = agent.sockets.get_mut(key) {
        *active = active.saturating_sub(1);
        if *active == 0 {
            agent.sockets.remove(key);
        }
    }
    if let Some(active) = agent.active_socket_handles.get_mut(key) {
        active.retain(|candidate| *candidate != socket);
        if active.is_empty() {
            agent.active_socket_handles.remove(key);
        }
    }

    let next = agent
        .queued_requests
        .get_mut(key)
        .and_then(|queue| (!queue.is_empty()).then(|| queue.remove(0)));
    if let Some(next_handle) = next {
        let remaining = agent.queued_requests.get(key).map(Vec::len).unwrap_or(0);
        if remaining == 0 {
            agent.queued_requests.remove(key);
            agent.requests.remove(key);
        } else {
            agent.requests.insert(key.to_string(), remaining as u32);
        }
        let (next_socket, reused) = if keep_alive && socket != 0 {
            (socket, true)
        } else {
            if socket != 0 {
                perry_ext_net::js_ext_net_destroy_socket(socket);
            }
            (allocate_agent_socket(), false)
        };
        perry_ext_net::js_ext_net_set_http_agent_phase(next_socket, 1);
        agent
            .active_socket_handles
            .entry(key.to_string())
            .or_default()
            .push(next_socket);
        *agent.sockets.entry(key.to_string()).or_default() += 1;
        return Some((next_handle, reused, next_socket));
    }

    if keep_alive && agent.keep_alive && !agent.destroyed && socket != 0 {
        let free = agent.free_sockets.entry(key.to_string()).or_default();
        if (*free as f64) < agent.max_free_sockets.max(0.0) {
            *free += 1;
            perry_ext_net::js_ext_net_set_http_agent_phase(socket, 0);
            agent
                .free_socket_handles
                .entry(key.to_string())
                .or_default()
                .push(socket);
            agent.next_free_socket_generation = agent.next_free_socket_generation.wrapping_add(1);
            let generation = agent.next_free_socket_generation;
            agent.free_socket_generations.insert(socket, generation);
            unsafe {
                let event = alloc_string("free");
                perry_ext_net::js_ext_net_socket_emit(
                    socket,
                    event.as_raw() as i64,
                    std::ptr::null(),
                    0,
                );
            }
            let key = key.to_string();
            perry_ffi::spawn_async(async move {
                // reqwest owns the physical pooled connection, so the public
                // net.Socket facade cannot receive its idle read/EOF edge.
                // Conservatively retire an unclaimed facade after the I/O
                // guard window; immediate/next-tick reuse cancels this via the
                // generation check below.
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                crate::push_event(crate::PendingHttpEvent::AgentIdleExpire {
                    agent_handle: handle,
                    key,
                    socket,
                    generation,
                });
            });
        } else {
            perry_ext_net::js_ext_net_destroy_socket(socket);
        }
    } else if socket != 0 {
        perry_ext_net::js_ext_net_destroy_socket(socket);
    }
    None
}

/// Retire a socket facade only if it is still the exact free-pool admission
/// scheduled by `release_request`. Reuse removes the generation, making stale
/// timers harmless.
pub(crate) fn expire_free_socket(handle: Handle, key: &str, socket: Handle, generation: u64) {
    let Some(agent) = get_handle_mut::<AgentHandle>(handle) else {
        return;
    };
    if agent.free_socket_generations.get(&socket).copied() != Some(generation) {
        return;
    }
    let Some(free) = agent.free_socket_handles.get_mut(key) else {
        agent.free_socket_generations.remove(&socket);
        return;
    };
    let before = free.len();
    free.retain(|candidate| *candidate != socket);
    if free.len() == before {
        agent.free_socket_generations.remove(&socket);
        return;
    }
    agent.free_socket_generations.remove(&socket);
    if free.is_empty() {
        agent.free_socket_handles.remove(key);
        agent.free_sockets.remove(key);
    } else {
        agent
            .free_sockets
            .insert(key.to_string(), free.len() as u32);
    }
    perry_ext_net::js_ext_net_destroy_socket(socket);
    invalidate_agent_client(handle);
    sync_default_https_agent(handle);
}

// ------------------------------------------------------------------
// GC scanner — call from lib.rs's scan_http_roots
// ------------------------------------------------------------------

pub(crate) fn scan_agent_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<AgentHandle, _>(|agent| {
        visitor.visit_nanbox_f64_slot(&mut agent.agent_keep_alive_timeout_buffer);
        if agent.create_connection != 0 {
            visitor.visit_i64_slot(&mut agent.create_connection);
        }
        if agent.create_socket != 0 {
            visitor.visit_i64_slot(&mut agent.create_socket);
        }
        if agent.tls_defaults.check_server_identity_callback != 0 {
            visitor.visit_i64_slot(&mut agent.tls_defaults.check_server_identity_callback);
        }
    });
}

#[no_mangle]
pub extern "C" fn js_ext_http_agent_is_handle(handle: Handle) -> i32 {
    if get_handle::<AgentHandle>(handle).is_some() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http_agent_dispatch_property(
    handle: Handle,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    if property_ptr.is_null() || property_len == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len));
    match property.as_ref() {
        "createConnection" => {
            let raw = js_http_agent_create_connection(handle);
            f64::from_bits(POINTER_TAG | (raw as u64 & PTR_MASK))
        }
        "createSocket" => {
            let raw = js_http_agent_create_socket(handle);
            f64::from_bits(POINTER_TAG | (raw as u64 & PTR_MASK))
        }
        "keepSocketAlive" => bind_agent_method_value(handle, b"keepSocketAlive"),
        "reuseSocket" => bind_agent_method_value(handle, b"reuseSocket"),
        "getName" => bind_agent_method_value(handle, b"getName"),
        "destroy" => bind_agent_method_value(handle, b"destroy"),
        // #4904: data properties — Agents constructed through the dynamic
        // value path (`const { Agent } = require('http'); new Agent(...)`)
        // read these through handle property dispatch rather than the
        // class-filtered native rows.
        "maxSockets" => js_http_agent_max_sockets(handle),
        "maxFreeSockets" => js_http_agent_max_free_sockets(handle),
        "maxTotalSockets" => js_http_agent_max_total_sockets(handle),
        "totalSocketCount" => js_http_agent_total_socket_count(handle),
        "keepAliveMsecs" => js_http_agent_keep_alive_msecs(handle),
        "agentKeepAliveTimeoutBuffer" => js_http_agent_keep_alive_timeout_buffer(handle),
        "keepAlive" => js_http_agent_keep_alive(handle),
        "destroyed" => js_http_agent_destroyed(handle),
        "defaultPort" => js_http_agent_default_port(handle),
        "protocol" => {
            let ptr = js_http_agent_protocol(handle);
            if ptr.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                f64::from_bits(JsValue::from_string_ptr(ptr).bits())
            }
        }
        "sockets" => js_http_agent_sockets(handle),
        "freeSockets" => js_http_agent_free_sockets(handle),
        "requests" => js_http_agent_requests(handle),
        "_sessionCache" => {
            let scope = perry_ffi::TransientRootScope::enter();
            let map = scope.root_nanbox(empty_object_f64());
            let (packed, shape_id) = perry_ffi::build_object_shape(&["map"]);
            let object = perry_ffi::js_object_alloc_with_shape(
                shape_id,
                1,
                packed.as_ptr(),
                packed.len() as u32,
            );
            if object.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                perry_ffi::js_object_set_field(object, 0, JsValue::from_bits(map.get().to_bits()));
                f64::from_bits(JsValue::from_object_ptr(object as *mut u8).bits())
            }
        }
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

/// #4904: property writes on a dynamically-dispatched Agent —
/// `agent.maxSockets = 4` and the `agent.createConnection = fn`
/// monkeypatch pattern Node's own tests use. Returns 1 when claimed.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_agent_dispatch_property_set(
    handle: Handle,
    property_ptr: *const u8,
    property_len: usize,
    value: f64,
) -> i32 {
    if property_ptr.is_null() || property_len == 0 || get_handle::<AgentHandle>(handle).is_none() {
        return 0;
    }
    let property = String::from_utf8_lossy(std::slice::from_raw_parts(property_ptr, property_len));
    match property.as_ref() {
        "maxSockets" => js_http_agent_set_max_sockets(handle, value),
        "maxFreeSockets" => js_http_agent_set_max_free_sockets(handle, value),
        "maxTotalSockets" => js_http_agent_set_max_total_sockets(handle, value),
        "keepAliveMsecs" => js_http_agent_set_keep_alive_msecs(handle, value),
        "agentKeepAliveTimeoutBuffer" => js_http_agent_set_keep_alive_timeout_buffer(handle, value),
        "keepAlive" => js_http_agent_set_keep_alive(handle, value),
        "createConnection" | "createSocket" => {
            let bits = value.to_bits();
            let ptr = if JsValue::from_bits(bits).is_pointer() {
                (bits & PTR_MASK) as i64
            } else {
                0
            };
            if property.as_ref() == "createConnection" {
                js_http_agent_set_create_connection(handle, ptr);
            } else {
                js_http_agent_set_create_socket(handle, ptr);
            }
        }
        _ => return 0,
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_http_agent_dispatch_method(
    handle: Handle,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    if method_ptr.is_null() || method_len == 0 || get_handle_mut::<AgentHandle>(handle).is_none() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let method = String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len));
    match method.as_ref() {
        "getName" => {
            let options = if !args_ptr.is_null() && args_len > 0 {
                *args_ptr
            } else {
                f64::from_bits(TAG_UNDEFINED)
            };
            let ptr = js_http_agent_get_name(handle, options);
            f64::from_bits(JsValue::from_string_ptr(ptr).bits())
        }
        "destroy" => handle_value(js_http_agent_destroy(handle)),
        "keepSocketAlive" | "reuseSocket" => handle_value(js_http_agent_noop_self(handle)),
        // Node's default implementation is `net.createConnection`. Keeping
        // this as a real socket factory matters when user code wraps the
        // method, inspects the options, and delegates with `.call(agent, ...)`.
        "createConnection" => {
            let undefined = f64::from_bits(TAG_UNDEFINED);
            let arg = |index: usize| {
                if !args_ptr.is_null() && index < args_len {
                    *args_ptr.add(index)
                } else {
                    undefined
                }
            };
            let is_https = agent_field(handle, false, |agent| {
                agent.protocol.as_deref() == Some("https:")
            });
            let socket = if is_https {
                perry_ext_net::js_tls_connect(arg(0), arg(1), arg(2), arg(3))
            } else {
                perry_ext_net::js_net_socket_connect(arg(0), arg(1), arg(2))
            };
            install_connection_abort_signal(arg(0), socket);
            handle_value(socket)
        }
        _ => f64::from_bits(TAG_UNDEFINED),
    }
}

// ------------------------------------------------------------------
// Constructor (with validation)
// ------------------------------------------------------------------

unsafe fn agent_new_with_protocol(options_f64: f64, default_protocol: &str) -> Handle {
    ensure_gc_scanner_registered();

    let mut agent = AgentHandle {
        protocol: Some(default_protocol.to_string()),
        ..AgentHandle::default()
    };

    if default_protocol == "https:" {
        agent.tls_defaults = crate::tls_client::parse_tls_options(options_f64);
        agent.tls_defaults.check_server_identity_from_agent =
            agent.tls_defaults.check_server_identity_callback != 0;
    }

    if !raw_object_ptr_is_null(options_f64) {
        // Booleans first so `keepAlive: false` plus `maxSockets: -1`
        // still hits the maxSockets validator (test-suite expects the
        // RangeError to win).
        if let Some(v) = read_bool_field(options_f64, "keepAlive") {
            agent.keep_alive = v;
        }
        if let Some(v) = read_number_field(options_f64, "keepAliveMsecs") {
            if v.is_nan() || v < 0.0 {
                throw_out_of_range("keepAliveMsecs", ">= 0", v);
            }
            agent.keep_alive_msecs = v;
        }
        agent.agent_keep_alive_timeout_buffer = normalize_agent_keep_alive_timeout_buffer(
            read_number_field(options_f64, "agentKeepAliveTimeoutBuffer"),
        );
        if let Some(v) = read_number_field(options_f64, "maxSockets") {
            if !(v.is_infinite() && v.is_sign_positive()) {
                validate_positive("maxSockets", v);
            }
            agent.max_sockets = v;
        }
        if let Some(v) = read_number_field(options_f64, "maxFreeSockets") {
            if !(v.is_infinite() && v.is_sign_positive()) {
                validate_positive("maxFreeSockets", v);
            }
            agent.max_free_sockets = v;
        }
        if let Some(v) = read_number_field(options_f64, "maxTotalSockets") {
            if !(v.is_infinite() && v.is_sign_positive()) {
                validate_positive("maxTotalSockets", v);
            }
            agent.max_total_sockets = v;
        }
        if let Some(v) = read_number_field(options_f64, "timeout") {
            agent.timeout_ms = Some(v);
        }
        if let Some(v) = read_number_field(options_f64, "maxCachedSessions") {
            agent.max_cached_sessions = if v.is_finite() && v > 0.0 {
                v as usize
            } else {
                0
            };
        }
        if let Some(s) = read_string_field(options_f64, "scheduling") {
            // Node throws TypeError [ERR_INVALID_ARG_VALUE] for
            // anything other than "fifo" / "lifo".
            if s != "fifo" && s != "lifo" {
                // Reuse the ERR_OUT_OF_RANGE path — the radar tests don't
                // distinguish ERR_INVALID_ARG_VALUE from ERR_OUT_OF_RANGE
                // here, only that *some* error is thrown synchronously.
                let message = format!(
                    "The argument 'scheduling' must be one of: 'fifo', 'lifo'. Received {:?}",
                    s
                );
                perry_ffi::throw_with_code(&message, "ERR_INVALID_ARG_VALUE", ErrorKind::TypeError)
            }
            agent.scheduling = s;
        }
        // createConnection / createSocket closure storage. GC-rooted via
        // `scan_agent_roots`. Invoking these on the request path needs
        // net.Socket bridging — tracked as a #2154 follow-up.
        let cc = read_closure_field(options_f64, "createConnection");
        if cc != 0 {
            agent.create_connection = cc;
        }
        let cs = read_closure_field(options_f64, "createSocket");
        if cs != 0 {
            agent.create_socket = cs;
        }
    }

    register_handle(agent)
}

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_new(options_f64: f64) -> Handle {
    agent_new_with_protocol(options_f64, "http:")
}

#[no_mangle]
pub unsafe extern "C" fn js_https_agent_new(options_f64: f64) -> Handle {
    agent_new_with_protocol(options_f64, "https:")
}

// ------------------------------------------------------------------
// `agent.getName([options])`
// ------------------------------------------------------------------
//
// PR #2259 brought https.Agent.getName parity (`lib/https.js` appends 20
// extension fields on top of the http base name). This module re-uses
// that logic via `parse_options_object` on a serde_json::Value so the
// truthy/defined coercion matches Node's `lib/_http_agent.js` +
// `lib/https.js` byte-for-byte (the `test-https-agent-getname.js`
// 20-colon empty-options string is the load-bearing fixture).

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_get_name(
    handle: Handle,
    options_f64: f64,
) -> *mut StringHeader {
    let is_https = get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.as_deref().map(|p| p == "https:"))
        .unwrap_or(false);

    let opts = crate::parse_options_object(options_f64);
    let mut name = build_http_agent_name(opts.as_ref());
    if is_https {
        append_https_agent_name_fields(&mut name, opts.as_ref());
        if let Some(identity) = parsed_pfx_identity(opts.as_ref(), options_f64) {
            name.push_str(":pfxid=");
            name.push_str(&identity);
        }
    }
    alloc_string(&name).as_raw()
}

fn build_http_agent_name(opts: Option<&serde_json::Value>) -> String {
    let opts = match opts {
        Some(v) => v,
        None => return "localhost::".to_string(),
    };

    let host = opts
        .get("host")
        .and_then(|v| v.as_str())
        .unwrap_or("localhost");
    let port = opts
        .get("port")
        .map(|v| {
            v.as_str()
                .map(String::from)
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_f64().map(|n| (n as i64).to_string()))
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let local_address = opts
        .get("localAddress")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let mut name = format!("{}:{}:{}", host, port, local_address);

    // Per Node's `lib/_http_agent.js`: family is appended first when it
    // is exactly 4 or 6, then socketPath. Both are independent.
    if let Some(family) = opts.get("family") {
        let f = family.as_i64().unwrap_or(0);
        if f == 4 || f == 6 {
            name.push(':');
            name.push_str(&f.to_string());
        }
    }
    if let Some(socket_path) = opts.get("socketPath").and_then(|v| v.as_str()) {
        name.push(':');
        name.push_str(socket_path);
    }

    name
}

/// Append the 20 extension fields that `lib/https.js`'s Agent.getName
/// adds on top of the http parent. Each field has its own `:` separator
/// regardless of whether the value is present, so an Agent with no
/// options produces 20 trailing colons.
fn append_https_agent_name_fields(name: &mut String, opts: Option<&serde_json::Value>) {
    let opts = match opts {
        Some(v) => v,
        None => {
            for _ in 0..20 {
                name.push(':');
            }
            return;
        }
    };

    let host_str = opts.get("host").and_then(|v| v.as_str()).unwrap_or("");

    let push_truthy_string = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(v) = opts.get(field) {
            if json_value_is_truthy(v) {
                name.push_str(&json_value_to_string(v));
            }
        }
    };
    let push_defined = |name: &mut String, field: &str| {
        name.push(':');
        if let Some(v) = opts.get(field) {
            if !v.is_null() {
                name.push_str(&json_value_to_string(v));
            }
        }
    };

    push_truthy_string(name, "ca");
    push_truthy_string(name, "cert");
    push_truthy_string(name, "clientCertEngine");
    push_truthy_string(name, "ciphers");
    push_truthy_string(name, "key");
    push_truthy_string(name, "pfx");
    push_defined(name, "rejectUnauthorized");

    // servername appears only when truthy AND distinct from host.
    name.push(':');
    if let Some(sn) = opts.get("servername") {
        if json_value_is_truthy(sn) {
            let sn_str = json_value_to_string(sn);
            if sn_str != host_str {
                name.push_str(&sn_str);
            }
        }
    }

    push_truthy_string(name, "minVersion");
    push_truthy_string(name, "maxVersion");
    push_truthy_string(name, "secureProtocol");
    push_truthy_string(name, "crl");
    push_defined(name, "honorCipherOrder");
    push_truthy_string(name, "ecdhCurve");
    push_truthy_string(name, "dhparam");
    push_defined(name, "secureOptions");
    push_truthy_string(name, "sessionIdContext");

    // sigalgs goes through JSON.stringify per Node.
    name.push(':');
    if let Some(v) = opts.get("sigalgs") {
        if json_value_is_truthy(v) {
            name.push_str(&serde_json::to_string(v).unwrap_or_default());
        }
    }

    push_truthy_string(name, "privateKeyIdentifier");
    push_truthy_string(name, "privateKeyEngine");
}

fn json_value_is_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => {
            n.as_f64().map(|f| f != 0.0 && !f.is_nan()).unwrap_or(false)
        }
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => {
            if *b {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .map(json_value_to_string)
            .collect::<Vec<_>>()
            .join(","),
        serde_json::Value::Object(map) => {
            if let (Some(serde_json::Value::String(ty)), Some(serde_json::Value::Array(data))) =
                (map.get("type"), map.get("data"))
            {
                if ty == "Buffer" {
                    let bytes: Vec<u8> = data
                        .iter()
                        .filter_map(|el| el.as_u64().map(|n| n as u8))
                        .collect();
                    return String::from_utf8_lossy(&bytes).into_owned();
                }
            }
            "[object Object]".to_string()
        }
    }
}

// ------------------------------------------------------------------
// keepSocketAlive / reuseSocket — chainable no-ops (reqwest owns the
// keep-alive pool, so there is no per-socket hook to forward to);
// destroy is real (drops the cached client below).
// ------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_http_agent_noop_self(handle: Handle) -> Handle {
    unsafe {
        perry_ffi::warn_stub(
            c"http.Agent keepSocketAlive/reuseSocket",
            c"reqwest owns the keep-alive pool; per-socket hooks are no-ops",
            Some(c"#4917"),
        )
    };
    handle
}

/// `agent.destroy()` — flag the agent as destroyed (so the `destroyed`
/// getter returns true) and drop the cached reqwest client (= release
/// its idle pool). Returns the handle for chainability.
#[no_mangle]
pub extern "C" fn js_http_agent_destroy(handle: Handle) -> Handle {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.destroyed = true;
        let sockets = agent
            .active_socket_handles
            .values()
            .chain(agent.free_socket_handles.values())
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        for socket in sockets {
            perry_ext_net::js_ext_net_destroy_socket(socket);
        }
        agent.sockets.clear();
        agent.free_sockets.clear();
        agent.active_socket_handles.clear();
        agent.free_socket_handles.clear();
        agent.free_socket_generations.clear();
        // Node leaves maxSockets waiters queued when `destroy()` tears down
        // the current sockets. The next released slot reconnects and serves
        // the oldest waiter, so retain both the private queue and its public
        // `requests` mirror instead of orphaning those ClientRequests.
    }
    invalidate_agent_client(handle);
    sync_default_https_agent(handle);
    handle
}

// ------------------------------------------------------------------
// Property getters
// ------------------------------------------------------------------

fn agent_field<T, F>(handle: Handle, default: T, f: F) -> T
where
    F: FnOnce(&AgentHandle) -> T,
{
    get_handle_mut::<AgentHandle>(handle)
        .map(|a| f(a))
        .unwrap_or(default)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_sockets(handle: Handle) -> f64 {
    agent_field(handle, f64::INFINITY, |a| a.max_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_free_sockets(handle: Handle) -> f64 {
    agent_field(handle, 256.0, |a| a.max_free_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_max_total_sockets(handle: Handle) -> f64 {
    agent_field(handle, f64::INFINITY, |a| a.max_total_sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_total_socket_count(handle: Handle) -> f64 {
    agent_field(handle, 0usize, |agent| {
        agent
            .active_socket_handles
            .values()
            .map(Vec::len)
            .sum::<usize>()
            + agent
                .free_socket_handles
                .values()
                .map(Vec::len)
                .sum::<usize>()
    }) as f64
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive_msecs(handle: Handle) -> f64 {
    agent_field(handle, 1000.0, |a| a.keep_alive_msecs)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive_timeout_buffer(handle: Handle) -> f64 {
    agent_field(handle, 1000.0, |a| a.agent_keep_alive_timeout_buffer)
}

#[no_mangle]
pub extern "C" fn js_http_agent_keep_alive(handle: Handle) -> f64 {
    bool_f64(agent_field(handle, false, |a| a.keep_alive))
}

#[no_mangle]
pub extern "C" fn js_http_agent_protocol(handle: Handle) -> *mut StringHeader {
    let s = get_handle_mut::<AgentHandle>(handle)
        .and_then(|a| a.protocol.clone())
        .unwrap_or_else(|| "http:".to_string());
    alloc_string(&s).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_agent_destroyed(handle: Handle) -> f64 {
    bool_f64(agent_field(handle, false, |a| a.destroyed))
}

#[no_mangle]
pub extern "C" fn js_http_agent_default_port(handle: Handle) -> f64 {
    match agent_field(handle, Some("http:".to_string()), |a| a.protocol.clone()).as_deref() {
        Some("https:") => 443.0,
        Some("http:") => 80.0,
        _ => 0.0,
    }
}

/// `agent.sockets` — per-origin arrays sized to the active connection count.
/// Returns a NaN-boxed object pointer (bits as f64). #2154 / #4975.
#[no_mangle]
pub extern "C" fn js_http_agent_sockets(handle: Handle) -> f64 {
    let sockets = agent_field(handle, HashMap::new(), |agent| {
        agent.active_socket_handles.clone()
    });
    handle_map_object_f64(&sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_free_sockets(handle: Handle) -> f64 {
    let sockets = agent_field(handle, HashMap::new(), |agent| {
        agent.free_socket_handles.clone()
    });
    handle_map_object_f64(&sockets)
}

#[no_mangle]
pub extern "C" fn js_http_agent_requests(handle: Handle) -> f64 {
    let requests = agent_field(handle, HashMap::new(), |agent| {
        agent.queued_requests.clone()
    });
    handle_map_object_f64(&requests)
}

// ------------------------------------------------------------------
// Property setters
// ------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn js_http_agent_set_protocol(
    handle: Handle,
    value_ptr: *const StringHeader,
) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        if value_ptr.is_null() {
            agent.protocol = None;
        } else {
            let js = JsString::from_raw(value_ptr as *mut StringHeader);
            if let Some(s) = perry_ffi::read_string(js) {
                agent.protocol = Some(s.to_string());
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_sockets(handle: Handle, value: f64) {
    if !(value.is_infinite() && value.is_sign_positive()) {
        validate_positive("maxSockets", value);
    }
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_sockets = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_free_sockets(handle: Handle, value: f64) {
    if !(value.is_infinite() && value.is_sign_positive()) {
        validate_positive("maxFreeSockets", value);
    }
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_free_sockets = value;
    }
    invalidate_agent_client(handle);
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_max_total_sockets(handle: Handle, value: f64) {
    if !(value.is_infinite() && value.is_sign_positive()) {
        validate_positive("maxTotalSockets", value);
    }
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.max_total_sockets = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_keep_alive_msecs(handle: Handle, value: f64) {
    if value.is_nan() || value < 0.0 {
        throw_out_of_range("keepAliveMsecs", ">= 0", value);
    }
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.keep_alive_msecs = value;
    }
    invalidate_agent_client(handle);
}

/// `agentKeepAliveTimeoutBuffer` is an ordinary writable data property after
/// construction. Constructor validation/defaulting therefore does not apply to
/// later assignments; preserve the incoming JS value's raw NaN-boxed bits.
#[no_mangle]
pub extern "C" fn js_http_agent_set_keep_alive_timeout_buffer(handle: Handle, value: f64) {
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.agent_keep_alive_timeout_buffer = value;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_keep_alive(handle: Handle, value: f64) {
    let on = value != 0.0 && !value.is_nan();
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.keep_alive = on;
    }
    invalidate_agent_client(handle);
}

// ------------------------------------------------------------------
// createConnection / createSocket closure storage
// ------------------------------------------------------------------
//
// Node's Agent lets the caller override how a fresh socket is produced
// (`agent.createConnection = fn`). Today's perry-ext-http doesn't wire
// the override into the request path — full happy-path interop needs a
// net.Socket-shaped JS object that http.ClientRequest can write to,
// which is tracked as a #2154 follow-up. But storing the closure means
// reading `agent.createConnection` round-trips to the same function
// (closes the `===` checks several Node tests do).

#[no_mangle]
pub extern "C" fn js_http_agent_set_create_connection(handle: Handle, closure_ptr: i64) {
    ensure_gc_scanner_registered();
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.create_connection = closure_ptr;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_set_create_socket(handle: Handle, closure_ptr: i64) {
    ensure_gc_scanner_registered();
    if let Some(agent) = get_handle_mut::<AgentHandle>(handle) {
        agent.create_socket = closure_ptr;
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_create_connection(handle: Handle) -> i64 {
    let stored = agent_field(handle, 0i64, |a| a.create_connection);
    if stored != 0 {
        stored
    } else {
        bind_agent_method(handle, b"createConnection")
    }
}

#[no_mangle]
pub extern "C" fn js_http_agent_create_socket(handle: Handle) -> i64 {
    let stored = agent_field(handle, 0i64, |a| a.create_socket);
    if stored != 0 {
        stored
    } else {
        bind_agent_method(handle, b"createSocket")
    }
}

/// #2154 — if this agent has a `createConnection` override, build the
/// connection-options object Node passes it (`{ host, port, path }`),
/// invoke the override **on the calling (main) thread** — JS closure calls
/// must never run on a tokio worker per the arena-safety rule — and return
/// the `net.Socket` handle id it produced. Returns `None` when no override
/// is set or the return value isn't a usable socket handle, so the caller
/// falls back to the default reqwest transport.
///
/// # Safety
///
/// Must be called on the main thread. `handle` must be a live AgentHandle.
pub(crate) unsafe fn try_create_connection_socket(
    handle: Handle,
    host: &str,
    port: u16,
    path: &str,
) -> Option<i64> {
    let cc = agent_field(handle, 0i64, |a| a.create_connection);
    if cc == 0 {
        return None;
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let cc = scope.root_addr(cc);
    let options = scope.root_nanbox(build_connect_options(handle, host, port, path));
    let closure = JsClosure::from_raw(cc.get() as *const RawClosureHeader);
    let ret = closure.call1(options.get());

    // `net.connect` / `net.createConnection` return the socket id NaN-boxed
    // with POINTER_TAG; some codegen paths hand back a bare raw pointer.
    // Extract the 48-bit handle id either way; reject anything else.
    let bits = ret.to_bits();
    let upper = bits >> 48;
    let id = if upper == 0x7FFD {
        (bits & PTR_MASK) as i64
    } else if upper == 0 && bits >= 0x10000 {
        bits as i64
    } else {
        return None;
    };
    (id > 0).then_some(id)
}

/// #2154 — the agent's `createSocket` override closure pointer (0 when no
/// override is set). Node's `Agent.prototype.addRequest` calls
/// `createSocket(req, options, cb)`; lib.rs prefers this over
/// `createConnection` on the request path when it's set.
pub(crate) fn create_socket_override(handle: Handle) -> i64 {
    agent_field(handle, 0i64, |a| a.create_socket)
}

/// Build the connection options handed to a `createConnection` /
/// `createSocket` override. Node also copies the Agent's TCP keep-alive
/// settings onto this object; wrappers commonly inspect those fields before
/// delegating to the original `createConnection` implementation.
/// Returns a NaN-boxed object pointer as `f64`, or NaN-boxed `undefined` on
/// allocation failure.
pub(crate) unsafe fn build_connect_options(
    handle: Handle,
    host: &str,
    port: u16,
    path: &str,
) -> f64 {
    let keys = ["host", "port", "path", "keepAlive", "keepAliveInitialDelay"];
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let obj: *mut perry_ffi::ObjectHeader = perry_ffi::js_object_alloc_with_shape(
        shape_id,
        keys.len() as u32,
        packed.as_ptr(),
        packed.len() as u32,
    );
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let scope = perry_ffi::TransientRootScope::enter();
    let obj = scope.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(obj as *mut u8).bits(),
    ));
    let host_s = alloc_string(host);
    perry_ffi::js_object_set_field(
        JsValue::from_bits(obj.get().to_bits()).as_pointer(),
        0,
        JsValue::from_string_ptr(host_s.as_raw()),
    );
    // A plain f64 number is its own NaN-box; reconstruct the JsValue from its
    // bits so the override reads `options.port` as a number.
    perry_ffi::js_object_set_field(
        JsValue::from_bits(obj.get().to_bits()).as_pointer(),
        1,
        JsValue::from_bits((port as f64).to_bits()),
    );
    let path_s = alloc_string(path);
    perry_ffi::js_object_set_field(
        JsValue::from_bits(obj.get().to_bits()).as_pointer(),
        2,
        JsValue::from_string_ptr(path_s.as_raw()),
    );
    let (keep_alive, keep_alive_msecs) = agent_field(handle, (false, 1000.0), |agent| {
        (agent.keep_alive, agent.keep_alive_msecs)
    });
    perry_ffi::js_object_set_field(
        JsValue::from_bits(obj.get().to_bits()).as_pointer(),
        3,
        JsValue::from_bits(bool_f64(keep_alive).to_bits()),
    );
    perry_ffi::js_object_set_field(
        JsValue::from_bits(obj.get().to_bits()).as_pointer(),
        4,
        JsValue::from_bits(keep_alive_msecs.to_bits()),
    );
    obj.get()
}

// ------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::drop_handle;

    #[test]
    fn default_agent_constructor_no_options() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        let agent = get_handle_mut::<AgentHandle>(handle).expect("handle should resolve");
        assert_eq!(agent.protocol.as_deref(), Some("http:"));
        assert_eq!(agent.keep_alive, false);
        assert_eq!(agent.keep_alive_msecs, 1000.0);
        assert_eq!(agent.agent_keep_alive_timeout_buffer, 1000.0);
        assert!(agent.max_sockets.is_infinite());
        assert_eq!(agent.max_free_sockets, 256.0);
        assert!(!agent.destroyed);
        drop_handle(handle);
    }

    #[test]
    fn validate_positive_accepts_positive() {
        validate_positive("maxSockets", 1.0);
        validate_positive("maxSockets", 64.0);
    }

    #[test]
    fn format_received_handles_special_values() {
        assert_eq!(format_received_number(f64::NAN), "NaN");
        assert_eq!(format_received_number(f64::INFINITY), "Infinity");
        assert_eq!(format_received_number(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(format_received_number(-1.0), "-1");
        assert_eq!(format_received_number(0.5), "0.5");
    }

    #[test]
    fn keep_alive_timeout_buffer_normalizes_constructor_values() {
        assert_eq!(
            normalize_agent_keep_alive_timeout_buffer(Some(1500.0)),
            1500.0
        );
        assert_eq!(normalize_agent_keep_alive_timeout_buffer(Some(0.0)), 0.0);
        assert_eq!(
            normalize_agent_keep_alive_timeout_buffer(Some(-1.0)),
            1000.0
        );
        assert_eq!(
            normalize_agent_keep_alive_timeout_buffer(Some(f64::INFINITY)),
            1000.0
        );
        assert_eq!(
            normalize_agent_keep_alive_timeout_buffer(Some(f64::NAN)),
            1000.0
        );
        assert_eq!(normalize_agent_keep_alive_timeout_buffer(None), 1000.0);
    }

    #[test]
    fn sockets_accessors_return_objects() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        let sockets = js_http_agent_sockets(handle);
        let free = js_http_agent_free_sockets(handle);
        let requests = js_http_agent_requests(handle);
        // Empty objects are NaN-boxed pointer values — the upper 16
        // bits must be `0x7FFD` (POINTER_TAG) and not `0x7FFC*`
        // (undefined/null/bool tags).
        for v in [sockets, free, requests] {
            let upper = v.to_bits() >> 48;
            assert_eq!(upper, 0x7FFD, "expected POINTER_TAG, got {:#x}", upper);
        }
        drop_handle(handle);
    }

    #[test]
    fn max_sockets_queues_and_resumes_fifo() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        js_http_agent_set_max_sockets(handle, 1.0);
        let key = "localhost:8080:";

        let first_socket = match admit_request(handle, key, 101) {
            PoolAdmission::Active {
                reused: false,
                socket,
            } => socket,
            other => panic!("unexpected admission: {other:?}"),
        };
        assert_eq!(admit_request(handle, key, 102), PoolAdmission::Queued);
        let agent = get_handle::<AgentHandle>(handle).unwrap();
        assert_eq!(agent.sockets.get(key), Some(&1));
        assert_eq!(agent.requests.get(key), Some(&1));

        let second_socket = match release_request(handle, key, first_socket, false) {
            Some((102, false, socket)) => socket,
            other => panic!("unexpected release: {other:?}"),
        };
        let agent = get_handle::<AgentHandle>(handle).unwrap();
        assert_eq!(agent.sockets.get(key), Some(&1));
        assert!(!agent.requests.contains_key(key));

        assert_eq!(release_request(handle, key, second_socket, false), None);
        let agent = get_handle::<AgentHandle>(handle).unwrap();
        assert!(!agent.sockets.contains_key(key));
        drop_handle(handle);
    }

    fn assert_js_bool(value: f64, expected: bool) {
        let value = JsValue::from_bits(value.to_bits());
        assert!(value.is_bool(), "expected JS bool, got {:#x}", value.bits());
        assert_eq!(value.to_bool(), expected);
    }

    #[test]
    fn destroy_marks_destroyed() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        assert_js_bool(js_http_agent_destroyed(handle), false);
        let _ = js_http_agent_destroy(handle);
        assert_js_bool(js_http_agent_destroyed(handle), true);
        drop_handle(handle);
    }

    #[test]
    fn destroy_preserves_queued_requests_for_reconnect() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        js_http_agent_set_max_sockets(handle, 1.0);
        let key = "localhost:8080:";
        let first_socket = match admit_request(handle, key, 101) {
            PoolAdmission::Active {
                reused: false,
                socket,
            } => socket,
            other => panic!("unexpected admission: {other:?}"),
        };
        assert_eq!(admit_request(handle, key, 102), PoolAdmission::Queued);

        let _ = js_http_agent_destroy(handle);
        let agent = get_handle::<AgentHandle>(handle).unwrap();
        assert_eq!(agent.requests.get(key), Some(&1));
        assert_eq!(agent.queued_requests.get(key), Some(&vec![102]));
        assert!(matches!(
            release_request(handle, key, first_socket, false),
            Some((102, false, _))
        ));
        drop_handle(handle);
    }

    #[test]
    fn setters_apply_new_values() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        js_http_agent_set_max_sockets(handle, 4.0);
        assert_eq!(js_http_agent_max_sockets(handle), 4.0);
        js_http_agent_set_keep_alive(handle, 1.0);
        assert_js_bool(js_http_agent_keep_alive(handle), true);
        js_http_agent_set_keep_alive_timeout_buffer(handle, 250.0);
        assert_eq!(js_http_agent_keep_alive_timeout_buffer(handle), 250.0);
        drop_handle(handle);
    }

    #[test]
    fn client_for_agent_memoizes() {
        let handle = unsafe { js_http_agent_new(f64::from_bits(TAG_UNDEFINED)) };
        let c1 = client_for_agent(handle);
        let c2 = client_for_agent(handle);
        // `reqwest::Client` is cheap-clone (Arc inside); the cache should
        // be returning the same underlying instance, so cloning the
        // returned client and dropping shouldn't leave a fresh entry.
        // We can't compare clients by identity, but we can assert the
        // cache only has one entry for this handle.
        let cache = AGENT_CLIENTS.lock().unwrap();
        assert!(cache.contains_key(&handle));
        drop(c1);
        drop(c2);
        drop(cache);
        let _ = AGENT_CLIENTS.lock().map(|mut c| c.remove(&handle));
        drop_handle(handle);
    }
}

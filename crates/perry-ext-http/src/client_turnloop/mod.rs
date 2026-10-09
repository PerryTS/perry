//! HTTP client exchanges run on ordinary net.Socket payloads. Request and
//! Agent app records own the actual objects; their separately owned framing
//! payload contains pure Rust bytes. Socket owns TLS, driver capabilities and
//! deadlines. Upgrade stores the current route without a transfer or retoken.

mod native_client;
mod pool;
mod protocol;
mod proxy;
pub(crate) mod tls;
mod wire;

use crate::tls_client::TlsOptions;
use crate::{push_event, ClientRequestHandle, PendingHttpEvent};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{Handle, JsValue, TransientRootScope};
pub(crate) use pool::{PoolKey, Reuse};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
pub(crate) const SUBSYSTEM: u8 = 6;
const NO_LOOP_CODE: &str = "ENOTSUP";

static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static REUSED: AtomicU64 = AtomicU64::new(0);
static HANDSHAKES: AtomicU64 = AtomicU64::new(0);
static TIMED_OUT: AtomicU64 = AtomicU64::new(0);
static RAW_COMPLETED: AtomicU64 = AtomicU64::new(0);
static DEFERRED_FIRED: AtomicU64 = AtomicU64::new(0);

/// Exchanges handed to this module (directly or posted to the loop owner).
pub fn accepted_total() -> u64 {
    ACCEPTED.load(Ordering::Relaxed)
}

/// Exchanges whose response was decoded through to its end.
pub fn completed_total() -> u64 {
    COMPLETED.load(Ordering::Relaxed)
}

/// Exchanges that ran on a pooled (kept-alive) connection.
pub fn reused_total() -> u64 {
    REUSED.load(Ordering::Relaxed)
}

/// TLS handshakes this module completed.
pub fn tls_handshakes_total() -> u64 {
    HANDSHAKES.load(Ordering::Relaxed)
}

/// Exchanges torn down by their deadline.
pub fn timed_out_total() -> u64 {
    TIMED_OUT.load(Ordering::Relaxed)
}

/// Exchanges on caller-supplied Sockets that
/// delivered a response or an upgrade.
pub fn raw_completed_total() -> u64 {
    RAW_COMPLETED.load(Ordering::Relaxed)
}

/// Deferred request timeout callbacks that fired.
pub fn deferred_fired_total() -> u64 {
    DEFERRED_FIRED.load(Ordering::Relaxed)
}

// ── The request ─────────────────────────────────────────────────────────────

/// Which of the four exchange shapes a request is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Streamed response, keep-alive eligible.
    Normal,
    /// `TE: trailers`: the response is buffered so its trailers can be
    /// delivered with it; the connection is closed afterwards.
    Trailers,
    /// `Connection: Upgrade`: a `101` hands the socket to `net`.
    Upgrade,
    /// `Expect: 100-continue`: the head goes out now, the body after the
    /// interim `100`, and `end()` supplies it through [`continue_body`].
    Continue,
}

/// One request, owned, ready to cross to the loop thread.
pub(crate) struct Outbound {
    pub(crate) request_handle: Handle,
    pub(crate) method: String,
    pub(crate) url: url::Url,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Vec<u8>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) mode: Mode,
    pub(crate) reuse: Option<Reuse>,
    pub(crate) key: PoolKey,
    /// Set for `https:`.
    pub(crate) tls: Option<tls::TlsPlan>,
    /// Set when `NODE_USE_ENV_PROXY=1` selects a proxy for this URL.
    pub(crate) proxy: Option<url::Url>,
    /// Headers the transport adds after the caller's own (the in-process
    /// HTTPS server's forwarding token).
    pub(crate) extra: Vec<(String, String)>,
}

struct LoopJob(Box<dyn FnOnce() + Send>);

impl perry_ffi::agent_post::AgentJob for LoopJob {
    fn run(self: Box<Self>) {
        (self.0)();
    }
}

fn on_loop(op: impl FnOnce() + Send + 'static) -> bool {
    if available() {
        op();
        return true;
    }
    if !perry_ffi::agent_post::available() {
        return false;
    }
    let mut job = Box::new(LoopJob(Box::new(move || {
        // The owner has registered the sink by definition, but the `Once`
        // must still run on whichever thread first reaches this module.
        let _ = available();
        op();
    })));
    for _ in 0..POST_ATTEMPTS {
        match perry_ffi::agent_post::post_job(job) {
            Ok(()) => return true,
            Err(rejected) if rejected.is_permanent() => return false,
            Err(rejected) => {
                job = rejected.into_job();
                std::thread::yield_now();
            }
        }
    }
    false
}

fn report_no_loop(request_handle: Handle) {
    push_event(PendingHttpEvent::TransportError {
        request_handle,
        message: format!("connect {NO_LOOP_CODE}"),
        code: NO_LOOP_CODE.to_string(),
        syscall: "connect".to_string(),
        errno: tl::errno_for_code(NO_LOOP_CODE) as i64,
    });
}

// ── Submission (JS thread) ──────────────────────────────────────────────────

/// Everything `dispatch_request_snapshot` knows about a request.
pub(crate) struct Request<'a> {
    pub(crate) request_handle: Handle,
    pub(crate) method: &'a str,
    pub(crate) url: &'a str,
    pub(crate) headers: HashMap<String, String>,
    pub(crate) body: Vec<u8>,
    pub(crate) timeout_ms: Option<u64>,
    pub(crate) agent_handle: Handle,
    pub(crate) tls: &'a TlsOptions,
    pub(crate) continue_mode: bool,
}

/// The shape a request's headers select, using the predicates the three
/// retired bypass modules triggered on.
fn mode_for(headers: &HashMap<String, String>, continue_mode: bool) -> Mode {
    if continue_mode {
        Mode::Continue
    } else if crate::client_upgrade::wants_upgrade(headers) {
        Mode::Upgrade
    } else if wants_trailers(headers) {
        Mode::Trailers
    } else {
        Mode::Normal
    }
}

/// `TE: trailers` as one token of a comma list.
fn wants_trailers(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("te")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("trailers"))
    })
}

/// Build the owned request. `Err` is the message for a request that cannot be
/// sent at all (an unparseable URL, bad TLS material, an unusable proxy).
fn prepare(request: Request<'_>) -> Result<Outbound, String> {
    let url = url::Url::parse(request.url).map_err(|e| e.to_string())?;
    let https = match url.scheme() {
        "http" => false,
        "https" => true,
        other => return Err(format!("unsupported protocol {other}:")),
    };
    let host = protocol::dial_host(&url).ok_or_else(|| "missing host".to_string())?;
    let port = url
        .port_or_known_default()
        .unwrap_or(if https { 443 } else { 80 });
    let proxy = proxy::proxy_for(&url)?;
    let tls = if https {
        Some(tls::plan(request.tls, &host)?)
    } else {
        None
    };
    let mode = mode_for(&request.headers, request.continue_mode);
    let reuse = if mode == Mode::Normal {
        pool::policy_for(request.agent_handle)
    } else {
        None
    };

    // The in-process HTTPS server's forwarding headers, exactly as the reqwest
    // path attached them (`tls_client::register_internal_https_server`).
    let mut extra = Vec::new();
    if https {
        if let Some(token) = crate::tls_client::internal_https_token_for_url(request.url) {
            extra.push(("x-perry-internal-tls-token".to_string(), token));
            if let Some(servername) = request.tls.servername.as_deref() {
                extra.push((
                    "x-perry-tls-servername".to_string(),
                    if servername.is_empty() {
                        "<false>".to_string()
                    } else {
                        servername.to_string()
                    },
                ));
            }
            if let Some(common_name) = request.tls.peer_certificate_cn.as_deref() {
                extra.push(("x-perry-tls-peer-cn".to_string(), common_name.to_string()));
            }
        }
    }

    let key = PoolKey {
        agent: request.agent_handle,
        https,
        host,
        port,
        proxy: proxy.as_ref().map(ToString::to_string),
        tls: if https { tls::identity(request.tls) } else { 0 },
    };
    Ok(Outbound {
        request_handle: request.request_handle,
        method: request.method.to_string(),
        url,
        headers: request.headers,
        body: request.body,
        // Node treats a zero timeout as "no timeout"; reqwest's zero-length
        // deadline timed the request out immediately.
        timeout_ms: request.timeout_ms.filter(|ms| *ms > 0),
        mode,
        reuse,
        key,
        tls,
        proxy,
        extra,
    })
}

/// How a request was carried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Submitted to this thread's own loop.
    Direct,
    /// Posted to the thread that owns this agent's loop.
    Posted,
    /// Refused before submission; a terminal event has been queued.
    Refused,
}

/// Carry a request. Always delivers exactly one terminal event for it.
pub(crate) fn dispatch(request: Request<'_>) -> Route {
    let request_handle = request.request_handle;
    let outbound = match prepare(request) {
        Ok(outbound) => outbound,
        Err(error_message) => {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message,
            });
            return Route::Refused;
        }
    };
    ACCEPTED.fetch_add(1, Ordering::Relaxed);
    let direct = available();
    let carried = on_loop(move || start(outbound));
    if !carried {
        report_no_loop(request_handle);
        return Route::Refused;
    }
    if direct {
        Route::Direct
    } else {
        Route::Posted
    }
}

/// Start an exchange on the loop thread.
pub fn available() -> bool {
    native_client::available()
}
const POST_ATTEMPTS: usize = 64;

fn socket_owner(request: Handle) -> Option<f64> {
    perry_ffi::get_handle::<ClientRequestHandle>(request)
        .filter(|req| req.socket_handle != 0)
        .map(|req| f64::from_bits(JsValue::from_object_ptr(req.socket_handle as *mut u8).bits()))
}

fn start(outbound: Outbound) {
    let request = outbound.request_handle;
    let Some(owner) = socket_owner(request) else {
        push_event(PendingHttpEvent::Error {
            request_handle: request,
            error_message: "HTTP request has no assigned Socket".into(),
        });
        return;
    };
    if let Err(message) = native_client::start_on_socket(owner, outbound, None, false) {
        push_event(PendingHttpEvent::Error {
            request_handle: request,
            error_message: message,
        });
    }
}

pub(crate) fn continue_body(request: Handle, body: Vec<u8>) {
    let _ = on_loop(move || {
        if let Some(owner) = socket_owner(request) {
            native_client::continue_body(owner, request, body);
        }
    });
}
pub(crate) fn cancel(request: Handle) {
    let _ = on_loop(move || {
        if let Some(owner) = socket_owner(request) {
            native_client::cancel(owner, request);
        }
    });
}

pub fn purge_agent(handle: Handle) {
    let sockets = perry_ffi::get_handle_mut::<crate::agent::AgentHandle>(handle)
        .map(|agent| {
            agent.free_sockets.clear();
            agent
                .free_socket_handles
                .drain()
                .flat_map(|(_, sockets)| sockets)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let scope = TransientRootScope::enter();
    for socket in scope.root_addrs(&sockets) {
        perry_ext_net::native_transport::destroy(f64::from_bits(
            JsValue::from_object_ptr(socket.get() as *mut u8).bits(),
        ));
    }
}

/// A deferred request timeout is an ordinary runtime Timeout callback. It
/// owns its callback on the current JS heap; no HTTP timer identity map.
pub(crate) fn arm_request_timeout(request: Handle, ms: u64) {
    extern "C" {
        fn js_set_timeout_callback(callback: i64, delay: f64) -> i64;
        fn js_timer_unref(timer: i64);
    }
    let scope = TransientRootScope::enter();
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(deferred_timeout, 0; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    unsafe {
        perry_ffi::set_closure_capture_f64(
            callback.get() as *mut perry_ffi::RawClosureHeader,
            0,
            request as f64,
        );
        let timer = js_set_timeout_callback(callback.get(), ms as f64);
        js_timer_unref(timer);
    }
}
unsafe extern "C" fn deferred_timeout(
    closure: *const perry_ffi::RawClosureHeader,
    _: perry_ffi::JsThis,
) -> f64 {
    let request = perry_ffi::closure_capture_f64(closure, 0) as Handle;
    if perry_ffi::get_handle::<ClientRequestHandle>(request).is_some_and(|req| !req.completed) {
        DEFERRED_FIRED.fetch_add(1, Ordering::Relaxed);
        push_event(PendingHttpEvent::Timeout {
            request_handle: request,
        });
    }
    f64::from_bits(JsValue::UNDEFINED.bits())
}

pub(crate) fn dispatch_supplied(request: Request<'_>, owner: f64, head: Vec<u8>) {
    let request_handle = request.request_handle;
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let outbound = match prepare(request) {
        Ok(mut out) => {
            out.reuse = None;
            out
        }
        Err(error_message) => {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message,
            });
            return;
        }
    };
    if let Err(error_message) =
        native_client::start_on_socket(owner.get(), outbound, Some(head), false)
    {
        push_event(PendingHttpEvent::Error {
            request_handle,
            error_message,
        });
    }
}

/// Queue a `'timeout'` for `request_handle` in `ms`. For
/// `tests/turnloop_client_exchange.rs`.
pub fn schedule_timeout_for_test(request_handle: Handle, ms: u64) {
    arm_request_timeout(request_handle, ms);
}

#[cfg(test)]
mod tests;

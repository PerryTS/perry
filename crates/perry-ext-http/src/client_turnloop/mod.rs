//! The `node:http` / `node:https` client, on turnloop — the only transport.
//!
//! Lane 1 (#11091) put the simplest shape here — a cleartext, bodyless request
//! on the implicit agent — and declined everything else to reqwest. This module
//! now carries **every** exchange `dispatch_request_snapshot` used to hand
//! reqwest, plus the three shapes that bypassed reqwest on raw tokio sockets,
//! and `reqwest` is no longer a dependency of this crate:
//!
//! | shape | how |
//! |---|---|
//! | request bodies | buffered at `end()`, so always a known length: `Content-Length`, or chunked when the caller set `Transfer-Encoding: chunked` ([`wire`]) |
//! | `options.timeout` / `req.setTimeout` | a deadline on the loop (`tl::timer_arm`) covering the whole exchange, as reqwest's `RequestBuilder::timeout` did; it fires `'timeout'` and tears the exchange down |
//! | `https:` | [`perry_tls_session::TlsSession`] above the same socket handle, with the verifier `tls_client` builds from Node's options ([`tls`]) |
//! | an explicit or HTTPS `Agent` | keep-alive with physical reuse ([`pool`]); the observable agent pools in `agent.rs` are untouched |
//! | an explicit `Host` | sent verbatim, as Node and reqwest both did ([`wire`]) |
//! | `NODE_USE_ENV_PROXY=1` | absolute-form through an HTTP proxy, or a `CONNECT` tunnel for `https:` ([`proxy`]) |
//! | `TE: trailers` | the codec's `Event::Trailers`, delivered with the buffered response — was `plain_client.rs` over a tokio `TcpStream` |
//! | `Expect: 100-continue` | head first, body withheld until the interim `100`, which fires `'continue'` — was `continue_client.rs` over tokio |
//! | `Connection: Upgrade` | the codec's `Event::Upgrade`; a `101` hands the live handle to `net` with `tl::transfer` — was `client_upgrade.rs` over tokio |
//!
//! # What is still not here
//!
//! * `agent.createConnection` / `createSocket` and a request-level
//!   `createConnection` run their exchange over a socket JS produced
//!   (`client_connect_override.rs`, perry-ext-net's raw vtable). They are
//!   decided before this module is offered the request and are unchanged.
//! * A thread that does not own its agent's loop **posts** the submission to
//!   the owner (`perry_ffi::agent_post`), which serves the same JS heap; only a
//!   host where no loop exists at all reports `ENOTSUP` — the rule `perry-ext-net`
//!   adopted when it dropped tokio.
//! * `Connection: Upgrade` over `https:` cannot hand a TLS session to `net`, so
//!   a `101` there is delivered as an ordinary response, exactly as reqwest did.
//!
//! # Redirects
//!
//! None are followed. Node's `http.request` never follows a 3xx; reqwest had to
//! be told `redirect::Policy::none()`, and driving the codec directly gives it
//! by construction.
//!
//! # Threading, locking and the GC
//!
//! Every connection lives on the agent's loop and is touched only from this
//! module's completion sink or from work running on that loop. State is one
//! mutex ([`State`]), and **no `tl::` call is ever made while it is held**: a
//! loopback completion can be delivered to the sink before the submitting call
//! returns, and the sink takes the same lock. Handlers therefore compute a list
//! of [`Effect`]s under the lock and perform them after releasing it.
//!
//! The sink runs no JS. Every outcome is a `PendingHttpEvent` for
//! `js_http_process_pending`, exactly as the reqwest task's were. Nothing here
//! holds a JS value — requests are owned `String`/`Vec` copies — so there is no
//! GC root to register and `scan_http_roots` is unchanged.

mod conn;
mod pool;
mod proxy;
pub(crate) mod tls;
mod wire;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use perry_ffi::turnloop_net as tl;
use perry_ffi::Handle;

use crate::tls_client::TlsOptions;
use crate::{push_event, PendingHttpEvent};

pub(crate) use pool::{PoolKey, Reuse};

/// This lane's slot in the runtime's completion-sink registry.
///
/// Distinct from `server/turnloop_serve`'s `1`, which is this crate's *server*.
/// The authority for the map is `perry-db-turnloop`'s `subsystem` module
/// header; `6` is the free slot between `perry-stdlib`'s framework server (5)
/// and `perry-ext-ws`'s client (7).
pub(crate) const SUBSYSTEM: u8 = 6;

/// Node sets `TCP_NODELAY` on client sockets; a request that sat in Nagle's
/// queue would add a round trip to every exchange.
const NODELAY: bool = true;

/// The code a request fails with when no loop exists for its agent at all.
/// Same as `perry-ext-net`'s: there is no second event loop to fall back to.
const NO_LOOP_CODE: &str = "ENOTSUP";

// ── Liveness counters ───────────────────────────────────────────────────────
//
// A transport that silently did nothing would leave every JS-level test green
// having exercised nothing — the "gate runs but its subject never did" shape.
// These let a test assert the subject was live, per shape.

static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static REUSED: AtomicU64 = AtomicU64::new(0);
static HANDSHAKES: AtomicU64 = AtomicU64::new(0);
static TIMED_OUT: AtomicU64 = AtomicU64::new(0);

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

// ── Shared state ────────────────────────────────────────────────────────────

/// A deadline this module armed, and what it is for.
#[derive(Clone, Copy, Debug)]
enum Timer {
    /// `options.timeout` for an in-flight exchange.
    Deadline { conn: i64, request: Handle },
    /// An idle pooled connection's expiry.
    Idle { conn: i64 },
    /// `req.setTimeout(ms, cb)` armed before (or independently of) dispatch.
    Standalone { request: Handle },
}

#[derive(Default)]
struct State {
    conns: HashMap<i64, conn::Conn>,
    /// Which connection is carrying a request, for `destroy()` and the
    /// deferred `Expect: 100-continue` body.
    by_request: HashMap<Handle, i64>,
    timers: HashMap<i64, Timer>,
    idle: HashMap<PoolKey, Vec<i64>>,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    f(&mut guard)
}

/// I/O decided under the lock and performed after it is released.
enum Effect {
    Connect {
        id: i64,
        host: String,
        port: u16,
    },
    ReadStart(i64),
    Write(i64, Vec<u8>),
    Close(i64),
    SetRef(i64, bool),
    ArmTimer(i64, u64),
    CancelTimer(i64),
    FreeId(i64),
    Push(PendingHttpEvent),
    /// Hand a connection to `net` after a `101`, then publish the event.
    Handoff(i64, PendingHttpEvent),
    /// Run a request again on a fresh connection (a reused one died before
    /// the response began), keeping its in-flight guard.
    Redispatch(Box<Outbound>, crate::ClientInflightGuard),
}

/// Perform effects in order. A failed submission feeds its handler, whose own
/// effects join the queue — that is how a write error on a connection becomes
/// the same teardown a read error would.
fn run(effects: Vec<Effect>) {
    let mut queue = std::collections::VecDeque::from(effects);
    while let Some(effect) = queue.pop_front() {
        let more = match effect {
            Effect::Connect { id, host, port } => {
                match tl::tcp_connect(id, SUBSYSTEM, &host, port, NODELAY) {
                    Ok(()) => Vec::new(),
                    Err(error) => with_state(|st| {
                        conn::on_error(st, id, &error.code, &error.syscall, error.errno as i64)
                    }),
                }
            }
            Effect::ReadStart(id) => match tl::read_start(id) {
                Ok(()) => Vec::new(),
                Err(error) => with_state(|st| {
                    conn::on_error(st, id, &error.code, &error.syscall, error.errno as i64)
                }),
            },
            Effect::Write(id, bytes) => {
                if bytes.is_empty() {
                    Vec::new()
                } else {
                    match tl::write(id, &bytes, 0) {
                        Ok(_) => Vec::new(),
                        Err(error) => with_state(|st| {
                            conn::on_error(st, id, &error.code, &error.syscall, error.errno as i64)
                        }),
                    }
                }
            }
            Effect::Close(id) => {
                let _ = tl::close(id);
                Vec::new()
            }
            Effect::SetRef(id, referenced) => {
                tl::set_ref(id, referenced);
                Vec::new()
            }
            Effect::ArmTimer(id, ms) => match tl::timer_arm(id, SUBSYSTEM, ms) {
                Ok(()) => Vec::new(),
                // No deadline could be armed: fire it now rather than never.
                Err(_) => with_state(|st| conn::on_timer(st, id)),
            },
            Effect::CancelTimer(id) => {
                let _ = tl::timer_cancel(id);
                perry_ffi::free_handle_id(id);
                Vec::new()
            }
            Effect::FreeId(id) => {
                perry_ffi::free_handle_id(id);
                Vec::new()
            }
            Effect::Push(event) => {
                push_event(event);
                Vec::new()
            }
            Effect::Handoff(id, event) => {
                handoff(id, event);
                Vec::new()
            }
            Effect::Redispatch(outbound, inflight) => with_state(|st| {
                let mut fx = Vec::new();
                conn::start(st, *outbound, Some(inflight), &mut fx);
                fx
            }),
        };
        queue.extend(more);
    }
}

/// A `101` on a cleartext upgrade request: the live handle becomes a
/// `net.Socket`, keeping its id and outstanding read — the server's own
/// `'upgrade'` handoff in `turnloop_serve`, from the client side.
fn handoff(id: i64, event: PendingHttpEvent) {
    let adopted = tl::transfer(id, perry_ext_net::TURNLOOP_SUBSYSTEM).is_ok()
        && perry_ext_net::adopt_turnloop_upgrade(id);
    if adopted {
        push_event(event);
        return;
    }
    let _ = tl::close(id);
    if let PendingHttpEvent::Upgrade { request_handle, .. } = event {
        push_event(PendingHttpEvent::CodedError {
            request_handle,
            message: "socket hang up".to_string(),
            code: "ECONNRESET".to_string(),
        });
    }
}

// ── Ids ─────────────────────────────────────────────────────────────────────

/// One authoritative id domain for the connections and deadlines this module
/// creates. The runtime keys its handle table by id across every subsystem,
/// so it must be globally unique.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("http client registry domains exhausted")
    })
}

fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(registry_domain())
}

/// This subsystem accepts nothing — it only dials.
extern "C" fn alloc_id() -> i64 {
    0
}

// ── Availability and routing ────────────────────────────────────────────────

/// Whether a request issued *now, on this thread* can be submitted directly.
///
/// Deliberately not cached: availability is a property of the calling thread,
/// and the first thread to ask claims its agent's route.
pub fn available() -> bool {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        tl::register_sink(SUBSYSTEM, sink, alloc_id);
    });
    tl::available(SUBSYSTEM)
}

struct LoopJob(Box<dyn FnOnce() + Send>);

impl perry_ffi::agent_post::AgentJob for LoopJob {
    fn run(self: Box<Self>) {
        (self.0)();
    }
}

/// How often a transiently refused post is retried before the request fails.
const POST_ATTEMPTS: usize = 64;

/// Run `op` on the loop: here when this thread owns it, else on the owner.
/// `false` means no loop exists for this agent at all; `op` was not run.
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
    let host = conn::dial_host(&url).ok_or_else(|| "missing host".to_string())?;
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
fn start(outbound: Outbound) {
    let effects = with_state(|st| start_locked(st, outbound));
    run(effects);
}

fn start_locked(st: &mut State, outbound: Outbound) -> Vec<Effect> {
    let mut fx = Vec::new();
    conn::start(st, outbound, None, &mut fx);
    fx
}

/// The body `end()` supplies to an `Expect: 100-continue` exchange.
pub(crate) fn continue_body(request_handle: Handle, body: Vec<u8>) {
    let carried = on_loop(move || {
        let effects = with_state(|st| {
            let mut fx = Vec::new();
            conn::continue_body(st, request_handle, body, &mut fx);
            fx
        });
        run(effects);
    });
    let _ = carried;
}

/// `req.destroy()` / `req.abort()`: stop carrying the request. The JS-visible
/// teardown (`'error'`/`'close'`) is the caller's; this only closes the socket
/// so the peer sees it, as Node's destroy does, and drops the exchange so no
/// late event reaches a completed request.
pub(crate) fn cancel(request_handle: Handle) {
    // Only a request this module is carrying has anything to cancel. Checking
    // first also keeps `destroy()` of a never-dispatched request from asking
    // for the loop route, which the first asker claims for its thread.
    if !with_state(|st| st.by_request.contains_key(&request_handle)) {
        return;
    }
    let _ = on_loop(move || {
        let effects = with_state(|st| {
            let mut fx = Vec::new();
            conn::cancel(st, request_handle, &mut fx);
            fx
        });
        run(effects);
    });
}

/// `agent.destroy()`: close the agent's idle pooled connections. Also
/// reachable from `tests/turnloop_client_exchange.rs`, which parks one.
pub fn purge_agent(agent_handle: Handle) {
    // Idle connections exist only once an agent has used the transport; an
    // agent that never did must not claim the loop route just by being
    // configured or destroyed.
    if !with_state(|st| st.idle.keys().any(|key| key.agent == agent_handle)) {
        return;
    }
    let _ = on_loop(move || {
        let effects = with_state(|st| {
            let mut fx = Vec::new();
            conn::purge_agent(st, agent_handle, &mut fx);
            fx
        });
        run(effects);
    });
}

/// `req.setTimeout(ms[, cb])` / `options.timeout` armed at request creation:
/// a one-shot `'timeout'` for the request, independent of any exchange.
pub(crate) fn arm_request_timeout(request_handle: Handle, ms: u64) {
    let carried = on_loop(move || {
        let id = next_id();
        if id == perry_ffi::INVALID_HANDLE {
            push_event(PendingHttpEvent::Timeout { request_handle });
            return;
        }
        with_state(|st| {
            st.timers.insert(
                id,
                Timer::Standalone {
                    request: request_handle,
                },
            )
        });
        run(vec![Effect::ArmTimer(id, ms)]);
    });
    if !carried {
        // No loop anywhere for this agent: a plain thread keeps the promise
        // that `'timeout'` fires, without a second event loop.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            push_event(PendingHttpEvent::Timeout { request_handle });
        });
    }
}

// ── The public liveness hooks ───────────────────────────────────────────────

/// Offer a request to this module the way `dispatch_request_snapshot` does,
/// with no TLS options. `true` means it was carried (directly or posted).
///
/// Kept `pub` for `tests/turnloop_client_exchange.rs`, which asserts the lane
/// was live rather than trusting a JS-level green.
#[allow(clippy::too_many_arguments)]
pub fn try_dispatch(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    timeout_ms: Option<u64>,
    agent_handle: Handle,
) -> bool {
    try_dispatch_tls(
        request_handle,
        method,
        url,
        headers,
        body,
        timeout_ms,
        agent_handle,
        Vec::new(),
    )
}

/// [`try_dispatch`] with an explicit `ca` (PEM) for an `https:` URL.
#[allow(clippy::too_many_arguments)]
pub fn try_dispatch_tls(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    timeout_ms: Option<u64>,
    agent_handle: Handle,
    ca_pems: Vec<Vec<u8>>,
) -> bool {
    let tls = TlsOptions {
        ca_pems,
        ..TlsOptions::default()
    };
    let route = dispatch(Request {
        request_handle,
        method,
        url,
        headers: headers.clone(),
        body: body.to_vec(),
        timeout_ms,
        agent_handle,
        tls: &tls,
        continue_mode: false,
    });
    matches!(route, Route::Direct | Route::Posted)
}

/// Keep-alive for the liveness test, which has no `AgentHandle` to read the
/// policy from: carry `request` with this reuse policy on pool key `agent`.
#[allow(clippy::too_many_arguments)]
pub fn try_dispatch_pooled(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    agent: Handle,
    max_free: usize,
    idle_ms: u64,
) -> bool {
    let tls = TlsOptions::default();
    let mut outbound = match prepare(Request {
        request_handle,
        method,
        url,
        headers: headers.clone(),
        body: body.to_vec(),
        timeout_ms: None,
        agent_handle: agent,
        tls: &tls,
        continue_mode: false,
    }) {
        Ok(outbound) => outbound,
        Err(_) => return false,
    };
    outbound.reuse = Some(Reuse { max_free, idle_ms });
    ACCEPTED.fetch_add(1, Ordering::Relaxed);
    on_loop(move || start(outbound))
}

// ── The completion sink ─────────────────────────────────────────────────────

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    let effects = match c.kind {
        tl::NET_CONNECT => with_state(|st| conn::on_connect(st, c.id)),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => {
            let bytes = unsafe { c.bytes() };
            with_state(|st| conn::on_data(st, c.id, bytes))
        }
        tl::NET_EOF => with_state(|st| conn::on_eof(st, c.id)),
        tl::NET_ERROR => {
            // SAFETY: the runtime builds these from `&'static str`s.
            let code = unsafe { c.code() }.unwrap_or("EPIPE").to_string();
            let syscall = unsafe { c.syscall() }.unwrap_or("").to_string();
            with_state(|st| conn::on_error(st, c.id, &code, &syscall, c.errno as i64))
        }
        tl::NET_CLOSED => with_state(|st| conn::on_closed(st, c.id)),
        tl::NET_TIMER => with_state(|st| conn::on_timer(st, c.id)),
        // `NET_WROTE` is an acknowledgement only: `tl::write` copies.
        _ => Vec::new(),
    };
    run(effects);
}

#[cfg(test)]
mod tests;

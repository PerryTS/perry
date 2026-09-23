//! turnloop P6: Perry's **outbound** HTTP/1.1 on turnloop handles.
//!
//! P1 put Perry's sockets on turnloop and P5 put the HTTP/1.1 *server* codec and
//! TLS there. This is the client half: `fetch`, `axios` and anything else that
//! issues an outbound request is a state machine driven by `NET_*` completions
//! on the agent's own `turnloop::Loop` rather than a `reqwest` future driven by
//! a tokio tick. Since lane G of the tokio removal it is the ONLY transport:
//! `perry-stdlib` no longer depends on `reqwest` at all.
//!
//! # What drives what
//!
//! ```text
//!   submit(spec)  ──► Pool::acquire ──► turnloop_net::tcp_connect_host
//!                                            │ NET_CONNECT
//!                                            ▼
//!                              [TlsClientSession handshake]   (https only)
//!                                            │
//!                                            ▼
//!            client::Http1Connection::start ──► turnloop_net::write
//!                                            │ NET_DATA
//!                                            ▼
//!              [TLS decrypt] ──► Http1Connection::receive ──► Event::Head
//!                                                             Event::Body
//!                                                             Event::End
//!                                            │
//!                                            ▼
//!                       client::Request::redirect  ── resend ──┐
//!                                            │ final           │
//!                                            ▼                 │
//!                    compression::StreamingDecoder ────────────┘
//!                                            │
//!                                            ▼
//!                             Sink::on_done → queue_promise_resolution
//! ```
//!
//! # What is refused
//!
//! A thread without a loop of its own is not refused: since turnloop P10
//! `submit` posts to the thread that owns the agent's loop (`posted`). What is
//! left is a genuine absence of a loop for the whole agent — a host where
//! `Loop::new` failed — plus three request-shaped refusals the owner would make
//! identically (`Declined`). Each used to fall back to a `reqwest` future; each
//! now rejects the caller's promise with Node's error for the same input
//! (`fetch::transport_error::Rejection`).
//!
//! Every policy decision — redirects, the pool, the per-phase deadlines, the
//! proxy environment, `Content-Encoding` — comes from `turnloop_http::client`
//! and `turnloop_http::compression` rather than being written here. This module
//! owns the *transport*: sockets, completions, ids and TLS.
//!
//! # Why not `turnloop_http::asynchronous::client`
//!
//! The same reason P5 gave for the server: it needs a `turnloop_io::
//! ExecutorHandle`, `LocalExecutor::with_config` constructs its **own** driver,
//! and even sharing one, `Shared::dispatch` returns early for a token without
//! its tag bit — P1's net tokens, P2's process tokens, P3's timer token and P4's
//! pool tokens would all be silently dropped (turnloop#45). Perry owns one
//! `turnloop::Loop` per agent, so the codecs are driven sans-I/O over P1's
//! completion layer.
//!
//! # Ids
//!
//! Every turnloop handle on a thread lives in ONE `HashMap<i64, Entry>` in
//! `turnloop_net`, regardless of subsystem — and perry-ffi's handle band
//! (`[1, 0x40000)`, which is what `perry-ext-net` names its sockets from) and
//! perry-stdlib's `common` handle band are two *different* registries over the
//! *same* numeric range. Ids allocated here therefore come from a private band
//! starting at `ID_BASE = 1 << 40`, far above both, so a client socket can never
//! be confused with a `net.Socket`. `ids_are_disjoint_from_the_binding_bands`
//! tests that rather than leaving it to the comment.
//!
//! # GC
//!
//! **No JS value and no heap pointer reaches the driver**, exactly as in P1 and
//! P5. A request carries owned `String`/`Vec<u8>` and the `usize` address of a
//! promise created by `js_promise_new_cross_thread`, which pins it (#9552);
//! reads land in turnloop's pooled
//! buffers and are copied out inside the dispatch call. So this module registers
//! no GC root scanner, and `scripts/gc_runtime_root_holders.py` needs no entry
//! for it.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use perry_runtime::turnloop_net as tl;
use turnloop_http::client::{self as tlc, ConnectionId, PoolKey, RedirectMode};
use turnloop_http::http1;

mod exchange;
mod posted;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) use exchange::ClientError;

/// The `turnloop_net` subsystem slot this module registers.
///
/// Slot 0 is `perry-ext-net`; slot 1 is reserved for the bundled stdlib `net`
/// (P1's note). This takes slot 2, leaving 3 free.
const SUBSYSTEM: u8 = 2;

/// Private id band. See the module note: it must not overlap perry-ffi's or
/// perry-stdlib's handle bands, both of which end at `0x40000`.
const ID_BASE: i64 = 1 << 40;
/// Ids wrap inside `[ID_BASE, ID_CEILING)`; `turnloop_net` tokens carry 56 bits.
const ID_CEILING: i64 = 1 << 55;

/// Matches the reqwest client `fetch` used to build:
/// `pool_max_idle_per_host(16)`, `pool_idle_timeout(90s)`. Preserving those two numbers is what keeps the
/// migration invisible to a long-running service's connection behaviour.
const POOL_MAX_PER_HOST: usize = 16;
const POOL_IDLE: Duration = Duration::from_secs(90);

/// A response body larger than this is refused rather than buffered. Sixteen
/// megabytes of *decompressed* body is already beyond what the buffered
/// `fetch` surface can usefully hand to JS, and an unbounded decoder is a
/// decompression bomb.
pub(crate) const BODY_LIMIT: usize = 512 * 1024 * 1024;

/// Lifetime counters. A "turnloop carried this fetch" claim is worth nothing if
/// these are zero, so `PERRY_LOOP_STATS` prints them (DESIGN §11).
static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static DECLINED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
static REUSED: AtomicU64 = AtomicU64::new(0);
static CONNECTED: AtomicU64 = AtomicU64::new(0);
static REDIRECTS: AtomicU64 = AtomicU64::new(0);
static DECODED: AtomicU64 = AtomicU64::new(0);
/// CONNECT tunnels established. A "the proxy path ran" claim is worth nothing
/// if this is zero, which is exactly the assertion the proxy fixture makes.
static TUNNELS: AtomicU64 = AtomicU64::new(0);

/// Why a submission could not be served. There is no other transport behind
/// this engine any more, so the caller turns every variant into a rejection
/// (`fetch::transport_error::Rejection::for_declined`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Declined {
    /// No loop exists for this AGENT — not merely for this thread.
    ///
    /// turnloop P9 gave every agent a loop and P10 (`posted`) lets a thread
    /// that does not own its agent's loop hand the submission to the thread
    /// that does, so the case left is a genuine absence: a host where
    /// `Loop::new` failed.
    NoLoop,
    /// A proxy this client cannot drive. An `http://` proxy is served here
    /// — `turnloop_http::client::Route` supplies the CONNECT head and the
    /// tunnel decision, and `exchange` runs it — so this variant is reached
    /// only for a proxy URL that is not `http` (socks5, https-to-proxy), or one
    /// that will not parse. The error is the policy layer's
    /// (`UND_ERR_NOT_SUPPORTED` / `ERR_INVALID_URL`).
    ///
    /// The `https://`-proxy half used to fall back to reqwest, which could
    /// speak TLS to a proxy. That configuration now rejects: carrying it needs
    /// `turnloop_http::client::ProxyEnvironment::proxy_for` to accept an
    /// `https` proxy and this engine to run TLS-in-TLS. A `socks5://` proxy
    /// loses nothing — reqwest was built without its `socks` feature, so it
    /// failed there too.
    Proxy(turnloop_http::Error),
    /// Not an `http:`/`https:` URL, embedded credentials, or a method fetch
    /// refuses: the error `client::Request::new` returned.
    Unsupported(turnloop_http::Error),
    /// TLS is wanted but the client configuration could not be built.
    NoTls,
}

pub(crate) fn note_declined() {
    DECLINED.fetch_add(1, Ordering::Relaxed);
}

/// `PERRY_LOOP_STATS`'s P6 line. Printed by the event pump's stats reporter.
pub fn stats_line() -> String {
    format!(
        "[perry-loop] p6 http_submitted={} declined={} completed={} failed={} \
         connects={} reused={} redirects={} decoded_bodies={} tunnels={}",
        SUBMITTED.load(Ordering::Relaxed),
        DECLINED.load(Ordering::Relaxed),
        COMPLETED.load(Ordering::Relaxed),
        FAILED.load(Ordering::Relaxed),
        CONNECTED.load(Ordering::Relaxed),
        REUSED.load(Ordering::Relaxed),
        REDIRECTS.load(Ordering::Relaxed),
        DECODED.load(Ordering::Relaxed),
        TUNNELS.load(Ordering::Relaxed),
    )
}

/// Tunnels established on this thread. The liveness assertion for the proxy
/// path: a green proxy fixture with this at zero went direct.
pub fn tunnels_total() -> u64 {
    TUNNELS.load(Ordering::Relaxed)
}

/// Whether this phase carried any request at all — the liveness assertion a
/// test needs before believing a green run says anything.
pub fn submitted_total() -> u64 {
    SUBMITTED.load(Ordering::Relaxed)
}

/// Requests still outstanding on this thread. Read by the keep-alive gate so
/// `main()` returning while a fetch is in flight does not exit the loop (the
/// shape #591 fixed for the tokio pool).
pub fn has_pending_requests() -> bool {
    ENGINE.with(|e| !e.borrow().requests.is_empty())
}

// ── What a caller hands in, and gets back ──────────────────────────────────

/// One outbound request, fully materialized on the owning thread before it is
/// submitted. Owned data only — no JS value crosses into the engine.
pub(crate) struct RequestSpec {
    pub(crate) url: String,
    pub(crate) method: String,
    /// Caller headers in insertion order. `host` is supplied by
    /// `client::Request::head`, so one here is dropped.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) redirect: RedirectMode,
    /// The `AbortSignal` object address, when the caller bound one. Used only
    /// as a cancellation key; never dereferenced here.
    pub(crate) abort_key: Option<usize>,
}

/// The finished response, in the shape `fetch` stores in `FETCH_RESPONSES`.
pub(crate) struct ResponseOut {
    pub(crate) status: u16,
    pub(crate) status_text: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    pub(crate) final_url: String,
    pub(crate) redirected: bool,
}

pub(crate) enum Outcome {
    Ok(Box<ResponseOut>),
    Err(ClientError),
}

/// Where a finished (or streaming) response goes.
///
/// Plain function pointers rather than a boxed closure: the engine is a
/// thread-local table that must hold nothing a moving collector could
/// invalidate, and a `fn` is exactly that. `ctx` is the caller's own key — for
/// `fetch` it is the pinned promise address.
#[derive(Clone, Copy)]
pub(crate) struct Sink {
    pub(crate) ctx: usize,
    /// Called once, at the FINAL response's head (never for a followed
    /// redirect). Only set by a streaming caller.
    pub(crate) on_head: Option<fn(usize, u16, &[(String, String)])>,
    /// Called per decoded body chunk of the final response. Only set by a
    /// streaming caller; when set, `on_done` receives an empty body.
    pub(crate) on_chunk: Option<fn(usize, &[u8])>,
    pub(crate) on_done: fn(usize, Outcome),
}

// ── Engine state ───────────────────────────────────────────────────────────

/// A socket this engine owns.
struct Conn {
    pool_id: ConnectionId,
    key: PoolKey,
    tls: Option<Box<crate::turnloop_tls_client::TlsClientSession>>,
    http: tlc::Http1Connection,
    /// Decoded bytes the codec has not consumed yet.
    ///
    /// `http1::Decoder`'s contract is that **the host retains unconsumed
    /// input**: a head that has not reached its blank line consumes nothing and
    /// returns no event, and a chunk size or trailer block split across two
    /// reads does the same. Feeding only the newest read would silently drop
    /// the earlier half — which is a defect no loopback fixture can catch,
    /// because a small response head always arrives in one piece. It was found
    /// against `https://github.com/`, whose head spans two TLS records.
    input: Vec<u8>,
    /// The request currently using this socket, if any.
    request: Option<u64>,
    /// Armed while the socket is idle in the pool; `NET_TIMER` closes it.
    idle_timer: Option<i64>,
    /// Set once `close` has been submitted so a late completion is ignored.
    closing: bool,
    /// The connection has served at least one complete response. A reused
    /// connection that dies before its first byte is retried once (the classic
    /// idle-connection race); a fresh one is not.
    used: bool,
    /// The HTTP proxy this socket is dialled through, if any. The socket's peer
    /// is the PROXY, not the origin, so this is also what makes the pool key
    /// distinct — a tunnelled connection must never be handed to a direct
    /// request for the same origin, and vice versa.
    proxy: Option<url::Url>,
    /// Live only while a `CONNECT` tunnel is being established.
    tunnel: Option<Box<Tunnel>>,
}

/// One in-flight `CONNECT` exchange with a proxy.
///
/// It gets its own `Http1Connection` rather than borrowing `conn.http`: the
/// request's codec must stay untouched until the tunnel is up, so that
/// `Http1Connection::start`'s "one request in flight" check still means what it
/// says when the real head finally goes out. The tunnel is always spoken in the
/// clear — TLS begins *after* the proxy answers 2xx, which is the whole point.
struct Tunnel {
    http: tlc::Http1Connection,
    /// Bytes of the proxy's answer the codec has not consumed. Same retention
    /// rule as `Conn::input`, for the same reason.
    input: Vec<u8>,
}

/// One in-flight logical request — possibly across several connections, if it
/// is redirected or retried.
struct Req {
    spec: RequestSpec,
    sink: Sink,
    /// The HTTP proxy resolved for this request's URL at submission time, from
    /// `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` or from
    /// `undici.setGlobalDispatcher(new ProxyAgent(...))`. Resolved once, not
    /// per connection attempt, so a retry cannot silently change route.
    proxy: Option<url::Url>,
    /// `turnloop_http`'s policy object: the URL, method, headers, body and the
    /// redirect counter. Rewritten in place by `Request::redirect`.
    request: tlc::Request,
    conn: Option<i64>,
    /// Head of the response currently being decoded.
    head: Option<http1::Head>,
    /// Accumulated body of the response currently being decoded, still encoded.
    body: Vec<u8>,
    /// Decoder for a `Content-Encoding`d body; `None` for identity.
    decoder: Option<Box<turnloop_http::compression::StreamingDecoder>>,
    /// Decoded body. For a streaming sink this stays empty and chunks go out as
    /// they are produced.
    decoded: Vec<u8>,
    /// True once a redirect has been followed, for `response.redirected`.
    redirected: bool,
    /// True once the final head has been streamed to a streaming sink.
    streaming: bool,
    /// Set when the request has been retried once after an idle-connection
    /// race, so a second failure is reported rather than looping.
    retried: bool,
    /// Terminal: the sink has been called. Guards exactly-once delivery
    /// (DESIGN D4) against a completion that arrives after the outcome.
    delivered: bool,
}

#[derive(Default)]
struct Engine {
    registered: bool,
    pool: Option<tlc::Pool>,
    conns: HashMap<i64, Conn>,
    requests: HashMap<u64, Req>,
    /// Requests the pool told to `Wait`, per origin, in arrival order.
    waiting: HashMap<String, VecDeque<u64>>,
    /// `AbortSignal` address → the requests bound to it.
    aborts: HashMap<usize, Vec<u64>>,
    next_id: i64,
    next_req: u64,
    /// Sinks to run once the current dispatch has finished touching the tables.
    /// A sink may call back into `submit`, so it must never run while the
    /// `RefCell` is borrowed.
    pending: Vec<(Sink, Outcome)>,
    /// Set while `drain_pending` is running, so a sink that submits a new
    /// request does not re-enter the drain.
    draining: bool,
}

thread_local! {
    /// Per agent, like the loop itself. A request belongs to the thread that
    /// submitted it; there is no cross-thread map to race on.
    static ENGINE: RefCell<Engine> = RefCell::new(Engine::default());
}

impl Engine {
    fn alloc_id(&mut self) -> i64 {
        loop {
            if self.next_id < ID_BASE || self.next_id >= ID_CEILING {
                self.next_id = ID_BASE;
            }
            let id = self.next_id;
            self.next_id += 1;
            if !self.conns.contains_key(&id) {
                return id;
            }
        }
    }

    fn pool(&mut self) -> &mut tlc::Pool {
        self.pool
            .get_or_insert_with(|| tlc::Pool::new(POOL_MAX_PER_HOST, POOL_IDLE))
    }
}

/// Install the completion sink. Idempotent; called from `submit`.
fn ensure_registered(engine: &mut Engine) -> bool {
    if engine.registered {
        return true;
    }
    // A client never accepts, so the accepted-connection id allocator refuses.
    // Returning zero is `register_sink`'s documented refusal.
    extern "C" fn no_accept() -> i64 {
        0
    }
    engine.registered = tl::register_sink(SUBSYSTEM, sink, no_accept);
    if engine.registered {
        // `PERRY_LOOP_STATS`'s P6 line. Installed here rather than at startup so
        // a program that never issues an outbound request prints nothing extra.
        perry_runtime::event_pump::register_stats_reporter(print_stats);
        // The keep-alive contributor. Without it a program whose only work is an
        // outbound request exits before the response arrives — "Detected
        // unsettled top-level await", which is exactly how this was found: the
        // fetch fixture passed only because its own `node:http` server was
        // holding the loop open. A separate registry from `InflightGuard` on
        // purpose (P4's note 2): that counter also feeds `native_work_inflight`,
        // which would make the park choose the legacy tokio tick over a turn.
        //
        // SAFETY: a plain registration with a `'static` function pointer.
        unsafe { js_register_aux_has_active(aux_has_active) };
    }
    engine.registered
}

unsafe extern "C" {
    /// perry-runtime's `stdlib_pump::js_register_aux_has_active`. It is
    /// `pub(crate)` there, so it is reached as a `#[no_mangle]` extern — the
    /// same route `perry-ext-net` takes.
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
}

extern "C" fn aux_has_active() -> i32 {
    i32::from(has_pending_requests())
}

extern "C" fn print_stats() {
    eprintln!("{}", stats_line());
}

/// The `turnloop_net` completion sink. Runs on the loop-owning thread inside
/// the dispatch call, after `turn` has returned — so it may allocate Rust state
/// and settle promises through the deferred queue, but it runs no JS itself.
extern "C" fn sink(completion: *const tl::NetCompletion) {
    // SAFETY: `turnloop_net::dispatch` borrows a live `NetCompletion` for the
    // duration of this call; nothing here retains it.
    let c = unsafe { &*completion };
    // SAFETY: only read inside this call, which is the documented lease.
    let bytes = unsafe { c.bytes() };
    let code = unsafe { c.code_str() };
    let syscall = unsafe { c.syscall_str() };
    exchange::on_completion(c.kind, c.id, c.errno, code, syscall, bytes, c.terminal != 0);
    drain_pending();
}

/// Run every queued sink outside the engine borrow. A sink may submit another
/// request (a redirect chain in JS, `Promise.all` fan-out), which takes the
/// same `RefCell`.
fn drain_pending() {
    let already = ENGINE.with(|e| {
        let mut engine = e.borrow_mut();
        if engine.draining {
            return true;
        }
        engine.draining = !engine.pending.is_empty();
        !engine.draining
    });
    if already {
        return;
    }
    loop {
        let next = ENGINE.with(|e| {
            let mut engine = e.borrow_mut();
            let next = engine.pending.pop();
            if next.is_none() {
                engine.draining = false;
            }
            next
        });
        let Some((sink, outcome)) = next else { break };
        (sink.on_done)(sink.ctx, outcome);
    }
}

/// Serializes the tests that must OWN this agent's loop, and hands the slot
/// back when one is done.
///
/// The route is a single slot per agent, claimed for the life of the CLAIMING
/// THREAD (`agent_loop::claim_route`). Two libtest threads racing for it stall
/// each other for the whole retry window: the loser spins while the winner is
/// still alive holding a claim it gives up only at thread exit, which is after
/// the test body has returned. The lease makes that explicit — one owner at a
/// time — and [`OwnerLease::drop`] releases the route at the END OF THE TEST
/// rather than at thread exit, which is what lets the next holder have it.
#[cfg(test)]
static OWNER_LEASE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Proof that the holder owns this agent's loop, for as long as it is alive.
#[cfg(test)]
pub(crate) struct OwnerLease(Option<std::sync::MutexGuard<'static, ()>>);

#[cfg(test)]
impl Drop for OwnerLease {
    fn drop(&mut self) {
        // Give the route back BEFORE releasing the lease. The other order lets
        // the next holder start spinning against a claim this thread still
        // holds and has nothing left to do with.
        perry_runtime::event_pump::shutdown_agent_loop();
        self.0 = None;
    }
}

/// Drive turns until this thread is the agent's PUBLISHED loop owner.
///
/// `turnloop_net::available()` only CLAIMS the route — it answers "may I take a
/// loop?" without paying for one, so a thread that has merely asked holds the
/// slot with no `Poster` behind it and nothing can be posted to it. A turn is
/// what builds the loop and publishes the endpoint, so this drives one and
/// checks both halves.
#[cfg(test)]
#[must_use = "the lease must outlive the assertions ownership makes possible"]
pub(crate) fn become_the_owner_for_test() -> OwnerLease {
    let guard = OWNER_LEASE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let limit = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        perry_runtime::event_pump::js_loop_turn_bounded(0);
        if tl::available() && perry_ffi::agent_post::available() {
            return OwnerLease(Some(guard));
        }
        assert!(
            std::time::Instant::now() < limit,
            "this thread never became the primary agent's PUBLISHED loop owner, \
             so the rest of this test would prove nothing"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ── Submission ─────────────────────────────────────────────────────────────

/// Everything about a submission that is a property of the REQUEST rather than
/// of the thread that made it.
///
/// Computed *before* the transport is chosen, and that order is the point: a
/// refusal that belongs to the request must reach the caller with its own error.
/// The agent's owner would refuse an unsupported URL, an undrivable proxy or a
/// missing TLS config for exactly the same reason this thread would, and by the
/// time the job lands over there the caller has already been told the engine
/// took the request. Only
/// [`Declined::NoLoop`] is a property of the thread, and only that one is worth
/// posting.
struct Prepared {
    request: tlc::Request,
    proxy: Option<url::Url>,
}

fn prepare(spec: &RequestSpec) -> Result<Prepared, Declined> {
    let request = tlc::Request::new(&spec.url, &spec.method).map_err(Declined::Unsupported)?;
    let proxy = proxy_for(&request.url)?;
    if request.url.scheme() == "https" && exchange::tls_config().is_none() {
        return Err(Declined::NoTls);
    }
    Ok(Prepared { request, proxy })
}

/// Take the turnloop path for one outbound request.
///
/// `Err(Declined)` means the request was refused: nothing has been allocated,
/// no completion will arrive, and the caller must settle it itself. `Ok(())`
/// means the sink will be called exactly once.
///
/// # The thread that has no loop of its own
///
/// turnloop P9 gave every JS *agent* a loop, so a thread without one is not a
/// worker: it is a second thread acting for an agent another thread already
/// owns — Android's shape, where `perry-native` runs the compiled TypeScript
/// while the UI thread pumps for the same heap. P10 lets that thread hand the
/// whole submission to the owner ([`posted`]), which is a thread serving the
/// *same* JS heap, so the promise is settled where that agent's values live.
/// Only a genuine absence of a loop — a host where `Loop::new` failed —
/// is refused.
pub(crate) fn submit(spec: RequestSpec, sink: Sink) -> Result<(), Declined> {
    let direct = tl::available();
    if !direct && !posted::available() {
        // No loop anywhere for this agent. Refuse BEFORE preparing the request: `prepare` reaches
        // `tls_config()`, whose first call loads the platform root store.
        return Err(Declined::NoLoop);
    }
    let prepared = prepare(&spec)?;
    if direct {
        start_here(spec, sink, prepared)
    } else {
        posted::try_submit(spec, sink)
    }
}

/// The submission the agent's owner runs on behalf of a thread that had no
/// loop. Never posts — it is already on the owner, and a second hop would be a
/// bounce rather than a fallback.
fn submit_on_owner(spec: RequestSpec, sink: Sink) -> Result<(), Declined> {
    let prepared = prepare(&spec)?;
    if !tl::available() {
        return Err(Declined::NoLoop);
    }
    start_here(spec, sink, prepared)
}

/// Enter a prepared request into THIS thread's engine and start it.
fn start_here(spec: RequestSpec, sink: Sink, prepared: Prepared) -> Result<(), Declined> {
    let Prepared { request, proxy } = prepared;
    let id = ENGINE.with(|e| {
        let mut engine = e.borrow_mut();
        if !ensure_registered(&mut engine) {
            return None;
        }
        engine.next_req = engine.next_req.wrapping_add(1).max(1);
        let id = engine.next_req;
        let mut request = request;
        request.headers = spec
            .headers
            .iter()
            .map(|(name, value)| http1::Header::new(name, value.as_bytes()))
            .collect();
        if let Some(body) = spec.body.clone() {
            request.body = body;
        }
        if let Some(key) = spec.abort_key {
            engine.aborts.entry(key).or_default().push(id);
        }
        engine.requests.insert(
            id,
            Req {
                spec,
                sink,
                proxy,
                request,
                conn: None,
                head: None,
                body: Vec::new(),
                decoder: None,
                decoded: Vec::new(),
                redirected: false,
                streaming: false,
                retried: false,
                delivered: false,
            },
        );
        Some(id)
    });
    let Some(id) = id else {
        return Err(Declined::NoLoop);
    };
    SUBMITTED.fetch_add(1, Ordering::Relaxed);
    exchange::start(id);
    drain_pending();
    Ok(())
}

/// Cancel every request bound to `signal_ptr`. Called from the abort bridge on
/// the main thread when `controller.abort()` or an `AbortSignal.timeout`
/// deadline fires. A miss is a no-op.
///
/// A thread that posted its requests to the agent's owner holds none of them in
/// its own engine, so the abort is posted too — otherwise `controller.abort()`
/// would be silently inert for exactly the requests P10 moved.
pub(crate) fn abort_signal(signal_ptr: usize) -> usize {
    let ids = ENGINE.with(|e| {
        e.borrow_mut()
            .aborts
            .remove(&signal_ptr)
            .unwrap_or_default()
    });
    let n = ids.len();
    for id in ids {
        exchange::abort(id);
    }
    drain_pending();
    if !tl::available() {
        posted::try_abort(signal_ptr);
    }
    n
}

/// Cancel every request bound to `signal_ptr` in THIS thread's engine, without
/// posting. The owner's arm of [`abort_signal`].
fn abort_signal_here(signal_ptr: usize) {
    let ids = ENGINE.with(|e| {
        e.borrow_mut()
            .aborts
            .remove(&signal_ptr)
            .unwrap_or_default()
    });
    for id in ids {
        exchange::abort(id);
    }
    drain_pending();
}

/// Node's proxy environment, read through `turnloop_http`'s own matcher so the
/// `NO_PROXY` rules are the crate's rather than a second implementation.
pub(super) fn proxy_for(url: &url::Url) -> Result<Option<url::Url>, Declined> {
    // `undici.setGlobalDispatcher(new ProxyAgent(uri, token))` is a process-wide
    // override rather than an environment variable, and it wins over the
    // environment for every origin — undici's own rule, and what the reqwest
    // client that preceded this engine did (`fetch_client()` returned the proxied client
    // unconditionally once one was installed). `NO_PROXY` does not apply to it.
    if let Some((uri, token)) = global_dispatcher_proxy() {
        let mut parsed = url::Url::parse(&uri).map_err(|_| Declined::Proxy(INVALID_PROXY))?;
        if parsed.scheme() != "http" || parsed.host_str().is_none() {
            return Err(Declined::Proxy(HTTP_PROXIES_ONLY));
        }
        // undici's `token` is the literal `Proxy-Authorization` value. The
        // route derives that header from the proxy URL's userinfo, so a token
        // that is a `Basic <base64>` is folded back into the URL rather than
        // carried as a second channel.
        if let Some(token) = token {
            if let Some(encoded) = token.strip_prefix("Basic ") {
                if let Some((user, pass)) = decode_basic(encoded) {
                    let _ = parsed.set_username(&user);
                    let _ = parsed.set_password(Some(&pass));
                }
            }
        }
        return Ok(Some(parsed));
    }
    let env = tlc::ProxyEnvironment {
        http_proxy: var("HTTP_PROXY").or_else(|| var("http_proxy")),
        https_proxy: var("HTTPS_PROXY").or_else(|| var("https_proxy")),
        no_proxy: var("NO_PROXY")
            .or_else(|| var("no_proxy"))
            .unwrap_or_default(),
    };
    env.proxy_for(url).map_err(Declined::Proxy)
}

/// The policy layer's own two proxy refusals, reused for the
/// `setGlobalDispatcher` proxy so both sources reject identically.
const INVALID_PROXY: turnloop_http::Error =
    turnloop_http::Error::new("ERR_INVALID_URL", "invalid proxy");
const HTTP_PROXIES_ONLY: turnloop_http::Error =
    turnloop_http::Error::new("UND_ERR_NOT_SUPPORTED", "only HTTP proxies are supported");

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The process-wide `setGlobalDispatcher` proxy, as `(uri, token)`.
///
/// `cfg`-gated rather than reached through a seam: the store lives in `fetch`,
/// which is `web-fetch`'s, and `turnloop-http-client` can be enabled without it.
#[cfg(feature = "web-fetch")]
fn global_dispatcher_proxy() -> Option<(String, Option<String>)> {
    crate::fetch::global_dispatcher_proxy()
}

#[cfg(not(feature = "web-fetch"))]
fn global_dispatcher_proxy() -> Option<(String, Option<String>)> {
    None
}

/// Split a `Basic` credential back into user and password.
fn decode_basic(encoded: &str) -> Option<(String, String)> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let text = String::from_utf8(raw).ok()?;
    let (user, pass) = text.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

// ── Delivery ───────────────────────────────────────────────────────────────

/// Queue a sink call for `drain_pending`, marking the request delivered so a
/// later completion cannot call it twice (DESIGN D4).
fn deliver(engine: &mut Engine, id: u64, outcome: Outcome) {
    let Some(req) = engine.requests.get_mut(&id) else {
        return;
    };
    if req.delivered {
        return;
    }
    req.delivered = true;
    match &outcome {
        Outcome::Ok(_) => COMPLETED.fetch_add(1, Ordering::Relaxed),
        Outcome::Err(_) => FAILED.fetch_add(1, Ordering::Relaxed),
    };
    let sink = req.sink;
    if let Some(key) = req.spec.abort_key {
        if let Some(list) = engine.aborts.get_mut(&key) {
            list.retain(|other| *other != id);
            if list.is_empty() {
                engine.aborts.remove(&key);
            }
        }
    }
    engine.requests.remove(&id);
    engine.pending.push((sink, outcome));
}

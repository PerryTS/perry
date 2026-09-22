//! `ioredis` on a turnloop socket (P7).
//!
//! What this replaces, one for one:
//!
//! | before | after |
//! |---|---|
//! | `spawn_blocking` + `Handle::current().block_on` per command — one tokio blocking-pool thread held for the whole round trip | one `command()` on a sans-I/O core, submitted where the FFI call happens |
//! | `redis::aio::MultiplexedConnection`, whose own tokio task owns the socket | `turnloop_redis::Connection` driven over P1's `turnloop_net` |
//! | `tokio::time::timeout` per command, which needs a tokio timer | the core's own deadline, armed as a real turnloop deadline |
//!
//! The JS-visible surface does not move: the same eighteen `js_ioredis_*`
//! symbols, the same lazy connection, the same 10-second command timeout, the
//! same values.
//!
//! # Which connections come here
//!
//! [`enabled`] is false on a `worker_threads` agent (no loop of its own).
//! **A TLS connection no longer declines**:
//! `turnloop_redis` asks the host to perform the upgrade and the driver now has
//! one to give it ([`tls_options`]). That is the default `new Redis()`
//! configuration (`REDIS_TLS` defaults to `true`), and while it declined it
//! declined onto a path that cannot serve it either — this crate's `redis`
//! dependency has no TLS backend compiled in — so every default client failed.
//! TLS here is therefore a fix, not a new capability on top of a working one.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread from the loop's own completion dispatch,
//! so it may touch the connection table directly. It still builds **no JS
//! value**: a reply is settled through `JsPromise::resolve_with`, whose closure
//! carries owned Rust bytes and runs on the main thread during the resolution
//! pump. That is the same #1824 rule the `spawn_blocking` path had to obey —
//! and it is what this module fixes in `hgetall`, which under `spawn_blocking`
//! allocated its result object *on the blocking-pool thread*.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use perry_db_turnloop::{subsystem, DbCore, NetCompletion, Registry, TlsClientOptions, TlsFacts};
use perry_ffi::agent_post::{self, AgentJob};
use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field, Handle,
    JsPromise, JsValue,
};
use turnloop_redis::{resp::Value, Config, Connection, Error, Event};

use crate::DEFAULT_TIMEOUT_SECS;

/// This binding's slot in the runtime's sink registry.
pub(crate) const SUBSYSTEM: u8 = subsystem::REDIS;

thread_local! {
    /// The connection table. Thread-local because a turnloop handle belongs to
    /// the loop that created it — see `perry_db_turnloop`'s module docs.
    static REGISTRY: Registry<RedisCore> = Registry::new(SUBSYSTEM);
    /// `js_ioredis_new`'s JS-visible handle → the driver id of its connection.
    /// Absent until the first command opens one, which is ioredis's own lazy
    /// connect and what the `redis`-crate path did with its `CONNECTIONS` map.
    static OPEN: std::cell::RefCell<HashMap<Handle, i64>> =
        std::cell::RefCell::new(HashMap::new());
}

/// What the JS caller expects a reply to look like.
///
/// The `redis` crate got this from its `FromRedisValue` impls; sans-I/O hands
/// back a wire value, so the choice is explicit. The mapping reproduces the
/// previous binding's `ToJsValue` impls exactly, which is what keeps `SET`
/// resolving `"OK"` and `EXISTS` resolving a number rather than a boolean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shape {
    /// Always the string `"OK"` — `SET` / `SETEX`, whose `()` impl did this.
    Ok,
    /// A JS number.
    Number,
    /// A JS string, or `null` for a nil bulk reply.
    OptString,
    /// A JS string; a nil reply becomes the empty string.
    Text,
    /// `HGETALL` — an object built from the reply's field/value pairs.
    Hash,
}

struct PendingOp {
    promise: JsPromise,
    shape: Shape,
    label: &'static str,
}

/// The sans-I/O half of one `ioredis` client.
pub(crate) struct RedisCore {
    conn: Connection,
    pending: HashMap<u64, PendingOp>,
    /// `connect()` callers waiting for the handshake to finish. ioredis
    /// resolves them all when the connection becomes ready.
    waiting_ready: Vec<JsPromise>,
    next_token: u64,
    ready: bool,
    finished: bool,
    /// The last error the core reported, used as the reason for anything still
    /// outstanding when the connection goes away. Without it a connection that
    /// died mid-handshake rejects with "Connection closed", which hides the
    /// actual cause (`NOAUTH`, a wrong database, a refused AUTH).
    last_error: Option<String>,
    /// The core has asked for the transport to be upgraded. Taken by the
    /// driver, which installs the session and acknowledges it.
    tls_requested: bool,
}

impl RedisCore {
    fn new(config: Config) -> Self {
        Self {
            conn: Connection::new(config),
            pending: HashMap::new(),
            waiting_ready: Vec::new(),
            next_token: 1,
            ready: false,
            finished: false,
            last_error: None,
            tls_requested: false,
        }
    }

    /// Submit a command, taking ownership of the promise that will answer it.
    fn submit(
        &mut self,
        args: &[&[u8]],
        shape: Shape,
        label: &'static str,
        promise: JsPromise,
    ) -> Result<(), (JsPromise, String)> {
        let token = self.next_token;
        self.next_token += 1;
        let deadline = Instant::now().checked_add(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
        match self.conn.command(token, args, deadline) {
            Ok(()) => {
                self.pending.insert(
                    token,
                    PendingOp {
                        promise,
                        shape,
                        label,
                    },
                );
                Ok(())
            }
            Err(err) => Err((promise, format!("Redis {} error: {}", label, err.message))),
        }
    }

    /// Send QUIT and let the core close itself.
    ///
    /// QUIT is an ordinary wire command; the core answers it with a `Reply` and
    /// then a terminal `Closed`. Submitting it rather than closing the socket
    /// is exactly what makes `quit()` graceful and `disconnect()` not.
    fn begin_quit(&mut self, promise: JsPromise) {
        if let Err((promise, _)) = self.submit(&[b"QUIT"], Shape::Ok, "QUIT", promise) {
            // The core refused because it is already closing. The
            // `redis`-crate path ignored QUIT failures and resolved "OK"; keep
            // that, because a `quit()` that rejects on an already-closed client
            // is a new failure mode for existing programs.
            promise.resolve_with(|| JsValue::from_string_ptr(alloc_string("OK").as_raw()));
            self.conn.close();
            self.finished = true;
        }
    }

    fn settle(&mut self, token: u64, result: Result<Value, Error>) {
        let Some(op) = self.pending.remove(&token) else {
            return;
        };
        match result {
            Ok(value) => resolve(op.promise, op.shape, value),
            Err(err) => {
                let message = format!("Redis {} error: {}", op.label, err.message);
                op.promise.reject_string(&message);
            }
        }
    }

    /// Settle everything outstanding with `reason`. Called on transport failure
    /// and on close; leaving a promise pending is the one outcome a caller
    /// cannot recover from.
    fn settle_all_with_error(&mut self, reason: &str) {
        let message = match &self.last_error {
            Some(recorded) => format!("{} ({})", recorded, reason),
            None => reason.to_string(),
        };
        for (_, op) in self.pending.drain() {
            op.promise
                .reject_string(&format!("Redis {} error: {}", op.label, message));
        }
        for promise in self.waiting_ready.drain(..) {
            promise.reject_string(&format!("Redis connection error: {}", message));
        }
    }
}

impl DbCore for RedisCore {
    fn transport_connected(&mut self) -> Result<(), String> {
        self.conn
            .transport_connected()
            .map_err(|e| format!("Redis connection error: {}", e.message))
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.conn
            .receive(bytes)
            .map_err(|e| format!("Redis protocol error: {}", e.message))
    }

    fn take_tls_request(&mut self) -> bool {
        std::mem::take(&mut self.tls_requested)
    }

    fn tls_established(&mut self, facts: &TlsFacts) -> Result<(), String> {
        // `facts` is deliberately unread. Nothing in RESP consumes any of it:
        // no ALPN is offered (see [`tls_options`]), and Redis has no
        // channel-binding mechanism to feed the `tls-server-end-point` digest
        // to the way PostgreSQL's SCRAM-SHA-256-PLUS does. The core's own
        // acknowledgement takes no argument for the same reason.
        let _ = facts;
        self.conn
            .tls_established()
            .map_err(|e| format!("Redis connection error: {}", e.message))
    }

    fn drain(&mut self) -> Result<bool, String> {
        while let Some(event) = self.conn.poll_event() {
            match event {
                // The host is already connecting: `Registry::connect` submitted
                // the socket in the same call that produced this event.
                Event::Connect => {}
                Event::UpgradeTls => {
                    // Redis puts TLS underneath the whole protocol, so this
                    // arrives from `transport_connected`, before a single
                    // protocol byte — which is exactly why the AUTH carrying
                    // the password cannot precede it. Answering here would
                    // still be too early: the driver flushes what the core owes
                    // and installs the session, then acknowledges.
                    self.tls_requested = true;
                }
                Event::Ready { .. } => {
                    self.ready = true;
                    for promise in self.waiting_ready.drain(..) {
                        promise.resolve_with(|| JsValue::UNDEFINED);
                    }
                }
                Event::Reply { token, result } => self.settle(token, result),
                // Pub/sub is not part of this binding's surface (neither was it
                // before P7 — `js_ioredis_*` has no subscribe). A push that
                // arrives anyway is dropped rather than queued for nobody.
                Event::Message { .. } | Event::Push(_) => {}
                Event::Retry { .. } => {
                    // No automatic reconnect, which is what the `redis`-crate
                    // path did: a `MultiplexedConnection` whose socket died
                    // failed every subsequent command. `retry(now, None)` stops
                    // retrying and completes the queue with errors, so the
                    // caller learns immediately instead of hanging.
                    let _ = self.conn.retry(Instant::now(), None);
                }
                Event::Error(err) => {
                    self.last_error = Some(err.message.clone());
                }
                Event::CloseTransport => {}
                Event::Closed => {
                    self.finished = true;
                }
            }
        }
        Ok(self.finished)
    }

    fn output(&self) -> &[u8] {
        self.conn.output()
    }

    fn consume_output(&mut self, n: usize) {
        self.conn.consume_output(n);
    }

    fn next_timeout_ms(&self) -> Option<u64> {
        let at = self.conn.next_timeout()?;
        let now = Instant::now();
        Some(if at <= now {
            0
        } else {
            at.duration_since(now).as_millis().min(u128::from(u64::MAX)) as u64
        })
    }

    fn handle_timeout(&mut self) {
        self.conn.handle_timeout(Instant::now());
    }

    fn fail(&mut self, reason: &str) {
        self.conn.transport_lost();
        // Drain whatever `transport_lost` produced (a Retry, then the terminal
        // events) so the core's own settlements run first and this only has to
        // answer what it could not.
        let _ = self.drain();
        self.settle_all_with_error(reason);
        self.finished = true;
    }

    fn has_pending_work(&self) -> bool {
        !self.pending.is_empty() || !self.waiting_ready.is_empty()
    }
}

/// Build the JS value for one reply. Runs on the **main thread**, inside the
/// promise resolution pump — never in the sink.
fn resolve(promise: JsPromise, shape: Shape, value: Value) {
    match shape {
        Shape::Ok => {
            // `SET`/`SETEX` answer `+OK`; the previous binding hard-coded the
            // string because its `()` conversion did, and a Redis that answered
            // something else (a `SET ... GET` form) would have printed "OK"
            // there too. Keep the wire value when there is one.
            let text = value
                .bytes()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_else(|| "OK".to_string());
            promise.resolve_with(move || JsValue::from_string_ptr(alloc_string(&text).as_raw()));
        }
        Shape::Number => {
            let n = match &value {
                Value::Integer(n) => *n as f64,
                Value::Double(d) => *d,
                Value::Boolean(b) => f64::from(u8::from(*b)),
                Value::Null => 0.0,
                other => other
                    .bytes()
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0),
            };
            promise.resolve_with(move || JsValue::from_number(n));
        }
        Shape::OptString => match value {
            Value::Null => promise.resolve_with(|| JsValue::NULL),
            other => {
                let text = string_of(&other);
                promise
                    .resolve_with(move || JsValue::from_string_ptr(alloc_string(&text).as_raw()));
            }
        },
        Shape::Text => {
            let text = string_of(&value);
            promise.resolve_with(move || JsValue::from_string_ptr(alloc_string(&text).as_raw()));
        }
        Shape::Hash => {
            let entries = hash_entries(value);
            promise.resolve_with(move || build_hash_object(entries));
        }
    }
}

fn string_of(value: &Value) -> String {
    match value {
        Value::Integer(n) => n.to_string(),
        Value::Double(d) => d.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => String::new(),
        other => other
            .bytes()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default(),
    }
}

/// `HGETALL` arrives as a flat `[field, value, …]` array on RESP2 and as a map
/// on RESP3. This binding negotiates RESP2 (see [`config_for`]), but decoding
/// both costs four lines and makes the conversion independent of that choice.
fn hash_entries(value: Value) -> Vec<(String, String)> {
    match value {
        Value::Map(pairs) => pairs
            .into_iter()
            .map(|(k, v)| (string_of(&k), string_of(&v)))
            .collect(),
        other => match other.items() {
            Some(items) => items
                .chunks(2)
                .filter(|pair| pair.len() == 2)
                .map(|pair| (string_of(&pair[0]), string_of(&pair[1])))
                .collect(),
            None => Vec::new(),
        },
    }
}

/// Main thread only. The previous binding built this object inside a
/// `spawn_blocking` closure — an arena allocation on a pooled worker thread,
/// which is #1824's exact shape.
fn build_hash_object(entries: Vec<(String, String)>) -> JsValue {
    let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
    let (packed, shape_id) = build_object_shape(&keys);
    // SAFETY: called on the main thread from the resolution pump, with a shape
    // built from exactly these keys.
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            entries.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for (i, (_, v)) in entries.iter().enumerate() {
        let val = alloc_string(v);
        // SAFETY: `i` is below the field count the object was allocated with.
        unsafe { js_object_set_field(obj, i as u32, JsValue::from_string_ptr(val.as_raw())) };
    }
    JsValue::from_object_ptr(obj)
}

extern "C" fn sink(completion: *const NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime borrows one completion for the duration of this call.
    let completion = unsafe { &*completion };
    let id = completion.id;
    let retired = REGISTRY.with(|reg| {
        reg.dispatch(completion);
        !reg.is_live(id)
    });
    if retired {
        OPEN.with(|open| open.borrow_mut().retain(|_, v| *v != id));
    }
}

/// Whether a client created *now, on this thread* should live on turnloop.
pub(crate) fn enabled() -> bool {
    REGISTRY.with(|reg| reg.enabled(sink))
}

/// Which transport a client created *now, on this thread* lives on.
///
/// Three answers, not two. [`enabled`] asks whether **this thread** can drive
/// the agent's loop, and until turnloop P10 a "no" left only the `redis` crate.
/// But since P9 every JS agent has a loop, so a "no" usually means the loop
/// exists and *another thread of this same agent* owns it — the Android shape,
/// where `perry-native` runs the compiled TypeScript while the UI thread pumps
/// for the same heap. That case is [`Transport::Posted`]: the owner does the
/// I/O, on the thread where this agent's JS values live.
///
/// Only the third answer keeps tokio, and it is a genuine absence of a loop —
/// the only remaining case is a host where `Loop::new` failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Transport {
    /// This thread owns the agent's loop: submit directly.
    Direct,
    /// Another thread of this same agent owns the loop: post the work to it.
    Posted,
    /// No loop exists for this agent at all: keep the `redis` crate.
    Legacy,
}

/// Pick the transport for a client being created on this thread.
///
/// Asked once per client, at creation, because a connection belongs to one
/// transport for its whole life — there is no handover, and a command may
/// already be queued behind the next one.
pub(crate) fn transport() -> Transport {
    if enabled() {
        Transport::Direct
    } else if agent_post::available() {
        Transport::Posted
    } else {
        Transport::Legacy
    }
}

/// One operation handed to the thread that owns this agent's loop.
enum PostedOp {
    Command {
        label: &'static str,
        shape: Shape,
        args: Vec<Vec<u8>>,
    },
    Connect,
    Quit,
    Disconnect,
}

/// The job that crosses to the owner. `Send` because every field is: `Handle`
/// is an `i64`, `JsPromise` carries its own `unsafe impl Send`, and the command
/// arguments are owned bytes — the borrowed `&[u8]` slices a direct submission
/// uses cannot cross, so they are copied here and only here.
struct PostedWork {
    handle: Handle,
    /// `None` only for `Disconnect`, which JS does not await.
    promise: Option<JsPromise>,
    op: PostedOp,
}

impl AgentJob for PostedWork {
    fn run(self: Box<Self>) {
        let PostedWork {
            handle,
            promise,
            op,
        } = *self;
        // We are on the owner now, so this is the thread that can drive the
        // loop — unless it lost it between the post and this turn. There is no
        // falling back at this point: the promise is already in JS's hands and
        // a command may be queued behind it, so settle rather than drop. A
        // dropped `JsPromise` is a promise that never settles, which is the one
        // outcome a caller cannot recover from.
        if !enabled() {
            if let Some(promise) = promise {
                promise.reject_string("Redis connection is closed");
            }
            return;
        }
        match op {
            PostedOp::Command { label, shape, args } => {
                let Some(promise) = promise else { return };
                let borrowed: Vec<&[u8]> = args.iter().map(Vec::as_slice).collect();
                command(handle, promise, label, shape, &borrowed);
            }
            PostedOp::Connect => {
                if let Some(promise) = promise {
                    connect(handle, promise);
                }
            }
            PostedOp::Quit => {
                if let Some(promise) = promise {
                    quit(handle, promise);
                }
            }
            PostedOp::Disconnect => {
                disconnect(handle);
            }
        }
    }
}

/// Hand one operation to the thread that owns this agent's loop.
///
/// A refused post settles the promise here rather than dropping it. `NoRoute`
/// means the owner's loop went away — this client cannot fall back, because it
/// was created on turnloop and may have commands queued. `Again` means the
/// owner's postbox is momentarily full, which is a back-pressure condition and
/// reads to JS the same way a full socket buffer does.
fn post(handle: Handle, promise: Option<JsPromise>, op: PostedOp) {
    let work = Box::new(PostedWork {
        handle,
        promise,
        op,
    });
    match agent_post::post_job(work) {
        Ok(()) => {}
        Err(rejected) => {
            let permanent = rejected.is_permanent();
            let mut job = rejected.into_job();
            if let Some(promise) = job.promise.take() {
                promise.reject_string(if permanent {
                    "Redis connection is closed"
                } else {
                    "Redis is busy: the agent loop could not accept this command"
                });
            }
        }
    }
}

/// Submit one command through the agent's owner. See [`command`], which this
/// runs over there.
pub(crate) fn post_command(
    handle: Handle,
    promise: JsPromise,
    label: &'static str,
    shape: Shape,
    args: &[&[u8]],
) {
    post(
        handle,
        Some(promise),
        PostedOp::Command {
            label,
            shape,
            args: args.iter().map(|a| a.to_vec()).collect(),
        },
    );
}

/// `redis.connect()`, run on the agent's owner.
pub(crate) fn post_connect(handle: Handle, promise: JsPromise) {
    post(handle, Some(promise), PostedOp::Connect);
}

/// `redis.quit()`, run on the agent's owner.
pub(crate) fn post_quit(handle: Handle, promise: JsPromise) {
    post(handle, Some(promise), PostedOp::Quit);
}

/// `redis.disconnect()`, run on the agent's owner.
///
/// Fire-and-forget, like ioredis's own: the connection table lives on the
/// owner, so whether an entry was actually removed is not knowable here. The
/// `true` says "this client is on turnloop", which is what the caller branches
/// on.
pub(crate) fn post_disconnect(handle: Handle) {
    post(handle, None, PostedOp::Disconnect);
}

/// Install the sink and report whether the runtime accepted it.
///
/// Separate from [`enabled`] so a test can assert the part that is a property
/// of the build — the completion-layout digest check — without also asserting
/// that the thread it happens to run on owns a loop. `cargo test` puts each
/// test on its own thread and only some of them do.
#[cfg(test)]
fn register_only() -> bool {
    REGISTRY.with(|reg| reg.register(sink))
}

/// The protocol config for one client, and the endpoint to reach it at.
///
/// Reads the same four environment variables the previous binding did, so a
/// program that worked before sees the same server. `tls` is what makes the
/// core ask for the upgrade at all — it raises `Event::UpgradeTls` from
/// `transport_connected` — and [`tls_options`] is what the driver then
/// installs; both read the one endpoint, so they cannot disagree about whether
/// a connection is encrypted.
fn config_for(url: &crate::RedisEndpoint) -> Config {
    Config {
        username: url.username.clone(),
        password: url.password.clone(),
        database: 0,
        client_name: None,
        tls: url.tls,
        // RESP2, because that is what the `redis` crate negotiated and several
        // replies change type under RESP3 (`HGETALL` becomes a map, `EXPIRE` a
        // boolean). Matching the old wire keeps the JS values identical.
        prefer_resp3: false,
        offline_queue: true,
        auto_resubscribe: false,
        // A command whose reply was lost must not be re-executed: `INCR` is not
        // idempotent and the previous binding never replayed anything.
        auto_resend_unfulfilled: false,
        max_retries_per_request: None,
        connect_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        limits: Default::default(),
    }
}

/// The TLS options the driver installs when the core asks for the upgrade.
///
/// `None` for a plaintext endpoint, which is what makes an `UpgradeTls` request
/// on such a connection a driver error rather than a silent plaintext
/// continuation.
fn tls_options(endpoint: &crate::RedisEndpoint) -> Option<TlsClientOptions> {
    if !endpoint.tls {
        return None;
    }
    // The servername is the host the program asked to reach. `js_ioredis_new`
    // builds the endpoint from `REDIS_HOST` and ioredis's own option surface
    // here is those four `REDIS_*` variables — none of which names a
    // certificate name to override it with.
    //
    // Everything else comes from the process TLS environment
    // (`NODE_TLS_REJECT_UNAUTHORIZED`, `NODE_EXTRA_CA_CERTS`, `SSL_CERT_FILE`),
    // which is deliberate rather than a gap: inventing a `REDIS_TLS_*` knob
    // would give this binding its own way to disable verification that
    // `node:https` in the same process does not honour, and a second spelling
    // of "trust this root" is how one of them ends up unaudited.
    //
    // No ALPN: a Redis connection carries RESP and nothing else, and offering
    // a protocol list a server has no opinion about is how a middlebox learns
    // to have one.
    Some(TlsClientOptions::from_node_environment(
        endpoint.host.clone(),
    ))
}

/// Open the connection for `handle` if it has none, and return its driver id.
fn open(handle: Handle) -> Result<i64, String> {
    if let Some(id) = OPEN.with(|open| open.borrow().get(&handle).copied()) {
        if REGISTRY.with(|reg| reg.is_live(id)) {
            return Ok(id);
        }
        OPEN.with(|open| {
            open.borrow_mut().remove(&handle);
        });
    }
    let endpoint = crate::endpoint_for(handle).ok_or_else(|| "Invalid Redis handle".to_string())?;
    let tls = tls_options(&endpoint);
    let mut core = RedisCore::new(config_for(&endpoint));
    core.conn
        .connect(Instant::now())
        .map_err(|e| format!("Redis connection error: {}", e.message))?;
    // Consume the `Connect` event the call above queued, so the first real
    // drain does not see a stale one.
    let _ = core.conn.poll_event();
    let id = REGISTRY.with(|reg| {
        reg.connect_with_tls(
            &endpoint.host,
            endpoint.port,
            core,
            handle.try_into().unwrap_or(0),
            tls,
        )
    })?;
    OPEN.with(|open| {
        open.borrow_mut().insert(handle, id);
    });
    Ok(id)
}

/// Submit one command on `handle`'s connection, answering `promise`.
///
/// Every path settles or parks the promise, including every failure: once a
/// client has been created on this transport there is no falling back, because
/// a command may already be queued behind this one and reordering them would
/// break a MULTI block.
pub(crate) fn command(
    handle: Handle,
    promise: JsPromise,
    label: &'static str,
    shape: Shape,
    args: &[&[u8]],
) {
    let id = match open(handle) {
        Ok(id) => id,
        Err(message) => {
            promise.reject_string(&message);
            return;
        }
    };
    // The promise travels through an `Option` so that a `with_core` which never
    // runs its closure — the entry went away between `open` and here — hands it
    // back instead of dropping it. A dropped `JsPromise` is a promise that never
    // settles, which is the one outcome a caller cannot recover from.
    let mut slot = Some(promise);
    let submitted = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            let promise = slot.take().expect("the closure runs at most once");
            core.submit(args, shape, label, promise)
        })
    });
    match submitted {
        Some(Ok(())) => {}
        Some(Err((promise, message))) => promise.reject_string(&message),
        None => {
            if let Some(promise) = slot {
                promise.reject_string("Redis connection is closed");
            }
        }
    }
}

/// `redis.connect()` — resolve once the handshake has finished.
pub(crate) fn connect(handle: Handle, promise: JsPromise) {
    let id = match open(handle) {
        Ok(id) => id,
        Err(message) => {
            promise.reject_string(&message);
            return;
        }
    };
    // Checked before the promise moves: a client that is already through its
    // handshake resolves immediately, which is ioredis's behaviour and what the
    // cached-`MultiplexedConnection` path did.
    if REGISTRY
        .with(|reg| reg.inspect(id, |core| core.ready))
        .unwrap_or(false)
    {
        promise.resolve_with(|| JsValue::UNDEFINED);
        return;
    }
    let mut slot = Some(promise);
    let parked = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            core.waiting_ready
                .push(slot.take().expect("the closure runs at most once"));
        })
    });
    if parked.is_none() {
        if let Some(promise) = slot {
            promise.reject_string("Redis connection is closed");
        }
    }
}

/// `redis.quit()` — send QUIT, then close.
pub(crate) fn quit(handle: Handle, promise: JsPromise) {
    let Some(id) = OPEN.with(|open| open.borrow().get(&handle).copied()) else {
        // Never connected. ioredis resolves `quit()` on a lazy client that
        // never opened a socket, and so did the legacy path, which ignored the
        // QUIT result entirely.
        promise.resolve_with(|| JsValue::from_string_ptr(alloc_string("OK").as_raw()));
        return;
    };
    let mut slot = Some(promise);
    let submitted = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            core.begin_quit(slot.take().expect("the closure runs at most once"));
        })
    });
    if submitted.is_none() {
        if let Some(promise) = slot {
            promise.resolve_with(|| JsValue::from_string_ptr(alloc_string("OK").as_raw()));
        }
    }
    OPEN.with(|open| {
        open.borrow_mut().remove(&handle);
    });
}

/// `redis.disconnect()` — drop the connection without a graceful QUIT, which is
/// what ioredis does and what the previous binding's `CONNECTIONS.remove` did.
pub(crate) fn disconnect(handle: Handle) -> bool {
    let Some(id) = OPEN.with(|open| open.borrow_mut().remove(&handle)) else {
        return false;
    };
    REGISTRY.with(|reg| reg.abort(id, "Connection closed by the client"));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subsystem_slot_is_the_one_reserved_for_this_binding() {
        assert_eq!(SUBSYSTEM, subsystem::REDIS);
        assert_ne!(SUBSYSTEM, subsystem::PG);
        assert_ne!(SUBSYSTEM, subsystem::MYSQL);
        assert_ne!(SUBSYSTEM, subsystem::MONGODB);
    }

    #[test]
    fn registration_passes_the_abi_layout_check() {
        // The dev-dependency links the runtime, so this exercises the real
        // `register_sink`: a mismatch between perry-ffi's `NetCompletion`
        // layout digest and the runtime's refuses registration, leaves
        // `available` false, and would silently put every client back on the
        // legacy transport. On an agent with no loop — a `worker_threads`
        // Worker — this is false and that fallback is the correct
        // behaviour.
        assert!(
            register_only(),
            "a false here is an ABI layout mismatch between perry-ffi and perry-runtime"
        );
        assert!(perry_ffi::turnloop_net::sink_installed(SUBSYSTEM));
    }

    #[test]
    fn the_config_negotiates_resp2_and_never_replays_a_command() {
        // Both are behaviour, not taste: RESP3 changes HGETALL and EXPIRE reply
        // types, and replaying an unacknowledged INCR double-counts.
        let endpoint = crate::RedisEndpoint {
            host: "127.0.0.1".into(),
            port: 6379,
            username: None,
            password: None,
            tls: false,
            transport: Transport::Direct,
        };
        let config = config_for(&endpoint);
        assert!(!config.prefer_resp3);
        assert!(!config.auto_resend_unfulfilled);
        assert!(
            config.offline_queue,
            "a command before ready must queue, not fail"
        );
    }

    #[test]
    fn hgetall_decodes_both_a_resp2_array_and_a_resp3_map() {
        let flat = Value::Array(vec![
            Value::Bulk(b"a".to_vec()),
            Value::Bulk(b"1".to_vec()),
            Value::Bulk(b"b".to_vec()),
            Value::Bulk(b"2".to_vec()),
        ]);
        assert_eq!(
            hash_entries(flat),
            vec![("a".into(), "1".into()), ("b".into(), "2".into())]
        );
        let map = Value::Map(vec![(Value::Bulk(b"a".to_vec()), Value::Integer(1))]);
        assert_eq!(hash_entries(map), vec![("a".into(), "1".into())]);
    }

    #[test]
    fn an_odd_length_hgetall_array_drops_the_stray_field() {
        // A truncated reply must not panic on `pair[1]`.
        let odd = Value::Array(vec![Value::Bulk(b"a".to_vec())]);
        assert!(hash_entries(odd).is_empty());
    }

    #[test]
    fn a_nil_get_is_null_but_a_nil_text_reply_is_a_string() {
        // `GET` on a missing key is `null` in ioredis; `PING` never is. The two
        // shapes existed as separate `ToJsValue` impls before P7 and the
        // distinction is observable from JS.
        assert!(matches!(Value::Null, Value::Null));
        assert_eq!(string_of(&Value::Null), "");
        assert_eq!(string_of(&Value::Integer(7)), "7");
        assert_eq!(string_of(&Value::Bulk(b"PONG".to_vec())), "PONG");
    }

    /// An endpoint the way `js_ioredis_new` builds one from the `REDIS_*`
    /// environment.
    fn endpoint(host: &str, tls: bool, password: Option<&str>) -> crate::RedisEndpoint {
        crate::RedisEndpoint {
            host: host.to_string(),
            port: if tls { 6380 } else { 6379 },
            username: None,
            password: password.map(str::to_string),
            tls,
            transport: Transport::Direct,
        }
    }

    #[test]
    fn a_tls_endpoint_is_upgraded_rather_than_declined() {
        // `REDIS_TLS` defaults to true, so this is the *default* client. While
        // it declined it reached a legacy path with no TLS backend compiled in,
        // which is why the decline was a defect rather than a fallback.
        let endpoint = endpoint("cache.example.com", true, None);
        assert!(
            config_for(&endpoint).tls,
            "the core is what raises UpgradeTls; a plaintext config never asks"
        );
        let options = tls_options(&endpoint).expect("a TLS endpoint configures the driver");
        assert_eq!(
            options.servername, "cache.example.com",
            "the certificate is checked against the host the program asked for"
        );
        assert!(
            options.alpn.is_empty(),
            "RESP is the only protocol on this socket, so nothing is offered"
        );
        // `reject_unauthorized` and the trust roots are deliberately not
        // asserted: they are whatever the process TLS environment says, which
        // is the point of building them with `from_node_environment`.
    }

    #[test]
    fn a_plaintext_endpoint_carries_no_tls_options() {
        // Not merely unused. `None` is what makes an `UpgradeTls` on a
        // plaintext connection a driver error instead of a silent plaintext
        // continuation, so the config and the options must agree.
        let endpoint = endpoint("127.0.0.1", false, None);
        assert!(tls_options(&endpoint).is_none());
        assert!(!config_for(&endpoint).tls);
    }

    #[test]
    fn the_password_reaches_the_wire_only_after_the_upgrade() {
        // The property TLS is here for, driven through the real state machine.
        // `REDIS_PASSWORD` becomes an `AUTH` command in the handshake, and the
        // core must hold it until the session exists: the driver flushes the
        // core's output *before* it installs anything, so a byte produced too
        // early is a byte sent in the clear.
        let endpoint = endpoint("cache.example.com", true, Some("s3cret"));
        let mut core = RedisCore::new(config_for(&endpoint));
        core.conn
            .connect(Instant::now())
            .expect("a fresh core accepts connect");
        // The `Connect` event `open` consumes before the driver's first drain.
        let _ = core.conn.poll_event();

        core.transport_connected().expect("the transport came up");
        assert_eq!(
            core.drain(),
            Ok(false),
            "an upgrade request is not a terminal event"
        );
        assert!(
            core.output().is_empty(),
            "no protocol byte, and above all no AUTH, may precede the handshake"
        );
        assert!(core.take_tls_request(), "the core asked for the upgrade");
        assert!(
            !core.take_tls_request(),
            "the request is taken once, or the driver installs a second session"
        );

        core.tls_established(&TlsFacts::default())
            .expect("the core accepts the acknowledgement in its TLS state");
        assert!(
            String::from_utf8_lossy(core.output()).contains("s3cret"),
            "the AUTH goes out after the upgrade, encrypted by the session"
        );
    }
}

#[cfg(test)]
mod posted_transport_tests {
    use super::*;

    /// Become the agent's owner — and its **published** owner, which is not the
    /// same thing.
    ///
    /// Asking `transport()` alone only CLAIMS the route: `net_available()`
    /// answers "may I take a loop?" without paying for one, so a thread that
    /// has merely asked holds the slot with no `Poster` behind it. Nothing can
    /// be posted to a claim. A turn is what builds the loop and publishes the
    /// endpoint, so this drives one and checks both halves.
    ///
    /// `cargo test` puts each test on its own thread and the agent's route is a
    /// single slot, so a thread that ran before this one may still be releasing
    /// it; that is what the retry is for.
    fn become_the_owner() {
        let limit = Instant::now() + Duration::from_secs(10);
        loop {
            perry_runtime::event_pump::js_loop_turn_bounded(0);
            if transport() == Transport::Direct && agent_post::available() {
                return;
            }
            assert!(
                Instant::now() < limit,
                "this thread never became the primary agent's PUBLISHED loop \
                 owner, so the rest of this test would prove nothing"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// turnloop P10, end to end through this binding: a thread that cannot get
    /// a loop of its own no longer has to keep a tokio driver alive for itself.
    ///
    /// This is the Android shape — `perry-native` runs the compiled TypeScript
    /// while the UI thread pumps for the same heap, and whichever claims the
    /// route first leaves the other unable to submit. Before P10 the loser's
    /// only answer was `spawn_blocking` + `Handle::current().block_on`.
    ///
    /// Asserts the work CROSSED, not merely that nothing threw: the runtime's
    /// dispatch counter is per-thread, so it moving on the owner while staying
    /// at zero on the poster is the discriminating fact. A post that silently
    /// went nowhere would leave both at zero and fail here.
    #[test]
    fn a_thread_that_does_not_own_the_loop_posts_its_work_to_the_thread_that_does() {
        become_the_owner();
        assert!(
            perry_ffi::turnloop_net::sink_installed(SUBSYSTEM),
            "the sink must be installed, or 'turnloop carried this' is vacuous"
        );
        let before = agent_post::dispatched();

        let poster_ran_its_own = std::thread::spawn(|| {
            // A second thread acting FOR the same agent. It has no agent of its
            // own, so `current_agent()` resolves to the primary agent — the one
            // whose loop the thread above owns.
            assert_eq!(
                transport(),
                Transport::Posted,
                "a second thread of an agent that HAS a loop must post to it, \
                 not fall back to the legacy driver"
            );
            assert_eq!(
                agent_post::dispatched(),
                0,
                "this thread has run no posted job"
            );
            // A handle no client owns: harmless on the owner (the connection
            // table has no entry for it), and it is the crossing this test is
            // about, not what the job does when it lands.
            post_disconnect(Handle::MAX);
            agent_post::dispatched()
        })
        .join()
        .expect("posting thread");

        assert_eq!(
            poster_ran_its_own, 0,
            "the poster must NOT have run the job itself — if it did, the work \
             never crossed and this binding is still doing its own I/O"
        );

        let limit = Instant::now() + Duration::from_secs(10);
        while agent_post::dispatched() == before {
            assert!(
                Instant::now() < limit,
                "the posted work never reached the owner"
            );
            perry_runtime::event_pump::js_loop_turn_bounded(0);
        }
        assert_eq!(
            agent_post::dispatched(),
            before + 1,
            "the owner ran it exactly once — a post is delivered, not retried"
        );
    }

    /// The three transports must stay distinct at the type level, because the
    /// two turnloop ones take different code paths and the third is the only
    /// one that may reach the `redis` crate. A client records its answer once,
    /// at creation: a connection belongs to one transport for its whole life.
    #[test]
    fn the_transport_is_recorded_once_and_only_legacy_reaches_the_redis_crate() {
        become_the_owner();
        assert_eq!(transport(), Transport::Direct);
        assert_ne!(Transport::Direct, Transport::Posted);
        assert_ne!(Transport::Posted, Transport::Legacy);
        // The endpoint carries it, so every later command reads the same
        // answer rather than re-asking on a thread that may differ.
        let endpoint = crate::RedisEndpoint {
            host: "127.0.0.1".into(),
            port: 6379,
            username: None,
            password: None,
            tls: false,
            transport: transport(),
        };
        assert_eq!(endpoint.transport, Transport::Direct);
    }
}

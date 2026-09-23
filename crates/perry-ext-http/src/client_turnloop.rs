//! The `node:http` client, on turnloop — lane 1.
//!
//! [`try_dispatch`] is called from `dispatch_request_snapshot` immediately
//! before the reqwest path. `true` means this module accepted the exchange and
//! will deliver exactly one terminal [`PendingHttpEvent`] for the request;
//! `false` means it declined and the caller must run its existing reqwest
//! future. That is the same coexistence rule `fetch`'s
//! `perry-stdlib/src/fetch/turnloop_bridge.rs` applies, and the reason
//! `reqwest` is still a dependency of this crate.
//!
//! # What this lane covers, and what declines
//!
//! Every decline below is a *named* condition, not a catch-all. Each one is a
//! later lane; see `docs/turnloop/` and the changelog fragment for the order.
//!
//! * **No loop on this thread** — `tl::available` is false (a host where
//!   `Loop::new` failed). Nothing else can be done here.
//! * **Not cleartext `http:`** — `https:` needs the TLS session layer
//!   (`perry-tls-session`, the way `perry-ext-ws`'s client drives it). Lane 3.
//! * **An explicit `Agent`** — `agent_handle != 0`. The admission engine in
//!   `agent.rs` (per-origin FIFO queue, `maxSockets`, `maxTotalSockets`,
//!   `maxFreeSockets`) already runs *above* the transport and is unchanged by
//!   this lane, but an Agent also selects a pooled reqwest client whose
//!   keep-alive this lane does not yet provide. Lane 4.
//! * **A request body** — upload framing (`BodyLength::Known`/`Chunked`,
//!   `send_body`, `'continue'`) is lane 2.
//! * **A per-request deadline** — `options.timeout` / `req.setTimeout`. The
//!   `Lifecycle` deadlines are real in `turnloop_http` but need the timer arm
//!   wired to `tl::timer_arm`; lane 2.
//! * **A proxy** — `NODE_USE_ENV_PROXY=1` selects `Route`'s CONNECT tunnel.
//!   Lane 5.
//! * **A request the codec refuses** — `client::Request::new` rejects a URL
//!   with embedded credentials and the `CONNECT`/`TRACE`/`TRACK` methods (a
//!   *fetch* normalization rule that `node:http` does not share). Declining
//!   rather than failing keeps today's behaviour for those exactly.
//! * **An explicit `Host` header** — `client::Request::head` drops a caller's
//!   `host` and substitutes the URL authority, which is Fetch's rule and not
//!   `node:http`'s. reqwest sends what the caller set, so routing these here
//!   would silently rewrite them.
//! * **A header with its own transport** — `TE: trailers` (`plain_client.rs`),
//!   `Connection: Upgrade` (`client_upgrade.rs`) and `Expect: 100-continue`
//!   (`continue_client.rs`) each have a raw-socket bypass today. This lane
//!   declines all three using the *same* predicates those modules trigger on,
//!   so the routing cannot disagree with itself.
//!
//! # Redirects
//!
//! There is deliberately no redirect handling here. Node's `http.request` /
//! `https.get` never follow a 3xx — the reqwest path spells that as
//! `redirect::Policy::none()` (`lib.rs`), and driving `Http1Connection`
//! directly gives it for free: the 3xx head and body are delivered verbatim.
//!
//! # Keep-alive
//!
//! Not in this lane. Every exchange gets its own connection and closes it once
//! `Event::End` has been observed. `turnloop_http::client::Pool` is the
//! mechanism for the next lane, and the ordering it demands — release *only*
//! after End, with `conn.reusable()` — is the one hazard worth isolating in a
//! change of its own, because getting it wrong hands a socket to the next
//! request mid-message and misattributes framing.
//!
//! This costs a connection per request on the covered set. It is invisible to
//! JS: `req.reusedSocket` and `agent.sockets` / `agent.freeSockets` are fed by
//! `agent.rs`'s *facade* pool, which is already decoupled from the physical
//! connection (reqwest owned that, and JS never saw it).
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread, from the loop's own turn. It runs no JS:
//! every outcome goes onto `HTTP_PENDING_EVENTS` and is dispatched by
//! `js_http_process_pending` on its own tick, exactly as the reqwest task's
//! did. Nothing here holds a JS value — a request is an owned `String`/`Vec`
//! copied before submission, and the only handle stored is the numeric
//! `Handle` the drain looks up — so there is no GC root to register and
//! `scan_http_roots` is unchanged.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use bytes::Bytes;
use perry_ffi::turnloop_net as tl;
use perry_ffi::Handle;
use turnloop_http::client::{Http1Connection, Request};
use turnloop_http::http1;

use crate::{push_event, ClientInflightGuard, PendingHttpEvent};

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

/// Exchanges this lane has ACCEPTED, and ones it has carried to a clean
/// `Event::End`.
///
/// These exist because a decline is invisible: a lane that silently returned
/// `false` for every request would leave the JS surface behaving exactly as it
/// does today, and every test over it would stay green having exercised
/// nothing — the "gate runs but its subject never did" shape. A test that
/// asserts `completed_total()` moved is asserting the subject was live.
static ACCEPTED: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Exchanges handed to turnloop rather than declined to reqwest.
pub fn accepted_total() -> u64 {
    ACCEPTED.load(Ordering::Relaxed)
}

/// Exchanges whose response was decoded through to `Event::End`.
pub fn completed_total() -> u64 {
    COMPLETED.load(Ordering::Relaxed)
}

/// One in-flight exchange: a connection this module opened, and the request it
/// is carrying.
struct Exchange {
    /// The `ClientRequestHandle` every event is addressed to.
    request_handle: Handle,
    /// The sans-I/O HTTP/1.1 driver. Owns framing, not the socket.
    conn: Http1Connection,
    /// Bytes to put on the wire once `NET_CONNECT` arrives. The head is
    /// serialized at submit time so a failure to build it declines rather than
    /// stranding a connected socket.
    pending_head: bool,
    /// Set once a terminal event has been pushed, so the teardown edges
    /// (`NET_EOF`, `NET_ERROR`, `NET_CLOSED`) cannot push a second one.
    settled: bool,
    /// Keeps the process alive across the exchange and re-arms the event-loop
    /// tick on drop — the same guard the reqwest task holds.
    _inflight: ClientInflightGuard,
}

fn exchanges() -> &'static Mutex<HashMap<i64, Exchange>> {
    static EXCHANGES: OnceLock<Mutex<HashMap<i64, Exchange>>> = OnceLock::new();
    EXCHANGES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with_exchange<R>(id: i64, f: impl FnOnce(&mut Exchange) -> R) -> Option<R> {
    let mut guard = exchanges().lock().unwrap_or_else(|e| e.into_inner());
    guard.get_mut(&id).map(f)
}

fn forget(id: i64) -> Option<Exchange> {
    let mut guard = exchanges().lock().unwrap_or_else(|e| e.into_inner());
    guard.remove(&id)
}

// ── Ids ─────────────────────────────────────────────────────────────────────

/// One authoritative id domain for the connections this module opens. The
/// runtime keys its handle table by this id across every subsystem, so it has
/// to be globally unique — which is why this is a reserved domain rather than
/// a private counter.
fn registry_domain() -> perry_ffi::NativeRegistryDomain {
    static DOMAIN: OnceLock<perry_ffi::NativeRegistryDomain> = OnceLock::new();
    *DOMAIN.get_or_init(|| {
        perry_ffi::NativeRegistryDomain::new().expect("http client registry domains exhausted")
    })
}

/// This subsystem accepts nothing — it only dials. Returning zero refuses,
/// which is the right answer for an accept that cannot happen.
extern "C" fn alloc_id() -> i64 {
    0
}

// ── Availability ────────────────────────────────────────────────────────────

/// Whether a request issued *now, on this thread* can live on turnloop.
///
/// Deliberately not cached: availability is a property of the calling agent,
/// and `register_sink` is refused outright if the runtime's completion layout
/// does not match this crate's — which leaves this false rather than letting
/// the caller submit work whose completions nothing would deliver.
pub fn available() -> bool {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| {
        tl::register_sink(SUBSYSTEM, sink, alloc_id);
    });
    tl::available(SUBSYSTEM)
}

// ── Decline predicates ──────────────────────────────────────────────────────

/// `TE: trailers` — `plain_client.rs` owns this exchange. Same predicate as
/// that module's `expects_response_trailers`, deliberately duplicated in
/// spirit rather than shared, because the two must agree by construction.
fn wants_trailers(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("te")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("trailers"))
    })
}

/// An explicit `Host` header.
///
/// `client::Request::head` drops any caller `host` and substitutes the URL's
/// authority (`headers.retain(|h| !h.name.eq_ignore_ascii_case("host"))`),
/// which is the Fetch rule. `node:http` is not Fetch: `setHeader('Host', …)`
/// reaches the wire, and reqwest sends it, so routing such a request here
/// would silently rewrite it. Declining keeps today's behaviour; building the
/// `Head` by hand instead of through `Request::head` is what removes this.
fn overrides_host(headers: &HashMap<String, String>) -> bool {
    headers.keys().any(|name| name.eq_ignore_ascii_case("host"))
}

/// `Expect: 100-continue` — `continue_client.rs` owns this exchange.
fn wants_continue(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("expect")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("100-continue"))
    })
}

// ── Submission ──────────────────────────────────────────────────────────────

/// Try the turnloop path. `false` means the caller keeps its reqwest future.
///
/// Runs on the agent thread, from `dispatch_request_snapshot` — never from a
/// tokio worker, which is what lets it submit to the loop directly.
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
    // Cheap, local refusals first: none of these touch the loop.
    if agent_handle != 0
        || !body.is_empty()
        || timeout_ms.is_some()
        || crate::node_env_proxy_enabled()
        || wants_trailers(headers)
        || wants_continue(headers)
        || crate::client_upgrade::wants_upgrade(headers)
        || overrides_host(headers)
    {
        return false;
    }
    if !url.starts_with("http://") {
        return false;
    }
    if !available() {
        return false;
    }

    // `Request::new` is the codec's own refusal set: a non-http(s) scheme,
    // credentials in the URL, a non-token or forbidden-fetch method.
    let Ok(mut request) = Request::new(url, method) else {
        return false;
    };
    let Some(host) = request.url.host_str().map(str::to_owned) else {
        return false;
    };
    let port = request.url.port_or_known_default().unwrap_or(80);

    for (name, value) in headers {
        request
            .headers
            .push(http1::Header::new(name, value.as_bytes()));
    }
    // Node's default agent is keep-alive (v19+) and sends the header
    // explicitly; servers reading `req.headers.connection` expect it. Mirrors
    // the reqwest path exactly so the wire is unchanged.
    if !headers.keys().any(|k| k.eq_ignore_ascii_case("connection")) {
        request
            .headers
            .push(http1::Header::new("connection", "keep-alive".as_bytes()));
    }

    let mut conn = Http1Connection::new(http1::Limits::default());
    // No body in this lane, so the upload is finished the moment the head is:
    // `BodyLength::Empty` plus an immediate `finish_body` leaves the decoder
    // waiting only on the response.
    if conn
        .start(&request.head(false), http1::BodyLength::Empty, None, None)
        .is_err()
        || conn.finish_body(&[]).is_err()
    {
        return false;
    }

    let id = next_id();
    if id == perry_ffi::INVALID_HANDLE {
        return false;
    }
    exchanges()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(
            id,
            Exchange {
                request_handle,
                conn,
                pending_head: true,
                settled: false,
                _inflight: ClientInflightGuard::new(request_handle),
            },
        );
    // Submitted last: the completion can arrive before this call returns (a
    // loopback connect completes in the same turn), and it must find the entry.
    if let Err(error) = tl::tcp_connect(id, SUBSYSTEM, &host, port, NODELAY) {
        forget(id);
        perry_ffi::free_handle_id(id);
        // The entry is gone and nothing was put on the wire, so the caller may
        // still run its reqwest future — except when the loop itself is
        // unavailable, which `available()` already ruled out. Report rather
        // than double-dispatch.
        report_net_error(request_handle, &error);
        return true;
    }
    ACCEPTED.fetch_add(1, Ordering::Relaxed);
    true
}

fn next_id() -> i64 {
    perry_ffi::reserve_handle_id_in_domain(registry_domain())
}

// ── The completion sink ─────────────────────────────────────────────────────

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime passes a live completion for the duration of the
    // call, which is this function's body.
    let c = unsafe { &*completion };
    match c.kind {
        tl::NET_CONNECT => on_connect(c.id),
        // SAFETY: same call; the pooled lease outlives it.
        tl::NET_DATA => on_data(c.id, unsafe { c.bytes() }),
        tl::NET_EOF => on_eof(c.id),
        tl::NET_ERROR => {
            // SAFETY: the runtime builds these from `&'static str`s.
            let code = unsafe { c.code() }.unwrap_or("EPIPE").to_string();
            let syscall = unsafe { c.syscall() }.unwrap_or("").to_string();
            on_error(c.id, &code, &syscall, c.errno as i64);
        }
        tl::NET_CLOSED => on_closed(c.id),
        // `NET_WROTE` is an acknowledgement only: `tl::write` copies the
        // caller's bytes, so output is consumed at submission time.
        _ => {}
    }
}

fn on_connect(id: i64) {
    // Start reading before the head goes out: a loopback server's response can
    // be in flight before this submission returns.
    if let Err(error) = tl::read_start(id) {
        fail(id, &error.code, &error.syscall, error.errno as i64);
        return;
    }
    let should_write = with_exchange(id, |exchange| std::mem::take(&mut exchange.pending_head));
    if should_write == Some(true) {
        flush(id);
    }
}

/// Put whatever the codec has produced on the wire.
///
/// `tl::write` copies before returning, so the output is acknowledged to the
/// codec immediately. The copy is also what lets `consume_output` take `&mut`
/// while the bytes are in flight — `client.rs` warns against mutating the
/// connection while a write borrows `output()`.
fn flush(id: i64) {
    loop {
        let chunk = with_exchange(id, |exchange| exchange.conn.output().to_vec());
        let Some(chunk) = chunk else { return };
        if chunk.is_empty() {
            return;
        }
        if let Err(error) = tl::write(id, &chunk, 0) {
            fail(id, &error.code, &error.syscall, error.errno as i64);
            return;
        }
        let consumed = with_exchange(id, |exchange| exchange.conn.consume_output(chunk.len()));
        if !matches!(consumed, Some(Ok(()))) {
            return;
        }
    }
}

/// What one `receive` step produced, lifted out of the borrow so the events can
/// be pushed without holding the exchange lock across JS-visible work.
enum Produced {
    Nothing,
    Head {
        status: u16,
        version: u8,
        headers: Vec<(String, String)>,
    },
    Body(Vec<u8>),
    End,
    /// An interim `1xx`, or a trailer section. Neither is deliverable on this
    /// lane — `Expect: 100-continue` and `TE: trailers` both decline in
    /// `try_dispatch`, so a server sending either unprompted is ignored exactly
    /// as the reqwest path ignored it.
    Ignored,
}

fn on_data(id: i64, bytes: &[u8]) {
    let mut offset = 0usize;
    // Consecutive steps that consumed nothing. The loop's exit condition is
    // *progress*, and an event is progress even at zero bytes — that is how
    // `End` arrives from a zero-byte step. But "event, zero consumed" repeated
    // forever would spin the agent's event loop with no way out, which is a
    // worse failure than a dropped response, so it is bounded. Only `End`
    // legitimately arrives this way, so anything past a couple of steps is a
    // decoder that is not advancing.
    let mut idle_steps = 0u32;
    const MAX_IDLE_STEPS: u32 = 8;
    loop {
        let stepped = with_exchange(id, |exchange| {
            if exchange.settled {
                return None;
            }
            let input = &bytes[offset.min(bytes.len())..];
            let step = match exchange.conn.receive(input) {
                Ok(step) => step,
                Err(error) => return Some(Err(error)),
            };
            let produced = match step.event {
                Some(http1::Event::Head(head)) => Produced::Head {
                    status: head.status,
                    version: head.version,
                    headers: head
                        .headers
                        .iter()
                        .map(|h| {
                            (
                                h.name.clone(),
                                String::from_utf8_lossy(&h.value).into_owned(),
                            )
                        })
                        .collect(),
                },
                Some(http1::Event::Body(chunk)) => Produced::Body(chunk.to_vec()),
                Some(http1::Event::End) => Produced::End,
                Some(http1::Event::Informational(_))
                | Some(http1::Event::Trailers(_))
                | Some(http1::Event::Upgrade) => Produced::Ignored,
                None => Produced::Nothing,
            };
            Some(Ok((step.consumed, produced)))
        });
        let Some(stepped) = stepped.flatten() else {
            return;
        };
        let (consumed, produced) = match stepped {
            Ok(stepped) => stepped,
            Err(error) => {
                protocol_failure(id, error);
                return;
            }
        };
        offset = offset.saturating_add(consumed).min(bytes.len());

        match produced {
            Produced::Head {
                status,
                version,
                headers,
            } => emit_head(id, status, version, headers),
            Produced::Body(chunk) => {
                if let Some(request_handle) = with_exchange(id, |e| e.request_handle) {
                    push_event(PendingHttpEvent::ResponseChunk {
                        request_handle,
                        chunk: Bytes::from(chunk),
                    });
                }
            }
            Produced::End => {
                finish(id);
                return;
            }
            Produced::Nothing if consumed == 0 => return,
            Produced::Nothing | Produced::Ignored => {}
        }
        if consumed == 0 {
            idle_steps += 1;
            if idle_steps >= MAX_IDLE_STEPS {
                return;
            }
        } else {
            idle_steps = 0;
        }
        // Loop while the step made progress. Once `offset` reaches the end the
        // next call is `receive(&[])`, which is how `End` arrives from a
        // zero-byte step — omitting that costs a full idle timeout per request
        // (turnloop#50), so the exit condition is progress, never "input
        // drained".
    }
}

fn emit_head(id: i64, status: u16, version: u8, headers: Vec<(String, String)>) {
    let Some(request_handle) = with_exchange(id, |exchange| exchange.request_handle) else {
        return;
    };
    // `http1::Head` carries no reason phrase, so the canonical one stands in —
    // which is exactly what the reqwest path already did
    // (`StatusCode::canonical_reason`).
    let status_message = http::StatusCode::from_u16(status)
        .ok()
        .and_then(|code| code.canonical_reason())
        .unwrap_or("")
        .to_string();
    push_event(PendingHttpEvent::ResponseHead {
        request_handle,
        status,
        status_message,
        headers,
        // `Head::version` is the HTTP/1 minor: 0 for HTTP/1.0, 1 for HTTP/1.1.
        http_version: (1, version),
    });
}

// ── Teardown ────────────────────────────────────────────────────────────────

/// The response finished cleanly.
fn finish(id: i64) {
    let Some(mut exchange) = forget(id) else {
        return;
    };
    let settled = std::mem::replace(&mut exchange.settled, true);
    let _ = tl::close(id);
    if settled {
        return;
    }
    COMPLETED.fetch_add(1, Ordering::Relaxed);
    push_event(PendingHttpEvent::ResponseEnd {
        request_handle: exchange.request_handle,
    });
}

/// The peer closed its write side. A clean close after `End` is the normal way
/// a `Connection: close` response ends and has already been settled by
/// [`finish`]; anything else truncated the message.
fn on_eof(id: i64) {
    let outcome = with_exchange(id, |exchange| {
        if exchange.settled {
            return None;
        }
        Some(exchange.conn.eof())
    });
    match outcome.flatten() {
        // The decoder accepted EOF as the end of an identity body.
        Some(Ok(())) => {
            // Drain whatever the zero-byte step yields — `End` can arrive only
            // now for a body delimited by the close.
            on_data(id, &[]);
            // Still live and unsettled means the decoder wants more than the
            // peer will send.
            if with_exchange(id, |exchange| !exchange.settled) == Some(true) {
                fail(id, "ECONNRESET", "read", 0);
            }
        }
        Some(Err(error)) => protocol_failure(id, error),
        None => {
            let _ = tl::close(id);
        }
    }
}

fn on_error(id: i64, code: &str, syscall: &str, errno: i64) {
    fail(id, code, syscall, errno);
}

fn on_closed(id: i64) {
    // A close that arrives with the exchange still live means the socket went
    // away without a terminal event of its own.
    if with_exchange(id, |exchange| exchange.settled) == Some(false) {
        fail(id, "ECONNRESET", "read", 0);
    }
    forget(id);
    // The terminal completion: nothing can name this id again and no JS object
    // holds it, so it goes back to the shared band rather than leaking one per
    // request — the #6441 id-exhaustion shape that `perry-ext-ws`,
    // `perry-ext-net` and `perry-http-server` each carry this arm for. Every
    // terminal path here (`finish`, `fail`, `protocol_failure`) submits
    // `tl::close`, so this completion is reached for every accepted exchange.
    perry_ffi::free_handle_id(id);
}

/// A framing/protocol refusal from the codec, which carries an undici-style
/// cause code rather than an OS one.
fn protocol_failure(id: i64, error: turnloop_http::Error) {
    let Some(mut exchange) = forget(id) else {
        return;
    };
    let settled = std::mem::replace(&mut exchange.settled, true);
    let _ = tl::close(id);
    if settled {
        return;
    }
    push_event(PendingHttpEvent::Error {
        request_handle: exchange.request_handle,
        error_message: format!("{} {}", error.code, error.message),
    });
}

/// A transport failure, in the Node `Error` shape the drain builds `.code` /
/// `.syscall` / `.errno` from.
fn fail(id: i64, code: &str, syscall: &str, errno: i64) {
    let Some(mut exchange) = forget(id) else {
        return;
    };
    let settled = std::mem::replace(&mut exchange.settled, true);
    let _ = tl::close(id);
    if settled {
        return;
    }
    push_transport_error(exchange.request_handle, code, syscall, errno);
}

fn report_net_error(request_handle: Handle, error: &tl::NetError) {
    push_transport_error(
        request_handle,
        &error.code,
        &error.syscall,
        error.errno as i64,
    );
}

fn push_transport_error(request_handle: Handle, code: &str, syscall: &str, errno: i64) {
    let message = if syscall.is_empty() {
        code.to_string()
    } else {
        format!("{syscall} {code}")
    };
    push_event(PendingHttpEvent::TransportError {
        request_handle,
        message,
        code: code.to_string(),
        syscall: syscall.to_string(),
        errno,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn this_lane_owns_a_slot_no_other_subsystem_claims() {
        // 0 net, 1 this crate's server, 2 stdlib's fetch client, 3 SMTP,
        // 4 fastify, 5 framework server, 7/8 ws, 9-12 the database bindings.
        // The authority for that map is `perry-db-turnloop`'s `subsystem`
        // module header; 6 was the one free slot below the database band.
        assert_eq!(SUBSYSTEM, 6);
        for taken in [0u8, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12] {
            assert_ne!(SUBSYSTEM, taken, "slot {taken} belongs to another lane");
        }
    }

    #[test]
    fn a_te_trailers_request_is_left_to_the_raw_socket_bypass() {
        assert!(wants_trailers(&headers(&[("TE", "trailers")])));
        assert!(wants_trailers(&headers(&[("te", "gzip, trailers")])));
        assert!(!wants_trailers(&headers(&[("te", "gzip")])));
        assert!(!wants_trailers(&headers(&[("accept", "trailers")])));
    }

    /// `Request::head` would substitute the URL authority for a caller's
    /// `Host`, which reqwest does not do. Declining is what keeps the two
    /// transports agreeing about the wire.
    #[test]
    fn an_explicit_host_header_is_left_to_reqwest() {
        assert!(overrides_host(&headers(&[("Host", "vhost.invalid")])));
        assert!(overrides_host(&headers(&[("host", "vhost.invalid")])));
        assert!(!overrides_host(&headers(&[("x-forwarded-host", "a")])));

        // The reason it has to decline: the codec rewrites it.
        let mut request = Request::new("http://example.invalid/p", "GET").expect("valid request");
        request
            .headers
            .push(http1::Header::new("host", "vhost.invalid".as_bytes()));
        let head = request.head(false);
        let hosts: Vec<String> = head
            .headers
            .iter()
            .filter(|h| h.name == "host")
            .map(|h| String::from_utf8_lossy(&h.value).into_owned())
            .collect();
        assert_eq!(
            hosts,
            vec!["example.invalid".to_string()],
            "the codec replaces a caller's Host with the URL authority"
        );
    }

    #[test]
    fn an_expect_continue_request_is_left_to_the_raw_socket_bypass() {
        assert!(wants_continue(&headers(&[("Expect", "100-continue")])));
        assert!(wants_continue(&headers(&[("expect", "100-CONTINUE")])));
        assert!(!wants_continue(&headers(&[("expect", "other")])));
    }

    /// The codec's own refusal set, which this lane turns into a decline
    /// rather than an error so the existing message text survives.
    #[test]
    fn the_codec_refuses_exactly_what_this_lane_declines_on() {
        assert!(Request::new("http://example.invalid/", "GET").is_ok());
        assert!(Request::new("http://example.invalid/", "TRACE").is_err());
        assert!(Request::new("http://example.invalid/", "CONNECT").is_err());
        assert!(Request::new("http://user:pw@example.invalid/", "GET").is_err());
    }

    /// A GET with no body produces a complete request head and nothing else,
    /// so the exchange is write-complete before the socket exists. This is the
    /// property that lets lane 1 skip `send_body` entirely.
    #[test]
    fn a_bodyless_get_serializes_a_complete_head_and_finishes_its_upload() {
        let request = Request::new("http://example.invalid/start", "GET").expect("valid request");
        let mut conn = Http1Connection::new(http1::Limits::default());
        conn.start(&request.head(false), http1::BodyLength::Empty, None, None)
            .expect("head starts");
        conn.finish_body(&[]).expect("an empty upload finishes");
        let wire = String::from_utf8(conn.output().to_vec()).expect("ascii head");
        assert!(wire.starts_with("GET /start HTTP/1.1\r\n"), "{wire}");
        assert!(wire.to_ascii_lowercase().contains("host: example.invalid"));
        assert!(wire.ends_with("\r\n\r\n"), "{wire}");
    }

    /// The reason a 3xx needs no redirect policy here: the codec hands the
    /// response back verbatim, which is what `node:http` must do.
    #[test]
    fn a_redirect_response_is_decoded_as_an_ordinary_response() {
        let request = Request::new("http://example.invalid/start", "GET").expect("valid request");
        let mut conn = Http1Connection::new(http1::Limits::default());
        conn.start(&request.head(false), http1::BodyLength::Empty, None, None)
            .expect("head starts");
        conn.finish_body(&[]).expect("an empty upload finishes");
        let response = b"HTTP/1.1 307 Temporary Redirect\r\nlocation: /target\r\ncontent-length: 8\r\n\r\nredirect";
        let step = conn.receive(response).expect("a head decodes");
        match step.event {
            Some(http1::Event::Head(head)) => {
                assert_eq!(head.status, 307);
                assert_eq!(
                    head.headers
                        .iter()
                        .find(|h| h.name == "location")
                        .map(|h| h.value.clone()),
                    Some(b"/target".to_vec())
                );
            }
            other => panic!("expected a head, got {other:?}"),
        }
    }
}

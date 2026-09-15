# turnloop P5 — the HTTP/1.1 and TLS server stack

Branch `turnloop/p5-servers`, based on `turnloop/integration` at `14803019fc`
(P0 + P1 + P2 + P3 merged). Built and tested on the shared Linux box
(EPYC 9354P) against the pinned gap oracle Node **26.5.1**. Nothing here was run
on Windows, and nothing was benchmarked.

## What moved, and what did not

| server surface | transport after P5 | why |
|---|---|---|
| `http.createServer().listen()` on the primary agent | **turnloop** + `turnloop_http::http1` | — |
| `https.createServer().listen()` on the primary agent | **turnloop** + `turnloop-tls` (unbuffered rustls) | — |
| `server.on('upgrade')` (raw upgrade → `net.Socket`) | **turnloop**, via `turnloop_net::transfer` | — |
| `net.connect(port, host)` outbound TCP client | **turnloop** | P1's deferred class; TLS above the socket removes its blocker |
| `tls.connect` | **turnloop** | ditto |
| `socket.upgradeToTLS` | **turnloop** | ditto — the P5 acceptance case |
| a server on a `worker_threads` agent | hyper | that agent has no loop before P3/P4 |
| a server in a cluster worker | hyper | SCHED_RR fd passing and the `SO_REUSEPORT` bind both need the `std::net::TcpListener` |
| a server with `WebSocketServer({ server })` attached at listen time | hyper | its handshake needs an owned stream for `tokio_tungstenite` |
| `http2.createSecureServer` | hyper + `h2` | not migrated; see "What P5 did not do" |
| `perry-ext-fastify` | hyper | not migrated; ditto |

This is a narrowing, not a removal — the same shape P1 left the tokio socket
task in, and for the same reason: the declining cases are real, they are still
exercised, and deleting the fallback would break them.

## Why sans-I/O, and not `turnloop_http::asynchronous`

`turnloop-http` ships a futures-io server driver
(`asynchronous::server::http1`/`http2`) that would have been far less code. It
needs a `turnloop_io::ExecutorHandle`, and that is where it stops being usable
from Perry:

* `LocalExecutor::with_config` **constructs its own `Driver`**
  (`crates/turnloop/src/executor.rs`). Perry already owns one
  `turnloop::Loop` per agent (`event_pump/agent_loop.rs`), and a second loop in
  the same process is exactly the mixed-transport deadlock P1 had to paper over
  with a 1 ms tick slice.
* Even sharing one, `LocalExecutor::turn` drains the driver's completions into
  `Shared::dispatch`, which returns early for any token without its own tag bit
  (`if completion.token.0 & TAG == 0 { return; }`). P1's net tokens, P2's
  process tokens and P3's timer token would be **silently dropped** — no error,
  no counter, just a socket that stops delivering.

So the codecs are driven sans-I/O over P1's completion layer instead, which is
also what DESIGN §5b asks for ("use a sans-IO or runtime-agnostic protocol crate
where a good one exists") and what keeps DESIGN D1 true (the driver never calls
host code, and neither does this). See "turnloop gaps found" for the two-line
change that would make the executor adoptable later.

## Architecture

```
NET_ACCEPT ─► turnloop_serve::conn::on_accept ─► [TlsSession::server]  ─┐
NET_DATA   ─► on_data ─► [TLS decrypt] ─► http1::Decoder ─► Building ──┤
                                                     │ Event::End      │
                                                     ▼                 │
                              IncomingMessage + ServerResponse handles  │
                                                     │                 │
                                       queue (this thread, no channel)  │
                                                     ▼                 │
              js_node_http_server_process_pending ─► the JS handler ────┘
                                                     │ res.end()
                                                     ▼
                       http1::Encoder ─► [TLS encrypt] ─► turnloop_net::write
```

Three rules hold it together:

1. **The sink runs no JS.** It runs inside `dispatch_staged`, after a turn has
   returned, so it may allocate Rust state and register handles — but a decoded
   request is *queued*, and the existing main-thread pump dispatches it on its
   own tick, exactly where hyper's `mpsc` delivered it. The event-loop phase
   order the gap suite pins does not move.
2. **One request in flight per connection.** The decoder is `reset()` only once
   the response has been written, so a pipelined request stays in the
   connection's input buffer and `res` is never ambiguous. That is Node's
   per-connection serialization.
3. **No JS value and no heap pointer reaches the driver.** Reads land in
   turnloop's pooled buffers and are copied out inside the dispatch call; writes
   are owned `Vec<u8>`s. P1's rule, unchanged, which is why this module
   registers no GC root scanner.

## GC decisions

* **No new roots.** A connection holds decoded head and body bytes as owned
  `Vec<u8>`s and the two *handle ids* of the request it produced. The
  `IncomingMessage` / `ServerResponse` handles are scanned by perry-ext-http's
  existing `scan_http_server_roots`; carrying ids rather than closure addresses
  is what keeps #8082's "a channel-parked snapshot goes stale across a moving
  collection" from reappearing.
* **The TLS session holds no JS value either** — only owned ciphertext and
  plaintext buffers. Plaintext is copied into a JS value by the binding's sink,
  on the owning thread, exactly as a cleartext read already was.
* **The `upgradeToTLS` promise is a `JsNativeAsyncCompletion`, not a bare
  `*mut Promise` in a side table.** The runtime pins and root-scans a promise
  behind such a token (#9552); a raw pointer cached in a Rust map is precisely
  the shape `scripts/gc_runtime_root_holders.py` exists to catch, and it would
  have been invisible to the static rooting checker.
* **Connection ids are freed on their terminal completion.** Unlike a
  `net.Socket` id, no JS object outlives a turnloop HTTP connection, so its id
  goes back to the shared band instead of leaking one per connection for the
  life of the server (the #6441 exhaustion class).

## The `keepAliveTimeout = 0` question

P0 recorded that `server.keepAliveTimeout = 0` means "never time out" in Node
but "no keep-alive" in Perry. Measured on the pinned oracle rather than argued
from the docs — a raw `net.Socket` client, one request, then idle:

| `keepAliveTimeout` | `keepAliveTimeoutBuffer` | response `Keep-Alive` header | server FIN at |
|---|---|---|---|
| 0 | 1000 (default) | *(none)* | **never** (still open at 2000 ms) |
| 0 | 0 | *(none)* | **never** |
| 300 | 0 | `timeout=0` | 305 ms |
| 300 | 500 | `timeout=0` | 801 ms |
| 300 | 1000 (default) | `timeout=0` | 1301 ms |
| 1000 | 1000 (default) | `timeout=1` | 2002 ms |
| 5000 (default) | 1000 (default) | `timeout=5` | *(not reached in 1000 ms)* |

Every row carried `Connection: keep-alive`, including both zero rows.

So there are **two** decisions, and Perry had fused them:

* whether the connection is reused — the protocol version and the request's
  `Connection` tokens, and nothing else;
* whether a timeout is advertised and armed — `keepAliveTimeout`, with zero
  meaning *no timeout*, and the real idle close at
  `keepAliveTimeout + keepAliveTimeoutBuffer`.

Perry's `apply_default_connection_headers` gated the first on the second
(`should_keep_alive && keep_alive_timeout_ms > 0.0`), so a server that disabled
the timeout answered `Connection: close` on every response and got no reuse at
all. Both halves are now Node's: the header split is in
`ServerResponse::apply_default_connection_headers_for` (with the matrix pinned
in `response_tests.rs`), and the idle close is armed as a real turnloop deadline
at `keepAliveTimeout + keepAliveTimeoutBuffer`, with zero arming nothing
(`server::idle_close_ms`, pinned in `turnloop_serve/tests.rs`).

Note what that second half required: **under hyper, Perry armed no idle timeout
at all.** `http1::Builder` was configured with neither `keep_alive` timeouts nor
`header_read_timeout`, so an idle keep-alive connection was held forever
whatever `keepAliveTimeout` said. The turnloop path is the first time the knob
does anything.

## New primitives in the runtime's turnloop net layer

Both are general, and both exist because a *binding* needed them and could not
express them:

* **`turnloop_net::timer_arm` / `timer_cancel`** (`NET_TIMER` completions).
  Node's server timeouts — `keepAliveTimeout`, `headersTimeout`,
  `requestTimeout`, a TLS handshake deadline, a lingering close — are deadlines
  on a connection, and a separately linked binding has no way to create a JS
  timer. Arming one here puts it in `Loop::next_deadline()`, so a park whose
  only work is an idle keep-alive connection ends on time rather than blocking
  until the peer moves. The handle is unreferenced, like the agent's own JS-timer
  deadline: a pending deadline must never keep the process alive by itself.
* **`turnloop_net::transfer`** — hand a live socket to another subsystem,
  keeping its id and every outstanding operation. An HTTP `'upgrade'` is exactly
  that handoff. The multishot read is deliberately **not** cancelled: the token
  carries only the id and routing reads the subsystem out of the entry at
  dispatch time, so the next byte reaches the new owner with no gap and no
  resubmission. Whatever the old owner had already buffered it hands over itself
  — which is Node's `'upgrade'` `head` argument.

## TLS, and how it unblocked P1's last socket class

P1's report named one blocker for outbound TCP clients: `socket.upgradeToTLS`
hands a live `TcpStream` to `tokio_rustls` mid-stream, and turnloop owns its
descriptor without exposing it. It listed two things that would unblock it —
a descriptor handoff, or TLS on turnloop.

This is the second. `perry-ext-net/src/turnloop_tls.rs` drives `turnloop-tls`'s
unbuffered rustls core from the outside: ciphertext in as `NET_DATA` arrives,
ciphertext out through `turnloop_net::write`, plaintext back to the binding, all
on the loop thread inside the dispatch call. With the session running *above*
the turnloop handle, **no descriptor has to move at all** — the same handle keeps
carrying bytes and a session is simply installed on top of it, mid-stream, which
is exactly PostgreSQL's `SSLRequest` shape.

`turnloop_tls_io.rs` is the per-socket layer. The part worth naming is the write
accounting: a caller writes *plaintext* and turnloop acknowledges *ciphertext*,
and the mapping is not one-to-one (a write issued during the handshake is
buffered by rustls and encrypted later; one flush can carry several application
writes plus handshake records). Each application write therefore records the
ciphertext offset at which its plaintext had been encrypted, and a `NET_WROTE`
completion advances an acknowledged-ciphertext counter; a write's callback fires
when the counter reaches its mark. That is what keeps `socket.write(chunk, cb)`
on an upgraded socket honouring Node's "cb fires when the bytes have left".

One deliberate deviation from `turnloop-tls`'s own async driver: rustls's
`TransmitTlsData` is acknowledged once the encoded records have been **queued**
on the turnloop handle rather than once they have been written. turnloop orders
a handle's writes, so nothing encrypted afterwards can overtake them, and the
caller submits the queued bytes before the next completion is processed.

## What P5 did not do

Named precisely, because each is a hole rather than a preference:

* **HTTP/2.** `http2.createSecureServer` keeps hyper + the `h2` crate. The
  `turnloop_http::http2::Connection` core exists and is sans-I/O, but Perry's
  HTTP/2 server is a second full surface (`http2_server/{session,dispatch,pump,
  controls}.rs`, ~2400 lines, its own stream handles, settings, ALPN and flow
  control) and migrating it is its own change.
* **`perry-ext-fastify`.** It carries its own hyper accept loop and has no
  dependency edge to perry-ext-http, so sharing this core needs either a new
  crate for it or a new dependency edge. Untouched.
* **A natively attached `WebSocketServer({ server })`.** Perry completes that
  handshake with `tokio_tungstenite` over an owned stream, which a turnloop
  connection cannot produce; `turnloop-websocket` is sans-I/O and would fit, but
  perry-ext-ws stores `WebSocketStream` values from a *different* tungstenite
  major (0.29's vs turnloop-websocket's 0.30), so the connection type has to
  change with it. Until then such a server declines the turnloop path at listen
  time. `server.on('upgrade')` — the documented `ws` integration, and what
  `@hono/node-server` uses — needs none of that and is served on turnloop.
* **The bundled stdlib server** (`perry-stdlib/src/framework/server.rs`) is
  untouched, like P1 left the bundled stdlib `net`.

## turnloop gaps found

Reported here in the shape #34, #35 and #38 were.

1. **`LocalExecutor` silently drops completions it did not issue.**
   `Shared::dispatch` returns early unless the token carries its tag bit, so a
   host that owns the loop *and* submits its own operations cannot use the
   executor at all — and the failure mode is a socket that stops delivering, with
   no error and no counter. An escape hatch (hand unrouted completions back, or
   let the host pass a fallback sink) would make `turnloop_http::asynchronous`,
   `turnloop_tls::asynchronous` and `turnloop_websocket::asynchronous` adoptable
   by a host like Perry.
2. **`http1::Encoder` cannot emit a custom reason phrase.** `Encoder::start`
   always writes the IANA canonical reason for the status, and
   `res.writeHead(404, 'Nope')` is observable on the wire in Node. Worked around
   by patching the status line after encoding.
3. **`http1::BodyLength` cannot express a close-delimited body.** An HTTP/1.0
   response with neither `Content-Length` nor chunked framing ends at EOF, and
   there is no variant for it; such a head is written by hand.
4. **A body-forbidden response has no framing of its own.** A HEAD response
   advertises the `Content-Length` it *would* have sent and emits no body, which
   `Encoder::start(…, Known(0))` rejects as a conflict and `Known(n)` then
   refuses to `finish`. Handled here by writing the head verbatim; a
   `BodyLength::None` (or a `head_response` flag) would belong in the crate.
5. **`turnloop_tls::{ClientConfig, ServerConfig}` cannot wrap an existing
   `rustls` config.** Their fields are private and `new()` takes chain + key +
   ALPN, so a host that already builds rustls configs from Node's option surface
   (SNI, client-cert auth, custom verifiers, session tickets, protocol-version
   masks) cannot use them. Perry constructs `rustls::…::Unbuffered*Connection`
   directly and uses the crate's re-exported `rustls`, `ConnectionState` and
   `node_error_code` instead — which works, but means the config wrapper is dead
   weight for this consumer.
6. **`ListenOpts` has no `reuse_port` reachable through Perry's binding**, which
   is one of the two reasons a cluster worker keeps the hyper path.
7. **`setNoDelay` on an accepted connection** is still unreachable (P1's finding,
   unchanged).

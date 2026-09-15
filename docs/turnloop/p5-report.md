# turnloop P5 — the HTTP/1.1 and TLS server stack

Branch `turnloop/p5-servers`, based on `turnloop/integration` at `14803019fc`
(P0 + P1 + P2 + P3 merged). Built and tested on the shared Linux box
(EPYC 9354P) against the pinned gap oracle Node **26.5.1**. Nothing here was run
on Windows, and nothing was benchmarked.

## Dependencies, added and not removed

`turnloop-http` and `turnloop-tls` 0.1.0-alpha.3 are added (default features
only). **hyper, hyper-util, h2, tokio-rustls and tokio-tungstenite are NOT
removed**, and the reason is the fallback table below rather than reluctance: a
`worker_threads` agent has no loop, a cluster worker needs the `std` listener,
`http2.createSecureServer` and `perry-ext-fastify` keep their own loops, and an
attached `WebSocketServer` still completes its handshake with
`tokio_tungstenite`. Every one of those paths is reachable and exercised, so
deleting the dependency would delete a working configuration. `reqwest` keeps
hyper in `perry-ext-http`'s tree regardless until P6.

`turnloop-websocket` is deliberately *not* added: nothing uses it yet (see
"What P5 did not do"), and an unused workspace dependency is a claim the
lockfile would then carry.

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
3. **One request in flight per connection.** The decoder is `reset()` only once
   the response has been written, so a pipelined request stays in the
   connection's input buffer and `res` is never ambiguous. That is Node's
   per-connection serialization.
4. **No JS value and no heap pointer reaches the driver.** Reads land in
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

The header half applies to **both** transports — the hyper path calls the same
`apply_default_connection_headers`, so a server that declines the turnloop path
gets the corrected headers too. The idle close is turnloop-only, because it is
armed as a turnloop deadline.

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

## Test evidence

Every command as run, on the shared Linux box, against Node **26.5.1**.

### The wire, byte-for-byte against Node

Two new gap tests, both compared with `diff` against
`node --experimental-strip-types` on the same file:

```
target/release/perry test-files/test_gap_turnloop_http_server.ts  -o /tmp/e_http.bin
target/release/perry test-files/test_gap_turnloop_https_server.ts -o /tmp/e_https.bin
```

| test | result |
|---|---|
| `test_gap_turnloop_http_server.ts` | **byte-identical** |
| `test_gap_turnloop_https_server.ts` | **byte-identical** |

They assert the wire rather than an HTTP client's view, through a raw
`net.Socket`: the status line, the framing decision, the `Connection` /
`Keep-Alive` pair and connection reuse are what changed transport, and a client
would hide all four. Between them they cover a plain GET; a POST read through
`'data'`/`'end'`; a chunked upload; a streamed response (head flushed by the
first `write`, chunked body); 204; HEAD; a custom reason phrase; two requests
on one connection; `Connection: close`; HTTP/1.0; `keepAliveTimeout = 0`; and,
over TLS, a 40 000-byte body that spans several TLS records.

Reaching byte-identity took four Node-fidelity fixes the first run exposed,
all of them the *wire* rather than the transport: `Transfer-Encoding: chunked`
spelled the way Node spells it rather than the way `Encoder::start`
synthesizes it, and a Content-Length Perry *synthesized* dropped where Node
sends none — on 204/304/1xx, on a HEAD response, and on a close-delimited
HTTP/1.0 body. A length the handler set is kept, as Node keeps it.

One difference is deliberately **not** in these tests, because it predates P5
and hyper framed it the same way: `res.writeHead(...)` followed by
`res.end(body)` is chunked by Node and length-framed by Perry. Node only
computes a `Content-Length` while the header block is still open at `end()`
time. The tests use `setHeader` + `end`, and `statusMessage` where a custom
reason phrase is wanted, so the file asserts P5's behaviour rather than that
one.

### The lifecycle edges

`test-files/test_gap_turnloop_http_lifecycle.ts` covers what is easy to get
wrong once the accept loop, the codec and the handler stop being three
different tasks. Run on Perry, on Node, **and on the base commit**:

| case | Node | P5 | base (hyper) |
|---|---|---|---|
| trailers after a chunked body | `X-Checksum: abc123` in the trailer block | same | **missing** |
| `Expect: 100-continue` → `'checkContinue'` → `res.writeContinue()` | interim `100 Continue`, then the 200 | same | same |
| a client that vanishes mid-request | `req` emits `'aborted'`; the late `res.end()` does not throw; the server keeps serving | same | **no `'aborted'`** |
| `server.close()` with a request in flight | the in-flight request completes, then `'close'`; a new connection is refused | same | same |

Two of those started out as differences. The trailer block was already right on
turnloop and wrong under hyper — the migration fixed it — and `'aborted'` was
missing on both until this change added it (the sink queues the
`IncomingMessage` because it may not run JS; the pump fires the listeners).
With both in place the file is byte-identical to Node except for one header
this test deliberately does not print: Perry answers `Connection: close` on a
response issued after `server.close()` where Node keeps `keep-alive`, which the
hyper path did too.

### An external client

`curl` against a Perry server, on the same build:

- `GET` and `POST` answer with `Content-Length` and `Connection: keep-alive` /
  `Keep-Alive: timeout=5`;
- `res.write()`×2 + `res.end()` arrives as `1\r\na … 0\r\n\r\n` — real chunked framing;
- **connection reuse**: `curl url1 url2 -w '%{http_code} %{num_connects}'` →
  `200 1` then `200 0`. The second request opened **no** connection;
- `--http1.0` answers `Connection: close` and a close-delimited body;
- a request with a control byte in a header value answers
  `HTTP/1.1 400 Bad Request` + `Connection: close`.

### `socket.upgradeToTLS` — the P5 acceptance case

`test-files/test_net_upgrade_tls.ts` against its Python `SSLRequest` companion:

```
plain connect ok
server negotiation byte: S
upgrading to TLS...
tls upgrade ok
echo over TLS: hello-after-upgrade
OK
[perry-loop] driver=turnloop turns=14 os_waits=7 zero_event_waits=3 native_ticks=0 turn_errors=0 completions=17
```

The same test on the base commit produces the same six lines with
`native_ticks=4` and `tokio_ticks=4`: the socket was on tokio there and is on
turnloop here.

It also found two real defects, both now fixed. `socket.end()` followed by the
peer's FIN shut the write side down twice — once from `end()`, once from the
`allowHalfOpen: false` close on `'end'` — and the second `shutdown(2)` answers
`ENOTCONN`, which reached JS as a spurious `'error'`. That is latent on a plain
turnloop socket with the same ordering; TLS makes the ordering certain. And a
rustls failure *after* the application has asked to close is teardown noise on
a socket nobody is reading, which Node does not report either.

### `server.on('upgrade')`

A probe that issues a plain request and then an upgrade on the same server,
with a trailing byte in the upgrade packet so `head` is non-empty:

| | Perry | Node |
|---|---|---|
| plain request still served | `HTTP/1.1 200 OK`, body correct | same |
| `req.url` in the listener | `/ws` | `/ws` |
| `req.headers.upgrade` | `echo` | `echo` |
| `head.length` | **2** | **2** |

So the `turnloop_net::transfer` handoff loses nothing: the id, the outstanding
multishot read and the bytes that followed the head all survive.

**Writing to that socket does not work — on either transport.**
`socket.write(...)` from the `'upgrade'` listener returns `undefined` and
nothing reaches the wire, *identically on the base commit's hyper path*, so the
raw-upgrade response path is a pre-existing Perry gap rather than anything P5
changed (`net`'s composite handle dispatch has no `write` row, and the listener
reaches the socket as an untyped value). Worth its own issue; the migration
reproduces the existing behaviour exactly.

### `server.keepAliveTimeout`, against the oracle

`test-files/test_gap_turnloop_keepalive_timeout.ts`, run on both:

| `keepAliveTimeout` | `keepAliveTimeoutBuffer` | Perry | Node |
|---|---|---|---|
| 300 | 0 | `keep-alive`, `timeout=0`, closed on schedule | identical |
| 300 | 1000 | `keep-alive`, `timeout=0`, closed on schedule | identical |
| 1000 | 1000 | `keep-alive`, `timeout=1`, closed on schedule | identical |
| 0 | 1000 | `keep-alive`, **no** `Keep-Alive` header, **never closed** | identical |

That is the answer to P0's open question, and it is also the live test of the
new `NET_TIMER` deadline: without it the idle close would never fire and every
finite row would read `closed=false`.

### GC stress with requests in flight

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 ./test_gap_turnloop_http_server
```

Clean on all three seeds, and the instruments prove they were **armed** rather
than merely quiet:

- **343** `[gc-fromspace-protect] retired_set=#N` lines — copying minors really
  ran and their from-space really was quarantined and `mprotect`ed. A run with
  zero copying minors protects nothing and would have passed vacuously;
- 13,244 `[gc…]` diagnostic lines;
- **`completions=209`** on the same run, so those collections landed while
  socket operations were in flight;
- no SIGSEGV from the quarantine reporter: no stale from-space pointer was
  dereferenced, and the whole exchange still printed its expected output.

All three seeds report identical counts, which is the documented behaviour at
`RATE=1`: every handled safepoint collects, so the seed stops selecting.

### `PERRY_LOOP_STATS` and thread count, before and after

A server answering 20 requests from an in-process client, reporting its own
`/proc/self/status` `Threads:`:

| | P0 (recorded in its report) | P5 |
|---|---|---|
| `driver` | turnloop | turnloop |
| `turns` | **0** | **101** |
| `completions` | — | **282** |
| `native_ticks` (tokio ticks inside the park) | every park | **0** |
| `tokio_ticks` | > 0 | **0** |
| threads at start / while serving | — | **1 / 1** |

P0 measured that a Perry server made **zero** turnloop turns, because the hyper
accept loop pinned a tokio task and the park always chose the tokio tick. The
same workload now turns the loop 101 times, dispatches 282 completions, and
makes **no tokio tick at all** — nothing in the process holds a tokio task. The
thread count is the same number from the other side: one thread serves the
whole workload.

## turnloop gaps found

Reported here in the shape #34, #35 and #38 were.

1. **`http1::Decoder` never raises `Event::Upgrade` on the request side.**
   `State::Upgrade` is only reachable in `Mode::Response` (a client reading a
   101), so a *server* decodes `GET / HTTP/1.1` + `Connection: upgrade` as an
   ordinary head with no body and has to recognize the upgrade itself. That is
   defensible sans-I/O design — the server decides — but the asymmetry is not
   documented, and taking the enum at face value silently served every upgrade
   request as a normal request.
2. **`LocalExecutor` silently drops completions it did not issue.**
   `Shared::dispatch` returns early unless the token carries its tag bit, so a
   host that owns the loop *and* submits its own operations cannot use the
   executor at all — and the failure mode is a socket that stops delivering, with
   no error and no counter. An escape hatch (hand unrouted completions back, or
   let the host pass a fallback sink) would make `turnloop_http::asynchronous`,
   `turnloop_tls::asynchronous` and `turnloop_websocket::asynchronous` adoptable
   by a host like Perry.
3. **`http1::Encoder` cannot emit a custom reason phrase.** `Encoder::start`
   always writes the IANA canonical reason for the status, and
   `res.writeHead(404, 'Nope')` is observable on the wire in Node. Worked around
   by patching the status line after encoding.
4. **`http1::BodyLength` cannot express a close-delimited body.** An HTTP/1.0
   response with neither `Content-Length` nor chunked framing ends at EOF, and
   there is no variant for it; such a head is written by hand.
5. **A body-forbidden response has no framing of its own.** A HEAD response
   advertises the `Content-Length` it *would* have sent and emits no body, which
   `Encoder::start(…, Known(0))` rejects as a conflict and `Known(n)` then
   refuses to `finish`. Handled here by writing the head verbatim; a
   `BodyLength::None` (or a `head_response` flag) would belong in the crate.
6. **`turnloop_tls::{ClientConfig, ServerConfig}` cannot wrap an existing
   `rustls` config.** Their fields are private and `new()` takes chain + key +
   ALPN, so a host that already builds rustls configs from Node's option surface
   (SNI, client-cert auth, custom verifiers, session tickets, protocol-version
   masks) cannot use them. Perry constructs `rustls::…::Unbuffered*Connection`
   directly and uses the crate's re-exported `rustls`, `ConnectionState` and
   `node_error_code` instead — which works, but means the config wrapper is dead
   weight for this consumer.
7. **`ListenOpts` has no `reuse_port` reachable through Perry's binding**, which
   is one of the two reasons a cluster worker keeps the hyper path.
8. **`setNoDelay` on an accepted connection** is still unreachable (P1's finding,
   unchanged).

### The full gap suite, against this branch's own base

Both trees built identically (the harness's default package set plus the
`perry-ext-*` wrappers, in one cargo invocation, with **no**
`external-*-pump` features — see the environment note below) and run as
`PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh`.

| | base `14803019fc` | P5 |
|---|---|---|
| parity pass | 791 | 793 |
| parity fail | 9 | 10 |
| compile fail | 0 | 0 |
| crash | 0 | 0 |
| total | 800 | 803 (+3 new tests) |
| parity rate | 98.8 % | 98.7 % |

**Per-test, the failure sets are identical except for one**, and that one was a
real regression this sweep caught: `test_gap_turnloop_net_sockets` — P1's own
net test — lost the body of its TCP echo. The `allowHalfOpen: false` close on
`'end'` had started calling `destroy()` outright whenever the application had
already ended the writable side, and `Loop::close` cancels outstanding
operations, so it cancelled exactly the writes the `'end'` handler had just
queued. That is P1's own third behaviour note, re-broken from the other
direction by this branch's ENOTCONN fix. Fixed by separating the two questions
— whether to submit a shutdown (no, one is in flight) from whether the socket
may close yet (only once that shutdown completes) — and the test is
byte-identical to Node again.

The three new tests all pass inside the sweep
(`turnloop_http_server`, `turnloop_https_server`, `turnloop_keepalive_timeout`);
`turnloop_http_lifecycle` was added after the sweep started and is verified
against the oracle separately.

The base's nine, none of them touched by this work:
`2159_defineproperty_class_prototype`, `2514_settracesigint`,
`2899_2779_2777_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch`, `json_lazy_defineproperty_index`,
`perfhooks_3088_3008_3010_3011`, `prop_plan_cache_invalidation`,
`v8_2_3680plus`. Three of those (`2899_…`, `disposablestack_2875`,
`iterator_prototype_next_patch`) the committed snapshot expects to PASS, so the
gate is red on the base commit before P5 changes anything — which is exactly
why this comparison is against the base rather than against the snapshot.

### Targeted parity filters, at HEAD

Re-run after the two socket-ordering fixes the sweep exposed, so these are the
numbers for the final tree rather than for the commit the sweep measured. Every
filter: **0 failures, 0 compile failures, 0 crashes**.

| filter | pass |
|---|---|
| `test_gap_turnloop` (P1's net test + P5's four) | 10 |
| `test_gap_net` / `test_gap_gc_net` / `test_net_` / `test_parity_net` | 2 / 1 / 4 / 1 |
| `test_gap_http` / `test_parity_http` / `test_parity_https` | 5 / 3 / 1 |
| `test_sock_write`, `test_issue_1852`, `2131`, `422`, `1123`, `1131`, `5021`, `647`, `1933` | 1,1,1,1,2,1,1,1,1 |

`test_gap_ws` matches no files; Perry's WebSocket coverage is elsewhere, and
P5 did not migrate that path.

## Perry-side defects this work found (not P5 regressions)

Each was reproduced on the base commit's hyper/tokio path too, so they are
pre-existing and worth their own issues rather than being folded into this
change:

1. **A `net.Socket` handed to an `'upgrade'` listener cannot be written to.**
   `socket.write(...)` returns `undefined` and nothing reaches the wire, on
   both transports. The listener receives the socket as an untyped value, so
   the call goes through the composite handle dispatch — and `net`'s
   `socket_method_name` table has no `write` row (it is normally reached through
   the statically resolved `js_ext_net_socket_write3`). The `'upgrade'` event
   itself is correct on both, arguments included.
2. **`res.writeHead(...)` followed by `res.end(body)` is framed differently
   from Node.** Node only computes a `Content-Length` while the header block is
   still open at `end()` time and falls back to chunked once `writeHead` has
   committed it; Perry length-frames both shapes. Hyper framed it the same way,
   so this predates P5.
3. **`socket.remoteAddress` is `undefined` on an accepted socket** — P1 recorded
   this and it is unchanged; the `'upgrade'` probe sees it too.
4. **`perry-ext-http`'s `tls_client::tests::needs_custom_client_logic` fails on
   `main`.** Reproduced alone, single-threaded, on the base commit: its very
   first assertion (`!t.needs_custom_client()` on a default `TlsOptions`) fails,
   so `perry_ffi::node_tls_client_environment()` is already reporting
   `NODE_TLS_REJECT_UNAUTHORIZED=0` or a CA list in that binary. Nothing here
   touches it.

## Environment notes for whoever runs this next

- **Build both trees with the harness's DEFAULT package set and NO
  `external-*-pump` features.** A stdlib built with `external-zlib-pump`
  references `js_ext_zlib_*` from `js_handle_method_dispatch`, so every test
  that pulls that object without linking `libperry_ext_zlib.a` fails to LINK
  and is reported as COMPILE_FAIL — indistinguishable from a real regression.
  The ext wrappers still get linked per-import through the compiler's
  well-known routing, and `perry-ext-http` registers its own pump and dispatch
  extensions at first use rather than needing the stdlib feature. This cost one
  full baseline gap run, whose fetch-test COMPILE_FAILs were entirely that.
  (`run_parity_tests.sh`'s own #7629 comment says the same thing.)
- **Auto-optimize needs the lockfile to already carry the new crates.** It runs
  a plain `cargo build`, without `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE`, so
  while `turnloop-http` / `turnloop-tls` are inside the 7-day
  `global-min-publish-age` window it fails to resolve and silently falls back to
  prebuilt archives that do not match. With the committed `Cargo.lock` there is
  nothing to resolve and it succeeds.
- **The box is shared with the P4 lane, which runs its own sharded gap suite.**
  The shard command lines do not name their tree, so a
  `pkill -f 'run_parity_tests.sh --filter'` matches theirs too — it did once
  here, killing three of their shards (they auto-resumed). Resolve
  `/proc/PID/cwd` and kill only your own.
- **Do not rsync a source mirror with `--delete` while a gap suite is running
  in it.** `test-parity/output/` is created by the harness at startup and
  written per test; deleting it under a live suite fails every remaining test
  with "No such file or directory".

## For the integrator

- Full gap suite in both tiers, and `cargo test --workspace`. The per-crate
  results here are: `perry-ext-net --lib` 36 passed; `perry-runtime turnloop_net`
  15 passed; `perry-ext-http --lib` 109 passed with one failure,
  `tls_client::tests::needs_custom_client_logic`, whose subject
  (`perry_ffi::node_tls_client_environment`) this change does not touch —
  confirm against the base commit before reading it as P5's.
- `./run_parity_tests.sh --suite node-suite --module http|https|net` — the
  behavioural corpora, far broader than the gap tests, with committed floors of
  22/53, 6/47 and 16/47.
- **A Windows arm.** Nothing here was run on Windows. The TLS session and the
  HTTP codec are platform-independent, but the accept path, `ListenOpts` and
  the error table are not.
- **An instruction A/B at cgu=1 with a control probe**, on a server-only
  workload. The tokio arm is `--features perry-stdlib/tokio-wait-driver`.
  Nothing here was benchmarked: the shared box was running another lane's gap
  suite throughout.
- The two trees are on the build box at `/root/claude-turnloop-p5/{base,perry}`
  (base at `14803019fc`), each with its own `target/`. Delete both when the A/B
  is done. `PERRY_RUNTIME_DIR` must be overridden per tree —
  `/etc/profile.d/perry.sh` points it at a different checkout.

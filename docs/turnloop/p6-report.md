# turnloop P6 — the outbound HTTP and SMTP clients

Branch `turnloop/p6-clients`, based on `turnloop/integration` at `7f77cce3c6`
(P0–P5 plus `main` through v0.5.1576). Built and tested on the shared Linux box
(`perrybuilder`, EPYC 9354P) against the pinned gap oracle Node **26.5.1**
(`/opt/node-v26.5.1-linux-x64/bin`, not the box default). Nothing here was run
on Windows or macOS, and nothing was benchmarked — see "What was not run".

## What this phase found, before what it changed

P6's scope is "outbound HTTP and SMTP". Measuring what Perry did first turned up
five divergences from Node that have nothing to do with the transport, and one
that makes a whole surface unreachable. Every one was reproduced on the **base
commit** before it was touched:

| subject | Node 26.5.1 | base `7f77cce3c6` |
|---|---|---|
| `fetch(url, { signal })`, `controller.abort()` | rejects `AbortError` | **runs to completion** — the abort never reached the request |
| a `Content-Encoding: gzip` response body | decompressed | **raw gzip bytes** handed to `response.text()` |
| `response.url` after a redirect | the final URL | **the original URL** |
| `response.redirected` after a redirect | `true` | **`false`** |
| a bodyless `POST` | `content-length: 0` | **no `content-length` at all** |
| `ECONNREFUSED`, `err.cause.code` | `"ECONNREFUSED"` | **`undefined`** |
| `transporter.sendMail(...)` / `.verify()` | — | **`TypeError: (number).sendMail is not a function`** |

The first is the sharpest. `url::abort::notify_fetch_abort` declared its stdlib
hook as an `extern` inside `#[cfg(feature = "external-fetch-symbols")]` and did
*nothing at all* in the other arm — and the other arm is the one a default
`fetch`-using build compiles to, because the global `fetch` is reached through
the registered `GLOBAL_FETCH_WITH_OPTIONS` pointer rather than a linked symbol.
So the entire `AbortSignal` path — `controller.abort()` **and**
`AbortSignal.timeout(ms)` — was dead for `fetch`, while `abort_bridge.rs`'s
`Notify` registry, the per-signal watch list and the `tokio::select!` all sat
there looking correct.

The last one is the widest, and it is **not fully fixed here**:
`nodemailer.createTransport()` returns a bare handle *number* (`NR_F64` in the
native table), so `transporter.sendMail(...)` has a receiver codegen types as a
primitive and lowers to a hard `js_throw_type_error_not_a_function` — the
runtime's handle dispatch is never consulted at all. Both methods fail, in every
call shape tried (module scope, inside an `async fn`, a `.then` chain, and with
the receiver annotated `any`), on the base commit and on this branch. **Perry's
nodemailer surface has never worked from JS.** See "SMTP" under test evidence
for what that costs this phase, and the defects section for the two halves.

## Dependencies, added and not removed

`turnloop-smtp 0.1.0-alpha.3` is added (default features only — the sans-I/O
`Connection` plus the `message` module, **not** the `turnloop` feature, which
would pull `turnloop-io`'s `LocalExecutor`; see "Why sans-I/O" below).
`turnloop-http` and `turnloop-tls` were already in the tree from P5;
`turnloop-http`'s `client` and `compression` modules are new consumers here.
`url` and `http` are named directly by perry-stdlib — both were already in its
graph through reqwest.

**reqwest, hyper and lettre are NOT removed.** The decline table below is the
reason, not reluctance: a proxy, a worker agent with no loop, and the
`tokio-wait-driver` A/B arm are all reachable and all still exercised.
`turnloop-smtp`'s `message` module re-exports the same `lettre` 0.11 builder
Perry's nodemailer surface already used, so lettre stays in the graph regardless
— which is also what makes the MIME bytes byte-identical across the migration.

## What moved, and what did not

| outbound surface | transport after P6 | why |
|---|---|---|
| global `fetch()` — `js_fetch_get`, `…_get_with_auth`, `…_post`, `…_post_with_auth`, `js_fetch_with_options`, `js_fetch_text` | **turnloop** + `turnloop_http::{client,http1}` | — |
| `fetch` over `https:` | **turnloop** + `turnloop-tls` (unbuffered rustls) | — |
| `undici.fetch`, and `undici.fetch` under a plain `Agent` dispatcher | **turnloop** | perry-ext-undici is glue over the same `js_fetch_*` symbols, so it moved for free; a plain `Agent` clears the proxy and the engine takes it |
| `js_nodemailer_send_mail` / `js_nodemailer_verify`, bundled surface | **turnloop** + `turnloop-smtp` | reachable from Rust; see the note below |
| the same two through `perry-ext-nodemailer` (what `import 'nodemailer'` selects) | **turnloop**, through the `js_perry_smtp_*` C seam | ditto |
| a fetch through a proxy (`HTTP_PROXY`, or `undici.setGlobalDispatcher(new ProxyAgent(…))`) | reqwest | Perry's proxy surface is a prebuilt `reqwest::Client`, not a URL a CONNECT tunnel could be driven from |
| a fetch on a `worker_threads` agent | reqwest | that agent has no loop (P3/P4 left per-agent loops to a later phase) |
| any fetch in the `tokio-wait-driver` A/B arm | reqwest | there is no loop at all |
| `js_fetch_stream_start` (the SSE line-poll surface) | reqwest | see "What P6 did not do" |
| `axios` (`perry-ext-axios`, and the stdlib mirror) | reqwest | ditto |
| `node-fetch` (`perry-ext-fetch`) | reqwest | ditto |
| `node:http` / `node:https` **client** (`http.request`, `https.get`) | reqwest | ditto |
| `http2.connect()` | `h2` + its own private tokio runtime | ditto |

This is a narrowing, not a removal — the same shape P1 left the tokio socket
task in, and for the same reason.

## Why sans-I/O, and not `turnloop_http::asynchronous::client`

The same reason P5 gave for the server, and it applies unchanged to the client
and to `turnloop_smtp::asynchronous`:

* `LocalExecutor::with_config` constructs its **own** `Driver`. Perry already
  owns one `turnloop::Loop` per agent, and a second loop in the same process is
  exactly the mixed-transport deadlock P1 had to paper over with a 1 ms tick
  slice.
* Even sharing one, `LocalExecutor::turn` drains completions into
  `Shared::dispatch`, which returns early for any token without its own tag bit.
  P1's net tokens, P2's process tokens, P3's timer token and P4's pool tokens
  would all be **silently dropped** — no error, no counter (PerryTS/turnloop#45).

So the codecs are driven sans-I/O over P1's completion layer, which is what
DESIGN §5b asks for and what keeps DESIGN D1 true.

## Architecture

```
submit(spec) ─► client::Pool::acquire ─► turnloop_net::tcp_connect_host
                                              │ NET_CONNECT
                                              ▼
                                  [TlsClientSession handshake]      (https)
                                              │ NET_DATA
                                              ▼
              client::Http1Connection::start ─► turnloop_net::write
                                              │ NET_DATA
                                              ▼
                [TLS decrypt] ─► Http1Connection::receive ─► Event::Head
                                                             Event::Body
                                                             Event::End
                                              │
                                              ▼
                            client::Request::redirect ── resend ──┐
                                              │ final             │
                                              ▼                   │
                       compression::StreamingDecoder ─────────────┘
                                              │
                                              ▼
                        Sink::on_done ─► queue_promise_resolution
```

SMTP is the same shape with a different protocol object:

```
send(config, job) ─► turnloop_net::tcp_connect_host
                             │ NET_CONNECT
                             ▼
                   Connection::connected ─► 220 greeting ─► EHLO
                             │ Event::UpgradeTls   (STARTTLS or implicit)
                             ▼
                   TlsClientSession ─► Connection::tls_established
                             │ EHLO ─► AUTH ─► Event::Ready
                             ▼
                   Connection::send(envelope, message)
                             │ Event::Sent { info } / Event::Failed
                             ▼
                   Sink::on_done ─► queue_deferred_resolution
```

Four rules hold it together, three of them inherited:

1. **The sink runs no JS.** It runs inside `dispatch_staged`, after a turn has
   returned. It may build Rust state and insert a `FetchResponse`; the promise
   is settled through the existing deferred-resolution queue, whose converter
   runs on the owning thread. (P5's rule, unchanged.)
2. **One request in flight per connection.** `Http1Connection` refuses to
   `start` while a response is outstanding, so HTTP/1 pipelining cannot happen
   by accident. That is also Node's per-connection serialization.
3. **No JS value and no heap pointer reaches the driver.** Reads land in
   turnloop's pooled buffers and are copied inside the dispatch call; writes are
   owned `Vec<u8>`s. (P1's rule, unchanged — and the reason neither engine
   registers a GC root scanner.)
4. **A sink never runs while the engine's tables are borrowed.** A completed
   request is pushed onto a `pending` list and delivered by `drain_pending`
   after the `RefCell` is released, because a sink may submit the *next* request
   (a redirect chain in JS, a `Promise.all` fan-out) and would otherwise re-enter
   the same borrow.

## Ids, and the collision that was waiting to happen

`turnloop_net` keys **every** handle on a thread in ONE `HashMap<i64, Entry>`,
regardless of subsystem. And perry-ffi's handle registry (which `perry-ext-net`
names its sockets from) and perry-stdlib's `common` registry are two *different*
registries over the *same* numeric range, `[1, 0x40000)` — so an id allocated
naively from either would have collided with a live `net.Socket`, and the
failure mode is one subsystem's completion reaching another's socket.

Both P6 engines therefore allocate from private bands far above both:
`1 << 40` for the HTTP client (subsystem slot 2) and `1 << 45` for SMTP
(slot 3), each ceilinged well inside `turnloop_net`'s 56-bit token field. Both
are asserted in tests rather than left to the comment.

## GC decisions

* **No new roots, and no root scanner.** A request holds owned `String`s and
  `Vec<u8>`s and the `usize` address of a promise from
  `js_promise_new_cross_thread`, which pins it across the crossing (#9552) —
  exactly the exposure the reqwest path already had, for the same duration.
  Read payloads are copied out inside the dispatch call.
* **The TLS session holds no JS value either** — only owned ciphertext and
  plaintext buffers.
* **The sinks are plain `fn` pointers, not boxed closures.** A thread-local
  engine table must hold nothing a moving collector could invalidate, and a `fn`
  is exactly that. The caller's key (`ctx`) is the pinned promise address.
* `scripts/gc_runtime_root_holders.py` has **four** new researched verdicts —
  the two `MESSAGE_IDS` maps (`HashMap<usize, String>`), the SMTP seam's
  `DRAFTS` and `NEXT_DRAFT`, and its `CALLBACKS` map of `extern "C"` fn
  pointers. None holds a NaN-boxed value or a heap pointer. The engines' own
  `ENGINE` / `STATE` thread-locals are not flagged by the gate at all, because
  the structs behind them contain no raw pointer to flag.
* **The gate also went red on five entries this work never touched** — P5's
  `CONNS` and four `regex/site_test.rs` test counters flipped from UNCOVERED to
  COVERED, which makes an inventory entry stale, and a stale entry fails. That
  is P1's finding repeating: the script resolves function names *across* crates,
  so adding reachable bodies to a registering crate pulls unrelated text into a
  scanner's reachable set. The five entries are deleted here, as the gate
  instructs; what is lost is the *record* of their reasoning, which is the cost
  P1 already flagged and which per-crate name resolution would remove.

## Connection pooling and keep-alive — measured, then preserved

Perry's reqwest fetch client has always been built with
`pool_idle_timeout(90 s)`, `pool_max_idle_per_host(16)` and
`tcp_keepalive(60 s)` (`fetch_client_builder`). The turnloop engine uses
`turnloop_http::client::Pool` with **the same two numbers**, so a long-running
service's socket behaviour does not change.

Two things the turnloop path has to do that reqwest did for itself:

* **An idle pooled socket must not keep the process alive.** A turnloop handle
  is referenced by default, so a program that finished its work would never
  exit. On release the socket is `set_ref(false)`d and the pool's idle deadline
  is armed as a real `NET_TIMER` (P5's primitive), unreferenced like the agent's
  own timer deadline. Re-acquiring cancels it and re-references the socket.
* **The idle-connection race.** A reused socket the peer closed while it sat in
  the pool fails before any response byte; such a request is retried **once** on
  a fresh connection, and only if it is replayable. A *fresh* connection's
  failure is never retried.

`tcp_keepalive(60 s)` is the one setting NOT carried over: turnloop's
`TcpOpts` exposes `nodelay` and nothing else, so `SO_KEEPALIVE` cannot be set
on a socket it owns. That is P1's `setNoDelay` finding from the other side and
it wants the same turnloop socket-option API. The practical difference is a
connection to a peer that vanishes without a FIN: reqwest's kernel keepalive
would have reaped it after a minute; here it sits idle until the pool's own
90-second deadline closes it, which is the same order of magnitude and strictly
bounded.

## HTTP/2 — a decision, not an omission

The turnloop client advertises **only `http/1.1`** in ALPN, so no server can
select h2 on this path. That is checked rather than assumed: after the handshake
the negotiated protocol is compared against `http/1.1` and a mismatch fails the
connection with `ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL`.

This is a real behavioural change. reqwest is built with its `http2` feature, so
before this an `https://` fetch to an ALPN-capable origin negotiated h2 and
multiplexed over one connection; now it uses HTTP/1.1 with the pool above.
Nothing observable from JS changes (status, headers, body and timing semantics
are identical), but a service issuing many concurrent requests to one h2 origin
now opens up to `max_per_host` sockets instead of one.

`turnloop_http::http2::Connection` is sans-I/O and `client::Pool` already models
an h2 slot's `max_streams`, so the piece that is missing is the HPACK/flow-
control driving, not the plumbing. Routing a fetch to the existing reqwest path
on an h2-capable origin was rejected as the alternative: it would mean deciding
the transport *after* the TLS handshake, on a socket turnloop owns, which is the
descriptor-handoff problem P1 was blocked on.

## `Content-Encoding` — the gap this closed

**No reqwest decompression feature is enabled anywhere in the workspace** —
not in perry-stdlib, perry-ext-fetch, perry-ext-http or the workspace default.
So Perry sent no `Accept-Encoding` of its own and, when a caller set one
explicitly (common in ported axios/got code), handed the *compressed bytes* to
`response.text()`. That is the base row in the table at the top of this report.

The turnloop path decodes `gzip`, `x-gzip`, `deflate` (with raw-deflate
detection), `br` and `zstd` through `turnloop_http::compression`, bounded by a
512 MiB limit so a decompression bomb cannot exhaust the heap. An encoding the
crate does not implement is left encoded — which is no worse than the reqwest
path, where *every* encoding was.

**Perry still sends no `Accept-Encoding` header of its own.** That is
deliberate: adding one would change the bytes of every outbound request and is a
separate decision from decoding a response that carries the header anyway. Node
sends `accept-encoding: gzip, deflate, br, zstd`; Perry sends none, on both
transports. It is why `test_gap_turnloop_fetch.ts` prints request headers
through an allowlist.

## Abort and timeout semantics

An aborted fetch cancels the in-flight operation on the loop **exactly once**:
the request is detached from its connection, `turnloop_net::close` is submitted
(which cancels the socket's outstanding operations), and the request is
delivered as `AbortError` through the exactly-once `delivered` guard. A second
`controller.abort()` finds no entry and is a no-op; the `NET_CLOSED` that
follows finds the request already delivered and does not settle it again.

`AbortSignal` reaches the engine through `js_fetch_notify_signal_aborted`, which
now also calls `turnloop_client::abort_signal(key)`. Getting there needed the
runtime-side fix described at the top: the hook is registered next to the fetch
hook (`js_register_global_fetch_notify_abort`) rather than being a linked
`extern` compiled in under a feature a default build does not carry.

Per-phase deadlines (`connect`, `headers`, `body`) exist in `client::Lifecycle`
and **this engine wires none of them** — not `next_timeout`, not
`handle_timeout`, not `set_body_deadline`. `Http1Connection::start` is called
with `None` for both deadline arguments. That is deliberate and it is a gap, not
a design: the reqwest fetch path set no `.timeout()` either, so arming one here
would reject requests that previously succeeded — but it also means a server
that accepts a connection and then says nothing holds a socket until the peer or
the OS gives up, where Node's undici would have raised
`UND_ERR_HEADERS_TIMEOUT`. Wiring it wants its own change with its own oracle
measurement, because every default it picks is observable.

The pool's own deadline IS armed, as a `NET_TIMER` per connection
(`arm_idle_timer`), rather than through `Pool::next_timeout` /
`Pool::handle_timeout` — a per-connection turnloop deadline is what puts the
close in `Loop::next_deadline()`, which a host-side scan of the pool would not.
The Pool methods this engine uses are exactly `acquire`, `connected`, `release`
and `closed`.

## The keep-alive gate, and the fixture that hid it needing one

A turnloop handle is referenced by default, which keeps `Loop::turn` blocking —
but it does not keep *Perry's event loop* running. That decision is the runtime's
`AUX_HAS_ACTIVE` registry, and an engine that registers nothing there is a
program that exits with "Detected unsettled top-level await" the moment its only
outstanding work is an outbound request.

This was found late, and by the right instrument rather than by luck: the fetch
gap fixture passed throughout, because it runs a local `node:http` server, and
**the server was holding the loop open**. The standalone remote probe — one
`fetch` and nothing else — exited after a single turn with
`http_submitted=1 completed=0`. Both engines now register an
`aux_has_active` contributor.

Deliberately NOT `InflightGuard`, which the reqwest path used: that counter also
feeds `native_work_inflight`, which makes the park choose the legacy tokio tick
instead of a turn (P4's note 2). A fetch on the turnloop path would then have
driven tokio to wait for work tokio was not carrying, and `tokio_ticks=0` would
have been false.

## Test evidence

Every command as run, on the shared Linux box, against Node **26.5.1**.

### Unit tests — the codecs and the policy, driven with real bytes

`turnloop_http::client`, `turnloop_http::http1`, `turnloop_http::compression`
and `turnloop_smtp::Connection` are all sans-I/O, so the parts of this phase
that decide *correctness* can be tested without a socket, and they are.

```
RUST_TEST_THREADS=1 cargo test --release -p perry-stdlib --lib turnloop_client  -> 10 passed
RUST_TEST_THREADS=1 cargo test --release -p perry-stdlib --lib turnloop_smtp    -> 8 passed
```

`turnloop_client` (10): the id band proven disjoint from both handle registries
and from the SMTP engine's; the `Event::End` regression test described below;
the framing decision for a bodyless GET / bodyless POST / sized POST, checked
against the bytes `Encoder::start` writes; the redirect policy (303 → GET with
the body dropped, cross-origin credential stripping, `manual`, the hop limit);
the pool reusing within an origin, refusing to overbook, and ageing a
connection out; every `Content-Encoding` round-tripped by content — whole-body
*and* chunk-by-chunk, because the chunked path is the one a real response takes;
the error-code re-interning, including the degrade-to-generic case; the debug
knob asserted OFF by default; every URL shape that must DECLINE rather than
fail; and the pool contract the admission fix rests on — that CLOSING a
connection frees an origin's seat exactly as releasing one does.

`turnloop_smtp` (8): a full delivery asserted command by command (EHLO, the
capability parse, `AUTH PLAIN`, `MAIL FROM … SIZE=`, per-recipient `RCPT TO`,
`DATA`, the terminated body, and the `Sent` event's token / `accepted` /
`rejected` / `response` / envelope); dot-stuffing asserted on the bytes,
including that the terminator cannot appear inside the body; a rejected
recipient failing the delivery rather than reporting success; STARTTLS
re-issuing EHLO on the secure channel and discarding the cleartext capability
list; implicit TLS writing nothing in the clear; a `421` ending the session;
and the id band and error-code interning.

Two of the eighteen failed on their first run, both because the *test* assumed
something the protocol does not do: the gzip flush error a decoder that has
already produced everything answers with, and — the more interesting one — that
`MAIL FROM` / `RCPT TO` / `DATA` are four round trips. The server in that test
advertises `PIPELINING`, so `turnloop_smtp` writes the first three as one block
and holds `DATA` back until their replies arrive. Asserting per command was
asserting the absence of pipelining; the test now asserts the block's contents
and order, and that `DATA` is **not** in it.

### `fetch`, byte-for-byte against the oracle

`test-files/test_gap_turnloop_fetch.ts` — a local `node:http` server and
thirteen cases: a plain GET (status, `statusText`, `ok`, headers, body); a POST
with a JSON body and a custom header, echoed back; a bodyless POST's framing; a
404; a followed redirect with `url` and `redirected`; a `Content-Encoding: gzip`
body; a binary body through `arrayBuffer()`; five requests in flight at once; an
abort; an already-aborted signal; `ECONNREFUSED` with `cause.code`/`syscall`;
`getaddrinfo ENOTFOUND` with Node's `errno`; and connection reuse.

```
target/release/perry test-files/test_gap_turnloop_fetch.ts -o /tmp/p6fetch
diff <(node --experimental-strip-types test-files/test_gap_turnloop_fetch.ts) <(/tmp/p6fetch)
```

→ **byte-identical**. The oracle output was pinned three times before Perry ever
ran the file, and the same file on the **base commit** differs on six lines —
the six rows in the table at the top of this report.

Nothing host-specific is printed: no port, no `Date`, no `user-agent`, no
`accept-encoding` (see the note above), and the reuse assertion is the property
(`connections < 12` for eighteen requests) rather than an exact count, because
the engines pool differently.

### `PERRY_LOOP_STATS`, both arms, same workload

| | base `7f77cce3c6` | P6 |
|---|---|---|
| `turns` | 48 | **53** |
| `completions` | 85 | **145** |
| `native_ticks` | **32** | **0** |
| `tokio_ticks` | **32** | **0** |
| `turnloop_wait_ns` | 0 | 2,676,017 |
| `tokio_tick_ns` | 44,638,675 | **0** |
| P6 line | — | `http_submitted=15 declined=0 completed=12 failed=3 connects=5 reused=9 redirects=1 decoded_bodies=1` |

`declined=0` is the load-bearing number: every one of the fifteen requests took
the turnloop path, so a green comparison is not a comparison of the reqwest
path against itself. `connects=5 reused=9` says the pool was live, and
`decoded_bodies=1` says the decompressor really ran.

### The five-second bug this caught, and the test that pins it

The first working build answered every request correctly and took **~5 seconds
per fetch**, with `turnloop_wait_ns=60008423200` over the run and zero
connection reuse. The cause: `http1::Decoder` emits `Event::End` from a step
that consumes **zero** bytes (`State::End -> Done` is a transition, not a
parse), and the feed loop stopped at `pos >= input.len()`. The response never
completed on data alone; the only thing that finished it was the server's
keep-alive timeout closing the socket — which also made every connection
unreusable, and which is why the symptom was *latency plus no pooling* rather
than a hang.

`the_end_event_arrives_from_a_step_that_consumes_nothing` drives both loop rules
against the same real response bytes and asserts the old one does **not** see
`End` while the new one does and leaves the connection reusable. It fails if the
defect is ever reintroduced, and its first assertion fails if the defect becomes
unreachable — so it cannot quietly stop discriminating.

### A real remote endpoint, through TLS

`scripts/turnloop/apps/p6_tls_remote.ts` — not a gap fixture, because it needs
the network. It asserts what a loopback test cannot: a real certificate chain
verified against the webpki roots, a real cross-origin `http:` → `https:`
redirect, and six TLS requests in flight at once.

| | Node 26.5.1 | P6 |
|---|---|---|
| `https://example.com/` | `200 true ctype=text/html bytes=true` | identical |
| `https://api.github.com/meta` | `200 true ctype=application/json bytes=true` | identical |
| `http://github.com/` → https | `200 redirected=true https=true` | identical |
| `https://expired.badssl.com/` | `rejected CERT_HAS_EXPIRED` | identical |
| six concurrent TLS requests | `200,200,200,200,200,200` | identical |

`[perry-loop] driver=turnloop turns=207 … native_ticks=0 completions=228` and
`p6 http_submitted=10 declined=0 completed=9 failed=1 connects=10 reused=1
redirects=1`. The one failure is the expired certificate, which is the correct
outcome; `tokio_ticks=0`.

**This probe found two defects that every loopback fixture passed through.**

1. **No default `User-Agent`.** Perry's reqwest client sets
   `user_agent("perry/<version>")` deliberately — #236 is about
   `api.github.com` rejecting anonymous requests — and the turnloop path sent
   none. `api.github.com/meta` answered **403** where Node answered 200. A
   caller's own header still wins, as `RequestBuilder::header` did.
2. **Unconsumed decoder input was not retained.** `http1::Decoder`'s contract is
   that the host keeps what a step did not consume; a response head that has not
   reached its blank line consumes nothing and returns no event. Feeding only
   the newest read threw the earlier half away, so any response whose HEAD spans
   two reads failed with `HPE_INVALID_HEADER_TOKEN: invalid response head`.
   `https://github.com/` is such a response; `example.com`, `google.com` and
   `crates.io` are not, and neither is anything a local fixture serves. That is
   the reason this probe exists, and it is the strongest argument in this report
   for not accepting loopback-only evidence for a client.

### Abort, mid-body

`scripts/turnloop/apps/p6_abort_midbody.ts`: the server sends the head and 100
of 1000 declared body bytes, then stalls; the client aborts.

| | Node 26.5.1 | P6 |
|---|---|---|
| `server-stalled` | true | true |
| abort surfaces at | `res.text()` (Node resolves at the head) | the `fetch()` await (Perry buffers the body) |
| error | `AbortError` | `AbortError` |
| a second `abort()` | no second settlement | no second settlement |
| the next request on the same loop | 200 `after-abort-ok` | 200 `after-abort-ok` |
| three more concurrent | 200,200,200 | 200,200,200 |
| P6 counters | — | `http_submitted=5 completed=4 failed=1 connects=4 reused=1` |

`completed=4 failed=1` for five submissions is the exactly-once assertion from
the other side. Where "mid-body" falls differs between the engines because
Perry's fetch buffers the whole body before resolving where Node streams it —
a pre-existing difference this phase did not change, and the probe prints both
shapes so neither engine can pass by accident.

### GC stress, with requests in flight

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
  PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 ./p6fetch
```

| seed | copying minors | objects moved | from-space quarantines | `[gc…]` lines | P6 counters |
|---|---|---|---|---|---|
| 1 | 105 | 13,741 | 105 | 4,419 | `http_submitted=15 completed=12 failed=3 connects=5 reused=9` |
| 7 | 105 | 13,741 | 105 | 4,419 | same |
| 12345 | 105 | 13,741 | 105 | 4,419 | same |

All three exit 0 with **stdout byte-identical to the unstressed run**, and the
instruments prove they were armed rather than merely quiet: 105
`[gc-fromspace-protect] retired_set=` lines say the from-space really was
detached, poisoned and `mprotect`ed, and 13,741 moved objects say survivors
really were copied — while fifteen requests, five connections and nine pool
reuses were in flight. No SIGSEGV from the quarantine reporter: no stale
from-space pointer was dereferenced. Identical counts across seeds is the
documented behaviour at `RATE=1`, where every handled safepoint collects and the
seed stops selecting.

### SMTP

`turnloop-smtp`'s `Connection` is sans-I/O, so the protocol is driven in
`turnloop_smtp/tests.rs` with real bytes and no socket — greeting, EHLO,
capability parsing, `AUTH PLAIN`, `MAIL FROM` with `SIZE`, per-recipient
`RCPT TO`, `DATA`, the dot-stuffed body, the `Sent` event's `accepted`/
`rejected`/`response`, a rejected recipient, STARTTLS re-issuing EHLO on the
secure channel, implicit TLS writing nothing in the clear, and a `421`.

**There is no end-to-end SMTP evidence, and that is the honest state of it.**
`test-files/test_turnloop_p6_smtp.ts` exists and drives a scripted SMTP
responder through `nodemailer`, but it cannot run: `transporter.sendMail(...)`
throws `TypeError: (number).sendMail is not a function` before any native code
is reached — on this branch **and on the base commit**, in every call shape
tried. So the engine below it is proven at the protocol level and unproven at
the surface level, and no claim is made here that a Perry program's mail now
goes over turnloop. What IS demonstrated: the engine drives the protocol
correctly against real bytes, the C seam links (the ext wrapper's externs
resolve once the driver re-asserts `turnloop-smtp-client`), and the two missing
dispatch rows are now present.

Be precise about what "unproven" covers. It is not only the JS surface: **the
SMTP transport wiring has never run** — `tcp_connect_host`, the `NET_DATA`
plaintext path, the STARTTLS install and the `flush` that routes protocol output
through the TLS session are reviewed and compile-checked and nothing more. The
HTTP engine's equivalents are exercised hard (fifteen requests per fixture run,
real TLS to four public origins, three GC-stress seeds); SMTP's are not
exercised at all. A reviewer should read that code rather than trust this
report's protocol evidence to cover it.

The fixture is also **not** a gap test for a second reason: `nodemailer` is not
in the repository's `package.json`, so the Node oracle cannot import it, and
adding a dependency to satisfy one fixture is a supply-chain decision this lane
should not make on its own.

What the remaining half needs, precisely: `js_nodemailer_create_transport` would
have to return a **handle-band NaN-boxed pointer** rather than a raw double (the
shape `fetch`'s `handle_to_f64` uses), so the receiver is an object and the
method call routes through `HANDLE_METHOD_DISPATCH` instead of being refused by
codegen. That also makes `typeof transporter === "object"`, which is what Node
reports. It is a two-sided change — the statically typed native-table rows take
the receiver as a raw `Handle` today — and it belongs with whoever owns that
binding rather than in a transport migration.

### The exactly-once bug an independent review found, and the test that pins it

The first build that passed everything above still had a **hang** in it, and no
fixture in this report could see it. Admitting a request the pool had parked at
`Acquire::Wait` lived only inside `release()`, and `release()` is reached from
exactly one place — `on_end`, a response that completed normally. Every failure
path (a connect error, a TLS failure, a `NET_ERROR`, an EOF with no head, an
idle close, an abort) goes straight to `close_conn()`, which retires the pool
seat and never looked at `engine.waiting`.

So: sixteen concurrent requests to one origin that all **fail** leave the
seventeenth parked forever. Its sink is never called, and
`has_pending_requests()` then keeps Perry's event loop alive on a promise that
can never settle — the process does not exit. Meanwhile a *later* request to the
same origin connects immediately and overtakes it.

`test-files/test_gap_turnloop_fetch_pool_wait.ts` fires twenty-four concurrent
fetches at a port nothing listens on — a connect error, which is precisely the
path `release()` never sees — and then a second round of twenty-four at the same
origin. It **deliberately does not call `process.exit()`**: reaching the last
line and returning is the assertion, because a fixture that only checked "all
twenty-four rejected" would pass while the loop still refused to drain.

| | unfixed build | fixed build |
|---|---|---|
| exit code, 45 s budget | **124 (timed out)** | **0** |
| output | **none at all** | byte-identical to Node, 3 runs |
| P6 counters | — | `http_submitted=48 declined=0 failed=48` |

The unfixed build printed *nothing* — it hung before the first `console.log`,
because `Promise.allSettled` over the first round never resolved.

Two smaller defects from the same review are fixed with it: a connection whose
idle timer could not be armed was pooled anyway, where nothing would ever
reclaim it (it is closed now — `timer_arm` fails only with no loop, impossible
there, or when the net profile's 4096-handle table is full, which is reachable
on a busy server); and `close_conn`'s `pooled` parameter, whose two arms did
exactly the same thing, is gone. `turnloop_smtp`'s `pump()` now *reports* an
exhausted event budget as a failure with a live counter
(`pump_exhausted=` on the stats line) instead of falling out of the loop
silently, which would have stalled an exchange with no way to notice.

`a_closed_connection_frees_a_seat_exactly_as_a_released_one_does` pins the pool
contract the engine reasons from, so a `turnloop-http` change that altered it
fails there rather than as a hang in a fixture.

### GC stress, and one fixture that refused to be counted

Re-run on the fixed build, unchanged from the table above: 105 copying minors,
13,741 moved objects, 105 from-space quarantines, stdout byte-identical, on all
three seeds.

The pool-wait fixture was stressed too and **exited 70 with the instrument's own
refusal**: `loop_polls=0`, "THIS RUN EXERCISED NOTHING WORTH TRUSTING … every
collection came from an event-loop boundary and no loop body was covered."
Its body is `Promise.allSettled` over an array map, a lowering codegen emits no
back-edge poll for. That is the instrument working — it declined to let a clean
exit be read as coverage — and it is why the GC evidence in this report rests on
the fetch fixture (33 loop polls) rather than on this one.

### The full gap suite, against this branch's own base

Both trees built identically — the harness's default package set plus the
`perry-ext-*` wrappers this work links (`http`, `net`, `ws`, `zlib`, `events`,
and `nodemailer` on the P6 arm, which links nothing extra into any gap test
because no gap test imports it), in one cargo invocation, with **no**
`external-*-pump` features — and run as
`PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh`.

| | base `7f77cce3c6` | P6 |
|---|---|---|
| tests | 805 | **807** (the two new fixtures) |
| pass | 796 | **798** |
| parity_fail | **9** | **9 — the same nine** |
| compile_fail | 0 | **0** |
| crash | 0 | **0** |
| parity rate | 98.8 % | 98.8 % |
| **status changes on the 805 common tests** | — | **0** |

Compared per test from the two JSON reports, not from the totals: every one of
the 805 tests both arms ran has the identical status, the two tests only P6 has
are the new fixtures, and both pass. Both arms exit 1 for the *same* reason —
the three tests the committed snapshot expects to pass and which are red on the
base commit before this branch changes anything.

Both trees were built with the identical package set:

```
cargo build --release --locked \
  -p perry -p perry-runtime -p perry-stdlib -p perry-runtime-static -p perry-stdlib-static \
  -p perry-ext-http -p perry-ext-net -p perry-ext-ws -p perry-ext-zlib -p perry-ext-events
```

The `perry-ext-*` wrappers are in the same invocation as `perry-stdlib-static`
on purpose (#7629): the gap suite adds them to its own build only for the
node-suite, so a gap run links whatever wrapper archive is already in the tree,
and an incoherent one makes every http/net fixture fail to *compile* with "the
wrapper archive bundles a DIFFERENT tokio compilation than the stdlib archive" —
indistinguishable from a real regression. `compile_fail 0` in both arms is what
says that did not happen here.

And the subject was asserted live before the sweep was believed: a hand-compiled
`test_gap_turnloop_fetch` reports `turns=53 completions=145 native_ticks=0
tokio_ticks=0` with `p6 http_submitted=15 declined=0`. A green sweep over a
build where the engine never ran would prove nothing.

The base's nine, none of them touched by this work:
`2159_defineproperty_class_prototype`, `2514_settracesigint`,
`2899_2779_2777_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch`, `json_lazy_defineproperty_index`,
`perfhooks_3088_3008_3010_3011`, `prop_plan_cache_invalidation`,
`v8_2_3680plus`. Three of those the committed snapshot expects to PASS, so the
gate is red on the base commit before P6 changes anything — which is exactly why
this comparison is against the base rather than against the snapshot.

### What was not run

Named precisely.

* **Windows and macOS.** Everything in this report ran on Linux x86_64.
* **A benchmark.** The box was running another lane's work throughout, and the
  brief forbids timing there. The five-second finding above is a *latency
  defect*, measured as the difference between 60 s and 2.4 ms of loop wait on
  one fixture — not a performance claim.
* **The auto-optimize gap tier.** Only the fast tier ran.
* **`cargo test --workspace`.**
* **A build of the `tokio-wait-driver` A/B arm.** Both engines decline on it
  (`turnloop_net::available()` is false without a loop) and
  `event_pump::register_stats_reporter` has an explicit no-op under that
  feature, so it *should* compile and fall back cleanly — but that is reasoning,
  not a build. `cargo build --features perry-stdlib/tokio-wait-driver` is one
  command and it was not run here.
* **The `node-suite` corpora.** P5 ran `--suite node-suite --module http|https|
  net` as its behavioural gate; the equivalent for a client would be the
  `http`/`https` client fixtures, and they were not run.

## What P6 did not do

Named precisely, because each is a hole rather than a preference, and each is a
path a real program still reaches.

* **`axios`.** `perry-ext-axios` is a real reqwest implementation in a
  separately linked `staticlib`, and `import 'axios'` routes there — so the
  stdlib mirror (`perry-stdlib/src/axios.rs`) is dead for any program that
  imports it. Moving the wrapper needs an HTTP C seam of the shape
  `js_perry_smtp_*` has, which is a second ABI's worth of design, and the crate
  has a defect of its own first (a fresh `reqwest::Client` per request — see the
  defects section). Migrating it without fixing that would be moving a bug onto
  a new transport. The stdlib mirror was left alone rather than migrated in
  isolation, because a change only reachable under `PERRY_DISABLE_WELL_KNOWN=1`
  is an untested configuration, which is exactly what CLAUDE.md's kill-policy
  says not to ship.
* **`node-fetch` (`perry-ext-fetch`).** Same shape, same seam missing. It is a
  near-duplicate of the stdlib fetch and has its own defect (no `AbortSignal`
  wiring at all), so it wants the duplication resolved rather than the
  duplication migrated.
* **The `node:http` / `node:https` CLIENT.** `http.request` / `https.get` in
  `perry-ext-http` — reqwest, plus three raw-`tokio::net::TcpStream` bypasses
  (`TE: trailers`, `Expect: 100-continue`, and an `agent.createConnection`
  override). P5 migrated that crate's SERVER; the client half is the larger of
  the two surfaces (`agent.rs` alone is ~1,950 lines with a second Node-
  semantics pool layered over reqwest's) and it is its own change.
* **`http2.connect()`.** Untouched, like P5 left `http2.createSecureServer`.
* **`js_fetch_stream_start`** — Perry's line-oriented SSE poll surface, still on
  reqwest. The engine carries the hooks for it (`Sink::on_head` / `on_chunk`
  stream the final response's decoded body as it arrives, and a followed
  redirect's body is deliberately withheld from them) and **nothing calls
  them**: that surface is a separate line-splitting state machine and wiring it
  is not a transport change.

  By CLAUDE.md's GC-knob kill-policy — "a mode that still exists is a decision
  that hasn't been made" — those ~25 lines should be deleted rather than left
  unexercised. They were not, for one reason and it is a schedule reason: the
  gap sweep in this report was running against the built tree when that became
  clear, and changing the engine would have invalidated it. The integrator
  should treat it as a live choice: deleting `on_head`/`on_chunk`, the
  `streaming` flag and the three branches that read them is a self-contained
  subtraction, and the report's numbers stay true either way because no test
  exercises them.

  Also note that Perry's WHATWG `response.body` is **not** affected: it is
  backed by the already-buffered body on both transports, so buffering the
  response here is parity rather than a regression.
* **`AbortSignal` on the `perry-ext-fetch` route.** Fixed for the global
  `fetch`; that crate still has no wiring.
* **Per-phase request deadlines.** `client::Lifecycle` exists and nothing in
  this engine touches it — see "Abort and timeout semantics". A stalled server
  therefore holds a socket until the OS gives up, where undici raises
  `UND_ERR_HEADERS_TIMEOUT`. The reqwest path had the same hole, so this is not
  a regression, but it is the most user-visible thing on this list.

## turnloop gaps found

Reported here in the shape P5's were; the coordinator files them.

1. **`http1::Decoder` emits `Event::End` from a step that consumes zero bytes,
   and nothing says so.** `State::End -> Done` is a transition, not a parse, so a
   host that stops feeding once every byte has been handed over never sees the
   response complete. Taking `Step { consumed, event }` at face value — "loop
   while there is input" — produces a client that works and is five seconds
   slower per request, because the only thing that finishes the exchange is the
   peer's idle timeout. A `Decoder::wants_step()` predicate, or one line in the
   `Step` docs, would have cost nothing. (Same asymmetry class as P5's finding
   about `Event::Upgrade` on the request side.)
2. **`client::Pool` has no way to ask which connection a `ConnectionId` is.**
   `Acquire::Reuse(id)` hands back an id whose socket the host must find in its
   own table; if the two ever disagree (a socket closed without the pool being
   told) the host has to recover by releasing-and-closing the slot and retrying.
   A `Pool::contains(id)` or a `release_unknown` would make the recovery path
   expressible rather than improvised.
3. **There is no client-wide `next_timeout`.** `Pool::next_timeout` and
   `Lifecycle::next_timeout` each answer for one structure, so a host with N
   in-flight requests must `min()` across N connections every turn to decide
   what deadline to arm. That cost is the reason this engine does not wire the
   per-phase deadlines at all (see "Abort and timeout semantics") and arms only
   a per-connection idle timer instead: one deadline per socket is expressible
   with `turnloop_net::timer_arm`, an O(N) rescan per turn is not. A single
   `next_timeout` over a client-wide structure would make the phase deadlines
   affordable for every consumer.
4. **`turnloop_tls::ClientConfig` hardcodes `rustls::crypto::ring`.** Perry's
   other TLS paths install `aws_lc_rs` as the process default (#6117), and both
   providers are in the final link. Naming the provider explicitly is what makes
   this safe, so the behaviour is right — but a host that wants ONE provider in
   the binary cannot express that, and `ClientOptions` has no field for it.
5. **`turnloop_tls::ClientConfig` cannot express a client certificate.**
   `ClientOptions` covers ALPN, roots, `reject_unauthorized` and SNI — enough for
   `fetch`, but not for `node:https`'s `cert`/`key`/`pfx`, which is the next
   consumer. (P5 recorded the server-side twin of this.)
6. **`compression::StreamingDecoder::process` gives no way to distinguish
   "needs more input" from "output buffer full".** Both surface as a step that
   consumed and wrote something, and the caller has to loop until a step does
   neither. That works, and it is what this host does, but a `needs_input` flag
   would let a host size its scratch buffer instead of guessing.
7. **`TcpOpts` still exposes only `nodelay`.** A client cannot set
   `SO_KEEPALIVE` on a socket turnloop owns, so the `tcp_keepalive(60 s)` the
   reqwest client set has no equivalent here. Same missing API as P1's
   `setNoDelay` finding, from the client side.
8. **`turnloop_smtp::Connection::send` takes the message as one `&[u8]`.** A
   large attachment is therefore materialized in full before the first byte
   reaches the socket, and `encode_data` copies it again for dot-stuffing. A
   streaming body (`send_chunk` / `finish_body`, as `http1::Encoder` has) would
   let a host with a 25 MB attachment avoid two copies of it.
9. **`turnloop_smtp` has no `Tls::Required` enforcement at `Ready`.** `Required`
   controls whether STARTTLS is *attempted*; a server that advertises no
   STARTTLS still reaches `Ready` in the clear, and the host must notice. Perry
   does (`on_ready` refuses), but "required" reading as "preferred" is a
   security-shaped surprise.
10. **`LocalExecutor` silently drops completions it did not issue** — P5's finding
   (turnloop#45), unchanged, and the reason this phase is sans-I/O too.

## Perry-side defects this work found (not P6 regressions)

Each was reproduced on the base commit. The first two are fixed here because
the phase's own acceptance case depends on them; the rest are recorded.

1. **`AbortSignal` never reached the global `fetch`** — fixed. See the top of
   this report. Registered twin `js_register_global_fetch_notify_abort`; the
   `#[cfg(feature = "external-fetch-symbols")]` arm is unchanged.
2. **The whole `nodemailer` transporter surface is unreachable from JS** —
   HALF fixed, and still broken. `createTransport` returns a bare handle
   *number*, so `sendMail` / `verify` answer `TypeError: (number).sendMail is
   not a function` on the base commit and on this branch, in every call shape.
   There are two independent holes:
   * **no `HANDLE_METHOD_DISPATCH` arm claimed a nodemailer handle.** Fixed, in
     two places because the two copies of the binding register handles in two
     different registries: an arm in perry-stdlib's `method_dispatch.rs` for the
     bundled surface, and a dispatch **extension** registered by
     `perry-ext-nodemailer` for the well-known-flip surface (the shape
     `perry-ext-http` already uses). Both gate on registry membership first and
     the two-name vocabulary second, so a user object wrapping the transporter
     keeps its own methods.
   * **codegen never gets there.** The receiver is a primitive `number`, so the
     call lowers to `js_throw_type_error_not_a_function` and the runtime is not
     consulted. **Not fixed** — see the SMTP evidence section for what it would
     take. The two dispatch rows above are therefore correct-but-unexercised
     today; they are kept because the hole they fill is real and the other half
     cannot be written without them. An integrator who would rather not carry an
     unexercised arm can drop both commits' dispatch hunks without touching the
     engine.
3. **`perry-ext-fetch` has no `AbortSignal` wiring at all.** `signal` is stored
   as a `Request` field and never consulted: no `Notify`, no `select!`, no
   `AbortError`. So `import 'node-fetch'` and the global `fetch` differ in abort
   behaviour — and after fix (1) they differ *more*, because the global one now
   works. Not touched here; it is the same crate P6 did not migrate.
4. **Both `axios` copies build a fresh `reqwest::Client` per request**
   (`perry-ext-axios/src/lib.rs`, `perry-stdlib/src/axios.rs`): ~250 KB of
   state, a cold DNS and TLS path, and no pooling, on every call — exactly the
   failure mode the `fetch`/`node:http` singletons exist to avoid.
5. **`http2.connect()` is cleartext-only and spins its own tokio runtime per
   session.** `connect_h2_stream` returns a bare `tokio::net::TcpStream` with no
   `tokio-rustls` wrap, so no ALPN; the session is hardcoded `h2c` /
   `encrypted: false`, and `http2.connect('https://…')` does not do TLS.
6. **`perry-ext-http`'s `AGENT_CLIENTS` cache never evicts** (its own comment
   says so): one `reqwest::Client` per `http.Agent`, held for the process.
7. **WHATWG's blocked-port list is not implemented.** `fetch('http://host:1/')`
   rejects with `cause.message === 'bad port'` on Node before a socket is
   created; Perry connects. Both transports; found while writing the fixture,
   which is why it uses a port the OS just released instead of a literal.

## For the integrator

- The branch is `turnloop/p6-clients` on `origin`. Nothing here bumps the
  version.
- `turnloop-smtp 0.1.0-alpha.3` is new in `Cargo.lock` — one crate, resolved
  with `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow` and then pinned
  `--precise` to alpha.3 so it stays in lockstep with turnloop/-http/-tls.
  alpha.4 exists and resolves cleanly, but was two hours old.
- **Build the ext wrappers in the same cargo invocation as
  `perry-stdlib-static`** (#7629). `perry-ext-nodemailer` is now in that set for
  any tree that wants to exercise SMTP.
- **`crates/perry/src/commands/compile/optimized_libs/driver.rs` re-asserts
  `turnloop-smtp-client`** for an `import 'nodemailer'` program, the same way it
  re-asserts `web-fetch` for `undici`. Without it the auto-optimize rebuild of
  perry-stdlib drops the engine and the wrapper's externs dangle at link time —
  which is how it first failed here.
- Run, on a machine with the pinned oracle:

```bash
RUST_TEST_THREADS=1 cargo test --release -p perry-stdlib --lib turnloop_client
RUST_TEST_THREADS=1 cargo test --release -p perry-stdlib --lib turnloop_smtp
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh
PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_turnloop_fetch
PERRY_LOOP_STATS=1 ./p6_tls_remote      # needs the network
PERRY_LOOP_STATS=1 ./p6_abort_midbody
```

- The two trees are on the build box at `/root/claude-turnloop-p6/{base,perry}`
  (base at `7f77cce3c6`), each with its own `target/`. Delete both when the A/B
  is done. `PERRY_RUNTIME_DIR` must be overridden per tree —
  `/etc/profile.d/perry.sh` points it at a different checkout.

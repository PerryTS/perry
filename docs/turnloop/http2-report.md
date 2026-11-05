# turnloop HTTP/2 — `node:http2` off `h2` and hyper

Branch `turnloop/http2`, based on `turnloop/integration` at **`ce480bb208`**.

> **Note on the base.** The brief named `7f77cce3c6` as the integration head.
> That commit is an *ancestor* of the current head by **140 commits** — P6, P7,
> P8, P9 and P11 have landed since the brief was written. This branch is based
> on the current head, not on the brief's SHA.

Read on the shared Linux box (`perrybuilder`, EPYC 9354P). **Nothing was run on
Windows or macOS, and nothing was benchmarked.**

---

## ⚠️ Status: this lane is NOT complete, and this branch is NOT a migration

Read this section before anything else.

**What is finished and verifiable:** the design, and the protocol-contract work
that has to precede any binding — including three findings about
`turnloop_http::http2` that are proven with a committed, runnable probe
(`docs/turnloop/http2-contract-probe.rs`), one of which is a **silent
connection-killing defect** that any host implementing the brief's own core
requirement ("a stream errors while its siblings are live") will hit.

**What is not finished:** the binding. `crates/perry-ext-http/src/server/turnloop_h2/`
contains ~1,250 lines of the new transport (connection state machine, stream
multiplexing, flow-control policy, response path) and
`http2_server/turnloop_glue.rs` contains its seam to the JS handles. They are
**committed but deliberately NOT wired into the module tree** — there is no
`mod turnloop_h2;` — so the crate builds exactly as it did before and this
branch changes no behaviour whatsoever.

**Therefore: `http2.createServer`, `http2.createSecureServer` and
`http2.connect` are all still on hyper and `h2` on this branch, the tokio
inventory is unchanged, and no gap sweep was run** (there would be nothing to
compare). Merging this branch ships documentation and unreferenced source; it
does not ship a migration. If that is not wanted, take the report and the probe
and drop `crates/`.

**What remains** is itemised under "What is left to do", with the reason the
remaining work is larger than it looks.

---

## What this found before it changed anything

Perry's HTTP/2 was measured before it was touched. Six findings, every one
reproduced on the base commit:

| subject | Node 26.5.1 | base `ce480bb208` |
|---|---|---|
| `http2.connect('https://host')` | TLS to port 443, ALPN `h2` | **cleartext TCP to port 80** |
| `session.request()` ×N concurrently | N multiplexed streams | **a race for one `SendRequest`**; the loser gets `"HTTP/2 session is not connected"` |
| one `session.request()` | one stream on the session | **a new OS thread and a new `current_thread` tokio runtime, per request** |
| `stream.id` | the RFC 9113 stream id | **a process-global odd counter** that corresponds to nothing on the wire |
| `session.settings(...)` / `.goaway(...)` / `.ping(...)` | frames on the wire | **never reach the wire** — they find the peer session *in the same process* by handle scan and push a synthetic event |
| `stream.sendTrailers()` / `.priority()` / `.setTimeout()` | real | **`=> self_ref`**, a no-op |

The third is the one the brief names (perry#10327), and it is worse than "a
private runtime per session": `js_node_http2_connect` builds one
(`session.rs:290`) and `start_client_request` builds **another per request**
(`session.rs:423`).

The fifth is the one that matters most for what a migration costs. Perry's
HTTP/2 control surface is not a thin binding over a protocol — it is a
**loopback simulation**. `queue_session_settings` and `queue_session_goaway`
enumerate `Http2SessionHandle`s with `iter_handle_ids_of`, pick the ones whose
`session_type` is the opposite of the caller's, and push an event into their
queues. No frame is encoded. That is why the `test-parity/node-suite/http2/`
corpus passes: every case in it is a Perry client talking to a Perry server in
one process. Moving to turnloop means those surfaces meet a real wire for the
first time, and the node-suite corpus stops being evidence that they work.

---

## The three design decisions the brief asked for

These are settled, and they are what the committed code implements.

### 1. Flow control — what `release_capacity` maps to

`turnloop_http::http2` **never reopens a receive window on its own**. A
`Stream`'s `unreleased` counter accumulates every DATA byte and only
`release_capacity` turns it back into WINDOW_UPDATE frames. So the host is the
policy. Perry's policy is three rules:

1. **Release on consume.** A DATA payload is copied into the stream's own
   buffer inside the completion sink. By the time the event returns there is no
   downstream consumer left to wait for — Perry's HTTP server buffers a request
   body before dispatching it, on both transports, and has since Phase 1 — so
   withholding window would only idle the peer for nothing. The receive window
   stays fully open for the common case.

2. **Bounded by `maxSessionMemory`.** Eager release with no bound is an
   unbounded upload buffer. Node bounds the same quantity with
   `maxSessionMemory` (default 10 MB). Once a connection holds that much
   *undispatched* body, the release is withheld per stream and the peer stalls,
   which is exactly what a flow-control window is for. The withheld amounts go
   back the moment a request is handed to the pump and its bytes leave the
   module. Node signals the condition by destroying the session with
   `ENHANCE_YOUR_CALM`; stalling first is strictly gentler, and a peer that
   keeps pushing past a shut window is then a flow-control error on its own
   terms.

3. **A terminated stream still releases — this is not optional.** See
   "turnloop gaps", finding 1. Getting this wrong kills the connection.

The write side is the mirror image and needs no policy: `send_data` returns
what it accepted and **zero** on a stall, the remainder stays in the stream's
`outbox`, and `Event::WindowUpdate` retries it. `res.write()` returns `false`
when the outbox plus the transport's `queued_bytes` exceed the 16 KiB
high-water mark — which is Node's `write()` / `'drain'` contract, and the same
quantity P5 reads for HTTP/1.1.

One thing the eager policy costs, recorded because it is a real difference: a
stream cannot exert *per-stream* backpressure, because Perry has no per-stream
consumer to be slow. `stream.pause()` on an `Http2Stream` therefore does not
close that stream's window. Node's does.

### 2. Stream multiplexing — ids, handles, and sibling isolation

An HTTP/2 stream id is a per-connection `u32`; a Perry handle is a process-wide
`i64`. The mapping is one `H2Stream` record per open stream, held in its
connection's `streams: Vec<H2Stream>`, carrying the **real** stream id. `stream.id`
in JS becomes RFC 9113's number instead of the global odd counter.

A record is created when a stream's first HEADERS is seen (server) or when
`session.request()` calls `Connection::open` (client), and dropped at a terminal
state. **A stream's lifetime is strictly inside its connection's**, and the
isolation rule is:

* **A stream error ends one stream.** `Event::Reset` finds one record, queues
  one event on one handle, releases that stream's capacity, drops that record.
  Nothing else is touched. Likewise a `send_headers` that the core rejects (a
  handler that emitted a malformed header block) is answered with
  `core.reset(id, INTERNAL_ERROR)` — a **stream** error — rather than being
  allowed to become a connection error.
* **A connection error ends all of them, exactly once each.** Only
  `Connection::receive` returning `Err` does this. The GOAWAY it already queued
  is flushed *first* — a `receive` error has side effects, and returning before
  flushing sends the peer nothing at all — then `eof()` + `poll_failed_stream()`
  produces one terminal per still-open stream, and the connection closes.

The one place sibling isolation is genuinely hard is the *shared* state: the
connection-level flow-control window and the core's stream table. Both are
returned by the same call, and both leak if a stream is retired without it.
That is gap 1.

### 3. GOAWAY and graceful close

| Node | Perry on turnloop | note |
|---|---|---|
| `session.close([cb])` | `Connection::shutdown()` → GOAWAY(NO_ERROR, last_remote), `draining`, close when `is_drained()` | matches |
| `session.destroy()` | no GOAWAY, `tl::close` | matches |
| `session.goaway(code, lastStreamID, opaqueData)` | the frame is **hand-encoded** with `http2::encode_frame` and written directly | `Connection` has no API for it; see gap 2 |
| `server.close([cb])` | stop accepting, live sessions finish | matches |
| a peer opens a stream after our graceful GOAWAY | **diverges — a connection error** | see gap 3; this is a race that happens in normal operation |

The last row is a genuine, measured divergence and it is not cosmetic: a
graceful close *always* has an in-flight window in which the peer, not yet
having seen the GOAWAY, opens a stream. Node answers `REFUSED_STREAM` and keeps
the session; `turnloop_http::http2` answers PROTOCOL_ERROR and kills it.

---

## turnloop gaps found

Reported here in the shape P5's and P6's were; the coordinator files them.
**All three are proven by `docs/turnloop/http2-contract-probe.rs`**, a
self-contained program that depends only on `turnloop-http` (build and run
instructions at the bottom of this section).

### 1. A stream reset with unreleased DATA burns its table slot permanently, and the connection then dies

`Connection::add_stream` reuses a closed stream's slot only when its
`unreleased` is zero. `reset()` sets `local_end`/`remote_end` but **does not
zero `unreleased`**, and neither does `poll_failed_stream`. So a stream reset
with DATA in flight — which is exactly what a stream error looks like — holds
its slot for the life of the connection.

The failure is not "fewer concurrent streams". When the table fills,
`add_stream` returns `REFUSED_STREAM` **from inside `receive`**, which sets
`self.failed = true` and emits a GOAWAY: the whole session dies, and the peer
is told PROTOCOL_ERROR (code 1), because `receive`'s error map has no case for
`REFUSED_STREAM`.

Measured, with a server whose table holds 2 streams, 6 streams attempted:

```
== server resets WITHOUT releasing capacity ==
  server refused stream #2: CONNECTION ERROR REFUSED_STREAM
   streams the server accepted: 2
== server releases capacity, THEN resets ==
   streams the server accepted: 6

verdict: without release = 2, with release = 6 (table size 2, 6 attempted)
```

Nothing in the API says so. `release_capacity`'s own contract reads as "return
window for data you have consumed", and a stream you just reset is precisely
the data you did *not* consume. A host that reasons that way writes a server
that works perfectly until the first N stream errors and then drops every
connection. Either `reset` should zero `unreleased` and return the window
itself, or the doc comment on `reset`/`add_stream` should say that the host
must. (Perry's implementation releases in `terminate()` before dropping the
record — rule 3 of the flow-control policy above.)

### 2. `shutdown()` cannot express `session.goaway(code, lastStreamID, opaqueData)`

`Connection::shutdown` always sends GOAWAY with code 0 and its own
`last_remote`, and there is no opaque-data parameter. Node's
`session.goaway(code, lastStreamID, opaqueData)` sets all three, and
`'goaway'` listeners receive the opaque data. Worked around by encoding the
frame with the crate's public `encode_frame` and writing it alongside the
core's own output — which works, but means the core's `draining` bookkeeping
and the host's GOAWAY can disagree. A `Connection::goaway(code, last, opaque)`
would belong in the crate.

### 3. A stream opened after a graceful GOAWAY is a connection error, not a stream error

After `shutdown()`, `receive`'s HEADERS arm rejects a new stream with
`protocol("invalid new stream")` because `self.draining` is set — and a
`protocol` error is a **connection** error, so the session dies and a second
GOAWAY goes out. Measured:

```
client2 opened late stream 1
server2 after late stream: CONNECTION ERROR PROTOCOL_ERROR
server2 emitted frame kind=7 len=17 (7 = GOAWAY)
```

RFC 9113 §6.8 says a peer that receives a GOAWAY "MUST NOT open additional
streams", but also that the sender of the GOAWAY should treat streams above
`last_stream` as refused — because the race is unavoidable: the peer cannot
have seen the GOAWAY yet. Node answers `RST_STREAM(REFUSED_STREAM)` and keeps
the connection. There is no way for a host to get Node's behaviour, because the
decision is inside `receive_inner`.

### 4. `Step`'s two independent zero cases are undocumented (the HTTP/2 analogue of #50)

`receive` can return `consumed == 0, event == None` (a partial preface or a
partial frame: **stop**) and `consumed > 0, event == None` (the preface itself,
a SETTINGS **ack**, PRIORITY, an unknown frame type: **keep going**). Both are
normal. A host that loops on "there is still input" spins forever on the first;
a host that loops on "an event came back" stalls on the second — and it stalls
at the *preface*, before a single frame is read, so the connection never starts
at all. Measured:

```
partial preface: consumed=0 event=false
step: consumed=24 event=None      <- the preface
step: consumed=9  event=Some(Settings)
step: consumed=9  event=None      <- the SETTINGS ack
step: consumed=0  event=None      <- exhausted
```

The correct condition is `consumed > 0 || event.is_some()`, which
`asynchronous::mod.rs`'s own driver uses and nothing else states. This is the
same class as PerryTS/turnloop#50 (`http1::Decoder`'s zero-consume
`Event::End`) and P5's `Event::Upgrade` asymmetry: three lanes, three different
shapes, one missing sentence in the `Step` docs.

### 5. `Event::Headers` does not distinguish a head, a trailer block and a 1xx

All three arrive as `Event::Headers`, and the host must track `received_head`
itself to tell them apart — even though `Connection` already knows, having just
enforced the distinction (`finish_headers` checks `s.received_head` and rejects
trailers without END_STREAM). A `kind: HeadersKind` field, or separate
`Event::Trailers` / `Event::Informational` variants, would remove a piece of
state every host has to duplicate and can get wrong.

### 6. No getter for a stream's `unreleased`

Finding 1's fix requires the host to mirror the core's own counter, byte for
byte, because `release_capacity(id, n)` errors when `n > unreleased` and there
is no way to ask. The mirror is exact only because padding is auto-released
inside `receive` (so the host's view increments by the *unpadded* `bytes.len()`),
which is itself undocumented and true by arithmetic rather than by contract.

### Reproducing

```bash
mkdir -p probe/src && cd probe
cat > Cargo.toml <<'EOF'
[package]
name = "h2probe"
version = "0.0.0"
edition = "2021"
[dependencies]
turnloop-http = "=0.1.0-alpha.5"
[workspace]
EOF
cp ../docs/turnloop/http2-contract-probe.rs src/main.rs
cargo run --release
```

---

## Perry-side defects found (not this lane's regressions)

### 1. Two bugs in P5's listen path that mask each other — found, **not fixed here**

`turnloop_serve::listen` passed `no_delay` into `tcp_listen`'s **sixth**
parameter, which is `reuse_port: bool`. The value is not dropped on the floor —
it is traced end to end: `perry-ffi`'s `tcp_listen` → `abi.rs:215` →
`turnloop_net::tcp_listen` → `ListenOpts { reuse_port, .. }` → turnloop's
`SO_REUSEPORT`. And `server.noDelay` defaults to **true**, so every turnloop
HTTP/1.1 and HTTPS server has been binding with `SO_REUSEPORT` since P5.

Measured, two `http.createServer().listen(47311)` calls in one process:

```
--- node 26.5.1 ---
A listening
B error: EADDRINUSE
--- perry, base ce480bb208 (P5 turnloop path) ---
A listening
B listening TOO (both bound the same port)
[perry-loop] driver=turnloop turns=2 ... native_ticks=0 completions=5
[perry-loop-waits] arm=turnloop turnloop_waits=2 tokio_ticks=0
```

The liveness counters are there on purpose: `driver=turnloop` with
`tokio_ticks=0` is what says the turnloop path — not hyper — produced that
answer.

The one-line fix is `false` in place of `no_delay` (with the parameter renamed
`_no_delay`: turnloop's `ListenOpts` has no `TCP_NODELAY` field, and Perry sets
per-socket options from JS afterwards, which is the runtime's own documented
reasoning). **Nothing on the turnloop path wanted `SO_REUSEPORT`** — the cluster
worker that genuinely needs it declines the turnloop path in
`try_listen_on_turnloop` and binds a `std::net::TcpListener`, which is one of the
two reasons that decline exists.

**That fix was written, built and verified, and then reverted.** It must not
land alone, because of the second bug it uncovers.

#### The second bug: a failed bind never reaches JS

`try_listen_on_turnloop`'s error arm `eprintln!`s and returns `Some(0)`. No
`'error'` event is emitted, and there is no deferred-error machinery to emit one
with — `server/deferred_events.rs` has `queue_deferred_listening_emit` and
`queue_deferred_close_emit` and nothing else.

This is **pre-existing and independent of the first bug**, which is worth
establishing rather than assuming, because `SO_REUSEPORT` only helps when *both*
sockets set it. Measured on the **base commit**, with the port held by a plain
Python listener:

```
--- node 26.5.1 ---
error event: EADDRINUSE
--- perry BASE ce480bb208 (no fix), port held by python ---
[node:http] bind 0.0.0.0:47399 failed: listen EADDRINUSE
NO error event fired
```

So the two interact: with the `SO_REUSEPORT` fix applied and verified, a
duplicate `listen()` correctly fails —

```
--- perry WITH the fix ---
A listening
[node:http] bind 0.0.0.0:47311 failed: listen EADDRINUSE
```

— but the program then **hangs**, because the `'error'` listener Node would have
called never fires. Going from "silently returns the wrong answer" to "hangs" is
not an improvement, so shipping the one-line fix by itself would be a
regression.

**The prescription**, for whoever takes it: land both together — `reuse_port:
false`, plus a deferred `'error'` emit carrying Node's shape (`code`, `errno`,
`syscall`, `address`, `port`) wired into the same pump that drains
`'listening'`/`'close'`, ideally on the hyper path too — and run a full gap
sweep, because this is P5's listen path and every `node:http` / `node:https` gap
test goes through it. A gap test was written and is **not** included here
because it would be a new failing test against the current tree; its assertions
are `first: listening` / `second: error EADDRINUSE` / `other: listening
port-matches=true` / `done`, which is Node 26.5.1's exact output.

### 2. The `http2` surface itself

**The five `http2` items in the table at the top** — cleartext `https://`,
the unmultiplexed client, the per-request runtime, the fake `stream.id`, and
the loopback-only control frames — are each worth their own issue.

---

## What is left to do, and why it is bigger than it looks

The transport is written. The *binding* is not, and the binding is the larger
half, because Perry's HTTP/2 surface is a simulation rather than a thin layer
(see "What this found"). Concretely:

| remaining | why it is not mechanical |
|---|---|
| `turnloop_h2/client.rs` — `http2.connect` | needs a **public TLS client installer** on turnloop sockets. `perry_ext_net::turnloop_tls_io` has `install_server_session` (public) but only `begin_client_upgrade` (`pub(crate)`, takes perry-ext-net's own `TlsClientConfigData`, and settles a `JsNativeAsyncCompletion`). A `pub fn install_client_session(id, servername, verify, alpn)` has to be added to perry-ext-net first. |
| `turnloop_serve::adopt_alpn_http1` | the ALPN `http/1.1` fallback moves one table entry between two modules **sharing subsystem 1** — which is why this design shares the slot rather than taking one of the 8. P5's `Conn` has no constructor that takes an already-TLS-installed id with buffered leftover input. |
| `Http2SessionHandle` / `Http2StreamHandle` gain `turnloop_conn: i64` | every construction site must be updated, including `pump.rs`'s test fixture; and `Http2StreamHandle::response_tx` (a `oneshot::Sender`) must become an enum over the two transports, or the tokio edge cannot go. |
| `dispatch.rs` routing | ~20 session/stream methods each need "if this handle is on turnloop, reach the wire; else the legacy path" — and for six of them (`settings`, `goaway`, `ping`, `setLocalWindowSize`, `priority`, `sendTrailers`) **there is no legacy path that reaches a wire at all**, so there is no reference behaviour to preserve and each needs its own Node measurement. |
| `response.rs` / `response_turnloop.rs` routing | 8 call sites; small. |
| `server.rs` pump splice + `note_turnloop_request_aborted` | small. |
| the `h2` / hyper-`http2` feature removal | see the inventory note below. |
| **validation** | h2spec against Perry's own server (the harness is ready — see below), fixtures against Node 26.5.1, GC stress, and the two full gap sweeps. |

### On the acceptance bar "group D gone"

Worth flagging before someone tries to satisfy it literally. Group D is **two**
edges: `perry-ext-http -> h2` and `perry-ext-http -> tokio`. The first is
HTTP/2's and this work removes it. **The second is not HTTP/2's** — its own
inventory entry says `blocker: "the union of the rows above"`, i.e. every
remaining tokio use in perry-ext-http: the hyper HTTP/1.1 fallback for worker
agents and cluster workers, the attached-`WebSocketServer` path, reqwest, and
the `oneshot`/`mpsc` types woven through `HttpServer` and `HyperResponseShape`.
Removing it means finishing P5's declining rows, not finishing HTTP/2.

So the honest target is **group D 2 edges → 1**, total 39 → 38, and `h2` gone
as a *direct* edge. `h2` will remain in `Cargo.lock` regardless, pulled by
reqwest's default `http2` feature.

### h2spec is ready to run

The checksum-pinned h2spec that turnloop's own CI uses is built on the box at
`/root/claude-turnloop-http2/tools/bin/h2spec` (commit
`70ac2294010887f48b18e2d64f5cccd48421fad1`, sha256 verified against
`scripts/ci/tools.json`, Go 1.25.1, `--strict`, 147 tests). Pointing it at a
Perry `http2.createServer()` is a one-line change to the driver, and it is the
single highest-value check for the binding — turnloop's CI proves the protocol
core, and the three gaps above are precisely the class of thing that lives in
the *binding* and that h2spec would catch.

---

## What was not done

Named precisely rather than left implied:

* **No migration landed.** The transport is unwired; `createServer`,
  `createSecureServer` and `connect` are unchanged.
* **No gap sweep**, either arm — this branch changes no behaviour at all, so a
  sweep would compare two identical binaries. The `SO_REUSEPORT` fix was built
  and verified and then **reverted** (see defects, §1); landing it needs the
  `'error'` emit beside it and a full sweep, because it touches P5's listen
  path and every `node:http` / `node:https` gap test goes through that.
* **No `PERRY_LOOP_STATS` / thread-count measurement**, for the same reason: the
  subject never ran, and a counter measured on an unchanged path is not
  evidence. (The baseline arm *is* built, at
  `/root/claude-turnloop-http2/base`, commit `ce480bb208`, with `npm ci` done —
  so whoever continues starts with the baseline already in hand.)
* **No GC stress.** Nothing new holds a JS value across a completion yet.
* **No h2spec run against Perry.** The binary is built; there is no Perry HTTP/2
  server on turnloop to point it at.
* **Nothing on Windows or macOS**, and **nothing benchmarked** — the box is
  shared and was under load 9–13 throughout.
* **Server push** was not added, per the brief. Perry does not implement
  `createPushResponse` today and `turnloop_http::http2` rejects PUSH_PROMISE
  outright (`protocol("server push disabled")`), so both agree.

## For whoever continues

* Read `crates/perry-ext-http/src/server/turnloop_h2/conn.rs`'s module header
  first — the three sharp edges of the receive loop are written down there, and
  two of them are invisible in the type signatures.
* The baseline tree and the h2spec binary are on the box under
  `/root/claude-turnloop-http2/` (`OWNER` file names this lane); the contract
  probe is at `/root/claude-turnloop-http2/probe`.
* Start with gap 1. Any binding that does not implement rule 3 of the
  flow-control policy will pass every test you write until the first stream
  error, and will then fail in a way that looks like a peer problem.

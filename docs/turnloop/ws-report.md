# turnloop WS — WebSockets, and the end of three tungstenite majors

Branch `turnloop/websockets`, based on `turnloop/integration` at `96326a45c4`.
Built and tested on the shared Linux box (EPYC 9354P) against the pinned gap
oracle Node **26.5.1**. Nothing here was run on macOS or Windows, and nothing
was benchmarked.

## The question this lane was given

> Can the WebSocket handshake be driven sans-I/O over a connection Perry keeps
> owning, or does something genuinely need the owned stream?

**It can, and nothing needs the owned stream.** That is not a judgement call —
it is the signature of the function that does it:

```rust
pub fn accept(request: &Head, protocols: &[&str]) -> Result<(Head, Option<String>), Error>
```

`turnloop_websocket::accept` takes a *decoded request head* and returns the
`101` head to write. There is no stream in the type, because RFC 6455's opening
handshake is an HTTP/1.1 request and a `101` and nothing else: a
`Sec-WebSocket-Key` goes in, `SHA-1(key + GUID)` base64'd comes out. The framing
that follows is the same shape — `Connection::receive(&[u8], &mut Vec<u8>)`.

What genuinely needed an owned stream was
`tokio_tungstenite::WebSocketStream<S>`, whose `S: AsyncRead + AsyncWrite +
Unpin + Send + 'static` bound is an API decision of that crate. P5's inventory
entry — *"the handshake needs an owned stream a turnloop connection cannot
produce"* — was right about the consequence and attributed it one layer too
low. **This is not a turnloop gap and there is nothing to file for it.**

The proof is a unit test that constructs a `101` with no transport anywhere in
scope, against RFC 6455 §1.3's worked example
(`turnloop_link::tests::a_handshake_needs_no_stream`).

## What that makes possible, and why nothing moves

P5 solved the same shape for TLS by putting the session *above* the socket, so
no descriptor had to move. A WebSocket is one layer further up and needs even
less. Compare the two upgrade paths `perry-ext-http` now has:

| | `server.on('upgrade')` (P5) | attached `WebSocketServer` (this lane) |
|---|---|---|
| who owns the connection afterwards | `perry-ext-net` | still `perry-ext-http` |
| mechanism | `turnloop_net::transfer` — the subsystem tag changes | nothing; a codec is installed beside the connection |
| why | a `net.Socket` is handed to JS and outlives the HTTP connection | a WebSocket has no such JS object; only the decoder changes |

So the id, the outstanding multishot read, the write queue and the TLS layer are
all untouched. `Conn` gains one `bool`; `feed` routes to
`perry_ext_ws::turnloop_link` instead of the HTTP decoder; `write_raw` — already
TLS-transparent — carries the `101` and every frame. An attached
`WebSocketServer` on an **HTTPS** server therefore works with no extra code at
all, which is the part that would have been expensive with a descriptor handoff.

`perry-ext-http` already depends on `perry-ext-ws`, so the callback direction is
fixed: the host installs a `turnloop_link::Transport` of three function
pointers (`write`, `finish`, `destroy`) at sink-registration time, the same
one-way seam `register_http_address_reader` already uses.

### `finish` is not `destroy`

The transport has two shutdown verbs on purpose. A closing handshake ends with a
close frame written and *then* a shutdown, and `turnloop`'s `close` cancels the
connection's outstanding operations — including the write just queued. P5 hit
this edge from the other side (its `allowHalfOpen` close cancelled the writes an
`'end'` handler had queued, and the fix was to separate "should we shut down"
from "may the socket go away yet"). Collapsing the two here would have sent every
peer 1006 instead of the code it asked for, intermittently.

## One codec instead of three tungstenite majors

The tree carried tungstenite 0.24 (`perry-ui-android`, sync, own thread), 0.29
(`perry-ext-ws`, `perry-ext-http`, `perry-ext-fastify`, `perry-stdlib`) and 0.30
(`turnloop-websocket`, unused). Because the sans-I/O core is a state machine
over byte slices, it serves a tokio stream exactly as well as a turnloop handle
— so it replaced 0.29 in all four crates rather than only in the migrated one.
**tungstenite 0.29 is gone from `Cargo.lock`.**

`perry-ext-ws` keeps `tokio`: a thread acting for an agent another thread already
owns has no loop, and P1's coexistence rule says a reachable configuration is not
deleted. It gained no TLS dependency for the outbound `wss://` client either —
`perry_ext_net::connect_tls_client` hands back a boxed
`AsyncRead + AsyncWrite`, so the TLS stack is named in exactly one crate.

## The `Received` contract (PerryTS/turnloop#86)

`Connection::receive` returns `Received { consumed, message }`, and the reading
is **not** "an event came back, so keep going":

| `consumed` | `message` | meaning |
|---|---|---|
| `0` | `None` | **wait** — no progress is possible until more bytes arrive |
| `> 0` | `None` | **keep going** — a partial frame, or a control frame answered internally |
| `0` | `Some` | **keep going** — a whole frame was already buffered from an earlier call |
| `> 0` | `Some` | **keep going** — tungstenite reads one chunk per pass |

Only the first row terminates the loop. `codec::Codec::receive` is the single
place in Perry that implements it, and `receive_loop_handles_both_zero_cases`
pins all four rows — including the third, which is the one a "stop when
`consumed == 0`" loop silently drops a decoded message on.

`flush` matters too: `receive` *queues* the pong for a ping but only *encodes* it
on a flush, so a host that flushed only around application writes answers a ping
whenever it happens to send something next. `Codec::receive` flushes before it
returns.

## What did NOT move

Named precisely, because each is a hole rather than a preference:

* **The outbound `ws` client's transport.** `new WebSocket(url)` still connects
  on a tokio stream. Its codec and handshake are the shared ones, so this is a
  transport migration that remains, not a protocol one — it needs turnloop DNS
  plus a `turnloop_tls_io` client install, both of which exist.
* **`new WebSocketServer({ port })`.** Same: its accept loop is still
  `tokio::net::TcpListener`, driving the shared codec. The *attached* shape is
  the one group A named, and it is the one that moved.
* **`perry-ext-ws` → `tokio`.** The declining-transport edge, kept by the P1
  coexistence rule. It is now the only tokio edge that crate has.
* **`perry-ui-android` → tungstenite 0.24.** Sync tungstenite on its own thread,
  Android-target only. It is a candidate for the same treatment — the sans-I/O
  core works over a blocking `std::net::TcpStream` too — but it cannot be built
  or run from this box, and a migration nobody can execute is not one this lane
  should land. It keeps the third major alive.
* **`perMessageDeflate`, `maxPayload`, `verifyClient`, and `WebSocketServer`'s
  `path` option.** `path` is the notable one: it is read nowhere, so
  `new WebSocketServer({ port, path: '/ws' })` accepts on **every** path,
  silently. Pre-existing; unchanged here.
* **`'error'` carries a string, not an `Error`.** Pre-existing; `err.message` is
  `undefined`. Unchanged here.

## Test evidence

Every command as run, on the shared Linux box, against Node **26.5.1**.

### The four fixtures, byte-for-byte against Node

Four new gap tests, each compared with `diff` against
`node --experimental-strip-types` on the same file. Where a `ws` API would hide
the thing under test, the fixture drives the wire directly with a raw
`net.Socket` and hand-built frames — `ws` will not emit a fragmented message on
demand, and a `.toString()`-shaped assertion cannot see a corrupted binary
payload, so binary payloads are printed as **hex**.

| test | drives | branch vs Node | base vs Node |
|---|---|---|---|
| `test_gap_turnloop_ws_client.ts` | a `ws` **client** against a hand-rolled `net.Server` | **byte-identical** | diverged (6 rows) |
| `test_gap_turnloop_ws_server.ts` | a `WebSocketServer({ port })` against a raw masked-frame client | **byte-identical** | diverged (8 rows) |
| `test_gap_turnloop_ws_attached.ts` | `http.createServer()` + `new WebSocketServer({ server })` | **byte-identical** | **HANG** (3/3, exit 124) |
| `test_gap_turnloop_ws_frames.ts` | fragmentation + interleaved control frames | **byte-identical** | diverged (7 rows) |

Between them: `'open'`; text and binary messages in both directions with
non-UTF-8 bytes; the `isBinary` argument; `ping`/`pong` events and the automatic
pong; a ping interleaved *between* two fragments of a message (legal per RFC
6455 §5.4); a three-fragment text message and a two-fragment binary one; an
empty text message; `close(4001, …)` and `close(1000, 'bye')` asserted on the
wire *and* in the `'close'` handler; a close with no status code reported as
1005; `wss.clients.size` before and after; the `101` head; and an ordinary HTTP
GET served on the same server before the upgrade.

The base arm's divergences are what the lane fixed, and each is isolated by one
row. The sharpest: `009f9296ff` (5 bytes) arrives as
`00efbfbdefbfbdefbfbdefbfbd` (13 bytes) — every non-ASCII byte replaced by
U+FFFD, in **both** directions. A test that compared `data.toString()` would
have passed on that.

The base arm's attached fixture **hangs**, three runs out of three: neither
`wss.close()`'s nor `server.close()`'s callback ever fires and the HTTP server
holds the loop open. Its pre-upgrade HTTP GET is byte-identical to Node, so the
attached shape's HTTP half was already clean; it is the WebSocket half that was
not.

### Which transport carried it — `PERRY_LOOP_STATS`, both arms

`ws_decline.ts` isolates the group-A decline: an `http` server with a
`WebSocketServer` attached at listen time, serving one ordinary request and
closing. No WebSocket traffic, so it terminates on both arms and the only
question it asks is which transport carried the HTTP half.

| | base `96326a45c4` | branch |
|---|---|---|
| `driver` | turnloop | turnloop |
| `turns` | **(none — the counter block is absent)** | **6** |
| `completions` | — | **11** |
| `tokio_ticks` | 0 | 3 |
| status + body | `status 200 body attached-ok` | identical |

The base prints `[perry-loop] driver=turnloop parked=0` and no turn counters at
all: the loop made **zero** turns, because `try_listen_on_turnloop` declined for
the attached `WebSocketServer` and the whole server ran on hyper. That is P0's
original signature, still reachable on the base commit for exactly this shape.
The same program now turns the loop six times and dispatches eleven completions.

`tokio_ticks=3` on the branch is **not** the server: it is `http.get`, Perry's
outbound HTTP client, which is group B and untouched here. Naming it rather than
quoting a zero is the point — the migrated path is the server, and a counter
that includes an unmigrated client would be a misleading zero either way.

On the full `test_gap_turnloop_ws_attached` fixture the branch reports
`driver=turnloop turns=22 os_waits=2 native_ticks=7 completions=44
timer_arms=7`, with `tokio_ticks=7` from the `ws` **client** in the same
process — the client transport this lane did not move. The base cannot be
compared on that file at all, because it hangs.

**Thread count: 1 on both arms**, sampled from `/proc/<pid>/status` while the
exchange ran. The attached path adds no thread, which is the expected shape: it
adds no task and no channel either.

### GC stress, with a WebSocket in flight

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1 \
PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 PERRY_LOOP_STATS=1 \
./test_gap_turnloop_ws_attached
```

Clean on all three seeds — stdout **byte-identical** to the unstressed run — and
the instruments were *armed* rather than merely quiet:

- **51 `[gc-fromspace-protect] retired_set=#N` lines**: copying minors really
  ran and their from-space really was quarantined and `mprotect`ed. A run with
  zero copying minors protects nothing and would have passed vacuously;
- `safepoints=51`, `forced_collections=51` — every handled safepoint collected,
  which is `RATE=1`'s documented behaviour and why all three seeds report
  identical counts;
- 2,321 `[gc…]` diagnostic lines;
- `completions=44` on the same run, so those collections landed while socket
  operations were in flight;
- no SIGSEGV from the quarantine reporter: no stale from-space pointer was
  dereferenced.

This matters more than usual for one specific reason. The turnloop transport
runs the codec **inside the host's completion dispatch**, not on a task, so a
collection can land between a frame arriving and its event reaching JS. What
crosses that boundary is deliberately not a JS value: a `Link` holds an id, a
codec and owned `Vec<u8>`s, which is why this module registers no root scanner
at all — the same rule P5's `turnloop_serve` follows, and the reason
`WS_PENDING_EVENTS` can stay free of JS values. `wss.close(cb)`'s callback is
the one closure the change adds, and it is parked in
`WsServerHandle::listeners`, which `scan_ws_roots` already walks and a moving
collection already rewrites — not in the pending queue, which nothing scans.

## turnloop gaps found

Reported here in the shape P5's were; the coordinator files them.

1. **`Received`'s two zero cases are undocumented, and both readings a host
   reaches for are wrong** (already PerryTS/turnloop#86). `Received { consumed,
   message }` is returned with no doc comment on either field, and the two
   natural loops — "stop when no message came back" and "stop when nothing was
   consumed" — are each wrong on a different row of the table above. The first
   stalls on a partial frame; the second drops a message tungstenite had already
   buffered. One sentence on the struct would close it: *"Only `consumed == 0`
   with `message: None` means no progress; call again otherwise."* Three lanes
   have now paid for this separately.

2. **`Connection::receive` queues automatic replies but does not encode them.**
   A ping is answered only once something calls `flush`, so a host that flushes
   around application writes answers pings at the peer's mercy. The behaviour is
   right — `flush` is where output is produced — but nothing in `receive`'s
   signature or the README says an incoming *control* frame leaves work behind.
   `flush`'s own doc comment ("Flush automatic pong/close replies after
   consuming an incoming message") is the only statement of it, and it is on the
   function a host has no reason to read.

3. **`derive_accept_key` is not re-exported.** A host that already has an HTTP
   server and wants only `Sec-WebSocket-Accept` — which is exactly what
   `perry-ext-http`'s and `perry-ext-fastify`'s hyper paths had — must either
   reach past the crate to `tungstenite`, reintroducing the dependency the
   sans-I/O core was adopted to remove, or go through `accept` and parse the
   header back out of the returned `Head`. Perry does the latter now and is
   better for it (it gets the validation too), but the alternative should not
   have been "add tungstenite back".

4. **`accept` cannot refuse with a reason a host can render.** It returns
   `turnloop_http::Error`, whose code is always `WS_ERR_INVALID_HANDSHAKE`
   regardless of whether the request was missing `Sec-WebSocket-Version`, had a
   duplicate header, or offered a subprotocol the server does not speak. `ws`
   distinguishes those to the client (400 vs 426), and a host cannot.

5. **`LocalExecutor` still silently drops completions it did not issue** — P5's
   gap #2, unchanged, and the reason `turnloop_websocket::asynchronous` is as
   unusable from Perry as `turnloop_http::asynchronous` was. The sans-I/O core
   is what both lanes used instead; the `turnloop` feature of these crates is
   dead weight for a host that owns its own `Loop`.

Not a gap, recorded because the inventory said it was: **the handshake never
needed an owned stream.** See "The question this lane was given".

## Pre-existing `ws` findings this lane surfaced but did NOT fix

Recorded because they were measured, not guessed, and because a reader of the
byte-identical fixture table above would otherwise conclude the `ws` binding is
finished. It is not.

* **`typeof WebSocket` is `'undefined'`** under Perry for
  `import { WebSocket } from 'ws'` and `import { WebSocketServer } from 'ws'`
  (Node: `'function'`), even though `new WebSocket(...)` off that same binding
  works. Any `typeof`-guarded feature detection against `ws` fails, and a
  `WebSocket.CLOSED`-style static read is a live risk.
* **`import WebSocket from 'ws'` (default) is an object**, with `.Server` and
  `.WebSocketServer` on it. Node resolves ESM through `wrapper.mjs` and gives a
  function with neither. Perry is serving the CJS namespace as the default
  export.
* **`WebSocketServer({ path })` is read nowhere**, so such a server accepts on
  every path. Silently.
* **`'error'` receives a JS string, not an `Error`** — `err.message` is
  `undefined`.
* **`ws.send(data)` on a message the peer sent as text delivers a JS string**
  where `ws` delivers a `Buffer`. The fixtures do not distinguish them because
  both stringify the same, but `Buffer.from(data)` does not.

These are the binding's, not the transport's, and none of them changed here.

## HTTPS, and one defect the fixtures could not see

The attached path was claimed to be TLS-transparent because `write_raw` already
is. That claim was **tested rather than asserted**, and testing it found a real
defect — which is the whole argument for testing it.

`https.createServer({key, cert})` + `new WebSocketServer({ server })`, driven by
a **Node** `ws` client over `wss://` (so nothing on the client side is Perry's):

```
client: open
client: message isBinary=false text=echo:over-tls
--- perry said ---
server: connection clients=1
server: message isBinary=false text=over-tls
server: close code=1000 reason=tls-done
```

The TLS handshake, the `101` over TLS, `wss.on('connection')`, `isBinary`, the
echo and the peer's close code all work. **But the first run of this probe
produced no server output at all** and the client hung after `open`: an
`https.createServer()` never drained its `'upgrade'` queue, because the
main-thread pump called `try_recv_upgrade` for every `HttpServer` handle and for
none of the `HttpsServer` ones. Fixed here (`drain_upgrades`, now shared by both
loops) — a pre-existing hole that only became reachable once an HTTPS server
stopped declining the turnloop path.

### Still open: an external client sees 1006 on a peer-initiated close

In the exchange above the client reports `close code=1006` where Node would
report `1000`. **This is not TLS-specific** — the identical probe over plain
`http.createServer()` reproduces it exactly, so the TLS layer is exonerated:

```
client: close code=1006 reason=
server: close code=1000 reason=tls-done
```

A raw-socket probe settles what actually reaches the wire. The client does the
handshake by hand, sends `hi`, then a masked close frame carrying `1000 "bye"`,
and prints every byte back:

```
101 seen: HTTP/1.1 101 Switching Protocols
frame op=1 fin=true len=7 hex=6563686f3a6869      <- the application's echo, "echo:hi"
server sent FIN                                    <- and then nothing else
socket closed hadError=false
```

So the connection is healthy, application frames go out, and the *answering
close frame never leaves*. The server's own `'close'` handler reports
`code=1000 reason=bye`, so the frame was decoded; a unit test
(`a_peer_close_is_echoed_back_onto_the_wire`) proves the codec puts the echo in
its output buffer on exactly this input; and `turnloop_link::on_data` takes that
output and writes it **before** anything closes.

Two hypotheses were tested and both are wrong, which is worth recording so the
next person does not retest them:

* **A second shutdown resetting the connection.** The peer's FIN reached
  `on_eof`, which shut down again; the second `shutdown(2)` answers `ENOTCONN`,
  and the error path answered *that* with `destroy_connection` — a
  `Loop::close`, which cancels outstanding operations. Plausible, and fixed
  anyway (`on_eof` / `on_error` now report whether this layer still owned the
  connection, and the host tears down only when it did). **It did not change the
  observable.**
* **The shutdown racing the write it should follow.** Removing the shutdown
  entirely — retiring the link and leaving the peer's FIN to drive it — made it
  *worse*: with nothing closing, `ws` hit its own close timeout and still
  reported 1006. Reverted.

What that leaves is the write itself. The strongest remaining suspect is the
`user` tag: the echo is submitted as `tl::write(id, bytes, 0)` and the shutdown
as `tl::shutdown(id, 0)`, the same tag, on the same handle — and P5's TLS
accounting treats `user == 0` as "not an application write". Untested; named so
it can be tested first.

Note carefully why `test_gap_turnloop_ws_attached` is byte-identical to Node
anyway: its client is **Perry's own** `ws` client, and that client does receive
the echo and reports 1000. A fixture with both ends on the same engine cannot
see this class of bug, which is exactly why the external-client probe exists and
why it is reported here rather than quietly passing. Filed as remaining work,
not as done.

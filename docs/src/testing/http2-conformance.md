# `node:http2` conformance fixtures

Fourteen gap fixtures under `test-files/test_gap_http2_*.ts`, plus the shared
wire codec `test-files/_helpers/h2_wire.ts`, that pin what Perry's `node:http2`
has to do — measured against **Node 26.5.1**, the `.node-version` pin, never
against Perry's current output.

## Why this set exists

Perry's HTTP/2 **streams** are real: `perry-ext-http`'s server is hyper
(`hyper_util::server::conn::auto::Builder`, ALPN-aware) and `http2.connect` is
the `h2` crate. What is *not* real is the **control surface**. In
`crates/perry-ext-http/src/server/http2_server/controls.rs`:

```rust
let mut peer_ids = Vec::new();
iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
    if get_handle::<Http2SessionHandle>(peer_id)
        .map(|session| session.session_type == peer_type && …)
```

`session.settings()`, `session.ping()` and `session.goaway()` never encode a
frame. They walk the process's own handle table for a session of the opposite
kind and push a synthetic event at it. `test-parity/node-suite/http2/` passes
because both ends of every one of its fixtures are Perry, in one process, so
the simulation is always able to find its "peer".

Three consequences that the existing suite cannot see:

| symptom | where |
|---|---|
| `ping()`'s round-trip duration is the literal `0.0` | `pump.rs`: `call3(callback, err, 0.0, payload_arg)` |
| `settings()`'s callback gets **two** arguments, not three | `pump.rs`: `call2(callback, err, settings_arg)` |
| a *server* session's `goaway()`/`settings()` reaches **every** client session in the process | `controls.rs`: the server-handle filter is `unwrap_or(true)` for a server caller |

And the load-bearing one: against a real peer — another process, curl, a
browser, a load balancer — none of those three methods does anything at all.

## How the fixtures defeat the simulation

Every fixture whose name contains `_wire_` puts a **raw TCP socket** on one end
and hand-encodes/decodes HTTP/2 frames (`test-files/_helpers/h2_wire.ts`). The
loopback path cannot satisfy them: the peer is a socket, so there is no second
`Http2SessionHandle` in the process to find, and a session that never writes a
frame prints `(none)`.

The codec is deliberately small:

* frame headers are assembled and parsed with plain index arithmetic rather
  than `Buffer.readUIntBE`, so a Buffer-method gap cannot make an http2 fixture
  fail for an unrelated reason;
* **only an HPACK encoder** is included, and only "literal header field without
  indexing, new name, no Huffman" (RFC 7541 §6.2.2) — a `0x00` prefix byte then
  length-prefixed name and value. That is enough to *open* real streams. There
  is no decoder and no Huffman table, because every assertion in the set reads
  **control** frames (SETTINGS, PING, GOAWAY, RST\_STREAM, WINDOW\_UPDATE,
  DATA), none of which carries a header block.

Event-driven awaits are **bounded** (`withTimeout` / `waitEvent` / `barrier`,
2–6 s). Node wins every one of those races by three orders of magnitude, so the
fallback never appears in the oracle output — verified by re-running the whole
set after adding them and diffing byte-for-byte against the pre-change capture.
An implementation that never fires the event prints a `!!`-prefixed line and
the fixture fails on *that* line, with a readable diff, instead of silently
consuming the harness's 10 s budget.

`RawClientPeer` speaks the connection preface at a Perry/Node **server**;
`RawServerPeer` accepts one connection and puts a real wire under a Perry/Node
**client**. Both record every frame they receive as a one-line rendering, which
is what the fixtures print — so the gap diff is a diff of the wire.

## Oracle evidence

| | |
|---|---|
| oracle | Node **26.5.1** (`.node-version`), `/opt/node-v26.5.1-linux-x64/bin` on the Linux box |
| determinism | every fixture run **3×** on Linux x86-64: byte-identical each time |
| portability | every fixture also run on macOS arm64: **byte-identical to Linux**, all 14 |
| `npm ci` | run in the tree before any sweep (`--ignore-scripts --no-audit --no-fund`) |

No expectation in this set was written from Perry's output or from the RFC. Two
of them contradict the RFC-derived folklore outright — see *Corrections* below.

## The behaviour catalogue

Each row is a behaviour, with Node's **actual** output beside it. The fixture
column names the file that pins it.

### SETTINGS on the wire — `test_gap_http2_wire_server_settings.ts`

| behaviour | Node 26.5.1 |
|---|---|
| a default `createServer()`'s first frame | `SETTINGS stream=0 {}` — **empty**, length 0 |
| it ACKs the peer's SETTINGS | `SETTINGS stream=0 ACK len=0` |
| the peer's advertised values do not change what the server sends | still `SETTINGS stream=0 {}` |
| `createServer({ settings: { enablePush:false, maxConcurrentStreams:7, initialWindowSize:1234 } })` | `SETTINGS stream=0 {2=0,3=7,4=1234}` |
| all six keys | `SETTINGS stream=0 {1=8192,2=0,3=11,4=131072,5=32768,6=40000}` |
| record order | **ascending identifier**, the same order `getPackedSettings` uses |
| connection-level WINDOW\_UPDATE at handshake | none is sent |

### SETTINGS acknowledgement ordering — `test_gap_http2_wire_settings_ack.ts`

This is the fixture the simulation cannot survive. A raw peer that ACKs only
when told to exposes the entire state machine:

```
pendingSettingsAck at connect: true
-- after ACK #1 (resolves the INITIAL settings) --
events: ["localSettings iws=65535 mcs=4294967295"]
pendingSettingsAck: false
-- after settings(), BEFORE ACK #2 --
wire:
  SETTINGS stream=0 {3=9,4=32768}
events: []
pendingSettingsAck: true
localSettings.initialWindowSize: 65535
callback: not-fired
-- after ACK #2 --
events: ["localSettings iws=32768 mcs=9"]
pendingSettingsAck: false
localSettings.initialWindowSize: 32768
callback: err=null iws=32768 mcs=9 durationIsNumber=true durationPositive=true
```

* `pendingSettingsAck` is **true from connect** — the initial SETTINGS frame is
  outstanding until the peer ACKs it.
* One ACK resolves one outstanding SETTINGS, in order. The first ACK fires
  `'localSettings'` with the **connect-time** values, not with anything the
  program asked for.
* `settings({…})` changes nothing observable until its own ACK arrives:
  `session.localSettings` still reports the old values and the callback has not
  fired.
* The callback takes **three** arguments — `(err, settings, duration)` — and
  `duration` is a number strictly greater than zero.
* A peer SETTINGS frame fires `'remoteSettings'` **and must be ACKed on the
  wire**.
* `'remoteSettings'` at connect reports `maxConcurrentStreams: 4294967295` and
  `maxHeaderListSize: 4294967295` for unspecified keys — note this differs from
  `getDefaultSettings()`, which reports `65535` for the header-list keys.

### PING — `test_gap_http2_wire_server_ping.ts`, `test_gap_http2_wire_client_ping.ts`

| behaviour | Node 26.5.1 |
|---|---|
| a PING arriving at the server | `PING stream=0 ACK len=8 7065727279683221` — identical 8 bytes echoed |
| an all-zero payload | `PING stream=0 ACK len=8 0000000000000000` — echoed, not omitted |
| `session.ping(payload, cb)` | writes `PING stream=0 len=8 …` (type 6, flags 0, stream 0) |
| the callback | fires only after the peer's ACK, as `(null, duration, payload)` |
| `duration` | `durationIsNumber:true durationPositive:true` — a real measurement |
| `ping(cb)` with no payload | generates a **random** 8-byte payload, returned verbatim to the callback |
| `ping()` with no callback | **throws** `TypeError ERR_INVALID_ARG_TYPE: The "callback" argument must be of type function. Received undefined` |
| a 7- or 9-byte payload | `RangeError ERR_HTTP2_PING_LENGTH: HTTP2 ping payload must be 8 bytes` |
| a string payload | `TypeError ERR_INVALID_ARG_TYPE: The "payload" argument must be an instance of Buffer, TypedArray, or DataView` |
| an **unsolicited** `PING\|ACK` | protocol error: `GOAWAY stream=0 last=0 code=2` and the connection is torn down |

### GOAWAY, sending — `test_gap_http2_wire_goaway_send.ts`

```
goaway():                                     GOAWAY stream=0 last=0 code=0 opaque=""
goaway(NGHTTP2_ENHANCE_YOUR_CALM):            GOAWAY stream=0 last=0 code=11 opaque=""
goaway(NO_ERROR, 0, Buffer('shutting-down')): GOAWAY stream=0 last=0 code=0 opaque="shutting-down"
```

Sending a GOAWAY leaves `closed:false destroyed:false` and the socket writable —
it is an announcement, not a teardown.

**The lastStreamID parity rule.** From a *client* session nghttp2 emits the
frame only when `lastStreamID` is **even**:

| `goaway(NO_ERROR, n)` from a client | wire |
|---|---|
| `n = 0` | `GOAWAY stream=0 last=0 code=0` |
| `n = 2` | `GOAWAY stream=0 last=2 code=0` |
| `n = 1` | **(none)** |
| `n = 2147483647` | **(none)** |

No frame, no throw, no error event. `2147483647` — the "graceful shutdown"
sentinel every HTTP/2 tutorial reaches for — is silently dropped from a client.

Each row needs its **own session**: a GOAWAY does not close the session, but
nghttp2 clamps a later GOAWAY's `lastStreamID` to be non-increasing, so a
second `goaway(0, 2)` on a session that already sent `goaway()` writes
`last=0`. The code and opaque-data variants have no such constraint and do
share one connection in the fixture.

`goaway(0, 0, "bye")` throws `TypeError ERR_INVALID_ARG_TYPE: The "opaqueData"
argument must be an instance of Buffer, TypedArray, or DataView`.

### GOAWAY, receiving — `test_gap_http2_wire_goaway_recv.ts`

| frame received | `'goaway'` args | session after |
|---|---|---|
| `last=0 NO_ERROR opaque="byebye"` | `code=0 last=0 dataType=Buffer(6) data="byebye"` | `closed:true destroyed:true`, two GOAWAYs written back |
| `last=0 NO_ERROR`, no opaque data | `code=0 last=0 dataType=`**`undefined`** | same |
| `last=2147483647 NO_ERROR` | `code=0 last=2147483647 dataType=undefined` | same — "graceful" does **not** keep an idle session alive |
| `last=0 ENHANCE_YOUR_CALM(11)` | `code=11 last=0` then `session error ERR_HTTP2_SESSION_ERROR` | `closed:`**`false`**` destroyed:true`, **one** GOAWAY back |

The third argument is `undefined` when the frame carried no opaque data — not a
zero-length Buffer. Perry's pump always builds a Buffer.

`request()` after that: the returned stream is already closed with
`rstCode: 2`, `stream.id` is `undefined`, and it emits
`ERR_HTTP2_INVALID_SESSION: The session has been destroyed`.

### GOAWAY on the server, and the stream that follows — `test_gap_http2_wire_goaway_server.ts`

```
server session.goaway(NO_ERROR, 1, 'draining'):
  GOAWAY stream=0 last=1 code=0 opaque="draining"
  session closed: false destroyed: false
  peer socket closed: false
HEADERS for stream 3 AFTER the graceful GOAWAY:
  (none)
  peer socket closed: false session destroyed: false
  notes: ["server 'stream' id=1 path=/hold"]
```

`server.close()` after one completed stream writes **two** GOAWAY frames
(nghttp2's shutdown notice then the real one), both
`last=1 code=0`, then closes the socket.

A *client* GOAWAY arriving while a stream is open fires the server session's
`'goaway'` (`code=0 last=0 data="bye"`), is answered with the server's own
`GOAWAY last=1 code=0`, and does **not** tear the connection down.

### maxConcurrentStreams and REFUSED\_STREAM — `test_gap_http2_wire_refused_stream.ts`

The limit has two regimes, and this is the part no reimplementation guesses:

| regime | four concurrent streams against `maxConcurrentStreams: 2` |
|---|---|
| peer has **not** ACKed the server's SETTINGS | `RST_STREAM stream=5 code=7`, `RST_STREAM stream=7 code=7` — REFUSED\_STREAM, session survives |
| peer **has** ACKed it | `RST_STREAM stream=1 code=2`, `GOAWAY stream=0 last=3 code=2` — INTERNAL\_ERROR, connection torn down, `ERR_HTTP2_ERROR errno=-505` |

Exceeding a limit the peer has already acknowledged is a protocol violation, so
nghttp2 stops being polite about it. Refused streams never reach the `'stream'`
handler; in the tolerated regime, finishing a held stream frees a slot and a
later stream id is accepted normally.

A client stream RST'd by its peer closes with that `rstCode` (`8` for CANCEL)
while its **siblings keep working** and the session stays up.

### Flow control — `test_gap_http2_wire_flow_control.ts`

```
initialWindowSize=1000 -> dataBytes: 1000 dataFrames: 1
after stream WINDOW_UPDATE +5000 -> dataBytes: 6000
after stream WINDOW_UPDATE +1000000 -> dataBytes: 65535
  stalled at the default connection window (65535): true
after connection WINDOW_UPDATE +1000000 -> dataBytes: 200000
  whole body delivered: true
```

Exact arithmetic, and two independent windows: opening only the stream window
leaves the transfer pinned at the connection default of 65535. On the receiving
side, `stream.pause()` stops the body at **0 bytes** and `resume()` delivers
every one of 300000 with no loss and no duplication.

### Protocol-error mapping — `test_gap_http2_wire_frame_errors.ts`

| injected frame | GOAWAY |
|---|---|
| SETTINGS with a 5-byte payload | `code=6` (FRAME\_SIZE\_ERROR) |
| `SETTINGS\|ACK` carrying a payload | `code=6` |
| PING with a 7-byte payload | `code=6` |
| DATA on stream 0 | `code=1` (PROTOCOL\_ERROR), `opaque="DATA: stream_id == 0"` |
| WINDOW\_UPDATE with increment 0 | `code=2` (INTERNAL\_ERROR) |
| RST\_STREAM on an idle stream | `code=2` |
| HEADERS on stream 0 | `code=2` |
| corrupt HPACK block | `code=9` (COMPRESSION\_ERROR), `last=1` |
| unknown frame type `0x63` | **(none)** — ignored, session survives |

The DATA-on-stream-0 case carries nghttp2's debug string in the GOAWAY's opaque
field. That is an implementation detail no spec reading produces; it is in the
fixture because Node emits it.

In every failing case the API surface is identical:
`Error [ERR_HTTP2_ERROR]: Protocol error` with `errno: -505`
(`NGHTTP2_ERR_PROTO`), followed by `'close'`. **No `'frameError'` is emitted on
the receiving side** — `'frameError'` is a send-side event.

### close() vs destroy() — `test_gap_http2_wire_session_lifecycle.ts`

| call | wire | `closed` | `destroyed` | events |
|---|---|---|---|---|
| `close(cb)` on an idle session | **two** GOAWAY `code=0` | `true` | `true` | `close`, then the callback |
| `close()` with a stream open | one GOAWAY `code=0` | `true` | `false` | none yet — draining |
| `destroy()` | one GOAWAY `code=0` | **`false`** | `true` | `close` |
| `destroy(new Error('boom'), NGHTTP2_PROTOCOL_ERROR)` | one GOAWAY `code=1` | `false` | `true` | `error` (message `boom`, **`code` undefined**), `close` |

`closed` is not a superset of `destroyed`.

### ALPN and TLS — `test_gap_http2_alpn_secure.ts`

| server | client offers | result |
|---|---|---|
| `createSecureServer()` | `["h2","http/1.1"]` | `alpnProtocol="h2"` |
| `createSecureServer()` | `["http/1.1"]` | handshake **fails**: `ERR_SSL_TLSV1_ALERT_NO_APPLICATION_PROTOCOL` |
| `createSecureServer()` | `[]` | `alpnProtocol=`**`false`** (the boolean) |
| `{ allowHTTP1: true }` | `["h2","http/1.1"]` | `alpnProtocol="h2"` |
| `{ allowHTTP1: true }` | `["http/1.1"]` | `alpnProtocol="http/1.1"` |

End to end: `status=200 scheme="https" body="secure-ok"`, session socket
`alpnProtocol: "h2"`, `encrypted: true`. With `allowHTTP1: true` an HTTPS/1.1
request reaches the `'request'` handler with `httpVersion=1.1`.

The certificate and key are the repo's existing
`test-parity/node-suite/tls/fixtures/localhost-{cert,key}.pem` (CN=localhost,
SAN `DNS:localhost` + `IP:127.0.0.1`, valid to 2036), inlined so the fixture is
self-contained.

### Streams end to end — `test_gap_http2_e2e_streams.ts`

Both ends are the implementation under test here, so this is the **regression
floor** the transport lane must not break while it replaces the control
surface.

* four concurrent requests on one session get ids 1, 3, 5, 7 in request order;
  `state.nextStreamID` goes 1 → 9;
* `:status` arrives as a **number**;
* `stream.close(NGHTTP2_INTERNAL_ERROR)` surfaces on **both** ends as
  `ERR_HTTP2_STREAM_ERROR` with `rstCode 2` — an unhandled `'error'` on the
  *server* stream takes the process down — and the siblings complete normally
  on the same session;
* trailers: event order is exactly `["response","data","trailers","end"]`;
* the session's EventEmitter surface is probed explicitly —
  `on once addListener off removeListener emit removeAllListeners`, all
  `function`. This is a subject here rather than an instrument: `once` is
  **not** in perry-ext-http's http2 session dispatch
  (`server/http2_server/dispatch.rs` accepts `on` / `addListener` only), which
  is why the shared barrier in `_helpers/h2_wire.ts` uses `on` with a one-shot
  guard. A barrier built on `once` would have made every fixture in this set
  fail for the wrong reason.

### Session isolation — `test_gap_http2_session_isolation.ts`

Two servers, one client each, one process. Every control frame lands on exactly
one session and nowhere else:

```
== alpha.client.settings({ maxConcurrentStreams: 21 }) ==
["alpha.server got remoteSettings mcs=21"]
== bravo.serverSession.settings({ maxConcurrentStreams: 32 }) ==
["bravo.client got remoteSettings mcs=32"]
== alpha.serverSession.goaway(NO_ERROR, 0) ==
["alpha.client got goaway code=0 last=0"]
== bravo.client.goaway(NGHTTP2_ENHANCE_YOUR_CALM) ==
["bravo.server got goaway code=11 last=0"]
```

This is the one fixture that needs no raw socket and still cannot pass under the
loopback shim: `controls.rs` pushes a server caller's event at **every** client
handle in the process.

## Corrections to widely-held beliefs

Two expectations that the brief for this work carried, and that the oracle
refuted:

1. **"A stream opened after a graceful GOAWAY gets `RST_STREAM(REFUSED_STREAM)`
   and the session is kept."** The session *is* kept, but Node sends **no frame
   at all** — nghttp2 ignores HEADERS for a stream id above the GOAWAY's
   `lastStreamID`. RFC 7540 §6.8 permits either; Node chose "ignore".
   REFUSED\_STREAM is real, but its trigger is `maxConcurrentStreams`.
2. **"`lastStreamID` is just a number you pass."** From a client session an odd
   `lastStreamID` suppresses the frame entirely, silently.

## What the transport lane must implement to pass

Ordered by how much of the fixture set each unlocks.

1. **Encode and decode SETTINGS, PING, GOAWAY, RST\_STREAM and WINDOW\_UPDATE
   on the real connection**, replacing `iter_handle_ids_of::<Http2SessionHandle>`
   in `controls.rs` entirely. Everything below depends on this.
   *Unlocks:* `_wire_server_settings`, `_wire_server_ping`, `_wire_client_ping`,
   `_wire_goaway_send`, `_wire_goaway_recv`, `_wire_goaway_server`,
   `_session_isolation`.
2. **A SETTINGS ACK state machine**: one outstanding-SETTINGS queue per session;
   `pendingSettingsAck` true from connect; `'localSettings'` and the user
   callback fire on the matching ACK and not before; `session.localSettings`
   updates only then; inbound SETTINGS must be ACKed on the wire.
   *Unlocks:* `_wire_settings_ack`.
3. **Measure the PING round trip.** Record a timestamp at submit, subtract at
   ACK, pass it as the callback's second argument — `pump.rs`'s hardcoded `0.0`
   is the current value. Match the payload by its 8 bytes so concurrent pings
   resolve to the right callback, and generate a random payload for
   `ping(cb)`.
4. **Give the settings callback its third argument** (`duration`);
   `pump.rs` currently calls `call2`.
5. **Distinguish "no opaque data" from "empty opaque data"** on the `'goaway'`
   event: `undefined`, not a zero-length Buffer.
6. **Session lifecycle**: `close()` writes two GOAWAYs when idle and one while
   draining; `destroy()` writes one and leaves `closed` false;
   `destroy(err, code)` puts `code` in the frame.
7. **Argument validation** at the Node error codes and messages:
   `ERR_HTTP2_PING_LENGTH`, `ERR_INVALID_ARG_TYPE` for a missing ping callback
   and for non-Buffer payload/opaqueData, `ERR_HTTP2_INVALID_SESSION` for a
   request after teardown.
8. **The lastStreamID parity rule** — drop a client `goaway()` whose
   `lastStreamID` is odd, silently.
9. **Protocol-error detection and mapping** to the GOAWAY codes in the table
   above, surfacing `ERR_HTTP2_ERROR` with `errno -505`, and **ignoring unknown
   frame types**.
10. **Flow control**: honour the peer's `SETTINGS_INITIAL_WINDOW_SIZE`, keep
    stream and connection windows independent, and act on WINDOW\_UPDATE.
11. **maxConcurrentStreams enforcement** in both regimes — REFUSED\_STREAM
    before the peer's ACK, PROTOCOL\_ERROR after it.
12. **ALPN on `createSecureServer`**: `h2` only by default (fail the handshake
    otherwise), `http/1.1` when `allowHTTP1` is set, `alpnProtocol === false`
    when the client offers nothing.

## Fixture status

| fixture | oracle-verified | notes |
|---|---|---|
| `test_gap_http2_wire_server_settings.ts` | yes | |
| `test_gap_http2_wire_server_ping.ts` | yes | |
| `test_gap_http2_wire_client_ping.ts` | yes | |
| `test_gap_http2_wire_settings_ack.ts` | yes | |
| `test_gap_http2_wire_goaway_send.ts` | yes | |
| `test_gap_http2_wire_goaway_recv.ts` | yes | |
| `test_gap_http2_wire_goaway_server.ts` | yes | |
| `test_gap_http2_wire_refused_stream.ts` | yes | |
| `test_gap_http2_wire_flow_control.ts` | yes | |
| `test_gap_http2_wire_frame_errors.ts` | yes | |
| `test_gap_http2_wire_session_lifecycle.ts` | yes | |
| `test_gap_http2_alpn_secure.ts` | yes | |
| `test_gap_http2_e2e_streams.ts` | yes | both ends are the implementation — a regression floor, not a wire test |
| `test_gap_http2_session_isolation.ts` | yes | single process, no raw socket, still defeats the loopback shim |

All fourteen are oracle-verified: run against Node 26.5.1 three times on Linux
and once on macOS, byte-identical every time.

**All fourteen fail against Perry today**, which is the intended state — the
set was written to fail and to flip when the transport lands. The per-fixture
divergence, and what it says about the current implementation, is in
`docs/turnloop/h2c-report.md`. Their statuses are recorded in
`test-parity/gap_snapshot.json`, so a fixture that starts passing fails the gap
gate until the snapshot diff is committed; the fixture set therefore cannot
silently stop being the acceptance criterion.

## Not covered

* **`push_promise` / server push.** Node still implements it; no fixture here.
* **`respondWithFile` / `respondWithFD`.** Not exercised.
* **CONNECT and the extended CONNECT protocol** (`enableConnectProtocol`, RFC
  8441). The setting's *encoding* is covered by `test_gap_http2_settings.ts`;
  its behaviour is not.
* **HPACK decoding.** By design — see above. Response header *content* is
  checked through the Node/Perry client in `_e2e_streams`, not off the wire.
* **`maxSessionMemory`.** Probed, but Node exposes no deterministic observable
  for it: it is absent from `session.state`, and the only symptom is a
  teardown under memory pressure that is not reproducible byte-for-byte. Left
  out rather than faked.
* **Two-process fixtures.** Everything here runs in one process (the gap
  harness compares one program's stdout); the raw socket is what makes the wire
  real, not a second process.
* **Windows and macOS as *Perry* targets.** The Perry side was run on Linux
  only. The Node oracle was cross-checked on macOS.

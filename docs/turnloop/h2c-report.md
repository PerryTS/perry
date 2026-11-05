# h2c — the `node:http2` conformance fixture set

Branch `h2c/http2-conformance`, based on `turnloop/integration` at `1db2f76e3`.
Written and measured on the shared Linux box (EPYC 9354P) against the pinned
gap oracle Node **26.5.1**, with `npm ci --ignore-scripts --no-audit --no-fund`
run in the tree first. Nothing here was run on Windows; the Node oracle was
cross-checked on macOS arm64, the Perry side was not. Nothing was benchmarked.

**This lane changes no transport code.** It delivers fourteen gap fixtures, a
shared HTTP/2 frame codec, a companion document
(`docs/src/testing/http2-conformance.md`) that lists every behaviour with
Node's actual output beside it, and the ordered list of what the transport lane
has to implement. A sibling lane owns the implementation.

## The problem this set exists to make visible

Perry's HTTP/2 **streams** are real. `perry-ext-http`'s server is hyper
(`hyper_util::server::conn::auto::Builder`, ALPN-aware) and `http2.connect` is
the `h2` crate. What is simulated is the **control surface**. From
`crates/perry-ext-http/src/server/http2_server/controls.rs`:

```rust
let mut peer_ids = Vec::new();
iter_handle_ids_of::<Http2SessionHandle, _>(|peer_id| {
    if get_handle::<Http2SessionHandle>(peer_id)
        .map(|session| {
            session.session_type == peer_type
                && !session.closed
                && !session.destroyed
                && local_server_handle
                    .map(|server_handle| session.server_handle == server_handle)
                    .unwrap_or(true)
```

`session.settings()`, `session.ping()` and `session.goaway()` never encode a
frame. They walk the **process's own handle table** for a session of the
opposite kind and push a synthetic event at it.
`test-parity/node-suite/http2/` passes because both ends of every fixture there
are Perry, in one process, so the scan always finds its "peer". Three of those
fixtures — `session/ping-echo.ts`, `session/goaway-opaque-data.ts`,
`session/settings-callback.ts` — are the ones that look like they cover this
surface, and each asserts only what the simulation hands straight back
(`typeof duration`, the opaque bytes it was given, the settings object it was
given).

Three defects the existing suite structurally cannot see, all confirmed by
running the new fixtures:

| defect | evidence |
|---|---|
| `ping()`'s round-trip duration is the literal `0.0` | `pump.rs`: `call3(callback, err, 0.0, payload_arg)`; fixture prints `durationPositive: false` |
| `settings()`'s callback gets two arguments, not three | `pump.rs`: `call2(callback, err, settings_arg)` |
| a server session's `goaway()`/`settings()` reaches **every** client session in the process | `controls.rs`: the server-handle filter is `unwrap_or(true)` for a server caller |

And the load-bearing one: against any real peer none of those three methods
does anything at all.

## How the fixtures defeat the simulation

Every fixture whose name contains `_wire_` puts a **raw TCP socket** on one end
and hand-encodes/decodes HTTP/2 frames. The loopback path cannot satisfy them:
the peer is a socket, so there is no second `Http2SessionHandle` in the process
to find, and a session that never writes a frame prints `(none)`.

The shared codec is `test-files/_helpers/h2_wire.ts` (436 lines). Three design
decisions are load-bearing:

* **Byte arithmetic, not `Buffer.readUIntBE`.** Frame headers are assembled and
  parsed with plain index arithmetic so a Buffer-method gap cannot make an
  http2 fixture fail for an unrelated reason.
* **An HPACK encoder, and no decoder.** Only "literal header field without
  indexing, new name, no Huffman" (RFC 7541 §6.2.2) — a `0x00` prefix byte then
  length-prefixed name and value. That is enough to *open* real streams. Every
  assertion in the set reads **control** frames (SETTINGS, PING, GOAWAY,
  RST_STREAM, WINDOW_UPDATE, DATA), none of which carries a header block, so no
  Huffman table is needed and none is shipped.
* **`on(...)` with a one-shot guard, never `once(...)`, in the shared barrier.**
  `once` is not in the method surface of perry-ext-http's http2 session handle
  (`server/http2_server/dispatch.rs` accepts `on` / `addListener` only). A
  barrier built on `once` would have made every fixture in the set fail for the
  wrong reason. The first Perry pass of this lane was run with a `once`-based
  barrier before that was caught; the results below are from the corrected
  pass. `typeof session.once` is probed on its own line in
  `test_gap_http2_e2e_streams.ts` instead, where it is the subject rather than
  the instrument.

Event-driven awaits are **bounded** (`withTimeout` / `waitEvent` / `barrier`,
2–6 s). Node wins every one of those races by three orders of magnitude, so the
fallback never appears in the oracle output — verified by re-running the whole
set after adding them and diffing byte-for-byte against the pre-change capture
(unchanged, all 14). An implementation that never fires the event prints a
`!!`-prefixed line and the fixture fails on *that* line, with a readable diff,
instead of silently consuming the harness's 10 s budget.

## Oracle evidence

| | |
|---|---|
| oracle | Node **26.5.1** — `/opt/node-v26.5.1-linux-x64/bin`, first on PATH (the box's default is 26.8.1 and gives different answers) |
| tree | `/root/claude-h2c`, cloned `--reference /root/projects/perry/perry --dissociate`, `OWNER` file present |
| `npm ci` | `--ignore-scripts --no-audit --no-fund`, 32 packages, before any sweep |
| determinism | every fixture run **3×** on Linux x86-64: byte-identical each time, md5 recorded |
| portability | every fixture also run on macOS arm64 under the same Node: **byte-identical to Linux**, all 14 |
| build | `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static -p perry-ext-http`, `PERRY_RUNTIME_DIR` pinned to this tree's `target/release` |

**No expectation in this set was written from Perry's output, and none from the
RFC where Node's behaviour is observable.** Two were written from the RFC first,
and the oracle refuted both — see *Corrections* below.

## Corrections the oracle forced

Both of these were in the brief for this lane as statements of fact. Neither
survived contact with Node.

**1. "A stream opened after a graceful GOAWAY gets `RST_STREAM(REFUSED_STREAM)`
and the session is kept."** The session is kept. Node sends **no frame at
all**:

```
server session.goaway(NO_ERROR, 1, 'draining'):
  GOAWAY stream=0 last=1 code=0 opaque="draining"
  session closed: false destroyed: false
HEADERS for stream 3 AFTER the graceful GOAWAY:
  (none)
  peer socket closed: false session destroyed: false
  notes: ["server 'stream' id=1 path=/hold"]
```

nghttp2 ignores HEADERS for a stream id above the GOAWAY's `lastStreamID` —
no RST_STREAM, no `'stream'` event, no error. RFC 7540 §6.8 permits either;
Node chose "ignore". Had this fixture been written from the spec it would have
pinned Perry to behaviour Node does not have.

REFUSED_STREAM is real, but its trigger is `maxConcurrentStreams`, and **it has
two regimes** — which is the part no reimplementation guesses:

| regime | four concurrent streams against `maxConcurrentStreams: 2` |
|---|---|
| peer has **not** ACKed the server's SETTINGS | `RST_STREAM stream=5 code=7`, `RST_STREAM stream=7 code=7` — REFUSED_STREAM, session survives |
| peer **has** ACKed it | `RST_STREAM stream=1 code=2`, `GOAWAY stream=0 last=3 code=2` — INTERNAL_ERROR, connection torn down, `ERR_HTTP2_ERROR errno=-505` |

Exceeding a limit the peer has already acknowledged is a protocol violation, so
nghttp2 stops being polite about it.

**2. "`lastStreamID` is just a number you pass."** From a *client* session
nghttp2 emits the frame only when `lastStreamID` is **even**:

| `goaway(NO_ERROR, n)` from a client | wire |
|---|---|
| `n = 0` / `2` | `GOAWAY stream=0 last=n code=0` |
| `n = 1` / `2147483647` | **(none)** |

No frame, no throw, no error event. `2147483647` — the graceful-shutdown
sentinel every HTTP/2 tutorial reaches for — is silently dropped from a client.

The full behaviour catalogue, every row with Node's actual output, is
`docs/src/testing/http2-conformance.md`.

## What the fixtures found when run against Perry

Compiled with this tree's `target/release/perry` (auto-optimize on, which is
what the gap harness does for any test importing `node:http2`), each binary run
against the same expected output the Node oracle produced, under a 25 s
watchdog. The `status` column is the gap harness's own classification;
`wall` is the Perry binary's wall time.

`parity_fail` means the program ran to completion and its stdout differed.
`crash` means the watchdog fired: the program produced its **complete** output
and then did not exit — see *The exit hang* below, and note that the "what
diverges" column is still measured, from the stdout captured before the kill.

| fixture | status · diff lines · wall | what diverges |
|---|---|---|
| `_wire_server_settings` | `parity_fail` · 12 · 1.0 s | server sends a **hardcoded** `{3=200,4=1048576,5=16384,6=16384}` whatever `createServer({settings})` says, plus an unsolicited connection `WINDOW_UPDATE inc=983041` Node never sends |
| `_wire_server_ping` | `parity_fail` · 8 · 0.9 s | inbound PING **is** ACKed correctly (h2 does it); `session.ping()` writes nothing, `durationPositive:false`; an unsolicited `PING\|ACK` is ignored instead of `GOAWAY code=2` |
| `_wire_client_ping` | `crash` · 18 · hang | `(none)` on the wire; `durationPositive:false`; every validation case `NO THROW` |
| `_wire_settings_ack` | `crash` · 24 · hang | no `remoteSettings` at connect; `settings()` applies **immediately** and fires its callback with no ACK and `durationIsNumber:false`; nothing on the wire; a peer SETTINGS frame mid-session is answered with **`GOAWAY code=1`** instead of an ACK |
| `_wire_goaway_send` | `crash` · 14 · hang | every `goaway(...)` form writes `(none)` |
| `_wire_goaway_recv` | `crash` · 18 · hang | no `'goaway'` event at all; session state unchanged |
| `_wire_goaway_server` | `parity_fail` · 14 · 2.2 s | server `goaway()` writes nothing, so the post-GOAWAY stream is **served normally**; `server.close()` writes no GOAWAY; a client GOAWAY is not surfaced |
| `_wire_refused_stream` | `parity_fail` · 35 · 2.3 s | `maxConcurrentStreams: 2` is **not enforced** — all four streams accepted; no RST_STREAM in either regime |
| `_wire_flow_control` | `parity_fail` · 4 · 5.0 s | **part 1 is byte-exact** (1000 → 6000 → 65535 → 200000); part 2 fails — after `pause()`, `resume()` delivers 0 of 300000 bytes |
| `_wire_frame_errors` | `parity_fail` · 32 · 3.2 s | every malformed frame answered with `GOAWAY code=1`; no session `'error'` event at all |
| `_wire_session_lifecycle` | `crash` · 15 · hang | `close()` writes one GOAWAY not two; `closed` and `destroyed` are always both true; `destroy(err, code)` writes `code=0` and emits no `'error'` |
| `_alpn_secure` | `parity_fail` · 12 · 2.0 s | a default secure server accepts `http/1.1`; `http2.connect` over TLS fails (`received corrupt message of type InvalidContentType`, printed to **stdout**); the `allowHTTP1` path reports `httpVersion` 2.0 for an HTTP/1.1 request |
| `_e2e_streams` | `parity_fail` · 25 · 2.4 s | `:status` is a **string**; `state.nextStreamID` never advances; stream ids diverge (4th request gets 9, siblings 23/25); `stream.resume` is `undefined`; a GET does not auto-end; `stream.close(code)` produces no error and no `rstCode`; trailers do nothing; `once`/`off`/`removeListener`/`emit`/`removeAllListeners` are all `undefined` on the session |
| `_session_isolation` | `parity_fail` · 6 · 1.7 s | **cross-talk**: a server session's `settings()` and `goaway()` reach *both* clients in the process |

**14 fixtures, 0 pass — 9 `parity_fail`, 5 `crash`.** That is the intended
state: the set was written so it would fail today and flip when the transport
lands.

### The control surface writes nothing

`session.ping()`, `session.goaway()` and `session.settings()` put **no bytes on
the wire**. Every `_wire_` fixture that drives one of them records `(none)`
where Node records a frame:

```
wire for ping(Buffer('PERRY-h2'), cb):
-   PING stream=0 len=8 50455252592d6832
+   (none)
- callback: {"err":"null","durationIsNumber":true,"durationPositive":true,…}
+ callback: {"err":"null","durationIsNumber":true,"durationPositive":false,…}
```

The callback still fires — the loopback pushed a synthetic event at the
caller's own handle — with `durationPositive: false`, which is the hardcoded
`0.0` in `pump.rs` showing through. Every argument-validation case is
`NO THROW` where Node throws `ERR_HTTP2_PING_LENGTH` / `ERR_INVALID_ARG_TYPE`;
`queue_session_ping` returns `bool_value(false)` for a missing callback rather
than raising.

### What already works

Three things the set confirms Perry gets right. They matter because they are the
regression floor the transport work must not break:

* **Server-side flow control is byte-exact.** Against a raw peer advertising
  `SETTINGS_INITIAL_WINDOW_SIZE=1000`, Perry's hyper server stalls at exactly
  1000 bytes in one DATA frame, releases exactly 5000 more on a stream
  WINDOW_UPDATE, stalls again at the 65535 connection window, then delivers all
  200000 — every number identical to Node.
* **The server answers a real PING with a correct PING|ACK**, payload echoed
  byte-for-byte, including an all-zero payload. `h2` does this below the
  shim, which is why it is the one control frame that works.
* **Streams multiplex and carry bodies** — four concurrent requests on one
  session all complete with the right paths and bodies.

### Where the client is broken beyond the control surface

Three defects that need no raw socket at all — both ends are Perry:

* **`stream.pause()` loses the body.** After a pause, `resume()` delivers 0 of
  300000 bytes (`_wire_flow_control` part 2). `stream.resume` is in fact
  `undefined` on a Perry http2 stream, which is the likely root.
* **`:status` is a string**, not a number, and `session.state.nextStreamID`
  never advances past 1.
* **Trailers do nothing.** `respond(headers, { waitForTrailers: true })` +
  `'wantTrailers'` + `sendTrailers()` produces no `'trailers'` event; the whole
  event order collapses from `["response","data","trailers","end"]` to `[]`.

### One instrumentation trap, and how it was caught

The first Perry pass reported `!! client never emitted connect` on every
`RawServerPeer` fixture, and `!! /a NEVER COMPLETED` for every request in
`_e2e_streams`. Both were **artefacts of the fixtures, not Perry defects**:

* the shared barrier used `target.once(event, …)`, and `once` is not in
  perry-ext-http's http2 session dispatch;
* `fetchPath` used `req.setEncoding("utf8")` and relied on a GET auto-ending,
  and Perry's stream does neither.

Rewritten against the narrowest surface the existing node-suite already proves
works — `on` with a one-shot guard, raw `'data'` Buffers, an explicit `end()` —
all four requests complete and the remaining diff is real. The missing methods
are now printed as their own named lines (`session emitter surface:`,
`stream surface:`), where they are the subject rather than the instrument.

This is worth stating because it is the failure mode the brief warned about in
the other direction: a fixture that fails for the wrong reason is as useless as
one that passes for the wrong reason.

### A second trap: bounded waits have to fit the harness budget

`run_parity_tests.sh` kills a test after `PERRY_RUN_TIMEOUT` seconds, default
**10**. The first harness pass classified `_wire_flow_control` as `crash` not
because it hung, but because its own bounded waits summed past that: part 1's
sleeps plus a 4 s `'response'` bound plus a 6 s `'end'` bound is 12 s when both
time out, which is exactly what Perry does. The bound that was supposed to
produce a readable diff had eaten the budget instead.

Every bound was then sized so that a run in which **every** wait times out still
finishes well inside 10 s — connect 800 ms, small-body events 1–1.5 s,
large-body `'end'` 2 s, callbacks 1.5 s, `server.close()` 300–500 ms, TLS
handshake 800 ms — and `_wire_goaway_send` was restructured to share one
session for the code/opaque variants instead of opening ten. Node's own wall
time for the whole set is 0.5–3.2 s per fixture; Perry's, where it terminates,
is 0.9–5.0 s. `_wire_flow_control` now classifies as `parity_fail` with a
four-line diff.

The general rule for anyone adding to this set: **worst case, not observed
case.** Sum every bound on the longest path and check it against 10 s.

### Protocol errors collapse to one code

Perry's server answers **every** malformed frame with `GOAWAY code=1` and emits
**no session `'error'` event at all**:

| injected frame | Node | Perry |
|---|---|---|
| SETTINGS with a 5-byte payload | `code=6` FRAME_SIZE_ERROR | `code=1` |
| `SETTINGS\|ACK` with a payload | `code=6` | `code=1` |
| PING with a 7-byte payload | `code=6` | `code=1` |
| DATA on stream 0 | `code=1`, `opaque="DATA: stream_id == 0"` | `code=1`, `opaque=""` |
| WINDOW_UPDATE increment 0 | `code=2` INTERNAL_ERROR | `code=1` |
| RST_STREAM on an idle stream | `code=2` | `code=1` |
| HEADERS on stream 0 | `code=2` | `code=1` |
| corrupt HPACK block | `code=9` COMPRESSION_ERROR | `code=1` |
| unknown frame type `0x63` | ignored | ignored ✓ |
| API surface in every failing case | `ERR_HTTP2_ERROR` `errno=-505`, then `close` | `(none)` |

### The exit hang — five fixtures, and what it is NOT

Five fixtures print their complete expected-shaped output and then fail to
terminate; the 25 s watchdog kills them at exactly 25.00 s. They are `crash`
rather than `parity_fail` for that reason alone — stdout was complete before
the kill, which is why the "what diverges" column above is still measured for
them.

This is a **separate defect from the one this lane measures**: after
`session.destroy()`, `server.close()` and `socket.destroy()` have all been
called and every handler has run, the process does not exit.

Three minimal reproductions were compiled and run to narrow it, and **all three
exit cleanly under Perry** (`rc=0`), so the obvious explanations are ruled out:

| probe | shape | Perry |
|---|---|---|
| A | `net.createServer` + `net.connect`, both destroyed | exits |
| B | `net.createServer` raw peer + `http2.connect`, both destroyed | exits |
| C | `http2.createServer` + `http2.connect`, both destroyed | exits |

So it is not `node:net` server teardown, not `http2.connect` against a
non-Perry peer, and not http2 teardown in general. The five that hang —
`_wire_client_ping`, `_wire_settings_ack`, `_wire_goaway_send`,
`_wire_goaway_recv`, `_wire_session_lifecycle` — are all and only the fixtures
that drive a **control-surface method** (`ping` / `settings` / `goaway` /
`close(cb)` / `destroy(err, code)`) against a peer that is **not** an
in-process Perry session, which is where `controls.rs` pushes callbacks into
`session.pending_callbacks` and `H2_PENDING_EVENTS` that no peer scan will ever
resolve. That is a hypothesis, not a root cause: it fits every row but was not
isolated further, because isolating it means reading the transport code the
sibling lane is rewriting.

Handing it over as an open question rather than a diagnosis. Expect these five
to stay `crash` until it is fixed; when it is, they change status, which the
snapshot records.

No `process.exit(0)` was added to paper over it: Node's stdout to a pipe is
asynchronous, so `process.exit` can truncate buffered output, and masking a
real non-termination bug inside a conformance fixture is exactly the kind of
thing this lane exists to stop.

## What the transport lane must implement to pass

Ordered by how much of the fixture set each unlocks. The full version, with
Node's output for every row, is `docs/src/testing/http2-conformance.md`.

1. **Encode and decode SETTINGS, PING, GOAWAY, RST_STREAM and WINDOW_UPDATE on
   the real connection**, deleting the `iter_handle_ids_of::<Http2SessionHandle>`
   peer scan in `controls.rs`. Everything below depends on it. Unlocks
   `_wire_server_settings`, `_wire_server_ping`, `_wire_client_ping`,
   `_wire_goaway_send`, `_wire_goaway_recv`, `_wire_goaway_server`,
   `_session_isolation`.
2. **A SETTINGS ACK state machine**: an outstanding-SETTINGS queue per session;
   `pendingSettingsAck` true from connect; `'localSettings'` and the user
   callback fire on the matching ACK and not before; `session.localSettings`
   updates only then; inbound SETTINGS ACKed on the wire. Unlocks
   `_wire_settings_ack`.
3. **Measure the PING round trip** — timestamp at submit, subtract at ACK, pass
   it as the callback's second argument (`pump.rs` passes `0.0`). Match by the
   8 payload bytes so concurrent pings resolve to the right callback; generate a
   random payload for `ping(cb)`.
4. **Give the settings callback its third argument** (`duration`); `pump.rs`
   calls `call2`.
5. **Distinguish "no opaque data" from "empty opaque data"** on `'goaway'`:
   `undefined`, not a zero-length Buffer.
6. **Session lifecycle**: `close()` writes two GOAWAYs when idle and one while
   draining; `destroy()` writes one and leaves `closed` **false**;
   `destroy(err, code)` puts `code` in the frame.
7. **Argument validation** at Node's codes and messages —
   `ERR_HTTP2_PING_LENGTH`, `ERR_INVALID_ARG_TYPE` for a missing ping callback
   and non-Buffer payload/opaqueData, `ERR_HTTP2_INVALID_SESSION` after
   teardown.
8. **The lastStreamID parity rule** — drop a client `goaway()` with an odd
   `lastStreamID`, silently.
9. **Protocol-error detection and mapping** to the GOAWAY-code table above,
   surfacing `ERR_HTTP2_ERROR` with `errno -505`, and ignoring unknown frame
   types.
10. **Client-side body delivery and `stream.pause()`/`resume()`** — currently
    zero bytes arrive after a pause.
11. **maxConcurrentStreams enforcement in both regimes** — REFUSED_STREAM before
    the peer's ACK, PROTOCOL_ERROR after it.
12. **ALPN on `createSecureServer`**: `h2` only by default (fail the handshake
    otherwise), `http/1.1` when `allowHTTP1` is set, `alpnProtocol === false`
    when the client offers no protocols.
13. **`once` / `off` / `removeListener` on the session handle**
    (`server/http2_server/dispatch.rs` accepts only `on` / `addListener`).
14. **Terminate.** See *The exit hang*.

An implementation note rather than a requirement: `h2::client::handshake` and
hyper's server do not expose SETTINGS/PING/GOAWAY submission, which is why the
shim exists. Whatever replaces it has to own the framing layer, not sit above
one that hides it.

## What this lane did NOT do

* **No transport implementation.** A sibling lane owns it; colliding would be
  worse than useless.
* **No issues filed.** Per the brief. `#10327` (`http2.connect()` is
  cleartext-only and spins up its own tokio runtime per session) is the closest
  standing issue and is what the snapshot entries reference, but it does not
  cover the loopback control surface, the protocol-error collapse, the client
  read-path bug, or the exit hang. Those want their own issues; the coordinator
  should file them.
* **No benchmarking.** Nothing here is a performance claim, and the box is
  shared.
* **Windows: nothing.** macOS: the Node oracle only — the Perry side was never
  built or run there.
* **No two-process fixtures.** The gap harness compares one program's stdout;
  the raw socket is what makes the wire real, and a second process would add
  nothing the socket does not already give.
* **`push_promise`, `respondWithFile`/`respondWithFD`, extended CONNECT
  (RFC 8441) and HPACK decoding are not covered.** Named rather than faked.
* **`maxSessionMemory` is not covered.** Probed and dropped: Node exposes no
  deterministic observable for it (absent from `session.state`; the only
  symptom is a teardown under memory pressure that is not reproducible
  byte-for-byte).

## turnloop gaps found

None. This lane touches no turnloop surface: it adds test fixtures, a TypeScript
frame codec and documentation, and reads Perry's existing `node:http2` path,
which is still hyper and `h2` on tokio and was explicitly out of scope for P5
(`docs/turnloop/p5-report.md`, "What P5 did not do"). The findings above are
Perry-side defects in that pre-turnloop stack, not turnloop shortcomings.

Worth flagging for whoever schedules the remaining lanes: `node:http2` is one
of the surfaces still on tokio per #10354's own inventory, and this fixture set
is the acceptance criterion for whatever moves it.

## For the integrator

* Branch `h2c/http2-conformance`, based on `turnloop/integration` at
  `1db2f76e3`. No version bump, no attribution lines, no `CHANGELOG.md` edit.
* Files added: 14 fixtures under `test-files/`, one helper under
  `test-files/_helpers/`, `docs/src/testing/http2-conformance.md` (linked from
  `docs/src/SUMMARY.md`), `changelog.d/10354-http2-conformance-fixtures.md`,
  and this report.
* `test-parity/gap_snapshot.json` and `test-parity/known_failures.json` each
  gain the same 14 entries (`issue: 10327`, `category: module-inventory`, one
  sentence of measured evidence per entry). **That is the deliverable's
  ratchet**: when the transport lands, these flip to passing and the gap gate
  fails until the snapshot diff is committed, so the fixture set cannot
  silently stop being the acceptance criterion.
* Both diffs are **purely additive** (98 and 84 insertions, zero deletions).
  `gap_snapshot.py update` re-encodes three pre-existing `\u2014` escapes as
  literal em-dashes; those three lines were restored so the diff carries
  nothing but the new entries.
* The snapshot entries came from `scripts/gap_snapshot.py update` against a
  `--filter test_gap_http2` harness report, which by design touches only the
  tests present in that report. Regenerating the whole file from a full sweep
  is not required and would be a larger diff for no signal.
* Local gates run green on this branch: `gap_snapshot.py --self-test`,
  `gap_snapshot.py check`, `parity_known_failures.py --self-test`,
  `parity_known_failures.py --audit`, `check_file_size.sh`,
  `check_node_version_consistency.py`. `parity_known_failures.py --audit` is
  the one that would otherwise have failed `lint`: it requires every
  `gap_snapshot.json` failure to carry a matching `known_failures.json` entry
  for the running platform, and a snapshot update alone does not write one.
* No Rust changed, so `cargo fmt`, clippy, the addr-class ratchet and
  `gc_runtime_root_holders.py` are untouched by this branch.

# turnloop HTTP/2 — `node:http2` on the loop

Branch `h2b`, continuing `turnloop/http2` (`917e999dbd`), which is
`turnloop/integration` at **`1db2f76e34`** plus two documentation commits.

Read on the shared Linux box (`perrybuilder`, EPYC 9354P), against the pinned
gap oracle Node **26.5.1**. **Nothing here was run on Windows or macOS, and
nothing was benchmarked** — the box was under load 13–20 throughout, which is
exactly the condition under which a timing number is worthless.

---

## What this continues

The previous lane did the design and stopped rather than half-landing it. It
left ~1,250 lines of transport committed but deliberately **not wired into the
module tree**, a design report, and a runnable probe proving three turnloop
contract gaps. That work is the foundation of this branch and most of its
shape survives; what changed is recorded under "What the previous lane's code
got wrong".

**This branch lands the migration.** `http2.createServer`,
`http2.createSecureServer` and cleartext `http2.connect` are on turnloop.

---

## The finding that shaped the job

Perry's HTTP/2 control surface was a **loopback simulation**.
`queue_session_settings` and `queue_session_goaway` enumerated
`Http2SessionHandle`s with `iter_handle_ids_of`, picked the ones whose
`session_type` was the opposite of the caller's, and pushed a synthetic event
into their queues; `queue_session_ping` fired its own callback straight back
without consulting anyone. **No frame was ever encoded.**

That is why `test-parity/node-suite/http2/` passes: every case in it is a Perry
client talking to a Perry server in one process. So `settings()`, `goaway()`
and `ping()` were not ported here — they were **implemented**, and the existing
tests constrained nothing, because both ends were Perry.

---

## What moved, and what did not

| surface | before | after |
|---|---|---|
| `http2.createServer().listen()` (h2c) | hyper + `h2`, a `spawn_blocking` accept loop with its own `current_thread` runtime | **turnloop** + `turnloop_http::http2` |
| `http2.createSecureServer().listen()` | ditto, plus `tokio_rustls::TlsAcceptor` | **turnloop** + `perry_ext_net::turnloop_tls_io` (unbuffered rustls), ALPN `h2` / `http/1.1` |
| ALPN → `http/1.1` with `allowHTTP1: true` | `hyper_util`'s `auto::Builder` | **turnloop**: the connection is handed to P5's HTTP/1.1 server by moving one table entry |
| `http2.connect('http://…')` | a private `current_thread` tokio runtime **per session**, plus **another per request** | **turnloop**, on the agent's own loop |
| `session.settings()` / `.goaway()` / `.ping()` on a turnloop session | a loopback event, no frame | **real frames**, acknowledged by the peer |
| `stream.id` | a process-global odd counter | the **RFC 9113 stream id** of its own connection |
| `stream.close([code])` | a local flag | **RST_STREAM** on the wire |
| `http2.connect('https://…')` | `h2` + a private runtime | unchanged — see "What this did not do" |
| a server on a `worker_threads` agent, or in a cluster worker | hyper + `h2` | unchanged, for P5's reasons |

This is a narrowing, not a removal, and the declining rows are real and still
exercised.

---

## Architecture

```
NET_ACCEPT ─► turnloop_h2::conn::on_accept ─► [TlsSession::server, ALPN] ─┐
                                        │ h2 │ http/1.1 → turnloop_serve  │
NET_DATA   ─► on_data ─► [TLS decrypt] ─► prescan ─► http2::Connection ───┤
                                                     │ Event::Headers/Data│
                                                     ▼                    │
                              IncomingMessage + ServerResponse handles     │
                                                     │                    │
                                       queue (this thread, no channel)     │
                                                     ▼                    │
              js_node_http_server_process_pending ─► the JS handler ───────┘
                                                     │ res.end()
                                                     ▼
                  http2::Connection::send_headers/send_data ─► [TLS] ─► write
```

P5's three rules hold unchanged — the sink runs no JS, no JS value or heap
pointer reaches the driver, and the dispatch tick is the one hyper's `mpsc` was
drained on — plus one that is HTTP/2's own:

**A stream error ends one stream; a connection error ends all of them, once
each.** `Event::Reset` finds one record, queues one event on one handle,
releases that stream's capacity and drops the record; nothing else is touched.
A `send_headers` the core rejects (a handler that emitted a malformed block) is
answered with `core.reset(id, INTERNAL_ERROR)` — a *stream* error — rather than
being allowed to become a connection error. Only `Connection::receive`
returning `Err` ends everything, and the GOAWAY it already queued is flushed
**first**: a `receive` error has side effects, and returning before flushing
sends the peer nothing at all.

---

## The pre-scan: three things `Connection` will not tell a host

Every frame is decoded twice — once by `conn::peek_frame`, once by the core.
The second decode is authoritative; the first exists because three facts a
`node:http2` session has to surface never leave `Connection`:

* **a SETTINGS acknowledgement** is consumed with `event: None`, so
  `session.settings(obj, cb)` has nothing to fire its callback on and
  `session.pendingSettingsAck` would never go false;
* **GOAWAY's opaque data** — `Event::Goaway` carries `last_stream` and `code`
  only, and Node's `'goaway'` listener takes a third argument;
* **the peer's SETTINGS values** — `Event::Settings` is a unit variant, so
  `session.remoteSettings` would sit at its defaults forever.

The pre-scan also **withholds** one frame class. `Connection` tracks exactly one
outstanding SETTINGS (its own, from the constructor) and answers a second
acknowledgement with `protocol("unsolicited SETTINGS ack")` — a **connection**
error. A `session.settings()` frame is therefore acknowledged by the peer into a
core that would kill the session for it, so this module counts the SETTINGS
frames it sent out of band and eats exactly that many acks before the core sees
them. Both halves are turnloop gaps (below); the count is the workaround.

---

## Flow control

`turnloop_http::http2` never reopens a receive window on its own: a stream's
`unreleased` accumulates every DATA byte and only `release_capacity` turns it
back into WINDOW_UPDATE frames. So the host is the policy, and Perry's is three
rules — the previous lane's design, kept:

1. **Release on consume.** A DATA payload is copied into the stream's own buffer
   inside the completion sink; by the time the event returns there is no
   downstream consumer left to wait for, because Perry's HTTP server buffers a
   request body before dispatching it on both transports. Withholding window
   would idle the peer for nothing.
2. **Bounded by `maxSessionMemory`** (Node's default, 10 MB). Once a connection
   holds that much *undispatched* body the release is withheld per stream and
   the peer stalls, which is what a flow-control window is for. Node signals the
   same condition by destroying the session with `ENHANCE_YOUR_CALM`; stalling
   first is strictly gentler.
3. **A terminated stream still releases.** Not optional: `add_stream` reuses a
   closed stream's slot only when its `unreleased` is zero, and `reset` does not
   zero it. See gap 1.

The write side needs no policy: `send_data` returns what it accepted and zero on
a stall, the remainder stays in the stream's `outbox`, and `Event::WindowUpdate`
retries it.

One cost, recorded because it is a real difference: a stream cannot exert
*per-stream* backpressure, because Perry has no per-stream consumer to be slow.
`stream.pause()` does not close that stream's window. Node's does.

---

## Two defects in already-landed code, found by wiring this

Neither is HTTP/2's; both were reached through it and both are fixed here,
because the migration does not work without them.

### `turnloop_serve::conn::on_closed` never dropped the socket's TLS layer

`perry_ext_net::turnloop_tls_io` keys its layer table by connection id, and the
**only** caller of `forget` was perry-ext-net's own `emit_close_once` — which is
subsystem 0's `net.Socket` path, not perry-ext-http's. So every turnloop HTTPS
connection left a `Layer` (a rustls session plus its buffers) behind for the
life of the process, and once `free_handle_id` handed the id back and a later
accept drew it, `install_server_session` answered *"socket is already TLS"* and
the connection was closed before a byte was read.

Found through the `allowHTTP1` handoff, which is the shape that reaches it
reliably: an ALPN `http/1.1` connection is handed to the HTTP/1.1 server and
closes there, and the **next** TLS connection to that server then got EOF —
every time, while a fresh server passed h2spec 147/147 twice. Measured, six
sequential curls against one `createSecureServer({ allowHTTP1: true })`:

```
                     before              after
1 h2   :  secure:/a:2.0|v=2     secure:/a:2.0|v=2
2 h2   :  secure:/b:2.0|v=2     secure:/b:2.0|v=2
3 h1.1 :  secure:/c:1.1|v=1.1   secure:/c:1.1|v=1.1
4 h2   :            |v=0        secure:/d:2.0|v=2
5 h1.1 :  secure:/e:1.1|v=1.1   secure:/e:1.1|v=1.1
6 h2   :            |v=0        secure:/f:2.0|v=2
```

The leak half is unconditional and pre-existing: it is one `Layer` per turnloop
HTTPS connection, on `main`'s integration branch, today. The *failure* half
needs the freed id to be drawn again, which five sequential `https.request`
calls against a plain `https.createServer` did **not** reproduce on the base
arm (5/5 answered) — the ALPN handoff is what makes the recycle happen promptly.
So this is reported as a defect found here rather than as an HTTP/2 regression,
and the fix is in `turnloop_serve`, where it belongs.

### The plaintext decrypted alongside the handshake's last flight was dropped

`turnloop_h2::on_data` ran `finish_handshake` and, for an ALPN handoff, returned
without feeding the plaintext it had just decrypted. A TLS 1.3 client sends
`Finished` and its first request back to back, so that plaintext *is* the
request that decided the handoff. Measured: `curl --http1.1` against
`createSecureServer({ allowHTTP1: true })` hung, every time. The handoff now
takes those bytes with the connection.

A third, in the pump rather than the transport: a request decoded on an adopted
`http/1.1` connection queues into P5's queue **keyed by an `Http2SecureServer`
handle**, and `js_node_http_server_process_pending` drains that queue only for
`HttpServer` handles. The request was decoded, queued, and never dispatched.
`try_recv_pending_h2_nonblocking` now drains both.

## What the previous lane's code got wrong

Reviewed rather than adopted. Six defects, each of which would have shipped:

1. **`skip_default_response: false` on every dispatched request.** The hyper path
   sets it to `has_stream_listener`, because a `'stream'` listener answers the
   request itself. Two responses would have gone out on one stream — which is
   `STREAM_CLOSED` from inside the core, i.e. a dead connection on the second
   request of any server written against the `'stream'` event. Every
   `node-suite/http2/plaintext/*` fixture is that shape.
2. **A bodyless response never reached the wire.** `h2_send_response` returned
   after `finish_stream` without flushing, so the HEADERS frame of a 204, a 304
   or a HEAD response sat in `core.output()` and the client hung.
3. **`finish_stream` retired a server stream whose peer had not finished.**
   The record its inbound DATA has to land on was dropped, and the bytes were
   released as an orphan and thrown away.
4. **Ownership was released on a failed write.** `with_owned` removed the id
   from `owned_ids` whenever `f` set `destroyed`, so the socket's remaining
   completions — including its terminal `NET_CLOSED` — were routed to P5's
   HTTP/1.1 sink, which has never heard of the id. The id then never went back
   to the shared band: one leaked handle id per connection that failed a write
   (the #6441 exhaustion class). Ownership now ends only at `NET_CLOSED`.
5. **Two hand-encoded control frames were written straight to the socket**,
   ahead of whatever the core had already queued in `core.output()`. Two frame
   streams interleaved is a protocol error on the *peer's* side, so nothing on
   this side would ever have reported it. `conn::write_raw` flushes the core
   first.
6. **`mod client;` named a file that was never written**, and
   `turnloop_serve::adopt_alpn_http1` — the whole reason the module shares
   subsystem slot 1 — did not exist. Both are here.

A seventh was found not by reading but by the gap suite, and is written up under
"The one regression this found": the three connection-level controls were sent
immediately even when the transport had not connected yet.

An eighth is not a defect but is worth naming: the module referenced twelve
`crate::server::http2_server::*` glue functions and three `crate::server::*`
entry points that did not exist, and named a `client.rs` that was never written.
None of it had ever been compiled.

## Unit tests

Eighteen, in three places, chosen for the things h2spec cannot reach.
`RUST_TEST_THREADS=1 cargo test --release -p perry-ext-http --lib`:

```
work: 127 passed; 1 failed          base: 109 passed; 1 failed
```

The one failure is `tls_client::tests::needs_custom_client_logic`, identical in
both arms and in a file this branch does not touch — pre-existing on the
integration branch. The eighteen added are:

* `turnloop_h2::control`'s `clamp_to_core` — that a setting may be **lowered**
  and that a **raise** is clamped back to what the core honours, plus that a
  server never advertises ENABLE_PUSH and that the payload is always a whole
  number of 6-byte entries. A raise that escaped the clamp is a connection error
  on the peer's side minutes later, which is exactly the failure a unit test
  should catch instead of a soak.
* `turnloop_h2::conn`'s `prescan` — that the core's **own** SETTINGS
  acknowledgement is never stolen (stealing it leaves `settings_awaiting_ack`
  set until the SETTINGS deadline kills the session, somewhere else entirely),
  that exactly the owed acks are withheld and the next belongs to the core, that
  a malformed ack is left for the core to reject, that GOAWAY opaque data and
  peer SETTINGS values are captured and that stale opaque data is cleared, and
  that nothing is peeked before the client preface or on a partial frame.
  h2spec never makes Perry send a second SETTINGS, so **none** of the
  ack-withholding is exercised by the conformance run at all.

* `turnloop_h2::tests`, which pins the response-header translation against
the core itself: the block `response_headers` produces is fed to a real
`Connection::send_headers` and must be accepted, rather than checked against
this module's own idea of the rules.

---

## Validation

### h2spec against Perry's own server

The checksum-pinned h2spec turnloop's CI uses (commit
`70ac2294010887f48b18e2d64f5cccd48421fad1`, sha256 verified against
`scripts/ci/tools.json`, Go 1.25.1), `--strict`, against
`http2.createServer((req, res) => { res.writeHead(200, …); res.end(…) })` —
Perry's own binding, not turnloop's example server.

```
147 tests, 147 passed, 0 skipped, 0 failed
```

All three suites ran: *Generic tests for HTTP/2 server*, *Hypertext Transfer
Protocol Version 2 (HTTP/2)* (RFC 9113 §3–§8) and *HPACK: Header Compression
for HTTP/2*. Finished in 0.48 s.

**The same run against the base arm — hyper + `h2`, built from this branch's own
base commit in its own tree — scores 146.** The one it fails is RFC 9113 §3.5-2:

```
  3.5. HTTP/2 Connection Preface
      × 2: Sends invalid connection preface
        -> The endpoint MUST terminate the TCP connection.
           Expected: GOAWAY Frame (Error Code: PROTOCOL_ERROR)
                     Connection closed
             Actual: Error: unexpected EOF
```

hyper drops the TCP connection without sending the GOAWAY. That is exactly the
third sharp edge written into `conn.rs`'s module header — a `receive` that
errors has *already queued* the GOAWAY carrying the error code, so a host that
returns before flushing sends the peer nothing at all. The turnloop path
flushes first and then fails the connection, which is why it is 147.

So the binding is not merely no worse than hyper on the protocol; it is one test
better, and the difference is a rule that had to be found by reading
`receive`'s implementation rather than its signature.

### h2spec over TLS

The same 147 against `http2.createSecureServer({ key, cert, allowHTTP1: true })`
with `--tls --insecure --strict`:

```
147 tests, 147 passed, 0 skipped, 0 failed     (twice, on a fresh server)
147 tests, 147 passed, 0 skipped, 0 failed     (after six mixed-ALPN curls)
```

For calibration, **the same h2spec TLS run against Node 26.5.1's own
`http2.createSecureServer` scores 136/147** on this box, so the TLS arm is not
a clean bar and the comparison that means something is the cleartext one above.

ALPN itself is proven by an external client rather than by Perry's own, because
`http2.connect('https://…')` is still the pre-existing cleartext-to-port-80
defect:

```
curl -k --http2   https://…/alpha  →  secure:/alpha:2.0   [http_version=2]
curl -k --http1.1 https://…/beta   →  secure:/beta:1.1    [http_version=1.1]
```

The second is the `allowHTTP1` handoff end to end: rustls negotiated
`http/1.1`, the connection moved from the HTTP/2 table to P5's by one table
entry — same id, same TLS layer, same outstanding multishot read — and the
HTTP/1.1 state machine answered it.


### Liveness — the counters, and the thread count

A green suite proves nothing if the code under test never ran, so both arms
were measured on the same program, built from source in their own trees.

**`h2serve.ts`** — `http2.createServer((req, res) => res.end(…))`, one process,
counted while h2spec drove it:

| | base (hyper + `h2`) | this branch (turnloop) |
|---|---|---|
| OS threads in the server process | **2** | **1** |

The second thread is the `spawn_blocking` accept loop and its private
`current_thread` runtime. It is gone.

**`h2smoke.ts`** — a Perry `http2.connect` client and a Perry
`http2.createServer` in one process, three multiplexed requests answered from a
`'stream'` listener. Identical stdout in both arms (`body:/a|body:/b|body:/c`),
and the counters say which transport produced it:

```
base:   [perry-loop] driver=turnloop parked=0 agent=0
        [perry-loop-waits] arm=turnloop turnloop_waits=0 tokio_ticks=0 … fast_drives=6 fast_drive_ns=731455

branch: [perry-loop] driver=turnloop turns=7 os_waits=2 zero_event_waits=0 native_ticks=0
                     turn_errors=0 completions=30 timer_arms=0 timer_expiries=0 agent=0
        [perry-loop-waits] arm=turnloop turnloop_waits=2 tokio_ticks=0 … fast_drives=0
```

Read them together rather than one at a time. `tokio_ticks=0` is **not** the
discriminator here — it is zero in both arms, because the tokio work happened
inside `spawn_blocking`'s own runtime rather than through the wait-driver's
tick path. The two that do discriminate are:

* **`completions=30` vs no completion line at all.** The base arm never built
  an agent loop for this program (`parked=0` is the loop-less shape); every
  HTTP/2 byte moved on a thread the loop never saw. The branch moved 30
  completions through it.
* **`fast_drives=6` vs `fast_drives=0`.** `fast_drives` counts turns given to
  the tokio wait-driver because something native was in flight. Six on the base
  arm; none on the branch, because nothing native is in flight any more.

**`h2tls.ts`** — `http2.createSecureServer`, driven by curl and h2spec over
TLS: **1 OS thread**, before and after.

What was *not* measured: any wall-clock or throughput number. The box carried
load 9–22 from other lanes for the whole session. Thread counts and completion
counters are structural and load-independent; a latency figure taken there
would not be.


### The gap suite

`PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh --shard N/6` on both arms —
CI's own fast-mode configuration — each arm built from source in its own tree
(`/root/claude-h2b/base` at the branch's base commit, `/root/claude-h2b/work`
at its head), each with `npm ci` done, against Node 26.5.1.

Compared **per test**, by merging the six shard journals per arm:

```
base tests: 819   work tests: 821
  base: parity_fail=9  pass=810
  work: parity_fail=9  pass=811  node_fail=1

common tests: 819
STATUS CHANGES: 1
  test_gap_9536_fetch_url_error: pass -> node_fail

work-only tests (2):
  test_gap_turnloop_http2_control: pass
  test_gap_turnloop_http2_server: pass
```

The one change is a `node_fail` — **Node** exited non-zero, which no Perry
change can cause. `test_gap_9536_fetch_url_error` resolves
`https://example.invalid/` and asserts the `ENOTFOUND` shape, so it depends on
the box's DNS resolver, and twelve concurrent shards were hammering it. Node
answers it correctly 3/3 when re-run directly, and the test re-runs `PASS` on
the work arm through the harness:

```
[1/1] (00:00:10) test_gap_9536_fetch_url_error … PASS
Parity Rate: 100.0%
```

So the attributable count is **zero**.

The nine non-passing tests are the **same nine in both arms**:

```
test_gap_2159_defineproperty_class_prototype   test_gap_json_lazy_defineproperty_index
test_gap_2514_settracesigint                   test_gap_perfhooks_3088_3008_3010_3011
test_gap_2899_2779_2777_static_helpers         test_gap_prop_plan_cache_invalidation
test_gap_disposablestack_2875                  test_gap_v8_2_3680plus
test_gap_iterator_prototype_next_patch
```

Three of them — `2899_2779_2777_static_helpers`, `disposablestack_2875`,
`iterator_prototype_next_patch` — are the ones the brief names as already red
against the committed snapshot on the base commit, and the base arm reproduced
exactly those three as snapshot "regressions", which is what says the baseline
is behaving as documented rather than as a coincidence.

### The one regression this found, and what it was

The **first** pair of sweeps was not zero. It reported exactly one status
change:

```
test_gap_gc_http2_pending_event_callback_rooting: pass -> crash
```

Reproduced immediately outside the harness — a hang, not a signal
(`perry-rc=124` under a 30 s timeout), preceded by `client error: undefined`:

```
--- node ---            --- perry, before the fix ---
settings cb fired       client error: undefined
ping cb fired           (hangs)
close cb fired
```

That test calls `client.settings(…)` on the **first tick after
`http2.connect()`**, before the TCP connect has completed. `send_settings` wrote
the SETTINGS frame straight to the socket, so it reached the peer **ahead of the
client connection preface** — a connection error on the server's side, and a
client whose callback could then never fire. `session.request()` was already
queued for exactly this reason (`H2Conn::queued_opens`); the three
connection-level controls were not.

They are now, in `H2Conn::pending_controls`, drained by
`control::drain_pending` from `client_transport_ready` **before** the queued
stream opens — so a control frame the caller issued first does not end up behind
a HEADERS it preceded. With that in place the test is byte-identical to Node
again and the per-test comparison above is the second pair of sweeps, at zero.


### The `node:http2` granular parity suite

`test-parity/node-suite/http2` — 60 fixtures, run on both arms with a
per-test-outcome variant of `scripts/node_suite_run.py` so the comparison is
per fixture rather than per count:

```
$ diff <(grep ^RESULT ns.base.txt) <(grep ^RESULT ns.work.txt)
diff-rc=0

base: http2  29  60  48.3%   diff=24 perry_err=7
work: http2  29  60  48.3%   diff=24 perry_err=7
```

**Byte-identical outcome sets.** Every one of the 60 fixtures lands in the same
bucket on both arms.

Two things about that number have to be said plainly rather than left to be
inferred.

**First, this corpus is no longer evidence for the control surface.** Every
network case in it is a Perry client talking to a Perry server in one process,
which is exactly the configuration the loopback simulation was built for. It
passed `settings`/`goaway`/`ping` before any frame existed, and it passes them
now that they are real; it cannot distinguish the two. The new gap tests and
h2spec are what distinguish them.

**Second, both arms sit below the committed floor of 32/60**
(`test-parity/node_suite_baseline.json`), and the seven `perry_err` — which are
30-second timeouts in the runner, not compile failures — are the same seven
fixtures in both arms. Being identical in both arms makes them not this
branch's, but it also means this run is not a clean check against the floor,
and the floor was captured on a quiet machine rather than one carrying load
15–25 from four other lanes.

They are not load, though, and they are not the integration branch's either.
`plaintext/response-body.ts` — a server that answers `stream.respond` +
`stream.end("hello h2")` and a client that reads it, i.e. the shape this branch
passes in its own probes — **hangs for 30 s on all three of**: the base arm, the
work arm, and a **pristine `main` checkout** (`/root/projects/perry/perry` at
`0c0e850e9` = v0.5.1573, `git status` clean), each run alone on an otherwise
idle tree:

```
base  rc=124      work  rc=124      main  rc=124      node  rc=0  ("hello h2")
```

So those seven fixtures are a pre-existing `main` regression against the
recorded floor, visible here only because this lane happened to run the corpus.
Caveat on the third figure: that binary is another session's build of that clean
tree, which makes it a characterisation and not a bisect endpoint.




### GC stress

Five subjects, four seeds each, under
`PERRY_GC_SCHEDULE_SEED=<n> PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`. **Every run's
stdout is byte-identical to its unstressed run**, and no from-space quarantine
fault, SIGSEGV or panic was raised in any of the twenty.

| subject | copying minors | objects moved | loop polls | exit |
|---|---|---|---|---|
| `test_gap_gc_http2_pending_event_callback_rooting` | 60,020 | 492,038 | 60,000 | 0 |
| `test_gap_turnloop_http2_server` | 63 | 8,957 | 0 | 70 |
| `test_gap_turnloop_http2_control` | 39 | 8,600 | 0 | 70 |
| `h2smoke` (client + server, `'stream'` path) | 30 | 8,634 | 0 | 70 |
| `h2stream` (streamed response + trailers) | 25 | 8,566 | 0 | 70 |

(Identical figures across all four seeds per subject, which is what a
single-threaded replay should give.)

**The exit 70 rows are the instrument telling the truth about itself**, and it
is worth not glossing: `loop_polls=0` means no back-edge poll was reached, so
the run "exercised nothing worth trusting" *as a loop-body test*. Those four
programs are event-driven and have no allocating loop to poll in. What they did
exercise is the collection point this module actually has — the window between
the completion sink queueing an event and the main-thread pump firing it — and
they ran 25–63 **copying** minors moving 8.5k+ objects each inside it. The
first row is the one that satisfies the instrument on its own terms, and it is
also the fixture written for precisely this hazard (`session.settings(cb)` /
`.ping(cb)` / `.close(cb)` callbacks parked as raw NaN-box bits across a pump
tick): 60,000 polls, 492k objects moved, identical output, exit 0.

No root scanner was added, and that is a claim rather than an omission: a
connection holds request/response bytes as owned `Vec<u8>`s and the *handle ids*
of the JS objects it produced. No JS value and no heap pointer reaches the
driver. The `IncomingMessage` / `ServerResponse` / `Http2SessionHandle` /
`Http2StreamHandle` records are scanned by the existing
`scan_http_server_roots`, the queued event callbacks by
`scan_h2_pending_event_roots`, and `Http2SessionHandle::pending_callbacks` —
where a `ping`/`settings` callback now waits out a real network round trip
instead of one tick — was already visited there. `scripts/gc_runtime_root_holders.py`
reports **OK**, with the same 1,489 holder declarations as before this change.

### The streaming response path

`writeHead` + two `write`s + `addTrailers` + `end` — `h2_begin_stream` /
`h2_send_body` / `h2_finish_body` rather than the single-shot
`h2_send_response`:

```
             node          this branch     base arm
write ret    true          true            true
status       200           200             200
body         alpha|beta|gamma  (same)      (same)
trailer      x-sum 3       x-sum none      x-sum none
```

The trailer block **is** encoded and sent — the server-side path builds it and
`send_headers(trailers, END_STREAM)` puts it on the wire. What is missing is the
**client**-side `'trailers'` event: `Http2PendingEvent` has no variant for a
trailer block and `queue_turnloop_client_body` drops the argument. That is
pre-existing — the base arm answers `none` too — and it belongs to the Node
surface the sibling fixture lane owns, so it is reported rather than fixed here.

### Reproducing

Everything above runs from three files on the box, against the shipped binary
(`/root/claude-h2b/work/target/release/perry`, archives from the same
`cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static
-p perry-ext-http -p perry-ext-net`):

```
bash /root/h2spec.sh    work 49011   # h2c:  147/147, threads=1
bash /root/h2b-tls.sh   work 49012   # TLS:  147/147 + both curl ALPN paths
bash /root/h2b-gapshard.sh work 6    # the gap suite, CI's fast mode, 6 shards
python3 /root/h2b-gapdiff.py         # the per-test A/B of the two arms
bash /root/h2b-gcstress.sh work      # 5 subjects x 4 seeds
```

The last h2spec pair in this report was run on the binary this branch pushes,
after everything else, so the headline number is the shipped artefact's:

```
h2c  : 147 tests, 147 passed, 0 skipped, 0 failed   threads=1
TLS  : 147 tests, 147 passed, 0 skipped, 0 failed   threads=1
       curl --http2   -> secure:/alpha:2.0  [http_version=2]
       curl --http1.1 -> secure:/beta:1.1   [http_version=1.1]
```

---

## turnloop gaps found

Reported here in the shape P4's, P5's and the previous lane's were; the
coordinator files them. Gaps 1–6 are the previous lane's, all confirmed by
building against them; 7–12 are this lane's.

Every one of 1–6 is proven by the committed, self-contained probe
`docs/turnloop/http2-contract-probe.rs` (it depends only on `turnloop-http`);
7–12 were each hit while wiring the binding, and the evidence for each is named
inline.

### 1. A stream reset with unreleased DATA burns its table slot permanently

`Connection::add_stream` reuses a closed stream's slot only when its
`unreleased` is zero, and `reset()` sets `local_end`/`remote_end` without
zeroing it. When the table fills, `add_stream` returns `REFUSED_STREAM` **from
inside `receive`**, which sets `failed` and emits a GOAWAY: the whole session
dies, and the peer is told PROTOCOL_ERROR, because `receive`'s error map has no
case for `REFUSED_STREAM`. Measured: a 2-slot table accepted **2** of 6 streams
without a release and **6** with.

**Worked around** — `stream::terminate` releases the remainder before dropping
the record (rule 3 of the flow-control policy). A host that reasons from
`release_capacity`'s own contract ("return window for data you have consumed")
will not do this, because a stream it just reset is precisely the data it did
*not* consume.

### 2. `shutdown()` cannot express `goaway(code, lastStreamID, opaqueData)`

`Connection::shutdown` always sends GOAWAY with code 0 and its own
`last_remote`, and takes no opaque data. Node's `session.goaway()` sets all
three. **Worked around** by encoding the frame with the crate's public
`encode_frame` and writing it through `conn::write_raw`, which flushes
`core.output()` first — without that the host's frame overtakes whatever the
core had queued, and two interleaved frame streams are a protocol error on the
*peer's* side, so nothing on this side would ever report it.

### 3. A stream opened after a graceful GOAWAY is a connection error

After `shutdown()`, `receive`'s HEADERS arm rejects a new stream with
`protocol("invalid new stream")` because `draining` is set — a **connection**
error, so the session dies and a second GOAWAY goes out. Node answers
`RST_STREAM(REFUSED_STREAM)` and keeps the session, which RFC 9113 §6.8 asks
for because the race is unavoidable: the peer cannot have seen the GOAWAY yet.

**Not worked around.** The decision is inside `receive_inner` and a host cannot
reach it. This is a live divergence on every graceful close with traffic in
flight.

### 4. `Step`'s two independent zero cases are undocumented

`consumed == 0, event == None` (a partial preface or frame: **stop**) and
`consumed > 0, event == None` (the preface, a SETTINGS ack, PRIORITY, an
unknown frame type: **keep going**) are both normal. A host that loops on "an
event came back" stalls at the *preface*, before a single frame is read.
The correct condition is `consumed > 0 || event.is_some()`, which
`asynchronous::mod.rs`'s own driver uses and nothing else states.

### 5. `Event::Headers` does not distinguish a head, a trailer block and a 1xx

All three arrive as `Event::Headers`, so every host duplicates the
`received_head` state `Connection` already keeps — and `finish_headers` has
just used it to enforce the distinction. A `kind: HeadersKind`, or separate
`Event::Trailers` / `Event::Informational`, would remove it.

### 6. No getter for a stream's `unreleased`

Gap 1's fix requires the host to mirror the core's counter byte for byte,
because `release_capacity(id, n)` errors when `n > unreleased`. The mirror is
exact only because padding is auto-released inside `receive` — so the host's
view increments by the *unpadded* `bytes.len()` — which is true by arithmetic
rather than by contract.

### 7. A second SETTINGS frame kills the connection on its acknowledgement

There is no `Connection::settings()`. Worse, a host that encodes one itself has
its peer's acknowledgement answered with `protocol("unsolicited SETTINGS ack")`
— a **connection** error — because `settings_awaiting_ack` is a single `bool`
set once in `new` and cleared by the first ack. So `session.settings()` cannot
be implemented at all without host-side interference.

**Worked around** by counting the SETTINGS frames this module writes out of
band (`H2Conn::owed_settings_acks`) and eating exactly that many acks in
`conn::prescan` before the core can see them. Guarded by `core_settings_acked`
so the handshake's own ack is never stolen. A `Connection::settings(&[(u16,
u32)])` that queued the frame and incremented its own counter would remove both
this and gap 8.

### 8. A SETTINGS acknowledgement produces no event

`receive` consumes it with `consumed > 0, event: None`. Node's
`session.settings(obj, cb)` fires its callback on the ack, emits
`'localSettings'` there, and flips `session.pendingSettingsAck` — none of which
a host can see. **Worked around** by the same pre-scan.

### 9. `Event::Settings` is a unit variant

`receive_inner` decodes the peer's identifiers, applies the ones the core cares
about, and throws the rest away. `session.remoteSettings` therefore cannot be
populated from the event. **Worked around** by decoding the payload a second
time in `conn::decode_settings`. An `Event::Settings { .. }` carrying the
decoded pairs, or a `peer_settings()` getter, would remove it.

### 10. `Event::Goaway` drops the opaque data

RFC 9113 §6.8 makes everything past the 8-byte header "Additional Debug Data",
and Node's `'goaway'` listener receives it as the third argument.
`Event::Goaway` carries `last_stream` and `code` only. **Worked around** by the
pre-scan.

### 11. `Connection::new` always advertises three identifiers, and Node advertises none

The constructor unconditionally sends MAX_CONCURRENT_STREAMS, MAX_FRAME_SIZE
and MAX_HEADER_LIST_SIZE. **Node's HTTP/2 server sends an empty SETTINGS
frame** — measured on the pinned oracle by speaking the preface by hand over a
raw socket:

```
SETTINGS flags=0 []
SETTINGS flags=1 []
```

So a Perry server's peer reads `session.remoteSettings.maxConcurrentStreams`
as 100 where Node's peer reads 4294967295 (the protocol's "unlimited"), and a
Perry client's peer reads 128 where Node's reads 4294967295.

**Deliberately not worked around.** Advertising "unlimited" while
`Limits::streams` refuses beyond 100 turns an ordinary burst into gap 1's
connection error, which is strictly worse than the divergence. A
`Limits::advertise: &[u16]`, or letting the host pass its own initial SETTINGS
payload, would close it.

### 12. No setter for any `Limits` after `new`

Every value in a SETTINGS frame is a promise about state inside `Connection`
that the host cannot change afterwards — the HPACK decoder's table size, the
per-stream `recv_window` (**hard-coded to 65535**), `Limits::streams` (which is
both the advertisement and the size of the stream table), `limits.frame_size`
(what `decode_frame` will accept) and the decoder's header-list limit. So
`session.settings()` here can only ever *lower* a setting — safe, because the
core still accepts anything within the larger original bound — and must clamp a
raise back (`control::clamp_to_core`, with the two directions pinned in unit
tests).

The same limitation is why **`session.setLocalWindowSize()` still does not
reach the wire**: a WINDOW_UPDATE the core did not issue would let the peer
send more DATA than `recv_window` allows, and the core would answer its own
peer with `FLOW_CONTROL_ERROR`.


---

## What this did not do

Named precisely rather than left implied.

* **`http2.connect('https://…')` is unchanged** — still `h2` on a private
  `current_thread` tokio runtime, and still opening a **cleartext** socket to
  port 80 (the previous lane's finding #1, unfixed). It needs a public TLS
  *client* installer on a turnloop socket: `perry_ext_net::turnloop_tls_io` has
  `install_server_session` (`pub`) but only `begin_client_upgrade`
  (`pub(crate)`, taking perry-ext-net's own `TlsClientConfigData` and settling a
  `JsNativeAsyncCompletion` of its own). Adding `install_client_session(id,
  servername, verify, alpn)` is the unblocking change, and it belongs in
  perry-ext-net rather than here.
* **A `worker_threads` agent and a cluster worker keep hyper + `h2`**, for
  P5's reasons (no loop; `SO_REUSEPORT` / fd passing need the `std` listener).
  Those paths are reachable and exercised, so the edge cannot be deleted.
* **Server push** is untouched. Perry does not implement `createPushResponse`
  and `turnloop_http::http2` rejects PUSH_PROMISE outright
  (`protocol("server push disabled")`), so both agree.
* **`stream.sendTrailers()`, `.priority()`, `.setTimeout()`** on the raw
  `Http2Stream` object are still `=> self_ref` no-ops. Trailers *do* reach the
  wire through the compat path (`res.addTrailers()` → `h2_finish_body` →
  `send_headers(trailers, END_STREAM)`); the raw-object spelling was left
  alone because it has no reference behaviour and the sibling Node-oracle
  fixture lane owns that surface.
* **`session.setLocalWindowSize()`** records a value and sends nothing. See
  gap 12 for why that is not a shortcut.
* **The `h2` / hyper-`http2` feature was not removed**, and the tokio
  inventory is byte-identical: `scripts/tokio_inventory.py` reports *38
  manifest edges across 12 workspace crates, 20 tokio-family packages in
  Cargo.lock — unchanged*. Group D is still 2 edges. The brief's "honest target
  D 2→1" assumed the `h2` edge would go with the migration; it cannot, because
  the declining paths above are real. What did change is what the edge *means*,
  and the inventory's prose for it was rewritten from "always, in any program
  that imports node:http2" to name the three remaining reasons.
* **The two P5 listen-path defects the previous lane found are still not
  fixed** (`SO_REUSEPORT` from `noDelay`, and a failed bind that never emits
  `'error'`). They must land together and with their own full sweep; this
  branch does not touch `turnloop_serve::listen`'s argument list.
* **Nothing ran on Windows or macOS**, and **nothing was benchmarked** — the
  box carried load 13–22 from other lanes throughout, which is the condition
  under which a timing number is worthless. The thread counts and the loop
  counters below are structural, not timing, and are load-independent.


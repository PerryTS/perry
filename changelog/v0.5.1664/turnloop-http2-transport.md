**turnloop HTTP/2 — `node:http2` on the loop.**

`http2.createServer`, `http2.createSecureServer` and cleartext `http2.connect`
now run on `turnloop_http::http2::Connection` over `turnloop_net` sockets,
driven sans-I/O from the loop's completion dispatch. The `h2` crate and hyper
remain for the declining paths only: `http2.connect('https://…')` (there is no
public TLS **client** installer on a turnloop socket yet), a `worker_threads`
agent with no loop of its own, and a cluster worker.

**h2spec against Perry's own server: 147 tests, 147 passed, 0 failed**
(`--strict`; the Generic, RFC 9113 §3–§8 and HPACK suites), against
`http2.createServer((req, res) => …)`. The server is one OS thread.

What changes that a user can see:

- **`session.settings()`, `.goaway()` and `.ping()` encode frames.** They used
  to be a loopback simulation: they enumerated `Http2SessionHandle`s, picked
  the ones whose `session_type` was the opposite of the caller's, and pushed a
  synthetic event into their queues. Nothing reached a wire, which is why the
  `test-parity/node-suite/http2/` corpus passed — every case in it is a Perry
  client talking to a Perry server in one process. A `ping` now round-trips a
  real PING/PING-ACK, `settings()` sends SETTINGS and fires its callback from
  the peer's acknowledgement, and `goaway(code, lastStreamID, opaqueData)`
  encodes all three fields.
- **`stream.id` is the RFC 9113 stream identifier of its own connection**,
  not a process-global odd counter that corresponded to nothing on the wire. A
  second session on one process restarts at 1, as Node's does.
- **`session.request()` calls multiplex.** The `h2` client raced N requests for
  one `SendRequest`; the losers got `"HTTP/2 session is not connected"`.
- **A cleartext client session no longer builds a private `current_thread`
  tokio runtime per session, nor a second one per request** (perry#10327).
- **`stream.close([code])` emits RST_STREAM**; it used to set a local flag.
- **`allowHTTP1`** is honoured: with ALPN `http/1.1` the connection is handed
  to the HTTP/1.1 server by moving one table entry, with no socket transfer and
  no window in which a completion could be misrouted — which is why the module
  shares P5's subsystem slot rather than taking one of the eight.

Seven defects in the previously committed-but-unwired transport were found by
wiring it and by the gap suite, each of which would have shipped: a `'stream'` listener would have
had a default response synthesized on top of its own (two responses on one
stream, i.e. `STREAM_CLOSED` and a dead connection); a 204/304/HEAD response's
HEADERS frame was never flushed; a half-closed(local) stream was retired while
its peer could still send DATA; a failed write released the connection id to
P5's sink and leaked one handle id per connection; two hand-encoded control
frames were written ahead of whatever the core had queued; and
`turnloop_serve::adopt_alpn_http1` — the whole reason for sharing the subsystem
slot — did not exist. The seventh was found by the gap suite rather than by
reading: `session.settings()` / `.ping()` / `.goaway()` called on the tick after
`http2.connect()` wrote their frame **ahead of the client connection preface**
and the peer answered a connection error, hanging
`test_gap_gc_http2_pending_event_callback_rooting`; those three are now queued
until the transport is ready, as `session.request()` already was.

Full writeup, the turnloop gaps this hit, and what it did not do:
`docs/turnloop/http2b-report.md`.

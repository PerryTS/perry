### turnloop P5 — the HTTP/1.1 and TLS server stack on turnloop, and TLS on a turnloop socket

**`node:http` and `node:https` servers no longer run on hyper.** A server bound
on the primary agent now binds, accepts, decodes, answers and closes entirely on
that agent's `turnloop::Loop`, with `turnloop_http::http1`'s sans-I/O decoder and
encoder in place of hyper's parser and framer
(`crates/perry-ext-http/src/server/turnloop_serve/`). What that deletes per
server is one `tokio::spawn`ed accept loop; per connection, one `tokio::spawn`ed
`serve_connection` plus its `tokio::select!`; and per request, the `mpsc` that
carried `(req, res)` to the main thread, the `notify_main_thread()` that woke it,
and the `oneshot` that carried the response shape back. The handler, the codec
and the socket are on one thread, so `res.end()` encodes the response and
submits the write where it is called.

The JS-visible surface is untouched: the same `IncomingMessage` and
`ServerResponse` handles, the same `js_node_http_server_process_pending` pump on
the same tick, the same `'connection'` / `'request'` / `'upgrade'` ordering. The
completion sink deliberately runs no JS — it decodes and queues, exactly where
hyper's `mpsc` delivered — so the event-loop phase order the gap suite pins does
not move.

**TLS runs above the socket, not beside it.** `perry-ext-net`'s
`turnloop_tls.rs` drives `turnloop-tls`'s unbuffered rustls core from the
completion sink: ciphertext in from `NET_DATA`, ciphertext out through
`turnloop_net::write`, plaintext back to the binding, all on the loop thread.
`turnloop_tls_io.rs` is the per-socket layer over it, including the ciphertext →
plaintext write accounting that keeps `socket.write(chunk, cb)` firing when the
bytes have actually left.

**That is what unblocks `socket.upgradeToTLS`, and with it the outbound TCP
client class P1 left behind.** P1 kept every TLS-upgradable socket on tokio
because the upgrade handed a live `TcpStream` to `tokio_rustls` mid-stream and
turnloop owns its descriptor without exposing it. With the session installed
*above* the turnloop handle, no descriptor has to move at all: the same handle
keeps carrying bytes. `net.connect(port, host)`, `tls.connect` and
`socket.upgradeToTLS` (PostgreSQL's `SSLRequest` flow) are all on turnloop now.
turnloop's own descriptor handoff (`Detached::into_fd`, issue #35) solves the
same problem by moving the socket out; this solves it by never leaving.

**`server.keepAliveTimeout = 0` now means what Node means by it.** Measured on
the pinned oracle (Node 26.5.1, raw-socket client): a zero timeout answers
`Connection: keep-alive` with **no** `Keep-Alive` header and never closes the
idle connection, while a finite one answers `Keep-Alive: timeout=floor(ms/1000)`
and FINs at `keepAliveTimeout + keepAliveTimeoutBuffer` — 300 + 1000 closes at
1303 ms, 1000 + 1000 at 2002 ms, and 5000 + 1000 (the defaults) at 6000 ms.
Perry folded the reuse decision and the timeout together
(`should_keep_alive && keep_alive_timeout_ms > 0.0`), so a server that disabled
the timeout got `Connection: close` on every response and no connection reuse at
all. They are separate decisions now, and the idle close is armed as a real
deadline rather than left unimplemented as it was under hyper.

**New in the runtime's turnloop net layer**, both used by the above and both
general:

- `turnloop_net::timer_arm` / `timer_cancel` — a subsystem-owned one-shot
  deadline delivered as a `NET_TIMER` completion. Perry's server timeouts are
  deadlines on a *connection*, not JS timers, and a binding has no way to create
  a JS timer; arming them here puts them in `Loop::next_deadline()`, so a park
  with nothing but an idle keep-alive connection still ends on time. Deliberately
  unreferenced: a pending deadline never keeps the process alive by itself.
- `turnloop_net::transfer` — hand a live socket to another subsystem, keeping its
  id and every outstanding operation. An HTTP `'upgrade'` is exactly this: the
  multishot read is *not* cancelled, routing reads the subsystem out of the entry
  at dispatch time, so the next byte reaches `net` with no gap, no resubmission
  and no descriptor moving.

**Node-fidelity fixes the migration exposed**, each measured against the
oracle rather than argued: `Transfer-Encoding: chunked` is spelled the way Node
spells it rather than the way the encoder synthesizes it; a Content-Length
Perry *synthesized* is dropped where Node sends none (204/304/1xx, a HEAD
response, a close-delimited HTTP/1.0 body) while one the handler set is kept;
the trailer block after a chunked body now reaches the wire, which the hyper
path dropped; `res.writeContinue()` and `res.writeProcessing()` reach the wire
instead of being no-ops that relied on hyper; and `req` emits Node's
`'aborted'` when the peer vanishes mid-request, which neither path did. Two
defects in the socket layer went with them: `socket.end()` followed by the
peer's FIN shut the write side down twice, and the second `shutdown(2)`
answered `ENOTCONN` as a spurious JS `'error'` (latent on a plain turnloop
socket, certain on the TLS path); and a rustls failure after the application
has asked to close is teardown noise Node does not report either.

**Still on hyper, and why.** The hyper accept loop is narrowed, not deleted —
the same shape P1 left the tokio socket task in. A server declines the turnloop
path, per listen, when the agent has no loop (a `worker_threads` agent, before
P3/P4 give it one), when it is a cluster worker (SCHED_RR fd passing and the
`SO_REUSEPORT` bind both need the `std::net::TcpListener`), or when a
`WebSocketServer({ server })` is already attached (its handshake is completed by
`tokio_tungstenite` over an owned stream, which a turnloop connection cannot
produce; `server.on('upgrade')` needs no such thing and is served on turnloop).
`http2.createSecureServer` and `perry-ext-fastify` keep their own hyper/h2 loops
and are P5 follow-ups.

Full writeup, every measurement and every gap found in the protocol crates:
`docs/turnloop/p5-report.md`.

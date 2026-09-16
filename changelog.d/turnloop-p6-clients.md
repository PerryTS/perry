### turnloop P6 — outbound HTTP (`fetch`, `undici`) and SMTP on turnloop

Perry's outbound HTTP/1.1 and its SMTP client leave tokio and reqwest/lettre for
turnloop handles, driven sans-I/O over `turnloop-http`'s `client` + `http1`
codecs, `turnloop-smtp`'s pull-driven `Connection`, and `turnloop-tls`'s
unbuffered rustls core. Full writeup: `docs/turnloop/p6-report.md`.

**New engines** (both in perry-stdlib, both registering their own
`turnloop_net` subsystem and their own private handle-id band):

- `turnloop_client/` — the outbound HTTP/1.1 engine: connection pool
  (`pool_max_idle_per_host = 16`, `pool_idle_timeout = 90 s`, the numbers the
  reqwest client already used), redirects, abort, `Content-Encoding` decoding,
  and the idle-close deadline that keeps a pooled socket from holding the
  process open. Per-phase request deadlines are deliberately NOT armed — the
  reqwest path armed none either, and every default they would pick is
  observable.
- `turnloop_smtp/` — SMTP: greeting, EHLO/HELO, STARTTLS and implicit TLS,
  AUTH PLAIN, envelope, dot-stuffed DATA, QUIT. Its C seam (`js_perry_smtp_*`)
  is how `perry-ext-nodemailer` — a separately linked staticlib — reaches it.
- `turnloop_tls_client.rs` — the client TLS session both engines drive.

**Wired:** the global `fetch` (every transport-bearing `js_fetch_*` entry
point) and `undici`, which rides the same stack. `axios`, `node-fetch`, the
`node:http` client and `http2.connect` are NOT moved — each needs an HTTP C seam
of the shape `js_perry_smtp_*` has, and each has a defect of its own worth
fixing first. reqwest and lettre are **not** removed: a proxy, a worker agent
with no loop and the `tokio-wait-driver` arm all still decline to them.

**SMTP is engine-complete and surface-blocked.** `turnloop_smtp` drives the
protocol correctly (8 tests over real bytes) and `perry-ext-nodemailer` reaches
it through the seam, but `transporter.sendMail(...)` has never worked from JS in
Perry: `createTransport` returns a bare handle NUMBER, so codegen refuses the
call on a primitive receiver before the runtime's handle dispatch is consulted.
Two missing dispatch rows are added here; the other half — returning a
handle-band pointer — belongs with whoever owns that binding. No claim is made
that a Perry program's mail now goes over turnloop.

**Node-fidelity fixes this exposed, all reproduced on the base commit first:**

- **`AbortSignal` never reached the global `fetch`.**
  `url::abort::notify_fetch_abort` declared its stdlib hook as an `extern`
  under `#[cfg(feature = "external-fetch-symbols")]` and did *nothing* in the
  other arm — which is the arm a default `fetch`-using build compiles to (the
  global fetch is reached through `GLOBAL_FETCH_WITH_OPTIONS`). So
  `controller.abort()` and `AbortSignal.timeout` were inert for every
  `fetch(url, { signal })`. Registered twin added
  (`js_register_global_fetch_notify_abort`).
- **`Content-Encoding` was never decoded.** No reqwest decompression feature is
  enabled anywhere in the workspace, so a `gzip`/`br`/`deflate`/`zstd` response
  reached JS as compressed bytes. The turnloop path decodes it, as Node does.
- **`response.url` and `response.redirected` ignored redirects** — the original
  URL and `false`, whatever happened on the wire.
- **A bodyless `POST` sent no `content-length`.** Node sends `content-length: 0`.
- **A transport failure carried no `cause.code`** unless it was DNS; an
  `ECONNREFUSED` reached JS with `cause.code === undefined`.

Three more defects, all found by a probe against **real remote endpoints** and
all invisible to every loopback fixture:

- **Unconsumed decoder input was not retained.** `http1::Decoder`'s contract is
  that the host keeps what a step did not consume. Feeding only the newest read
  threw the earlier half away, so any response whose HEAD spans two reads failed
  with `HPE_INVALID_HEADER_TOKEN`. `https://github.com/` is such a response;
  nothing a local fixture serves is.
- **No default `User-Agent`.** The reqwest client sets `perry/<version>`
  deliberately (#236 is about `api.github.com` rejecting anonymous requests);
  the turnloop path sent none and got a 403 where Node got a 200.
- **No keep-alive contributor.** A turnloop handle keeps `Loop::turn` blocking
  but not Perry's event loop, so a program whose only work was an outbound
  request exited before the response arrived. The fetch gap fixture hid it by
  running a server of its own.

An independent review of the exactly-once delivery paths then found a **hang**
that none of the fixtures above could see, now fixed:

- **A request the pool parked was never admitted when the connections ahead of
  it failed.** Admitting a waiter lived only in `release()`, reached from
  exactly one place — a response that completed normally. Every failure path
  closes the connection directly. Sixteen concurrent requests to one origin that
  all failed left the seventeenth parked forever: its sink was never called and
  `has_pending_requests()` kept the event loop alive on a promise that could not
  settle, so the process never exited. `test_gap_turnloop_fetch_pool_wait.ts`
  pins it by asserting the **exit** (24 concurrent fetches at a dead port, no
  `process.exit()`); the unfixed build times out having printed nothing.
- **A connection whose idle timer could not be armed was pooled anyway**, where
  nothing would reclaim it and its pool seat was occupied for the process's
  life. It is closed instead.
- **`turnloop_smtp::pump` fell out of its 64-event budget silently**, which
  would leave an exchange unsettled forever. It now fails the exchange and
  counts it (`pump_exhausted=` on the stats line).

`turnloop-smtp 0.1.0-alpha.3` is added (default features: sans-I/O, no
`turnloop-io`); it re-exports the same `lettre` 0.11 message builder the
nodemailer surface already used, so the MIME bytes are produced by the same code
and only the transport changed. `turnloop-http` and `turnloop-tls` were already
in the tree from P5.

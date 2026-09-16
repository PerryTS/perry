### turnloop P6 — outbound HTTP (`fetch`, axios) and SMTP on turnloop

Perry's outbound HTTP/1.1 and its SMTP client leave tokio and reqwest/lettre for
turnloop handles, driven sans-I/O over `turnloop-http`'s `client` + `http1`
codecs, `turnloop-smtp`'s pull-driven `Connection`, and `turnloop-tls`'s
unbuffered rustls core. Full writeup: `docs/turnloop/p6-report.md`.

**New engines** (both in perry-stdlib, both registering their own
`turnloop_net` subsystem and their own private handle-id band):

- `turnloop_client/` — the outbound HTTP/1.1 engine: connection pool
  (`pool_max_idle_per_host = 16`, `pool_idle_timeout = 90 s`, the numbers the
  reqwest client already used), redirects, per-phase deadlines, abort,
  `Content-Encoding` decoding, and the idle-close deadline that keeps a pooled
  socket from holding the process open.
- `turnloop_smtp/` — SMTP: greeting, EHLO/HELO, STARTTLS and implicit TLS,
  AUTH PLAIN, envelope, dot-stuffed DATA, QUIT. Its C seam (`js_perry_smtp_*`)
  is how `perry-ext-nodemailer` — a separately linked staticlib — reaches it.
- `turnloop_tls_client.rs` — the client TLS session both engines drive.

**Wired:** the global `fetch` (every transport-bearing `js_fetch_*` entry
point), `undici` (which rides the same stack), the bundled `nodemailer`, and
`perry-ext-nodemailer`. reqwest and lettre are **not** removed: a proxy, a
worker agent with no loop and the `tokio-wait-driver` arm all still decline to
them.

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

`turnloop-smtp 0.1.0-alpha.3` is added (default features: sans-I/O, no
`turnloop-io`); it re-exports the same `lettre` 0.11 message builder the
nodemailer surface already used, so the MIME bytes are produced by the same code
and only the transport changed. `turnloop-http` and `turnloop-tls` were already
in the tree from P5.

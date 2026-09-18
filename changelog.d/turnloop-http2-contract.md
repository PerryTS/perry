**turnloop HTTP/2 — contract findings and the unwired transport.**

Groundwork for moving `node:http2` off the `h2` crate and hyper onto
`turnloop_http::http2`. This lands the design and the protocol-contract
evidence; it does **not** land the migration — `crates/perry-ext-http/src/server/turnloop_h2/`
is committed but deliberately not wired into the module tree, so behaviour is
unchanged and `http2.createServer` / `createSecureServer` / `connect` are all
still on hyper and `h2`.

Three findings about `turnloop_http::http2` 0.1.0-alpha.5, each proven by
`docs/turnloop/http2-contract-probe.rs` (depends only on `turnloop-http`):

- a stream reset with unreleased DATA holds its slot in `Connection`'s stream
  table for the life of the connection, and once the table fills `receive`
  raises `REFUSED_STREAM` as a **connection** error — a server that resets
  streams without first returning their flow-control window works until the
  first N stream errors and then drops every connection (2 of 6 streams
  accepted without the release, 6 of 6 with it);
- a stream opened by a peer that has not yet seen a graceful GOAWAY is a
  connection error, where Node answers `RST_STREAM(REFUSED_STREAM)` and keeps
  the session;
- `Step` has two independent zero cases — `consumed == 0` with no event means
  "wait for bytes", `consumed > 0` with no event (the preface, a SETTINGS ack,
  PRIORITY) means "keep going" — and a host that loops on "an event came back"
  stalls at the preface.

Also records six divergences in Perry's existing HTTP/2 measured against
Node 26.5.1, including `http2.connect('https://…')` opening a cleartext socket
to port 80, a client that cannot multiplex, a fresh tokio runtime per request,
and `session.settings`/`goaway`/`ping` that never reach the wire.

And two bugs in P5's already-landed listen path, found while reading it as the
template and measured against Node 26.5.1 — **reported, not fixed**, because
they have to land together and with a full gap sweep:

- `turnloop_serve::listen` passes `server.noDelay` (default `true`) into
  `tcp_listen`'s `reuse_port` parameter, traced end to end into turnloop's
  `SO_REUSEPORT`, so **every turnloop HTTP/1.1 and HTTPS server binds with
  `SO_REUSEPORT`** and a second `listen()` on the same port silently succeeds
  where Node answers `EADDRINUSE`;
- a bind that genuinely fails `eprintln!`s and returns, and **never emits
  `'error'`** — reproduced on the base commit with the port held by a non-Perry
  process, so it is independent of the first. Fixing only the first turns a
  wrong answer into a hang, which is why neither is fixed here.

Full writeup, the flow-control / multiplexing / GOAWAY design decisions, and
what remains: `docs/turnloop/http2-report.md`.

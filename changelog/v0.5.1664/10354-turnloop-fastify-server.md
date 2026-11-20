### turnloop: fastify and the bundled framework server move off hyper

`perry-ext-fastify` and `perry-stdlib`'s `framework/server.rs` were the two
HTTP servers turnloop P5 did not migrate. Both now serve on turnloop through a
new crate, **`perry-http-server`** — the HTTP/1.1 server core (one multishot
`accept_start`, one multishot `read_start`, the `turnloop_http::http1` codec
driven sans-I/O, Node's framing rules, its `Connection`/`Keep-Alive` decision
and its idle-close arithmetic) behind a `Host` trait.

**Why a crate and not a dependency edge.** P5 left the choice open: extract the
sans-I/O server into a crate both bindings can depend on, or give
`perry-ext-fastify` an edge to `perry-ext-http`. The edge is cheaper and does
not work. `perry-ext-http` still needs hyper after P5 (a `worker_threads`
agent, a cluster worker, an attached `WebSocketServer`, `reqwest`), so
`perry-ext-fastify → perry-ext-http → hyper` would be a live edge — and
`scripts/tokio_inventory.py` gates *manifest* edges, so the swap would delete
four lines from the ratchet while every fastify program still linked hyper.
Both crates are `staticlib`s, so it would also bundle hyper, h2, reqwest and
rustls into `libperry_ext_fastify.a`. And it does nothing for `perry-stdlib`,
whose framework server is the *fallback* the well-known flip replaces with
`perry-ext-http` and must not depend on it. The crate has two consumers on the
day it lands. What was given up: `perry-ext-http` did **not** migrate onto it,
so two HTTP/1.1 state machines exist until it does — see
`docs/turnloop/fastify-report.md`.

**Edges removed** (`scripts/tokio_inventory.json`, updated in this commit):
`perry-stdlib`'s `hyper` and `hyper-util`. Both were behind the `http-server`
feature, which `full` enables, so every default build of `perry-stdlib` carried
them; the feature no longer requires `async-runtime` either.

**Edges that did not move, and why.** `perry-ext-fastify`'s four all survive,
for one reason rather than four: an app with `app.server.on('upgrade', …)`
handlers declines the turnloop path at listen time, because the handshake ends
in `perry_ext_ws::register_external_ws_stream`, which needs an owned
`AsyncRead + AsyncWrite` stream a turnloop connection cannot produce — the same
blocker P5 recorded for `perry-ext-http`'s attached `WebSocketServer`. Every
other fastify app is served on turnloop. And group H's other four edges are not
this lane's subject at all: `sqlx` / `redis` / `mongodb` are the bundled
database drivers and `tokio-rustls` is the bundled TLS server and net client
TLS. Neither is reachable through `framework/server.rs`.

**Node-fidelity behaviour this changes for fastify.** Responses now carry
Node's `Connection: keep-alive` / `Keep-Alive: timeout=5` pair and honour an
idle close, a HEAD request shadowing a GET route is framed body-forbidden from
the *real* request method, and a full request queue answers `503` instead of
dropping a channel.

**Two traps the P5 and HTTP/2 lanes already paid for, written into the core so
a third lane does not.** `http1::Decoder` raises `Event::End` from a step that
consumes zero bytes (PerryTS/turnloop#50), so the decode loop continues on "an
event, **or** bytes consumed" and only `None` with `consumed == 0` ends it —
the rule and the stall it prevents are in `conn.rs`'s `drain` doc comment. And
`perry_http_server::listen` takes `reuse_port` and `no_delay` as separate named
arguments with the history in the doc comment, because P5's listen path had
`noDelay` sitting in `reuse_port`'s slot for its whole life.

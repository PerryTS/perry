### `fetch` and `axios` stop declining to reqwest on a thread that does not own its agent's loop

`scripts/tokio_inventory.json` recorded three blockers on the
`perry-stdlib -> reqwest` edge. **Two of them were already closed in the tree**
before this lane opened it, and the annotation had simply gone stale:

- *"a CONNECT tunnel driven from a URL rather than from a prebuilt
  `reqwest::Client` for the proxy"* — `turnloop_client::proxy_for` resolves the
  proxy from `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` or from
  `undici.setGlobalDispatcher(new ProxyAgent(…))`, and `exchange`'s `Tunnel`
  runs the CONNECT. `tunnels_total()` is its liveness counter.
- *"a line-splitting state machine over the engine's `Sink::on_head`/`on_chunk`
  hooks … those hooks exist and NOTHING calls them"* —
  `fetch::turnloop_bridge::try_dispatch_stream` is the caller, so the SSE
  poll surface (`js_fetch_stream_start`) runs on the engine and the hooks are
  no longer an unexercised mode.

The third — *"per-agent loops for the decline"* — is what this change closes.
turnloop P9 gave every JS agent a loop, so the thread that still could not
submit was never a worker: it is a **second thread acting for an agent another
thread already owns**. Android is the shape, with `perry-native` running the
compiled TypeScript on the primary heap while the UI thread pumps for the same
agent; whichever claims the route first left the other with no loop, and its
only answer was to run a reqwest future for itself.

`turnloop_client::submit` now posts the whole submission to the thread that owns
the agent's loop (`perry_ffi::agent_post`, turnloop P10). The work runs on a
thread serving the *same* JS heap, so the promise is settled where that agent's
values live. `abort_signal` posts with it: without that, `controller.abort()`
would have been silently inert for exactly the requests that moved.

Two ordering rules the implementation is built around, both asserted:

- **Only a decline that belongs to the THREAD may be posted.** `Unsupported`,
  `Proxy` and `NoTls` are properties of the request and of process-wide
  configuration, so the owner would refuse them identically — and by the time
  the job lands over there the caller has been told the engine took the request
  and has not spawned its fallback. `submit` therefore prepares the request
  before it chooses a transport.
- **When no loop exists for the agent at all, decline before preparing.**
  `prepare` reaches `tls_config()`, whose first call loads the platform root
  store, and on the `tokio-wait-driver` A/B arm every request declines. The arm
  exists to measure the transport this replaces and must not be charged for a
  client it can never use.

The bundled `axios.rs` — which took reqwest *unconditionally*, and built a fresh
`reqwest::Client` per call, so no two axios requests to the same host ever
shared a connection — now offers every request to the same engine `fetch` uses,
with its pool, proxy handling and TLS. Its six near-identical entry points
collapse onto one dispatch path.

#### The edge stays, and here is exactly why

`perry-stdlib -> reqwest` is **not** removed and the inventory still counts 21
edges. What holds it open is no longer a hole but an absence, and it is the same
pair every plan-A edge names: the `tokio-wait-driver` A/B arm compiles no agent
loop at all, and a host where `Loop::new` failed has nothing to post to. Two
request-shaped declines also still reach reqwest — an `https://` proxy, and a URL
the fetch policy layer rejects, where reqwest is the *more* permissive of the two
(it sends embedded credentials as Basic auth, and will send CONNECT/TRACE/TRACK).

A `socks5://` proxy is **not** one of them, contrary to what the `Declined::Proxy`
doc comment implied: reqwest is built here without its `socks` feature and every
socks arm of its connector is behind that `cfg`, so declining a socks proxy
routes it to a transport that cannot do it either. The decline is kept only
because the two paths' error *text* differs and the suite pins reqwest's; that is
now written down where the variant is declared.

#### Tests

`a_thread_with_no_loop_posts_its_fetch_to_the_thread_that_owns_one` asserts the
crossing rather than the absence of a throw: the poster's own
`agent_post::dispatched()` stays at zero, `turnloop_client::submitted_total()`
does not move while the poster runs and does move once the owner has run the job
(so the job did the work, not merely arrived), and the sink runs on the *owner's*
thread. It was watched failing twice before being trusted — once with posting
disabled, once with `prepare` moved after the transport choice, which is what
turns the request-shaped decline into an accepted post.

`an_axios_request_rides_the_engine_fetch_uses` watches the engine's own
`submitted_total()` move for an axios call; with the engine call removed it fails
on "accepted and did nothing", which is the case a `bool` return cannot see.

Both tests need to OWN the agent's loop, and the route is a single slot per agent
claimed for the life of the *claiming thread* — two libtest threads racing for it
stall each other for the whole retry window. `OwnerLease` serializes them and
gives the route back at the end of the test body rather than at thread exit.

#### Group H, read against the tree but not moved

`perry-stdlib -> tokio-rustls` (`node:tls`) stays, and its inventory note is
corrected rather than acted on. The note said it "needs a turnloop-tls SERVER
session (an accept-side counterpart to `perry-tls-session`) first — a transport
job, not a policy one". That is half stale: an accept-side session over a
turnloop handle exists and is in production use —
`perry_ext_net::turnloop_tls_io::install_server_session` over `perry-ext-net`'s
own `turnloop_tls::TlsSession::server`, which is what `https.createServer()` and
`http2.createSecureServer()` ride on the turnloop path. What is missing is that
it was never extracted into a shared crate the way `perry-tls-session` /
`perry-tls-turnloop` extracted the client half, and perry-stdlib cannot depend on
perry-ext-net.

An extraction alone would still not move the edge, because the session is the
second half of the job. All three surfaces that hold it run on **tokio sockets**,
not turnloop handles: `tls.rs`'s `TlsAcceptor` server with its own
`tokio::net::TcpListener` accept loop, `net/mod.rs`'s `TlsConnector` client, and
`ws.rs`'s `wss://` connector — which already uses `turnloop-websocket` for its
*codec* while keeping a tokio stream underneath, and says in its own comment why
it builds a `tokio_rustls` connector instead of reaching for the sans-I/O one.
`grep turnloop` over those ~3,700 lines returns eight hits and every one is a
comment or the WS codec. The prerequisite is P1 (put the bundled net/tls/ws
sockets on turnloop handles) plus P5 (install sessions above them) for this
stack — the work perry-ext-net and perry-ext-http each had a whole phase for.

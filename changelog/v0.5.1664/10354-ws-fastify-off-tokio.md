### turnloop: `perry-ext-ws` and `perry-ext-fastify` off tokio (groups E and F)

`scripts/tokio_inventory.py`: **29 manifest edges → 25**, across 11 workspace
crates → 9. Both groups are removed whole, and their entries are deleted from
`scripts/tokio_inventory.json`:

* **E** — `perry-ext-ws -> tokio`
* **F** — `perry-ext-fastify -> hyper`, `-> hyper-util`, `-> tokio`

Neither crate declares an async runtime any more. `perry-ext-fastify` also
stops declaring `http-body-util`, `bytes` and `socket2`, which existed only for
the deleted hyper path.

#### What the blocker actually was

The inventory recorded F as blocked on "an owned `AsyncRead + AsyncWrite`
stream a turnloop connection cannot produce", and named two ways out: a
descriptor handoff (`turnloop::Driver::detach` exposed through
`perry_ffi::turnloop_net`), or `perry-ext-ws` on `turnloop-websocket`. Neither
was needed. `perry-ext-ws` was *already* on `turnloop-websocket` — the protocol
had moved and only the transport was left on tokio — so the fix was the
transport, and closing E closed F.

#### `perry-ext-ws`

* `io.rs`, the tokio stream driver, is **deleted**, and with it the per-connection
  `tokio::spawn`ed `select!` loop and its `mpsc` command channel.
* New `turnloop_io.rs`: the outbound client (`new WebSocket(url)`) on a turnloop
  `tcp_connect` (subsystem slot 7), with `perry_tls_session::TlsClientSession`
  above the same handle for `wss://` — so the TLS session is host-driven rather
  than owning the socket, and `perry-ext-net` (and its `tokio-rustls`) is no
  longer a dependency. TLS config comes from Node's own environment through
  `perry_ffi::node_tls_client_environment()`, so `NODE_TLS_REJECT_UNAUTHORIZED`
  / `NODE_EXTRA_CA_CERTS` / `SSL_CERT_FILE` behave as before.
* The standalone `WebSocketServer({ port })` binds through `perry-http-server`
  (slot 8) instead of a `tokio::net::TcpListener` accept loop, and takes the
  upgrade through that crate's new `Host` hook. The hand-rolled
  read-until-head loop (`accept_on_stream`) is gone: a `ws` server is an HTTP
  server that answers one kind of request, and the core already gets the head
  decode, the `400` for a non-upgrade request, and the pipelining right.
* `register_upgraded_stream<S: AsyncRead + AsyncWrite>` is replaced by
  `adopt_host_connection(conn_id, Transport, leftover)`. The host keeps the
  stream and supplies three function pointers, which is the seam
  `perry-ext-http`'s turnloop server already used.
* `turnloop_link`'s `Transport` is now **per link** rather than one process-wide
  `OnceLock`. Three hosts exist in one binary now (perry-ext-http's turnloop
  server, its hyper upgrade path, and this crate's own listener); a single
  global would have silently handed all three the first registration's writer.
  `each_link_keeps_the_transport_it_was_adopted_with` pins that.

#### `perry-http-server`

Grew the upgrade hook its own module header recorded as withheld "until that is
solved, with a caller" — `Host::takes_upgrades` / `Host::on_upgrade` /
`Host::on_upgraded`, plus a public `finish()` for the graceful close an upgraded
protocol needs (`destroy` cancels the close frame the codec just queued, and the
peer then reports 1006 instead of the code it was sent). It went in with two
callers, not on spec. A host that leaves `takes_upgrades` false is unaffected:
an upgrade is still served as an ordinary request, which is what Node does with
no `'upgrade'` listener (#4973).

#### `perry-ext-fastify`

`FastifyHost` implements the hook; `app.server.on('upgrade', …)` is answered by
`perry_ext_ws::accept_http_upgrade` on the connection the core already owns.
The listen-time decline, `listen_on_hyper`, the hyper service fn, the
`hyper::upgrade::on` task, `Reply::Hyper` and `cluster_bind`'s second binder are
all deleted. `Reply` gained a `#[cfg(test)] Captured` variant so two unit tests
that had borrowed the production hyper variant do not keep a live reply mode
nothing reaches.

#### `perry-runtime`

`turnloop_net::sink::MAX_SUBSYSTEMS` 8 → 16. Slot 8 would otherwise have been
refused, leaving `available()` false and every WebSocket declining to a
transport that no longer exists. It is not part of the ABI digest — a binding
names a slot number, never this constant — so the change is additive. The same
comment now records that **the slot map is over-subscribed today**: perry-stdlib's
turnloop HTTP client collides with perry-ext-pg on 2, perry-ext-fastify with
perry-ext-mysql2 on 4, and perry-stdlib's framework server with perry-ext-ioredis
on 5, because the P5 and P7 lanes numbered from two different ledgers. Each pair
is only reachable in a program linking both bindings, which is why nothing has
caught it. Not fixed here; written down so the next lane to take a slot does not
read the list as complete.

#### The narrowing, stated plainly

There is no second transport now, so an agent that owns no `turnloop::Loop` has
**no WebSocket client, no standalone WebSocket server and no fastify server at
all**. That is the `tokio-wait-driver` A/B arm, which compiles no agent loop by
construction, and a host where `Loop::new` failed — the same pair every
remaining plan-A and plan-B edge names.

It is **not** `worker_threads`. turnloop P9 gave every JS agent its own loop, and
`agent_loop_tests::a_worker_agent_gets_its_own_loop` asserts exactly that: a
worker agent's `net_available()` is true and its `ensure_loop()` succeeds. The
code here is right — it gates on `perry_ffi::turnloop_net::available()`, which
answers true for a worker — so workers keep WebSockets; only this sentence was
wrong, and a false "WebSockets do not work in a Worker" is too expensive a
belief to leave in the changelog. Each says
so rather than doing nothing: `new WebSocket(url)` rejects and raises `'error'`,
`new WebSocketServer({port})` raises `'error'`, and fastify's `listen()` reports
it through the `(err, address)` callback. Previously these agents silently got
the tokio path.

#### Evidence

`turnloop_io::tests::both_slots_register_a_live_sink_in_the_runtime` asserts
`perry_ffi::turnloop_net::sink_installed()` for both slots against the linked
runtime, so it cannot pass with nothing listening. Sabotage-checked: reverting
`MAX_SUBSYSTEMS` to 8 makes it fail on slot 8 and nothing else.
`host_upgrade::tests::prepare_runs_before_the_pipelined_leftover_is_decoded`
pins the ordering a host depends on, using a masked ping whose pong proves the
leftover actually reached a writer.

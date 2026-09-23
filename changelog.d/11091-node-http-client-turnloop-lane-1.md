### `node:http` client: lane 1 on turnloop (`perry-ext-http`)

`http.request()` / `http.get()` no longer reach `reqwest` for the most common
shape. `crates/perry-ext-http/src/client_turnloop.rs` drives
`turnloop_http::client::Http1Connection` over `perry_ffi::turnloop_net`, on the
agent's own loop, and is offered every exchange in `dispatch_request_snapshot`
before the reqwest path. This is the coexistence rule `fetch` already uses
(`perry-stdlib/src/fetch/turnloop_bridge.rs`): accept what is covered, decline
the rest by a named condition, shrink the decline set per lane.

**Covered by lane 1**: cleartext `http://`, no request body, no explicit
`http.Agent`, no per-request timeout, no proxy, and none of the three headers
that already own a bypass.

**Declined** — each condition is named in the module header, and each is a
later lane: `https://` (needs the `perry-tls-session` layer, lane 3); a request
body (upload framing, lane 2); an explicit Agent (lane 4); `options.timeout` /
`req.setTimeout` (`Lifecycle` deadlines on `tl::timer_arm`, lane 2);
`NODE_USE_ENV_PROXY=1` (the CONNECT tunnel, lane 5); a URL carrying credentials
and the `CONNECT`/`TRACE`/`TRACK` methods (`client::Request::new` refuses these
— declining keeps today's error text); an explicit `Host` header, because
`client::Request::head` drops a caller's `host` and substitutes the URL
authority (Fetch's rule, not `node:http`'s — reqwest sends what the caller set,
and a unit test pins that the codec really does rewrite it); and `TE: trailers`,
`Connection: Upgrade`, `Expect: 100-continue`, which keep their existing
raw-socket bypasses and are declined using the *same* predicates those modules
trigger on, so the routing cannot disagree with itself.

Nothing about the JS surface moves. `PendingHttpEvent` was already
transport-agnostic, so the lane emits the same `ResponseHead` /
`ResponseChunk` / `ResponseEnd` / `TransportError` the reqwest task emitted, and
`js_http_process_pending` drains them unchanged — including the agent-admission
release that hangs off those terminal edges.

Two properties come out for free rather than being ported. Node's client must
**never follow a redirect** (`test_gap_http_client_no_redirect_follow.ts`, the
Next.js `proxyRequest` infinite loop); the reqwest path spells that as
`redirect::Policy::none()`, while driving the codec directly delivers the 3xx
verbatim because nothing in this lane follows anything. And `res.statusMessage`
is unchanged: `http1::Head` carries no reason phrase, so the canonical one
stands in — which is exactly what the reqwest path already did.

**Not in this lane: keep-alive.** Every exchange gets its own connection and
closes it once `Event::End` has been observed. `turnloop_http::client::Pool` is
the mechanism for the next lane, and the ordering it demands (release only
after End, with `conn.reusable()`) is the one hazard worth isolating in a change
of its own — releasing early hands a socket to the next request mid-message and
misattributes framing. The cost is invisible to JS: `req.reusedSocket` and
`agent.sockets` / `agent.freeSockets` are fed by `agent.rs`'s *facade* pool,
which was already decoupled from the physical connection.

`scripts/tokio_inventory.json` keeps the `perry-ext-http -> reqwest` edge —
reqwest is still reached by everything in the decline set — but its
`reached_when` no longer says "always", and its `blocker` is corrected: the
older note called `agent.rs` "a second Node-semantics connection pool layered
over reqwest's own", which understates it in one direction and overstates it in
the other. It is a real per-origin admission engine (FIFO waiter queue,
`maxSockets` / `maxTotalSockets` / `maxFreeSockets`, socket facades) sitting
*above* the transport; it survives the migration nearly intact, and its reqwest
coupling is 9 call sites. The same note's "three raw `tokio::net::TcpStream`
bypasses" is two: `agent.createConnection` never used tokio's `TcpStream` — it
runs on perry-ext-net's `raw_net` vtable.

New subsystem slot: `6`, the one free number below the database band (the
authority for that map is `perry-db-turnloop`'s `subsystem` module header).

### turnloop lane A — the cluster worker's SO_REUSEPORT bind, and `socket.connect()`'s missing turnloop gate

Group A of the tokio-removal plan is `perry-ext-net → {tokio, tokio-rustls}` and
`perry-ext-http → {hyper, hyper-util}`. **None of those four manifest edges
moves here, and that is the honest answer** — the inventory still reports 21
edges. What moves is the part of the recorded blocker that had genuinely
expired, plus a correction to the blocker text itself, which was wrong in the
direction that matters: it understated how much of the tree reaches tokio.

#### The cluster worker's shared bind is on turnloop now

`http.createServer()`, `https.createServer()` and `http2.createServer()` each
declined the turnloop listen path for a `cluster.fork()`ed worker, because a
worker binds with `SO_REUSEPORT` and only the hyper path's
`std::net::TcpListener` could do that (`cluster_bind::bind_listener`'s
`socket.set_reuse_port(true)`).

turnloop 0.1.0-alpha.6 split `ListenOpts::reuse_port` into
`ReusePort::{No, Share, Distribute}`, and `perry-runtime`'s `listen_opts` maps
Perry's `true` to `Share` — the same option `bind_listener` sets by hand, on
every platform Perry ships. `turnloop_serve::listen` and `turnloop_h2::listen`
take a `reuse_port` argument instead of a hard-wired `false`, and
`try_listen_on_turnloop` (all three servers) passes it for a cluster worker.

Deliberately **`Share` and not `Distribute`**: `Distribute` is the
kernel-balanced variant, it is `Unsupported` on macOS, NetBSD, OpenBSD,
DragonFly, Windows, WASI and the web, and Perry's cluster has never had it, so
asking for it would change behaviour where it works and fail the bind where it
does not.

The decision is a pure function (`turnloop_listen::listen_plan`) with unit
tests, for the reason `listen_opts` is one: this exact argument position once
received `no_delay`, which silently gave every HTTP and HTTPS listener
`SO_REUSEPORT` and left `TCP_NODELAY` unset, and nothing between the caller and
the kernel could observe it. `reuse_port` is keyed on *being a worker*, not on
`resolved.is_some()`, so a worker whose primary did not answer still shares
rather than colliding with its siblings.

`crates/perry-ext-http/tests/turnloop_reuse_port.rs` binds twice through the
same ABI and asserts the duplicate bind succeeds *where an exclusive one is
refused* — two successful binds are evidence about `SO_REUSEPORT` only if the
control is a real `EADDRINUSE`.

**The SCHED_RR half is NOT closed**, and it is a different thing: under Node's
default non-Windows policy the primary owns the listening socket and passes
accepted *descriptors* over the cluster IPC channel (`spawn_rr_inject_loop`).
turnloop's `Driver` has no API that adopts a foreign fd — `tcp_listen` /
`pipe_listen` take an address or a pipe name, and `attach` takes turnloop's own
`Detached` — so that worker keeps the hyper path, and `perry-ext-http → hyper`
stays for it.

#### `socket.connect()` was never gated on turnloop at all

`js_net_socket_method_connect` — `new net.Socket()` then `socket.connect(port)`
(issue #422's deferred connect, and what `Bun.connect` and
`net.Socket.prototype.connect` lower to) — had **no `turnloop_io::enabled()`
check**. It built a tokio socket task unconditionally, on the primary agent,
with the loop fully available, while `net.connect()` (the sibling entry point,
through `spawn_socket_task_initialized`) had taken turnloop since P5.

The tokio inventory recorded this crate's edge as reached only by "a thread that
could not get a loop of its own". For this shape that was never true: the gate
was missing, not declined. It is present now, in the same shape the sibling path
uses, including the `no_loop` fall-through and libuv's `err.code` error shape.

#### Corrections to the map, not just the code

- `scripts/tokio_inventory.json`: `perry-ext-net → tokio`'s `reached_when` is
  now **ALWAYS**, for two shapes it never named — `SocketState::cmd_tx` /
  `pending_rx` and `ServerState::shutdown_tx` are tokio channels on *every*
  socket and server including turnloop ones, and two transport-agnostic paths
  (`server_state::schedule_server_connection` behind `push_event`,
  `tls::schedule_tls_abort`) call `tokio::time::sleep` on the turnloop path. The
  `blocker` for that edge lists five reasons where it listed one, including the
  cross-crate one: `adopt::adopt_upgraded_tcp_stream` takes a
  `tokio::net::TcpStream` **by value** and its only caller is `perry-ext-http`'s
  hyper raw-`'upgrade'` peeler, so `perry-ext-net`'s edge cannot close before
  `perry-ext-http`'s does.
- `perry-ext-net/src/turnloop_io.rs`'s header claimed a socket that might be
  TLS-upgraded "is created on tokio and stays there for its whole life". P5 made
  that false — the rustls session runs above the turnloop handle
  (`turnloop_tls_io`) — and `lib.rs`'s connect sites have said so since. The
  header now does too.
- `tls.rs` named `rustls` through `tokio_rustls`'s re-export, so
  `build_client_config` — the one piece of that file **both** transports share —
  read as if it belonged to the tokio one. It is spelled through this crate's
  own `rustls` dependency now (same rustls 0.23; `turnloop-tls` re-exports it
  too, which is why the config types unify).

#### One incidental fix

`turnloop_serve::enabled()` published "registered" with an
`AtomicBool::swap(true)` *on entry*, so a second thread arriving mid-registration
skipped the registration and asked `turnloop_net::sink_installed` before the sink
existed — then took the hyper path for a loop it actually had. It is a
`std::sync::Once` now, matching `perry-ext-ws`, so that thread waits instead of
racing past. Found by a multi-threaded `cargo test` showing two `enabled()`
assertions disagreeing in the same run.

#### A note for anyone writing turnloop tests

An agent's turnloop route is claimed **once per thread, by the first thread to
ask**, and every other thread acting for that agent is declined for the rest of
its life (`event_pump::agent_loop::claim_route`). A unit test that asserts
`enabled()` is therefore a lottery under a multi-threaded `cargo test` — the
first draft of this change's coverage failed two of three such assertions in one
run while a sibling passed. Two shapes avoid it: one `#[test]` in its own
integration binary (always the first asker), or an assertion that is an
*equivalence* over availability so neither arm is a skip. Both are used here, and
each is paired with an independent witness — `turnloop_net::live_handles` for the
socket, a refused control bind for the listener — so neither can pass on a flag
the code under test merely set.

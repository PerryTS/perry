**Fix two defects in the turnloop listen path, both caused by one misplaced argument.**

`turnloop_serve::listen` passed `server.noDelay` into `perry_ffi::turnloop_net::tcp_listen`'s
**sixth** parameter, which is `reuse_port: bool`. The value was not dropped: it
travels `perry-ffi` → `abi.rs` → `turnloop_net::tcp_listen` → turnloop's
`ListenOpts { reuse_port }` → `SO_REUSEPORT`. `noDelay` defaults to `true`
(Node's `http.createServer` default since v16.5.0), so since P5 every turnloop
HTTP and HTTPS listener has had two things wrong at once:

- **`SO_REUSEPORT` was set on every server listener.** A second `listen()` on a
  port another Perry server already held silently succeeded, where Node answers
  `EADDRINUSE`. Measured against Node 26.5.1: Node `B error: EADDRINUSE`, Perry
  `B listening TOO (both bound the same port)`, with `driver=turnloop
  tokio_ticks=0` in the loop stats so it is the turnloop path answering.
  `perry-ext-net`'s own `tcp_listen` call always passed `false`; only this one
  drifted. Nothing on this path wanted `SO_REUSEPORT` — the cluster worker that
  does declines the turnloop path in `try_listen_on_turnloop` and binds a
  `std::net::TcpListener`, which is one of the two reasons that decline exists.
- **`TCP_NODELAY` was never applied**, so the turnloop transport was the only
  one serving HTTP with Nagle on; the hyper path sets it by hand on every
  accepted stream (`apply_accept_no_delay`). `no_delay` now reaches turnloop
  0.1.0-alpha.5's `ListenOpts::accept_defaults`, which applies it to every
  accepted socket before the completion reaches the binding — which is where
  Node applies it, `noDelay` being a *server* option rather than a per-socket
  one. `net.createServer`'s `noDelay` defaults to `false` in Node and is still
  not wired, so `perry-ext-net` passes `false` and its behaviour is unchanged.
  `accept_defaults.keep_alive` stays absent: `server.keepAlive` is wired on
  neither transport, and a default here would be a change nothing measured.

**A failed bind now emits `'error'` instead of printing to stderr.** This is a
second, independent defect — reproduced on the base commit with the port held
by a non-Perry process, so `SO_REUSEPORT` was not masking it — and it is why
the first fix could not land alone: with the bind correctly failing and nothing
reaching JS, a program that waits on `server.on('error', …)` hangs instead of
merely getting the wrong answer. `try_listen_on_turnloop` and the hyper
`bind_listener` arm both queued the same `eprintln!`-and-return; both now queue
a `ListenError` that the existing pump drains as `'error'`, asynchronously,
with `this` bound to the server — Node's ordering, measured: `after-listen-call,
error`, `'listening'` never fires, the `listen(cb)` callback never runs, and
`server.listening` stays false. The payload carries Node's `message`, `code`,
`errno`, `syscall`, `address`, `port` and `name`.

Pinned by `test_gap_turnloop_listen_conflict.ts` and
`test_gap_turnloop_listen_error.ts`, both byte-identical to
`node --experimental-strip-types` on Node 26.5.1, and by
`turnloop_net::tests::listen_opts_put_each_argument_in_its_own_field` — a unit
test over a new pure `listen_opts()` seam, which is the test that would have
caught the original defect, since neither symptom is visible at the call site
and both live in options handed to the OS.

Known divergence, pre-existing and Perry-wide: the `'error'` payload is a plain
object carrying every field a program reads, not a real `Error`, so
`err instanceof Error` is false. This follows `perry-ext-net`'s existing
`build_error_object` rather than introducing a second shape. An `'error'` with
no listener also does not throw the way Node's `EventEmitter` does.

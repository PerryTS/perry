`perry-ext-http` no longer depends on tokio (tokio lane D). Together with
#11205 (reqwest, tokio-rustls) and #11144 (hyper), the crate now has no
tokio-family dependency at all: `cargo tree -p perry-ext-http -i tokio -e
normal,dev` finds no tokio, and the tokio inventory drops from 5 to 4 manifest
edges.

The last two tokio users in the crate moved onto the agent's turnloop loop:

- **`agent.createConnection` / `createSocket`, and a request-level
  `createConnection`.** The HTTP exchange over the socket JS produced used to
  run in a tokio task that called the raw-net vtable's `poll_read` in a loop,
  sleeping 1 ms after each empty read. It now runs in
  `client_turnloop::raw_socket`. perry-ext-net calls a new perry-ffi hook,
  `raw_net_notify`, whenever a raw-mode socket gains bytes or goes terminal;
  the client then schedules a drain on a 0 ms loop timer. The socket stays
  perry-ext-net's, including its connect and TLS state, and the exchange is
  unchanged:
  - `Connection: close`, read to EOF, and the same parser.
  - A `101` detaches the socket for `'upgrade'`.
  - The 30 s default deadline is kept, now as a loop timer.
  - In `PERRY_NO_AUTO_OPTIMIZE=1` builds, `createConnection` also needs #11263
    (one perry-ffi and one perry-ext-net per link); auto-optimize builds work
    now.
- **The keep-alive Agent's socket-facade idle expiry.** The 40 ms
  `tokio::time::sleep` is now a loop deadline through
  `client_turnloop::push_after`. `req.setTimeout`'s early `'timeout'` uses the
  same mechanism.

Neither loop deadline keeps the process alive by itself.

Not part of this change: compiled http programs still link tokio. The compiler
driver's `binding_needs_shared_tokio` still lists `http` / `https` / `http2`,
and perry-stdlib's `external-http-{client,server}-pump` features still imply
`async-runtime`, although nothing in perry-ext-http uses either any more. That
driver change is the next step, recorded in the tokio inventory's
`perry-stdlib -> tokio` row.

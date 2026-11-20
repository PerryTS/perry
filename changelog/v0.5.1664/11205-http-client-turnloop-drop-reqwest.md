`node:http` / `node:https` client: every request now runs on turnloop, and
`reqwest` and `tokio-rustls` are no longer dependencies of `perry-ext-http`
(tokio lane C). With #11144 having taken the server off hyper, this also removes
`reqwest`, `hyper`, `hyper-util`, `hyper-rustls`, `h2`, `tower` and
`tower-http` from `Cargo.lock` entirely. The tokio inventory goes from 11 to 9
manifest edges and from 14 to 7 tokio-family lockfile packages.

`client_turnloop` (now `src/client_turnloop/`) was lane 1's
bodyless-cleartext-GET-only path (#11091). It now carries every shape reqwest
did, plus the three raw tokio `TcpStream` bypasses:

- **Request bodies.** They are buffered at `end()`, so the length is always
  known: `Content-Length`, or chunked when the caller set
  `Transfer-Encoding: chunked`.
- **`options.timeout` / `req.setTimeout`.** A `tl::timer_arm` deadline over the
  whole exchange, as reqwest's `RequestBuilder::timeout` was. It fires
  `'timeout'` and tears the exchange down. The creation-time `'timeout'` timer
  (`arm_client_timeout`) is a turnloop deadline too; it was a tokio sleep.
- **`https:`.** `perry_tls_session::TlsSession` runs above the same socket
  handle, with the verifier `tls_client` already built from Node's options (CA,
  `servername`/SNI, `rejectUnauthorized`, `checkServerIdentity`, PKCS#12 client
  identities). Configs are cached per option identity, so TLS session
  resumption still works.
- **Keep-alive.** An Agent with `keepAlive` reuses physical connections, using
  the knobs reqwest's per-agent pool used (`maxFreeSockets`, `keepAliveMsecs`).
  A connection goes back to the pool only after the decoder reports `End` and
  `reusable()`. A reused connection that dies before any response byte is
  retried once on a fresh one. `agent.destroy()` closes its idle connections.
- **`NODE_USE_ENV_PROXY=1`.** An `http:` target is sent in absolute-form
  through the proxy; an `https:` target goes through a `CONNECT` tunnel. The
  proxy URL's credentials become `Proxy-Authorization`.
- **`TE: trailers`, `Expect: 100-continue` and `Connection: Upgrade`** now run
  on the codec's `Event::Trailers` / `Informational` / `Upgrade`. A `101` hands
  the live handle to `net` with `turnloop_net::transfer`, as the server's
  upgrade does. The old modules keep only their predicates and parsers.
- **Off-loop threads.** A thread that does not own its agent's loop posts the
  request to the thread that does. A host with no loop at all reports
  `ENOTSUP`.

Changes you can observe, each toward Node:

- `res.statusMessage` is now the server's own reason phrase, not the canonical
  one.
- Unknown methods go out as written; reqwest sent them as `GET`.
- A caller's header names keep their case.
- `timeout: 0` means no timeout.
- `https` offers no ALPN, so there is no accidental HTTP/2.
- `https` requests get `'continue'` too.
- `req.destroy()` / `abort()` close the socket.
- Connect failures read `connect ECONNREFUSED 127.0.0.1:1` (lane 1 had dropped
  the address), and a close before the response head is `socket hang up` /
  `ECONNRESET`.
- TLS verification failures carry Node's `.code`
  (`UNABLE_TO_VERIFY_LEAF_SIGNATURE`, `ERR_TLS_CERT_ALTNAME_INVALID`, …).
  Before, they were an uncoded string.
- There is no 30-second default timeout any more (reqwest applied one; Node
  does not).

Also fixed from lane 1: bytes of a response head (or chunk-size line) split
across two reads are kept for the next read instead of dropped.

Still on tokio in this crate (the one remaining `perry-ext-http -> tokio`
edge): the `agent.createConnection` / `createSocket` exchange
(`client_connect_override.rs` polls the raw-net vtable), and the keep-alive
socket facade's 40 ms idle-expiry sleep in `agent.rs`.

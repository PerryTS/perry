### turnloop — a public TLS client for a turnloop socket, and TLS for the four database drivers and `http2.connect`

P5 put TLS *above* the turnloop socket rather than beside it, which is what made
`socket.upgradeToTLS` possible without a descriptor handoff. The server half of
that work was public — `install_server_session`, the one Perry's HTTP/2 server
scored h2spec 147/147 through — but the client half was not:
`begin_client_upgrade` was `pub(crate)`, took `perry-ext-net`'s own
`TlsClientConfigData` (built by reading JS values) and settled a
`JsNativeAsyncCompletion`. Two callers needed it and neither could reach it.

**`crates/perry-tls-turnloop`** is that client, extracted and made
binding-agnostic. A caller hands over `TlsClientOptions` — servername, an **ALPN
list**, `rejectUnauthorized`, trust material — and gets a `TlsClientTransport`
bound to one turnloop handle, plus `TlsFacts` back when the handshake completes:
the negotiated ALPN protocol, the peer chain, and the RFC 5929
`tls-server-end-point` digest of the verified leaf. ALPN is in the contract
rather than bolted on because the callers disagree about it —
`http2.connect` offers `h2` alone, a database client offers nothing, `fetch`
offers both — and an installer that could not express all three is how the
`pub(crate)` one came to be shaped for exactly one caller. The state machine
itself is `perry-tls-session`'s, which gains `peer_certificates()` and
`tls_server_end_point()`; there is still one copy of it.

**`perry-db-turnloop`** grows `Registry::connect_with_tls` and two `DbCore`
methods, `take_tls_request` and `tls_established`. All four protocol crates
already asked for the upgrade the same way — an `UpgradeTls` event — and already
had the acknowledgement; only the host had nothing to give them. They disagree
about *when*: `turnloop-redis` and `turnloop-mongodb` raise it from
`transport_connected`, before a protocol byte, while `turnloop-postgres` raises
it after the one-byte `S` answer to its `SSLRequest` and `turnloop-mysql` after
the server greeting and its own `SSLRequest` packet. The driver does not need to
know which: it flushes whatever plaintext the core still owes — which is what
puts both `SSLRequest`s on the wire unencrypted — and *then* installs the
session.

So:

* **`pg`** reads an `ssl` option (`true`, a string, or `{ rejectUnauthorized, ca,
  servername }`) and connects with `SslMode::Require`. Never `Prefer`: a client
  that asked for TLS and silently got none would send its password in the clear.
  **SCRAM-SHA-256-PLUS** comes with it — the binding derives the channel binding
  from the verified leaf, and a `plus` request with no digest in hand is refused
  rather than answered with `ChannelBinding::unsupported()`, which would be a
  silent downgrade of exactly the thing the `p=` header exists to prevent.
* **`mysql2`** reads the same `ssl` option, and now also parses `ssl-mode=` out
  of a `mysql://…?…` URI — which it previously swallowed into the database name.
* **`ioredis`** no longer declines a `rediss://` client. That is a repair, not a
  new capability: `REDIS_TLS` defaults to `true`, so the **default**
  `new Redis()` declined onto a legacy path whose `redis` dependency has no TLS
  backend compiled in, and failed.
* **`mongodb`** no longer declines a `tls=true`/`ssl=true` URI. `+srv`,
  `replicaSet=`, several hosts, `compressors=` and the per-connection `tls*`
  keys still do.

**`http2.connect('https://…')`** now works, having never worked:
`parse_authority` returned port **80** for every scheme and the fallback opened a
cleartext `tokio::net::TcpStream`, so the HTTP/2 preface went to an HTTPS
listener and the peer answered `received corrupt message of type
InvalidContentType`. It takes the turnloop path with a real client session,
`h2` in ALPN, and `options.ca` / `options.rejectUnauthorized` honoured;
`session.alpnProtocol` reports what was negotiated instead of always `"h2c"`.
`perry_ext_net::turnloop_tls_io::install_client_session` is the public installer
that unblocked it.

Three defects fixed in passing, each reachable before this change:

* **`perry_ffi::object_field_by_name` held an unrooted heap pointer across an
  allocation** — it took `*mut ObjectHeader` out of the receiver, then called
  `alloc_string(key)`, then dereferenced it. A moving collection in that window
  leaves a stale pointer, the `#7184`/`#7192` shape. The receiver is now parked
  in a `TransientRootScope` and re-read after the allocation.
* **A silent TLS downgrade on both sqlx paths.** `ssl` only became parseable
  with this change, so a client that asked for it and then *declined* the
  turnloop transport — a Unix-socket host, a thread with no loop — would have
  connected in plaintext. `to_url` now emits `sslmode=verify-full` /
  `ssl-mode=REQUIRED`, and this crate's sqlx has no TLS backend, so it refuses.
* **`perry-ext-http` did not compile** on this branch: the merge that brought
  `tcp_listen`'s split `reuse_port`/`noDelay` arguments onto it left the HTTP/2
  listener's call site at six arguments.

No tokio manifest edge is removed, and the inventory still reports 38. Every
group-B edge has a second, non-TLS reason to decline (a thread with no loop of
its own; a Unix-socket host for `pg` and `mysql2`; SRV and replica sets for
`mongodb`), and group D's `h2` still serves the paths HTTP/2 declines on. What
moves is the *blocker*: `scripts/tokio_inventory.json`'s annotations for those
nine edges are re-written to say what is actually left.

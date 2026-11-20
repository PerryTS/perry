`node:http2`'s client no longer links the `h2` crate. `perry-ext-http`'s
`h2` manifest edge is gone, taking the tokio inventory from 22 edges to 21.

The inventory recorded this edge as blocked on a missing capability: "the TLS
**client**: `perry_ext_net::turnloop_tls_io` exposes `install_server_session`
publicly but only `begin_client_upgrade` (`pub(crate)`…), so
`http2.connect('https://…')` has no way to install a client session on a
turnloop socket." That had already been built and wired —
`turnloop_tls_io::install_client_session` is public, takes neither
perry-ext-net's `TlsClientConfigData` nor a `JsNativeAsyncCompletion`, and its
own doc comment names `http2.connect('https://…')` as the reason it exists.
`turnloop_h2::conn::connect_client` has been installing a real client session
with `h2` in ALPN since it landed. The blocker text, not the tree, was stale.

What that left behind was a fallback with no remaining reason to exist, and
it is deleted rather than kept:

* `h2::client::handshake` on a private `current_thread` tokio runtime, built
  per `http2.connect`, plus a **second** private runtime built per
  `session.request()`.
* `connect_h2_stream`, which returned a bare `tokio::net::TcpStream` and
  ignored `secure` entirely — so on that path `https://` opened a CLEARTEXT
  socket and sent an HTTP/2 preface at a TLS listener. The surface the
  recorded blocker was about could not work there at all.
* `Http2SessionHandle::sender`, the `Arc<Mutex<Option<SendRequest>>>` that
  concurrent `session.request()` calls raced for.

An agent that cannot reach a loop — a second thread acting for an agent
another thread already owns, the `tokio-wait-driver` A/B arm, a host where
`Loop::new` failed — now gets an `'error'` from `http2.connect` saying so, and
a `session.request()` on such a session errors its stream instead of hanging.
That is a real narrowing of a path that only ever carried cleartext, and it
follows the rule `perry-ext-ws` set when it deleted its own tokio transport:
write the narrowing down, rather than keep a fallback nobody exercises.

The `perry-ext-http` → `tokio` edge survives, and its inventory entry now says
why in its own terms instead of pointing at "the union of the rows above":
`reqwest` + `tokio-rustls` (the `node:http` / `node:https` CLIENT, plan C) and
`hyper` + `hyper-util` (the declining HTTP/1.1 and HTTP/2 SERVER, plan A). The
plan's own row D already said this edge was "its last, once C and E are done".
`python3 scripts/tokio_inventory.py --list` puts the crate at 99 tokio-shaped
source sites, down from 110; `http2_server/session.rs` — the whole HTTP/2
client — now contains none at all, and what is left in `http2_server.rs` is the
hyper accept loop plan A owns.

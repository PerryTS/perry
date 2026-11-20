Fixed three `node:http`/`node:https` client-side defects. The client `IncomingMessage` now exposes
`rawHeaders`/`httpVersion`/`httpVersionMajor`/`httpVersionMinor`/`complete` (previously `undefined` on both the
typed and dynamically-dispatched surface); `httpVersion*`/`complete` fall back to the server-side accessor when
the handle is a server `IncomingMessage`, since the codegen native table shares one `class_filter` namespace
across client and server (#10467 — `rawHeaders` header-name casing on the pooled reqwest transport is a known
remaining gap, documented in the PR). `http.request`'s client now fires `req.on('upgrade', (res, socket, head) =>
...)` on a `101 Switching Protocols` response instead of delivering it as an ordinary `'response'`: an upgrade
request speaks HTTP/1.1 over a raw socket (mirroring the existing trailer-aware bypass), and on `101` adopts the
stream as a `net.Socket` via `perry_ext_net::adopt_upgraded_tcp_stream` — write, inbound data delivery, and the
`head` Buffer (always a Buffer, never `undefined`, even zero-length) all match Node (#10468). The request option
`options.createConnection` (distinct from `agent.createConnection`) is now honored when the request has no
explicit Agent, taking the same raw-socket path the Agent-level override already used (#10469).

Follow-up (unrooted-local-shape ratchet, caught before merge): `build_raw_headers_array`
(`res.rawHeaders`, added for #10467 above) held its result array's raw pointer in a plain local across
`alloc_string`/`js_array_push` calls that can allocate and therefore collect — a stale-pointer-after-collection
shape (`scripts/unrooted_local_shape.py`), not merely a scanner nit. Rooted it through
`perry_ffi::TransientRootScope::root_nanbox` and re-derive the pointer via `.get()` after each allocating call
instead of reusing the pre-call copy, matching the pattern already used throughout this crate (e.g.
`agent.rs`, `client_events.rs`). Found and fixed the same pre-existing shape in the neighboring
`set-cookie` array builder in `build_response_headers_object` (unrelated to this PR's diff, same file); extracted
it into its own top-level `build_set_cookie_array` so the rooting lines aren't deep enough for `rustfmt` to wrap a
`let` binding across lines, which had been hiding the second half of the binding from the ratchet's
line-oriented scanner. `scripts/unrooted_local_shape.py --check` now reports 558 (down from the pre-PR baseline
of 561; response_headers.rs's own ceiling drops from 1 to 0).

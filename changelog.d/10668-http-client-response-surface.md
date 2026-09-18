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

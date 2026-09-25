Fixed `http.Server`/`https.Server` `'connection'` listeners receiving no
argument — Node passes the accepted `net.Socket`, the same object later
exposed as `req.socket` for every request on that connection, and libraries
key open connections by `conn.remoteAddress + ':' + conn.remotePort` and drop
them on the socket's `'close'` (e.g. the `server-destroy` package, used by
Astro's `@astrojs/node` adapter). Perry fired the listener with no args
(`emit_no_arg_to_listeners`); user code reading `conn.remoteAddress` then
threw `TypeError: Cannot read properties of undefined`.

Root cause / fix: `PENDING_CONNECTION_EVENTS` queued only the server handle.
Every accepted turnloop connection now also builds a connection-socket object
at accept time (`alloc_connection_socket`, reusing the existing
`IncomingMessage` type rather than a new one — it already has
`remoteAddress`/`remotePort`, an `on`/`once`/`destroy` dispatch surface, and a
`'close'` emit), and the queue carries `(server_handle, socket_handle)`. The
`'connection'` listener now receives that socket; `finish_request` assigns it
as `req.socket`/`req.connection` for every request on the connection, through
the override mechanism `#4904` already added for `new
http.IncomingMessage(socket)`, so `req.socket === conn`. When the connection's
terminal `Closed` completion arrives, its socket handle is queued (the
completion sink cannot run JS) and the pump fires `'close'` on it before
draining the server's own deferred `'close'` callback. HTTP/2's accept path
gets a socket argument too, through the same shared queue, without wiring
`req.socket` on H2 streams (out of scope: `Http2ServerRequest` has no such
alias).

Known gaps left out of scope: `res.socket`/`res.connection` still self-refer
instead of aliasing the connection socket; `conn.destroy()` fires the socket's
`'close'` immediately without tearing down the underlying TCP connection (it
reuses the generic `IncomingMessage.destroy()`, which never closed the
connection for a plain `req.destroy()` either); the `http2` ALPN-to-HTTP/1.1
handoff (`adopt_alpn_http1`) still never fires `'connection'` at all (a
pre-existing gap, unrelated to this fix — `req.socket` is still wired there
for consistency).

Test: `test-parity/node-suite/http/server/connection-event-socket.ts`
(argument shape, `req.socket === conn`, `'close'` firing and connection
bookkeeping via the `server-destroy` idiom, event ordering).

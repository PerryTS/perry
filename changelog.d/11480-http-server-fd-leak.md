**http: the server closes each connection's socket again, so sequential `http.get` round trips no longer leak one fd each (#11452).** The turnloop HTTP/1.1 server (`perry-ext-http/src/server/turnloop_serve/conn.rs`) ended a connection with `finish_and_close`, which only submitted a write-side shutdown. The comment said "close once it has drained", but the completion sink had no `NET_SHUTDOWN` arm, so nothing closed the handle after the shutdown finished. The only other path to a close was the peer's EOF, and `on_eof` returns early once a connection is `closing`. So every connection whose client hung up first (a keep-alive client that disconnects, such as `curl` or an `agent: false` `http.get`), or whose FIN crossed the server's shutdown, kept its descriptor for the life of the process. The socket was shut down in both directions but never `close()`d, which is why `ss` listed none of them. #11205 put the client on turnloop with one connection per request for the implicit agent, where reqwest had pooled one connection, and that exposed the leak: 301 fds after 300 in-process requests, then `connect ECONNREFUSED` after about 1,015 at `ulimit -n 1024`.

- The server now handles `NET_SHUTDOWN`. It closes the handle once the shutdown it asked for has completed, which matches Node's `destroySoon()` (`end()`, then `destroy()` on `'finish'`). A WebSocket connection, as in `ws`, closes on whichever comes second: its own FIN going out or the peer's FIN arriving. The WebSocket path had the same leak once the close handshake had finished. A TLS `close_notify` shutdown that cannot be submitted now falls back to a close instead of being silently dropped.
- The client: `agent: false` now sends `Connection: close`, as Node's throwaway `keepAlive: false` agent does. Perry used to send `keep-alive`. The header is added only to the dispatched copy, so `req.getHeader('connection')` stays `undefined`, as in Node. A caller's own `Connection` header still wins.

Measured on Linux against Node 26.5.1, open fds before → after:

| probe | before | after | Node |
|---|---|---|---|
| 300 × `http.get({agent:false})`, in-process server | 7 → 307 | 7 → 7 | 23 → 23 |
| 2,000 × `http.get({agent:false})` | ECONNREFUSED at ~1,015 | completes, 7 → 7 | completes |
| 200 × `curl` against a perry server | 7 → 207 | 7 → 7 | 23 → 23 |

New gap tests: `test_gap_http_sequential_get_no_fd_leak`, `test_gap_http_server_closes_after_client_fin`, `test_gap_https_server_closes_after_client_fin` and `test_gap_ws_attached_server_socket_closes`. All four fail on the base and match Node with the fix.

Still divergent (not in scope): with no `agent` option, Node's `http.globalAgent` pools and reuses one socket, while perry's turnloop client still opens one connection per request. Both ends now close those connections, so descriptors stay bounded. A perry `https.Server` also emits no `'secureConnection'` event, so the https test counts `'connection'` instead.

## Keep HTTP servers live during microtask-heavy startup

The single-thread async wait driver's fast path now counts a listening external
HTTP server as native work. Previously it only drove Tokio for blocking tasks
and HTTP clients. If JavaScript kept queuing microtasks after `server.listen()`,
`js_wait_for_event` stayed on that fast path while the server's accept task sat
unpolled. The production Next App Route provider gate therefore depended on the
accept task winning its initial spawn race and usually parked without serving
its first request.

`perry-stdlib` now gives the reactor a bounded turn while an external HTTP
server is active, and its async-bridge unit tests pin that liveness condition.

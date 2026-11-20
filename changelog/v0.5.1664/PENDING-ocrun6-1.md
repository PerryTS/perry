Fix HTTP client response delivery from workers. The issuing request now owns
every queued response, error, body, timeout and abort event; each agent drains
only its own events through the existing agent pump. The existing in-flight RAII marker owns
the keepalive lifetime independently of the request registry. HTTP activity and
mutable GC roots follow the same agent ownership. This replaces process-wide delivery that
could invoke a worker's callback on the main thread, where its module values
were unset and reads such as response.headers returned undefined.

No new registry, cache or alternate delivery path. The existing request and
Agent handles supply ownership for the existing event queue. Regression coverage
checks response headers and callback context for both http.get's callback and
request.on("response"), against Node, plus queue isolation and FIFO order.

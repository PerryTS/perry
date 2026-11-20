**node:stream: a Readable resolves `_read` when it is read, so fastify `inject()` with a body completes**

fastify's `app.inject({ method: "POST", payload })` printed nothing and the
process exited 0: the request-body promise never settled, so `main()` was
dropped with the event loop empty. light-my-request's `Request` calls
`Readable.call(this, …)` and only then assigns `this._read = readEverythingElse`.
Perry captured `_read` once, inside the Readable constructor (from
`options.read` or the prototype chain at that moment), so the later
assignment was never seen and the body was never read.

Node's `Readable.prototype.read` calls `this._read(n)` at read time. Perry now
resolves the method the same way when the stream is first read: an own
`_read` (assigned after construction) wins, then `options.read`, then a
`_read` on the prototype chain, including one added after the instance was
created. The fastify/inject workload now prints node's checksum.

Test: `test-files/test_gap_readable_read_resolved_at_read.ts` (fails on the
old runtime: prints nothing).

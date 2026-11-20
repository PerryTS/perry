Reading a body stream through the default-reader drain no longer leaves the
read's rejection unhandled. `drain_readable_into_bytes` creates a reader read,
polls its settled state directly and drops it; when the stream ended in an
error, that read rejected with the same error the caller already reports
through `Request#text()` / `Response#text()`, so a caught error still printed
`Uncaught (in promise) Error: ...` and exited 1
(`node-suite/fetch/request/async-iterable-body`, #9202).

The private read promise is now marked with `js_promise_mark_internally_handled`
(Node's `markPromiseAsHandled`) — the same mark the stream machinery puts on its
own internal `closed` / `ready` promises (#1545): the drain is that promise's
only observer and it is never handed to user code, so no user-visible rejection
can be masked. A regression test drains an errored stream and asserts the
unhandled-rejection tracker gains no entry.

An uncaught exception in a `worker_threads` Worker now ends that worker only,
as in Node: the parent gets an `error` event with a clone of the thrown value,
then `exit` with code 1, and keeps running. This covers a throw in a message
handler, in a timer, or while the worker module loads, and an unhandled
rejection in the worker (Node's default `--unhandled-rejections=throw`), which
was silently dropped before. An `error` with no listener on the Worker is
thrown in the parent, like an EventEmitter. Also fixed a closure passed to
`parentPort.on("message")` that was not a GC root: after a collection in the
worker the next message called a stale pointer (`value is not a function`).

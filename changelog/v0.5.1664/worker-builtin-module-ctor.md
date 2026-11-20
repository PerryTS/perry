`new ns.Worker(...)` on the `worker_threads` namespace reached as a value now
starts a real Worker.

`process.getBuiltinModule("node:worker_threads").Worker`, a `Worker` read from
a stored namespace, or any other `Worker` the compiler cannot see at the call
site used to fall through to the generic native construct path and return an
empty object, so `worker.on` was not a function. A program that reaches the
namespace lazily (upm's `builtin.workers`) therefore never got a thread.

The compiler now records every worker entry it compiles (the module's init
symbol under its absolute paths), and the entry module's `main` registers them
with a run-time worker entry table (`js_worker_threads_register_entry`) before
any module runs. A Worker the compiler could not match at its call site calls
`js_worker_threads_worker_new_by_spec`: the namespace constructor, an
unresolved lexical `new Worker(x)`, and the "no candidate matched" branch of a
partly resolved one. It normalizes the filename as Node does (a `file:` URL, an
absolute path, or a `./` / `../` path relative to the current directory;
otherwise `ERR_WORKER_PATH`, `ERR_INVALID_URL_SCHEME` or
`ERR_INVALID_ARG_TYPE`), finds the compiled entry and starts it.

A filename with no compiled entry throws `Error [ERR_WORKER_NOT_COMPILED]`
synchronously from the constructor. Node would emit an asynchronous `error`
event instead; the synchronous throw is easier to diagnose and keeps a caller's
`try` fallback working. An `eval: true` Worker whose source is built at run
time throws `ERR_WORKER_EVAL_NOT_COMPILED`.

`threadId` read through the namespace object was always `0`. It now reads the
current thread's Worker id, so it is `1..n` inside workers, as in Node, for the
namespace object and for named imports.

Tests: `test_gap_worker_namespace_ctor` (parity) and
`test_gap_worker_not_compiled` (expected output, because Node reports a missing
file asynchronously).

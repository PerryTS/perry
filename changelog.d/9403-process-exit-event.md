
### Fixed

- **`process.on("exit", …)` handlers now run.** They never ran at all: the
  generated event-loop epilogue emitted `beforeExit` and then went straight to
  cleanup, and nothing anywhere in the runtime ever emitted `exit`. The
  `process` EventEmitter accepted the registration, kept the listener alive and
  rooted it for the GC — the listener was simply never called, on any exit
  path, with no error and no diagnostic.

  This is silent data loss, not a cosmetic gap: `exit` is where a program does
  its last synchronous flush. claude-code registers **17** `exit` handlers, and
  under Perry a `claude --bare -p hi` session transcript came out with 1 line
  where node writes 5 — the queue-op enqueue/dequeue records, the user message
  and the assistant message were all dropped, leaving `--resume` / `--continue`
  with almost nothing to resume. With 1 MB of stdin the same run wrote
  1,195,651 bytes against node's 2,409,463.

  Node's exit sequence (`handleProcessExit`) is now one runtime function,
  `process::run_process_exit_sequence`, driven from every path that ends the
  process:

  - `crates/perry-codegen/src/codegen/entry.rs` — the natural-drain epilogue
    calls it after `beforeExit` and its microtask drain.
  - `crates/perry-runtime/src/process/env_misc.rs` — `process.exit()` runs it
    before terminating, and so does the fatal-path terminator
    `exit_after_current_thread_collection_teardown` (uncaught exception with an
    `uncaughtException` listener that rethrows, unhandled rejection).
  - `crates/perry-runtime/src/exception.rs` — an uncaught throw with no open
    `try` runs it *before* printing its report, which is the order node uses.
    The listeners are JS, so the fatal branch was lifted out of the
    `with_exception_state` access it used to run under.
  - `crates/perry-runtime/src/os/os_process_emitter.rs` — `js_process_emit_exit`
    does the emit itself, guarded to fire at most once. The guard is
    load-bearing: a listener may call `process.exit()` or throw, and node's
    answer to both is that the listeners *after* it never run.

  The sync-only half of the contract needed no suppression machinery, only the
  right splice point. Every caller terminates — or returns out of generated
  `main` — as soon as the emit returns, and nothing past it ticks the timer,
  `setImmediate` or `nextTick` queues, so a `writeFileSync` in a listener lands
  while a `setTimeout` scheduled beside it is simply never given a turn. The
  one piece of async work node *does* honour here is V8's microtask checkpoint
  after the emit returns to the top level, so the natural-drain arm ends with a
  promise-jobs-only drain: a `.then` queued by a listener runs, after every
  listener, and only on that path.

  Two smaller divergences in the same epilogue fell out of pinning it against
  the oracle:

  - `beforeExit` was emitted with a literal `0`. Node passes the code the
    process is about to leave with, so `process.exitCode = 5` made every
    `beforeExit` listener see the wrong number.
  - The status a listener sets is now honoured. Node re-reads
    `process.exitCode` after the listeners run, on every path: a handler
    assigning `9` turns a natural exit, a `process.exit(3)` and an uncaught
    throw all into status 9. Perry exited 5 where node exits 9.

  `process.exitCode` is published before the emit exactly where node publishes
  it — an explicit `process.exit(3)`, and the fatal paths, which force `1` even
  over an already-set code — and left alone on natural drain and a bare
  `process.exit()`, where a listener reading it must still see `undefined`.

  Validation: `test-files/test_gap_9403_process_exit_event*.ts` — three
  programs, one per process status (natural 0, explicit 3, listener-rewritten
  9) — byte-compared against node 26.5.1, covering handler order, the code
  argument and its arity, `once` / `prependListener` / `removeListener`,
  `beforeExit` firing first and being skipped on an explicit exit, a
  `writeFileSync` + read-back inside a handler, and `setTimeout` /
  `setImmediate` / `nextTick` / promise jobs. On unfixed `main` all three
  diverge — the natural-drain program prints 2 of node's 8 lines and the
  `exitCode` program exits 5 instead of 9. `perry-runtime --lib` 2907 passed /
  0 failed.

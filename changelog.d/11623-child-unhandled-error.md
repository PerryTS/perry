**child_process: an `error` event with no listener now throws, as in Node (#10730).** When `spawn()` or `fork()` fails (for example `ENOENT` for a missing binary), Perry emits a deferred `error` event on the ChildProcess. If no `error` listener was registered, Perry used to drop the error, fire `close`, and let the program continue. Node's EventEmitter throws an unhandled `error` event instead: the process dies with `Unhandled 'error' event` before `close` fires.

That gap is why `test_gap_9592_child_timeout_threads` passed for Perry on macOS while Node crashed. The fixture spawned `/bin/true`, which macOS does not have, and never listened for `error`. #10730 guessed Perry was resolving the missing absolute path through `PATH`. It was not: Perry reported `ENOENT` correctly, and nothing was listening for it.

The emit now reports whether any listener took the error (`failed_spawn::emit_spawn_error`). If none did, the error object is thrown as-is from the `setImmediate` callback that delivers it, after that frame's handle scopes have dropped, so `uncaughtException` and the uncaught-exit path see the same object Node rethrows. `close` is not scheduled in that case. A handled error behaves as before. No JS-callable native signature changed.

Regression coverage:
- `test-files/test_gap_10730_child_unhandled_error.ts` checks three cases: a handled error, an unhandled one, and a listener that was removed. It is byte-identical to Node with the fix. Without the fix Perry prints `close without listener` and no `uncaught` lines.
- `child_process::failed_spawn::tests::spawn_error_{without,with}_listener_is_{unhandled,handled}` are the unit-level half.

`perry-runtime --lib` single-threaded: main 4704 passed, 0 failed. This change: 4706 passed, 0 failed (the two new tests).

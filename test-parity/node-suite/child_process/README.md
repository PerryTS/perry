# node:child_process parity cases

Focused compatibility cases for `node:child_process` process execution APIs.

## Upstream comparison

Reviewed on 2026-07-16 against primary repository snapshots:

- Node.js [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test): the 126 `test/parallel` and `test/sequential` files whose names contain `child-process`, with particular attention to validation, cwd/env, spawn events, stdio flushing, sync result shapes, timeouts, AbortSignal, and advanced serialization.
- Deno [`f8a17c8`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/child_process_test.ts): its selected spawn/error/event, cwd/env, stdio, sync, and IPC compatibility coverage.
- Bun [`5d350cc`](https://github.com/oven-sh/bun/tree/5d350cc17525a493fcb55b0a014f75af7c414580/test/js/node/test): its selected Node child-process parallel/sequential tests, including validation, lifecycle, buffering, serialization, and platform-specific cases.

The granular suite now has 34 fixtures. The eight expansion fixtures add:

- deterministic option and embedded-null validation;
- sync cwd, environment coercion/omission, binary input, and result output;
- async cwd/env propagation, spawn/stream/exit/close ordering, and `execFile` callback ordering;
- 128 KiB stdout/stderr draining and 192 KiB stdin flushing;
- portable JSON IPC plus basic advanced Buffer, Map, and BigInt serialization.

All new subprocesses invoke a controlled `node -e` program or a temporary local helper. Temporary files/directories are PID-scoped and removed in `finally` blocks.

## Measured status

With the repository-pinned Node 26.5.0 oracle:

```text
./run_parity_tests.sh --suite node-suite --module child_process
28 pass, 6 output mismatches, 0 compile failures, 0 crashes, 34 total
```

Five new fixtures intentionally expose stable Perry differences:

- `validation/null-bytes`: Perry accepts command, file, argument, and fork paths containing `\0` instead of throwing `ERR_INVALID_ARG_VALUE`.
- `validation/options`: Perry accepts invalid options that Node rejects with `ERR_INVALID_ARG_TYPE`, `ERR_INVALID_ARG_VALUE`, `ERR_OUT_OF_RANGE`, or `ERR_UNKNOWN_SIGNAL`.
- `async/callback-ordering`: Perry invokes the `execFile` callback but returns no `ChildProcess` handle, so stream and lifecycle listeners cannot be attached.
- `async/cwd-env`: the child exits successfully but Perry captures no stdout when custom cwd/env options are used.
- `serialization/basic`: JSON, Buffer, and BigInt round trips match; advanced serialization loses Map identity/content.

The pre-existing `async/abort-signal` fixture remains the sixth mismatch.

## Stopping judgment and exclusions

Further direct ports in the upstream corpus are redundant with current fixtures or belong in separate platform/slow feature assessments:

- fork/IPC races, disconnect/ref timing, fd/handle and socket/server transfer, diagnostics-channel integration, and cluster/dgram interaction;
- signal-number matrices, kill races, UID/GID privilege behavior, detached process groups, and POSIX scheduler-sensitive cleanup;
- Windows batch files, command quoting, overlapped stdio, `windowsHide`, and `windowsVerbatimArguments`;
- shell discovery/quoting and arbitrary external-command behavior;
- EMFILE/resource exhaustion, large-buffer IPC framing, host-object/circular serialization, and multi-megabyte maxBuffer stress.

The suite stops here because those cases require platform gates, elevated privileges, network handles, shell assumptions, or longer stress windows. They should not be mixed into the deterministic portable floor.

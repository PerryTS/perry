# node:child_process parity cases

Focused compatibility cases for `node:child_process` process execution APIs.

## Upstream comparison

Reviewed on 2026-07-16 against primary repository snapshots:

- Node.js [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test): the 126 `test/parallel` and `test/sequential` files whose names contain `child-process`, with particular attention to validation, cwd/env, spawn events, stdio flushing, sync result shapes, timeouts, AbortSignal, and advanced serialization.
- Deno [`f8a17c8`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/child_process_test.ts): its selected spawn/error/event, cwd/env, stdio, sync, and IPC compatibility coverage.
- Bun [`5d350cc`](https://github.com/oven-sh/bun/tree/5d350cc17525a493fcb55b0a014f75af7c414580/test/js/node/test): its selected Node child-process parallel/sequential tests, including validation, lifecycle, buffering, serialization, and platform-specific cases.

The granular suite now has 49 fixtures. The 23 expansion fixtures add:

- deterministic embedded-null, option, fork overload, stdio layout, IPC placement, and kill-signal validation;
- sync cwd/default-env/value coercion, missing-cwd errors, optional args, binary input views, encoding aliases, argument preservation, option immutability, `windowsHide`, shell execution, and infinite maxBuffer behavior;
- async spawn metadata/default-env/optional-args behavior, option immutability, exit-code matrices, `exec`/`execFile` success shapes, callback and stream lifecycle ordering, text/binary stdin, stream piping, fd3, and bounded stdio flushing;
- portable JSON IPC plus advanced Buffer, Map, Set, Date, RegExp, Uint16Array, BigInt, and Error serialization.

All new subprocesses invoke a controlled `node -e` program or a temporary local helper. Temporary files/directories are PID-scoped and removed in `finally` blocks.

## Measured status

With the repository-pinned Node 26.5.0 oracle:

```text
./run_parity_tests.sh --suite node-suite --module child_process
39 pass, 9 output mismatches, 0 compile failures, 0 crashes, 1 host Node failure, 49 fixtures

NODE_BIN="$(command -v node)" python3 scripts/node_suite_run.py \
  "$PWD/target/release/perry" "$PWD" child_process
40 pass, 9 output mismatches, 49 total
```

Eight expansion fixtures intentionally expose stable Perry differences:

- `validation/null-bytes`: Perry accepts command, file, argument, and fork paths containing `\0` instead of throwing `ERR_INVALID_ARG_VALUE`.
- `validation/options`: Perry accepts invalid option/fork/stdio/IPC/signal inputs that Node rejects with `ERR_INVALID_ARG_TYPE`, `ERR_INVALID_ARG_VALUE`, `ERR_INVALID_SYNC_FORK_INPUT`, `ERR_IPC_ONE_PIPE`, `ERR_IPC_SYNC_FORK`, `ERR_OUT_OF_RANGE`, or `ERR_UNKNOWN_SIGNAL`.
- `async/callback-ordering`: Perry invokes the `execFile` callback but returns no `ChildProcess` handle, so stream and lifecycle listeners cannot be attached.
- `async/cwd-env`: the child exits successfully but Perry captures no stdout when custom cwd/env options are used.
- `async/spawn-behavior`: process metadata/status match, but async stdout is empty for direct, inherited-env, optional-args, and immutable-options probes.
- `async/stdio-dataflow`: writable calls and exit statuses match, but Perry does not expose readable state/end transitions, return stdout for text/binary/pipe flows, or create fd3.
- `sync/error-shell`: missing-cwd error objects lack Node's constructor/code shape and basic shell execution is treated as ENOENT.
- `serialization/basic`: JSON plus advanced Buffer, Date, Uint16Array, and BigInt round trips match; Map, Set, RegExp, and Error brands/content are lost.

The pre-existing `async/abort-signal` fixture remains the ninth mismatch. The focused harness's single host Node failure is the pre-existing detached-process-group fixture intermittently ending with Node's unsettled top-level-await exit 13; the baseline runner measures it cleanly.

## Stopping judgment and exclusions

Further direct ports in the upstream corpus are redundant with current fixtures or belong in separate platform/slow feature assessments:

- fork/IPC races, disconnect/ref timing, fd/handle and socket/server transfer, diagnostics-channel integration, and cluster/dgram interaction;
- exhaustive signal-number matrices, kill races, UID/GID privilege behavior, detached process groups, and POSIX scheduler-sensitive cleanup;
- Windows batch files, command quoting, overlapped stdio, and `windowsVerbatimArguments`;
- custom-shell discovery/quoting and arbitrary external-command behavior;
- EMFILE/resource exhaustion, large-buffer IPC framing, host-object/circular serialization, and multi-megabyte maxBuffer stress.

The suite stops here because those cases require platform gates, elevated privileges, network handles, shell assumptions, or longer stress windows. They should not be mixed into the deterministic portable floor.

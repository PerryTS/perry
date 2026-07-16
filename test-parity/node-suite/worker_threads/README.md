# node:worker_threads parity cases

Focused compatibility cases for `node:worker_threads` APIs that Perry wires
through its Node-compat stdlib shim.

## Upstream selection

The expansion was compared against these primary-source snapshots:

- Node.js [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel), especially the `test-worker-message-port-*`, `test-worker-message-channel*`, `test-worker-message-mark-as-uncloneable`, `test-worker-invalid-workerdata`, `test-worker-environmentdata`, `test-worker-event`, and `test-worker-broadcastchannel` cases.
- Deno [`f8a17c8`](https://github.com/denoland/deno/tree/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/specs/node/worker_threads) and its [`worker_threads_test.ts`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/worker_threads_test.ts) selection, including port transfer, listener removal/deduplication, `unref`, auto-exit, and broadcast coverage.
- Bun [`0bffb47`](https://github.com/oven-sh/bun/tree/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads), especially [`worker_threads.test.ts`](https://github.com/oven-sh/bun/blob/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads/worker_threads.test.ts) and [`worker-transfer-list.test.ts`](https://github.com/oven-sh/bun/blob/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads/worker-transfer-list.test.ts).

Node `26.5.0`, pinned by this repository, remains the executable differential
oracle. The upstream snapshots above guide case selection rather than changing
that oracle.

## Coverage added

- `structured-clone/`: ArrayBuffer cloning, typed-array backing identity,
  transfer detachment, MessagePort ownership transfer, and transfer-list
  validation/rollback.
- `message-port/`: synchronous FIFO receives, invalid-port validation,
  explicit `start()`, listener deduplication, close ordering, queued-message
  delivery, and `ref`/`unref`/`hasRef` state.
- `worker-lifecycle/`: constructor transfer validation, structured `workerData`,
  environment-data snapshots, `online`/`message`/`error`/`exit` ordering, and
  deterministic termination settlement.
- `transfer-markers/`: marker return/value semantics, inheritance boundaries,
  clone rejection, transfer rejection, and retained ownership after rejection.
- `broadcast-channel/`: same-name fanout/FIFO isolation, sender exclusion,
  typed-array cloning, untransferable-value rejection, and closed-channel
  validation.

Every asynchronous fixture uses a message, close, exit, or promise-settlement
barrier. No added fixture uses a sleep as a completion condition.

## Stopping judgment

The remaining upstream cases were not copied because they are redundant with
the cases above or belong to a separate slow/risky runtime feature:

- Resource-limit enforcement, stdio timing/backpressure, heap snapshots and
  CPU/heap profiling are resource- and platform-sensitive. The pre-existing
  `worker-file/options-diagnostics.ts` keeps surface coverage only.
- Eval/data-URL/ESM loaders, preload/`execArgv`, signals, inspector integration,
  process exit variants, and source maps primarily exercise loader, CLI, or
  process subsystems rather than the worker messaging contract.
- Nested-worker stress, GC/finalization of unreachable ports, shared native
  handles, large message loops, and termination races are scheduler-sensitive
  and need dedicated stress infrastructure.
- SharedArrayBuffer/Atomics cross-agent behavior is a separate runtime feature.
  Single-thread Atomics invariants are not worker-specific, so adding them here
  would duplicate existing language/runtime coverage.
- The four existing `web-locks/` fixtures already cover deterministic surface,
  validation, shared/exclusive ordering, `ifAvailable`, and query snapshots.
  Abort/steal races and cross-agent lock ownership remain scheduler-sensitive.
- The existing `direct-message/` fixtures already cover deterministic
  `postMessageToThread` delivery, rejection, and timeout behavior.

The measured focused result is `20/34`: all 17 pre-existing cases remain green,
three new cases pass, and 14 new cases expose stable diagnostic differences.

The passing additions are `broadcast-channel/fanout-fifo.ts`,
`message-port/start-and-listeners.ts`, and
`worker-lifecycle/termination-ordering.ts`. The diagnostic differences are:

- All four `structured-clone/` fixtures: Perry preserves indexed values but
  loses ArrayBuffer/typed-array brands, does not detach transferred buffers or
  move MessagePort ownership, and accepts invalid transfer lists.
- `message-port/close-ordering.ts`, `receive-fifo-validation.ts`, and
  `ref-state.ts`: closing drops a queued message, invalid receivers return
  `undefined` instead of throwing, and `hasRef()` does not track listener or
  explicit ref state.
- `worker-lifecycle/constructor-validation.ts`, `data-and-environment.ts`, and
  `error-ordering.ts`: invalid constructor payloads are accepted, transferred
  workerData is not detached or type-preserving and environment data is not
  snapshotted, while a worker throw escapes instead of producing ordered
  `error` then `exit` events.
- All three `transfer-markers/` fixtures: primitives are reported as marked,
  nested clone rejection and ArrayBuffer exceptions differ, and marked
  transferables are still accepted.
- `broadcast-channel/close-and-clone.ts`: typed-array branding is lost and
  MessagePort/closed-channel posts are accepted.

The clean measurement used:

```sh
NODE_BIN="$(command -v node)" \
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" \
python3 scripts/node_suite_run.py \
  "$PWD/target/perry-dev/perry" "$PWD" worker_threads
```

It reported `20/34 (58.8%), diff=14`, with no compile failures or timeouts.
The SharedArrayBuffer/Atomics stopping decision is backed by the existing
granular `globals/atomics-*.ts`, `buffer/from/shared-array-buffer.ts`, and
`util/types/arraybuffer-sharedarraybuffer.ts` cases; cross-agent fixtures such
as Node's `test-worker-message-channel-sharedarraybuffer.js` and Deno's
`broadcast_channel_sab.mjs` remain outside this messaging-focused batch. A
broader differential regression check of all six `globals/atomics-*.ts` cases
also remained clean at `6/6`.

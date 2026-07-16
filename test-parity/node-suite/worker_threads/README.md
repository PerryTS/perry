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
  built-in brands, cycles/aliasing, SharedArrayBuffer sharing, multiple views,
  transfer detachment, MessagePort ownership, overloads, and atomic rollback.
- `message-port/`: synchronous FIFO receives, invalid-port validation,
  explicit `start()`, event fields/ports, listener deduplication, close callback
  ordering, transfer-state validation, queued delivery, overload validation,
  and `ref`/`unref`/`hasRef` state.
- `worker-lifecycle/`: constructor transfer validation, structured `workerData`,
  default and built-in workerData, MessagePort/ArrayBuffer transfer, environment
  snapshots, `online`/`message`/`error`/`exit` ordering, ref return values,
  natural exit, and deterministic termination settlement.
- `transfer-markers/`: marker return/value semantics, inheritance boundaries,
  clone rejection, transfer rejection, and retained ownership after rejection.
- `broadcast-channel/`: same-name fanout/FIFO isolation, sender exclusion,
  listener management, typed-array and SharedArrayBuffer cloning,
  untransferable-value rejection, and closed-channel validation.
- `web-locks/`: deterministic pre-aborted requests and lock stealing, extending
  the existing surface, option, query, and shared/exclusive ordering cases.

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
- SharedArrayBuffer aliasing is now covered deterministically through both a
  MessagePort and BroadcastChannel. Cross-agent wait/notify coordination stays
  separate because it requires scheduler-aware runtime infrastructure.
- Web Locks now cover deterministic pre-abort and steal behavior in addition to
  surface, validation, shared/exclusive ordering, `ifAvailable`, and query
  snapshots. In-flight abort races and cross-agent ownership remain excluded.
- The existing `direct-message/` fixtures already cover deterministic
  `postMessageToThread` delivery, rejection, and timeout behavior.

The measured focused result is `24/55`: all 17 pre-existing cases remain green,
seven new cases pass, and 31 new cases expose stable diagnostic differences.

The passing additions are `broadcast-channel/fanout-fifo.ts`,
`broadcast-channel/listener-management.ts`,
`message-port/start-and-listeners.ts`,
`worker-lifecycle/termination-ordering.ts`,
`worker-lifecycle/worker-ref-state.ts`, `web-locks/web-locks-abort.ts`, and
`web-locks/web-locks-steal.ts`. The diagnostic differences are:

- All nine `structured-clone/` fixtures: Perry preserves some indexed values but
  loses built-in/ArrayBuffer/view/SharedArrayBuffer brands and aliasing, rejects
  cycles/BigInt, does not detach or move ownership, and accepts invalid lists.
- Nine `message-port/` diagnostics: closing drops queued data and callbacks,
  invalid receivers/options/transfers do not throw, event `ports` are missing,
  transferred ownership is not enforced, and `hasRef()` does not track state.
- Eight `worker-lifecycle/` diagnostics: invalid constructor payloads are
  accepted; transfers, omitted workerData, Map cloning, environment snapshots,
  post-exit terminate values, and worker error routing differ.
- All three `transfer-markers/` fixtures: primitives are reported as marked,
  nested clone rejection and ArrayBuffer exceptions differ, and marked
  transferables are still accepted.
- Two `broadcast-channel/` diagnostics: typed-array/SharedArrayBuffer branding
  and sharing are lost, while MessagePort and closed-channel posts are accepted.

The clean measurement used:

```sh
NODE_BIN="$(command -v node)" \
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" \
python3 scripts/node_suite_run.py \
  "$PWD/target/perry-dev/perry" "$PWD" worker_threads
```

It reported `24/55 (43.6%), diff=31`, with no compile failures or timeouts.
The SharedArrayBuffer/Atomics boundary is backed by the added same-process
channel cases plus the existing
granular `globals/atomics-*.ts`, `buffer/from/shared-array-buffer.ts`, and
`util/types/arraybuffer-sharedarraybuffer.ts` cases; cross-agent fixtures such
as Node's `test-worker-message-channel-sharedarraybuffer.js` and Deno's
`broadcast_channel_sab.mjs` remain outside this messaging-focused batch. A
broader differential regression check of all six `globals/atomics-*.ts` cases
also remained clean at `6/6`.

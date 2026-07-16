# node:worker_threads parity cases

Focused compatibility cases for `node:worker_threads` APIs that Perry wires
through its Node-compat stdlib shim.

## Upstream selection

The expansion was compared against these primary-source snapshots:

- Node.js
  [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel),
  especially the `test-worker-message-port-*`, `test-worker-message-channel*`,
  `test-worker-message-mark-as-uncloneable`, `test-worker-invalid-workerdata`,
  `test-worker-environmentdata`, `test-worker-event`, and
  `test-worker-broadcastchannel` cases.
- Deno
  [`f8a17c8`](https://github.com/denoland/deno/tree/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/specs/node/worker_threads)
  and its
  [`worker_threads_test.ts`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/worker_threads_test.ts)
  selection, including port transfer, listener removal/deduplication, `unref`,
  auto-exit, and broadcast coverage.
- Bun
  [`0bffb47`](https://github.com/oven-sh/bun/tree/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads),
  especially
  [`worker_threads.test.ts`](https://github.com/oven-sh/bun/blob/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads/worker_threads.test.ts)
  and
  [`worker-transfer-list.test.ts`](https://github.com/oven-sh/bun/blob/0bffb4767dd13b4f5aaf119b13dcf37bd094e2f1/test/js/node/worker_threads/worker-transfer-list.test.ts).

The inventory review covered all 143 `test-worker-*` files across the parallel
and sequential directories in that Node snapshot, all 48 cases in Deno's unit
selection plus its directory fixtures, and the 72 declarations in Bun's two
central worker test files. Cases are represented here by observable contract
rather than copied one-for-one when several upstream files exercise the same
behavior.

Node `26.5.0`, pinned by this repository, remains the executable differential
oracle. The upstream snapshots above guide case selection rather than changing
that oracle.

## Coverage added

- `main-thread/`: complete namespace export availability and types, main-thread
  values, Worker and MessagePort prototype descriptors, constructor brands, and
  the process-level `worker` event, including asynchronous late `process.emit`
  lookup.
- `environment-data/`: key identity, live main-thread values, deletion, worker
  snapshot cloning, built-in Map/Set/Date values, nested-worker inheritance,
  non-cloneable construction rejection, and parent/worker mutation isolation.
- `structured-clone/`: ArrayBuffer cloning, typed-array backing identity,
  built-in brands, cycles/aliasing, SharedArrayBuffer sharing, multiple views,
  multiple SharedArrayBuffers, transfer detachment, MessagePort ownership,
  unsupported URL rejection, overloads, and atomic rollback.
- `message-port/`: synchronous FIFO receives, invalid-port validation, explicit
  `start()`, event fields/ports, listener deduplication, close callback
  ordering, listener removal/`once`, `onmessage` replacement, method receiver
  validation, NodeEventTarget surface/listener validation, iterable MessageEvent
  ports and transfer options, dispatch-time close flushing,
  duplicate/self/closed transfers, atomic clone rollback, transfer-state
  validation, queued delivery, overload validation, first-listener registration,
  pending `once` removal, per-event listener scope, close-callback registration
  order, tamper-resistant listener bookkeeping, post-dispatch `once` registry
  state, non-function `onmessage` clearing, peer-close ref behavior, throwing
  and malformed transfer iterators, accepted empty transfer forms, custom-event
  EventTarget/EventEmitter bridging, max-listener controls, scoped
  `removeAllListeners`, transfer targets, and `ref`/`unref`/`hasRef` state.
- `message-channel/`: module/global identity, construction rules, port brands,
  asynchronous and synchronous delivery, BroadcastChannel synchronous receive,
  and VM-context port movement with closed-port and argument validation.
- `worker-lifecycle/`: constructor transfer validation, structured `workerData`,
  default and built-in workerData, MessagePort/ArrayBuffer transfer, environment
  snapshots, eval/CJS require, file URLs with Unicode names, argv and execArgv
  coercion/validation, inherited/explicit/shared environments, SharedArrayBuffer
  through workerData and postMessage, queued transferred-port receive, thread
  id/name uniqueness and lifecycle, parentPort `onmessage`/ref state, Worker
  EventEmitter surface and listener validation, process restrictions, captured
  stdout/stderr, explicit and `process.exitCode` exits, unsupported paths,
  method receivers, postMessage clone rollback,
  `online`/`message`/`error`/`exit` ordering, ref return values, natural exit,
  and deterministic repeated termination settlement, transferred workerData port
  methods, sequential `SHARE_ENV` sibling visibility, post-exit method safety,
  indexed `SHARE_ENV` keys, eval syntax-error routing, URL postMessage
  rejection, exit-listener exception routing, and custom-stack and non-Error
  serialization.
- `transfer-markers/`: marker return/value semantics, inheritance boundaries,
  clone rejection, transfer rejection, retained ownership after rejection, and
  the distinction between marking a port uncloneable and transferring it, plus
  private, unforgeable, and permanent marker behavior.
- `broadcast-channel/`: same-name fanout/FIFO isolation, sender exclusion,
  listener management/`once`, `onmessage` replacement, name coercion, close/ref
  idempotence, typed-array and SharedArrayBuffer cloning, untransferable-value
  rejection, and closed-channel and method-receiver validation.
- `web-locks/`: deterministic pre-aborted requests and lock stealing, extending
  the existing surface, option, query, callback settlement/cleanup,
  independent-name concurrency, and shared/exclusive ordering cases.
- `direct-message/`: delivery, no-listener, timeout, and handler-failure
  rejection, plus thread id and timeout argument validation.

Every asynchronous fixture uses a message, close, exit, stream-EOF, or
promise-settlement barrier. No added fixture uses a sleep as a completion
condition.

## Stopping judgment

The remaining upstream cases were not copied because they are redundant with the
cases above or belong to a separate slow/risky runtime feature:

- Resource-limit enforcement, stdio backpressure/large-write timing, heap
  snapshots, and CPU/heap profiling are resource- and platform-sensitive. Basic
  captured stdout/stderr and the diagnostic method surface are covered without
  introducing pressure or timing assertions.
- Basic eval/CJS require, execArgv, file-URL construction, process restrictions,
  and exit codes are now covered. Data-URL/ESM/custom loaders, preload chains,
  signals, inspector integration, beforeExit and worker-side uncaught-exception
  variants, and source maps remain separate loader, CLI, or process-subsystem
  work.
- One controlled nested worker now covers environment-data inheritance. Deeper
  nesting stress, GC/finalization of unreachable ports, shared native handles,
  large message loops, and termination races remain scheduler-sensitive.
- SharedArrayBuffer aliasing is covered through same-thread MessagePort and
  BroadcastChannel paths plus cross-agent workerData and postMessage. Atomics
  wait/notify coordination remains separate scheduler-aware infrastructure.
- A cross-worker BroadcastChannel delivery candidate was rejected after Node
  required cross-agent scheduling turns while Perry's absent delivery left no
  deterministic completion event. It belongs with scheduler-aware cross-agent
  infrastructure rather than a sleep/timeout-based fixture here.
- Web Locks now cover deterministic pre-abort and steal behavior in addition to
  surface, validation, shared/exclusive ordering, `ifAvailable`, and query
  snapshots. In-flight abort races and cross-agent ownership remain excluded.
- The existing `direct-message/` fixtures already cover deterministic
  `postMessageToThread` delivery, rejection, handler failures, and timeout
  behavior.

The measured focused result is `32/140`: all 17 pre-existing cases remain green,
15 added cases pass, and 108 added cases expose stable diagnostic differences.

The passing additions are `broadcast-channel/fanout-fifo.ts`,
`broadcast-channel/listener-management.ts`,
`message-port/start-and-listeners.ts`,
`worker-lifecycle/termination-ordering.ts`,
`worker-lifecycle/worker-ref-state.ts`, `web-locks/web-locks-abort.ts`, and
`web-locks/web-locks-steal.ts`. The latest broad pass also adds green coverage
for `environment-data/value-identity.ts`,
`message-port/onmessage-replacement.ts`, `worker-lifecycle/eval-basic.ts`,
`worker-lifecycle/method-receivers.ts`, and
`web-locks/web-locks-independent-names.ts`. Sequential sibling `SHARE_ENV`
visibility in `worker-lifecycle/share-env-siblings.ts` also passes. The
indexed-key variant in `worker-lifecycle/share-env-indexed.ts` passes too. The
accepted optional transfer forms in `message-port/transfer-optional-forms.ts`
also pass. The diagnostic differences are:

- All eleven `structured-clone/` fixtures: Perry preserves some indexed values
  but loses built-in/ArrayBuffer/view/SharedArrayBuffer brands and aliasing,
  rejects cycles/BigInt, does not detach or move ownership, and accepts invalid
  lists.
- Thirty-three `message-port/` diagnostics: closing and dispatch flushing,
  queued data/callbacks, NodeEventTarget surface, listener counts and
  validation, receiver/options/iterables/transfers, MessageEvent construction
  and `ports`, duplicate/self/closed rollback, moved ownership, and `hasRef()`
  state differ.
- Thirty-nine `worker-lifecycle/` diagnostics: invalid constructor payloads and
  paths, execArgv, captured stdio, process restrictions/exit codes, shared
  environments and nested messages, worker/parentPort EventEmitter behavior,
  metadata/unique ids, repeated termination, workerData and postMessage SAB
  sharing, queued port receive, rollback, post-exit values, and error routing
  differ. Basic eval with CJS require passes.
- All five `transfer-markers/` fixtures: primitives are reported as marked,
  nested clone rejection, private/permanent marker enforcement, ArrayBuffer
  exceptions, and marked transferables/uncloneable ports differ.
- Seven `broadcast-channel/` diagnostics: name coercion, once listeners,
  close/ref behavior, typed-array/SharedArrayBuffer branding and sharing,
  MessagePort validation, and closed-channel posts differ.
- Thirteen additional diagnostics cover namespace/prototype completeness,
  environment-data built-ins/inheritance/construction, MessageChannel
  construction, direct-message argument validation, and Web Locks callback
  rejection cleanup.

The clean measurement used:

```sh
NODE_BIN="$(command -v node)" \
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" \
python3 scripts/node_suite_run.py \
  "$PWD/target/perry-dev/perry" "$PWD" worker_threads
```

It reported `32/140 (22.9%), diff=108`, with no compile failures or timeouts.
The Worker URL-post diagnostic consistently exits by signal 11 in Perry while
Node rejects synchronously with `DataCloneError`, keeps the worker usable, and
terminates cleanly. The SharedArrayBuffer/Atomics boundary is backed by the
added same-process channel and cross-worker clone/alias cases plus the existing
granular `globals/atomics-*.ts`, `buffer/from/shared-array-buffer.ts`, and
`util/types/arraybuffer-sharedarraybuffer.ts` cases; cross-agent fixtures such
as Node's `test-worker-message-channel-sharedarraybuffer.js` and Deno's
`broadcast_channel_sab.mjs` remain outside this messaging-focused batch. A
broader differential regression check of all six `globals/atomics-*.ts` cases
also remained clean at `6/6`.

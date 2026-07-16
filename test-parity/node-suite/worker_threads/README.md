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

The follow-up inventory was refreshed against the 2026-07-16 heads of Node
[`cf882a79042c`](https://github.com/nodejs/node/tree/cf882a79042cba4146acfdb7993b6a97c21e7239/test),
Deno
[`68d51a1fc8f3`](https://github.com/denoland/deno/tree/68d51a1fc8f32faf200ac4c5ba7b69648699c610/tests/unit_node),
and Bun
[`aca54d5c2b87`](https://github.com/oven-sh/bun/tree/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/test/js/node/worker_threads).
That pass added Node's Worker asynchronous-disposal, MessagePort and
BroadcastChannel custom-inspection, direct-message transferable graph, and
BroadcastChannel dispatch-time lifecycle contracts. Bun independently selects
Worker asynchronous disposal and Node's worker corpus, while Deno's unit suite
provides a second executable Node-compat comparison for the same core objects.
The latest deterministic audit also selected Node's filename and constructor
validation, MessagePort prototype/Event brands, and resource-limit metadata;
Deno's same-reference listener deduplication, pending synchronous receives, and
`process.env` option regression; and Bun's queued peer-close and orphaned
transferred-port regressions. The next audit selected Node's deterministic
FileHandle clone/transfer ownership and process restrictions, plus Bun's
FileHandle construction rollback and workerData alias regressions.
Resource-consuming in-use-handle races remain separate; these cases use small
controlled read-only files and explicit message/exit barriers. The following
pass selected Node's closing/closed-port listener safety and Bun's
spawning-thread `SHARE_ENV` tree isolation/founder regressions. Those
environment fixtures coordinate only through worker exit and scoped environment
stores, not nested-message scheduling. The latest pass adds Node's
regular-worker `isInternalThread` state, URL/eval and competing-option
constructor validation, closed-port `DOMException` branding, VM-context
FileHandle deserialization failure, and orphaned FileHandle ownership/rollback.
Deno's ordered listener and unref delivery selections and Bun's invalid-name
FileHandle rollback regression provide independent coverage for the same
lifecycle boundaries. The latest audit adds Node's worker-created buffer
lifetime, post-exit diagnostics, CPU-usage validation, process-environment
descriptors, and code-after-`process.exit()` contracts; Node's BroadcastChannel
WPT event metadata; and Bun's nested Map/Set FileHandle transfer regression. A
reverse worker-created MessagePort case closes the remaining deterministic
ownership direction without relying on scheduler timing.

The inventory review covered all 146 current `test-worker-*` files across Node's
parallel and sequential directories, all 48 cases in Deno's unit selection plus
its directory fixtures, and 88 `test`/`it` declarations across Bun's current
`worker_threads/*.test.ts` files. Cases are represented here by observable
contract rather than copied one-for-one when several upstream files exercise the
same behavior.

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
  cyclic and aliased graph snapshots, non-cloneable construction rejection, and
  parent/worker mutation isolation.
- `structured-clone/`: ArrayBuffer cloning, typed-array backing identity,
  built-in brands, cycles/aliasing, SharedArrayBuffer sharing, multiple views,
  multiple SharedArrayBuffers, transfer detachment, detached-buffer retransfer
  rejection, MessagePort ownership, unsupported URL rejection, overloads, and
  atomic rollback, and FileHandle clone rejection, transfer detachment,
  received-handle readability, and parent ownership loss.
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
  `removeAllListeners`, missing-message validation, transfer rejection during
  close flushing, frozen EventTarget prototype resilience, MessageEvent
  defaults/coercion, transfer targets, `ref`/`unref`/`hasRef` state, custom
  inspection of active/refed state, and proof that synchronous draining
  suppresses later event delivery, same-reference `.on()` deduplication and
  event-name scoping, synchronous drains after `start()`, queued-message
  ordering before a peer close, orphaned-transfer peer notification, and the
  exact MessagePort/Event prototype brands. Listener aliases are also exercised
  while both endpoints are closing and from the close callback after closure.
  Distinct listener registration order, delivery while the receiving port is
  explicitly unrefed, and the full closed-transfer `DOMException` brand are
  checked separately.
- `message-channel/`: module/global identity, construction rules, port brands,
  asynchronous and synchronous delivery, BroadcastChannel synchronous receive,
  and VM-context port movement with cross-realm branding, closed-port, argument
  validation, and FileHandle deserialization failure routing through
  `messageerror`. Worker-created MessagePorts are also drained synchronously
  after the creating worker exits, proving queued data and ownership survive the
  source realm.
- `worker-lifecycle/`: constructor transfer validation, structured `workerData`,
  default and built-in workerData, MessagePort/ArrayBuffer transfer, environment
  snapshots, eval/CJS require, file URLs with Unicode names, argv and execArgv
  coercion/validation, inherited/explicit/shared environments, SharedArrayBuffer
  through workerData and postMessage, queued transferred-port receive, thread
  id/name uniqueness and lifecycle, parentPort `onmessage`/ref state, Worker
  EventEmitter surface, MessagePort/EventTarget inheritance brands and listener
  validation, process restrictions, captured stdout/stderr, explicit and
  `process.exitCode` exits, unsupported paths, method receivers, postMessage
  clone rollback, `online`/`message`/`error`/`exit` ordering, ref return values,
  natural exit, and deterministic repeated termination settlement, transferred
  workerData port methods, sequential `SHARE_ENV` sibling visibility, post-exit
  method safety, indexed `SHARE_ENV` keys, eval syntax-error routing, URL
  postMessage rejection, missing postMessage validation, exit-listener exception
  routing, global `postMessage` replacement, workerData alias/view/cycle
  cloning, multiple and aliased MessagePorts, MessagePorts nested in Map/Set,
  broader filename, falsy execArgv, and disallowed `NODE_OPTIONS` validation,
  custom-stack and non-Error serialization, Worker asynchronous disposal,
  construction-time transfer rollback, default environment isolation, and the
  base file/eval `process.argv` shape, complete invalid filename types,
  `process.env` as an explicit environment option, and deterministic
  `resourceLimits` snapshots/reset without memory-pressure enforcement,
  FileHandle construction rollback and workerData alias identity, default/falsy
  thread names, the complete worker-disabled process stub/getter surface, and
  disjoint/founder `SHARE_ENV` spawning-tree semantics, regular-worker
  `isInternalThread` state, URL/eval rejection, constructor option-validation
  precedence with transfer ownership, and referenced/unreferenced FileHandle
  construction ownership and rollback. Reverse worker-created
  ArrayBuffer/SharedArrayBuffer lifetime, process-environment descriptor rules,
  post-exit diagnostic behavior, CPU-usage argument validation, Map/Set-nested
  FileHandle traversal, and the hard boundary after `process.exit()` are covered
  with exit or termination barriers.
- `transfer-markers/`: marker return/value semantics, inheritance boundaries,
  clone rejection, transfer rejection, retained ownership after rejection, and
  the distinction between marking a port uncloneable and transferring it, plus
  private, unforgeable, and permanent marker behavior.
- `broadcast-channel/`: same-name fanout/FIFO isolation, sender exclusion,
  listener management/`once`, `onmessage` replacement, name coercion, close/ref
  idempotence, typed-array and SharedArrayBuffer cloning, untransferable-value
  rejection, missing-message validation, clone-versus-closed error precedence,
  closed-channel and method-receiver validation, custom inspection, complete
  MessageEvent metadata, channel creation during dispatch, and suppression of
  delivery already queued for a channel that closes.
- `web-locks/`: deterministic pre-aborted requests and lock stealing, extending
  the existing surface, option, query, callback settlement/cleanup,
  independent-name concurrency, and shared/exclusive ordering cases.
- `direct-message/`: delivery, no-listener, timeout, and handler-failure
  rejection, plus thread id and timeout argument validation and atomic
  ArrayBuffer/MessagePort transfer with observable ownership.

Every asynchronous fixture uses a message, close, exit, stream-EOF, or
promise-settlement barrier. No added fixture uses a sleep as a completion
condition.

## Stopping judgment

The remaining upstream cases were not copied because they are redundant with the
cases above or belong to a separate slow/risky runtime feature:

- Resource-limit enforcement, stdio backpressure/large-write timing, heap
  snapshots, and CPU/heap profiling are resource- and platform-sensitive. Basic
  resource-limit metadata/reset, captured stdout/stderr, CPU-usage validation,
  and live/post-exit diagnostic method behavior are covered without introducing
  pressure or timing assertions.
- Basic eval/CJS require, execArgv, file-URL construction, synchronous URL/eval
  rejection, process restrictions, and exit codes are now covered.
  Data-URL/ESM/custom loaders, preload chains, signals, inspector integration,
  beforeExit and worker-side uncaught-exception variants, and source maps remain
  separate loader, CLI, or process-subsystem work.
- Controlled nested workers now cover environment-data inheritance and
  spawning-thread `SHARE_ENV` tree isolation without nested message scheduling.
  Deeper nesting stress, GC/finalization of unreachable ports, shared
  native-handle races, large message loops, and termination races remain
  scheduler-sensitive.
- Basic FileHandle clone rejection, transfer ownership, construction rollback,
  and workerData graph aliasing are covered with controlled read-only handles.
  Orphaned transfer ownership, rollback after a later clone failure, and
  invalid-name prevalidation are also covered. Pending read/close transfer
  races, descriptor recycling/leak detection, and cross-context disposal remain
  shared-handle stress rather than granular parity cases.
- SharedArrayBuffer aliasing is covered through same-thread MessagePort and
  BroadcastChannel paths plus cross-agent workerData and postMessage. Atomics
  wait/notify coordination remains separate scheduler-aware infrastructure.
- A Node `markAsUncloneable(Blob)` candidate was stable under Node 26.5.0 but
  exceeded the 120-second Perry per-case compilation threshold. It belongs with
  Blob runtime/compilation coverage rather than turning a marker diagnostic into
  a granular timeout.
- A self-contained WebAssembly.Module worker-post candidate produced the same
  module brand and result under Node 26.5.0, Deno 2.9.2, and Bun 1.2.18, but
  exceeded the 120-second Perry compilation threshold. Like the Blob candidate,
  that is WebAssembly/compiler coverage rather than a useful runtime diagnostic.
- A cross-worker BroadcastChannel delivery candidate was rejected after Node
  required cross-agent scheduling turns while Perry's absent delivery left no
  deterministic completion event. It belongs with scheduler-aware cross-agent
  infrastructure rather than a sleep/timeout-based fixture here.
- A Node live-`process.cwd()` propagation candidate passed three oracle runs,
  but Perry could not shut its worker down deterministically: `parentPort.close`
  is unsupported, a consumed `once` listener retained the worker, and explicit
  exit variably dropped the final message. That contract needs worker-shutdown
  infrastructure rather than a timeout-prone granular fixture.
- Web Locks now cover deterministic pre-abort and steal behavior in addition to
  surface, validation, shared/exclusive ordering, `ifAvailable`, and query
  snapshots. In-flight abort races and cross-agent ownership remain excluded.
- The existing `direct-message/` fixtures already cover deterministic
  `postMessageToThread` delivery, transferable ownership, rejection, handler
  failures, and timeout behavior. Bun's prototype-tampering regression was not
  retained: worker-to-main delivery can remain pending in Perry without a
  non-timing completion event, so a faithful fixture would rely on an arbitrary
  timeout.
- Bun's current non-function `process.emit` worker regression was not selected:
  the pinned Node 26.5.0 oracle exits fatally from `process._fatalException`
  before an `uncaughtException` handler can establish the recovery contract that
  Bun asserts. It is therefore not a Node parity case.
- A main-to-grandchild `postMessageToThread` candidate completed under Node and
  Deno, but Perry's nested worker never reached the explicit ready/inspection
  barrier and retained the event loop beyond 30 seconds. It needs nested-worker
  readiness/shutdown infrastructure rather than a granular timeout.
- Operating on the original MessagePort after `moveMessagePortToContext` was
  rejected because Node 26.5.0 itself exited by signal 11. The retained
  cross-realm-brand case observes the moved port without probing undefined
  moved-from behavior.
- Invalid-path transfer rollback was not retained: a literal nonexistent Worker
  target fails Perry compilation, while hiding it behind a dynamic path only
  exercises the already covered unresolved-path fallback and leaves the buffer
  untouched vacuously. Bun's peer-close-while-in-transit case and Deno's
  cross-worker unref liveness case were also not copied literally: the missing
  event in Perry retains the worker indefinitely. Their deterministic local
  subcontracts are covered through peer-close ordering and unrefed delivery
  without adding timeout-based completion.

The measured focused result is `38/205`: all 17 pre-existing cases remain green,
21 added cases pass, and 167 added cases expose stable diagnostic differences.
The prior ten cases were each repeated three times under Node 26.5.0 and Perry
with stable output and exit status. Deno 2.9.2 was stable across the same
matrix. Bun 1.2.18 was stable for nine cases; its older orphaned-transfer
implementation did not emit the peer close and hit the external three-second
comparison timeout in three runs, while current Bun upstream carries that exact
regression test. The latest four additions plus the two strengthened
path/process cases were also stable in three runs per runtime under Node, Perry,
Deno, and Bun. Bun 1.2.18 still rejects FileHandle workerData transfer selected
by current Bun upstream, and its bare-relative Worker path exits before the
remaining path matrix; both behaviors were stable and are recorded as
old-runtime comparison gaps. The latest closed-listener and two `SHARE_ENV` tree
cases were each repeated three times under all four runtimes with stable output
and exit status. Deno shares the process-wide tree-isolation gap, while Bun
1.2.18 rejects nested `SHARE_ENV`; Perry exposes process-wide leakage but passes
founder preservation. The latest ten cases were likewise repeated three times
under Node 26.5.0, Perry, Deno 2.9.2, and Bun 1.2.18. Bun's older unrefed-port
implementation retained the external six-second comparison process in all three
runs; every retained Node/Perry case completed through an explicit event or exit
barrier. The latest eight cases and strengthened workerData built-in fixture
were also stable in three runs under all four runtimes. Bun 1.2.18 emits an
expected not-implemented warning for Worker performance; all retained processes
terminate without an external comparison timeout.

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
also pass. The latest refresh adds passing dispatch-time coverage in
`broadcast-channel/create-during-dispatch.ts` and
`broadcast-channel/close-queued-delivery.ts`. Synchronously drained messages are
also proven not to reappear through the event listener in
`message-port/receive-consumes-event.ts`. Passing `process.env` directly as a
Worker environment option is also covered by
`worker-lifecycle/process-env-option.ts`. Explicit-env `SHARE_ENV` founder
preservation in `worker-lifecycle/share-env-founder.ts` passes too. Worker
CPU-usage argument validation in `worker-lifecycle/cpu-usage-validation.ts` also
matches Node. The diagnostic differences are:

- All thirteen `structured-clone/` fixtures: Perry preserves some indexed values
  but loses built-in/ArrayBuffer/view/SharedArrayBuffer brands and aliasing,
  rejects cycles/BigInt, does not detach or move ownership, accepts invalid
  lists, and treats FileHandles as cloneable plain objects.
- Forty-eight `message-port/` diagnostics: closing and dispatch flushing, queued
  data/callbacks, NodeEventTarget surface, listener counts and validation,
  receiver/options/iterables/transfers, MessageEvent construction and `ports`,
  duplicate/self/closed rollback, moved ownership, and `hasRef()` state and
  custom inspection differ, as do `.on()` deduplication, pending synchronous
  receives, queued peer-close ordering, orphaned transfers, Event brands, and
  listener alias behavior after closure.
- Seventy-one `worker-lifecycle/` diagnostics: invalid constructor payloads and
  paths, execArgv, captured stdio, process restrictions/exit codes, shared
  environments and nested messages, worker/parentPort EventEmitter behavior,
  metadata/unique ids, repeated termination, workerData and postMessage SAB
  sharing, queued port receive, rollback, post-exit values, error routing,
  Worker asynchronous disposal, default environment isolation, process argv
  shape, constructor rollback, invalid filename types, and resource-limit reset
  differ. FileHandle rollback/aliasing, default/falsy thread names, and the full
  disabled process surface also differ, as does spawning-tree `SHARE_ENV`
  isolation. Basic eval with CJS require, explicit `process.env`, and preserving
  an explicit-env founder value all pass.
- All five `transfer-markers/` fixtures: primitives are reported as marked,
  nested clone rejection, private/permanent marker enforcement, ArrayBuffer
  exceptions, and marked transferables/uncloneable ports differ.
- Twelve `broadcast-channel/` diagnostics: name coercion, once listeners,
  close/ref behavior, typed-array/SharedArrayBuffer branding and sharing,
  MessagePort validation, closed-channel posts, custom inspection, and complete
  MessageEvent metadata differ.
- Eighteen additional diagnostics cover namespace/prototype completeness,
  environment-data built-ins/inheritance/construction, MessageChannel
  construction/callability and VM-context branding, direct-message argument
  validation/transfer ownership, and Web Locks callback rejection cleanup.

The clean measurement used:

```sh
NODE_BIN="$(command -v node)" \
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" \
python3 scripts/node_suite_run.py \
  "$PWD/target/perry-dev/perry" "$PWD" worker_threads
```

It reported `38/205 (18.5%), diff=167`, with no compile failures or timeouts.
The Worker URL-post diagnostic consistently exits by signal 11 in Perry while
Node rejects synchronously with `DataCloneError`, keeps the worker usable, and
terminates cleanly. Cyclic `workerData` construction also consistently exits by
signal 11 in Perry while Node preserves both cyclic references and exits zero.
The SharedArrayBuffer/Atomics boundary is backed by the added same-process
channel and cross-worker clone/alias cases plus the existing granular
`globals/atomics-*.ts`, `buffer/from/shared-array-buffer.ts`, and
`util/types/arraybuffer-sharedarraybuffer.ts` cases; cross-agent fixtures such
as Node's `test-worker-message-channel-sharedarraybuffer.js` and Deno's
`broadcast_channel_sab.mjs` remain outside this messaging-focused batch. A
broader differential regression check of all six `globals/atomics-*.ts` cases
also remained clean at `6/6`.

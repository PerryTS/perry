# `node:async_hooks` granular parity suite

This directory tests deterministic public behavior from `node:async_hooks`.
Fixtures assert ID relationships rather than exact numeric IDs and use explicit
completion barriers for asynchronous work.

## Upstream comparison

The expansion was reviewed on 2026-07-16 against primary repository sources:

- Node.js main at
  [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/async-hooks),
  especially the AsyncResource lifecycle, AsyncLocalStorage nesting,
  enable/disable, promise, pre-hook Promise creation, late hook activation,
  execution-resource identity, default trigger, concurrent HTTP/socket, and
  async/await cases, plus its
  [bind](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-async-local-storage-bind.js)
  and
  [snapshot](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-async-local-storage-snapshot.js)
  selections.
- Deno main at
  [`f8a17c8`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/unit_node/async_hooks_test.ts),
  whose selected compatibility coverage independently emphasizes nesting,
  enterWith, bind/snapshot, AsyncResource scope callbacks, and async API
  propagation.
- Bun main at
  [`5d350cc`](https://github.com/oven-sh/bun/tree/5d350cc17525a493fcb55b0a014f75af7c414580/test/js/node/async_hooks),
  plus its selected Node compatibility cases for constructor behavior,
  receiver preservation, context isolation, and exit cleanup.

The correctness oracle remains the repository-pinned Node 26.5.0.

The final expansion directly maps the deterministic public contracts from
Node's `test-async-hooks-disable-during-promise.js`,
`test-async-hooks-enable-during-promise.js`,
`test-async-hooks-promise-triggerid.js`, both
`test-promise.*-before-init-hooks.js` cases, `test-late-hook-enable.js`,
`test-nexttick-default-trigger.js`, `test-async-exec-resource-match.js`,
`test-async-hooks-correctly-switch-promise-hook.js`,
`test-async-hooks-close-during-destroy.js`,
`test-async-hooks-execution-async-resource-await.js`,
`test-async-local-storage-http-multiclients.js`,
`test-async-local-storage-socket.js`, and
`test-eventemitter-asyncresource.js`. Deno's selected bind/snapshot, nesting,
enterWith, resource-scope, and propagation contracts and Bun's async-context
provider matrix are represented by smaller single-boundary fixtures rather
than copied monolithic tests.

The current focused result is **72/147** and is recorded in `node_suite_baseline.json`. The suite
keeps every stable mismatch as a diagnostic rather than removing unsupported
cases: failures identify context loss, missing hook callbacks/resources,
lifecycle differences, validation gaps, or a compile/runtime boundary for the
specific provider named by the fixture.

The 75 non-matching diagnostics are stable and grouped as follows:

- hook delivery/configuration: custom and built-in provider lifecycle callbacks,
  cancelled resource destruction and identity, simultaneous hooks, late
  activation during timers/immediates/next ticks and Promise chains,
  pre-created Promise relationships, mixed Promise hook shapes, destroy work
  queued from a destroy callback, `promiseResolve`, resource arguments,
  execution-resource mapping/metadata, and `trackPromises`
  behavior/validation;
- scheduling/context: zlib, HTTP/HTTPS keep-alive reuse and concurrent clients,
  net callback/data isolation, dgram, subprocess, worker, VM, dynamic import,
  readline, events.on, and stream.finished boundaries;
- callback contract: several async crypto APIs invoke their callback before the
  call returns, while prime callbacks do not settle;
- resource/storage semantics: snapshot receiver handling, top execution-resource
  restoration, disable cleanup, and caught async `exit()` rejection routing; and
- compilation: direct `node:tls` activation currently builds rustls without its
  `ring` provider and then cannot find a fallback runtime archive. The local
  certificate fixture itself passes the pinned Node oracle.

## Coverage

- `resource/`: construction/type and ID invariants, scope/receiver/arguments,
  instance bind, deterministic hook scope callbacks, and explicit destroy.
- `storage/`: run nesting and restoration, independent instances, enterWith,
  exit and its async descendants, EventEmitter listener bleed/isolation,
  multiple store value types, disable/re-entry, repeated disable, and
  promise-boundary behavior.
- `static/`: AsyncLocalStorage bind/snapshot and AsyncResource.bind context,
  empty and populated captures, receiver, argument, return-value, re-entry, and
  restoration behavior.
- `propagation/`: controlled concurrent promises, catch/finally, thenables,
  thenables returned from async functions and handlers, async iterators,
  dynamic import, local fetch, VM scheduling, queueMicrotask, nested nextTick,
  immediate, ref/unref, interval, and timer propagation with awaited or
  callback-driven completion barriers.
- `integrations/`: individual fs access, mutation, metadata, descriptor,
  directory, watch, promises, and stream callbacks; crypto random, KDF, key,
  key-pair, and prime callbacks; all major zlib callback/stream families;
  Readable/Writable/Transform/finished, timers/promises, util.promisify,
  EventEmitter, and EventEmitterAsyncResource behavior selected from Bun's
  async-context matrix and Node's provider tests.
- `hooks/`: enable/disable/re-enable, simultaneous observers, enabling and
  disabling observers from `init` or while callbacks and Promise chains are
  active, mixed Promise hook shapes, pre-created and async/await Promise trigger
  chains, execution-resource identity and writable metadata propagation,
  default next-tick triggers, re-entrant destroy queuing, `trackPromises`,
  `promiseResolve`, resource arguments, cancelled timer/immediate destruction,
  deterministic timer/microtask/nextTick/fs/crypto/PBKDF2 lifecycles, and
  throwing scoped callbacks.
- `providers/`: DNS, child processes, HTTP and HTTPS including keep-alive agent
  and concurrent-client isolation, HTTP execution-resource mapping, TLS, net
  including concurrent data isolation and `getConnections`, dgram, workers,
  readline, events.on, and stream async iterators with local endpoints,
  ephemeral ports, and explicit close/exit barriers. This directory runs in the
  sequential lane.
- `validation/`: synchronous throw propagation and cleanup of storage and
  execution-resource state, plus hook-sensitive empty resource type behavior.
  The pre-existing root fixtures retain detailed callback, constructor, and
  hook-option argument validation.

## Remaining slow, redundant, or environment-sensitive categories

The following current upstream selections are not counted as coverage. Each is
kept out for a concrete reason rather than to cap the suite size:

- Exact numeric async IDs are runtime-specific; only relationships and
  restoration invariants are asserted here.
- GC-driven destroy delivery, weak-reference collection, destroy-vs-scheduler
  priority, recursive hooks, deep stacks, stress/leak probes, and process
  shutdown require slow or timing-sensitive runners.
- Cross-worker storage inheritance is not a Node contract; the retained worker
  cases instead check the parent-side `online`, `message`, and `exit` provider
  callbacks with an explicitly terminated local worker.
- Uncaught exception and unhandled rejection routing mutates process-global
  handlers and belongs in a dedicated isolation fixture.
- Bun's DNS CNAME/MX/TXT/reverse cases depend on external resolver state. Local
  `lookup`, `resolve4`, and the Node lifecycle selections cover the stable DNS
  provider boundary without making network availability an oracle input.
- Bun's crypto cipher/hash/sign/randomUUID selections perform their work
  synchronously and only check a following `setImmediate`; that scheduling
  behavior is already isolated by the immediate propagation and hook fixtures.
- Node's HTTP parser/socket reuse graphs, provider enum internals, signals,
  pipes, TTY, process-shutdown, and inspector/trace-event cases assert native
  implementation topology rather than portable public async-context behavior.
- Node's HTTP/2 ALS selection cannot yet reach an async-context callback in
  Perry because its local plaintext client never emits `connect`; it belongs
  after the underlying `node:http2` provider can complete a loopback session.
- Bun's ReadableStream `cancel` selection expects captured context while Node
  26.5.0 reports `undefined`. Its `pull` selection matches Node, but activates a
  separate Web Streams feature whose cold Perry build exceeds the granular
  runner's compile budget; both belong with Web Streams compatibility rather
  than this Node module suite.
- Node 26.5.0 supports AsyncLocalStorage `defaultValue` and `name`, but Perry's
  API manifest does not claim those constructor/name surfaces, so they are not
  counted here.

The raw hook cases retained here are limited to a user-created AsyncResource:
its init callback is synchronous, its before/callback/after sequence is fully
controlled, and explicit `emitDestroy()` is observed behind a setImmediate
barrier. These cases are deterministic and expose lifecycle gaps without
depending on provider internals or GC timing.

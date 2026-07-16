# `node:inspector/promises` granular parity suite

Deterministic Promise-API coverage for Node's `node:inspector/promises` module.
Node **26.5.0** is the oracle. Each fixture isolates one public Promise contract
and prints only stable semantic fields; protocol IDs, contexts, scripts, stacks,
locations, timestamps, profiles, and engine text are not compared.

## Coverage

- exact callback-module export-key parity, descriptors, import identities, the
  distinct `Session` subclass, prototype chains, constructor behavior, inherited
  control methods, and Promise `post()` receiver behavior
- synchronous `connect()`/`connectToMainThread()`/`disconnect()` control versus
  asynchronous `post()` validation and connection rejection
- reconnect, independent sessions, pending-post disconnect, same-turn
  non-settlement, stable rejection identity, and independent concurrent posts
- controlled out-of-order completion with a resolver barrier, proving both
  independent settlement and `Promise.all()` input ordering without timers
- deterministic `Runtime`, `Schema`, Debugger, Profiler, and HeapProfiler
  resolution/rejection shapes, including `getProperties`/`releaseObject`
- primitive, special-number, BigInt, by-value, fulfilled `awaitPromise`, thrown
  evaluation, and rejected `awaitPromise` results
- the one Promise-distinct notification contract: specific and generic events
  are delivered before the enabling `post()` Promise fulfills

The module already runs in the sequential lane in `scripts/node_suite_run.py`.

## Oracle and alternate-runtime evidence

Primary sources audited at exact tags:

- Node 26.5.0: `lib/inspector/promises.js`, `lib/inspector.js`,
  `doc/api/inspector.md`, and `test/parallel/test-inspector-promises.js`.
- Deno 2.9.2: `ext/node/polyfills/inspector/promises.js`, `promises_esm.js`, the
  callback implementation, and `tests/unit_node/inspector_test.ts`.
- Bun 1.3.14: `src/js/node/inspector.promises.ts`, `src/js/node/inspector.ts`,
  and `test/js/node/inspector/inspector.test.ts`.

Five complete Node oracle passes (150 executions) exited zero with identical
aggregate stdout/exit data at SHA-256
`994cab8da5deb3aa67bb505560eae5f6d7ee0754dfdd0aad92f4c95266688d49`. Deno
produced 28 exact matches, one surface diff, and one deterministic process
error. Bun produced one exact match, 11 diffs, 17 errors, and one bounded
timeout. Three focused release-runner passes classified Perry identically at
**3/30**, with 27 stable output/exit diffs and no compile failure, runtime
timeout, or crash.

Deno mirrors Node's subclass-plus-promisify design but omits `NetworkResources`.
Its pending-disconnect case reproducibly aborts with a V8 evaluate-callback
assertion instead of rejecting the Promise. Bun implements a Promise subclass
around its callback inspector, but only Profiler commands have a backend;
Runtime and Schema commands reject, and several validation/lifecycle semantics
differ from Node. Node remains the oracle for both divergences.

See [EVIDENCE.md](EVIDENCE.md) for per-entry classification.

## Upstream and callback-suite reconciliation

Node's exact `test-inspector-promises.js` inventory is reconciled as follows:

1. callback-module export keys: `surface/exports.ts`;
2. `Session.post()` returns a Promise: receiver, validation, disconnected, and
   settlement fixtures;
3. resolved protocol payloads: safe Runtime/Schema/enable-disable fixtures;
4. submission/input order with a slow response: `post/concurrent-order.ts`,
   using an explicit resolver barrier rather than the upstream 100 ms timer;
5. CPU-profile URL payload: excluded because sampling/profile contents and
   source URLs are volatile; safe empty results and Runtime values prove
   resolution.

The 37 callback fixtures from PR #6490 are also accounted for:

- Promise-specific counterparts retained: `lifecycle/main-thread-connect`,
  `method-receivers`, `repeated-sessions`, `session-connect`;
  `post/circular-params`, `disconnected`, `method-validation`,
  `params-validation`, `pending-disconnect`, `unknown-command`;
  `protocol/enable-disable`, `schema-domains`; `runtime/await-promise`,
  `exception-details`, `get-properties-release`, `numeric-specials`,
  `primitives`, `return-by-value`; `events/notification-order`; and
  `surface/exports`, `session-class`.
- Callback-only and non-applicable: `post/callback-validation`, `overloads`, and
  `session/callback-runtime`.
- Inherited or protocol-payload duplicates deliberately not copied:
  `events/console-api`, `listener-lifecycle`, `script-parsed`;
  `lifecycle/endpoint`, `open-range-validation`; `network/helpers`;
  `protocol/debugger-metadata`, `get-script-source`; `runtime/object-preview`,
  `release-object-group`, `remote-subtypes`; and `surface/domain-helpers`,
  `domain-validation`.

## Deliberate exclusions and stopping boundary

- Endpoint `open`/`close`/`url`, range checks, domain helpers, and callback
  event inventories are shared identities/inherited behavior already proven by
  PR #6490; only their Promise-module import identity is retained here.
- No fixed port, external DevTools/WebSocket client, frontend, internet, worker
  race, active `waitForDebugger`, breakpoint, signal, crash, large payload, or
  stress case is used.
- CPU/heap profiles, sampling, coverage payloads, heap snapshots,
  GC/finalization, `queryObjects` retention, memory pressure, and
  source/position payloads remain excluded as timing-, allocation-, or
  engine-dependent.
- An explicit third callback is outside the Promise signature and can strand the
  promisified Promise forever; it is documented rather than tested.
- Every rejection is awaited/caught. Every connected session disconnects in a
  `finally` path, every installed listener is removed, and the controlled global
  resolver is deleted. The only derived Promise observers include rejection
  handlers or are themselves awaited.

This exhausts the reachable deterministic Promise-specific surface in Node
26.5.0 and the requested safe protocol subset without count-driven assertions,
arbitrary sleeps, or duplicated callback contracts.

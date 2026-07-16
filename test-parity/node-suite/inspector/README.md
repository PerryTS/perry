# `node:inspector` granular parity suite

Deterministic callback-API coverage for Node's `node:inspector` module. Node
**26.5.0** is the oracle. Every case is a small TypeScript program whose stdout
is compared byte-for-byte with Perry; volatile protocol identifiers and engine
state are reduced to type, presence, ordering, or boolean semantic checks inside
the fixture.

## Coverage

- public export inventory, descriptors, domain helper inventory, `Session`
  inheritance, prototype methods, and receiver behavior
- endpoint inactive/active/disposed lifecycle, range validation, main-thread
  rejection, connect/double-connect/disconnect/reconnect, and independent
  sessions
- `post()` method/params/callback validation, overloads, circular serialization,
  connection errors, unknown commands, and pending callback completion during
  disconnect
- `Runtime.enable`,
  primitive/special-number/BigInt/object/exception/promise/preview result
  shapes, `getProperties`, `releaseObject`, and `releaseObjectGroup`
- `Schema.getDomains`, safe Debugger/Profiler/HeapProfiler enable-disable
  contracts, debugger metadata, and exact controlled script-source retrieval
- execution-context, console, and script-parsed notifications;
  specific-before-generic ordering; exact `on`/`once`/`off` cleanup
- Network/DOMStorage helper inventories and validation without external
  frontends

The module is in the sequential lane in `scripts/node_suite_run.py`; no new
runner exception is needed.

## Oracle and alternate-runtime evidence

Primary sources audited at their exact tags:

- Node 26.5.0: `lib/inspector.js`, `doc/api/inspector.md`,
  `test/parallel/test-inspector-module.js`, `test-inspector-bindings.js`,
  `test-inspector-multisession-js.js`, `test-inspector-emit-protocol-event*.js`,
  `test-inspector-scriptparsed-context.js`,
  `test-inspector-runtime-evaluate-with-timeout.js`, and
  `test/sequential/test-inspector-open-dispose.mjs`.
- Deno 2.9.2: `ext/node/polyfills/inspector.js`, `inspector_esm.js`,
  `ext/node/ops/inspector.rs`, and `tests/unit_node/inspector_test.ts`.
- Bun 1.3.14: `src/js/node/inspector.ts` and
  `test/js/node/inspector/inspector.test.ts`.

After final cleanup and endpoint subprocess isolation, three complete Node
oracle passes (111 executions) exited zero with identical aggregate stdout at
SHA-256 `fedccaf62d97bd5c1ebe1589a063e872222510b591536816ab38fe5bfb39fecd`. Deno
produced 30 exact matches, 4 diffs, and 3 errors. Bun's pinned source implements
Profiler commands while retaining stubs for the remaining endpoint surface; its
run produced 14 diffs, 19 errors, and 4 bounded timeouts.

See [EVIDENCE.md](EVIDENCE.md) for per-entry classification.

## Deliberate exclusions and stopping boundary

- `node:inspector/promises` remains a separate suite; no promise-API case is
  added here.
- No external DevTools/WebSocket client, fixed port, internet dependency, or
  inspector frontend is used. The endpoint smoke case runs in a bounded child
  process, uses loopback port `0`, closes in a `finally` path, and is forcibly
  terminated by `spawnSync` on timeout.
- Worker-to-main-thread connection is limited to deterministic main-thread
  rejection. Worker scheduling and cross-thread debugger traffic remain
  excluded.
- `waitForDebugger()` is checked only while inactive. Its active behavior
  intentionally blocks for a frontend command.
- CPU/heap profiles, sampling, coverage payloads, heap snapshots,
  GC/finalization, retained-object queries, source-map/loader matrices,
  breakpoints, signals, crashes, large payloads, concurrency, and stress remain
  out because their useful payloads depend on timing, allocation, source
  positions, or external control.
- Network and DOMStorage event delivery requiring experimental flags/frontends
  is not forced into the default runner. Inventory and argument contracts remain
  useful and deterministic without weakening Node assertions.
- `exceptionThrown`/`exceptionRevoked` were prototyped but not retained:
  same-thread uncaught/rejected probes either terminate or do not deliver a
  stable notification barrier. `Runtime.evaluate` exception details provide the
  deterministic semantic invariant instead.

The retained surface exhausts the reachable, deterministic callback contracts in
the public API and requested safe protocol subset without turning
engine-specific or timing-sensitive payloads into count-only tests.

## `node:inspector/promises` follow-on inventory

Node 26.5.0's `test/parallel/test-inspector-promises.js` establishes: exact
callback-module export-key parity, promise-returning `Session.post`, resolution
payload parity, and submission-order preservation when responses complete out of
order. The next isolated suite should mirror the safe lifecycle, validation,
Runtime/Schema/enable-disable, notification, and cleanup cases here using
promise settlement. The upstream CPU profile URL assertion and delayed-timer
ordering probe should be replaced by bounded deterministic semantics rather than
copied literally.

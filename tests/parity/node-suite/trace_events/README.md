# `node:trace_events` granular parity suite

This directory covers deterministic public `node:trace_events` contracts. Node
26.5.0 is the behavioral oracle. Cases print normalized semantic facts rather
than volatile trace payloads, paths, process/thread identifiers, timestamps, or
scheduler-dependent event order.

## Source audit

- Node.js `v26.5.0` (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`):
  `lib/trace_events.js`, `doc/api/tracing.md`, and the 29
  `test/parallel/test-trace-events-*.js` selections.
- Deno `main` (`803a3c933e1e23e0972445293ec0b34b8da96ccc`):
  `ext/node/polyfills/trace_events.ts` and `trace_events_esm.ts`. Deno keeps the
  source categories array by reference and preserves insertion order in its
  JavaScript category registry. Deno 2.9.2 executed all 24 TypeScript fixtures,
  matching Node stdout in 13; its Node-style CLI flags did not feed the public
  category registry or produce native trace files in the controlled lane.
- Bun `main` (`aca54d5c2b874ac304a3bbe1d67630e4daf17b43`):
  `src/js/node/trace_events.ts` plus Bun's selected Node fixtures. Current
  source copies the categories array and delegates category/file behavior to
  `internal/trace_events`. The locally available Bun 1.2.18 predates that
  implementation: 14 of 24 fixtures executed and the stateful tests exposed its
  older stub behavior. It is recorded as support evidence, not used as the
  oracle.

## Covered contracts

- Module exports and descriptors; `Tracing` prototype, constructor, accessors,
  and method surface.
- Options/category validation, inherited/accessor options, duplicate and empty
  category entries, and the distinct property-vs-native-handle mutation rules.
- Read-only accessors, incompatible receivers, idempotent repeated cycles,
  global normalization, overlap/reference counts, tracer isolation, and the
  enabled-object warning threshold.
- Categories inherited from `--trace-event-categories` in a controlled child.
- Native flags, exit flushing, semantic JSON shape, metadata, category filtering
  across `disable()`, and normalized `${pid}`/`${rotation}` file-pattern
  expansion.

Every subprocess file-output case uses a fresh temporary directory and removes
it in `finally`. In-process cases that enable tracing also redirect Node's trace
file into a temporary directory before the first enable.

## Measured Perry boundary

Repeated focused runs produce 14/24 parity matches with no compile failures,
timeouts, harness errors, leaked subprocesses, or temporary artifacts. The ten
stable differences diagnose:

- module function names/configurability, the exposed `Tracing` constructor's
  arity/constructibility, and Perry's own `__perryTraceEventsId`/`constructor`
  instance properties;
- Node's live `categories` property view of the source array (including later
  join coercion) versus Perry's property snapshot;
- the warning emitted after more than ten enabled controllers;
- categories inherited from process flags; and
- four native-output contracts: JSON/file creation, metadata/category filtering,
  disable filtering, and file-pattern placeholder expansion.

## Deliberate stopping boundaries

- Inspector/Perfetto/Chrome tooling and dynamic inspector collection require a
  separate inspector integration lane.
- Worker aggregation, worker metadata, and cross-thread async-hooks topology
  introduce process/thread ordering and aggregation races.
- Console, fs, net, HTTP, promise, VM, V8, thread-pool, and async-hooks provider
  payload tests belong to those provider suites; the single console marker here
  diagnoses the public `disable()` filter without snapshotting its payload.
- Signals, crashes, permissions/kernel faults, rotation/buffer overflow, huge
  traces, profiling/coverage, and stress tests are platform- or timing-sensitive
  native tracing work.
- Forced-GC retention of an otherwise unreachable enabled controller depends on
  the separate `--expose-gc`/collector lane; the enabled-controller warning is
  covered without forcing a collection.
- Perry's implementation explicitly provides category control without Chrome
  trace-event emission. Native-file cases intentionally diagnose that boundary;
  this parity-only suite does not change runtime behavior.

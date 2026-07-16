# `node:perf_hooks` parity coverage

This suite uses Node.js **v26.5.0** as its exact behavioral oracle. The audit is
based on that tag's `lib/internal/perf/*`, `lib/internal/histogram.js`,
`test/parallel/test-perf-hooks-*.js`, and `test/sequential/test-perf-hooks.js`.
The comparison sources are Deno **v2.9.2** (`ext/node/polyfills/perf_hooks*`,
`ext/node/ops/perf_hooks.rs`, and `tests/unit_node/perf_hooks_test.ts`) and Bun
**v1.3.14** (`src/js/node/perf_hooks.ts`, the internal event-loop-delay module,
and the primary histogram/perf-hooks tests). Node remains authoritative where
those runtimes deliberately expose a smaller or Web-oriented surface.

## Deterministic contract map

The 148 granular fixtures cover:

- module exports, aliases, global identity, public descriptors, prototypes,
  tags, constructors, receivers, and exact supported observer types;
- `now()`, `timeOrigin`, `nodeTiming`, `toJSON()`, and ELU
  shape/order/arithmetic invariants without snapshotting host timestamps;
- marks, measures, clears, timeline sorting, repeated-name resolution, detail
  cloning, validation, and snapshot/reference identity;
- observer subscription modes, buffering, queue draining, disconnect/reobserve,
  replacement, multiple observers, callback/list identity, EntryList filtering,
  branding, sorting, and explicit `setImmediate` delivery barriers;
- timerify call/construct behavior, `this`, arguments, result/error identity,
  wrapper metadata, nesting, async settlement, observer entries, and optional
  histograms;
- recordable histogram empty/populated state, Number/BigInt twins, validation,
  percentiles, reset, add/isolation, `recordDelta()`, serialization, and illegal
  construction;
- event-loop-delay handle identity, inherited histogram surface, option
  validation, enable/disable transitions, reset, and cleanup;
- controlled synthetic resource timing accessors, transfer-size cache modes,
  serialization, timeline/observer delivery, validation, and cleanup;
- all stable Node GC constants and Node bootstrap milestone relationships.

The module is in the sequential node-suite lane because observer dispatch,
immediate barriers, timerify settlement, and event-loop histogram lifecycle must
not be perturbed by six concurrent parity jobs.

## Isolation and stopping boundary

Every created mark, measure, resource record, observer, or delay histogram is
cleared, disconnected, or disabled by the fixture that owns it. Time values are
reported only through types, ordering, ranges, or controlled synthetic inputs.
No fixture requires an exact timestamp, duration, percentile, delay,
utilization, or scheduler threshold.

The following remain intentionally outside this deterministic lane:

- GC entries/forced GC/finalization;
- exact event-loop delay or utilization measurements and CPU contention;
- real DNS/HTTP/network resource timings;
- workers, inspector/trace integration, bootstrap timing snapshots;
- races, stress, leak, exhaustion, and large-sample percentile tests.

Node 26.5.0 event-loop-delay histograms do **not** expose `ref()`, `unref()`, or
`hasRef()`; their absence is documented rather than inventing an unsupported
contract. Buffer-full event timing and `recordDelta()` magnitude are not used as
pass conditions because they depend on loop scheduling. Resource timing uses
fully synthetic Fetch Timing Info rather than real I/O.

## Measured provenance

The final 148-fixture tree was executed twice with byte-identical output under
Node 26.5.0: **148/148** in both runs. Two sequential Perry classifications with
the release compiler were also identical: **87 pass, 60 output differences, one
compile failure, zero timeouts**. The compile failure is the isolated named
`eventLoopUtilization` export contract; Perry does not currently export it.

The same fixtures produced **101 pass, 33 differences, 14 runtime errors, zero
timeouts** on Deno 2.9.2, and **97 pass, 24 differences, 27 runtime errors, zero
timeouts** on Bun 1.3.14. Those results describe intentional and incomplete
compatibility surfaces; they do not replace Node as the oracle.

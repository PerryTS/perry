# `node:test` granular parity coverage

This directory compares deterministic `node:test` semantics through controlled
console markers. The differential runner still compares the test runner's own
stdout and exit status, but canonicalizes test durations and source locations;
fixtures do not assert paths, stacks, process IDs, or terminal formatting.

## Upstream selection

The expansion was selected on 2026-07-16 from these primary repository
snapshots:

- Node.js [`34c28d5a69f4f00cd599adcbe57834435d3a683b`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b), especially
  [`test-runner-mocking.js`](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-runner-mocking.js),
  [`test-runner-plan.mjs`](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-runner-plan.mjs),
  [`test-runner-aftereach-runtime-skip.js`](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-runner-aftereach-runtime-skip.js),
  [`test-runner-subtest-after-hook.js`](https://github.com/nodejs/node/blob/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/parallel/test-runner-subtest-after-hook.js), and the deterministic parts of the run and mock-timer tests.
- Deno [`f8a17c8171569fa2870d740030aaa59c91fdf9ee`](https://github.com/denoland/deno/tree/f8a17c8171569fa2870d740030aaa59c91fdf9ee). Deno's current Node-compat selection does not carry a dedicated `node:test` file under `tests/unit_node`; its runner, context, hooks, mocks, timer, snapshot, and reporter compatibility lives in
  [`ext/node/polyfills/testing.ts`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/ext/node/polyfills/testing.ts).
- Bun [`6173d6431ee8ad086bf79d1d5354080cfe937964`](https://github.com/oven-sh/bun/tree/6173d6431ee8ad086bf79d1d5354080cfe937964), especially its
  [`node:test` selection](https://github.com/oven-sh/bun/blob/6173d6431ee8ad086bf79d1d5354080cfe937964/test/js/node/test_runner/node-test.test.ts) and
  [hook-order fixture](https://github.com/oven-sh/bun/blob/6173d6431ee8ad086bf79d1d5354080cfe937964/test/js/node/test_runner/fixtures/02-hooks.js).

## Added diagnostic categories

- `imports` and `runner/registration` (10 fixtures): export aliases, callback
  overloads, deferred registration, async/callback completion, option
  validation, nested suites/subtests, and parent-child completion.
- `runner/context` and `runner/api`: assertion surface, plans, runtime
  skip/todo/only, failure propagation, names, the existing diagnostic case, and
  the claimed `run()` surface (16 fixtures).
- `runner/hooks` (9 fixtures): global, repeated, and nested ordering, hook
  context, runtime-skip cleanup, and cleanup after body or setup throws.
- `mock-fn` (27 fixtures): successful, async, bound, no-op, and throwing call
  records; receiver/prototype/inheritance behavior; implementation queues and
  indexed overrides; reset/restore behavior; property/accessor contexts;
  symbols, descriptors, `times`, and validation.
- `mock-timers` (13 fixtures): timeout arguments and cancellation, interval
  repetition/self-clear, reset, validation, boundary ordering, nested
  `runAll()`, Date construction, and deterministic `setTime()` behavior.
- `snapshots` and `reporters` (4 fixtures): serializer/assertion validation and
  synthetic directive/nesting events.

Snapshot fixtures use fixed local files, and reporter fixtures feed synthetic
events. The 79 added fixtures expand the module from 11 to 90 cases without
depending on wall-clock time or external files.

## Stopping judgment

The remaining Node runner corpus is intentionally left for separate work:

- CLI discovery, watch mode, coverage, source maps, process isolation, worker
  IDs, global setup, rerun state, and force-exit behavior require subprocess or
  multi-file harness support rather than this in-process granular suite.
- concurrency, randomization, timeouts, abort scheduling, refed handles, and
  scheduler-sensitive timer APIs are not deterministic enough for byte-for-byte
  stdout comparison here.
- enabling mock timers twice currently leaves a Perry process handle alive even
  after `reset()`; that validation belongs with the runtime cleanup fix rather
  than a granular case that times out the harness.
- colors/TTY, absolute locations, stacks, durations, and reporter formatting
  tied to those values are environment-specific.
- `TestContext.waitFor()`, `runOnly()`, tags, full names, signals, and custom
  assertions are not listed in Perry's current `node:test` manifest; testing
  those as claimed compatibility would overstate the supported surface.
- module mocking and snapshot update/CLI behavior are separate runtime and CLI
  projects. Constructor-target mocks are also deferred because Perry does not
  currently expose constructable mock wrappers. Further core cases are
  redundant with the focused fixtures above or depend on one of these excluded
  surfaces.

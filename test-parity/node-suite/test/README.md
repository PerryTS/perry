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

- `runner/registration`: deferred registration, nested suites/subtests, and
  parent-child completion.
- `runner/context` and `runner/api`: assertion surface, plans, runtime
  skip/todo/only, failure propagation, names, the existing diagnostic case, and
  the claimed `run()` surface.
- `runner/hooks`: global and nested ordering plus cleanup after body or setup
  throws.
- `mock-fn`: successful and throwing call records, implementation queues,
  reset/restore behavior, nested method restoration, property/accessor contexts,
  `times`, and validation.
- `mock-timers`: boundary ordering, nested `runAll()`, and deterministic Date
  `setTime()` behavior.

The existing snapshot fixtures already use fixed local files, and the existing
reporter fixture feeds synthetic events. Additional snapshot/reporter cases were
not added because the remaining upstream coverage is primarily path, stack,
duration, coverage, and process-output formatting.

## Stopping judgment

The remaining Node runner corpus is intentionally left for separate work:

- CLI discovery, watch mode, coverage, source maps, process isolation, worker
  IDs, global setup, rerun state, and force-exit behavior require subprocess or
  multi-file harness support rather than this in-process granular suite.
- concurrency, randomization, timeouts, abort scheduling, refed handles, and
  scheduler-sensitive timer APIs are not deterministic enough for byte-for-byte
  stdout comparison here.
- colors/TTY, absolute locations, stacks, durations, and reporter formatting
  tied to those values are environment-specific.
- `TestContext.waitFor()`, `runOnly()`, tags, full names, signals, and custom
  assertions are not listed in Perry's current `node:test` manifest; testing
  those as claimed compatibility would overstate the supported surface.
- module mocking and snapshot update/CLI behavior are separate runtime and CLI
  projects. Further core cases are redundant with the focused fixtures above or
  depend on one of these excluded surfaces.

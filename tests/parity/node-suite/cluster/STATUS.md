# node:cluster source audit and stopping record

## Primary-source snapshot

Audited on 2026-07-16:

- Node.js v26.5.0 commit
  [`1e320ec`](https://github.com/nodejs/node/tree/v26.5.0/test), including 83
  `test/parallel/test-cluster-*.js`, five sequential cluster tests, the known
  inspector-port-clash case, `lib/internal/cluster/*`, and the API docs.
- Deno main commit
  [`803a3c9`](https://github.com/denoland/deno/tree/803a3c933e1e23e0972445293ec0b34b8da96ccc),
  especially `tests/unit_node/cluster_test.ts` and
  `ext/node/polyfills/internal/cluster/*`. Deno's selected unit test currently
  checks the primary export surface; its implementation supplies additional
  lifecycle comparison evidence.
- Bun main commit
  [`aca54d5`](https://github.com/oven-sh/bun/tree/aca54d5c2b874ac304a3bbe1d67630e4daf17b43),
  with 54 imported Node cluster tests plus three cluster-specific TypeScript
  selections. Bun's own cases add advanced structured-clone and Worker
  disconnect evidence.

The Node tag is the output oracle. Deno and Bun are comparison selections, not
substitute expected-output sources.

## Measured coverage

- 43 granular TypeScript fixtures (42 added over the original one).
- Node v26.5.x: all 43 complete successfully.
- Deno 2.9.2 local comparison: 41/41 complete successfully before the two
  maintainer-audit additions.
- Bun 1.2.18 local comparison: 40/41 complete successfully before the two
  maintainer-audit additions; its older local release fails the
  `ChildProcess.channel` ref/unref probe. Current Bun source, rather than this
  older binary, was used for selection evidence.
- Perry differential result: 43/43 (100%) on the final maintainer-audit run,
  with no output differences, compile failures, crashes, or skips.

## Repaired diagnostic boundaries

The formerly measured Worker construction/prototype, setup alias/event, empty
disconnect timing, fork/event ordering, worker/cluster message forwarding,
worker state/exit payload, option validation, TCP listening, and advanced IPC
differences now match Node across this suite.

## Stopping exclusions

The remaining upstream cases were reviewed and stopped in these categories:

- **Broader scheduler stress:** multi-worker connection distribution, server
  restart, backlog, pipe handles, socket transfer, and shared-handle races. The
  deterministic suite now covers single-worker SCHED_RR request/response in both
  JSON and advanced serialization modes.
- **UDP foundation / duplicate semantics:** dgram sharing, reuse, fd binding,
  IPv6-only and unshared-UDP disconnect cases. These belong after TCP cluster
  lifecycle is reliable and otherwise repeat the granular `node:dgram` suite.
- **Platform or privilege dependent:** Windows named pipes/quoting and
  `windowsHide`, UID/GID execution, privileged ports, EACCES/EADDRINUSE text,
  Unix-domain relative paths, and platform-specific signal behavior.
- **Inspector/tooling:** inspect/debug port allocation, preload/profiling,
  coverage, and inspector port clashes. Only deterministic `inspectPort: null`
  validation is retained.
- **Stress/resource pressure:** large IPC payloads, send deadlocks, infinite
  loops, EMFILE/accept failure, leak probes, crash loops, and timing races.
- **Redundant foundation coverage:** generic child-process stdio/error/kill
  behavior, generic net server options, HTTP/TLS ticket behavior, and raw dgram
  semantics already have dedicated granular modules. Cluster cases are kept only
  where worker coordination changes the contract.

No fixture uses a hard-coded port, exact PID, absolute repository path, internet
access, arbitrary readiness sleep, or scheduler-dependent worker ordering.

## Final verification

- Three direct Node v26.5.0 rounds — 41/41 exited successfully each round; Node
  v26.5.1 also completes both maintainer-audit additions.
- Focused maintainer checks: listener validation/bookkeeping, child-side Worker
  shape/send delegation, and advanced-mode TCP handoff all pass.
- Final normal-mode differential run: 43/43 (100%), with no Node failures,
  output differences, compile/link failures, crashes, or skips.

Only the measured cluster module floor was changed in
`node_suite_baseline.json`; its historical full-suite aggregate remains the last
full-suite snapshot rather than an unmeasured extrapolation.

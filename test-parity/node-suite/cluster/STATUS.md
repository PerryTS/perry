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

- 41 granular TypeScript fixtures (40 added over the original one).
- Node v26.5.0: 41/41 complete successfully in three consecutive direct rounds.
- Deno 2.9.2 local comparison: 41/41 complete successfully.
- Bun 1.2.18 local comparison: 40/41 complete successfully; its older local
  release fails the `ChildProcess.channel` ref/unref probe. Current Bun source,
  rather than this older binary, was used for selection evidence.
- Perry differential result: stable 15/41 (36.6%), with 25 behavioral
  differences and one runtime timeout.

## Stable diagnostic boundaries

The granular cases intentionally report, rather than repair, mismatches in areas
such as Worker construction/prototypes, setup aliases/events, empty disconnect
timing, fork/event ordering, worker/cluster message forwarding, worker state and
exit payloads, option validation, and TCP listening.

## Stopping exclusions

The remaining upstream cases were reviewed and stopped in these categories:

- **Blocked by the single-worker TCP result:** round-robin versus shared
  scheduling, multi-worker connection distribution, server restart, backlog,
  pipe handles, socket transfer, and shared-handle races. Adding scheduler
  assertions before basic listening/request-response parity would obscure the
  primary gap.
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

- `deno fmt --check test-parity/node-suite/cluster` — 43 files checked.
- Three direct Node v26.5.0 rounds — 41/41 exited successfully each round.
- Two consecutive warmed focused differential rounds — 15/41 each, 25 behavioral
  differences and one Perry runtime timeout; no Node failures or compile/link
  failures.
- A per-fixture diagnostic run reproduced the same 15 pass, 25 difference, one
  runtime-timeout partition and identified the timeout as invalid
  serialization/inspect-port validation.

Only the measured cluster module floor was changed in
`node_suite_baseline.json`; its historical full-suite aggregate remains the last
full-suite snapshot rather than an unmeasured extrapolation.

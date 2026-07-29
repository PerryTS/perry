# `node:domain` parity evidence

This directory compares Perry with the legacy `node:domain` contract. Each
TypeScript file prints one stable behavior and the parity harness compares that
output and exit status byte for byte.

## Fixed sources

- Node.js `v26.5.0`, commit `bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`:
  `lib/domain.js` and all 50 `test/parallel/test-domain-*.js` files.
- Deno commit `34c46613cbe20450b74c0e8d4f0fd8f6f781d807`:
  `tests/unit_node/domain_test.ts` and `tests/specs/node/domain_timer/`. Direct
  fixture comparison uses the installed Deno `2.9.3` runtime.
- Bun commit `44f6469e0d4ae93467aa65c7e3bc9001000c7b31`: its carried Node domain
  tests under `test/js/node/test/parallel/`. Direct fixture comparison uses the
  installed Bun `1.2.18` runtime.

Node `v26.5.0` is the oracle. Deno and Bun show which contracts other runtimes
also cover; they do not change expected output.

## Coverage map

| Category                   | Fixtures                                                        | Upstream basis                                                             |
| -------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Module and class surface   | `exports`, aliases, descriptors, class and prototype fixtures   | `lib/domain.js` exports and `Domain` definition                            |
| Active state and lifecycle | `create-state`, `enter-*`, `exit-*`, `run-*`, detached receiver | `test-domain-enter-exit`, `test-domain-safe-exit`, `test-domain-run`       |
| Membership                 | `add-*`, `remove-*`, plain object and EventTarget fixtures      | `test-domain-add-remove` and `Domain.prototype.add/remove`                 |
| Bind and intercept         | `bind-*`, `intercept-*`                                         | `test-domain-bind-timeout`, `test-domain-intercept`, Deno `domain_test.ts` |
| EventEmitter routing       | `emitter-*`                                                     | `test-domain-ee*` and the `EventEmitter` overrides in `lib/domain.js`      |
| Async propagation          | next tick, Promise, timeout and immediate fixtures              | `test-domain-nexttick`, `test-domain-promise`, `test-domain-timer`         |

## Validation and runtime comparison

- Node `v26.5.0`: `48/48` fixtures exited cleanly in three runs. Their combined
  stdout SHA-256 was
  `5110542e52596f8c820b7278ecd0afb0856ecbf8cf64352eeb1d88b6bda16d30` in every
  run.
- Perry: three focused harness runs each produced `18/48` parity passes and 30
  stable output differences, with no compile failures, crashes or timeouts.
- Deno `2.9.3`: two runs each produced 17 exact matches, 29 output differences
  and two runtime failures.
- Bun `1.2.18`: two runs each produced two exact matches, 18 output differences
  and 28 runtime failures. The newer Bun source carries Node tests for local
  emitter error listeners, nested routing, next-tick identity, crypto, HTTP and
  VM Promise isolation; the old installed runtime does not implement most of
  that surface.

Perry's stable differences cover export aliases and prototypes, constructor and
receiver checks, live member arrays and domain transfer, bind/intercept wrapper
properties, EventEmitter inheritance and context, and propagation through next
tick, Promise, timeout and immediate callbacks. They are kept as direct Node
contracts rather than hidden by adapters.

## Normalization and exclusions

The fixtures print booleans, stable strings and small counts. They do not print
paths, stacks, PIDs or durations, so this suite adds no domain-specific
normalization. The shared runner only removes its documented volatile duration,
test-location and stack-frame data from both sides.

Fatal no-handler paths, abort flags, signals, GC/finalization, scheduler races,
fixed ports and global shutdown tests are excluded. Nested fatal routing and
uncaught async error metadata are also excluded because they require process
termination semantics that Perry cannot exercise without turning the fixture
into a provider or process-management test. The fs callback control crashes in
Perry without any domain, so it belongs to the fs provider suite. HTTP and
crypto integration tests are also omitted because their domain-specific
contracts are already isolated here without network or random data.

The suite stops when every deterministic contract in Node `v26.5.0` either has
one focused fixture, is already covered by an equivalent fixture, or falls under
an exclusion above.

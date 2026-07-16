# `node:async_hooks` granular parity suite

This directory tests deterministic public behavior from `node:async_hooks`.
Fixtures assert ID relationships rather than exact numeric IDs and use explicit
completion barriers for asynchronous work.

## Upstream comparison

The expansion was reviewed on 2026-07-16 against primary repository sources:

- Node.js main at
  [`34c28d5`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test/async-hooks),
  especially the AsyncResource lifecycle, AsyncLocalStorage nesting,
  enable/disable, promise, and async/await cases, plus its
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

The current focused run is **51/77**. Twenty-six stable diagnostic fixtures cover
custom AsyncResource lifecycle delivery, snapshot receiver handling,
execution-resource restoration, disable cleanup, hook-sensitive type
validation, zlib callback propagation, stream.finished completion, simultaneous
hooks, init resource arguments, throwing scope lifecycle, and caught async exit
rejections. They also identify context loss across subprocess, HTTP, net, dgram
event, worker event, readline, and events.on provider boundaries. The other 51
fixtures match the pinned oracle.

## Coverage

- `resource/`: construction/type and ID invariants, scope/receiver/arguments,
  instance bind, deterministic hook scope callbacks, and explicit destroy.
- `storage/`: run nesting and restoration, independent instances, enterWith,
  exit and its async descendants, multiple store value types, disable/re-entry,
  repeated disable, and promise-boundary behavior.
- `static/`: AsyncLocalStorage bind/snapshot and AsyncResource.bind context,
  empty and populated captures, receiver, argument, return-value, re-entry, and
  restoration behavior.
- `propagation/`: controlled concurrent promises, catch/finally, thenables,
  async iterators, queueMicrotask, nextTick, immediate, interval, and timer
  propagation with awaited or callback-driven completion barriers.
- `integrations/`: fs callbacks/promises/streams, crypto callbacks, zlib,
  Readable/Writable/Transform/finished, timers/promises, util.promisify, and
  EventEmitter propagation selected from Bun's Node comparison matrix.
- `hooks/`: enable/disable/re-enable, simultaneous observers, init resource
  arguments, and lifecycle behavior when a scoped callback throws.
- `providers/`: DNS, child processes, HTTP, net, dgram, workers, readline,
  events.on, and stream async iterators with ephemeral ports and explicit
  close/exit barriers. This directory runs in the sequential lane.
- `validation/`: synchronous throw propagation and cleanup of storage and
  execution-resource state, plus hook-sensitive empty resource type behavior.
  The pre-existing root fixtures retain detailed callback, constructor, and
  hook-option argument validation.

## Stopping judgment and exclusions

The remaining upstream cases are intentionally not copied into this fast,
granular module suite:

- HTTPS/TLS provider propagation requires certificate fixtures and remains a
  separate batch so certificate setup does not obscure context assertions.
- Exact numeric async IDs are runtime-specific; only relationships and
  restoration invariants are asserted here.
- GC-driven destroy delivery, weak-reference collection, destroy-vs-scheduler
  priority, recursive hooks, deep stacks, stress/leak probes, and process
  shutdown require slow or timing-sensitive runners.
- Worker/thread propagation requires the worker suite and is not a
  single-thread AsyncLocalStorage contract.
- Uncaught exception and unhandled rejection routing mutates process-global
  handlers and belongs in a dedicated isolation fixture.
- Node 26.5.0 supports AsyncLocalStorage `defaultValue` and `name`, but Perry's
  API manifest does not claim those constructor/name surfaces, so they are not
  counted here.

The raw hook cases retained here are limited to a user-created AsyncResource:
its init callback is synchronous, its before/callback/after sequence is fully
controlled, and explicit `emitDestroy()` is observed behind a setImmediate
barrier. These cases are deterministic and expose lifecycle gaps without
depending on provider internals or GC timing.

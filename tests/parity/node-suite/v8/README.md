# node:v8 granular parity suite

Deterministic Node.js compatibility coverage for Perry's `node:v8` surface. The
fixtures compare semantic contracts rather than V8-internal byte streams,
volatile heap sizes, engine addresses, stacks, timestamps, or GC timing.

## Pinned primary evidence

Node 26.5.0 is the oracle. Its tag resolves to commit
[`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb).
The audit used:

- [`lib/v8.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/v8.js)
- [`doc/api/v8.md`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/doc/api/v8.md)
- [`test-v8-serdes.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-v8-serdes.js)
- [`test-v8-stats.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-v8-stats.js)
- [`test-v8-version-tag.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-v8-version-tag.js)
- [`test-v8-flag-type-check.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-v8-flag-type-check.js)
- [`test-v8-getheapsnapshot-twice.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-v8-getheapsnapshot-twice.js)
- the `test-promise-hook-*` and `test-v8-startup-snapshot-api.js` families at
  the same commit

The cross-runtime audit used Deno 2.9.3 stable commit
[`f39575ecd50602a5b42b1ba8e93849460de9fcf4`](https://github.com/denoland/deno/blob/f39575ecd50602a5b42b1ba8e93849460de9fcf4/ext/node/polyfills/v8.ts)
and current source commit
[`803a3c933e1e23e0972445293ec0b34b8da96ccc`](https://github.com/denoland/deno/blob/803a3c933e1e23e0972445293ec0b34b8da96ccc/ext/node/polyfills/v8.ts),
plus Bun 1.3.14 stable commit
[`0d9b296af33f2b851fcbf4df3e9ec89751734ba4`](https://github.com/oven-sh/bun/blob/0d9b296af33f2b851fcbf4df3e9ec89751734ba4/src/js/node/v8.ts)
and current source commit
[`aca54d5c2b874ac304a3bbe1d67630e4daf17b43`](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/src/js/node/v8.ts).

## Coverage (46 fixtures)

- **Exports and classes (8):** exact ESM surface, descriptors, import identity,
  constructor/prototype metadata, inheritance, instance identity, and receiver
  validation.
- **Structured clone helpers (12):** primitives and special numbers, sparse
  arrays, objects, Map/Set identity, typed arrays, Buffer/DataView/ArrayBuffer,
  Date/RegExp, Error cause, Float16Array, shared/cyclic graphs, corrupt wire
  data, invalid sources, and invalid values.
- **Serializer classes (8):** headers and sequential values, raw integer/double
  methods, raw Buffer/TypedArray/DataView input, release reuse, wire version,
  ArrayBuffer transfer identity, validation, and every Node 26 ArrayBufferView
  input family.
- **Heap diagnostics (7):** stable heap/code/space/C++ statistic shapes and
  types, parsed heap-snapshot schema, two consecutive streams, isolated file
  output, and cleanup.
- **Promise hooks (5):** namespace metadata, validation, stopper behavior,
  parent identity, and filtered init/before/settled/after ordering.
- **GCProfiler (2):** lifecycle/report shape and `Symbol.dispose` behavior.
- **Version flags (2):** repeatable uint32 tag and isolated flag/type behavior.
- **String representation (1):** stable Latin-1/BMP/astral cases and validation.
- **Startup snapshots (1):** ordinary-process surface and
  `ERR_NOT_BUILDING_SNAPSHOT` boundary.

The module runs in the sequential lane because heap snapshots are native,
memory-heavy work and `setFlagsFromString()` mutates process-global V8 state.
Every parity fixture still executes in a fresh process.

## Cross-runtime divergences

- Deno backs the serializer classes with V8 and rewrites wire version 16 to 15
  for Node interoperability. It omits several modern exports and its
  `startupSnapshot` callbacks are surface-only rather than Node's ordinary-mode
  errors.
- Bun uses JavaScriptCore serialization, so its wire bytes intentionally do not
  interoperate with Node/Deno. Most serializer lifecycle, promise-hook, flag,
  and profiler APIs are stubs. Bun's heap statistics are explicitly approximate.
- Node remains the assertion oracle. These engine-design differences are
  documented rather than hidden by weakening Node contracts.

## Deliberate stopping boundary

Excluded from this deterministic lane:

- exact serialized bytes or cross-engine wire compatibility;
- Node-internal host objects and private `_writeHostObject`, `_readHostObject`,
  or `_getDataCloneError` overrides; these require internal bindings and are not
  public `node:v8` contracts;
- pinned legacy wire blobs and unaligned legacy typed-array bytes, which test
  exact engine wire compatibility rather than semantic round trips;
- exact heap sizes, addresses, space population, GC/finalization timing,
  retention and `queryObjects()` counts;
- raw CPU/heap profile and coverage payloads, inspector coupling, code-cache
  bytes, memory pressure, leak/stress, crash/fault injection, and concurrency;
- `setHeapSnapshotNearHeapLimit()` and other near-OOM behavior;
- startup snapshot building, which requires process flags and generated
  artifacts. Ordinary-process behavior is already exhausted by the focused
  startup fixture.

Heap snapshot fixtures parse only stable schema relationships, consume or
destroy streams, write inside an isolated temporary directory, and remove all
files. The valid flag fixture is process-isolated and cannot leak into another
fixture.

## Measured Perry classification

Repeated Node 26.5.0 oracle runs produced identical output for all 46 fixtures.
Repeated focused Perry differential runs produced the same **14/46** result: 14
passes, 30 output mismatches, one stable SIGSEGV (`serialize/builtins` after
printing its semantic observations), one stable timeout
(`serialize/references-cycles`), and zero compile failures. The compact runner
classifies the signal mismatch in its `diff` bucket, yielding
`pass=14, diff=31, perry_err=1`.

Validation also includes Node syntax checks for every fixture,
`deno fmt
--check`, `cargo fmt --all -- --check`, the Rust file-size gate, JSON
parsing and baseline/count invariants, release builds for the compiler/runtime
packages, `git diff --check`, and staged-scope/artifact checks.

These failures are intentional parity targets. This tests-only suite does not
change compiler, linker, runtime, or build behavior to conceal them.

# `node:wasi` granular parity suite

Deterministic Node 26.5.0 oracle cases for Perry's `node:wasi` compatibility
layer. The suite has 45 focused fixtures in five groups:

- `classes/` (5): ESM/CommonJS export shape, constructor/prototype/instance
  descriptors, call-without-`new`, and a warning-event assertion that normalizes
  the experimental warning to a count rather than comparing PID or stderr text.
- `constructor/` (6): options/version, args, env, preopens, stdio descriptors,
  and `returnOnExit` validation. Preopens stop at type/empty-object validation;
  no host path is opened.
- `imports/` (7): the complete 46-function preview1 surface, function metadata,
  preview1/unstable namespace identity, ordinary wrapper descriptors,
  replacement behavior that preserves the selected namespace, method receivers,
  and pre-start syscall validation.
- `lifecycle/` (23): input/export validation, memory binding, single-start
  rules, start/initialize exclusivity, entrypoint invocation, return-on-exit
  behavior, patched-import errors, real wasm instance shape, imported-function
  linking, failure-state transitions, explicit-memory override and option
  validation, and cross-realm memory acceptance.
- `semantics/` (4): UTF-8 argument/environment encoding, constructor-time
  snapshots, and empty defaults, plus predicate-only clock and zero-length
  random behavior. No random bytes or wall-clock values are compared.

The fixtures use `const W: any = WASI; new W(...)` intentionally. Perry's typed
`new WASI(...)` path currently bypasses the native WASI constructor, which is a
separate compiler-dispatch gap and would prevent these tests from reaching the
API implementation under test.

## WebAssembly fixtures

`fixtures/` checks in three tiny wasm binaries beside readable WAT provenance:

- `counter-command`: exports memory and `_start`, which stores `42` at byte 0.
- `counter-reactor`: exports memory and `_initialize`, which stores `43` at
  byte 0.
- `exit-7-command`: imports preview1 `proc_exit` and calls it with status 7.

Regenerate every binary without adding a repository dependency:

```sh
set -eu
for source in test-parity/node-suite/wasi/fixtures/*.wat; do
  npx -y -p wabt@1.0.39 wat2wasm "$source" -o "${source%.wat}.wasm"
done
```

The real-wasm tests do not substitute plain objects when Perry's loader lacks
Node's standard `Promise<{ module, instance }>` shape. Separate lifecycle API
cases use a real `WebAssembly.Memory` under Node and a guarded object fallback
under Perry so loader limitations and WASI lifecycle limitations remain
separately observable.

## Upstream comparison

Coverage was compared against primary sources at these revisions:

- Node.js 26.5.0 (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`):
  [`lib/wasi.js`](https://github.com/nodejs/node/blob/v26.5.0/lib/wasi.js) and
  [`test/wasi`](https://github.com/nodejs/node/tree/v26.5.0/test/wasi), plus the
  documented
  [`finalizeBindings()` contract](https://github.com/nodejs/node/blob/v26.5.0/doc/api/wasi.md#wasifinalizebindingsinstance-options).
  The constructor and start/initialize validation, `finalizeBindings()`
  memory/options validation, return-on-exit, eager args/env snapshots, and
  bounded clock/random contracts are represented here.
- Deno (`803a3c933e1e23e0972445293ec0b34b8da96ccc`):
  [`ext/node/polyfills/wasi.ts`](https://github.com/denoland/deno/blob/803a3c933e1e23e0972445293ec0b34b8da96ccc/ext/node/polyfills/wasi.ts).
  Its current preview1 implementation follows most Node constructor, import,
  memory-brand, and not-started validation, but validates entrypoints before
  consuming lifecycle state, keeps `finalizeBindings()` idempotent, and exposes
  `wasiImport` as a getter. Its `finalizeBindings()` also treats null memory or
  options as absent instead of rejecting them; its constructor does match Node's
  eager args/env snapshots, but stringifies undefined env values instead of
  omitting them. No separate checked-in `node:wasi` compatibility selection was
  present at that revision.
- Bun (`aca54d5c2b874ac304a3bbe1d67630e4daf17b43`):
  [`src/js/node/wasi.ts`](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/src/js/node/wasi.ts)
  and the
  [preview1 fixture harness](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/test/js/node/test/fixtures/wasi-preview-1.js).
  Bun's selected fixture exercises preview1 imports and start behavior, while
  its implementation retains legacy `getImports()`/optional-memory behavior,
  lacks Node's initialize/finalize helpers, and uses realm-sensitive memory
  branding. It also retains the caller's args array and env object, so mutations
  after construction remain visible, and does not omit undefined env values;
  Node 26.5.0 remains the oracle.

The direct Node mapping is: `test-wasi-options-validation.js` to `constructor/`;
`test-wasi-start-validation.js` and `test-wasi-initialize-validation.js` to the
matching `lifecycle/` validation, state, and execution cases;
`test-return-on-exit.js` to the three `return-on-exit-*` cases plus
patched-import rethrow; `test-wasi-not-started.js` to
`imports/syscall-before-start.ts`; and `test-wasi-main_args.js` to
`semantics/args-exposure.ts`. The portable parts of `test-wasi-clock_getres.js`
and `test-wasi-gettimeofday.js` map to `semantics/clock-random.ts`, which checks
both realtime and monotonic clock success with positive-value predicates. That
fixture limits random coverage to the deterministic zero-length no-write
boundary; it does not claim Node's nonzero `test-wasi-getentropy.js` behavior.
The documented `finalizeBindings(instance[, options])` memory/options contract
maps to the `finalize-*` lifecycle cases, including invalid explicit memory and
null options; Node's checked-in WASI tests use its valid external-memory path
only for the separately excluded pthread harness. Generic invalid
`instance`/`instance.exports` branches are already isolated for both public
lifecycle entry methods and are not duplicated for `finalizeBindings()`. Node's
eager `Array.prototype.map`/`Object.entries` copies in `lib/wasi.js` map to
`semantics/options-snapshot.ts`; Deno matches those snapshots, while Bun's
current implementation retains both caller-owned inputs by reference. The same
fixture verifies that omitted args/env report zero count and byte size through
the two `*_sizes_get` calls. It intentionally skips `*_get` for empty lists:
Node 26 passes an empty native vector to uvwasi and returns `EINVAL`, while Deno
and Bun return success, an implementation-specific errno edge rather than the
portable documented default.

## Measured result and stopping evidence

With Node 26.5.0, a `perry-dev` compiler/runtime build, and the optional wasm
host archive, focused runs were stable at **15/45**, with **30 behavioral
diffs**, no compile failures, no timeouts, and no harness errors. A related
`globals,wasi` run completed at **127/165** (`globals` 112/120 and `wasi`
15/45), also without compile failures or timeouts. The stable mismatch families
are:

- module namespace and descriptor/enumerability differences plus no normalized
  experimental warning;
- import-function name/arity and receiver differences, plus loss of the
  `wasi_unstable` namespace after replacing `wasiImport`;
- import syscalls return `28` before memory binding instead of throwing
  `ERR_WASI_NOT_STARTED`;
- `WebAssembly.Memory` construction/branding and standard async instance shape
  differ, while the optional wasm host uses Perry's synchronous opaque handle;
- lifecycle methods validate but do not invoke `_start`/`_initialize`, do not
  consume state after post-bind validation failures, do not implement exit-code
  flow, do not bind or honor explicitly overridden syscall memory, and do not
  validate `finalizeBindings()` memory/options;
- args/env encoding and constructor snapshots plus clock/random semantics remain
  unavailable behind those memory/syscall gaps.

The suite stops before upstream filesystem/fd cases because the core standard
wasm instance, memory binding, and syscall lifecycle are not yet stable. It also
excludes sockets, threads, preview2/component model, external runtimes,
platform-specific errno or error text, actual entropy/time values, large or
concurrent modules, permissions/locking, symlink escape, signals,
GC/finalization, worker termination, and stress. Those require separate
WASI/runtime/compiler work and would be redundant or less diagnostic here.
Node's explicit cross-realm `WebAssembly.Instance` validation case is not a
separate fixture: current `lib/wasi.js` validates the instance structurally and
brands only its memory, so `lifecycle/cross-realm-memory.ts` exercises the
distinct cross-realm WASI contract without duplicating the same Perry failure.

## Verification

From the repository root:

```sh
cargo build --profile perry-dev \
  -p perry -p perry-runtime-static -p perry-stdlib-static -p perry-wasm-host
test "$(node --version)" = "v26.5.0"
NODE_BIN="$(command -v node)" \
PERRY_RUNTIME_DIR="$PWD/target/perry-dev" \
python3 scripts/node_suite_run.py target/perry-dev/perry "$PWD" wasi
```

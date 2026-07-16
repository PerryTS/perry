# `node:wasi` granular parity suite

Deterministic Node 26.5.0 oracle cases for Perry's `node:wasi` compatibility
layer. The suite has 51 focused fixtures in five groups:

- `classes/` (6): ESM/CommonJS export shape, constructor/prototype/instance
  descriptors, ordinary subclass construction, call-without-`new`, and a
  warning-event assertion that separates import-time from construction-time
  emission and normalizes each phase to a count rather than comparing PID or
  stderr text.
- `constructor/` (7): options/version, args, env, preopens, stdio descriptors,
  `returnOnExit` validation, and observable option-property access order.
  Preopens stop at type/empty-object validation; no host path is opened.
- `imports/` (8): the complete 46-function preview1 surface, function metadata,
  preview1/unstable namespace identity, ordinary wrapper descriptors,
  replacement behavior that preserves the selected namespace, method receivers,
  pre-start syscall validation, and representative arity/type validation before
  and after memory binding.
- `lifecycle/` (25): input/export validation, memory binding, single-start
  rules, start/initialize exclusivity, entrypoint invocation, return-on-exit
  behavior, patched-import errors, real wasm instance shape, imported-function
  linking, exports-accessor ordering, failure-state transitions (including
  finalization exclusivity), explicit-memory override and option validation, and
  cross-realm memory acceptance.
- `semantics/` (5): UTF-8 argument/environment encoding, embedded-NUL
  termination, constructor-time snapshots, and empty defaults, plus
  predicate-only clock and zero-length random behavior. No random bytes or
  wall-clock values are compared.

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
  [`lib/wasi.js`](https://github.com/nodejs/node/blob/v26.5.0/lib/wasi.js),
  [`src/node_wasi.cc`](https://github.com/nodejs/node/blob/v26.5.0/src/node_wasi.cc),
  and [`test/wasi`](https://github.com/nodejs/node/tree/v26.5.0/test/wasi), plus
  the documented
  [`finalizeBindings()` contract](https://github.com/nodejs/node/blob/v26.5.0/doc/api/wasi.md#wasifinalizebindingsinstance-options).
  The constructor and start/initialize validation, `finalizeBindings()`
  memory/options validation and shared lifecycle state, return-on-exit, eager
  args/env snapshots, and bounded clock/random contracts are represented here.
- Deno (`803a3c933e1e23e0972445293ec0b34b8da96ccc`):
  [`ext/node/polyfills/wasi.ts`](https://github.com/denoland/deno/blob/803a3c933e1e23e0972445293ec0b34b8da96ccc/ext/node/polyfills/wasi.ts)
  and its 14-case
  [`tests/unit_node/wasi_test.ts`](https://github.com/denoland/deno/blob/803a3c933e1e23e0972445293ec0b34b8da96ccc/tests/unit_node/wasi_test.ts)
  selection. Its current preview1 implementation follows most Node constructor,
  import, memory-brand, and not-started validation, but validates entrypoints
  before consuming lifecycle state, snapshots `instance.exports` once per entry
  method, keeps `finalizeBindings()` idempotent, and exposes `wasiImport` as a
  getter. The instance descriptor fixture reports that inherited accessor
  separately from Node's own writable data property instead of assuming either
  shape. Its `finalizeBindings()` also treats null memory or options as absent
  instead of rejecting them; its constructor does match Node's eager args/env
  snapshots, but stringifies undefined env values instead of omitting them. The
  portable version, namespace, lifecycle, args/env, import-surface,
  finalization, and warning cases selected by Deno are represented here. Its
  warning is constructor-triggered (`0` after import, `1` after two
  constructions), while Node emits once at module load (`1`, then still `1`).
  Bun and Perry emit neither phase. Deno's mixed coercion case is split
  deliberately: its args/env assertions are represented, but its non-empty
  preopen object coercion is not. That preopen portion, the standalone preopen
  case, and hello-world `fd_write` cross the host-fd boundary. Its getter-only
  `wasiImport` rejects replacement while `getImportObject()` continues to expose
  the original object. Its JavaScript wrappers check memory before syscall
  arguments and otherwise defer to op coercion, unlike Node's native
  arity/type-first validation.
- Bun (`aca54d5c2b874ac304a3bbe1d67630e4daf17b43`):
  [`src/js/node/wasi.ts`](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/src/js/node/wasi.ts)
  plus its four-case
  [`test/js/bun/wasm/wasi.test.js`](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/test/js/bun/wasm/wasi.test.js)
  selection and the
  [preview1 fixture harness](https://github.com/oven-sh/bun/blob/aca54d5c2b874ac304a3bbe1d67630e4daf17b43/test/js/node/test/fixtures/wasi-preview-1.js).
  Bun's selected fixture exercises preview1 imports and start behavior, while
  its implementation retains legacy `getImports()`/optional-memory behavior,
  lacks Node's initialize/finalize helpers, and uses realm-sensitive memory
  branding. Its `wasiImport` property can be replaced, but it lacks Node's
  `getImportObject()` method; the identity and wrapper-descriptor fixtures
  normalize that absence instead of terminating before reporting it. Its missing
  `initialize()` is likewise reported explicitly by the direct and real-wasm
  reactor execution fixtures; the command-linking fixture reports the missing
  import-object helper without substituting Bun's legacy `getImports()` API. It
  also accepts `args: null` instead of applying Node's non-array validation,
  retains the caller's args array and env object so mutations after construction
  remain visible, does not omit undefined env values, and retains payload bytes
  after embedded NULs like Deno; Node 26.5.0 truncates each native
  argument/environment string at its first NUL and remains the oracle. Bun does
  support ordinary WASI subclass construction. The args/env/clock semantic
  fixtures use Bun's existing optional-memory `start(instance, memory)` only
  when the standard `initialize()` helper is absent, allowing its import
  behavior to be compared without changing the Node/Deno/Perry path. Bun's
  numeric/non-string args fail during `args_sizes_get`, and its two
  `clock_res_get` calls throw while time calls succeed. Its zero-length
  `random_get` returns success but changes the guarded eight-byte range, unlike
  Node and Deno; the thrown outcomes are normalized by name/code rather than
  engine text. Bun's selected standalone hello-world runner is an external
  runtime case, while its descriptor-rights, failed-open errno, and path-escape
  cases all require non-empty preopens and host filesystem operations.

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
`finalizeBindings()` one-way lifecycle transition maps to
`lifecycle/finalize-state.ts`, which isolates both transition directions for the
public entry methods from the unrelated explicit-memory validation gaps. Node's
parameter destructuring is covered in
`lifecycle/finalize-options-validation.ts`: Node reads an `options.memory`
accessor before instance/state validation, Deno reads it only after instance
validation, and Perry currently ignores the second argument entirely. Node's
eager `Array.prototype.map`/`Object.entries` copies in `lib/wasi.js` map to
`semantics/options-snapshot.ts`; Deno matches those snapshots, while Bun's
current implementation retains both caller-owned inputs by reference. The same
fixture verifies that omitted args/env report zero count and byte size through
the two `*_sizes_get` calls. It intentionally skips `*_get` for empty lists:
Node 26 passes an empty native vector to uvwasi and returns `EINVAL`, while Deno
and Bun return success, an implementation-specific errno edge rather than the
portable documented default. `semantics/nul-termination.ts` separately covers
the preview1 C-string boundary: it compares reported sizes and a tail-payload
predicate rather than raw bytes. Node truncates at the first NUL, while Deno and
Bun retain bytes after the first guest-visible terminator. The native
`SlowCallback` arity/type checks in `src/node_wasi.cc` map to
`imports/syscall-arguments.ts`. It samples both uint32 and BigInt signatures
before and after binding; exhaustive repetition across all 46 wrappers would be
redundant because they share the same callback template. The upstream lifecycle
validation tests override `instance.exports` with a getter. The representative
callbacks in `imports/function-descriptors.ts` also cover own-`prototype` shape:
Node replaces every native callback with a bound function, Deno defines
object-literal methods, and Bun uses arrow functions, so none has an own
constructor prototype; Perry currently gives each callback an object-valued own
`prototype`. The same fixture checks one ordinary callback's independent
constructibility with valid arity: Node creates its shared native callback
template with `ConstructorBehavior::kThrow`, while Deno's method and Bun's arrow
also reject construction; Perry currently constructs an object. It does not use
the replaced `proc_exit` wrapper for this check because Node's bound exit helper
throws its private exit sentinel when invoked.
`lifecycle/exports-access-order.ts` makes that observable ordering explicit:
Node reads it for default memory, validation, and entrypoint lookup, while Deno,
Bun, and Perry snapshot at most once per implemented entry method. Its export
member accessors separately show Node reading `memory`, `_start`, and
`_initialize` exactly once in that order for both entry methods. Deno and Bun
re-read members around validation and invocation; Perry matches the Node member
order for `start()` but does not invoke it, while `initialize()` stops after
reading `memory` and `_start` without reading or invoking `_initialize`.
`constructor/options-access-order.ts` applies the same accessor-based method to
the constructor's complete option surface. Node's `lib/wasi.js` reads version
twice, args/env/preopens and `returnOnExit` three times, and each stdio property
once in a stable sequence. Deno performs additional validation/default reads,
Bun reads only its legacy preopens/env/args inputs, and Perry reads every option
once in a different order.

## Measured result and stopping evidence

With Node 26.5.0, a `perry-dev` compiler/runtime build, and the optional wasm
host archive, focused runs were stable at **17/51**, with **34 behavioral
diffs**, no compile failures, no timeouts, and no harness errors. A related
`globals,wasi` run completed at **129/171** (`globals` 112/120 and `wasi`
17/51), also without compile failures or timeouts.

An independent bounded sweep of all fixtures completed under Deno with 50
status-0 results and the one intentional status-7 exit, and under Bun with 48
status-0 results and the three intentional status-7 exits. Neither sweep had a
timeout or another nonzero result.

The stable mismatch families are:

- module namespace, descriptor/enumerability (including an enumerable,
  configurable, writable constructor `prototype` property), and
  subclass-construction differences plus no normalized experimental warning;
- import-function name/arity, own-`prototype`, constructibility, and receiver
  differences, plus loss of the `wasi_unstable` namespace after replacing
  `wasiImport`;
- import syscalls return `28` before memory binding instead of throwing
  `ERR_WASI_NOT_STARTED`;
- `WebAssembly.Memory` construction/branding and standard async instance shape
  differ, while the optional wasm host uses Perry's synchronous opaque handle;
- lifecycle methods validate but do not invoke `_start`/`_initialize`, and
  `initialize()` does not even read the latter; they do not consume state after
  post-bind validation failures, implement exit-code flow, bind or honor
  explicitly overridden syscall memory, or validate `finalizeBindings()`
  memory/options;
- args/env encoding and constructor snapshots plus clock/random semantics remain
  unavailable behind those memory/syscall gaps.

The suite stops before upstream filesystem/fd cases because the core standard
wasm instance, memory binding, and syscall lifecycle are not yet stable. It also
excludes sockets, threads, preview2/component model, external runtimes,
platform-specific errno or error text, actual entropy/time values, large or
concurrent modules, permissions/locking, symlink escape, signals,
GC/finalization, worker termination, and stress. Those require separate
WASI/runtime/compiler work and would be redundant or less diagnostic here. All
26 JavaScript entrypoints in Node 26.5.0's `test/wasi` directory were enumerated
during the final audit: each is mapped above, represented by a more focused
local case, or named in the exclusions below. The same recursive pinned-tree
audit accounts for every Deno and Bun selection: Deno's hello-world fd-write
case, standalone preopen case, and preopen portion of its mixed coercion case,
plus all four Bun cases, remain in the corresponding fd/preopen,
external-runtime, errno, rights, or path-escape exclusions. Concretely, the
remaining Node 26 files `test-wasi-cant_dotdot.js`,
`test-wasi-fd_prestat_get_refresh.js`, `test-wasi-ftruncate.js`,
`test-wasi-io.js`, `test-wasi-notdir.js`, `test-wasi-preopen_populates.js`,
`test-wasi-readdir.js`, `test-wasi-stat.js`, `test-wasi-stdio.js`,
`test-wasi-symlinks.js`, and `test-wasi-write_file.js` all cross that host-fd
boundary. `test-wasi-getentropy.js`, `test-wasi-getrusage.js`, and
`test-wasi-poll.js` require non-deterministic values or platform scheduling;
`test-wasi-sock.js`, `test-wasi-pthread.js`, and `test-wasi-worker-terminate.js`
stay in the explicitly separated socket, thread, and worker categories.
`test-wasi-exitcode.js` is already represented by the isolated
default/true/false `returnOnExit` fixtures rather than copied as another
external-process harness. Node's explicit cross-realm `WebAssembly.Instance`
validation case is not a separate fixture: current `lib/wasi.js` validates the
instance structurally and brands only its memory, so
`lifecycle/cross-realm-memory.ts` exercises the distinct cross-realm WASI
contract without duplicating the same Perry failure.

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

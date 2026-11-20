# `node:vm` granular parity

This directory tests Perry's documented `node:vm` surface with independent,
print-and-diff fixtures. Node 26.5.0 is the oracle. Every executable source
passed to `Script`, `runIn*Context`, or `compileFunction` is statically declared
in the fixture, so failures in the core groups are not hidden dependencies on
runtime source discovery.

## Coverage

- `api/`, `imports/`, and `require/`: export identity, classes, constants,
  callable metadata, property descriptors, ESM/CommonJS shape, and the builtin
  module lookup path.
- `context/` and `validation/`: `createContext()`/`isContext()`, object and
  array sandboxes, `DONT_CONTEXTIFY`, name/origin and code-generation
  validation, string-code generation policy, and deterministic `microtaskMode`
  draining. The `context/properties/` group covers accessor forwarding,
  descriptors, writable/configurable behavior, deletion, inheritance, circular
  references, symbol keys, and own-key enumeration.
- `execution/`: main-context bindings, isolated new contexts, persistent context
  mutation/lexicals, receiver identity, repeated Script execution, and
  `createScript()` construction.
- `script/` and `metadata/`: constructor/run option validation, stable error
  class/code checks, filename/offset presence, `displayErrors`, source-map URL,
  cached-data acceptance/rejection, and deprecated produced-cache properties.
- `compile-function/`: parameters, parsing contexts, context extensions,
  arguments, receiver behavior, validation, and portable cache observations.
- `cross-context/`: built-in/prototype identity, structured values, descriptors,
  errors, and promises without raw inspector or engine-specific stack output.
- `modules/`: the separately gated, documented `Module`, `SourceTextModule`, and
  `SyntheticModule` lifecycle, validation, namespace, linking, evaluation, and
  cached-data subset. These fixtures use `--experimental-vm-modules` and
  `PERRY_EXPERIMENTAL_VM_MODULES=1` explicitly.

## Upstream selection evidence

The selection was reviewed on 2026-07-16 against these primary repositories:

- Node.js `v26.5.0` (`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`):
  `test/parallel/test-vm-*`, `test/es-module/test-vm-*`,
  `test/sequential/test-vm-*`, and `doc/api/vm.md`.
- Deno main (`c99e6904d8e297712ba859a64bbe848532d8f90f`):
  `tests/unit_node/vm_test.ts`, `tests/specs/node/vm_*`, and
  `ext/node/polyfills/vm.js`.
- Bun main (`0ecd508247c7e99477717389a6cad44552cac023`): `test/js/node/vm/`, its
  vendored `test/js/node/test/parallel/test-vm-*` selection, and
  `src/js/node/vm.ts`.

The fixtures intentionally reduce those upstream assertions to stable semantic
output: booleans, values, property flags, error names, and error codes. They do
not compare absolute paths, raw error messages, full stacks, cache bytes, or
object inspection order.

## AOT boundary and stopping evidence

Perry documents a V8-free, narrowed constant-source VM evaluator. Consequently:

- `code-generation-strings.ts` and `run-new-code-generation.ts` record
  `eval`/`new Function` policy behavior, but Perry mismatches there are
  deliberate runtime-generated-code AOT exclusions. They are kept separate from
  literal-source context behavior.
- Literal-source mutation, isolation, receiver, lexical, option-validation,
  error, compile-function, and cross-context failures are genuine gaps in the
  currently claimed VM subset, not dynamic-source discovery failures.
- Dynamic import callbacks, `importModuleDynamically`, default/custom loaders,
  network imports, and import-meta initialization require separate loader work.
- Inspector/debug breaks, signals, `breakOnSigint`, timeout races/infinite
  loops, escaped promise timeouts, and parse-abort behavior are excluded because
  they need process control or can hang the granular runner.
- Context `name` and `origin` type validation and acceptance are covered, but
  their deeper metadata effects are inspector-facing and stay with inspector
  integration work.
- `measureMemory()` exact values, GC/weak-reference/leak/stress tests, and code
  cache byte identity are engine- or environment-specific. Only portable shapes
  and acceptance state are covered.
- WebAssembly code-generation execution, code-cache corruption fuzzing, exact
  syntax/stack strings, and source-map path formatting remain separate risky
  categories.
- Proxy-backed sandboxes were evaluated from Node/Bun's selection. Node 26.5.0
  completes deterministically, but Perry currently exits with `SIGSEGV`; the
  crashing probe is excluded from this print-and-diff suite and requires
  separate runtime crash coverage.
- Node 26.5.0 aborts the process for the invalid `compileFunction()` parameter
  name `"a-b"` on the tested macOS build, so that unsafe oracle case is
  excluded; type validation around it remains covered.

The remaining Node/Deno/Bun cases are therefore redundant with these semantic
contracts, outside Perry's documented AOT model, experimental loader work, or
belong to one of the unreliable categories above.

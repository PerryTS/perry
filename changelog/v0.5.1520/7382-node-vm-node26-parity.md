### Fixed

- **Completed Node.js 26.5.0 parity for `node:vm`.** Contexts now preserve
  sandbox, lexical, descriptor, strict-write, code-generation, microtask, and
  cross-realm behavior; `Script`, `compileFunction`, cached-data metadata, and
  experimental VM modules now match the Node oracle across the full 64-case
  module suite.

### Performance

- **One array literal in `new Function(…)` source no longer disarms the
  array-index fast path for the whole process.** The interpreter links every
  literal it builds to its creation realm's intrinsic prototype, but a plain
  `new Function(…)` / `eval(…)` body runs with `intrinsics == globalThis`, so
  that prototype is the one the value already resolves to. Recording it changed
  nothing observable while latching the process-wide
  `ARRAY_TARGET_PROTO_RECORDED` flag (standing `plain_array_index_guard` down
  permanently), bumping the prop-plan epoch and retiring every element-shape
  proof — on every literal. Every ajv / fast-json-stringify / find-my-way
  validator paid it. The record is now made only when the creation realm's
  prototype actually differs from the base-realm default, which is exactly the
  cross-realm `vm` case it was added for.

### Fix large JSON defines stalling LLVM code generation

- Lower large JSON-compatible object/array literals, including build-time
  defines, to the same serialized-string `JSON.parse` intrinsic used for JSON
  imports. The threshold is 1,024 value nodes or 64 KiB of key/string data,
  independent of generated function instruction budgets.
- Keep parsing at the original evaluation site, preserving fresh values on
  repeated reads and avoiding evaluation in untaken branches. Existing
  `typeof` folding and define inputs to both cache keys are unchanged.
- Preserve property order, string escaping and negative zero; fall back for
  JavaScript-specific constructs such as prototype setters, holes, spreads,
  getters and non-finite numbers.
- Add threshold unit tests and timed compile/run regressions for a >1 MiB
  define, cache invalidation, small direct literals, fresh nested objects,
  shadowed `JSON` bindings and array behavior.

Fixes #10151.

### Fix large JSON defines stalling LLVM code generation

- Lower large JSON-compatible literals, including build-time defines, to the
  serialized-string `JSON.parse` intrinsic used for JSON imports. Flat arrays
  of primitives use the 1,024-value-node / 64 KiB string threshold; other
  object/array literals use a higher 24,576-node / 1 MiB key/string threshold.
  Either size limit is sufficient, independent of function instruction budgets.
- Keep mid-size records on ordinary lowering so hot property reads retain their
  static layouts. The higher threshold bounds the LLVM cost of huge record
  literals while still handling the 4.6 MB OpenCode models.dev define.
- Keep parsing at the original evaluation site, preserving fresh values on
  repeated reads and avoiding evaluation in untaken branches. Existing
  `typeof` folding and define inputs to both cache keys are unchanged.
- Preserve property order, string escaping and negative zero; fall back for
  JavaScript-specific constructs such as prototype setters, holes, spreads,
  getters and non-finite numbers.
- Add unit coverage for both threshold tiers and direct lowering of 400 typed
  records, a reproducible record-array/codegen benchmark, and timed regressions
  for a >1 MiB define, cache invalidation, small direct literals, fresh nested objects,
  shadowed `JSON` bindings and array behavior.

Fixes #10151.

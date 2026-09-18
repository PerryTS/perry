### Fixed

- **`x instanceof F` no longer folds to `false` for an imported non-class constructor.** Lowering
  (`crates/perry-hir/src/lower/lower_expr/arm_bin.rs`) only attached a runtime value to an identifier
  `instanceof` RHS for a local, a module function, or a native module — an imported binding was never
  consulted, so codegen resolved the bare name to no class id and folded the check to
  `js_instanceof(v, 0)` (always false). Affected every import form (named, default, CJS
  `module.exports`/`exports.F`) for a plain ES5-style or factory-built constructor; `ns.F`, a local
  alias, and the check written inside the defining module all worked already. Codegen
  (`crates/perry-codegen/src/expr/instance_misc1.rs`) keeps the static class-id fast path for
  imported classes and every non-compiled-source import, so those emit unchanged LLVM IR.

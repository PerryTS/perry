### Fixed

- **`function F(){}; export default F;` exports `F` itself (#10434).** Importers
  used to get a second function object for the default export: prototype
  methods and statics assigned on `F` were missing (`new F().m` undefined),
  `F === imported` was false, and calling the import through a value
  (`const g = f; g()`, `.call`, `.apply`, `new`) passed garbage for missing
  arguments and skipped default/rest parameter handling. It blocked axios 1.19.0
  (`AxiosURLSearchParams.prototype.append`), uuid 14 (`v5.DNS`), lodash-es
  (`MapCache.prototype.clear`) and long 5.3 (`Long.fromInt`).

  Root cause: the `ExportDefaultExpr` arm in `perry-hir`'s `module_decl.rs`
  recorded a `FuncRef` default export as `Export::Named { local: "default" }`.
  The CLI driver only maps a renamed declared-function export back to its local
  name when the row names that local, so importers resolved the `default`
  closure wrapper instead of `F`'s. That wrapper forwards calls to `F`'s body
  but is a separate closure with no expandos and no registered arity.
  `export { F as default }` written after the declaration already wrote
  `{ local: "F" }` and worked.

  Fix: when the exported expression is an identifier naming the function
  (through parentheses and erased TypeScript wrappers), the row is
  `{ local: "F", exported: "default" }`, the same as the alias form.

  A second, hoisting-order bug had the same symptoms. The export arms mark a
  function `is_exported` only if its body is already lowered, so an export
  clause that comes before its hoisted declaration (`export default F;
  function F(){}` or `export { F as default }; function F(){}`) left the flag
  unset, and the driver skipped the origin-name mapping. That made even the
  alias form lose identity, but only in that ordering. After the whole module
  is lowered, a function is now marked exported when an export row names it as
  its local binding and `exported_functions` lists its id. A value alias
  (`export const g = F`) names `g`, so `F` is left as it was.

  Validation: new `test_gap_10434_export_default_fn_identity` (identity across
  two importers, a barrel, a namespace import and a dynamic import; prototype
  methods; statics; `instanceof`; argument padding through a value; default and
  rest parameters; export ahead of a hoisted declaration; cyclic imports; the
  alias, `export default function`, class, arrow and function-expression forms
  as controls) fails on the baseline and matches Node on the fix, plus four
  `perry-hir` unit tests. Package probes that failed on the baseline now match
  Node: axios 1.19.0's default ESM entry (a GET with params and a JSON POST
  against a local server), uuid 14.0.1 (`v5.DNS`, `v3.URL`, `v1`/`v4`/`v7`
  called through values) and lodash-es 4.18.1 (`get`, `memoize`, `set`).

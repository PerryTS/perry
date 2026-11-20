The CommonJS wrapper preamble is emitted into every wrapped module, so its
fixed cost is paid once per module in the dependency graph. Four changes cut it
by a third on a 400-module fixture (598,533 to 397,896 instructions per
module):

* One `createRequire` instance per program instead of one per module. The call
  costs ~117,000 instructions and was made once per module, plus again inside
  `require` for every builtin specifier. It is used only for `.cache`,
  `.extensions` and loading builtins, all of which are process-global in Node.
* `require.cache = {}` and `require.extensions = { … }` were dead stores,
  overwritten on the following line — an object and three closures allocated
  and dropped per module.
* The builtin-specifier test uses `isBuiltin` from `node:module` instead of a
  switch over all 58 builtin names emitted into every module, which interned
  ~120 string constants per module. **This also fixes a divergence:** the
  switch accepted the bare spellings of `sea`, `sqlite`, `test` and
  `test/reporters`, which are builtins only in their `node:` form, so
  `require("test")` could resolve to the builtin instead of a local module.
* The module record is built as one object literal rather than eleven
  sequential assignments, so it is allocated with its final shape instead of
  walking eleven shape transitions. `cjs_scaffolding`'s recogniser is widened
  to match the folded template; that rule carries no soundness weight and the
  allocation half of the collector is report-only.

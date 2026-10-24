fix(wpfacts): per-unit facts model a class that reaches other units through a binding (class expressions, alias bindings)

Per-unit facts claimed things whole-program facts do not for a class bound by
`var _V = class _V {..}` and handed on as `var V = _V; export { V }`
(typescript.js `Version`, esbuild/rolldown output, CommonJS class
declarations): writers 0/0, readonly, fixed layout, a kind of "none", the
methods' reads of other instances dead. Codegen consumes those template facts
behind a class-id guard. Three paths lost the class between the declaration
units of one module:

- A class expression's evaluation (`ClassExprFresh`, every class in a run-once
  body such as typescript.js's or a CommonJS wrap) named no class: the binding's
  unit read the class as ⊤ and the class unit was never told it is constructed.
  The evaluation is now the class's definition, one unit with it (its statics
  are the class's state, as `StaticFieldSet` is).
- The class `_V` and the binding `_V` shared one owner-by-name map: the binding
  took the name, so the binding's unit read the class as its own binding and
  the class unit imported its own name back. Name references (`ClassRef`,
  `new C`, `extends C`) now resolve in the reference namespace (functions,
  classes, globals); an export's local resolves to the binding. A unit naming
  both imports each under its own local.
- A name a unit exports to its module's other units only was also published
  under the module's path, where it stood for the module's export of that name
  (the class `_V` for the binding `_V`).

An aborted unit is unknown code holding everything its code names: imports its
walk did not reach before a node bound are made and reported too.

Per-unit template facts on awfy_Richards no longer claim more than
whole-program facts (10 claims before, cd.js `Aircraft`/`CallSign`; 0 after).

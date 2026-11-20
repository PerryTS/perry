Fixed the builder fold (#6812) turning a successful read into a TDZ
`ReferenceError` when a folded value runs an implicit conversion (#10357).
Folding `const o = {}; o.a = v;` into `const o = { a: v }` evaluates `v` before
`o` is initialized, which is unobservable only if `v` runs no user code that
can read `o`. `value_is_fold_safe` claimed exactly that, yet admitted every
converting operator: `"" + w`, `-w`, `w < 1`, `w == 1` and `` `${w}` `` all call
`w`'s `valueOf`/`toString`/`Symbol.toPrimitive`. With
`w = { valueOf() { return o; } }` node builds `{ a: "[object Object]" }` and
perry threw.

Every such conversion could have been dropped from the predicate, but that
would have stopped the fold's own motivating example (`o.b = r + i`) from
folding. Instead the fold now asks whether anything *can* read the binding
early. A binding is only readable by code that names it, so user code reached
through a conversion must be a function-like nested in the builder's scope
that mentions the name. `FoldScope` scans each function body (or the module)
for such observers, and keeps that scan precise because every false positive
costs a fold:

- a function-like created after a `let`/`const` builder cannot run before it
  (the binding is fresh per loop pass and execution within a pass only moves
  forward); a hoisted function declaration always counts, and so does any
  observer of a `var`, whose binding every loop pass shares;
- a nested function-like that re-binds the name as a parameter or top-level
  body declaration is not an observer (body declarations do not shadow
  parameter defaults);
- `eval` (Perry compiles a literal `eval("o")` into a closure that reads
  `o`), `with`, an exported name and a module-level `var` make a builder
  observable outright.

The same hazard hides in a bare identifier read: a name that resolves to no
binding reads the global object, and `Object.defineProperty(globalThis, "g",
{ get() { return o; } })` makes that read user code. `o.a = g` threw exactly
like the conversion. An unobservable builder folds exactly as before. On an
observable builder, a conversion needs operands that are primitive by
construction, and a read needs a proven declarative binding. `Visible`
proves that from a chain built only of declarations that cover the whole
region: a statement list's own declarations, parameters and
function-scoped `var`s, loop heads, catch parameters, and module imports and
declarations. A sibling block's declaration, an ambient `declare` (see
#10363), or any `with` in the module does not count. #10355's gap statements
share the same answers. The scan covers the enclosing function rather than
the folded statement list, because a `var` is function-scoped and a `let` in
one `case` is visible to every other case of its `switch`.

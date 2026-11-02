A TypeScript import-equals with an entity name (`import G = Geo.Inner`,
`export import E = Geo.Inner`) fell into the catch-all module-item arm and
was dropped, so every use of the alias threw "ReferenceError: G is not
defined". Since #12775 a namespace is an ordinary object, so the alias is
the declaration tsc emits for it: `var G = Geo.Inner` (`export var` for the
exported form). The module is rewritten before any lowering pass, so the
hoisting pre-scan sees the `var` too: a function declared above the alias
reads it. Modules without such an alias lower with no clone. The `require`
form and type-only aliases are unchanged.

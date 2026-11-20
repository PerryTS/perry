Compile constant-source `Function`/`eval` through ordinary parsing and lowering,
including private source-building helpers whose calls pass a finite set of
literals (get-intrinsic's `getEvalledConstructor`), so no dynamic code remains
for them. Their builtin writes and `Function`-value uses count as program code.

Non-constant source sites carry a precise `dynamic_code_reach`: `new Function`,
indirect eval, `vm` and data-URL imports name only globals (builtin namespace
and prototype members become replaceable, and the whole-program field facts
yield to possible Object.prototype accessors); a direct eval also names its
lexical scope, so classes in that scope keep prototype lookups. A `Function`
value held as data links the interpreter but is not a site. Each site is
named in a compile-time diagnostic.

A sloppy function with a direct eval of runtime source has a dynamic scope:
the eval's `var`s and functions land in a per-call scope object, and the
function's free identifiers read and write through it, so native code after
the eval sees them. Direct eval uses the linked interpreter with native
accessor closures for live lexical bindings. Eval throw-on-reach values are
gone, eval spread arguments are honored, and legacy strict-eval settings no
longer refuse executable source.

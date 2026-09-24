Classes nested in a function no longer store their captured outer variables
on every instance. A class whose definition runs once, or a class expression
evaluated to a fresh class object (every class in a CommonJS module body),
keeps its captures in a per-class environment read with one compare and one
load; a second evaluation (a re-run module body) is resolved per receiver,
so each instance still sees its own evaluation's values. TypeScript's AST
nodes lose their 3-10 hidden `__perry_cap_*` keys (25% fewer bytes per node),
`pos`/`end`/`kind` no longer shift with a class's capture count, and
`ts.transpileModule` runs about 11% fewer instructions.

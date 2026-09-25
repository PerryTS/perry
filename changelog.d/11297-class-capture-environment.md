Classes nested in a function no longer store their captured outer variables
on every instance. A class whose definition runs once, or a class expression
evaluated to a fresh class object (every class in a CommonJS module body),
keeps its captures in a per-class environment read with one compare and one
load; a second evaluation (a re-run module body) is resolved per receiver,
so each instance still sees its own evaluation's values. TypeScript's AST
nodes lose their 3-10 hidden `__perry_cap_*` keys (25% fewer bytes per node),
`pos`/`end`/`kind` no longer shift with a class's capture count, and
`ts.transpileModule` runs about 11% fewer instructions.

A class expression in env mode that closes over a `for (let …)` head binding
keeps #11250's expired-head rewrite: the refresh re-reads that evaluation's
own capture array, which the runtime republishes into the environment slots
only for the owning evaluation (`test_gap_11297_env_class_for_let_capture`).

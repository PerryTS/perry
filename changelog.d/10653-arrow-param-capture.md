Fixed: a closure created inside a local arrow function that captured the
arrow's OWN parameter kept seeing the FIRST call's argument on every later
call to the same arrow — a per-call closure that should differ between
`iter(fields, false)` and `iter(optFields, true)` silently ran the first
call's body for both. This blocked `@noble/curves` 2.2.0's
`validateObject` helper, among any code shaped like a local arrow that
constructs a callback for `forEach`/`map`/etc. and hands it a value derived
from the arrow's own parameter.

Root cause: `closure_local_inline` (`crates/perry-transform/src/closure_local_inline.rs`)
beta-reduces a local `let f = (a, b) => <expr>` closure that is only ever
called, cloning the return expression fresh per call site and substituting
each parameter via `substitute_locals`. For a parameter read inside a
NESTED closure, `substitute_locals` bakes a non-`LocalGet` argument straight
into that nested closure's body and drops it from the closure's `captures`
list — correct for one clone in isolation — but never mints a fresh
`func_id`, and codegen compiles exactly one body per `func_id` (whichever
`Expr::Closure` occurrence it encounters first). With more than one call
site, every clone of the nested closure shared the same `func_id`, so only
the first-seen clone's baked-in argument was ever compiled.

Fix: `arrow_candidate` now bails out of the beta-reduction when any
parameter is captured by a closure nested in the arrow's body (reusing the
`collect_closure_captured_local_ids` helper the #858 fix already
established for the sibling FuncRef-keyed inliner), leaving such an arrow
as a real, per-call closure.

Validation: new gap test
(`test_gap_10567_arrow_param_closure_capture.ts`) covering the issue repro
plus several-params, nested-arrows, arrow-in-a-method, and by-write-capture
variants — proven to fail on the pre-fix tree (e.g. `A:1,B:1` → wrongly
prints the first call's value on later calls) and pass on this one,
byte-identical to Node 26.5.1. `cargo test --release -p perry-transform
--tests`: 152 passed. Instructions regress ~7.3% for the exact bug shape
(the correctness cost of no longer sharing one wrongly-baked closure body
across call sites with different arguments); the safe single-call-site
shape is unaffected (within noise).

### Representation selection: `Ptr<Shape>` return-shape facts now reach inside Perry's CommonJS IIFE (#7170 R1)

`compile/cjs_wrap/wrap.rs` emits every CommonJS module body inside
`const _cjs = (function () { … })();`. Inside that wrapper a module-level
`function` declaration never reaches `hir.functions` — it lowers to
`Stmt::Let { init: Expr::Closure }`, and a call to it to
`Call { callee: LocalGet(id) }`. #7107's return-shape mechanism walked
`hir.functions` for producers and accepted only a bare `Expr::FuncRef` callee
for consumers, so it was **structurally unreachable across the whole CommonJS
ecosystem**: 91.6 % of dependency-JS `Ptr<Shape>` allocation sites sit in
`closure` regions (#7170 §2/§6). This is the third time Perry's own CJS
scaffolding turned out to be the wall, after #7139 (the wrap preamble arming
the rule-5 barrier) and #7152/#7171 (`__cjs_module`).

Both halves are extended, because both missed it:

* **Producer** (`collectors/ptr_shape_returns.rs`): every `Expr::Closure` in
  the module is now a candidate body, keyed by the `FuncId` the closure already
  carries — the same module-wide `fresh_func` counter as `hir.functions`, so no
  key can mean two things. A `Function` and a closure are the same thing to
  this proof but carry differently-spelled context flags
  (`Function::was_plain_async` versus `Module::async_step_closures`), so both
  are projected onto one `ProducerBody` view and the closure arm cannot prove
  something weaker than the function arm.
* **Consumer**: an `Expr::LocalGet` callee resolves through a new module-wide
  binding proof, `collectors/spec_abi_sites.rs::single_binding_closure_locals`
  — exactly one `Stmt::Let` with a closure init, never reassigned at any depth
  in any body, never also a parameter or a `catch` binding. That is the same
  statement `Expr::FuncRef` makes directly, and it is the only property the
  seed needs of a callee: *which body runs*. Deliberately **not**
  `FnCtx::local_closure_func_ids`, which `lower_call` pairs with a runtime
  `js_typed_feedback_closure_direct_call_guard` because it is populated in
  statement order.

Box-backed bindings are admitted on purpose: a hoisted inner `function`
referenced from a sibling closure is `PreallocateBoxes`-boxed by construction
(`lower_decl/block.rs`), and that is the entire dependency-JS population.
Freshness is unchanged — the full Phase 3b proof still re-runs over the
producer's body.

**Measured, on a real transpiled CommonJS module**, both arms pinned at one
SHA and compared on emitted IR with call sites checked: `Ptr<Shape>` goes from
`selected 0 / consumed 0` to `selected 1 / consumed 2`, and the emitted IR
loses **25 opaque `js_*` call sites** — `js_object_get_field_by_name_f64`
19 → 13, and three whole typed-feedback guard diamonds
(`js_typed_feedback_object_get_field_by_name_f64` /
`observe_property_get` / `record_guard_pass` / `record_guard_fail` /
`record_fallback_call`, each 10 → 7). Three call sites are *added* and are
reported as the promotion's own cost: the guard-free store emits a direct
`js_write_barrier_slot` where `js_put_value_set_dyn_ic` did the barrier
internally, plus one slot-layout note and one string addref.

**On the 197-module dependency corpus the mechanism fires 10 more times and
promotes nothing more**, and that is reported rather than smoothed: the
`return` allocation bucket moves 231 → 221 unserved and 4 → 14 served, while
corpus `selected`/`consumed` stay at 3/11. The producers R1 reaches are
*exported* helpers with no same-module `const x = f(…)` call site — #7170 R2's
cross-module half, which is structurally blocked because
`Expr::ExternFuncRef` carries no `FuncId`. A throwaway instrumented compiler
put a number on the residual wall: over the same corpus the first refusing
conjunct is the **return form** in 1397 of 1971 refusals, while
"can fall off the end" refuses exactly **one** body — so widening the producer
to conditional returns whose arms agree (R0 §3b measured 88 such sites) is the
next increment, not more consumer reach.

Also fixes a latent hole this proof would otherwise have inherited:
`Expr::WithSet` carries its fallback `LocalId` in `WithSetFallback` rather than
in a child expression, so `spec_abi_sites::record_expr_use` — whose every other
arm delegates to the exhaustive `walk_expr_children` — had never recorded
`with (o) { x = v }` as a reassignment. `reassigned_locals` had been wrong
about that since it was written; the fix can only make it more conservative,
and only in a module containing `with`.

New gates: census liveness fixture
`benchmarks/repsel_census/fixtures/fixture_ptr_shape_cjs_iife.ts` (floors held
in code) and behavioural gap test
`test-files/test_gap_repsel_cjs_iife_return_shape.ts`, registered in
`test-parity/gc_repsel_corpus.txt`. The fixture deliberately lands **two**
allocation buckets from inside one IIFE — a served return and an unserved one —
because the served flag for a closure region is set in `codegen/closure.rs`,
which no compiler unit test can reach: hard-coding it `false` or `true` was a
green hole across all 526 of them and is red only in the census.

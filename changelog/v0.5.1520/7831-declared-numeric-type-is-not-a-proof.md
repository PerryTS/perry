### Fixed

- **A declared numeric type is no longer treated as proof that the value is a
  number** (#7773, #7776). Perry does not enforce annotations at runtime, but
  codegen answered `is_numeric_expr` = `true` on the strength of one and then
  emitted bare f64 arithmetic on whatever the slot actually held.

  That is worse than producing a `NaN`, because arithmetic on a NaN-BOXED value
  is not a no-op: `fadd`/`fmul` propagate the input NaN's payload, so a
  NaN-boxed string comes back out of the instruction still tagged as that
  string and flows on as if nothing happened — `typeof (v * 2)` answered
  `"string"`. Four divergences from Node, all silent: `o.x + 1` gave `NaN`
  where Node concatenates; `const v = o.x; v + 1` looked as though the `+ 1`
  had evaporated; `v * 2` returned the string; and summing a `P[]` with one
  `as any`-stored `Q` element gave `NaN`.

  A new `numeric_proof_is_declared_only` separates "an annotation said so" from
  a real proof. It is deliberately narrower than
  `expr_may_return_boxed_value_from_raw_f64_fallback` (which answers "is there
  a raw-f64 tier worth trying" and stays true for reads with no boxed fallback
  at all): element-shape and class-field loop facts, `Ptr<Shape>` numeric
  fields, scalar replacement, POD records and typed arrays all answer `false`
  and keep their bare loads. `+` then lowers through an inline NaN-box tag test
  — `fadd` on the fast arm, `js_dynamic_string_or_number_add` on the cold one —
  because the spec's `+` dispatches on the runtime value; every other
  arithmetic operator is a plain `ToNumber` and only needed the existing
  residual-coerce rule taught to see a refined LOCAL.

  `expr/mod.rs::lower_numeric_binary_value` turned out to be a second
  arithmetic tier that bypasses `binary::lower` entirely and emits bare
  `fadd`/`fmul` with no residual coerce at all; it was the path both
  refined-local shapes took, and it now hands declared-only operands down the
  same way its two existing `Mod` cases do.

  Two details are load-bearing and are pinned by the test. The diamond covers
  the whole `+` **tree**, not one node each: per-node diamonds make the outer
  add of `s += o.x + 1` consume a phi that LLVM cannot prove is a canonical
  double, which killed the `fadd` in the loop (+38% before fusing, +8.6%
  after). And every leaf is tested except those `expr_produces_canonical_raw_f64`
  vouches for — testing only the declared-only leaves skips the ACCUMULATOR,
  which holds a string the moment this lowering's own cold arm concatenates,
  and summed `16zw1113151719` down to `16zw`.

  Measured on the quiet M1 mini, same runtime in both arms: element-shape clone
  218 → 217 ms (−0.5%, untouched), `this.v + 1` in a method 70 → 76 ms (+8.6%),
  `s += p.x + p.y` with an escaped receiver 196 → 263 ms (+34.2%). The cost
  falls only on reads nothing could prove, which already pay an inline header
  precheck or a `js_typed_feedback_class_field_get_guard` call.

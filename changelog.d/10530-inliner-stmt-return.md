### Fixed

- A small helper called as a statement (`f(x);`, result discarded) whose
  inlined body ended in an `if` containing `return` no longer returns out of
  the caller (#10416). The HIR inliner spliced that `return` into the calling
  function, so the caller returned the helper's value: decimal.js `intPow`
  calls `truncate(x.d, k);` inside `for (;;)`, and `new Decimal(2).pow(100)`
  was `true`. At module top level the stray `ret double` did not match
  `main`'s `i32` result, and the module failed to compile.

  `inline_calls_in_stmts`'s statement arm checked for nested returns only in
  `take(len - 1)` and rewrote the last statement only when it was a bare
  `Return`, so a trailing `if` passed both checks unchanged. The arm now runs
  the inlined body through `discard_inlined_returns`
  (`crates/perry-transform/src/inline/discarded_result.rs`), which removes
  every return structurally. `return e` becomes `e;`, and the statements after
  an `if` that returns move into the branch that falls through to them. It
  adds no `do { } while (false)` wrapper, so the caller's hot loop gains no
  nested loop, GC back-edge poll or cleared receiver facts. It declines, which
  keeps the call, rather than duplicate or drop statements, and whenever a
  return sits under a loop, `switch`, `try` or label.

  Two siblings of the same blind spot are fixed too. When the arm declined to
  inline, its fallback replaced the call statement with the setup hoisted out
  of the call's arguments: `f(g(i));` lost the call to `f`. Separately, the
  void-method expression inliner kept going past `return;`, so
  `m() { return; this.x = 1; }` used as a value ran `this.x = 1`.

  Helpers with an early return before more statements
  (`if (v < 0) return; acc[0] += v;`) were never inlined at statement call
  sites before this change. They are now, as they already were for
  `let r = f(x)`. In a 50M-iteration hot loop that is 40.5e9 → 10.6e9
  instructions. The #10416 shape itself measures the same as before when its
  `if` never fires (6.31e9 on both), and `09_method_calls` and a decimal.js
  arithmetic loop are unchanged within noise.

  Covered by `test_gap_10416_inliner_stmt_return` (statement calls in
  `for`/`while`/`do-while`/`for (;;)`, a function with more than 10
  statements, class methods, arrows, module top level, generator and async
  callers, with value-use controls) and by rewrite and inliner unit tests in
  `discarded_result.rs`, of which the inliner-level ones fail on the old arm.

// Liveness fixture for the `Ptr<Shape>` census key **inside an IIFE** (#7170 R1).
//
// `fixture_ptr_shape.ts` is the same proof at module scope, where `mk` and
// `run` are `hir.functions` entries. This file wraps identical code in
// `(function () { … })()` and nothing else. That one difference is what Perry's
// own `cjs_wrap` does to *every* CommonJS module (`compile/cjs_wrap/wrap.rs`,
// `const _cjs = (function() { … })();`), and #7170 §6 measured what it costs:
//
//   probe        wrapper                result for `const p = mk(i)`
//   p7_esm.ts    none                   selected, and consumed
//   p8_iife.ts   (function(){ … })()    NOT EVEN A CANDIDATE
//
// Inside the IIFE `mk` is not a function declaration the compiler can see —
// it lowers to `Stmt::Let { init: Expr::Closure }`, and `mk(i)` to
// `Call { callee: LocalGet(id) }`. #7107's producer walked `hir.functions`
// (empty here) and its caller-side seed accepted only `Expr::FuncRef`, so the
// whole return-shape mechanism was structurally unreachable across CommonJS —
// 91.6% of dependency-JS allocation sites (#7170 §2).
//
// This fixture is what makes that reachability falsifiable. Reverting either
// half of R1 — the closure arm of `collect_return_shape_functions`, or
// `callee_names_one_function`'s `LocalGet` arm — takes its `ptr-shape` count
// to zero while `fixture_ptr_shape.ts` stays green, because that one is at
// module scope and never needed either.
//
// Do not "tidy" this file:
//
//   * Removing the IIFE turns it back into `fixture_ptr_shape.ts` and it stops
//     testing anything R1 added.
//   * Removing `p.x = p.x + 1` makes the object non-escaping, `escape_news.rs`
//     deletes it outright, and the promotion becomes `unconsumed —
//     scalar_replaced` (#7170 §6.1). The `ptr-shape-consumed` floor is what
//     catches that, and the store is what satisfies it.
//   * Reassigning `mk`, or declaring it twice, disqualifies the callee binding
//     (`single_binding_closure_locals`) and takes the count to zero.
//   * Deleting `maybe` removes the fixture's only UNSERVED return-position
//     allocation, and with it the census's ability to catch
//     `codegen/closure.rs` reporting every closure as served. See
//     ALLOC_BUCKET_FLOORS in `scripts/compiler_output_harness/repsel_census.py`
//     — this file is the only workload that lands both bucket rows in a
//     `closure` region, and no compiler unit test can reach that wiring
//     (they all set the report scope by hand).

const _cjs = (function () {
  function mk(i: number) {
    return { x: i, y: i + 1 };
  }
  // Deliberately NOT a return-shape producer: the second return is not a fresh
  // allocation, so the returns disagree and `producer_return_class` refuses.
  // Its `{ tag: n }` is therefore an unserved return-position allocation in a
  // closure region — the anti-vacuity half of the served classification.
  function maybe(n: number) {
    if (n > 2) {
      return { tag: n };
    }
    return null;
  }
  function run(n: number): number {
    let total = 0;
    for (let i = 0; i < n; i++) {
      const p = mk(i);
      p.x = p.x + 1;
      total = total + p.x + p.y;
    }
    if (maybe(n) !== null) {
      total = total + 1;
    }
    return total;
  }
  return run(4);
})();

console.log("ptr_shape_cjs_iife:" + _cjs);

// Issue #318: closures hidden inside HIR variants the codegen
// closure-collector did not descend into were silently dropped at
// collect time but still referenced at codegen time, producing
// "use of undefined value @perry_closure_*" link errors.
//
// The fix delegates `collect_closures_in_expr`'s descent to the
// centralised `perry_hir::walker::walk_expr_children` helper, which
// is exhaustive (compile error on any new HIR variant) — same pattern
// the v0.5.329 Tier 1.1 fix used for the four other walker consumers.
//
// This regression covers a representative shape: a closure nested
// inside the operand of an `Expr::MathSin` (and friends) — a HIR
// variant that the old ad-hoc match did not enumerate, so the inner
// IIFE-built closures were unreachable from the collector and codegen
// emitted dangling `@perry_closure_*` references. Math.floor / sqrt
// / abs were in the old arm list, but Math.sin / cos / log2 / exp
// were among the >80 HIR variants the old walker silently skipped.

const angle = ((mk: () => number) => mk())(() => 1.5);
const s = Math.sin(((mk: () => number) => mk())(() => angle));
const c = Math.cos(((mk: () => number) => mk())(() => angle));
const l = Math.log2(((mk: () => number) => mk())(() => 8));

console.log(s.toFixed(4));
console.log(c.toFixed(4));
console.log(l.toFixed(4));

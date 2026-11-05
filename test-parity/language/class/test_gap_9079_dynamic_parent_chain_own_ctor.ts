// Issue #9079: two levels of dynamic parent — a mixin applied to a mixin —
// SIGSEGVed when the leaf class had its own constructor.
//
// `const Mixed2 = mixin(Mixed)` extends a lexical VALUE binding, so the class
// the HIR mixin fast path synthesizes carries a DYNAMIC parent (extends_expr).
// That path bound the class without emitting the declaration-time
// `RegisterClassParentDynamic` its sibling `const X = class …` path emits, so
// the synthesized class had no registered parent: `js_fetch_or_value_super`
// fell back to the most-derived receiver, re-selected the same class, and
// recursed until the stack overflowed. One level was already correct (#9073),
// which is why the failure looked arbitrary.

// --- the exact reproducer from the issue ------------------------------------
class Root { r = 1; }
function mixin(Base: any) { return class extends Base { m() { return 1; } }; }
const Mixed = mixin(Root);
const Mixed2 = mixin(Mixed);
class Deep extends Mixed2 { d = 4; constructor() { super(); } }
console.log(new Deep().d);

// --- the chain is WALKED, not merely survived -------------------------------
// Root's field initializer must have run, and the mixin method must be
// reachable through both synthesized levels.
const deep: any = new Deep();
console.log("state:", deep.r, deep.d, "method:", deep.m());
console.log(
  "instanceof:",
  deep instanceof Deep,
  deep instanceof Mixed2,
  deep instanceof Mixed,
  deep instanceof Root,
);

// The issue left open whether the leaf's OWN constructor was required. It is
// not the only shape that must work: an implicit constructor over the same
// two-level chain has to reach Root too.
class DeepImplicit extends Mixed2 { d2 = 5; }
const implicit: any = new DeepImplicit();
console.log("implicit:", implicit.r, implicit.d2, implicit.m());

// One level still works — it did before this fix; keep it pinned so the new
// registration cannot regress the shape #9073 fixed.
class One extends Mixed { x = 2; constructor() { super(); } }
const one: any = new One();
console.log("one-level:", one.r, one.x, one.m(), one instanceof Mixed, one instanceof Root);

// --- every level of a longer chain contributes ------------------------------
// Distinct mixins so a missing level is visible as a missing METHOD rather
// than being masked by an identical body at each level.
function withA(Base: any) { return class extends Base { a() { return "a"; } }; }
function withB(Base: any) { return class extends Base { b() { return "b"; } }; }
function withC(Base: any) { return class extends Base { c() { return "c"; } }; }
const A = withA(Root);
const B = withB(A);
const C = withC(B);
class Leaf extends C { n = 9; constructor() { super(); } }
const leaf: any = new Leaf();
console.log("three-level:", leaf.r, leaf.n, leaf.a(), leaf.b(), leaf.c());
console.log(
  "three-level instanceof:",
  leaf instanceof C,
  leaf instanceof B,
  leaf instanceof A,
  leaf instanceof Root,
);

// #7154: the CALLEE of a dynamic value-call must be rooted across the
// evaluation of the call's arguments.
//
// `f(g())` evaluates the callee first and the arguments second — spec order,
// and codegen follows it — which left the callee in a bare SSA register while
// `g()` was lowered. `g()` allocates, and under `PERRY_GC_MOVING_LOOP_POLLS=1`
// a loop back-edge poll inside it runs an evacuating minor. The callee
// SURVIVES that minor (the closure capture cell holding it is a root), which
// means it MOVES: the collector rewrites the capture cell but not the caller's
// register. `js_closure_unbox_callee_checked` then masks a from-space address
// and `js_closure_callN` reads its header out of abandoned memory —
// `TypeError: value is not a function`.
//
// Same invariant as #7206, #7192, #7184 and #7114, one operand over: a GC
// value's root must dominate every subsequent collection point, and a
// rewritten location is worthless unless the code below the collection point
// READS that location again.
//
// LIVE BY CONSTRUCTION. `fn` is an `any`-typed closure read out of a capture
// cell, so the call takes the generic `js_closure_callN` fallthrough rather
// than a static direct call, and the argument allocates hard enough to reach
// the collector. A non-moving collection cannot expose this, so the evacuating
// arms are the ones that bite.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 600;
}

function make(t: number): (p: number) => number {
  const fn: any = (v: number): number => t + v;
  return (p: number) => fn(churn(p));
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 400; r++) {
    const f = make(r);
    const got = f(1);
    if (got !== r + 1) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());

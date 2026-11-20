// #7154: the `this` RECEIVER of a dynamic value-call must be rooted across the
// read of the callee off it and across the call's argument list.
//
// `recv.m(g())` where `recv.m` is a closure-VALUED property (not a native
// method) lowers through the generic `js_closure_callN` fallthrough. That path
// evaluates the receiver, reads the callee off it, lowers the arguments, and
// only then binds the receiver as the implicit `this` and hands it to
// `js_closure_unbox_callee_checked_rebind`. The receiver sat in a bare SSA
// register for that whole span: an evacuating minor inside `g()` rewrites the
// capture cell it was read from and leaves the register naming from-space, so
// the rebind clones captures out of abandoned memory and the body's `this.tag`
// reads garbage.
//
// #7206 fixed this same operand on the sibling `js_native_call_method_by_id`
// dispatch. This is the `js_closure_callN` one — the dispatch a closure-valued
// property takes (hono's `RegExpRouter.match = match`, the #519 shape).
//
// LIVE BY CONSTRUCTION. `m` is a non-arrow function declaration assigned onto
// an object literal, so it reads `this` through the implicit-this binding and
// the call takes the fallthrough rather than the by-name method dispatch. The
// receiver, the callee and the argument list are all in flight at once here —
// this lowering held all three in registers, and the three tests in this group
// name them one at a time.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 600;
}

function meth(this: any, v: number): number {
  return this.tag + v;
}

function make(t: number): (p: number) => number {
  const inst: any = { tag: t, m: meth };
  return (p: number) => inst.m(churn(p));
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

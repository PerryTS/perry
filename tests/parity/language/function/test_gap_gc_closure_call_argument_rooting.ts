// #7154: an already-lowered ARGUMENT of a dynamic value-call must be rooted
// across the evaluation of the arguments that follow it.
//
// `f(a, g())` lowers `a` into a bare SSA register and then lowers `g()`. `g()`
// allocates, and under `PERRY_GC_MOVING_LOOP_POLLS=1` a loop back-edge poll
// inside it runs an evacuating minor: `a` survives (the capture cell holding
// it is a root) and therefore MOVES, the collector rewrites the cell, and the
// register keeps naming from-space. `js_closure_callN` then passes the
// pre-move address as argument 0 and the callee reads its fields out of
// abandoned memory.
//
// This is the third register the generic dynamic-call lowering held across its
// own argument list; #7206 named all three and fixed none of them.
//
// ISOLATED ON PURPOSE. The callee is a module-level binding assigned a plain
// function declaration, so its function object is created once at module init
// and is long tenured by the time the loop runs — it is not what moves. `inst`
// is allocated fresh per iteration and is squarely in the nursery, so argument
// 0 is the operand under test.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 600;
}

function add(o: any, v: number): number {
  return o.tag + v;
}

const fn: any = add;

function make(t: number): (p: number) => number {
  const inst: any = { tag: t };
  return (p: number) => fn(inst, churn(p));
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

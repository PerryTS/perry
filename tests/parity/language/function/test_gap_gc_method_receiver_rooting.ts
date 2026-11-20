// #7154: the RECEIVER of a dynamic method call must be rooted across the
// evaluation of the call's arguments.
//
// `recv.m(f())` evaluates the receiver first and the arguments second — spec
// order, and codegen follows it — which left the receiver in a bare SSA
// register while `f()` was lowered. `f()` allocates, and under
// `PERRY_GC_MOVING_LOOP_POLLS=1` a loop back-edge poll inside it runs an
// evacuating minor. The receiver SURVIVES that minor (the closure capture cell
// holding it is a root), which means it MOVES: the collector rewrites the
// capture cell but not the caller's register. The dispatch then resolves `m`
// against abandoned from-space memory and throws
// `TypeError: value is not a function`.
//
// This is the site that kept `sfw-registry --help` red under
// `PERRY_GC_MOVING_LOOP_POLLS=1` after #7192 fixed the four alloc-anchored
// sites. In the registry it is zod's `classic/schemas.ts:301`,
// `inst.regex = (...args) => inst.check(checks.regex(...args))` — `inst` read
// from the arrow's capture cell, held across a real user call, then used as the
// receiver of `.check`.
//
// Same invariant as #7192, #7184 and #7114: a GC value's root must dominate
// every subsequent collection point, and a rewritten location is worthless
// unless the code below the collection point READS that location again.
//
// LIVE BY CONSTRUCTION. `inst` is a plain object literal read out of a closure
// capture, so the call takes the dynamic by-name dispatch rather than a static
// class-method call, and the argument allocates hard enough to reach the
// collector. A non-moving collection cannot expose this, so the evacuating arms
// are the ones that bite.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 600;
}

function make(t: number): (p: number) => number {
  const inst: any = {
    tag: t,
    check(v: number): number {
      return this.tag + v;
    },
  };
  return (p: number) => inst.check(churn(p));
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

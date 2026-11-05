// #7154: the instance of `new C(...)` must be ROOTED across the constructor
// body, not carried in an SSA register.
//
// `new C(n)` lowers to: allocate the instance, call `<C>_constructor(inst, n)`,
// then `js_gc_init_typed_shape_layout(inst, ...)` and
// `js_ctor_return_override(inst, ret, derived)`. The constructor body allocates,
// and under `PERRY_GC_MOVING_LOOP_POLLS=1` a loop back-edge poll inside it runs
// an evacuating minor. The instance SURVIVES that minor — the callee's own
// `this` parameter has a shadow slot — which means it MOVES, and the collector
// rewrites the callee's root but not the caller's register. Every use after the
// call then names from-space memory, and the return-override publishes that
// dead address into the caller's shadow slot: a rooted slot holding a dangling
// pointer, read back later as garbage or "value is not a function".
//
// This is #7184's sibling. There the root store landed OUTSIDE the pushed frame
// and silently no-opped; here it lands AFTER a collection point. Same
// invariant: a value's root store must dominate every site that can collect.
//
// LIVE BY CONSTRUCTION. The constructor allocates hard enough to reach the
// collector, and the caller READS a field of the instance right after the call
// — a non-moving collection cannot expose this, so the evacuating arms are the
// ones that bite.

class Node1 {
  payload: any;
  constructor(n: number) {
    const bits: any[] = [];
    for (let i = 0; i < 600; i++) {
      bits.push({ i: i, s: "x" });
    }
    this.payload = { n: n, len: bits.length };
  }
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 400; r++) {
    const node = new Node1(r);
    const p = node.payload;
    if (p === null || p === undefined) {
      bad++;
    } else if ((p.n as number) !== r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());

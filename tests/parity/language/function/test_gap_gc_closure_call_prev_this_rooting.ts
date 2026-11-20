// #7211: the PREVIOUS implicit `this` that `js_closure_callN` saves across a
// dynamic call must be rooted, because the restore publishes it back into a
// root the collector scans.
//
// `js_implicit_this_set(v)` swaps the `IMPLICIT_THIS` cell and returns what was
// there. That cell is a registered MUTABLE root — `scan_implicit_this_roots_mut`
// (object/this_binding.rs:176) marks it and rewrites it on an evacuating cycle.
// The swap has already overwritten it, so the returned value is now held only
// in a bare SSA register, and it stays there across the allocating rebind unbox
// AND the entire user call. A minor inside the callee moves the object, rewrites
// the enclosing frame's copy, and leaves this register naming from-space. The
// restore then writes that pre-move address BACK INTO the cell — so the damage
// outlives the call and lands on whatever reads `this` next.
//
// #7214 fixed the callee, the receiver and every argument of this same lowering
// and deliberately left this one, measured but unfixed. It was invisible to
// scripts/gc_root_dominance_check.py at both ends: `js_implicit_this_set` was
// NONCOLLECTING but not a root READ, so the register had no recognised
// heap-value source, and the restore is not a fatal sink. Both ends are now
// classified, so this shape stays gated as well as fixed.
//
// LIVE BY CONSTRUCTION. `outer` is a non-arrow function assigned onto an object
// literal, so `obj.outer(1)` takes the generic `js_closure_callN` fallthrough
// and binds `obj` as the implicit `this`. `h(v)` inside it is a RECEIVERLESS
// dynamic call, which per #3576 must reset `this` to undefined and restore it —
// that restore is the subject. `this.tag` is then read AFTER the restore, so a
// corrupted cell is observable rather than merely present.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 600; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 600;
}

function helper(v: number): number {
  return churn(v);
}

function outer(this: any, v: number): number {
  const before = this.tag;
  // Receiverless dynamic call: saves the current implicit `this` (= this
  // object), binds undefined, and restores the saved value afterwards.
  const h: any = helper;
  const got = h(v);
  // Read `this` again BELOW the restore. If the restore republished a
  // pre-move address, this reads out of abandoned memory — a wrong number if
  // the bytes were recycled, a SIGSEGV if the page was retired.
  const after = this.tag;
  return before === after ? after + got - v : -1;
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 400; r++) {
    const obj: any = { tag: r, outer: outer };
    const got = obj.outer(1);
    if (got !== r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());

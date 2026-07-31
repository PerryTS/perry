// Liveness fixture for the `Ptr<NumArray>` census key (#7106).
//
// Written to satisfy `collectors/ptr_numarray.rs` in full so a zero count
// means the counter is dead, not that the corpus is uninteresting. This one
// matters more than most: before #7106 `opt_report` had a `PtrNumArray`
// analysis variant and a `Ptr<NumArray>` target-rep string, but there was no
// `select()` call site for it anywhere in the tree. Its `selected` tally was a
// number that could not be incremented.
//
// Proof obligations satisfied:
//   provenance  — `new Array(<static n>)` (HolesOK) and `[]` (Dense), one Let each
//   containment — element reads, numeric element writes, `.length`, numeric
//                 `push`, and a bare `return` only; never passed, captured,
//                 logged, reassigned or aliased
//   density     — no `pop`/`shift`/`splice`/`unshift`/`copyWithin`
//   in-bounds   — loop indices bounded by a compile-time-constant length
//   module-wide — no shape barrier, no `Array.prototype[i] = …` anywhere
//
// Do not add `console.log(buf)` or pass either array to a helper: a bare
// reference is an escape and disqualifies the local outright.

function preallocated(): number {
  const buf = new Array(64);
  for (let i = 0; i < 64; i++) {
    buf[i] = i * 2;
  }
  let total = 0;
  for (let i = 0; i < 64; i++) {
    total = total + buf[i];
  }
  return total;
}

function grownFromEmpty(n: number): number {
  const acc: number[] = [];
  for (let i = 0; i < n; i++) {
    acc.push(i + 0.5);
  }
  let total = 0;
  for (let i = 0; i < acc.length; i++) {
    total = total + acc[i];
  }
  return total;
}

console.log("ptr_numarray:" + (preallocated() + grownFromEmpty(8)));

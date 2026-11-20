// #7661: deforestation turns `const keep = build(n)` into
// `let keep = []; keep = build(n, keep)`. The assignment is load-bearing:
// `js_array_grow` does not grow in place — it allocates elsewhere, copies, and
// leaves a forwarding stub at the old address — so a caller that keeps the
// head it allocated before the call is holding a stub the moment the array
// outgrows MIN_ARRAY_CAPACITY (16).
//
// This file is a SMOKE TEST of the three call-site shapes, not a detector.
// A stale head is invisible to behaviour: every runtime entry point resolves
// the forwarding chain via `clean_arr_ptr`, so the printed answers are right
// either way. The load-bearing coverage is structural and lives in
// `crates/perry-transform/src/deforest/tests.rs`.

class Node {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

// Shape 1 — non-consumer call site: `const keep = build(n)`.
function build(n: number): Node[] {
  const out: Node[] = [];
  for (let i = 0; i < n; i++) out.push(new Node(i));
  return out;
}

// Shapes 2 and 3 — consumer-fuse call sites and pass-through recursion
// (the ABC451D shape deforestation exists for).
function tree(depth: number, label: number): number[] {
  const out: number[] = [];
  out.push(label);
  if (depth > 0) {
    const left = tree(depth - 1, label * 2 + 1);
    for (let j = 0; j < left.length; j++) out.push(left[j]);
    const right = tree(depth - 1, label * 2 + 2);
    for (let j = 0; j < right.length; j++) out.push(right[j]);
  }
  return out;
}

const keep = build(1000);
let sum = 0;
for (let i = 0; i < keep.length; i++) sum += keep[i].v;
console.log("build:", keep.length, sum, keep[0].v, keep[999].v);

const t = tree(9, 0);
let tsum = 0;
for (let i = 0; i < t.length; i++) tsum += t[i];
console.log("tree:", t.length, tsum, t[0], t[t.length - 1]);

// Right at the growth threshold: MIN_ARRAY_CAPACITY is 16, so 17 is the
// smallest N that reallocates. #7612's SIGBUS reproduced at exactly this size.
const small = build(17);
console.log("17:", small.length, small[16].v);

// Liveness fixture for the `Ptr<Shape>` census key (#7106).
//
// This file exists to make one assertion checkable: that the census can
// observe a `Ptr<Shape>` promotion AT ALL. A census key that reads zero
// because nothing was promoted and one that reads zero because nobody
// increments the counter look identical in a report; this program is written
// to satisfy every rule in `collectors/ptr_shape.rs`, so if its count is zero
// the instrument is broken, not the corpus.
//
// Rules satisfied, in the collector's own numbering:
//   1. provenance   — `let p = new Point(...)`, exactly one `Let`, init is `new`
//   2. containment  — `p` is only ever field-read/written and method-called;
//                     never reassigned, captured, passed, returned or aliased
//   3. this-flow    — `Point`'s constructor and methods only touch `this.<field>`
//   4. dispatch     — no own-property write can shadow `norm2`
//   5. module-wide  — no defineProperty / delete / setPrototypeOf / Proxy /
//                     mutating Reflect anywhere in this module
//
// Do not "tidy" this file. Passing `p` to a function, returning it, or storing
// it in an array all disqualify it (rule 2) and would silently take the
// fixture to zero.

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  norm2(): number {
    return this.x * this.x + this.y * this.y;
  }
}

function shapeProven(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const p = new Point(i, i + 1);
    p.x = p.x + 1;
    total = total + p.norm2();
  }
  return total;
}

console.log("ptr_shape:" + shapeProven(4));

// Constructor arguments must capture the value they had at `new` time, even
// when a LATER argument, a field initializer or the constructor body reassigns
// the variable they came from.
//
// Regression guard for the GC-rooting work (#6969/#6983): an argument that
// reads a registered root (a local, a module global) must be SNAPSHOT into a
// temp-root slot, not re-lowered after the fact. Re-lowering recovers the right
// address after an evacuating cycle but reads the variable's *current* value —
// which is a miscompile, not a rooting fix. Only immutable sources (string
// literals) may be re-loaded.
let g = "before";
function bump(): number {
  g = "after";
  return 1;
}

class C {
  p: unknown;
  q: unknown;
  constructor(p: unknown, q: unknown) {
    this.p = p;
    this.q = q;
  }
}

const sink: unknown[] = [];

// Argument 0 reads `g` before bump() reassigns it.
const c = new C(g, bump());
sink.push(c); // force a real allocation (defeat scalar replacement)
console.log("global captured:", c.p, "now:", g);

// Same, with a local reassigned by the constructor body itself.
let local = "L0";
class D {
  v: unknown;
  constructor(v: unknown) {
    local = "L1";
    this.v = v;
  }
}
const d = new D(local);
sink.push(d);
console.log("local captured:", d.v, "now:", local);
console.log("allocated:", sink.length);

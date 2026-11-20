// Issue #313: when a class constructor stored an arrow into another class's
// field via `this.s = new Store((x) => …)`, scalar-replacement of `new
// Holder()` inlined the constructor with a dummy `this_stack` slot that was
// never populated. `const self = this` then captured TAG_UNDEFINED, and
// `this`-using arrow bodies blew up — Symptom 1: `self.v` printed
// `undefined`; Symptom 2: direct `this.v` SIGSEGV'd.

// ─── Symptom 1: `const self = this` then closure references self ──────────
class Store1 {
  fn: (x: number) => void;
  constructor(f: (x: number) => void) {
    this.fn = f;
  }
  call(x: number) {
    this.fn(x);
  }
}

class Holder1 {
  v = 10;
  s: Store1;
  constructor() {
    const self = this;
    console.log("[Symptom1] self.v in ctor:", self.v);
    this.s = new Store1((x) => {
      console.log("[Symptom1] result:", x + self.v);
    });
  }
}

const h1 = new Holder1();
h1.s.call(5);

console.log("---");

// ─── Symptom 2: arrow body references `this.v` directly ──────────────────
class Store2 {
  fn: (x: number) => void;
  constructor(f: (x: number) => void) {
    this.fn = f;
  }
  call(x: number) {
    this.fn(x);
  }
}

class Holder2 {
  v = 10;
  s: Store2;
  constructor() {
    this.s = new Store2((x) => {
      console.log("[Symptom2] result:", x + this.v);
    });
  }
}

const h2 = new Holder2();
h2.s.call(5);

// ─── Bonus: chained method dispatch path also exercised ──────────────────
console.log("---");
h1.s.fn(7);
h2.s.fn(7);

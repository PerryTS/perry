// #11769: instances of a per-evaluation class (a class expression evaluated
// many times) carry their class's own keys at their evaluation's prototype.
// Field reads and writes, method calls and prototype identity must stay
// exact while the runtime reuses one evaluation's instance link and a field
// site remembers one evaluation's instance identity.

function mk(tag: string) {
  return class {
    x: number;
    y: string;
    constructor(x: number) {
      this.x = x;
      this.y = tag;
    }
    sum() {
      return this.x + this.y.length;
    }
    bump(d: number) {
      this.x = this.x + d;
      return this.x;
    }
    who() {
      return tag;
    }
  };
}

const A: any = mk("a");
const B: any = mk("bb");

// Alternating evaluations: every instance is linked to its own evaluation's
// prototype, never to the other one's.
let ok = 0;
for (let i = 0; i < 200; i++) {
  const a = new A(i);
  const b = new B(i);
  if (Object.getPrototypeOf(a) === A.prototype && Object.getPrototypeOf(b) === B.prototype) ok++;
  if (a instanceof A && !(a instanceof B) && b instanceof B && !(b instanceof A)) ok++;
  if (a.who() === "a" && b.who() === "bb") ok++;
}
console.log("alternating", ok);

// One evaluation used many times, then another: field reads and writes
// through the methods see each instance's own slots.
let s = 0;
for (let i = 0; i < 100; i++) s += new A(i).sum();
for (let i = 0; i < 100; i++) s += new B(i).sum();
console.log("sum", s);
const p = new A(5);
console.log("bump", p.bump(3), p.bump(0.5), p.x);

// A non-number stored into a numeric field generalizes that instance only.
const q = new A(1);
q.x = "str";
console.log("generalized", q.x, typeof q.sum(), new A(2).sum());

// Keys, order and descriptors match a shared class instance.
console.log("keys", Object.keys(new A(1)).join(","), JSON.stringify(new B(7)));

// A frozen instance rejects writes; a deleted field reads undefined.
const f = new A(9);
Object.freeze(f);
try {
  (function () {
    "use strict";
    f.x = 1;
  })();
  console.log("frozen write accepted");
} catch (e) {
  console.log("frozen", (e as Error).constructor.name, f.x);
}
const d = new B(4);
delete d.x;
console.log("deleted", d.x, d.sum());

// Re-linking one instance leaves the others on their evaluation.
const r = new A(3);
Object.setPrototypeOf(r, B.prototype);
console.log("relinked", r.who(), new A(3).who(), r instanceof B);

// A prototype method replaced on one evaluation only.
B.prototype.who = function () {
  return "patched";
};
console.log("patched", new A(1).who(), new B(1).who());

// A getter on the prototype never shadows an own field.
Object.defineProperty(A.prototype, "x", { get() { return -1; }, configurable: true });
console.log("own over getter", new A(42).x, new A(42).sum());

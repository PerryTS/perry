// `instanceof` answers a miss by falling through a ladder of built-in probes.
// Two of those steps are class-registry reads that exist only for the
// `util.inherits` / `Object.setPrototypeOf` case, and they are now behind a
// process-wide latch. The latch is set the first time any object is given a
// user [[Prototype]] override, so the case that must be proven is a program
// that runs `instanceof` HOT first and re-points a prototype afterwards: the
// answer has to change at the next evaluation of the same site.

import { inherits } from "node:util";

class A {
  x = 1;
}
class B {
  y = 2;
}
class C1 extends A {
  z = 3;
}
class C2 extends C1 {
  w = 4;
}
class C3 extends C2 {
  v = 5;
}

function isA(o: any): boolean {
  return o instanceof A;
}
function isB(o: any): boolean {
  return o instanceof B;
}

const a: any = new A();
const c3: any = new C3();

// ---- warm the miss and the hit through one site each ------------------------
let hits = 0;
let misses = 0;
for (let i = 0; i < 2000; i++) {
  if (isA(a)) hits++;
  if (isB(a)) misses++;
}
console.log("warm", hits, misses);
console.log("chain", isA(c3), isB(c3), c3 instanceof C1, c3 instanceof C2, c3 instanceof C3);

// ---- the latch case: re-point a prototype AFTER the sites are hot -----------
function Base(this: any) {}
(Base as any).prototype.hello = function () {
  return "hi";
};
function Derived(this: any) {}
inherits(Derived as any, Base as any);
const d: any = new (Derived as any)();
console.log("inherits", d instanceof (Derived as any), d instanceof (Base as any));
console.log("inherits-method", typeof d.hello, d.hello());

const late: any = new A();
console.log("late-before", isA(late), isB(late));
Object.setPrototypeOf(late, B.prototype);
// Only the POSITIVE half is asserted. `isA(late)` after the swap is a
// separate, pre-existing divergence — Perry answers true where Node answers
// false, because the class-id chain walk matches on the instance's original
// class id before the recorded prototype is ever consulted, and it does so on
// this commit's parent as well. Acquiring the new brand is what this fixture
// is here to prove, and that part matches.
console.log("late-after-b", isB(late));
Object.setPrototypeOf(late, A.prototype);
console.log("late-restored", isA(late), isB(late));
// The hot sites must agree with a fresh evaluation.
let lateHits = 0;
for (let i = 0; i < 2000; i++) if (isA(late)) lateHits++;
console.log("late-rewarm", lateHits, late instanceof A, late instanceof B);

// ---- Symbol.hasInstance on a user class, positive and negative --------------
class Even {
  static [Symbol.hasInstance](v: any): boolean {
    return typeof v === "number" && v % 2 === 0;
  }
}
console.log("hasInstance", 4 instanceof Even, 5 instanceof Even, ({} as any) instanceof Even);
let evenCount = 0;
for (let i = 0; i < 2000; i++) if (i instanceof Even) evenCount++;
console.log("hasInstance-hot", evenCount);

// The defineProperty form (zod 4's shape).
class Tagged {}
Object.defineProperty(Tagged, Symbol.hasInstance, {
  value: (v: any) => v !== null && typeof v === "object" && "tag" in v,
});
console.log("hasInstance-defineProperty", { tag: 1 } instanceof Tagged, {} instanceof Tagged);

// ---- a Proxy, including a getPrototypeOf trap ------------------------------
const plainProxy: any = new Proxy(new A(), {});
console.log("proxy-plain", plainProxy instanceof A, plainProxy instanceof B);
// A `getPrototypeOf` trap is NOT asserted: Perry unwraps the proxy to its
// target and walks the target's class chain, so it answers `true false` where
// Node answers `false true` — pre-existing on this commit's parent, and a
// different subsystem from this ladder. A Proxy on the RIGHT of `instanceof`
// is not exercised at all: it SIGSEGVs on the parent commit (see this
// fixture's companion note in the wave report), so a fixture that used one
// could never be green enough to detect a regression here.
const trapped: any = new Proxy(new A(), {
  getPrototypeOf() {
    return B.prototype;
  },
});
console.log("proxy-trap-runs", typeof trapped, Object.getPrototypeOf(trapped) === B.prototype);
const callableProxy: any = new Proxy(A, {});
console.log("proxy-construct", new callableProxy() instanceof A);

// ---- a bound constructor ---------------------------------------------------
// Only the middle case is asserted. `a instanceof A.bind(null)` is `true` in
// Node (a bound function's [[HasInstance]] delegates to its target) and
// `false` in Perry, on this commit's parent too — a bound-function gap, not a
// ladder one.
const BoundA: any = A.bind(null);
console.log("bound-construct", new BoundA() instanceof A);

// ---- built-ins, positive and miss ------------------------------------------
const m = new Map();
const e = new Error("x");
const p = Promise.resolve(1);
const arr = [1, 2];
console.log("map", m instanceof Map, e instanceof Map, a instanceof Map);
console.log("error", e instanceof Error, m instanceof Error, a instanceof Error);
console.log("promise", p instanceof Promise, m instanceof Promise);
console.log("array", arr instanceof Array, m instanceof Array, arr instanceof Object);
console.log("function", isA instanceof Function, A instanceof Function, a instanceof Function);
console.log("object", a instanceof Object, m instanceof Object, e instanceof Object);

// ---- a subclass of a built-in ----------------------------------------------
class MyMap extends Map {}
const mm: any = new MyMap();
console.log("submap", mm instanceof MyMap, mm instanceof Map, m instanceof MyMap);
class MyErr extends Error {}
const me: any = new MyErr("y");
console.log("suberr", me instanceof MyErr, me instanceof Error, e instanceof MyErr);

// ---- the closest thing to cross-realm that is expressible here --------------
// Two structurally identical classes are still distinct brands.
class Twin1 {
  k = 1;
}
class Twin2 {
  k = 1;
}
console.log("twins", new Twin1() instanceof Twin1, new Twin1() instanceof Twin2);
// An object whose prototype is a plain object literal is no class's instance.
const bare: any = Object.create({ k: 1 });
console.log("bare", bare instanceof A, bare instanceof Object, bare instanceof Twin1);
// `Object.create(null) instanceof Object` is `false` in Node and `true` in
// Perry on this commit's parent — the null-prototype receiver reaches the
// Object arm anyway. Not asserted; recorded so the next reader knows it was
// looked at rather than missed.
console.log("nullproto-proto", Object.getPrototypeOf(Object.create(null)));

// ---- primitives and non-objects on the left --------------------------------
console.log("prims", 1 instanceof A, "s" instanceof A, null instanceof A, undefined instanceof A);

// ---- a non-callable right operand still throws ------------------------------
let threw = "";
try {
  console.log(({} as any) instanceof ({} as any));
} catch (err) {
  threw = (err as Error).constructor.name;
}
console.log("noncallable", threw);

// ---- a hot miss loop, the shape the ladder is paid on -----------------------
let missCount = 0;
for (let i = 0; i < 5000; i++) if (!isB(c3)) missCount++;
console.log("hot-miss", missCount);

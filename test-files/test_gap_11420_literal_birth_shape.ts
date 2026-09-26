// #11420: an object literal's anon-shape class used to be registered AFTER
// module init minted its ShapeId, so the id recorded the wrong birth
// prototype and every literal allocation declined it and was stamped with a
// second id. Guarded reads (`this.a` in a literal method, `it.n` on a
// returned literal) then always missed and went by name. With the order
// fixed those reads hit the shape check, so every receiver below must still
// behave exactly as in node: the literal itself, foreign shapes, an extracted
// method, `.call`, shape changes after construction, deletes, spreads,
// representation changes, and literals built in functions and loops.

const o: any = {
  a: 1,
  b: 2,
  sum() { return this.a + this.b; },
  bump(v: number) { this.a = this.a + v; return this.a; },
};
let acc = 0;
for (let i = 0; i < 2000; i++) acc += o.sum();
console.log("hot-read", acc, o.sum());
for (let i = 0; i < 1000; i++) o.bump(1);
console.log("hot-write", o.a, o.sum());

// Extracted method: `this` is undefined -> globalThis in sloppy code.
const extracted = o.sum;
try {
  console.log("extracted", String(extracted()));
} catch (e) {
  console.log("extracted threw", (e as Error).constructor.name);
}

// `.call` with foreign receivers: another literal with a different key order,
// a class instance, a primitive-free plain object.
const foreign: any = { b: 40, a: 30, z: 0 };
console.log("call-foreign", o.sum.call(foreign), o.bump.call(foreign, 5), foreign.a);
class K { a = 100; b = 200; }
const k: any = new K();
console.log("call-class", o.sum.call(k), o.bump.call(k, 1), k.a);
let callAcc = 0;
for (let i = 0; i < 1000; i++) callAcc += o.sum.call(i % 2 ? foreign : o);
console.log("call-mixed", callAcc);

// Shape change after construction: add a key, delete a key, redefine as accessor.
const grow: any = { a: 3, b: 4, sum() { return this.a + this.b; } };
console.log("before-grow", grow.sum());
grow.c = 5;
console.log("after-add", grow.sum(), Object.keys(grow).join(","));
delete grow.b;
console.log("after-delete", String(grow.sum()), Object.keys(grow).join(","));
Object.defineProperty(grow, "a", { get() { return 1000; }, configurable: true });
grow.b = 1;
console.log("after-accessor", grow.sum());

// Representation change: a numeric field becomes a string, then an object.
const rep: any = { a: 1.5, get() { return this.a; } };
let repAcc = 0;
for (let i = 0; i < 500; i++) repAcc += rep.get();
rep.a = "str";
console.log("rep", repAcc, rep.get());
rep.a = { deep: 1 };
console.log("rep-obj", JSON.stringify(rep.get()));

// Spread copies: same keys, a fresh object; the copy's methods see the copy.
const copy: any = { ...o };
copy.b = 10;
console.log("spread", copy.sum(), o.sum(), copy.bump(2), o.a, copy.a);
const extended: any = { ...o, extra: true };
console.log("spread-extended", extended.sum(), Object.keys(extended).join(","));

// Literals built in a function and in a loop share one shape; all must read right.
function make(n: number): any {
  return { n, warm: n * 2, get() { return this.n + this.warm; } };
}
let madeAcc = 0;
for (let i = 0; i < 1000; i++) {
  const m = make(i);
  madeAcc += m.get() + m.n;
}
console.log("made", madeAcc);
const it = make(7);
let loopAcc = 0;
for (let i = 0; i < it.n; i++) loopAcc += it.warm;
console.log("returned-literal", loopAcc);

// The literal's identity facts must be unchanged.
console.log(
  "identity",
  Object.getPrototypeOf(o) === Object.prototype,
  o.constructor === Object,
  o instanceof Object,
  Object.keys(o).join(","),
  JSON.stringify({ a: 1, b: [2] }),
);

// Generic class specializations (their origin edge moved with the anon-shape
// registration): instanceof and field reads through methods.
class Box<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  get(): T { return this.v; }
}
const bn = new Box<number>(5);
const bs = new Box<string>("s");
let boxAcc = 0;
for (let i = 0; i < 100; i++) boxAcc += bn.get();
console.log("generic", boxAcc, bs.get(), bn instanceof Box, bs instanceof Box, Object.getPrototypeOf(bn) === Box.prototype);

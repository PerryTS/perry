// #10906: `this` inside object-literal methods and function expressions.
// Such a closure now binds its receiver once at entry, and `this.field` in a
// closed-shape literal method takes the guarded class-field path keyed on the
// literal's shape class. Every receiver below must still behave exactly as in
// node: foreign shapes, class instances, prototypes, primitives and nullish
// values, shape transitions, freeze, accessors, nested arrows, callees that
// rebind or throw, generators, async bodies, and allocation pressure.
const o: any = {
  a: 1,
  b: 2,
  get2() { return this.a + this.b; },
  set(v: number) { this.a = v; return this.a; },
};
console.log("basic", o.get2(), o.set(10), o.get2());

// Foreign receivers: different shape, class instance, Object.create, spread copy.
const other: any = { b: 5, a: 7, z: 1 };
console.log("call-other", o.get2.call(other), o.set.call(other, 3), other.a, other.z);
class K { a = 100; b = 200; }
const kk: any = new K();
console.log("call-class", o.get2.call(kk), o.set.call(kk, 4), kk.a, kk.b);
const derived: any = Object.create(o);
derived.a = 50;
console.log("proto", derived.get2(), o.a);
const copy: any = { ...o };
copy.b = 30;
console.log("spread", copy.get2(), copy.set(6), copy.a, o.a);
const holder: any = { a: 8, b: 9 };
holder.m = o.get2;
console.log("reassigned", holder.m());

// Two literals with the same fields share one shape class.
const s1: any = { a: 1, m() { return this.a; } };
const s2: any = { a: 2, m() { return this.a * 10; } };
console.log("shared-shape", s1.m(), s2.m(), s1.m.call(s2), s2.m.call(s1));

// Primitive and nullish receivers (sloppy conversion happens once).
const p: any = {
  kind() { return typeof this; },
  same() { return this === this; },
  val() { return this.valueOf(); },
  isGlobal() { return this === globalThis; },
};
console.log("prim", p.kind.call(5), p.same.call(5), p.kind.call("s"), p.same.call("s"), p.val.call(7));
console.log("nullish", p.isGlobal.call(undefined), p.isGlobal.call(null), p.kind.call(undefined));

// Shape transitions inside the method.
const t: any = {
  a: 1,
  b: 2,
  add() { this.q = 5; return this.q + this.a; },
  del() { delete this.a; return this.a; },
  again() { this.a = 11; return this.a + this.b; },
};
console.log("transition", t.add(), t.del(), t.again(), Object.keys(t).join(","));

// Frozen receiver: sloppy store silently fails.
const fr: any = Object.freeze({ a: 1, set() { try { this.a = 2; } catch (e) { return "threw"; } return this.a; } });
console.log("frozen", fr.set());

// Accessor installed over a field slot.
const g: any = { a: 1, read() { return this.a; }, write(v: number) { this.a = v; return this.a; } };
let stored = 0;
Object.defineProperty(g, "a", { get() { return 42 + stored; }, set(v: number) { stored = v; }, configurable: true });
console.log("accessor", g.read(), g.write(3), g.read());

// Nested arrows.
const n: any = {
  a: 3,
  arrows() { const f = () => this.a; return [1, 2].map((x) => x * this.a + f()).join(","); },
  arrowGlobal() { return (() => this)() === globalThis; },
};
const arrowGlobal = n.arrowGlobal;
console.log("nested", n.arrows(), arrowGlobal());

// A callee rebinding the implicit receiver, and a callee that throws.
const q: any = { a: 99, g() { return this.a; }, boom() { throw new Error("x"); } };
const c: any = {
  a: 1,
  viaCall() { const r = q.g(); return this.a + r; },
  viaThrow() { try { q.boom(); } catch (e) { } return this.a; },
  viaCallback() { return [10, 20].map(function (this: any, x: number) { return x + (this ? this.a : 0); }, q).join(","); },
};
console.log("rebind", c.viaCall(), c.viaThrow(), c.viaCallback());

// `arguments` alongside `this`, and identity.
const ar: any = { a: 1, m(...xs: number[]) { return arguments.length + this.a + xs.length; }, self() { return this; } };
console.log("args", ar.m(1, 2, 3), ar.self() === ar, ar.self.call(other) === other);

// Function expressions: method-style call, plain call, constructor.
const fe = function (this: any) { let s = 0; for (let i = 0; i < 10; i++) s += this.v; return s; };
console.log("fnexpr", fe.call({ v: 2 }), fe.call({ w: 1, v: 3 }));
const F: any = function (this: any, x: number) { this.x = x; this.y = this.x * 2; };
const fi = new F(3);
console.log("ctor", fi.x, fi.y);

// Allocation pressure while `this` stays live across collections.
const gc: any = {
  a: 1,
  arr: [] as any[],
  run(n: number) {
    let s = 0;
    for (let i = 0; i < n; i++) {
      this.arr.push({ v: i, s: "x" + i });
      if (this.arr.length > 1000) this.arr = [];
      s += this.a + this.arr.length;
    }
    return s;
  },
};
console.log("gc", gc.run(50000), gc.arr.length);

const acc: any = {
  _x: 2,
  get x() { return this._x * 10; },
  set x(v: number) { this._x = v + 1; },
};
acc.x = 4;
console.log("accessor-literal", acc.x, acc._x);
const viaProto: any = Object.create(acc);
viaProto._x = 7;
console.log("accessor-proto", viaProto.x);

const def: any = { _v: 3 };
Object.defineProperty(def, "v", { get: function (this: any) { return this._v + 1; }, set: function (this: any, n: number) { this._v = n; } });
def.v = 9;
console.log("defineProperty", def.v);

const bound = function (this: any, k: number) { return this.base + k; }.bind({ base: 40 });
console.log("bind", bound(2), bound.call({ base: 1 }, 2));

const gen: any = { a: 5, g: function* (this: any) { yield this.a; yield this.a + 1; } };
console.log("generator", [...gen.g()].join(","));

const lit: any = {
  n: 3,
  twice() { return [1, 2].map(function (this: any, x: number) { return x + this.n; }, this).join(","); },
  recurse(k: number): number { return k <= 0 ? this.n : this.recurse(k - 1) + 1; },
};
console.log("thisArg", lit.twice(), "recurse", lit.recurse(3));

const asy: any = {
  a: 11,
  run: async function (this: any) { const x = this.a; await null; return x + this.a; },
  arrowAsync: async function (this: any) { await null; return (() => this.a)(); },
};
asy.run().then((v: number) => console.log("async", v));
asy.arrowAsync().then((v: number) => console.log("async-arrow", v));

// Method reads `this` only on a rarely taken path.
const rare: any = { d: 99, pick(x: any) { if (x === undefined) return this.d; return x; } };
let s = 0;
for (let i = 0; i < 1000; i++) s += rare.pick(i % 100 === 0 ? undefined : 1);
console.log("rare", s);

// Primitive receivers through a literal method in a loop.
const prim: any = { tag() { return typeof this + ":" + (this === this) + ":" + String(this); } };
console.log("prim-loop", [1, "a", true].map((v) => prim.tag.call(v)).join(" "));

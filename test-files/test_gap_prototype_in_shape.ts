// The [[Prototype]] as a fact of the shape: every way an ordinary object gets
// or changes its prototype, read back through every reader.
const out: string[] = [];
const log = (...a: unknown[]) => out.push(a.map((x) => String(x)).join(" "));

// 1. plain function constructors, prototype reassigned after instances exist
function F(this: any, v: number) {
  this.v = v;
}
(F as any).prototype.get = function (this: any) {
  return this.v;
};
(F as any).prototype.kind = "old";
const fs: any[] = [];
for (let i = 0; i < 2000; i++) fs.push(new (F as any)(i));
const oldProto = (F as any).prototype;
(F as any).prototype = { kind: "new", tag: "new" };
const f2 = new (F as any)(7);
// Prototype reads only: a call of a method registered on the OLD
// `F.prototype` is a separate codegen question (reported with this lane).
log("fn", fs[5].get(), f2.kind, fs[5].kind, f2.tag, fs[5].tag, fs[5] instanceof (F as any), f2 instanceof (F as any));
log("fnproto", Object.getPrototypeOf(fs[1999]) === oldProto, Object.getPrototypeOf(f2) === (F as any).prototype);
(F as any).prototype = oldProto;
const f3 = new (F as any)(9);
log("fnrestore", fs[0] instanceof (F as any), f3 instanceof (F as any), f2 instanceof (F as any), f3.get(), f3.kind);

// 2. inherited null / undefined values, symbol keys 2+ hops
const s1 = Symbol("s1");
const s2 = Symbol.for("s2");
const base: any = { n: null, u: undefined, z: 0, [s1]: "sym1", [s2]: null };
const mid: any = Object.create(base);
mid.m = "mid";
const leaf: any = Object.create(mid);
log("null", leaf.n === null, leaf.u === undefined, "n" in leaf, leaf.z, leaf[s1], leaf[s2] === null, s1 in leaf);
function G(this: any) {}
(G as any).prototype = leaf;
const g: any = new (G as any)();
log("deep", g.n === null, g[s1], g.m, g[s2] === null, g instanceof (G as any), Object.getPrototypeOf(g) === leaf);

// 3. Object.create(null) and __proto__: null
const nul: any = Object.create(null);
nul.a = 1;
const nul2: any = { __proto__: null, b: 2 };
log("nullproto", Object.getPrototypeOf(nul) === null, Object.getPrototypeOf(nul2) === null, "toString" in nul, "toString" in nul2, nul.a, nul2.b);
const nuls: any[] = [];
for (let i = 0; i < 500; i++) { const o: any = Object.create(null); o.k = i; nuls.push(o); }
log("nulls", nuls[499].k, Object.getPrototypeOf(nuls[3]) === null, nuls[3] instanceof Object);

// 4. __proto__ in literals and as a setter
const p1 = { hello() { return "p1"; } };
const p2 = { hello() { return "p2"; } };
const lit: any = { __proto__: p1, x: 1 };
log("lit", lit.hello(), Object.getPrototypeOf(lit) === p1);
lit.__proto__ = p2;
log("litset", lit.hello(), Object.getPrototypeOf(lit) === p2, lit.x);

// 5. setPrototypeOf on class instances and on classes
class A { who() { return "A"; } static s() { return "sA"; } }
class B { who() { return "B"; } static s() { return "sB"; } }
const a = new A();
const a2 = new A();
Object.setPrototypeOf(a, B.prototype);
log("cls", a.who(), a2.who(), a instanceof A, a instanceof B, a2 instanceof A);
class C2 {}
Object.setPrototypeOf(C2, B);
log("clsstatic", (C2 as any).s(), Object.getPrototypeOf(C2) === B);
Object.setPrototypeOf(a, null);
log("clsnull", Object.getPrototypeOf(a) === null, a instanceof A, typeof (a as any).who);

// 6. functions and arrays as prototypes (identities with no serial)
function H() { return 1; }
(H as any).extra = "fnprop";
const viaFn: any = Object.create(H);
const viaArr: any = Object.create([10, 20, 30]);
log("unique", viaFn.extra, typeof viaFn.call, viaArr[1], viaArr.length, viaFn instanceof Function, Array.isArray(viaArr));
viaFn.own = 1;
viaFn.own2 = 2;
log("unique2", viaFn.extra, Object.getPrototypeOf(viaFn) === H);

// 7. prototype changes across many shapes (transitions keep the prototype)
const pr = { kind: "pr" };
const many: any[] = [];
for (let i = 0; i < 3000; i++) {
  const o: any = Object.create(pr);
  o["k" + (i % 7)] = i;
  if (i % 3 === 0) o.extra = i;
  if (i % 5 === 0) delete o.extra;
  many.push(o);
}
let ok = 0;
for (const o of many) if (Object.getPrototypeOf(o) === pr && o.kind === "pr") ok++;
log("many", ok);

// 8. instanceof across all of these
function K() {}
const k1 = new (K as any)();
const k2 = Object.create((K as any).prototype);
const k3: any = { __proto__: (K as any).prototype };
const k4 = {};
Object.setPrototypeOf(k4, (K as any).prototype);
log("inst", k1 instanceof (K as any), k2 instanceof (K as any), k3 instanceof (K as any), k4 instanceof (K as any));
Object.setPrototypeOf(k4, null);
log("inst2", k4 instanceof (K as any), (K as any).prototype.isPrototypeOf(k1), Object.prototype.isPrototypeOf(k3));

// 9. a prototype that itself changes shape and prototype later
const top1: any = { t: 1 };
const top2: any = { t: 2 };
const midp: any = Object.create(top1);
const objs: any[] = [];
for (let i = 0; i < 100; i++) objs.push(Object.create(midp));
midp.added = "yes";
Object.setPrototypeOf(midp, top2);
log("chain", objs[50].t, objs[50].added, Object.getPrototypeOf(objs[0]) === midp);

// 10. garbage: many short-lived prototypes
let sum = 0;
for (let i = 0; i < 20000; i++) {
  const p: any = { base: i };
  const o: any = Object.create(p);
  o.own = 1;
  sum += o.base + o.own;
}
log("churn", sum);
function mk(i: number) {
  function Local(this: any) { this.i = i; }
  (Local as any).prototype.twice = function (this: any) { return this.i * 2; };
  return new (Local as any)();
}
let s = 0;
for (let i = 0; i < 5000; i++) s += mk(i).twice();
log("localctor", s);

console.log(out.join("\n"));

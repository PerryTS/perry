// Each evaluation of a class declaration or expression creates a distinct
// class: its own constructor function, statics and prototype.

function make(n: number) {
  return class K {
    static s = n;
    static m() { return this.s; }
    x = n;
    get() { return this.x; }
  };
}

const K1 = make(1);
const K2 = make(2);
console.log("K1 !== K2", K1 !== K2);
console.log("statics", K1.s, K2.s);
console.log("static m", K1.m(), K2.m());
const a = new K1();
const b = new K2();
console.log("fields", a.x, b.x, a.get(), b.get());
console.log("a instanceof K1", a instanceof K1, "a instanceof K2", a instanceof K2);
console.log("b instanceof K1", b instanceof K1, "b instanceof K2", b instanceof K2);
console.log("proto own", Object.getPrototypeOf(a) === K1.prototype, Object.getPrototypeOf(a) !== K2.prototype);
console.log("prototypes differ", K1.prototype !== K2.prototype);
console.log("constructor", a.constructor === K1, b.constructor === K2);
K1.s = 10;
console.log("write K1.s", K1.s, K2.s, K1.m(), K2.m());

// A declaration inside a factory, with a subclass created in the same scope.
function makePair(tag: string) {
  class Base {
    static label = tag;
    static who() { return "base:" + this.label; }
    hello() { return "hello " + tag; }
  }
  class Sub extends Base {
    static extra = tag + "!";
    hello() { return super.hello() + " from sub"; }
  }
  return { Base, Sub };
}

const p = makePair("p");
const q = makePair("q");
console.log("Base distinct", p.Base !== q.Base, "Sub distinct", p.Sub !== q.Sub);
console.log("labels", p.Base.label, q.Base.label, p.Sub.label, q.Sub.label);
console.log("who", p.Base.who(), q.Base.who(), p.Sub.who(), q.Sub.who());
console.log("extra", p.Sub.extra, q.Sub.extra);
console.log("super chain", Object.getPrototypeOf(p.Sub) === p.Base, Object.getPrototypeOf(q.Sub) === q.Base,
  Object.getPrototypeOf(p.Sub) === q.Base);
const ps = new p.Sub();
const qs = new q.Sub();
console.log("hello", ps.hello(), qs.hello());
console.log("ps instanceof", ps instanceof p.Sub, ps instanceof p.Base, ps instanceof q.Sub, ps instanceof q.Base);
console.log("qs instanceof", qs instanceof q.Sub, qs instanceof q.Base, qs instanceof p.Sub, qs instanceof p.Base);
console.log("sub proto chain", Object.getPrototypeOf(p.Sub.prototype) === p.Base.prototype,
  Object.getPrototypeOf(p.Sub.prototype) === q.Base.prototype);

// Class expressions in a loop.
const classes: any[] = [];
for (let i = 0; i < 3; i++) {
  classes.push(class {
    static id = i;
    static twice() { return this.id * 2; }
    v = i * 10;
  });
}
console.log("loop distinct", classes[0] !== classes[1], classes[1] !== classes[2], classes[0] !== classes[2]);
console.log("loop ids", classes.map((c) => c.id).join(","), classes.map((c) => c.twice()).join(","));
const objs = classes.map((C) => new C());
console.log("loop fields", objs.map((o) => o.v).join(","));
console.log("loop instanceof", objs.map((o, i) => classes.map((C) => (o instanceof C ? 1 : 0)).join("")).join(" "));
console.log("loop prototypes", classes[0].prototype !== classes[1].prototype);

// The same evaluation still has one identity.
function once() { class Z { static t = 1; } return [Z, Z]; }
const [z1, z2] = once();
console.log("same evaluation", z1 === z2);

// A hoisted function and a sibling class method that name a class declared later.
function f1(n: number) {
  function g() { return new K(); }
  class K { static count = 0; v = n; constructor() { K.count++; } static make() { return new K(); } self() { return K; } }
  const a = g(); const b = K.make();
  return { K, a, b };
}
const r1 = f1(1), r2 = f1(2);
console.log(r1.K.count, r2.K.count, r1.a.v, r2.b.v, r1.a.self() === r1.K, r2.a.self() === r1.K);
console.log(r1.a instanceof r1.K, r1.a instanceof r2.K, r1.K.name, typeof r1.K);
function f2(x: string) {
  class A { tag() { return x; } }
  class B extends A { tag() { return "B" + super.tag(); } static of() { return new B(); } }
  class C extends B {}
  return [new C(), B.of(), C];
}
const [c1, b1, C1] = f2("1") as any; const [c2, b2, C2] = f2("2") as any;
console.log(c1.tag(), c2.tag(), b1.tag(), b2.tag(), c1 instanceof C1, c1 instanceof C2, C1 === C2);
function f3() { class P { #p = 1; static has(o: any) { return #p in o; } } return P; }
const P1 = f3(), P2 = f3();
console.log(P1.has(new P1()), P1.has(new P2()), P1 !== P2);
function f4(k: string) { class M { static [k] = 5; ["m" + k]() { return k; } } return M; }
const M1: any = f4("a"), M2: any = f4("b");
console.log(M1.a, M2.b, M1.b, new M1().ma(), new M2().mb());
function f5() { class Q { static s = 1; static get g() { return this.s + 1; } static set g(v) { this.s = v; } } return Q; }
const Q1: any = f5(), Q2: any = f5(); Q1.g = 10;
console.log(Q1.g, Q2.g, Object.keys(Q1).join(), Object.getOwnPropertyNames(Q2.prototype).join());
const lobjs: any[] = []; for (let i = 0; i < 3; i++) { class L { i = i; static k = i; } lobjs.push(new L()); }
console.log(lobjs.map((o: any) => o.i + ":" + o.constructor.k).join(" "), lobjs[0].constructor !== lobjs[1].constructor);
function f6() { class E extends Error { constructor(m: string) { super(m); this.name = "E"; } } return E; }
const E1 = f6(), E2 = f6();
try { throw new E1("boom"); } catch (e: any) { console.log(e instanceof E1, e instanceof E2, e instanceof Error, e.message, e.name); }
function f7() { class Arr extends Array {} return Arr; }
const A1 = f7(); const arr = new A1(); arr.push(1, 2); console.log(arr.length, arr instanceof A1, Array.isArray(arr));
function f8() { class S { static inst: any; static get() { return S.inst ??= new S(); } } return S; }
const S1 = f8(), S2 = f8(); console.log(S1.get() === S1.get(), S1.get() !== S2.get());
function f9(n: number) {
  class Node0 { wrap() { return new Wrapped(this); } isWrapped(o: any) { return o instanceof Wrapped; } }
  class Wrapped extends Node0 { static tag = n; inner: any; constructor(inner: any) { super(); this.inner = inner; } }
  return { Node0, Wrapped };
}
const w1 = f9(1), w2 = f9(2);
const ww = new w1.Node0().wrap();
console.log(ww instanceof w1.Wrapped, ww instanceof w2.Wrapped, new w2.Node0().isWrapped(ww), new w1.Node0().isWrapped(ww), (ww.constructor as any).tag);

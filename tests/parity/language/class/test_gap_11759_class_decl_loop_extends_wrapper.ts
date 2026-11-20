// #11759: class declarations in a module-top loop body, `extends` of an
// evaluated class, instanceof across evaluations, and an esbuild-style lazy
// `__esm` wrapper whose body runs once (it keeps one class).

const top: any[] = [];
for (let i = 0; i < 4; i++) {
  class L { static k = i; v() { return i; } }
  top.push(L);
}
console.log("loop-distinct", top[0] === top[1], top[2] === top[3]);
console.log("loop-statics", top.map((c: any) => c.k).join(), top.map((c: any) => new c().v()).join());
console.log("loop-instanceof", new top[3]() instanceof top[0], new top[0]() instanceof top[3], new top[3]() instanceof top[3]);

// `extends` of a class evaluated in the same body follows that evaluation.
function chain() {
  class P { p() { return "p"; } }
  class Q extends P { q() { return "q" + this.p(); } }
  return [P, Q];
}
const [P1, Q1]: any = chain(), [P2, Q2]: any = chain();
console.log("extends", P1 === P2, Q1 === Q2, new Q2().q(), new Q1().q());
console.log("extends-instanceof", new Q2() instanceof P2, new Q2() instanceof P1, new Q1() instanceof P1, new Q1() instanceof P2);
console.log("extends-proto", Object.getPrototypeOf(Q2) === P2, Object.getPrototypeOf(Q1) === P1, Object.getPrototypeOf(Q2.prototype) === P2.prototype);

// A module-level parent shared by every evaluation of the subclass.
class Base { hi() { return "hi"; } }
function sub() { class D extends Base { static s = 1; } return D; }
const d1: any = sub(), d2: any = sub();
d2.s = 2;
console.log("ext-base", d1 === d2, new d1().hi(), new d2() instanceof Base, new d2() instanceof d1, d1.s, d2.s);

// esbuild's lazy ESM wrapper: the init arrow runs its body at most once.
var __esm = (fn: any, res?: any) => function () {
  return fn && (res = (0, fn[Object.keys(fn)[0]])(fn = 0)), res;
};
let E1: any;
const init_x: any = __esm({ "x.ts"() { class EK { static q = 3; } E1 = EK; } });
init_x();
const e1 = E1;
init_x();
console.log("esm", e1 === E1, E1.q, new E1() instanceof e1);

// A CommonJS-style factory body called twice.
function factory(exports: any) { class C { static id = Math.random(); } exports.C = C; return exports; }
const m1 = factory({}), m2 = factory({});
console.log("cjs-twice", m1.C === m2.C, m1.C.id === m2.C.id, new m1.C() instanceof m2.C);

// A three-level chain declared in one body, evaluated three times: super
// calls, super method calls, inherited statics and every cross-evaluation
// instanceof.
function chain3(tag: string) {
  class A { static kind = "A" + tag; a: string; constructor() { this.a = tag; } who() { return "A" + this.a; } }
  class B extends A { b = 1; who() { return "B" + super.who(); } }
  class C extends B { static extra = 2; who() { return "C" + super.who(); } }
  return { A, B, C };
}
const k1: any = chain3("1"), k2: any = chain3("2"), k3: any = chain3("3");
for (const k of [k1, k2, k3]) {
  const c = new k.C();
  console.log("chain3", c.who(), c.b, k.C.kind, k.C.extra, c instanceof k.A, c instanceof k.B,
    Object.getPrototypeOf(k.C) === k.B, Object.getPrototypeOf(k.B) === k.A,
    Object.getPrototypeOf(k.C.prototype) === k.B.prototype, c.constructor === k.C);
}
console.log("chain3-cross", new k2.C() instanceof k1.A, new k1.C() instanceof k2.B, new k3.B() instanceof k3.A,
  k1.C === k2.C, k1.A === k3.A);

// A subclass declared in a nested function, extending the enclosing body's class.
function outer(n: number) {
  class Base2 { n = n; }
  function inner() { class Sub2 extends Base2 { twice() { return this.n * 2; } } return Sub2; }
  return [Base2, inner(), inner()];
}
const [ob1, os1, os1b]: any = outer(1), [ob2, os2]: any = outer(2);
console.log("nested-extends", new os1().twice(), new os2().twice(), new os2() instanceof ob2, new os2() instanceof ob1,
  new os1b() instanceof ob1, os1 === os1b, Object.getPrototypeOf(os2) === ob2);

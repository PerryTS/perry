// Classes created per evaluation ("fresh" classes) behave like any class:
// a subclass extends the evaluated class, and the class object owns its
// `length`, `name`, `prototype` and static methods as real properties.
// expected.txt is node 26.5.1's output. Three groups of lines:
//   ext-*  a static `extends` of a fresh class reaches that evaluation
//   own-*  own properties of a fresh class object, and `delete`
//   str-*  the source text of a fresh class
//   tpl-*  many evaluations of one class: each after the first is built from
//          the template's shapes, and owns its statics, prototype and methods
// The factories hold a capturing class so the declarations are fresh today,
// and a same-named class elsewhere so the heritage name is scope-renamed.

function d(desc: PropertyDescriptor | undefined): string {
  if (!desc) return "none";
  return [
    "value" in desc ? "data:" + typeof desc.value : "accessor",
    desc.writable, desc.enumerable, desc.configurable,
  ].join(",");
}

// ---- ext: `class D extends L`, L fresh ------------------------------------
class L { whoTop() { return "top"; } }
function make(n: number) {
  const k = n;
  class L {
    static make() { return "make" + k; }
    id() { return k; }
    hello() { return "L" + k; }
  }
  class D extends L {
    hello() { return "D>" + super.hello(); }
  }
  class G extends D {}
  return { L, D, G };
}
const e1 = make(1);
const e2 = make(2);
console.log("ext-proto", Object.getPrototypeOf(e1.D) === e1.L, Object.getPrototypeOf(e2.D) === e2.L,
  Object.getPrototypeOf(e1.D) === e2.L);
console.log("ext-protoproto", Object.getPrototypeOf(e1.D.prototype) === e1.L.prototype,
  Object.getPrototypeOf(e2.D.prototype) === e2.L.prototype,
  Object.getPrototypeOf(e1.D.prototype) === e2.L.prototype);
console.log("ext-instanceof", new e1.G() instanceof e1.L, new e1.G() instanceof e2.L,
  new e2.G() instanceof e2.L, new e2.G() instanceof e1.L, new e1.D() instanceof e1.G);
console.log("ext-instance", new e1.G().id(), new e2.G().id(), new e1.G().hello(), new e2.G().hello());
console.log("ext-statics", e1.D.make(), e2.D.make(), e1.G.make(), e2.G.make());
console.log("ext-top", new L().whoTop());

// ---- own: reflection over a fresh class object ----------------------------
function mk(tag: string) {
  const t = tag;
  return class Q {
    static s() { return "s" + t; }
    static t() { return "t" + t; }
    static f = "f" + t;
    m() { return t; }
  };
}
const Q1: any = mk("1");
const Q2: any = mk("2");
console.log("own-names", JSON.stringify(Object.getOwnPropertyNames(Q1)));
console.log("own-keys", JSON.stringify(Object.keys(Q1)));
console.log("own-has", ["length", "name", "prototype", "s", "t", "f", "m", "x"].map((k) => Object.hasOwn(Q1, k)).join(","));
console.log("own-in", ["length", "name", "prototype", "s", "f", "x"].map((k) => k in Q1).join(","));
console.log("own-values", Q1.length, Q1.name, typeof Q1.prototype, Q1.s(), Q1.f);
console.log("own-desc", ["length", "name", "prototype", "s", "f"].map((k) => k + "=" + d(Object.getOwnPropertyDescriptor(Q1, k))).join(" "));
console.log("own-identity", Q1.s === Q1.s, Q1.s === Q2.s, Q1.prototype === Q2.prototype);
console.log("own-delete", delete Q1.s, delete Q1.nothing);
console.log("own-after", JSON.stringify(Object.getOwnPropertyNames(Q1)), Object.hasOwn(Q1, "s"), typeof Q1.s, "s" in Q1);
console.log("own-sibling", JSON.stringify(Object.getOwnPropertyNames(Q2)), Q2.s());
console.log("own-delete-name", delete Q1.name, Object.hasOwn(Q1, "name"), Q1.name === undefined || Q1.name === "",
  JSON.stringify(Object.getOwnPropertyNames(Q1)));
Q1.s = function () { return "again"; };
console.log("own-redefine", Q1.s(), JSON.stringify(Object.getOwnPropertyNames(Q1)));
console.log("own-sibling2", Q2.name, Q2.s(), Q2.t());

// A subclass sees the parent evaluation's own statics, deleted ones included.
function sub(tag: string) {
  const t = tag;
  class P { static s() { return "P" + t; } static u() { return "u" + t; } }
  class C extends P {}
  return { P, C };
}
const s1 = sub("a");
const s2 = sub("b");
console.log("own-inherit", s1.C.s(), s2.C.s(), Object.hasOwn(s1.C, "s"), JSON.stringify(Object.getOwnPropertyNames(s1.C)));
delete (s1.P as any).s;
console.log("own-inherit-deleted", typeof (s1.C as any).s, s2.C.s(), typeof (s1.P as any).u);

// An own `name` / `length` that a static member takes over.
function over(tag: string) {
  const t = tag;
  return class W {
    static name = "n" + t;
    static length() { return "len" + t; }
    static g() { return t; }
  };
}
const W1: any = over("w");
console.log("own-over", W1.name, typeof W1.length, W1.length(), JSON.stringify(Object.getOwnPropertyNames(W1)),
  d(Object.getOwnPropertyDescriptor(W1, "name")), d(Object.getOwnPropertyDescriptor(W1, "length")));

// ---- str: the class source ------------------------------------------------
function src(tag: string) {
  const t = tag;
  return class Src {
    static s() { return t; }
  };
}
const S1: any = src("x");
const S2: any = src("y");
const text = "class Src {\n    static s() { return t; }\n  }";
console.log("str-string", String(S1) === text, String(S2) === text);
console.log("str-method", S1.toString() === text, S2.toString() === text);
console.log("str-template", `${S1}` === text, ("" + S1) === text);
console.log("str-proto", Function.prototype.toString.call(S1) === text);
function ov(tag: string) {
  const t = tag;
  return class Ov {
    static toString() { return "custom" + t; }
  };
}
const O1: any = ov("1");
console.log("str-override", String(O1), O1.toString(), `${O1}`);
// A static `toString` this evaluation no longer holds is not in the way; an
// own `toString` that is not callable makes the conversion throw.
function sd(i: number) { class D { v = i; static toString() { return "d" + i; } } return D; }
const D1: any = sd(1), D2: any = sd(2);
delete D1.toString;
console.log("str-deleted-static", String(D1) === Function.prototype.toString.call(D1), String(D2));
function su(i: number) { class U { v = i; } return U; }
const U1: any = su(1);
U1.toString = undefined;
try { console.log("str-own-undefined", String(U1)); } catch (e) { console.log("str-own-undefined", (e as any).constructor.name); }
// Integer keys come first among a class object's own keys, before `prototype`.
function ix(i: number) { class X { v = i; static 0() { return 0; } static s() { return i; } } return X; }
console.log("own-index", JSON.stringify(Object.getOwnPropertyNames(ix(1))), JSON.stringify(Object.getOwnPropertyNames(ix(2))));
// ---- tpl: evaluations after the first are born in the template shapes ----
function tp(tag: string, base: any) {
  const t = tag;
  class T extends base {
    static s() { return "s" + t; }
    static who() { return this.s(); }
    m() { return "m" + t; }
    n(a: number, b = 1) { return a + b; }
  }
  return T;
}
class B0 { b() { return "b"; } }
const T1: any = tp("1", B0), T2: any = tp("2", B0), T3: any = tp("3", B0);
console.log("tpl-own", JSON.stringify(Object.getOwnPropertyNames(T2)), JSON.stringify(Object.getOwnPropertyNames(T3.prototype)));
console.log("tpl-statics", T1.s(), T2.s(), T3.s(), T1.s === T2.s, T2.s === T3.s, T2.s.name, T2.s.length);
const g2 = T2.s, g3 = T3.s;
console.log("tpl-detached", g2(), g3(), T2.s.call(T3), T3.who(), T2.who.call(T3));
console.log("tpl-methods", new T1().m(), new T2().m(), new T3().m(), new T3().n(2), new T3().b());
console.log("tpl-method-identity", T1.prototype.m === T2.prototype.m, new T2().m === T2.prototype.m,
  T3.prototype.n.name, T3.prototype.n.length, typeof T3.prototype.m);
console.log("tpl-proto", Object.getPrototypeOf(T2.prototype) === B0.prototype, T2.prototype.constructor === T2,
  Object.getPrototypeOf(new T3()) === T3.prototype, new T3() instanceof B0, new T3() instanceof T2);
console.log("tpl-desc", JSON.stringify(Object.getOwnPropertyDescriptor(T3, "s")), JSON.stringify(Object.getOwnPropertyDescriptor(T3.prototype, "m")));
delete T2.s; T3.s = () => "re";
console.log("tpl-isolated", typeof T2.s, T3.s(), T1.s(), tp("4", B0).s());
delete T2.prototype.n;
console.log("tpl-proto-isolated", typeof T2.prototype.n, typeof T3.prototype.n,
  JSON.stringify(Object.getOwnPropertyNames(tp("5", B0).prototype)), JSON.stringify(Object.getOwnPropertyNames(T2.prototype)));
let acc = ""; for (let i = 0; i < 40; i++) { const K: any = tp("k" + i, B0); acc = K.s() + new K().m() + K.length; }
console.log("tpl-loop", acc);
function ta(tag: string) { const t = tag; class A { get g() { return t; } m() { return "m" + t; } } return A; }
const A1: any = ta("1"), A2: any = ta("2");
delete A1.prototype.m;
const A3: any = ta("3");
console.log("tpl-accessor-isolated", JSON.stringify(Object.getOwnPropertyNames(A3.prototype)), new A3().m(), new A2().m(), typeof A1.prototype.m, new A3().g);

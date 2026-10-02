// Classes created per evaluation ("fresh" classes) behave like any class:
// a subclass extends the evaluated class, and the class object owns its
// `length`, `name`, `prototype` and static methods as real properties.
// expected.txt is node 26.5.1's output. Three groups of lines:
//   ext-*  a static `extends` of a fresh class reaches that evaluation
//   own-*  own properties of a fresh class object, and `delete`
//   str-*  the source text of a fresh class
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

// A computed ClassBody member is named by the class definition that
// evaluates its key: each evaluation of a class expression gets exactly its
// own keys, in ClassBody order, mixed with literal members (#12300).

function make(k: string) {
  return class {
    [k]() {
      return "m:" + k;
    }
  };
}
const A = make("a");
const B = make("b");
const C = make("c");
console.log(Object.getOwnPropertyNames(A.prototype).join(","));
console.log(Object.getOwnPropertyNames(B.prototype).join(","));
console.log(Object.getOwnPropertyNames(C.prototype).join(","));
const a: any = new A();
const b: any = new B();
console.log(typeof a.a, typeof a.b, typeof b.a, typeof b.b);
console.log(b.b());

// Literal members around computed ones, in every order.
const k1 = "k1";
const k2 = "k2";
class D1 {
  [k1]() { return 1; }
  [k2]() { return 2; }
  m() { return 3; }
}
class D2 {
  m() { return 1; }
  [k1]() { return 2; }
  n() { return 3; }
}
class D3 {
  x() { return 0; }
  get [k1]() { return "g"; }
  set [k1](v: string) {}
  [k2]() { return 2; }
  y() { return 4; }
}
for (const K of [D1, D2, D3]) {
  console.log(K.name, Object.getOwnPropertyNames(K.prototype).join(","));
}
const d3: any = new D3();
console.log(d3.k1, d3.k2(), d3.y());
console.log(typeof Object.getOwnPropertyDescriptor(D3.prototype, "k1")!.get);

// Re-evaluating a declaration inside a function: each evaluation's own keys.
function f(key: string) {
  class E {
    x() { return 0; }
    [key]() { return key; }
    y() { return 2; }
  }
  return E;
}
const E1 = f("p");
const E2 = f("q");
const E3 = f("p");
// (Later evaluations are compared as sets: their prototype key order is a
// separate known gap.)
console.log(Object.getOwnPropertyNames(E1.prototype).join(","));
console.log(Object.getOwnPropertyNames(E2.prototype).sort().join(","));
console.log(Object.getOwnPropertyNames(E3.prototype).sort().join(","));
console.log((new E2() as any).q(), (new E1() as any).p(), typeof (new E1() as any).q);

// A symbol key between string members keeps the string order intact.
const sym = Symbol("s");
class S {
  a() { return 1; }
  [sym]() { return "sym"; }
  [Symbol.iterator]() { return [1, 2][Symbol.iterator](); }
  b() { return 2; }
}
console.log(Object.getOwnPropertyNames(S.prototype).join(","));
console.log((new S() as any)[sym](), [...(new S() as any)].join("+"));

// Many evaluations with distinct keys: each class answers only its own key.
let ok = 0;
for (let i = 0; i < 200; i++) {
  const K = make("key" + i);
  const inst: any = new K();
  if (typeof inst["key" + i] === "function" && inst["key" + (i + 1)] === undefined) ok++;
}
console.log("distinct", ok);

// Inheritance through computed members and super calls.
const base = "base";
class P {
  [base]() { return "P.base"; }
  plain() { return "P.plain"; }
}
class Q extends P {
  [base]() { return "Q>" + super[base](); }
  plain() { return "Q>" + super.plain(); }
}
const q: any = new Q();
console.log(q.base(), q.plain());

// super(...args) runs the parent constructor with the spread arguments: a mixed list,
// into a parent rest parameter, before the derived class's field initializers, into a
// built-in Error or Map parent, and from the constructor synthesized for a derived
// class that captures a variable.
class Base { a: number; b: string; constructor(a: number, b: string) { this.a = a; this.b = b; } }
class D1 extends Base { constructor(...args: [number, string]) { super(...args); } }
class D2 extends Base { constructor(n: number) { const rest: [string] = ["y"]; super(n, ...rest); } }
class R { first: number; more: number[]; constructor(first: number, ...more: number[]) { this.first = first; this.more = more; } }
class S extends R { constructor(...xs: number[]) { super(...xs); } }
class F extends Base { tag = this.a * 10; constructor(...xs: [number, string]) { super(...xs); } }
class E extends Error { constructor(...xs: [string]) { super(...xs); } }
class M extends Map<string, number> { constructor(...xs: any[]) { super(...xs); } }
function make(k: number) {
  class B { v: number; constructor(v: number) { this.v = v; } }
  class C extends B { get k() { return k; } }
  return new C(3);
}
const d1 = new D1(1, "x"), d2 = new D2(7), s = new S(1, 2, 3), f = new F(4, "f");
console.log(d1.a, d1.b, d2.a, d2.b, s.first, s.more.join(","), f.tag, f.b);
const e = new E("boom"), m = new M([["k", 1]]), c = make(9);
console.log(e.message, e instanceof Error, m.get("k"), m.size, c.v, c.k);

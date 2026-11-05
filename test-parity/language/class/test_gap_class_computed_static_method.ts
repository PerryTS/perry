// A computed-name ClassBody static method is an own data property of the class
// function object, like a named one: one function object per (class, method),
// `name` = the key, the declared attributes, inherited by a subclass, and a
// direct call runs it only while the property still holds it.
const k = "comp";
const w = "who";
let order: string[] = [];
function key(s: string): string {
  order.push(s);
  return s;
}
class A {
  static first() {
    return 0;
  }
  static [k](x: number) {
    return x + 1;
  }
  static [key("late")](a: number, b = 2) {
    return a + b;
  }
}
console.log(order.join(","));
console.log(Object.getOwnPropertyNames(A).join(","));
console.log(typeof A.comp, A.comp(1), A.comp.name, A.comp.length);
console.log((A as any).late.name, (A as any).late.length, (A as any).late(1));
const d = Object.getOwnPropertyDescriptor(A, "comp")!;
console.log(d.writable, d.enumerable, d.configurable, d.value === A.comp);
console.log(A.comp === A.comp, Object.keys(A).length);
class B extends A {}
console.log(B.comp === A.comp, B.comp(2), Object.hasOwn(B, "comp"));
(A as any).comp = function (x: number) {
  return -x;
};
console.log(B.comp(2), A.comp(3));
delete (A as any).comp;
console.log(typeof (A as any).comp, "comp" in A, "comp" in B);
class W {
  static [w]() {
    return this === W ? "W" : this === V ? "V" : "other";
  }
}
class V extends W {}
console.log((W as any).who(), (V as any).who());
const f = (W as any).who;
console.log(f.call(V), String(f).startsWith("[w]") || String(f).includes("return this"));

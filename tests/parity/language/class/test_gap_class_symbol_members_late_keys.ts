// A born prototype with symbol members, then keys added at run time: node order is
// integer keys ascending, then strings in insertion order, then symbols in insertion order.
const tag = Symbol.for("thr1.late");
const extra = Symbol("extra");
class C {
  a() { return 1; }
  *[Symbol.iterator]() { yield 7; }
  [tag]() { return "t"; }
  b() { return 2; }
}
const show = (o: object) => Reflect.ownKeys(o).map(String).join(",");
console.log(show(C.prototype));
(C.prototype as any).late = function late() { return 3; };
console.log(show(C.prototype));
(C.prototype as any)[extra] = 4;
(C.prototype as any).later = 5;
(C.prototype as any)[2] = "two";
console.log(show(C.prototype));
console.log(Object.getOwnPropertySymbols(C.prototype).map(String).join(","));
console.log(Object.getOwnPropertyNames(C.prototype).join(","));
delete (C.prototype as any)[Symbol.iterator];
(C.prototype as any)[Symbol.iterator] = function* () { yield 9; };
console.log(show(C.prototype));
const c = new C();
console.log([...c].join("+"), (c as any)[tag](), (c as any).late(), (c as any)[extra], (c as any).later);
class D extends C { [Symbol.toPrimitive]() { return 42; } m() { return 1; } }
(D.prototype as any).z = 1;
console.log(show(D.prototype), +new D());

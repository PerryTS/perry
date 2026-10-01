// Static class accessors are accessor properties of the class constructor:
// reflection, attributes, delete, inheritance and [[Set]] all see one property.
class A {
  static count = 0;
  static get x() { return "x:" + (this as any).name; }
  static set x(v: string) { A.count++; }
  static get ro() { return 1; }
}
class B extends A {}
console.log(A.x, B.x, (B as any).ro);
const d = Object.getOwnPropertyDescriptor(A, "x")!;
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable);
console.log(d.get === Object.getOwnPropertyDescriptor(A, "x")!.get);
console.log(Object.getOwnPropertyNames(A).join(","), Object.keys(A).join(","));
(A as any).x = "v";
(B as any).x = "w";
console.log(A.count);
// #11521: a getter-only static refuses the write.
try {
  (A as any).ro = 5;
  console.log("accepted", (A as any).ro);
} catch (e) {
  console.log("TypeError", e instanceof TypeError, (A as any).ro);
}
try {
  (B as any).ro = 5;
  console.log("accepted", (B as any).ro);
} catch (e) {
  console.log("TypeError", e instanceof TypeError);
}
Object.defineProperty(A, "x", { enumerable: true });
console.log(Object.keys(A).join(","), A.propertyIsEnumerable("x"), A.x);
Object.defineProperty(A, "dyn", { get() { return "dyn"; }, set(v) {}, configurable: true, enumerable: false });
console.log((A as any).dyn, (B as any).dyn, Object.getOwnPropertyNames(A).includes("dyn"), "dyn" in B);
console.log(delete (A as any).dyn, (A as any).dyn, "dyn" in A);
console.log(delete (A as any).x, A.x, B.x, "x" in A, Object.getOwnPropertyDescriptor(A, "x"));
class C {
  static get v() { return 1; }
}
Object.defineProperty(C, "v", { configurable: false });
console.log(Reflect.deleteProperty(C, "v"), C.v, Object.getOwnPropertyDescriptor(C, "v")!.configurable);
Object.defineProperty(C, "w", { value: 7, configurable: true });
Object.defineProperty(C, "w", { get() { return 8; } });
console.log((C as any).w, typeof Object.getOwnPropertyDescriptor(C, "w")!.get);

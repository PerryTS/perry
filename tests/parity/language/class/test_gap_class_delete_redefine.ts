// delete is a real delete, and a later definition brings the key back
class A {
  static s() { return "A.s"; }
  static f = 1;
  m() { return "A.m"; }
}
class B extends A {
  static s() { return "B.s"; }
  m() { return "B.m"; }
}
const b = new B();
console.log(B.s(), b.m());
delete (B as any).s;
console.log(B.s(), Object.prototype.hasOwnProperty.call(B, "s"), Object.getOwnPropertyNames(B).join(","));
delete (B.prototype as any).m;
console.log(b.m(), Object.prototype.hasOwnProperty.call(B.prototype, "m"), "m" in b);
(B as any).s = function () { return "B.s2"; };
console.log(B.s(), Object.getOwnPropertyDescriptor(B, "s")!.enumerable);
(B.prototype as any).m = function () { return "B.m2"; };
console.log(b.m(), Object.prototype.hasOwnProperty.call(B.prototype, "m"));
Object.defineProperty(B, "s", { value: () => "B.s3", writable: false, enumerable: false, configurable: true });
console.log(B.s(), JSON.stringify(Object.getOwnPropertyDescriptor(B, "s")!.writable));
// A static store of the same name never resurrects a deleted prototype member
class C { k() { return "C.k"; } }
const c = new C();
delete (C.prototype as any).k;
(C as any).k = 5;
console.log(typeof (c as any).k, (C as any).k, "k" in c);
// intrinsic name/length: delete, then define again
class D { constructor(a: number, b: number) {} }
delete (D as any).name;
console.log(Object.prototype.hasOwnProperty.call(D, "name"), JSON.stringify(D.name));
Object.defineProperty(D, "name", { value: "Dee" });
console.log(D.name, D.length);
delete (D as any).length;
console.log(D.length, Object.getOwnPropertyNames(D).join(","));
// static field delete then re-store
delete (A as any).f;
console.log((A as any).f, (B as any).f, Object.prototype.hasOwnProperty.call(A, "f"));
(A as any).f = 7;
console.log((A as any).f, (B as any).f);
// delete the parent's static: the child now misses it too
delete (A as any).s;
console.log(typeof (B as any).s, typeof (A as any).s);

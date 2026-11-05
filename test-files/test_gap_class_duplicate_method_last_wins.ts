class A {
  b() { return "first"; }
  c() { return "c"; }
  b() { return "last"; }
  get g() { return 1; }
  get g() { return 2; }
}
const a = new A();
console.log(a.b(), a.g);
console.log(Object.getOwnPropertyNames(A.prototype).join(","));
const f: any = a;
console.log(f["b"](), (A.prototype as any).b.call(a));
class B extends A { m() { return super.b(); } }
console.log(new B().m());

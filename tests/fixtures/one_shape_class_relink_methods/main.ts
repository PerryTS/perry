// `Object.setPrototypeOf(C.prototype, X)` replaces the chain a class instance
// inherits through. A method, getter or setter DECLARED in C's old parent class
// body is then off the chain: it must not be read, found by `in`, or called.
// C's own declared members stay, X's members (including another class's
// declared methods) become visible, `super.m()` in C's methods follows the
// relink (the home object's [[Prototype]] is the new link), and static members
// (C's constructor still inherits from B) are unaffected.
//
// Class names are unique per block: a block-scoped duplicate class name is a
// separate bug, and this file must not depend on it.
//
// Each line is `name value`, compared with node. Same-site cases relink
// mid-loop at a parameter receiver with an argv trip count (a fixed small loop
// is unrolled and a literal receiver refined, so neither would reach the site).
const N = Number(process.argv[2] ?? "40");
const H = N >> 1;
function show(name: string, v: any): void {
  console.log(name + " " + String(v));
}
function tryCall(f: () => any): string {
  try {
    return "ok:" + String(f());
  } catch (e) {
    return (e instanceof TypeError) ? "TypeError" : "other:" + String(e);
  }
}
function readM(o: any): any { return typeof o.m; }
function callM(o: any): string { return tryCall(() => o.m()); }
function hasM(o: any): boolean { return "m" in o; }
function loopRead(o: any, n: number, at: number, relink: () => void): string {
  let before: any = undefined;
  let after: any = undefined;
  for (let i = 0; i < n; i++) {
    if (i === at) relink();
    const v = typeof o.m;
    if (i < at) before = v; else after = v;
  }
  return String(before) + "/" + String(after);
}
function loopCall(o: any, n: number, at: number, relink: () => void): string {
  let before: any = undefined;
  let after: any = undefined;
  for (let i = 0; i < n; i++) {
    if (i === at) relink();
    let v: any;
    try { v = o.m(); } catch (e) { v = (e instanceof TypeError) ? "TypeError" : "other"; }
    if (i < at) before = v; else after = v;
  }
  return String(before) + "/" + String(after);
}
function loopIn(o: any, n: number, at: number, relink: () => void): string {
  let before: any = undefined;
  let after: any = undefined;
  for (let i = 0; i < n; i++) {
    if (i === at) relink();
    const v = "m" in o;
    if (i < at) before = v; else after = v;
  }
  return String(before) + "/" + String(after);
}

// 1. unprimed: the old parent's declared method is gone
{
  class B1 { m() { return 1; } }
  class C1 extends B1 {}
  const inst: any = new C1();
  Object.setPrototypeOf(C1.prototype, { k: 7 });
  show("unprimed-typeof", typeof inst.m);
  show("unprimed-in", "m" in inst);
  show("unprimed-call", tryCall(() => inst.m()));
  show("unprimed-k", inst.k);
}
// 2. primed at other sites before the relink
{
  class B2 { m() { return 2; } }
  class C2 extends B2 {}
  const inst: any = new C2();
  show("primed-before-typeof", typeof inst.m);
  show("primed-before-call", tryCall(() => inst.m()));
  show("primed-before-fn", readM(inst) + "," + callM(inst) + "," + hasM(inst));
  Object.setPrototypeOf(C2.prototype, { k: 7 });
  show("primed-after-typeof", typeof inst.m);
  show("primed-after-in", "m" in inst);
  show("primed-after-call", tryCall(() => inst.m()));
  show("primed-after-fn", readM(inst) + "," + callM(inst) + "," + hasM(inst));
  show("primed-new-instance", readM(new C2()) + "," + callM(new C2()));
}
// 3. same site, relink mid-loop
{
  class B3 { m() { return 3; } }
  class C3a extends B3 {}
  class C3b extends B3 {}
  class C3c extends B3 {}
  show("same-site-read", loopRead(new C3a(), N, H, () => Object.setPrototypeOf(C3a.prototype, { k: 7 })));
  show("same-site-call", loopCall(new C3b(), N, H, () => Object.setPrototypeOf(C3b.prototype, { k: 7 })));
  show("same-site-in", loopIn(new C3c(), N, H, () => Object.setPrototypeOf(C3c.prototype, { k: 7 })));
}
// 4. methods declared on C4 itself stay visible, and `this` is the instance
{
  class B4 { m() { return 4; } }
  class C4 extends B4 { own() { return "own" + this.v; } v = 1; }
  const inst: any = new C4();
  show("own-before", inst.own());
  Object.setPrototypeOf(C4.prototype, { k: 7 });
  show("own-after", inst.own() + "," + typeof inst.own + "," + ("own" in inst));
  show("own-inherited-gone", typeof inst.m);
}
// 5. relink to null: the old parent and Object.prototype are both off the chain
{
  class B5 { m() { return 5; } }
  class C5 extends B5 { own() { return "own5"; } }
  const inst: any = new C5();
  show("null-before", typeof inst.m + "," + typeof inst.toString);
  Object.setPrototypeOf(C5.prototype, null);
  show("null-after", typeof inst.m + "," + ("m" in inst) + "," + typeof inst.toString + "," + ("toString" in inst));
  show("null-call", tryCall(() => inst.m()));
  show("null-own", inst.own());
}
// 6. relink onto another class's prototype: that class's methods appear
{
  class B6 { m() { return "B6"; } only() { return "onlyB"; } }
  class D6 { m() { return "D6:" + this.tag; } dOnly() { return "dOnly"; } }
  class C6 extends B6 { tag = "t"; }
  const inst: any = new C6();
  show("to-class-before", inst.m() + "," + typeof inst.dOnly);
  Object.setPrototypeOf(C6.prototype, D6.prototype);
  show("to-class-call", tryCall(() => inst.m()));
  show("to-class-dOnly", tryCall(() => inst.dOnly()));
  show("to-class-only-gone", typeof inst.only + "," + ("only" in inst));
  show("to-class-fn", readM(inst) + "," + callM(inst) + "," + hasM(inst));
  show("to-class-identity", inst.m === D6.prototype.m);
}
// 7. getters and setters declared in the old parent
{
  class B7 {
    get g() { return "getB"; }
    set s(v: any) { (this as any).fromSetter = v; }
  }
  class C7 extends B7 {}
  const inst: any = new C7();
  show("accessor-before", inst.g + "," + ("g" in inst) + "," + ("s" in inst));
  Object.setPrototypeOf(C7.prototype, { k: 7 });
  show("accessor-getter", String(inst.g) + "," + ("g" in inst));
  inst.s = 9;
  show("accessor-setter", String(inst.fromSetter) + "," + Object.prototype.hasOwnProperty.call(inst, "s") + "," + inst.s);
}
// 8. super.m() in C8's own methods follows the home object's new [[Prototype]]
{
  class B8 { m() { return "B8"; } }
  class D8 { m() { return "D8"; } }
  class C8 extends B8 {
    m() { return "C8>" + super.m(); }
  }
  const inst: any = new C8();
  show("super-before", inst.m());
  Object.setPrototypeOf(C8.prototype, D8.prototype);
  show("super-to-class", tryCall(() => inst.m()));
  Object.setPrototypeOf(C8.prototype, { k: 7 });
  show("super-to-plain", tryCall(() => inst.m()));
}
// 9. static members: C9's constructor still inherits from B9
{
  class B9 { static sm() { return "sB"; } m() { return 9; } }
  class C9 extends B9 {}
  Object.setPrototypeOf(C9.prototype, { k: 7 });
  show("static-call", tryCall(() => (C9 as any).sm()));
  show("static-in", ("sm" in C9) + "," + typeof (C9 as any).sm);
  show("static-proto-of-C9", Object.getPrototypeOf(C9) === B9);
}
// 10. deeper: an intermediate class prototype relinked
{
  class A10 { m() { return "A10"; } a() { return "a"; } }
  class B10 extends A10 { b() { return "b"; } }
  class C10 extends B10 {}
  const inst: any = new C10();
  show("deep-before", inst.m() + "," + inst.b() + "," + inst.a());
  Object.setPrototypeOf(B10.prototype, { m() { return "X10"; } });
  show("deep-after", tryCall(() => inst.m()) + "," + tryCall(() => inst.b()) + "," + typeof inst.a + "," + ("a" in inst));
}
// 11. relinking back to the old parent restores the method
{
  class B11 { m() { return "B11"; } }
  class C11 extends B11 {}
  const inst: any = new C11();
  Object.setPrototypeOf(C11.prototype, { k: 7 });
  show("back-gone", typeof inst.m);
  Object.setPrototypeOf(C11.prototype, B11.prototype);
  show("back-restored", tryCall(() => inst.m()) + "," + ("m" in inst));
}
// 12. reflection on the relinked chain
{
  class B12 { m() { return 12; } }
  class C12 extends B12 { own() { return 1; } }
  const inst: any = new C12();
  Object.setPrototypeOf(C12.prototype, { k: 7 });
  show("reflect-get", typeof Reflect.get(inst, "m"));
  show("reflect-has", Reflect.has(inst, "m"));
  show("own-names-proto", Object.getOwnPropertyNames(C12.prototype).join("|"));
  const keys: string[] = [];
  for (const k in inst) keys.push(k);
  show("for-in", keys.join("|"));
}
// 13. statically typed receivers (a class-typed parameter, a subclass, `this`)
class B13 { m(): number { return 13; } get g(): string { return "g13"; } }
class C13 extends B13 { own(): number { return this.m(); } }
class E13 extends C13 {}
function viaTyped13(c: C13): string { return tryCall(() => c.m()); }
function viaBase13(b: B13): string { return tryCall(() => b.m()); }
{
  const c = new C13();
  const e = new E13();
  show("typed-before", viaTyped13(c) + "," + viaBase13(c) + "," + viaTyped13(e) + "," + tryCall(() => c.own()) + "," + c.g);
  Object.setPrototypeOf(C13.prototype, { k: 7 });
  show("typed-after", tryCall(() => c.m()) + "," + viaTyped13(c) + "," + viaBase13(c) + "," + typeof c.m + "," + ("m" in c));
  show("typed-subclass", viaTyped13(e) + "," + tryCall(() => e.m()) + "," + typeof e.m);
  show("typed-this-call", tryCall(() => c.own()));
  show("typed-getter", String(c.g) + "," + String(e.g));
}

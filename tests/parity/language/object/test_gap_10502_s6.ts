// Refs #10502, DESIGN.md S6: prototype writes belong to the actual holder.
function F(this: any, v: number) { this.v = v; }
(F as any).prototype.get = function (this: any) { return this.v; };
const first: any = new (F as any)(5);
const original: any = F.prototype;
console.log("function before", first.get(), original.get.call(first), F.prototype.get.call(first));
(F as any).prototype = { get() { return -1; }, tag: "new" };
const second: any = new (F as any)(7);
const computed = ["g", "et"].join("");
console.log("function replacement", first[computed](), second[computed](), second.tag, F.prototype.get.call(second));
(F as any).prototype.get = function () { return 3; };
original.get = function () { return 4; };
console.log("function patch", first.get(), second.get(), original.get.call(first));
delete (F as any).prototype.get;
console.log("function delete", typeof second.get, first.get());

// Ordinary writes also retire current main's direct receiver guards when
// the holder's method has no ConstFn lane, or adds an inherited override.
class Wide {
  a00() {} a01() {} a02() {} a03() {} a04() {} a05() {} a06() {} a07() {}
  a08() {} a09() {} a10() {} a11() {} a12() {} a13() {} a14() {} a15() {}
  a16() {} a17() {} a18() {} a19() {} a20() {} a21() {} a22() {} a23() {}
  a24() {} a25() {} a26() {} a27() {} a28() {} a29() {} a30() {} a31() {}
  tail() { return 50; }
}
const wide = new Wide();
function wideCall(o: Wide) { return o.tail(); }
console.log("wide before", wideCall(wide));
// Prime the same store site on an ordinary object with the same members.
// A prototype holder must retain its own mutation notification path.
const twin = Object.create(Object.getPrototypeOf(Wide.prototype));
Object.defineProperties(twin, Object.getOwnPropertyDescriptors(Wide.prototype));
function patchTail(o: any, value: any) { o.tail = value; }
patchTail(twin, function () { return 51; });
patchTail(Wide.prototype, function () { return 60; });
console.log("wide patch", wideCall(wide));
class BaseExtra { extra() { return 70; } }
class Shadow extends BaseExtra {}
const shadow = new Shadow();
function shadowCall(o: Shadow) { return o.extra(); }
console.log("shadow before", shadowCall(shadow));
Shadow.prototype.extra = function () { return 80; };
console.log("shadow patch", shadowCall(shadow));

class Parent { m() { return 1; } }
class X extends Parent {
  m() { return 10; }
  static m() { return 100; }
}
class Child extends X { viaSuper() { return super.m(); } }
const x: any = new X();
const child = new Child();
function mono(o: X) { return o.m(); }
function mega(o: any) { return o.m(); }
class A0 extends X { a0 = 0; }
class A1 extends X { a1 = 1; }
class A2 extends X { a2 = 2; }
class A3 extends X { a3 = 3; }
class A4 extends X { a4 = 4; }
class A5 extends X { a5 = 5; }
class A6 extends X { a6 = 6; }
class A7 extends X { a7 = 7; }
class A8 extends X { a8 = 8; }
class A9 extends X { a9 = 9; }
const many = [new A0(), new A1(), new A2(), new A3(), new A4(), new A5(), new A6(), new A7(), new A8(), new A9()];
function sumMega() { let n = 0; for (const o of many) n += mega(o); return n; }
const before = x.m;
console.log("class before", mono(x), sumMega(), child.viaSuper(), before.call(x), X.m());
X.prototype.m = function () { return 20; };
const after = x.m;
console.log("class patch", mono(x), sumMega(), child.viaSuper(), before.call(x), after.call(x), before === after);
console.log("method identity", after === X.prototype.m, after === child.m);
Object.defineProperty(X.prototype, "m", { value: function () { return 30; }, writable: true, configurable: true });
console.log("define", mono(x), sumMega(), child.viaSuper(), after.call(x), x.m.call(x));
let getterCalls = 0;
Object.defineProperty(X.prototype, "m", {
  get() { getterCalls++; return function () { return 40; }; },
  configurable: true
});
console.log("getter", mono(x), sumMega(), child.viaSuper(), x.m.call(x), getterCalls);
delete (X.prototype as any).m;
console.log("delete inherited", mono(x), sumMega(), child.viaSuper(), x.m.call(x), Object.hasOwn(X.prototype, "m"));
const oldStatic = X.m;
X.m = function () { return 200; };
console.log("static patch", oldStatic(), X.m(), (X as any)[computed.replace("get", "m")]());

// Class dispatch with a prototype method replaced IN PLACE after the site
// first ran. The program writes the method name, so the write census proves
// it no constant (object model step 3); every build must print node's output.
class A { n = 2; step(x: number) { return this.n + x; } }
class B extends A { step(x: number) { return this.n * x; } }
class C extends A {}
class D extends C {}
function at(o: any) { return o.step(3); }
const xs: any[] = [new A(), new B(), new C(), new D()];
for (let r = 0; r < 3; r++) for (const o of xs) console.log("warm", r, at(o));
// The holder is the receivers' grandparent for D: one hop plus the holder.
(A.prototype as any).step = function (this: any, x: number) { return 100 + this.n + x; };
for (const o of xs) console.log("replaced A", at(o));
// Replaced repeatedly from one store site (a store PIC must not learn it).
for (let k = 0; k < 4; k++) {
  (B.prototype as any).step = function (this: any, x: number) { return k * 1000 + x; };
  console.log("replaced B", k, at(xs[1]));
}
// The method read happens before the arguments: an argument that replaces
// the method must not change which body runs for this call.
function mutate() {
  (C.prototype as any).step = function (x: number) { return -x; };
  return 7;
}
function atArg(o: any) { return o.step(mutate()); }
console.log("read before args", atArg(xs[2]));
console.log("after args", at(xs[2]), at(xs[3]));
// Restored to a class body: compiled again.
delete (C.prototype as any).step;
for (const o of xs) console.log("deleted C", at(o));

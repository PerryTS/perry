// A by-name class method call repeats from its site's chain memo
// (receiver word, every prototype hop's word, the holder's slot). Every
// change to an object the memo names must be seen on the next call.

class A { m(x: number): string { return "A" + x; } n(x: number): string { return "nA" + x; } }
class B extends A {}
class C extends B {}
class D extends C {}

function computed(o: any, k: string, x: number): string { return o[k](x); }
function named(o: any, x: number): string { return o.m(x); }

const keyM = ["m"].join("");
const keyN = ["n"].join("");
const out: string[] = [];
const d = new D();

// 1. computed key, holder three hops up: record, then repeat.
for (let i = 0; i < 4; i++) out.push(computed(d, keyM, i));
// 2. an intermediate prototype gains the name: its word changes.
(B.prototype as any).m = function (x: number) { return "B" + x; };
for (let i = 0; i < 3; i++) out.push(computed(d, keyM, i));
// 3. ... and loses it again.
delete (B.prototype as any).m;
for (let i = 0; i < 3; i++) out.push(computed(d, keyM, i));
// 4. the holder's value is overwritten in place.
(A.prototype as any).m = function (x: number) { return "A2:" + x; };
for (let i = 0; i < 3; i++) out.push(computed(d, keyM, i));
// 5. two keys through one site.
for (let i = 0; i < 4; i++) out.push(computed(d, i & 1 ? keyN : keyM, i));
// 6. a prototype hop is relinked.
class X { m(x: number): string { return "X" + x; } }
Object.setPrototypeOf(C.prototype, X.prototype);
for (let i = 0; i < 3; i++) out.push(computed(d, keyM, i));
// 7. the receiver gets an own property.
const d2: any = new D();
for (let i = 0; i < 3; i++) out.push(computed(d2, keyM, i));
d2.m = function (x: number) { return "own" + x; };
for (let i = 0; i < 3; i++) out.push(computed(d2, keyM, i));

// 8. a fixed-name call on an untyped receiver, deep holder.
class P { m(x: number): string { return "P" + x; } }
class Q extends P {}
class R extends Q {}
class S extends R {}
const s: any = new S();
for (let i = 0; i < 4; i++) out.push(named(s, i));
(R.prototype as any).m = function (x: number) { return "R" + x; };
for (let i = 0; i < 3; i++) out.push(named(s, i));
(Q.prototype as any).m = function (x: number) { return "Q" + x; };
delete (R.prototype as any).m;
for (let i = 0; i < 3; i++) out.push(named(s, i));

// 9. many receiver classes through one site.
class K0 { k(): number { return 0; } }
class K1 extends K0 { k(): number { return 1; } }
class K2 extends K0 {} class K3 extends K1 {} class K4 extends K3 {}
const ks: any[] = [new K0(), new K1(), new K2(), new K3(), new K4()];
let sum = 0;
for (let i = 0; i < 200; i++) sum += ks[i % 5][["k"].join("")]();
out.push("sum=" + sum);
(K1.prototype as any).k = function () { return 10; };
sum = 0;
for (let i = 0; i < 200; i++) sum += ks[i % 5][["k"].join("")]();
out.push("sum2=" + sum);

// 10. a direct class-method site whose name another prototype assigns
// (the compiled arm misses on every call: its miss edge repeats from the memo).
function Fn(this: any) {}
Fn.prototype.plus = function (y: number) { return y; };
class Dec { v: number; constructor(v: number) { this.v = v; } plus(y: number): Dec { return new Dec(this.v + y); } }
class Dec2 extends Dec {}
let x: Dec = new Dec2(1);
for (let i = 0; i < 50; i++) x = x.plus(i);
out.push("dec=" + x.v);
(Dec.prototype as any).plus = function (this: Dec, y: number) { return new Dec(this.v - y); };
let z: Dec = new Dec2(1000);
for (let i = 0; i < 5; i++) z = z.plus(i);
out.push("dec2=" + z.v + " " + (new (Fn as any)()).plus(3));

// 11. garbage between calls: the memo's prototypes may move.
let junk: any[] = [];
for (let r = 0; r < 30; r++) {
  for (let j = 0; j < 2000; j++) junk.push({ a: j, b: [j], c: "s" + j });
  if (junk.length > 20000) junk = [];
  out.push(computed(d, keyM, r) + named(s, r));
}
console.log(out.join("\n"));

// A proof about an object's property types is taken at the binding (an
// object literal's construction, the argument an inlined call binds). It may
// be kept only while nothing can write the object: a `delete`, a store of
// another type, an alias, a callee, a closure or a builtin that writes it
// changes the property between the proof and the read. Every read below must
// see the property as it is now. The last lines are literals nothing writes:
// their reads stay exact.

const n = { x: 1, y: 2 };
delete (n as any).y;
console.log(n.y * 10, n.y + 1, -n.y, n.y);

const s = { x: 1, y: 2 };
(s as any).y = "s";
console.log(s.y + 1, s.y * 2, typeof s.y);

function r(o: { x: number; y: number; z: number }): number {
  return o.x * 100 + o.y * 10 + o.z;
}
const h = { x: 1, y: 2, z: 3 };
delete (h as any).y;
console.log(r(h));

const q = { x: 1, y: 2, z: 3 };
(q as any).x = "x";
console.log(r(q));

const k = { x: 1, y: 2, z: 3 };
delete (k as any).y;
(k as any).y = 7;
console.log(r(k), Object.keys(k).join());

type V = { x: number; y: number };
const v: V = { x: 1, y: 2 };
(v as any).y = "s";
console.log(v.y + 1, v.y * 2);

const a1 = { x: 1, y: 2 };
const a2 = a1;
(a2 as any).y = "alias";
console.log(a1.y + 1);

function w(o: any) {
  o.y = "callee";
}
const a3 = { x: 1, y: 2 };
w(a3);
console.log(a3.y + 1);

const a4 = { x: 1, y: 2 };
const cl = () => {
  (a4 as any).y = "closure";
};
cl();
console.log(a4.y + 1);

const a5 = { x: 1, y: 2 };
function g5() {
  (a5 as any).y = "module fn";
}
g5();
console.log(a5.y + 1);

const a6 = { x: 1, y: 2 };
const hold = { r: a6 };
(hold.r as any).y = "held";
console.log(a6.y + 1);

const a7 = { x: 1, y: 2 };
Object.assign(a7, { y: "assign" });
console.log(a7.y + 1);

const a8 = { x: 1, y: 2 };
Object.defineProperty(a8, "y", { value: "define" });
console.log(a8.y + 1);

const a9 = { x: 1, y: 2 };
const arr9 = [a9];
(arr9[0] as any).y = "array";
console.log(a9.y + 1);

const n1 = { a: { b: 1 } };
const t1 = n1.a;
(t1 as any).b = "child";
console.log(n1.a.b + 1);

function w2(z: any) {
  z.b = "child callee";
}
const n2 = { a: { b: 1 } };
w2(n2.a);
console.log(n2.a.b + 1);

function f3() {
  const o = { x: 1, y: 2 };
  (o as any).y = "local";
  return o.y + 1;
}
function f4() {
  const o = { x: 1, y: 2 };
  delete (o as any).y;
  return o.y * 10;
}
console.log(f3(), f4());

function f5(p: { x: number; y: number }, other: any) {
  other.y = "other param";
  return p.y + 1;
}
const z5 = { x: 1, y: 2 };
console.log(f5(z5, z5));

function readOnly(o: { x: number; y: number }): number {
  return o.x * 2 + o.y;
}
const u = { a: 1.5, b: 2 };
const ro = { x: 3, y: 4 };
console.log(u.a * u.b, readOnly(ro), ro.x * ro.y);

// #10498: class getters/setters answered from shape facts at the read and
// store sites (receiver ShapeId -> holder, holder ShapeId -> accessor lane,
// the lane's pair -> the compiled getter/setter). Every scenario runs a site
// hot first, then changes one fact the site relies on, and keeps reading.

class Point {
  _x = 0;
  _y = 0;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  get x() { return this._x; }
  set x(v: number) { this._x = v; }
  get y() { return this._y; }
  set y(v: number) { this._y = v * 2; }
}

function sumXY(ps: any[]): number {
  let s = 0;
  for (let i = 0; i < ps.length; i++) s += ps[i].x + ps[i].y;
  return s;
}
function writeX(p: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { p.x = i; s += p._x; }
  return s;
}

const pts: Point[] = [];
for (let i = 0; i < 40; i++) pts.push(new Point(i, i + 1));
console.log("basic", sumXY(pts), writeX(pts[3], 50), pts[3].x, pts[3].y);

// Inherited through two levels, and overridden in a subclass.
class A {
  _v = 1;
  get v() { return this._v * 10; }
  set v(n: number) { this._v = n; }
}
class B extends A { tag = "b"; }
class C extends B { more = 3; }
class D extends B {
  get v() { return -this._v; }
  set v(n: number) { this._v = n + 100; }
}
function readV(o: any): number { return o.v; }
function writeV(o: any, n: number): void { o.v = n; }
const c = new C();
const d = new D();
let acc = 0;
for (let i = 0; i < 60; i++) {
  writeV(c, i);
  acc += readV(c);
}
console.log("two-levels", acc, c.v, (c as any)._v);
acc = 0;
for (let i = 0; i < 60; i++) {
  const o: any = i % 3 === 0 ? d : c;
  writeV(o, i);
  acc += readV(o);
}
console.log("override", acc, d.v, (d as any)._v, c.v);

// Redefined at runtime on the prototype: hot sites must see the new pair.
class R {
  _n = 5;
  get n() { return this._n; }
  set n(v: number) { this._n = v; }
}
const r = new R();
function readN(o: any): number { return o.n; }
function writeN(o: any, v: number): void { o.n = v; }
let before = 0;
for (let i = 0; i < 50; i++) { writeN(r, i); before += readN(r); }
Object.defineProperty(R.prototype, "n", {
  get() { return this._n + 1000; },
  set(v: number) { this._n = v * 3; },
  configurable: true,
});
let after = 0;
for (let i = 0; i < 50; i++) { writeN(r, i); after += readN(r); }
console.log("redefine", before, after, r.n, (r as any)._n);
// Getter-only replacement: the store has no setter now (sloppy: ignored).
Object.defineProperty(R.prototype, "n", { get() { return 7; }, configurable: true });
writeN(r, 99);
console.log("getter-only-redefine", readN(r), (r as any)._n);
// Back to a data property on the prototype.
Object.defineProperty(R.prototype, "n", { value: 42, writable: true, configurable: true });
console.log("data-redefine", readN(r), Object.prototype.hasOwnProperty.call(r, "n"));
writeN(r, 8);
console.log("own-after-data", readN(r), Object.prototype.hasOwnProperty.call(r, "n"));
// Deleted from the prototype: reads fall to undefined.
class Del { get g() { return 1; } }
const del = new Del();
function readG(o: any): any { return o.g; }
let gs = 0;
for (let i = 0; i < 30; i++) gs += readG(del);
delete (Del.prototype as any).g;
console.log("delete", gs, readG(del));

// An own property shadowing the accessor on one instance.
const sh1 = new Point(1, 2);
const sh2 = new Point(3, 4);
Object.defineProperty(sh2, "x", { value: 77, writable: true });
let shs = 0;
for (let i = 0; i < 30; i++) shs += (i % 2 ? sh1 : sh2).x;
console.log("shadow", shs);

// setPrototypeOf on an instance moves it off the class.
const moved = new Point(5, 6);
let ms = 0;
for (let i = 0; i < 20; i++) ms += readX(moved);
Object.setPrototypeOf(moved, { get x() { return -1; } });
for (let i = 0; i < 20; i++) ms += readX(moved);
function readX(o: any): number { return o.x; }
console.log("setPrototypeOf", ms);

// Static accessors.
class S {
  static _count = 0;
  static get count() { return S._count; }
  static set count(v: number) { S._count = v + 1; }
}
let ss = 0;
for (let i = 0; i < 30; i++) { S.count = i; ss += S.count; }
console.log("static", ss, S.count);

// Accessors on object literals, several objects of one shape.
function mk(k: number) {
  return {
    _k: k,
    get k2() { return this._k * 2; },
    set k2(v: number) { this._k = v; },
  };
}
const lits = [mk(1), mk(2), mk(3)];
let ls = 0;
for (let i = 0; i < 30; i++) {
  const o: any = lits[i % 3];
  ls += o.k2;
  o.k2 = i;
}
console.log("literal", ls, lits.map((o) => o.k2).join(","));

// Setter-only and getter-only class accessors.
class OnlyOne {
  _w = 0;
  set w(v: number) { this._w = v; }
  get r() { return this._w + 1; }
}
const oo: any = new OnlyOne();
let os = 0;
let refused = 0;
for (let i = 0; i < 30; i++) {
  oo.w = i;
  os += oo.r;
  os += oo.w === undefined ? 1 : 0;
  // Module code is strict: a store to a getter-only accessor throws.
  try { oo.r = 5; } catch (e) { refused += (e as Error) instanceof TypeError ? 1 : 0; }
}
console.log("one-sided", os, refused, oo._w, oo.r);

// Non-Number values through a setter (strings and objects) and back.
class Box {
  _v: any = null;
  get v() { return this._v; }
  set v(x: any) { this._v = x; }
}
const bx: any = new Box();
const kept: any[] = [];
for (let i = 0; i < 40; i++) {
  bx.v = i % 2 ? "s" + i : { i };
  kept.push(bx.v);
}
const lastAssign = (bx.v = { done: true });
console.log("values", kept.length, JSON.stringify(kept[38]), kept[39], JSON.stringify(lastAssign), JSON.stringify(bx.v));

// A throwing getter and setter, from hot sites inside try/catch.
class T {
  _t = 0;
  get t() { if (this._t > 25) throw new Error("get " + this._t); return this._t; }
  set t(v: number) { if (v === 30) throw new Error("set " + v); this._t = v; }
}
const tt: any = new T();
let caught = "";
let ts = 0;
for (let i = 0; i < 35; i++) {
  try { tt.t = i; ts += tt.t; } catch (e) { caught += (e as Error).message + ";"; }
}
console.log("throws", ts, caught);

// The getter sees the original receiver as `this`.
class Self { get me() { return this; } }
class Sub2 extends Self { id = 9; }
const s2 = new Sub2();
let same = 0;
for (let i = 0; i < 20; i++) if ((s2 as any).me === s2) same++;
console.log("this", same, (s2 as any).me.id);

// Values allocated inside getters, many collections while sites are hot.
class Alloc {
  n = 0;
  get fresh() { return { n: this.n++, pad: [1, 2, 3, 4, 5, 6, 7, 8] }; }
  set slot(v: any) { this.n += v.pad.length; }
}
const al: any = new Alloc();
let as = 0;
for (let i = 0; i < 20000; i++) {
  const f = al.fresh;
  as += f.pad[i & 7];
  al.slot = f;
}
console.log("alloc", as, al.n);

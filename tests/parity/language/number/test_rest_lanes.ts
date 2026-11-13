// I32 at rest, PR C: a class field whose representation is I32 (int32
// Numbers with int demand) is an I32 lane of its class's shape: its slot
// holds an I32 word, read and written by the proven sites as an i32 and by
// every other site through the runtime's layout accessor.
// Covered: a class born keyless (its field definitions, then the lanes), an
// object-literal class born with its lanes, copies between the two, `++`/`--`
// and compound stores, values >= 4096 with the low bits clear kept across
// collections, int32 extremes, a site that sees an admitted instance and a
// plain object (the complete read and store), and a constant-key store.
// Negative controls that must stay double (or stay off the lane): a field
// that may hold -0 observed by `Object.is`, one holding 2^31, a fraction, an
// instance held by a global and converted by `String` (a protocol touch), one
// written under a dynamic key, ones read by `Object.keys`, `JSON.stringify`
// and spread, one read through a prototype chain, and a class with a
// subclass.

function churn(n: number): number {
  // Allocation pressure, so collections run between the writes and reads.
  let keep: { v: number }[] = [];
  let s = 0;
  for (let i = 0; i < n; i++) {
    keep.push({ v: i });
    if (keep.length > 64) keep = [];
    s = (s + keep.length) | 0;
  }
  return s;
}

interface Snap {
  a: number;
  b: number;
  king: number;
}

class Hash {
  a = 0;
  b = 0;
  king = -1;
  count = 0;
  label = "h";
  mix(k: number): void {
    this.a = (this.a ^ Math.imul(k, 2654435761 | 0)) | 0;
    this.b = (this.b + k * 4096) & 0x7ffff000;
    this.king = (this.king + 1) & 127;
    this.count++;
  }
  unmix(): void {
    this.count--;
    this.b ^= 0x1000;
  }
  snapshot(): Snap {
    return { a: this.a, b: this.b, king: this.king };
  }
  restore(s: Snap): void {
    this.a = s.a;
    this.b = s.b;
    this.king = s.king;
  }
}

const h = new Hash();
const snaps: Snap[] = [];
for (let i = 0; i < 300; i++) {
  h.mix(i);
  if ((i & 7) === 0) snaps.push(h.snapshot());
  if ((i & 31) === 0) churn(200);
}
h.unmix();
console.log("hash", h.a, h.b, h.king, h.count, h.label, snaps.length);
let back = 0;
for (const s of snaps) {
  h.restore(s);
  back = (back + (h.a ^ h.b) + h.king) | 0;
}
console.log("restore", back, h.a, h.b, h.king);
const last = snaps[snaps.length - 1];
console.log("snap", last.a, last.b, last.king, last.a >>> 0, last.b | 0);

// int32 extremes.
class Edge {
  lo = 0;
  hi = 0;
  set(i: number): void {
    this.lo = i === 0 ? -2147483648 : -1;
    this.hi = i === 0 ? 2147483647 : 1;
  }
}
const ed = new Edge();
ed.set(0);
console.log("edge", ed.lo, ed.hi, ed.lo | 0, (ed.hi + 1) | 0);
ed.set(1);
console.log("edge", ed.lo, ed.hi);

// A site that sees an admitted instance and a plain object: the complete
// read and store through the accessor.
function readA(o: any): any {
  return o.a;
}
function writeA(o: any, v: any): void {
  o.a = v;
}
const plain: any = { a: 1.5 };
const other: any = { a: "x" };
const objs: any[] = [h, plain, other];
for (const o of objs) console.log("poly", readA(o));
const plain2: any = { a: 3 };
writeA(h, 41);
writeA(plain2, 7);
console.log("poly-set", h.a, plain2.a, readA(h) + 1);

// A constant-key store (`PutValueSet` shape) inside a method.
class Box {
  v = 0;
  put(k: number): void {
    this["v"] = (k * 3) | 0;
  }
}
const bx = new Box();
bx.put(14);
console.log("putvalue", bx.v);

// Negative controls: each keeps its double (or Any) slot.
const n = process.argv.length > 100 ? 1 : -1;
class Zero {
  v = 0;
  set(x: number): void {
    this.v = x * n;
  }
}
const z = new Zero();
z.set(0);
console.log("negzero", Object.is(z.v, -0), 1 / z.v);
class Wide {
  v = 0;
}
const w = new Wide();
w.v = 2147483647 + (h.count & 1) + 1;
console.log("wide", w.v, w.v | 0);
class Frac {
  v = 0;
}
const f = new Frac();
f.v = (h.count & 3) + 0.5;
console.log("frac", f.v);
// Each control's field is int32 with int demand (`bump`, `^ 1`), so only the
// reason the probe prints keeps it off the lane.
class Escaped {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const es = new Escaped();
es.bump(7);
(globalThis as any).heldInstance = es;
console.log("global", (globalThis as any).heldInstance.v ^ 1, String(es));
class Keyed {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const kd = new Keyed();
kd.bump(2);
const key = process.argv.length > 100 ? "w" : "v";
(kd as any)[key] = 9;
console.log("dynkey", kd.v ^ 1);
class Listed {
  v = 0;
  u = 1;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const ls = new Listed();
ls.bump(5);
console.log("keys", Object.keys(ls).join(","), ls.v ^ 1);
class Printed {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const pr = new Printed();
pr.bump(6);
console.log("json", JSON.stringify(pr), pr.v ^ 1);
class Spread {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const sp = new Spread();
sp.bump(8);
const copy = { ...sp };
console.log("spread", copy.v, sp.v ^ 1);
class Proto {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
const pt = new Proto();
pt.bump(3);
const child = Object.create(pt);
console.log("proto", child.v, pt.v ^ 1);
class Base {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
}
class Derived extends Base {
  u = 2;
}
const bd = new Derived();
bd.bump(4);
const bb = new Base();
bb.bump(5);
console.log("heritage", bd.v ^ 1, bd.u, bb.v ^ 1);
churn(1000);
console.log("after", h.a, h.b, last.a, last.b, ed.lo);

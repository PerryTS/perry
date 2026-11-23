// I32 at rest, push-filled births: an Array born empty by an instance field
// initializer (`[]`, `new Array()`) and filled by `this.k.push(v)` in the
// constructor (straight-line pushes and counted loops) before anything else
// can see it is stored at rest with its final length.
// Covered: the chess Zobrist shape (two tables, one loop, `?? 0` reads with a
// computed key), `new Array()`, straight-line pushes plus two loops, values
// at and past 4096 with clear low bits and negative values across
// collections (young and promoted instances), a table past 16 KiB, an
// instance per round, `length`, a read past the end, a loop over the table.
// Negative controls that must stay ordinary Arrays and match node: a push
// after a read, a value that is no int32, an Array that reaches a generic
// reader (JSON, Object.keys, console.log), a loop with an unbounded trip
// count, a conditional push, a `break` in the fill, a read inside the fill, a
// leak of `this` before the fill, an early `return`, a derived class, a
// constructor that throws in the fill.

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

class Zob {
  private za: number[] = [];
  private zb: number[] = [];
  private hashA = 0;
  private hashB = 0;
  constructor() {
    let seed = 0x6d2b79f5;
    for (let i = 0; i < 13 * 128; i++) {
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      this.za.push(seed);
      seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
      this.zb.push(seed);
    }
  }
  set(square: number, piece: number): void {
    this.hashA ^= this.za[(piece + 6) * 128 + square] ?? 0;
    this.hashB ^= this.zb[(piece + 6) * 128 + square] ?? 0;
  }
  key(): string { return String(this.hashA) + ":" + String(this.hashB); }
  size(): number { return this.za.length + this.zb.length; }
  past(): number | undefined { return this.za[13 * 128]; }
  sum(): number {
    let s = 0;
    for (let i = 0; i < this.za.length; i++) s = (s + (this.za[i] ?? 0)) | 0;
    return s;
  }
}
const z = new Zob();
console.log("zob", z.size(), z.past(), z.sum());
for (let i = 0; i < 2000; i++) z.set(i & 127, (i % 13) - 6);
console.log("zob key", z.key(), churn(20000));
for (let i = 0; i < 2000; i++) z.set((i * 7) & 127, ((i * 5) % 13) - 6);
console.log("zob key 2", z.key(), z.sum());

// `new Array()`, straight-line pushes and two loops; values with clear low
// bits (a raw word would look like a pointer) and negative values.
class Mixed {
  tab: number[] = new Array();
  constructor(base: number) {
    this.tab.push(4096);
    this.tab.push(-8192);
    for (let i = 0; i < 6; i++) { this.tab.push((base + i) * 4096); }
    for (let j = 1; j <= 8; j++) { this.tab.push(-(j * 65536)); }
  }
  at(i: number): number { return this.tab[i & 15] ?? -1; }
  total(): number {
    let s = 0;
    for (let i = 0; i < 16; i++) s += this.at(i);
    return s;
  }
}
const kept: Mixed[] = [];
let acc = 0;
for (let round = 0; round < 300; round++) {
  const m = new Mixed(round);
  if (round % 50 === 0) kept.push(m);
  acc = (acc + m.total() + churn(40)) | 0;
}
console.log("mixed", acc, kept.length, kept.map((m) => m.total()).join(","), kept[2].tab.length);

// A table past 16 KiB (no in-place lengthening of the fresh cell).
class Big {
  private t: number[] = [];
  constructor() { for (let i = 0; i < 5000; i++) { this.t.push((i * 2654435761) | 0); } }
  at(i: number): number { return this.t[i] ?? 0; }
  len(): number { return this.t.length; }
}
const bigs: Big[] = [];
for (let r = 0; r < 20; r++) { bigs.push(new Big()); churn(500); }
let bs = 0;
for (const b of bigs) for (let i = 0; i < 5000; i += 7) bs = (bs + b.at(i)) | 0;
console.log("big", bs, bigs[0].len(), bigs[19].at(4999), bigs[19].at(5000));

// ---- negative controls ----

// A push after a read.
class Late {
  t: number[] = [];
  constructor() { for (let i = 0; i < 8; i++) { this.t.push(i * 3); } }
  at(i: number): number { return this.t[i] ?? -1; }
  add(v: number): void { this.t.push(v | 0); }
}
const late = new Late();
console.log("late", late.at(7), late.at(8));
late.add(99);
for (let i = 0; i < 40; i++) late.add(i);
console.log("late 2", late.at(8), late.at(48), late.t.length);

// A value that is no int32.
class Frac {
  t: number[] = [];
  constructor() { for (let i = 0; i < 8; i++) { this.t.push(i / 4); } this.t.push(2 ** 31); this.t.push(-0); }
  at(i: number): number { return this.t[i] ?? -1; }
}
const fr = new Frac();
console.log("frac", fr.at(1), fr.at(3), fr.at(8), Object.is(fr.at(9), -0), fr.at(10));

// A generic reader.
class Shown {
  t: number[] = [];
  constructor() { for (let i = 0; i < 6; i++) { this.t.push(i * 4096); } }
  at(i: number): number { return this.t[i] ?? -1; }
}
const sh = new Shown();
console.log("shown", JSON.stringify(sh.t), Object.keys(sh.t).join(""), sh.at(5));
console.log(sh.t);
class Keyed {
  t: number[] = [];
  constructor() { for (let i = 0; i < 6; i++) { this.t.push(i - 3); } }
  at(i: number): number { return this.t[i] ?? -1; }
  keys(): string { return Object.keys(this.t).join("") + this.t.join("/"); }
}
console.log("keyed", new Keyed().keys(), new Keyed().at(0));

// An unbounded trip count, a conditional push, a `break`, a written counter.
class Open {
  t: number[] = [];
  u: number[] = [];
  v: number[] = [];
  w: number[] = [];
  constructor(n: number) {
    for (let i = 0; i < n; i++) { this.t.push(i * 2); }
    for (let i = 0; i < 16; i++) { if (i !== 5) this.u.push(i); }
    for (let i = 0; i < 16; i++) { if (i === n) break; this.v.push(i); }
    for (let i = 0; i < 16; i++) { this.w.push(i); i += n & 1; }
  }
  sums(): string {
    let a = 0, b = 0, c = 0, d = 0;
    for (let i = 0; i < 16; i++) {
      a += this.t[i] ?? 100; b += this.u[i] ?? 100; c += this.v[i] ?? 100; d += this.w[i] ?? 100;
    }
    return [a, b, c, d, this.t.length, this.u.length, this.v.length, this.w.length].join(" ");
  }
}
console.log("open", new Open(5).sums(), "|", new Open(20).sums());

// A read inside the fill, a leak of `this` before the fill, an early return.
const seen: Leak[] = [];
class Leak {
  a: number[] = [];
  b: number[] = [];
  c: number[] = [];
  note = 0;
  constructor(stop: boolean) {
    for (let i = 0; i < 8; i++) { this.a.push(i); this.note += this.a.length; }
    seen.push(this);
    for (let i = 0; i < 8; i++) { this.b.push(seen[0].b.length); }
    if (stop) return;
    for (let i = 0; i < 8; i++) { this.c.push(i * i); }
  }
  at(i: number): string { return [this.a[i] ?? -1, this.b[i] ?? -1, this.c[i] ?? -1].join(","); }
}
const l1 = new Leak(false);
const l2 = new Leak(true);
console.log("leak", l1.note, l1.at(7), l2.at(7), l2.c.length, l1.c.length);

// A derived class, and a constructor that throws in the fill.
class Base {
  pre: number[] = [];
  constructor() { for (let i = 0; i < 4; i++) { this.pre.push(i + 1); } }
}
class Derived extends Base {
  own: number[] = [];
  constructor() { super(); for (let i = 0; i < 4; i++) { this.own.push(this.pre[i] ?? 0); } }
  sum(): number { let s = 0; for (let i = 0; i < 4; i++) s += (this.own[i] ?? 0) * (this.pre[i] ?? 0); return s; }
}
console.log("derived", new Derived().sum());
function check(i: number, limit: number): number {
  if (i === limit) throw new Error("stop at " + i);
  return i * 11;
}
class Thrower {
  t: number[] = [];
  constructor(limit: number) { for (let i = 0; i < 8; i++) { this.t.push(check(i, limit)); } }
  at(i: number): number { return this.t[i] ?? -1; }
}
let caught = "";
try { new Thrower(5); } catch (e) { caught = (e as Error).message; }
console.log("thrower", caught, new Thrower(99).at(7), new Thrower(99).at(8));

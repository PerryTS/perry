// I32 at rest, PR B: an Array born by `new Array(k).fill(v)` whose elements
// are int32 Numbers with int demand, whose length never moves and whose
// elements only element sites touch stores each element as an I32 word.
// Covered: a local, a parameter, a class field read through a method call,
// keys at and past the length, `-0`, fractional, NaN and string keys,
// values >= 4096 with the low bits clear kept across collections, negative
// values, and int32 extremes. Negative controls that must stay double: an
// Array a builtin reads (JSON, indexOf, for-of, spread, a non-fused fill),
// one with holes, one that may hold -0, one holding 2^31, and one sharing a
// site with a literal.

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

// A local, read and written by index, its values >= 4096 with the low bits
// clear (no pointer predicate may take one for an address).
const t = new Array<number>(16).fill(0);
for (let i = 0; i < 400; i++) {
  t[i & 15] = ((t[(i + 3) & 15] | 0) + 4096 * (i & 7)) & 0x7ffff000;
  if ((i & 31) === 0) churn(200);
}
let ts = 0;
for (let i = 0; i < 16; i++) ts = (ts + (t[i] | 0)) | 0;
console.log("local", ts, t.length, t[3] | 0, t[15] | 0);

// Keys that are no element.
const k = new Array<number>(4).fill(7);
k[1] = -2147483648;
k[2] = 2147483647;
k[3] = -1;
const keys: any[] = [0, -0, 1, 2, 3, 4, 100, -1, 1.5, NaN, "2", "length"];
for (const key of keys) console.log("key", String(key), k[key]);
console.log("ints", k[1] | 0, k[2] | 0, (k[3] | 0) >>> 0, k[0] & 3);

// A parameter.
function fold(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s = (s ^ (a[i] | 0)) | 0;
  return s;
}
const p = new Array<number>(32).fill(1);
for (let i = 0; i < 32; i++) p[i] = (i * 2654435761) | 0;
console.log("param", fold(p, 32), fold(p, 5));

// A class field read through a method (the b1 field read call shape).
class Grid {
  private cells: number[] = new Array<number>(64).fill(0);
  at(sq: number): number {
    return this.cells[sq] ?? 0;
  }
  put(sq: number, v: number): void {
    this.cells[sq] = v;
  }
  sum(): number {
    let s = 0;
    for (let i = 0; i < 64; i++) s = (s + (this.at(i) | 0)) | 0;
    return s;
  }
}
const g = new Grid();
for (let i = 0; i < 64; i++) g.put(i, (i * 8192) & 0x7fffe000);
churn(500);
console.log("field", g.sum(), g.at(63) | 0, g.at(64) | 0);

// Negative controls: each stays a double Array.
const viaJson = new Array<number>(4).fill(0);
for (let i = 0; i < 4; i++) viaJson[i] = (i * 3) | 0;
console.log("json", JSON.stringify(viaJson), viaJson[2] | 0);
const viaIndexOf = new Array<number>(4).fill(0);
viaIndexOf[2] = 9;
console.log("indexOf", viaIndexOf.indexOf(9), viaIndexOf[2] | 0);
const viaForOf = new Array<number>(4).fill(2);
let fo = 0;
for (const x of viaForOf) fo = (fo + x) | 0;
console.log("forof", fo, viaForOf[1] | 0);
const viaSpread = new Array<number>(3).fill(5);
console.log("spread", [...viaSpread].length, viaSpread[0] | 0);
const viaFill = new Array<number>(3).fill(5);
viaFill.fill(6);
console.log("refill", viaFill[1] | 0);
const holes = new Array<number>(4);
holes[1] = 3;
console.log("holes", holes[0], holes[1] | 0, 0 in holes);
const n = process.argv.length > 100 ? 1 : -1;
const negZero = new Array<number>(2).fill(0);
negZero[0] = (ts & 0) * n;
console.log("negzero", Object.is(negZero[0], -0), 1 / negZero[0], negZero[0] | 0);
const wide = new Array<number>(2).fill(0);
wide[1] = 2147483647 + (ts & 1) + 1;
console.log("wide", wide[1], wide[1] | 0);
const lit = [1, 2, 3];
const shared = new Array<number>(3).fill(4);
shared[2] = ts & 7;
const either = ts > 5 ? lit : shared;
console.log("mixed", either[1] | 0, shared[2] | 0);

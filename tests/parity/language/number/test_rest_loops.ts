// I32 at rest, B2: loops and held bases over at-rest Arrays. The element
// loop tiers and the held bases of PR-D / N2 read an at-rest Array's I32
// words the way they read a double Array's slots. Covered: `for`, `while`
// and `do-while` loops over a local, a parameter and a class field (read
// through a method call, in and outside the call's own floor), stores in a
// loop, a loop mixing an at-rest Array with a double one, keys past the
// floor (the cold arm), negative keys, values >= 4096 with the low bits clear
// across collections. Negative controls: an Array a builtin reads, and one
// holding fractions, read by the same loops.

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

// A local: a `for` over its floor, a store loop, a `while` and a `do-while`.
const a = new Array<number>(64).fill(0);
for (let i = 0; i < 64; i++) a[i] = (i * 4096) & 0x3ffff000;
churn(300);
let s1 = 0;
for (let round = 0; round < 50; round++) {
  for (let i = 0; i < 64; i++) s1 = (s1 + a[i]) | 0;
  for (let i = 0; i < 64; i++) a[i] = (a[i] + i) | 0;
}
let j = 0;
let s2 = 0;
while (j < 200) {
  s2 = (s2 ^ (a[j & 63] | 0)) | 0;
  j++;
}
let k = 0;
let s3 = 0;
do {
  s3 = (s3 + a[(k * 7) & 63]) | 0;
  k++;
} while (k < 100);
console.log("local", s1, s2, s3, a[0], a[63]);

// A parameter, read in a loop whose key may pass the floor.
function scan(xs: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s = (s + (xs[i] ?? -1)) | 0;
  return s;
}
const p = new Array<number>(16).fill(3);
for (let i = 0; i < 16; i++) p[i] = (i * 2654435761) | 0;
console.log("param", scan(p, 16), scan(p, 20));

// A mixed loop: an at-rest Array and a double Array side by side.
const halves = new Array<number>(64).fill(0.5);
for (let i = 0; i < 64; i++) halves[i] = i / 4;
let mixed = 0;
for (let i = 0; i < 64; i++) mixed += a[i] * halves[i];
console.log("mixed", mixed);

// A class field read through a method call: loops with a held base, keys in
// the call's floor and outside it (rays leave the board), and a method with
// two reads (a function-level base).
function onBoard(sq: number): boolean {
  return sq >= 0 && sq < 128 && (sq & 0x88) === 0;
}
class Board {
  private cells: number[] = new Array<number>(128).fill(0);
  at(sq: number): number {
    return this.cells[sq] ?? 0;
  }
  put(sq: number, v: number): void {
    this.cells[sq & 127] = v;
  }
  total(): number {
    let s = 0;
    for (let sq = 0; sq < 128; sq++) {
      if ((sq & 0x88) !== 0) {
        sq += 7;
        continue;
      }
      const v = this.at(sq);
      if (v === 0) continue;
      s = (s + v) | 0;
    }
    return s;
  }
  ray(from: number, delta: number): number {
    let sq = from + delta;
    let n = 0;
    while (onBoard(sq)) {
      const v = this.at(sq);
      if (v !== 0) return v * 100 + n;
      sq += delta;
      n++;
    }
    return -n;
  }
  pair(sq: number): number {
    const x = this.at(sq);
    const y = this.at(sq + 16);
    return x - y;
  }
  far(sq: number): number {
    let s = 0;
    let i = 0;
    do {
      s = (s + this.at(sq + i)) | 0;
      i += 50;
    } while (i < 300);
    return s;
  }
}
const b = new Board();
for (let sq = 0; sq < 128; sq++) if ((sq & 0x88) === 0) b.put(sq, ((sq * 37) & 15) - 7);
b.put(0x33, 0);
churn(400);
let rays = 0;
for (let sq = 0; sq < 128; sq++) {
  if (!onBoard(sq)) continue;
  for (const d of [1, -1, 16, -16, 15, 17, -15, -17]) rays = (rays * 31 + b.ray(sq, d)) | 0;
}
console.log("field", b.total(), rays, b.pair(3), b.pair(120), b.far(10), b.at(-1), b.at(200));

// Negative controls: the same loops over Arrays that stay double.
const viaJson = new Array<number>(64).fill(1);
for (let i = 0; i < 64; i++) viaJson[i] = i * 3;
let nj = 0;
for (let i = 0; i < 64; i++) nj = (nj + viaJson[i]) | 0;
console.log("json", nj, JSON.stringify(viaJson).length);
const frac = new Array<number>(64).fill(0.25);
let nf = 0;
for (let i = 0; i < 64; i++) nf += frac[i];
let nf2 = 0;
for (let i = 0; i < 70; i++) nf2 += frac[i] ?? 1;
console.log("frac", nf, nf2);

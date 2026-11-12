// An integer interval's bounds are those of doubles (wpfacts `model::range`,
// `PERRY_FACTS_INT_BOUNDS`).
//
// A product past 2^60 came out of the interval rules as the empty interval
// (the exact lower bound beside the "unbounded" upper one), so the binding it
// was assigned to kept the interval of its other values: `let h = 1406932606;
// h *= h; h *= h;` stayed "the int32 1406932606" and `~h` printed 89915391.
// Each section prints what the int consumers see (`~x`, `x | 0`, `x >>> 0`,
// `x & 0xffff`, an element read, `x % 7`) beside the value itself.

const a: number[] = [10, 11, 12, 13];
const u = new Uint8Array([20, 21, 22, 23]);

// ---- 1. the reported shape: a module `let` squared twice ----
let h = 1406932606;
h *= h;
h *= h; // 3.9e36, past 2^63
console.log("huge:", ~h, ~h & 0xffff, ~~h, (~h + 1) | 0, h | 0, h >>> 0, a[h], u[h], h % 7, h);

// ---- 2. one product past 2^60, in every binding shape ----
let m1 = 1406932606;
m1 *= m1; // 1.98e18
console.log("module:", ~m1, m1 | 0, m1 >>> 0, m1 & 0xffff, a[m1], u[m1], m1 % 7, m1);
function inFunction(): void {
  let x = 3037000500;
  x = x * x; // 9.2e18, just past 2^63
  console.log("function:", ~x, x | 0, x >>> 0, x & 0xffff, a[x], u[x], x % 7, x);
}
inFunction();
let l1 = 2147483647;
for (let k = 0; k < 3; k++) {
  l1 *= l1;
  console.log("loop:", k, ~l1, l1 | 0, l1 >>> 0, l1 & 0xffff, a[l1], u[l1], l1 % 7, l1);
}
let c1 = -2147483648;
const square = () => {
  c1 *= c1;
};
square();
console.log("closure:", ~c1, c1 | 0, c1 >>> 0, c1 & 0xffff, a[c1], u[c1], c1 % 7, c1);
function cube(x: number): number {
  x = x * x * x;
  return x;
}
const p1 = cube(4294967296); // 2^96
const p2 = cube(1406932606);
console.log("param:", ~p1, p1 | 0, p1 >>> 0, a[p1], p1 % 7, p1, ~p2, p2 | 0, p2 >>> 0, p2 & 0xffff, p2 % 7, p2);
class Cell {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  step(): void {
    this.v = this.v * this.v + 1;
  }
}
const cell = new Cell(1406932606);
cell.step();
console.log("field:", ~cell.v, cell.v | 0, cell.v >>> 0, cell.v & 0xffff, a[cell.v], cell.v % 7, cell.v);
cell.step();
console.log("field:", ~cell.v, cell.v | 0, cell.v >>> 0, cell.v & 0xffff, a[cell.v], cell.v % 7, cell.v);

// ---- 3. int32 x int32 and uint32 x uint32 ----
function products(x: number): void {
  const i = x | 0;
  const w = x >>> 0;
  const ii = i * i; // below 2^62
  const iii = i * i * i;
  const ww = w * w; // below 2^64
  const lcg = i * 1103515245 + 12345;
  console.log("products:", x, ~ii, ii | 0, ii % 7, ii, ~iii, iii | 0, iii, ~ww, ww >>> 0, ww % 7, ww, ~lcg, lcg | 0, lcg);
}
for (const x of [0, 1, -1, 46340, 46341, 65536, 2147483647, -2147483648, 4294967295, 1406932606]) {
  products(x);
}

// ---- 4. a left shift's exact interval stays inside int32 ----
let s1 = 2147483647;
s1 <<= 31; // -2147483648
let s2 = 1;
s2 <<= 31;
let s3 = 3;
s3 = s3 << 30; // -1073741824
console.log("shift:", s1, a[s1], u[s1], ~s1, s2, a[s2], s3, a[s3], s3 >>> 0);

// ---- 5. sums and steps at 2^53 ----
let t1 = 9007199254740991;
t1 += 1;
t1 += 1; // 2^53 + 1 is 2^53
let t2 = 9007199254740992;
t2 = t2 + t2;
let t3 = 9007199254740991;
t3++;
t3++;
let t4 = -9007199254740991;
t4 -= 2;
console.log("sums:", t1, ~t1, t1 | 0, t1 % 7, t2, ~t2, t2 % 7, t3, ~t3, t4, ~t4, t4 | 0, t4 % 7);

// ---- 6. a loop that keeps multiplying ----
let w = 7;
const seen: number[] = [];
for (let k = 0; k < 8; k++) {
  w = w * w;
  seen.push(~w, w & 0xffff, w % 7);
}
console.log("growth:", w, seen.join(","));
let f = 1;
let fs = 0;
for (let k = 1; k < 26; k++) {
  f *= k;
  fs = (fs + (f | 0)) | 0;
}
console.log("factorial:", f, fs, ~f, f % 7);

// ---- 7. controls: these products stay int32 (the claims must remain) ----
let q1 = 46340;
q1 *= q1; // 2147395600
let q2 = 65535;
q2 = q2 * 32767; // 2147385345
let q3 = -46340;
q3 = q3 * 46341; // -2147441940
console.log("int32:", q1, ~q1, a[q1], q1 % 7, q2, ~q2, q2 & 0xffff, q3, ~q3, q3 | 0, q3 >>> 0);
function hash(n: number): number {
  let x = 7;
  for (let i = 0; i < n; i++) {
    x = (x * 31 + i) | 0;
  }
  return x;
}
function board(): number {
  const cells = new Uint8Array(128);
  let sum = 0;
  for (let r = 0; r < 8; r++) {
    for (let c = 0; c < 8; c++) {
      cells[r * 16 + c] = r * 8 + c;
      sum += cells[r * 16 + c] * (r - c);
    }
  }
  return sum;
}
function mix(seed: number): number {
  let s = seed | 0;
  let out = 0;
  for (let i = 0; i < 64; i++) {
    s = (Math.imul(s, 1103515245) + 12345) | 0;
    out ^= (s >>> 16) & 0x7fff;
    out = (out << 1) | (out >>> 31);
  }
  return out;
}
console.log("controls:", hash(1000), board(), mix(1406932606), mix(-1));

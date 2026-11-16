// x ** 2 and Math.pow(x, 2) equal x * x (V8) over 2M doubles from a fixed xorshift and specials; arg = seed
const seedArg = process.argv[2];
let s = (seedArg === undefined ? 7 : Number(seedArg)) | 0;
if (s === 0) s = 7;
function next(): number {
  s ^= s << 13; s ^= s >>> 17; s ^= s << 5;
  return s >>> 0;
}
const pw: number[] = [];
let p = 1;
for (let i = 0; i <= 1100; i++) { pw.push(p); p = p * 2; }
const nw: number[] = [];
p = 1;
for (let i = 0; i <= 1100; i++) { nw.push(p); p = p / 2; }
function scale(m: number, e: number): number {
  return e >= 0 ? m * pw[e] : m * nw[-e];
}
const specials: number[] = [0, -0, Infinity, -Infinity, NaN, 1, -1, 2, 0.1, -0.1, 5e-324, -5e-324,
  2.2250738585072014e-308, 1.4916681462400413e-154, 1.4916681462400412e-154, 1.3407807929942596e154,
  1.3407807929942597e154, 1.7976931348623157e308, -1.7976931348623157e308, 3, 1e-160, 1e154, 1e155];
let mism = 0, mismPow = 0, mismOp = 0, hash = 0;
let first = "";
function check(x: number): void {
  const a = x ** 2;
  const b = x * x;
  const c = Math.pow(x, 2);
  if (!Object.is(a, b)) { mism++; if (first.length < 400) first += " x=" + x + " pow=" + a + " mul=" + b; }
  if (!Object.is(c, b)) mismPow++;
  if (!Object.is(a, c)) mismOp++;
  // hash over the squares: an exact digest of the result bits via scaled fraction
  if (b === b && b !== Infinity && b !== 0) {
    let m = b < 0 ? -b : b; let e = 0;
    while (m >= 2) { m = m / 2; e++; if (e > 2100) break; }
    while (m < 1) { m = m * 2; e--; if (e < -2100) break; }
    const t = Math.floor((m - 1) * 4294967296) >>> 0;
    hash = (Math.imul(hash, 31) + t + e) | 0;
  }
}
for (const x of specials) check(x);
const N = 2000000;
for (let i = 0; i < N; i++) {
  const hi = next(), lo = next(), r = next();
  const m = 1 + hi / 4294967296 + lo / 18446744073709551616;
  const e = (r % 2120) - 1090;
  const x = (r & 0x80000000) !== 0 ? -scale(m, e) : scale(m, e);
  check(x);
}
console.log("n=" + (N + specials.length) + " pow2!==mul " + mism + " Math.pow!==mul " + mismPow + " **!==Math.pow " + mismOp + " hash " + hash);
if (first.length > 0) console.log("first:" + first);

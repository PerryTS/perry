// P3 B1 controls (DESIGN-p3 rev 3.1 §5): compares and arithmetic over element
// reads the facts prove plain Numbers (C1, C2) and Number parameters (C4).
// Every result must match node exactly, with the fact on, off and in check
// mode. Loops run inside functions with parameter bounds.

// Holey arrays and reads past the end: Number-or-undefined (REVIEW-p3 D1).
const holey = [1, , 3];
holey.length = 10;
function same(i: number, j: number): boolean { return holey[i] === holey[j]; }
function differ(i: number, j: number): boolean { return holey[i] !== holey[j]; }
console.log(same(5, 6), same(1, 5), same(1, 20), same(0, 0), same(0, 2), differ(5, 6), differ(0, 2));
function rel(i: number): string {
  const v = holey[i];
  return [v < 1, v <= 1, v > 1, v >= 1, !(v < 1)].join(",");
}
console.log(rel(0), rel(1), rel(2), rel(15));

// NaN, -0, +0 (both operands proven Numbers).
const z = [0, -0, NaN, 0 / 0, Infinity, -Infinity];
function eq(i: number, j: number): boolean { return z[i] === z[j]; }
console.log(eq(0, 1), eq(2, 2), eq(2, 3), eq(3, 3), eq(4, 4), eq(4, 5), eq(0, 9), eq(9, 9));
function zrel(n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) for (let j = 0; j < n; j++) s += (z[i] < z[j] ? "<" : z[i] > z[j] ? ">" : "=");
  return s;
}
console.log(zrel(z.length));

// `?? d` over a Number-or-undefined read is a Number: arithmetic (C2) and compare.
const counts = [3, , 5];
function sum(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s = s + (counts[i] ?? 0) * 2 - (counts[i] ?? 1);
  return s;
}
console.log(sum(5), sum(0));

// Builtin NUM producers stored and read back: Math.round, .length, push, indexOf.
function produced(k: number): number[] {
  const out: number[] = [];
  out.push(Math.round(k + 0.4));
  out.push(out.length);
  out.push([5, 6, 7].indexOf(k));
  out.push(out.push(9));
  return out;
}
function useProduced(k: number): string {
  const p = produced(k);
  let s = "";
  for (let i = 0; i < p.length; i++) s += (p[i] === k ? "k" : p[i] > k ? "+" : "-") + (p[i] * 3 | 0) + " ";
  return s;
}
console.log(useProduced(6), useProduced(2));

// int32 overflow to double, through arithmetic on proven reads.
const big = [2147483647, -2147483648, 4294967295];
function wrap(i: number): string {
  const v = big[i];
  return [v + 1, v * 2, v | 0, v >>> 0, v - 1 === 2147483646].join(",");
}
console.log(wrap(0), wrap(1), wrap(2));

// A Number parameter (C4) fed by proven reads and literals.
function half(x: number): number { return x / 2; }
function halves(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += half(z[i % 2]) + half(counts[0] ?? 0);
  return s;
}
console.log(halves(4), Object.is(half(-0), -0));

// Values the facts do not cover keep their tests: JSON.parse through any, BigInt.
function parsed(text: string): string {
  const xs: any[] = JSON.parse(text);
  let s = "";
  for (let i = 0; i < xs.length; i++) s += (xs[i] === 1) + ":" + (xs[i] + 1) + " ";
  return s;
}
console.log(parsed("[1, \"1\", null, 2.5]"));
const bigs = [1n, 2n];
function bigsum(n: number): bigint {
  let s = 0n;
  for (let i = 0; i < n; i++) s = s + bigs[i] * 2n;
  return s;
}
console.log(String(bigsum(2)));

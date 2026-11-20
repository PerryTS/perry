// b2 P1: loop reads of an array at an index that is not the loop counter
// (a module global and a local), consumed by strict comparisons (no
// coercion). Holes, out-of-range, fractional, negative and -0 indices take
// the slow arm and read exactly what Get reads. The `+=` loop coerces its
// read (a user `valueOf` could run there), so it keeps the guarded route.
let g: number[] = [];
for (let i = 0; i < 128; i++) g.push((i * 37) % 101);
function scan(a: number[], n: number): number {
  let hits = 0;
  for (let i = 0; i < n; i++) {
    if (a[(i * 7) & 15] === 3) hits++;
    const r = i & 7;
    for (let f = 0; f < 8; f++) {
      if (a[r * 16 + f] === 37) hits += 2;
      if (a[r * 16 + f + 100] === undefined) hits += 3;
    }
  }
  return hits;
}
function sum(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += a[(i * 7) & 15];
  return s;
}
function edges(a: number[]): string {
  const out: string[] = [];
  const keys = [0, -0, 1, 127, 128, 200, -1, 0.5, 2.5, NaN, Infinity, 2147483648];
  for (let j = 0; j < keys.length; j++) out.push(String(a[keys[j]]));
  return out.join(",");
}
const local: number[] = new Array<number>(64).fill(3);
local[10] = 11;
delete (local as any)[20];
console.log(scan(g, 100), scan(local, 50), sum(g, 30), edges(g), edges(local));
let w = 0;
while (w < 3) { console.log(g[w * 40], local[w * 10]); w++; }

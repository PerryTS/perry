// #10718: array read-modify-write in a multi-statement loop body
// (`a[i] = a[i] + 1; s += a[i]`, `a[i] += x; s += a[i]`). The dense range
// tier runs these with one entry guard and raw f64 loads/stores; every case
// below must match the generic path exactly, including the ones whose guard
// fails (holes, out-of-bounds windows, non-number elements, frozen arrays)
// and arrays whose element kind changes between loop entries.

function rmw(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] = a[i] + 1; s += a[i]; }
  return s;
}
function rmwCompound(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] += 0.5; s += a[i]; }
  return s;
}
function rmwOffset(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i + 1] = a[i] * 2 - a[i + 1]; s += a[i + 1]; }
  return s;
}
function rmwTwoArrays(a: number[], b: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] = a[i] + b[i]; b[i] = a[i] - b[i]; s += a[i] * b[i]; }
  return s;
}
function rmwNeg(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] = -a[i]; s += 1 / a[i]; }
  return s;
}
function rmwDiv(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] = a[i] / 0; s += a[i]; }
  return s;
}
function rmwAny(a: any, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) { a[i] = a[i] + 1; s += a[i]; }
  return s;
}
function rmwBranch(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    a[i] = a[i] * 3;
    if (a[i] > 10) s += a[i]; else s -= 1;
  }
  return s;
}

const show = (label: string, v: unknown, a?: unknown[]) =>
  console.log(label, String(v), a === undefined ? "" : a.map((x) => (Object.is(x, -0) ? "-0" : String(x))).join(","));

// Plain packed doubles: the fast copy.
let a: number[] = [];
for (let i = 0; i < 8; i++) a.push(i * 0.25);
show("rmw", rmw(a, 8), a);
show("rmwCompound", rmwCompound(a, 8), a);
show("rmwOffset", rmwOffset(a, 7), a);
let b: number[] = [];
for (let i = 0; i < 8; i++) b.push(8 - i);
show("rmwTwoArrays", rmwTwoArrays(a, b, 8), b);
// Aliasing: the same array through two bindings.
show("rmwAlias", rmwTwoArrays(b, b, 8), b);

// -0, NaN, Infinity through the raw stores.
const z = [0, -0, 1, -1, NaN, Infinity, -Infinity, 2.5];
show("rmwNeg", rmwNeg(z, 8), z);
const d = [1, -1, 0, -0, NaN, 3, -3, 0.5];
show("rmwDiv", rmwDiv(d, 8), d);

// Holes: the hole-free guard declines, the generic path runs.
const holes: number[] = new Array(6);
holes[0] = 1; holes[2] = 3; holes[5] = 6;
show("holes", rmw(holes, 6), holes);
const sparse = [1, 2, 3, 4];
delete sparse[1];
show("deleted", rmw(sparse, 4), sparse);

// Out of bounds: the window exceeds the length, so the guard declines and
// the generic path grows the array and reads undefined.
const short = [1, 2, 3];
show("oob", rmw(short, 5), short);
show("oobOffset", rmwOffset([1, 2], 3), []);

// Non-number elements: strings that look numeric concatenate, BigInt throws.
const mixed: any[] = [1, "2", 3, "x"];
show("strings", rmwAny(mixed, 4), mixed);
try {
  rmwAny([1, 2n, 3], 3);
  console.log("bigint no throw");
} catch (e) {
  console.log("bigint", (e as Error).constructor.name);
}
const objs: any[] = [1, { valueOf() { return 10; } }, 3];
show("valueOf", rmwAny(objs, 3), objs.map((x) => typeof x));

// Element kind changes between loop entries: doubles, then a string is
// stored, then doubles again.
const k: any[] = [0.5, 1.5, 2.5, 3.5];
const k1 = rmwAny(k, 4);
k[2] = "s";
const k2 = rmwAny(k, 4);
k[2] = 7;
const k3 = rmwAny(k, 4);
show("kindChange", [k1, k2, k3].join("|"), k);

// Integer-valued literal arrays and frozen arrays.
const ints = [1, 2, 3, 4, 5];
show("ints", rmw(ints, 5), ints);
const frozen = Object.freeze([1, 2, 3]) as number[];
try {
  show("frozen", rmw(frozen, 3), frozen as number[]);
} catch (e) {
  show("frozen", (e as Error).constructor.name, frozen as number[]);
}

// A branch after the store.
const br = [1, 2, 3, 4, 5, 6];
show("branch", rmwBranch(br, 6), br);

// Zero-trip and large loops.
show("zero", rmw([1, 2, 3], 0), []);
const big: number[] = [];
for (let i = 0; i < 1000; i++) big.push(i % 7);
let total = 0;
for (let r = 0; r < 50; r++) total += rmw(big, 1000);
show("big", total, big.slice(0, 5));

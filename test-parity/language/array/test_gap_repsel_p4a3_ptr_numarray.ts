// Test: repsel Phase 4a.3 — Ptr<NumArray> guard-free element access.
// This module contains NO shape/prototype barriers, so eligible locals are
// promoted; every behavior here must be byte-exact vs
// `node --experimental-strip-types` whether or not promotion happened
// (PERRY_PTR_NUMARRAY_LOCALS=0 must produce identical output).

// --- the driving shape: new Array(n) histogram, returned ---
function histogram(data: number[], size: number): number[] {
  const counts: number[] = new Array(size);
  const mask = size - 1;
  for (let i = 0; i < data.length; i++) {
    const v = data[i] & mask;
    counts[v] = (counts[v] || 0) + 1;
  }
  return counts;
}
const data: number[] = [];
let seed = 99991;
for (let i = 0; i < 5000; i++) {
  seed = (seed * 48271) % 2147483647;
  data.push(seed);
}
const h = histogram(data, 32);
console.log(h.join(","));
let total = 0;
for (let i = 0; i < h.length; i++) total += h[i] || 0;
console.log(total);

// --- literal-length alloc, static in-bounds reads/writes, holes kept ---
function pointProbe(): number {
  const c: number[] = new Array(8);
  c[0] = 1.5;
  c[3] = -0;
  c[5] = 0 / 0; // NaN
  // statically in-bounds number-context reads over values, -0, NaN, holes
  let acc = (c[0] || 9) + (c[1] || 9) + (c[5] || 9); // 1.5 + 9 + 9
  acc += c[2] * 2; // hole -> undefined -> NaN
  if (Number.isNaN(acc)) acc = -1;
  acc += Object.is(c[3] || 7, 7) ? 100 : 0; // -0 falsy
  acc += c[3] ?? 55; // -0 not nullish -> -0
  acc += c[6] ?? 55; // hole -> 55
  return acc;
}
console.log(pointProbe());

// --- hole observability on a promoted local: bare reads stay exact ---
function bareReads(): void {
  const c: number[] = new Array(4);
  c[1] = (c[1] || 0) + 2;
  console.log(c[0], c[1], c[3]); // undefined 2 undefined
}
bareReads();

// --- Dense provenance: [] + numeric pushes + bounded-loop consumers ---
function pushAndSum(n: number): number {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    out.push(i * 0.5);
  }
  let s = 0;
  for (let i = 0; i < out.length; i++) {
    s += out[i] || 0;
  }
  for (let i = 0; i < out.length; i++) {
    out[i] = (out[i] || 0) * 2;
  }
  let s2 = 0;
  for (let i = 0; i < out.length; i++) {
    s2 += out[i] || 0;
  }
  return s + s2;
}
console.log(pushAndSum(1000));

// --- disqualification cases: each must simply stay byte-exact ---

// alias escape (bare reference)
function aliasCase(): string {
  const a: number[] = new Array(3);
  a[0] = 4;
  const b = a; // bare LocalGet -> not promoted
  b[1] = 5;
  return JSON.stringify(a);
}
console.log(aliasCase());

// call-argument escape + JSON/keys/in observability
function escapeCase(): string {
  const a: number[] = new Array(3);
  a[1] = 8;
  const keys = Object.keys(a).join("|");
  const has0 = 0 in a;
  return JSON.stringify(a) + " " + keys + " " + has0;
}
console.log(escapeCase());

// non-numeric store poisons the local (never promoted)
function mixedStore(): string {
  const a: number[] = new Array(3);
  a[0] = 1;
  (a as any)[1] = "x";
  return JSON.stringify(a) + " " + ((a[1] as any) || "fallback");
}
console.log(mixedStore());

// length shrink + reordering mutators
function shrinkCase(): string {
  const a: number[] = new Array(4);
  a[0] = 3;
  a[1] = 1;
  a[2] = 2;
  a.length = 2;
  return JSON.stringify(a) + " " + (a[2] ?? -1);
}
console.log(shrinkCase());
function popCase(): number {
  const a: number[] = [];
  a.push(1.5);
  a.push(2.5);
  const p = a.pop() || 0;
  return p + (a[1] ?? 100);
}
console.log(popCase());
function sortCase(): string {
  const a: number[] = new Array(3);
  a[0] = 3;
  a[1] = 1;
  a[2] = 2;
  a.sort();
  return a.join(",");
}
console.log(sortCase());

// sparse-extend beyond the allocation length (not in-bounds-proven)
function sparseCase(): string {
  const a: number[] = new Array(2);
  a[0] = 1;
  a[6] = 7;
  return JSON.stringify(a) + " " + a.length + " " + (3 in a);
}
console.log(sparseCase());

// fractional / out-of-range keys (property writes, not elements)
function fractionalCase(): string {
  const a: number[] = new Array(2);
  a[0] = 1;
  (a as any)[0.5] = 9;
  (a as any)[-1] = 8;
  return JSON.stringify(a) + " " + (a as any)[0.5] + " " + (a as any)[-1] + " " + a.length;
}
console.log(fractionalCase());

// specialized-callee growth (call-arg escape -> guarded tiers + self-heal)
function growInto(a: number[], n: number): void {
  for (let i = 0; i < n; i++) {
    a.push(i * 0.25);
  }
}
function calleeGrowth(): number {
  const owned: number[] = [1.5];
  growInto(owned, 100);
  let s = 0;
  for (let i = 0; i < owned.length; i++) s += owned[i] || 0;
  owned[3] = (owned[3] || 0) + 1;
  return s + owned.length + (owned[3] || 0);
}
console.log(calleeGrowth());

// zero-length edge: new Array(0) and [] with no writes
function emptyCase(): number {
  const a: number[] = new Array(0);
  const b: number[] = [];
  return a.length + b.length + (a[0] ?? 5) + (b[0] ?? 6);
}
console.log(emptyCase());

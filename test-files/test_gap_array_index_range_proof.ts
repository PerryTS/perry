// #7286: array-index range proofs (monotone strided induction + affine
// indices + interprocedural parameter ranges). Every case below either
// exercises a newly-proven fast path with edge-case values, or is a shape the
// proof must REFUSE — a wrong index proof is a memory-safety bug, so the
// refusals matter as much as the admissions.

// --- A. the 11_prime_sieve shape: `j = i * i; j = j + i` over a dense array
const a: number[] = [];
for (let i = 0; i < 12; i++) {
  a[i] = 0;
}
for (let i = 2; i * i < 12; i++) {
  for (let j = i * i; j < 12; j = j + i) {
    a[j] = a[j] + 1;
  }
}
console.log("A:" + a.join(","));

// --- B. same shape over a HOLEY array: reads must still see undefined holes
const b: number[] = [];
b[11] = 1;
let bOut = "";
for (let j = 2; j < 12; j = j + 3) {
  bOut = bOut + j + "=" + b[j] + ";";
}
console.log("B:" + bOut + "len=" + b.length);

// --- C. `<=` guard, stride > 1, last iteration exactly at the bound
const c = [0, 1, 2, 3, 4, 5, 6];
let cOut = "";
for (let j = 0; j <= 6; j = j + 2) {
  cOut = cOut + c[j] + ";";
}
console.log("C:" + cOut);

// --- D. negative / fractional / NaN indices are PROPERTIES, not elements
const d = [1, 2, 3];
console.log("D1:" + d[-1] + "," + d[1.5] + "," + d[NaN] + "," + d[-0]);
d[-1] = 91;
d[1.5] = 92;
d[NaN] = 93;
console.log("D2:" + d.length + "," + d[-1] + "," + d[1.5] + "," + d[NaN]);
console.log("D3:" + Object.keys(d).join("|"));

// --- E. indices at and beyond the array-index limit
const e: number[] = [];
e[0] = 1;
e[4294967294] = 2; // 2^32-2 — the LAST real array index
console.log("E1:" + e.length + "," + e[4294967294]);
e[4294967295] = 3; // 2^32-1 — a property, NOT an index
console.log("E2:" + e.length + "," + e[4294967295]);
e[4294967296] = 4; // 2^32 — a property
console.log("E3:" + e.length + "," + e[4294967296]);
const eBig: number[] = [];
eBig[3000000000] = 5; // > i32::MAX but a valid index
console.log("E4:" + eBig.length + "," + eBig[3000000000]);

// --- F. affine `i * size + k` where the product overflows i32
function affine(arr: number[], size: number): string {
  let out = "";
  for (let i = 0; i < 2; i++) {
    for (let k = 0; k < 2; k++) {
      out = out + arr[i * size + k] + ";";
    }
  }
  return out;
}
const f = [1, 2, 3, 4, 5, 6];
console.log("F1:" + affine(f, 2));
console.log("F2:" + affine(f, 2000000000)); // i*size+k reaches 2e9 and 2e9+1
console.log("F3:" + affine(f, 4000000000)); // 4e9 > 2^32-2 — properties

// --- G. interprocedural parameter range: two constant call sites meet
function pick(arr: number[], n: number): number {
  return arr[n];
}
const g = [10, 20, 30];
console.log("G:" + pick(g, 0) + "," + pick(g, 2));

// --- H. a parameter the callee reassigns must NOT inherit the caller's range
function reassigned(arr: number[], n: number): string {
  n = -1;
  return "" + arr[n];
}
console.log("H:" + reassigned(g, 0));

// --- I. a body write to the counter breaks the induction: the loop re-enters
// with a NEGATIVE counter, which must read the "-1" property (undefined).
const i1 = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
let iOut = "";
for (let j = 0; j < 10; j = j + 2) {
  iOut = iOut + i1[j] + ";";
  if (j === 4) {
    j = -3;
  }
}
console.log("I:" + iOut);

// --- J. a stride the body rewrites (to a negative value) must not be trusted
const j1 = [0, 1, 2, 3, 4, 5, 6, 7, 8];
let jStride = 3;
let jOut = "";
for (let j = 0; j < 9; j = j + jStride) {
  jOut = jOut + j + ":" + j1[j] + ";";
  if (j >= 6) {
    jStride = 100;
  }
}
console.log("J:" + jOut);

// --- K. the callee escapes as a value, so its parameter has unseen call sites
function escaping(arr: number[], n: number): number {
  return arr[n];
}
const kRef = escaping;
console.log("K:" + kRef(g, 1) + "," + escaping(g, 0) + "," + kRef(g, -1));

// --- L. holes vs. explicit undefined through the strided path
const l = new Array(6);
l[3] = 7;
let lOut = "";
for (let j = 0; j < 6; j = j + 1) {
  lOut = lOut + (j in l) + "/" + l[j] + ";";
}
console.log("L:" + lOut);

// --- M. a strided counter whose start is huge: the guard rejects immediately
const m = [1, 2, 3];
let mCount = 0;
for (let j = 1e300 * 1e300; j < 3; j = j + 1) {
  mCount = mCount + 1;
}
console.log("M:" + mCount + "," + m[0]);

// --- N. non-integral stride keeps fractional indices (property reads)
const n1 = [0, 1, 2, 3];
let nOut = "";
for (let j = 0; j < 3; j = j + 0.5) {
  nOut = nOut + j + "=" + n1[j] + ";";
}
console.log("N:" + nOut);

// --- O. a sparse array grown through a proven strided store
const o: number[] = [];
for (let j = 5; j < 20; j = j + 5) {
  o[j] = j;
}
console.log("O:" + o.length + "," + JSON.stringify(o));

// --- P. string-keyed reads on the same array still resolve as properties
const p: number[] = [1, 2, 3];
(p as unknown as Record<string, number>)["x"] = 9;
let pOut = "";
for (let j = 0; j < 3; j = j + 1) {
  pOut = pOut + p[j] + ";";
}
console.log("P:" + pOut + (p as unknown as Record<string, number>)["x"] + "," + p.length);

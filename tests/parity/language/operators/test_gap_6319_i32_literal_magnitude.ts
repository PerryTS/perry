// #6319 (third face of #6072): an integer literal past 2^31 still seeded the
// i32 fast path, so *copying* such a local wrapped it.
//
// `let x = 3000000000` kept the correct double in its own slot (the `Let`-site
// gate checks the literal's magnitude), but it was still admitted to
// `integer_locals`. `let y = 0; y = x;` then counted as a "strictly i32-bounded"
// write, `y` got an i32 shadow, and the store truncated it — `y` read back as
// -1294967296. Same for a direct out-of-range store, a copy chain, and a
// counter seeded past 2^31 under an `arr.length` / `i < n` loop.

// ---- the issue's repro ----
let x = 3000000000;
let y = 0;
y = x;
console.log("copy:", y);

let z = 0;
z = x + 4;
console.log("add:", z);

// ---- direct out-of-range store into an otherwise-i32 local ----
let d = 0;
d = 3000000000;
console.log("direct store:", d);

// ---- boundary neighbourhood, each copied through a fresh local ----
function copyOf(v: number): number {
  let out = 0;
  out = v;
  return out;
}
const a = 2147483647; // 2^31 - 1  — fits, stays on the i32 path
const b = 2147483648; // 2^31
const c = 2147483649; // 2^31 + 1
const e = 4294967296; // 2^32
const f = -2147483648; // -2^31    — fits
const g = -2147483649; // -2^31 - 1
const h = 9007199254740991; // Number.MAX_SAFE_INTEGER
console.log("2^31-1:", copyOf(a));
console.log("2^31:", copyOf(b));
console.log("2^31+1:", copyOf(c));
console.log("2^32:", copyOf(e));
console.log("-2^31:", copyOf(f));
console.log("-2^31-1:", copyOf(g));
console.log("MAX_SAFE:", copyOf(h));

// ---- copy chain ----
const c1 = 5000000000;
const c2 = c1;
const c3 = c2;
console.log("chain:", c3);

let m1 = 0;
let m2 = 0;
let m3 = 0;
m1 = 6000000000;
m2 = m1;
m3 = m2;
console.log("mut chain:", m3);

// ---- a counter seeded past 2^31 under an `arr.length` bound ----
// The length-hoist path used to install the counter's i32 shadow from
// `integer_locals` membership alone, seeding it with a poison `fptosi`.
const arr = [11, 22, 33];
let big = 3000000000;
const hits: number[] = [];
for (let i = 0; i < arr.length; i++) {
  hits.push(big + arr[i]);
}
console.log("length-hoist:", hits.join(","), "big:", big);

// ---- the i32 chain that must stay exact: FNV-1a over bytes ----
function imul32(p: number, q: number): number {
  return Math.imul(p, q);
}
let fnv = 0x811c9dc5 | 0;
const bytes = [1, 2, 3, 4, 5, 250, 251, 252];
for (let i = 0; i < bytes.length; i++) {
  fnv = (fnv ^ bytes[i]) | 0;
  fnv = imul32(fnv, 0x01000193);
}
const hash = fnv >>> 0;
console.log("fnv:", hash.toString(16).padStart(8, "0"));

// A `>>> 0` seed above INT32_MAX stays unsigned and must not be truncated.
const SEED = 0x9e3779b9 >>> 0;
console.log("seed:", SEED, (SEED | 0) === -1640531527);

// ---- the array-index fast path must survive ----
const nums = [10, 20, 30, 40, 50];
let acc = 0;
for (let i = 0; i < nums.length; i++) {
  acc = acc + nums[i];
}
console.log("arrsum:", acc, "nums[2]:", nums[2]);

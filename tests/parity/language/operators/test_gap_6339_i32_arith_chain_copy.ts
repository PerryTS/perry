// #6339 (fourth face of #6072): copying a local whose value came from an
// *arithmetic chain* past 2^31 wrapped.
//
// `is_strictly_i32_bounded_expr`'s `LocalGet` arm answered from
// `integer_locals`, which means integer-VALUED and by design admits overflowing
// `Add`/`Sub`/`Mul`. So `let big2 = big1 + big1` (4e9) stayed correct in its own
// slot — an `Add` write is not strictly-i32-bounded — yet the oracle still
// vouched for it, and `t = big2` counted as a strictly-i32-bounded write. `t`
// got an i32 shadow at its `Let` site and the store truncated: -294967296.
//
// The oracle is now the greatest fixpoint of "every write is i32-bounded", so a
// local with an overflowing arithmetic write can no longer vouch for its copies.
// Intentionally-i32 code (`| 0`, `>>> 0`, bitwise, `Math.imul`) proves itself
// without ever consulting the oracle and keeps its i32 slot.

// ---- the issue's repro ----
let big1 = 2000000000;
let big2 = big1 + big1; // 4e9 — exceeds i32
let t = 0;
t = big2;
console.log("copy of add:", t);

// ---- accumulator past 2^31, then copied ----
let sum = 0;
for (let i = 0; i < 5; i++) sum = sum + 1000000000;
let sumCopy = 0;
sumCopy = sum;
console.log("copy of accumulator:", sumCopy);

// ---- multiply / subtract chains ----
let m = 100000 * 100000; // 1e10
let mCopy = 0;
mCopy = m;
console.log("copy of mul:", mCopy);

let neg = -2000000000 - 2000000000; // -4e9
let negCopy = 0;
negCopy = neg;
console.log("copy of sub:", negCopy);

// ---- a copy chain hop-by-hop: disqualification must propagate transitively ----
let c1 = 3000000000 - 1; // 2999999999
let c2 = 0;
let c3 = 0;
let c4 = 0;
c2 = c1;
c3 = c2;
c4 = c3;
console.log("copy chain:", c4);

// ---- `x++` past 2^31, then copied (the #6258 accumulator, one hop out) ----
let inc = 2147483646;
inc++;
inc++;
inc++;
let incCopy = 0;
incCopy = inc;
console.log("copy of ++:", incCopy, inc);

// ---- boundary matrix, each value copied through a fresh local ----
function copyOf(v: number): number {
  let out = 0;
  out = v;
  return out;
}
function addOf(a: number, b: number): number {
  let s = a + b;
  let out = 0;
  out = s;
  return out;
}
console.log("2^31-1:", addOf(2147483646, 1));
console.log("2^31:", addOf(2147483647, 1));
console.log("2^31+1:", addOf(2147483647, 2));
console.log("2^32:", addOf(2147483648, 2147483648));
console.log("-2^31:", addOf(-2147483647, -1));
console.log("-2^31-1:", addOf(-2147483647, -2));
console.log("MAX_SAFE:", copyOf(9007199254740991));

// ---- intentional i32 must still be i32 AND still wrap ----
// FNV-1a: `| 0` and Math.imul prove themselves; they never ask the oracle.
function imul32(a: number, b: number): number {
  return Math.imul(a, b);
}
let h = 0x811c9dc5 | 0;
const bytes = [1, 2, 3, 4, 5, 250, 251, 252];
for (let i = 0; i < bytes.length; i++) {
  h = (h ^ bytes[i]) | 0;
  h = imul32(h, 0x01000193);
}
console.log("fnv:", (h >>> 0).toString(16).padStart(8, "0"));

// A hand-rolled imul32 (the shape image_convolution uses) still wraps mod 2^32.
function imul32Manual(a: number, b: number): number {
  const aHi = (a >>> 16) & 0xffff;
  const aLo = a & 0xffff;
  const bHi = (b >>> 16) & 0xffff;
  const bLo = b & 0xffff;
  return (aLo * bLo + (((aHi * bLo + aLo * bHi) << 16) >>> 0)) | 0;
}
console.log("imul32Manual:", imul32Manual(0x9e3779b9 | 0, 0x01000193));

// Bit-mixing chain: every write is a bitwise op, so every copy stays i32.
let mix = 0x9e3779b9 | 0;
let mixCopy = 0;
mix = (mix ^ (mix << 13)) | 0;
mix = (mix ^ (mix >>> 17)) | 0;
mix = (mix ^ (mix << 5)) | 0;
mixCopy = mix;
console.log("mix:", mixCopy, mixCopy | 0, (mixCopy >>> 0) === mixCopy >>> 0);

// `| 0` of an overflowing sum: the programmer asked for the wrap, so we wrap.
let wrapped = 0;
wrapped = (big1 + big1) | 0;
console.log("explicit wrap:", wrapped);

// A `>>> 0` seed above INT32_MAX stays unsigned, and an all-`>>> 0` xorshift
// recurrence keeps its (unsigned) i32 slot and wraps mod 2^32 exactly.
const SEED = 0x9e3779b9 >>> 0;
console.log("seed:", SEED, (SEED | 0) === -1640531527);
let s = SEED >>> 0;
s = (s ^ ((s << 13) >>> 0)) >>> 0;
s = (s ^ (s >>> 17)) >>> 0;
s = (s ^ ((s << 5) >>> 0)) >>> 0;
console.log("xorshift:", s);

// ---- the array-index fast path (#6299/#6312) must survive ----
const nums = [10, 20, 30, 40, 50];
let acc = 0;
for (let i = 0; i < nums.length; i++) {
  acc = acc + nums[i];
}
console.log("arrsum:", acc, "nums[2]:", nums[2]);

// ---- clamp-style index arithmetic (image_convolution's blur shape) ----
function clampIdx(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}
const W = 8;
const grid = [
  1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
  23, 24, 25, 26, 27, 28, 29, 30, 31, 32,
];
let gsum = 0;
for (let y = 0; y < 4; y++) {
  for (let x = 0; x < W; x++) {
    for (let kx = -1; kx <= 1; kx++) {
      const xx = clampIdx(x + kx, 0, W - 1);
      const row = y * W;
      gsum += grid[row + xx];
    }
  }
}
console.log("blur-shape sum:", gsum);

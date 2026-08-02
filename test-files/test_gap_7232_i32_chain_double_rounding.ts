// #7232 — an i32-native arithmetic chain must not evaluate past double precision.
//
// ECMAScript numbers are IEEE-754 doubles, so `*` and `+` round their result to
// the nearest double BEFORE the next operator runs. Perry's i32 fast path
// (`expr/i32_fast_path.rs`) evaluates the same chain in exact two's-complement
// `mul/add i32`, which agrees with JS only while every intermediate integer is
// exactly representable — |v| <= 2^53. `(x * 1103515245 + 12345) & 0x7fffffff`,
// the classic LCG step, has a ~2^61 product: the exact chain kept low bits that
// the double had already rounded away and the mask read them straight back, so
// Perry printed 654583775 where Node prints 654583808.
//
// The three shapes below are the issue's own repro. They fail independently:
// a fix that only rounds at a function boundary passes the third and none of
// the others.

// ---- 1. straight-line, both intermediates in SSA locals ----
let s0 = 12345;
let s1 = (s0 * 1103515245 + 12345) & 0x7fffffff;
let s2 = (s1 * 1103515245 + 12345) & 0x7fffffff;
console.log("straight:", s1, s2);

// ---- 2. loop-carried ----
let t = 12345;
for (let i = 0; i < 4; i++) {
  t = (t * 1103515245 + 12345) & 0x7fffffff;
}
console.log("loop:", t);

// ---- 3. through a function boundary (already correct pre-fix) ----
function step(x: number): number {
  return (x * 1103515245 + 12345) & 0x7fffffff;
}
console.log("call:", step(step(step(12345))));

// ---- the same divergence under every ToInt32-shaped consumer ----
let u = 1406932606;
console.log("or0:", (u * 1103515245 + 12345) | 0);
console.log("ushr:", (u * 1103515245 + 12345) >>> 0);
console.log("xor:", (u * 1103515245 + 12345) ^ 0);
console.log("shr:", (u * 1103515245 + 12345) >> 3);
console.log("shl:", (u * 1103515245 + 12345) << 1);
console.log("and-neg:", (u * -1103515245 - 12345) & 0x7fffffff);

// ---- compound assignment and `const` forms ----
let c = 1406932606;
c *= 1103515245;
c += 12345;
c &= 0x7fffffff;
console.log("compound:", c);

const k0 = 1406932606;
const k1 = (k0 * 1103515245 + 12345) & 0x7fffffff;
console.log("const:", k1);

// ---- the un-masked value itself: the rounded double is observable ----
let raw = 1406932606;
const prod = raw * 1103515245;
console.log("product:", prod, prod + 12345);

// ---- a full LCG run, the shape that surfaced this ----
let seed = 42;
const drawn: number[] = [];
for (let i = 0; i < 6; i++) {
  seed = (seed * 1103515245 + 12345) & 0x7fffffff;
  drawn.push(seed);
}
console.log("lcg:", drawn.join(","));

// ---- exactness boundary: 2^53 is where the two models part ----
// 94906265 * 94906265 = 9007199326062225 > 2^53 (rounds); 94906264 * 94906265
// = 9007199231155960 < 2^53 (exact). Both must print Node's answer.
const lo = 94906264;
const hi = 94906265;
console.log("under-2^53:", (lo * hi + 1) & 0x7fffffff);
console.log("over-2^53:", (hi * hi + 1) & 0x7fffffff);
console.log("at-2^53:", (67108864 * 134217728 + 1) & 0x7fffffff);

// ---- the other direction: chains that ARE f64-exact must stay exact ----
// Java-style string hash: |h| < 2^31 and 31 < 2^5, so the product is < 2^36 —
// well inside 2^53. This must keep matching Node bit for bit.
function javaHash(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = (h * 31 + s.charCodeAt(i)) | 0;
  }
  return h;
}
console.log("javaHash:", javaHash("the quick brown fox"), javaHash(""));

// Math.imul is defined as an exact low-32 multiply, so it is NOT subject to the
// double-rounding rule and must stay on the exact path even past 2^53.
function fnv1a(bytes: number[]): number {
  let h = 0x811c9dc5 | 0;
  for (let i = 0; i < bytes.length; i++) {
    h = h ^ bytes[i];
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
console.log("fnv1a:", fnv1a([1, 2, 3, 4, 250, 251, 252, 253]));
console.log("imul-big:", Math.imul(1406932606, 1103515245));

// 16x16 masked operands multiply to 32 bits — exact, must stay correct.
const m0 = 0xdeadbeef | 0;
const m1 = 0xfeedface | 0;
console.log("masked16:", ((m0 & 0xffff) * (m1 & 0xffff)) | 0);
console.log("shifted:", ((m0 >>> 16) * (m1 >>> 16)) | 0);

// Computed array indices: i * cols + j stays small and must stay exact.
const cols = 7;
const grid: number[] = [];
for (let i = 0; i < 5 * cols; i++) {
  grid.push(i * 3);
}
let gridSum = 0;
for (let i = 0; i < 5; i++) {
  for (let j = 0; j < cols; j++) {
    gridSum = gridSum + grid[i * cols + j];
  }
}
console.log("grid:", gridSum, grid[2 * cols + 3]);

// Squaring a loop counter (the sieve shape) — small, exact, must not change.
let sieveTrips = 0;
for (let i = 2; i * i < 400; i++) {
  sieveTrips = sieveTrips + i * i;
}
console.log("sieve:", sieveTrips);

// ---- negatives and zero through the same chain ----
const probes = [0, 1, -1, 2147483647, -2147483648, 65536, -65536];
const out: string[] = [];
for (let i = 0; i < probes.length; i++) {
  const p = probes[i];
  out.push(String((p * 1103515245 + 12345) & 0x7fffffff));
}
console.log("probes:", out.join(","));

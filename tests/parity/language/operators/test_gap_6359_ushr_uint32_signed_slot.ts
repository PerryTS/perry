// #6359 (fifth face of the #6072 i32 fast-path family): a `>>> 0` uint32
// copied into a local whose *other* write is a signed integer literal took a
// SIGNED i32 slot and read back negative.
//
// `>>> 0` is ToUint32 (result 0..2^32-1), NOT ToInt32. But
// `is_strictly_i32_bounded_expr` used to accept `expr >>> 0` in the same arm as
// `expr | 0`, and it accepted *any* `UShr` in the general bitwise arm. So a
// local mixing a plain `0` init with a `>>> 0` write (too mixed for the
// unsigned-i32 slot, wrongly admitted to the signed one) landed in
// `strictly_i32_bounded_locals`, got a signed i32 shadow at its `Let`, and
// every read `sitofp`'d the bit pattern: `0x9e3779b9` → -1640531527.
//
// Fix: `>>> 0` no longer proves a signed i32. `x >>> k` is admitted to a signed
// slot only when the effective shift `k & 31` is a nonzero literal (drops the
// top bit, capping the value at 2^31-1). `>>> 0`, `>>> 32`, and `>>> <variable>`
// (which can be a shift-by-0 at runtime) all fall back to the f64 slot.

// ---- the issue's exact repro ----
const SEED = 0x9e3779b9 >>> 0;

let seedCopy = 0;
seedCopy = SEED >>> 0;
console.log("seedCopy:", seedCopy); // 2654435769

let d = 0;
d = 4000000000 >>> 0;
console.log("d:", d); // 4000000000

// ---- a whole boundary matrix of `>>> 0` copied over a signed-literal seed ----
function ushrCopy(v: number): number {
  let out = 0; // signed literal seed — the poison
  out = v >>> 0; // uint32 write
  return out;
}
console.log("2^31-1:", ushrCopy(2147483647)); // 2147483647
console.log("2^31:", ushrCopy(2147483648)); // 2147483648
console.log("2^31+1:", ushrCopy(2147483649)); // 2147483649
console.log("2^32-1:", ushrCopy(4294967295)); // 4294967295
console.log("neg wraps unsigned:", ushrCopy(-1)); // 4294967295
console.log("small stays small:", ushrCopy(42)); // 42

// ---- the "also suspect" case: `x >>> k` with a VARIABLE k ----
// k can be 0 at runtime, making the result a uint32 with no literal `0` in the
// HIR to key on. The variable-shift write must not confer a signed i32 slot.
function ushrVar(x: number, k: number): number {
  let out = 0;
  out = x >>> k;
  return out;
}
console.log("ushrVar k=0:", ushrVar(0x9e3779b9, 0)); // 2654435769
console.log("ushrVar k=1:", ushrVar(0x9e3779b9, 1)); // 1327217884
console.log("ushrVar k=8:", ushrVar(0xffffffff, 8)); // 16777215

// ---- copy chain: disqualification must propagate transitively ----
let a1 = 3000000000 >>> 0; // 3000000000
let a2 = 0;
let a3 = 0;
a2 = a1;
a3 = a2;
console.log("copy chain:", a3); // 3000000000

// ==== what MUST still work (fast path preserved) ====

// A `>>> k` with a literal k>=1 genuinely fits signed i32; copying it stays
// correct whether or not it keeps the fast i32 slot.
function ushrLit(v: number): number {
  let out = 0;
  out = v >>> 1; // <= 2^31-1, signed-safe
  return out;
}
console.log("ushr>>1:", ushrLit(0xffffffff)); // 2147483647
console.log("ushr>>16:", 0xffffffff >>> 16); // 65535

// `| 0` (ToInt32) still proves a signed i32 slot and still wraps mod 2^32.
let h = 0x811c9dc5 | 0;
const bytes = [1, 2, 3, 4, 5, 250, 251, 252];
for (let i = 0; i < bytes.length; i++) {
  h = (h ^ bytes[i]) | 0;
  h = Math.imul(h, 0x01000193);
}
console.log("fnv:", (h >>> 0).toString(16).padStart(8, "0"));

// An all-`>>> 0` xorshift keeps its (unsigned) i32 slot and wraps mod 2^32.
let s = SEED >>> 0;
s = (s ^ ((s << 13) >>> 0)) >>> 0;
s = (s ^ (s >>> 17)) >>> 0;
s = (s ^ ((s << 5) >>> 0)) >>> 0;
console.log("xorshift:", s);

// A const `>>> 0` seed above INT32_MAX prints unsigned (no i32 slot at all).
console.log("SEED:", SEED, (SEED | 0) === -1640531527);

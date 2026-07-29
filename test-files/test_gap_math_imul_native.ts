// Math.imul lowering to a native `mul i32` when both operands are provably
// in-range i32 (perry-codegen expr/math_simple.rs generic arm +
// expr/i32_fast_path.rs i32-native / accumulator path). Multiplication mod 2^32
// has identical low 32 bits for signed and unsigned operands, so the native
// path is exact — but only for provable i32 operands. Non-finite / fractional /
// >2^32 operands MUST keep JS ToUint32/ToInt32 semantics via the runtime
// helper. Every result must match `node --experimental-strip-types` exactly.

// --- Edge cases the native path must NOT take (kept on the runtime helper) ---
console.log(Math.imul(NaN, 5));          // 0   (NaN -> ToInt32 -> 0)
console.log(Math.imul(Infinity, 5));     // 0
console.log(Math.imul(-Infinity, 3));    // 0
console.log(Math.imul(1.9, 2));          // 2   (1.9 -> ToInt32 -> 1)
console.log(Math.imul(2 ** 32 + 3, 1));  // 3   (ToUint32(2^32+3) = 3)

// --- Boundary / >i32::MAX constants the native path handles exactly ---
console.log(Math.imul(0x7fffffff, 2));   // -2  (wraps at i32 boundary)
console.log(Math.imul(0xffffffff, 5));   // -5  (0xffffffff -> -1 as i32)
console.log(Math.imul(-5, -3));          // 15
console.log(Math.imul(0x9e3779b1, 3));   // multiplier > i32::MAX
console.log(Math.imul(0x9e3779b1 | 0, 0x85ebca6b | 0));

// --- Nested native imul (result of imul is itself a provable i32) ---
console.log(Math.imul(Math.imul(3, 7), 5)); // 105

// --- Variable i32 operands ---
let p = 123456789 | 0;
let q = -987654321 | 0;
console.log(Math.imul(p, q));

// --- The i32-accumulator chain: `a = Math.imul(a, K)` on a local with an i32
//     slot, whose constant K exceeds i32::MAX — the exact shape that failed to
//     lower before the Integer-gate fix (0x9e3779b1 = 2654435761 > i32::MAX). ---
function mix(x: number): number {
  let a = x | 0;
  a = Math.imul(a, 0x9e3779b1);
  a = (a ^ (a >>> 15)) | 0;
  a = Math.imul(a, 0x85ebca6b);
  a = (a ^ (a >>> 13)) | 0;
  a = Math.imul(a, 0xc2b2ae35);
  a = (a ^ (a >>> 16)) | 0;
  return a | 0;
}
let acc = 0 | 0;
for (let i = 0; i < 5000; i++) acc = (acc ^ mix(acc ^ i)) | 0;
console.log(acc);

// --- imul feeding `| 0` and arithmetic, in a tight loop (hash-like) ---
function fnv1aish(seed: number): number {
  let h = seed | 0;
  for (let i = 0; i < 32; i++) {
    h = (h ^ i) | 0;
    h = Math.imul(h, 0x01000193); // 16777619, a prime > i16 but < i32
  }
  return h | 0;
}
console.log(fnv1aish(0x811c9dc5 | 0));
console.log(fnv1aish(1), fnv1aish(-1), fnv1aish(0));

// --- Scoping guard: the 32-bit-literal relaxation is confined to Math.imul.
//     A plain `*` computes its product in f64 (precision loss above 2^53), so
//     `x * BIGLIT | 0` must NOT be lowered to an exact `mul i32` — it must stay
//     `ToInt32(f64_product)`, matching Node. `+`/`-`/bitwise with a >i32::MAX
//     literal stay f64-exact too. ---
let g = 5 | 0;
g = (g + 3000000000) | 0;
console.log(g); // Add: sum < 2^53, exact
g = (g * 2654435761) | 0;
console.log(g); // Mul: product > 2^53 -> f64 rounding, NOT exact mul i32
g = (g ^ 0x9e3779b1) | 0;
console.log(g); // bitwise with >i32::MAX literal
console.log((123456789 * 2654435761) | 0); // large product | 0
console.log((0xffffffff * 0xffffffff) | 0); // 2^64-ish product | 0
console.log((2000000000 * 2000000000) | 0); // two large i32 values
console.log((1000003 * 1000033) | 0); // product < 2^53 (exact either way)


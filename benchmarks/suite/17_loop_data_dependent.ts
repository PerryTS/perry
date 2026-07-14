// Benchmark: data-dependent loop body with array reads.
//
// This is the honest companion to 02_loop_overhead. Where loop_overhead
// measures whether the compiler applied reassoc + IndVarSimplify to a
// trivially-foldable accumulator (which is a flag-aggressiveness
// probe, not a runtime perf comparison), this benchmark forces the
// compiler to actually execute work.
//
// Kernel: sum = sum * x[i & 63] + x[(i*7) & 63]
//   - Sequential dependency on `sum` (the multiplicative carry).
//     LLVM cannot reorder this under reassoc because reassoc applies
//     to identical operands; here each iteration's multiplicand is a
//     different runtime-loaded f64.
//   - Array reads from a runtime-allocated f64 array filled BEFORE the
//     timed section from a seeded LCG. The array contents are not
//     compile-time constants from the loop's perspective (the load is
//     a memory operation; LLVM doesn't propagate through 100M-iter
//     loops over volatile-ish addresses).
//   - The sequential carry through `sum` defeats the vectorizer:
//     iteration i+1 cannot start until iteration i's `sum` is known.
//
// N = 64 keeps the array cache-resident (64 × 8B = 512B, well within
// L1d). The values are in [0.5, 1.0) so the multiplicative carry
// STRICTLY CONTRACTS (every multiplicand x < 1) — sum converges to a
// bounded fixed point near mean(x_add)/(1-mean(x_mul)) ≈ 3.0 in
// steady state. Values centered on 1.0 (e.g. [0.99, 1.01]) overflow
// to Infinity because tiny random drifts in mean compound
// geometrically across 100M iterations.

const N = 64;
const ITERATIONS = 100000000;

// LCG-seeded array fill. Same recurrence as glibc's rand(); the fixed
// seed of 42 is fine because: (a) the array contents are runtime
// memory loads, not folded into the loop body's IR; (b) verified via
// asm dump that LLVM keeps the multiply chain as a scalar fmul/fadd
// loop with vmovsd loads — see the Rust source's verification comment.
let seed = 42;
const x: number[] = [];
for (let i = 0; i < N; i++) {
    // Math.imul makes the intended 32-bit LCG multiplication explicit.
    // Plain Number multiplication rounds above 2^53 before the bitwise mask,
    // producing a different sequence from the native-language implementations.
    seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
    x.push(0.5 + (seed / 2147483647) * 0.5);
}

const start = Date.now();
let sum = 1.0;
for (let i = 0; i < ITERATIONS; i++) {
    sum = sum * x[i & 63] + x[(i * 7) & 63];
}
const elapsed = Date.now() - start;

console.log("loop_data_dependent:" + elapsed);
console.log("sum:" + sum);

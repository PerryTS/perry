// Benchmark: integer-modulo accumulation loop (published as "modulo_loop").
// Despite the historical file name this computes no factorial: it sums
// `i % 1000` over a counted loop. It is a microbenchmark of loop overhead and
// the integer `%` fast path, not of general TypeScript performance.
const ITERATIONS = 100000000;
let sum = 0;

const start = Date.now();
for (let i = 0; i < ITERATIONS; i++) {
    sum = sum + (i % 1000);
}
const elapsed = Date.now() - start;

console.log("accumulate:" + elapsed);
console.log("sum:" + sum);

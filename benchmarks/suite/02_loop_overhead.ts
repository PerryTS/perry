// Benchmark: Loop overhead
// Measures a minimal loop-carried integer dependency without array access.
// A plain `sum = sum + 1` induction variable is equivalent to ITERATIONS and
// can be removed completely, leaving a misleading 0 ms benchmark. Every
// iteration therefore feeds an inexpensive FNV-style recurrence whose final
// value escapes below.
const ITERATIONS = 100000000;
let checksum = 0x811c9dc5 | 0;

const start = Date.now();
for (let i = 0; i < ITERATIONS; i++) {
    checksum = Math.imul(checksum ^ i, 0x01000193);
}
const elapsed = Date.now() - start;

console.log("loop_overhead:" + elapsed);
console.log("checksum:" + (checksum >>> 0));

// Micro-benchmark: small Buffer.alloc performance (issue #92)
// Measures 100k Buffer.alloc(16) calls in a tight loop.
// Expected: Perry ~8ms before fix, ~2-3ms after, Bun ~2ms.

const N = 100_000;

const start = Date.now();
let acc = 0;
for (let i = 0; i < N; i++) {
    const buf = Buffer.alloc(16);
    buf[0] = i & 0xFF;
    acc += buf[0]; // prevent dead-code elimination
}
const elapsed = Date.now() - start;

console.log(`Buffer.alloc(16) x${N}: ${elapsed}ms (acc=${acc})`);

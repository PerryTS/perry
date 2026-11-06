import { randomFillSync } from 'node:crypto';
const buf = new Uint8Array(8);
randomFillSync(buf);
// Avoid `Uint8Array.prototype.some` — Perry's typed-array dispatch
// currently returns `undefined` from `.some()` on Uint8Array, which
// makes this test diverge from Node even when randomFillSync itself
// works. Iterate manually and accumulate the result.
let anyNonZero = false;
for (let i = 0; i < buf.length; i++) {
    if (buf[i] !== 0) { anyNonZero = true; break; }
}
console.log(buf.length, anyNonZero);  // 8 true

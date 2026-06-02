// crypto.getRandomValues(buf) is the WebCrypto sync randomness API
// (fills `buf` in-place with random bytes, returns it). Perry's call
// form already worked via the buffer's $$cryptoFillRandom synthetic
// method, but the property-read form (`typeof crypto.getRandomValues
// === "function"`, `const f = crypto.getRandomValues`) returned
// "number" because it wasn't on the callable-export list.
// Regression cover for #1366.
import * as crypto from "node:crypto";
console.log("typeof:", typeof crypto.getRandomValues);
const buf = new Uint8Array(8);
const ret = crypto.getRandomValues(buf);
console.log("length:", buf.length);
console.log("returns same buffer:", ret === buf);
// Walk by index to avoid the Uint8Array.some() gap.
let anyNonZero = false;
for (let i = 0; i < buf.length; i++) {
  if (buf[i] !== 0) {
    anyNonZero = true;
    break;
  }
}
console.log("filled (some byte non-zero):", anyNonZero);

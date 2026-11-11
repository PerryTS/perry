// Regression test for crypto.randomFillSync — required by axios.
// Fills a typed array in-place with cryptographically strong random
// bytes and returns the same buffer.

import * as crypto from "crypto";

const u8 = new Uint8Array(16);
const ret8 = crypto.randomFillSync(u8);
console.log("u8.length =", u8.length);
console.log("ret is same array =", ret8 === u8);
// Probability of all-zeros for 16 cryptographically random bytes is
// 2^-128 — effectively never. Sum > 0 is the cheap "non-zero" check.
let sum8 = 0;
for (let i = 0; i < u8.length; i++) sum8 += u8[i];
console.log("u8 nonzero =", sum8 > 0);

// Uint32Array path (the shape axios actually uses).
const u32 = new Uint32Array(8);
crypto.randomFillSync(u32);
console.log("u32.length =", u32.length);
let sum32 = 0;
for (let i = 0; i < u32.length; i++) sum32 += u32[i];
console.log("u32 nonzero =", sum32 > 0);

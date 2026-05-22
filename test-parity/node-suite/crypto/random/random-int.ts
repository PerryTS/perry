import crypto from "node:crypto";
// randomInt(min, max) returns an integer in [min, max).
const n = crypto.randomInt(0, 10);
console.log("in range:", n >= 0 && n < 10);
console.log("integer:", Number.isInteger(n));

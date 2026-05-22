import crypto from "node:crypto";
// randomBytes(n) returns an n-byte Buffer (value is non-deterministic).
const b = crypto.randomBytes(16);
console.log("isBuffer:", Buffer.isBuffer(b));
console.log("length:", b.length);
console.log("zero length:", crypto.randomBytes(0).length);

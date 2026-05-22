import crypto from "node:crypto";
// digest() with no encoding returns a 32-byte Buffer for SHA-256.
const buf = crypto.createHash("sha256").update("hello").digest();
console.log("isBuffer:", Buffer.isBuffer(buf));
console.log("length:", buf.length);

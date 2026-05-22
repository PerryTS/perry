import crypto from "node:crypto";
// Multiple update() calls accumulate; the digest equals hashing the
// concatenation.
const a = crypto.createHash("sha256").update("hel").update("lo").digest("hex");
const b = crypto.createHash("sha256").update("hello").digest("hex");
console.log("equal:", a === b);
console.log(a);

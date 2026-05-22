import crypto from "node:crypto";
// Hashing a Buffer hashes its bytes — same digest as hashing the equivalent
// string.
const fromBuf = crypto.createHash("sha256").update(Buffer.from("hello")).digest("hex");
const fromStr = crypto.createHash("sha256").update("hello").digest("hex");
console.log("equal:", fromBuf === fromStr);
console.log(fromBuf);

import crypto from "node:crypto";
// hkdfSync derives a key deterministically; it returns an ArrayBuffer.
const out = crypto.hkdfSync("sha256", "key", "salt", "info", 16);
console.log(Buffer.from(out).toString("hex"));

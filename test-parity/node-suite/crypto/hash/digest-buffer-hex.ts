import crypto from "node:crypto";
// The no-encoding digest Buffer round-trips through Buffer#toString("hex") to
// the same hex string as digest("hex").
const buf = crypto.createHash("sha256").update("hello").digest();
console.log("hex:", buf.toString("hex"));

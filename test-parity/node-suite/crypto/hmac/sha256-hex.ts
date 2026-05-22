import crypto from "node:crypto";
// HMAC-SHA256 hex digest of a known (key, message) pair is fixed.
console.log(crypto.createHmac("sha256", "key").update("hello").digest("hex"));

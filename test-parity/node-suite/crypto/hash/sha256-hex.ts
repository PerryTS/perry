import crypto from "node:crypto";
// SHA-256 hex digest of a known string is a fixed value.
console.log(crypto.createHash("sha256").update("hello").digest("hex"));

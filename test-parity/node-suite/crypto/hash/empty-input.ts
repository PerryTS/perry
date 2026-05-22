import crypto from "node:crypto";
// SHA-256 of the empty string (the canonical e3b0c442... constant).
console.log(crypto.createHash("sha256").update("").digest("hex"));

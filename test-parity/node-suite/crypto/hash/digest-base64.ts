import crypto from "node:crypto";
// digest("base64") / digest("base64url") encode the digest bytes, not hex.
console.log("base64:", crypto.createHash("sha256").update("hello").digest("base64"));
console.log("base64url:", crypto.createHash("sha256").update("hello").digest("base64url"));

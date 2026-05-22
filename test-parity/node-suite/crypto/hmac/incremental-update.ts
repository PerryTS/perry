import crypto from "node:crypto";
// Chained HMAC update() accumulates; equals hashing the concatenation.
const a = crypto.createHmac("sha256", "key").update("he").update("llo").digest("hex");
const b = crypto.createHmac("sha256", "key").update("hello").digest("hex");
console.log("equal:", a === b);
console.log(a);

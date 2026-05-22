import * as crypto from "crypto";

console.log("sha256:", crypto.createHash("sha256").update("abc").digest("hex"));
console.log("hmac:", crypto.createHmac("sha256", "key").update("abc").digest("hex"));

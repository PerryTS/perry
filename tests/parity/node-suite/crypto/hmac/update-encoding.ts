import * as crypto from "node:crypto";

console.log("hmac hex input:", crypto.createHmac("sha256", "key").update("616263", "hex").digest("hex"));
console.log("hmac base64 input:", crypto.createHmac("sha256", "key").update("YWJj", "base64").digest("hex"));
console.log("hmac latin1 input:", crypto.createHmac("sha1", "key").update("abc", "binary").digest("hex"));

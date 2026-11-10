import * as crypto from "node:crypto";

console.log("md5 fox:", crypto.createHmac("md5", "key").update("The quick brown fox jumps over the lazy dog").digest("hex"));
console.log("sha1 empty-data:", crypto.createHmac("sha1", "key").update("").digest("hex"));
console.log("sha256 empty-key:", crypto.createHmac("sha256", "").update("The quick brown fox jumps over the lazy dog").digest("hex"));
console.log("sha256 empty-both:", crypto.createHmac("sha256", "").update("").digest("hex"));

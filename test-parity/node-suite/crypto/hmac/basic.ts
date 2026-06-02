import * as crypto from "node:crypto";

console.log("sha1:", crypto.createHmac("sha1", "key").update("The quick brown fox jumps over the lazy dog").digest("hex"));
console.log("sha256:", crypto.createHmac("sha256", "key").update("The quick brown fox jumps over the lazy dog").digest("hex"));
console.log("sha512:", crypto.createHmac("sha512", "key").update("hello world").digest("hex"));
console.log("md5:", crypto.createHmac("md5", "key").update("The quick brown fox jumps over the lazy dog").digest("hex"));

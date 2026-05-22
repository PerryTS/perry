import * as crypto from "node:crypto";

console.log("sha1:", crypto.createHash("sha1").update("abc").digest("hex"));
console.log("sha256:", crypto.createHash("sha256").update("abc").digest("hex"));
console.log("sha512:", crypto.createHash("sha512").update("abc").digest("hex"));
console.log("md5:", crypto.createHash("md5").update("abc").digest("hex"));

import * as crypto from "node:crypto";

console.log("sha224:", crypto.createHash("sha224").update("abc").digest("hex"));
console.log("sha384:", crypto.createHash("sha384").update("abc").digest("hex"));
console.log("sha512-256:", crypto.createHash("sha512-256").update("abc").digest("hex"));
console.log("sha-256 alias:", crypto.createHash("sha-256").update("abc").digest("hex"));

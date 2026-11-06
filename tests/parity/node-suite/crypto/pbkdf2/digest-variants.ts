import * as crypto from "node:crypto";

console.log("sha1:", crypto.pbkdf2Sync("password", "salt", 1, 20, "sha1").toString("hex"));
console.log("sha256:", crypto.pbkdf2Sync("password", "salt", 1, 20, "sha256").toString("hex"));
console.log("sha512:", crypto.pbkdf2Sync("password", "salt", 1, 20, "sha512").toString("hex"));

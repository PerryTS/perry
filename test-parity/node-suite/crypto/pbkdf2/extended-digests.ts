import * as crypto from "node:crypto";

console.log("sha224:", crypto.pbkdf2Sync("password", "salt", 1, 28, "sha224").toString("hex"));
console.log("sha384:", crypto.pbkdf2Sync("password", "salt", 1, 48, "sha384").toString("hex"));
console.log("sha512-256:", crypto.pbkdf2Sync("password", "salt", 1, 32, "sha512-256").toString("hex"));

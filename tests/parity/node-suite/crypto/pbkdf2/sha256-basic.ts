import * as crypto from "node:crypto";

console.log("pbkdf2 sha256 1:", crypto.pbkdf2Sync("password", "salt", 1, 32, "sha256").toString("hex"));
console.log("pbkdf2 sha256 2:", crypto.pbkdf2Sync("password", "salt", 2, 32, "sha256").toString("hex"));
console.log("pbkdf2 len:", crypto.pbkdf2Sync("password", "salt", 2, 17, "sha256").length);

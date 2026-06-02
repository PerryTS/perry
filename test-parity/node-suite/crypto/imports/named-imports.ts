import { createHash, createHmac, pbkdf2Sync, randomBytes, createSecretKey } from "node:crypto";

console.log("named hash:", createHash("sha256").update("abc").digest("hex"));
console.log("named hmac:", createHmac("sha256", "key").update("abc").digest("hex"));
console.log("named pbkdf2 len:", pbkdf2Sync("password", "salt", 1, 16, "sha256").length);
console.log("named random len:", randomBytes(4).length);
console.log("named secret key typeof:", typeof createSecretKey("secret"));

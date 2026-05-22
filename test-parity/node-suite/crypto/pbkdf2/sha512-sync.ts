import crypto from "node:crypto";
// pbkdf2Sync honors the digest argument — sha512 produces a different (and
// specific) result than sha256.
console.log(crypto.pbkdf2Sync("pw", "salt", 50, 16, "sha512").toString("hex"));

import crypto from "node:crypto";
// pbkdf2Sync with SHA-256 is deterministic for fixed inputs.
console.log(crypto.pbkdf2Sync("pw", "salt", 100, 16, "sha256").toString("hex"));

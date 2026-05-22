import * as crypto from "node:crypto";

console.log("createHash typeof:", typeof crypto.createHash);
console.log("createHmac typeof:", typeof crypto.createHmac);
console.log("pbkdf2Sync typeof:", typeof crypto.pbkdf2Sync);
console.log("randomBytes typeof:", typeof crypto.randomBytes);
console.log("randomUUID typeof:", typeof crypto.randomUUID);
console.log("createCipheriv typeof:", typeof crypto.createCipheriv);
console.log("createDecipheriv typeof:", typeof crypto.createDecipheriv);
console.log("constants typeof:", typeof crypto.constants);

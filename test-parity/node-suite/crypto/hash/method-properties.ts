import * as crypto from "node:crypto";

const hash = crypto.createHash("sha256");
console.log("hash update name:", hash.update.name);
console.log("hash digest name:", hash.digest.name);
console.log("hash copy name:", hash.copy.name);
console.log("hash update typeof:", typeof hash.update);
console.log("hash digest typeof:", typeof hash.digest);
console.log("hash copy typeof:", typeof hash.copy);

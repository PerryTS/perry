import * as crypto from "node:crypto";

console.log("createHash name:", crypto.createHash.name);
console.log("createHmac name:", crypto.createHmac.name);
console.log("createSign name:", crypto.createSign.name);
console.log("createVerify name:", crypto.createVerify.name);
console.log("createCipheriv name:", crypto.createCipheriv.name);
console.log("createDecipheriv name:", crypto.createDecipheriv.name);
console.log("createDiffieHellman name:", crypto.createDiffieHellman.name);
console.log("createECDH name:", crypto.createECDH.name);
console.log("hash name:", crypto.hash.name);
console.log("pbkdf2 name:", crypto.pbkdf2.name);
console.log("Sign name:", crypto.Sign.name);
console.log("Verify name:", crypto.Verify.name);

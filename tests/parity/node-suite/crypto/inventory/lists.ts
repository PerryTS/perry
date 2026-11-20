import * as crypto from "node:crypto";

const hashes = crypto.getHashes();
const ciphers = crypto.getCiphers();
const curves = crypto.getCurves();
console.log("hashes array:", Array.isArray(hashes));
console.log("hashes has sha256:", hashes.includes("sha256"));
console.log("hashes has sha512:", hashes.includes("sha512"));
console.log("ciphers array:", Array.isArray(ciphers));
console.log("ciphers has aes-256-cbc:", ciphers.includes("aes-256-cbc"));
console.log("ciphers has aes-256-gcm:", ciphers.includes("aes-256-gcm"));
console.log("curves array:", Array.isArray(curves));
console.log("curves has prime256v1:", curves.includes("prime256v1"));
console.log("getFips:", crypto.getFips());

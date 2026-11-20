import { getHashes, getCiphers, getCurves, getFips } from "node:crypto";

console.log("named hashes:", getHashes().includes("sha256"));
console.log("named ciphers:", getCiphers().includes("aes-128-gcm"));
console.log("named curves:", getCurves().includes("secp256k1"));
console.log("named fips:", getFips());

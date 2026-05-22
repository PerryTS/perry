import crypto from "node:crypto";
// Diffie-Hellman / ECDH factory functions.
console.log("createDiffieHellman:", typeof crypto.createDiffieHellman === "function");
console.log("createECDH:", typeof crypto.createECDH === "function");

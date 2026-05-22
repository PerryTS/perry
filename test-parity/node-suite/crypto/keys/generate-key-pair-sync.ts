import crypto from "node:crypto";
// generateKeyPairSync is a function (used to produce {publicKey, privateKey}).
console.log("generateKeyPairSync:", typeof crypto.generateKeyPairSync === "function");
console.log("generateKeySync:", typeof crypto.generateKeySync === "function");

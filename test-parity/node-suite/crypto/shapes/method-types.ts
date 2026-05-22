import crypto from "node:crypto";
// The crypto methods are callable function values (the call forms work, but
// reading them as values must also report "function").
console.log("createHash:", typeof crypto.createHash === "function");
console.log("createHmac:", typeof crypto.createHmac === "function");
console.log("randomBytes:", typeof crypto.randomBytes === "function");
console.log("pbkdf2Sync:", typeof crypto.pbkdf2Sync === "function");

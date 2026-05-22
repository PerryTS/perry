import crypto from "node:crypto";
// createSecretKey(buffer) returns a KeyObject (an object).
const k = crypto.createSecretKey(Buffer.from("secret-key-bytes"));
console.log("typeof:", typeof k);

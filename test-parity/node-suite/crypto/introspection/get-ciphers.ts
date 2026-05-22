import crypto from "node:crypto";
// crypto.getCiphers() lists supported cipher algorithms.
const c = crypto.getCiphers();
console.log("is array:", Array.isArray(c));
console.log("includes aes-256-cbc:", c.includes("aes-256-cbc"));

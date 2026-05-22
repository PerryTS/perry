import crypto from "node:crypto";
// crypto.getHashes() lists supported hash algorithms.
const h = crypto.getHashes();
console.log("is array:", Array.isArray(h));
console.log("includes sha256:", h.includes("sha256"));

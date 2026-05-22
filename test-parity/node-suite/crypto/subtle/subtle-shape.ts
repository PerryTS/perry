import crypto from "node:crypto";
// crypto.subtle is the WebCrypto SubtleCrypto object with async primitives.
console.log("subtle object:", typeof crypto.subtle === "object" && crypto.subtle !== null);
console.log("digest:", typeof crypto.subtle.digest === "function");
console.log("importKey:", typeof crypto.subtle.importKey === "function");
